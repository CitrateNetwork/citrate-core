// =====================================================================
// citrate-core — agent file changes and their undo (HUP-S2.9)
//
// The Hermes sidecar's file tools (fs_write, fs_edit, fs_delete, fs_rename, and the grant-session
// file_write and sheet_write) run in the sidecar, inside folders the member granted for writing,
// and take an undo checkpoint around every change: every agent write a member can trigger.
// Their tool result names that checkpoint ({session, seq}). This module reads it, and turns the
// sidecar's undo outcomes into one honest sentence: a refusal (a file changed since, a pruned step,
// undo not enabled) is never shown as success.
//
// Data source (Rule 7): the sidecar session's `tool_result` events (the change) and the sidecar's
// `/checkpoints` routes through Rust (`hermes_checkpoints`, `hermes_undo_step`,
// `hermes_undo_session`) for the list and the undo.
// =====================================================================

export const FILE_TOOLS = ["fs_write", "fs_edit", "fs_delete", "fs_rename", "file_write", "sheet_write"] as const;
export type FileTool = (typeof FILE_TOOLS)[number];

export function isFileTool(name: string): name is FileTool {
  return (FILE_TOOLS as readonly string[]).includes(name);
}

/** One agent file change that can be undone. */
export interface FileChange {
  /** The sidecar session id (also the checkpoint session). */
  session: string;
  /** The checkpoint step. */
  seq: number;
  tool: string;
  /** Absolute paths changed (a rename lists from, then to). */
  paths: string[];
}

/** A checkpointed step as the sidecar lists it (paths relative to `root`). */
export interface CheckpointStep {
  seq: number;
  status: "prepared" | "committed" | "interrupted" | "undone" | string;
  paths: string[];
  root: string;
}

export interface CheckpointList {
  session: string;
  /** false: the sidecar cannot undo (not enabled, or too old); `note` says why. */
  enabled: boolean;
  steps: CheckpointStep[];
  note: string | null;
}

export interface UndoConflict {
  seq: number;
  path: string;
  found: string;
}

export interface UndoOutcome {
  ok: boolean;
  undone: number[];
  restored: string[];
  prunedThrough: number | null;
  /** On a refusal: conflict, busy, already_undone, not_found, pruned, disabled, invalid, unsupported. */
  kind: string | null;
  reason: string | null;
  conflicts: UndoConflict[];
}

/** A sidecar session id as Rust accepts it (`[A-Za-z0-9-]`, 1 to 64). */
export function validSessionId(s: unknown): s is string {
  return typeof s === "string" && /^[A-Za-z0-9-]{1,64}$/.test(s);
}

const isObj = (v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null && !Array.isArray(v);

/** The change a file tool's result names, or null when the text is anything else. */
export function parseFileChange(toolName: string, content: unknown): FileChange | null {
  if (!isFileTool(toolName) || typeof content !== "string") return null;
  let v: unknown;
  try {
    v = JSON.parse(content);
  } catch {
    return null;
  }
  if (!isObj(v) || !isObj(v.checkpoint) || !Array.isArray(v.paths)) return null;
  const { session, seq } = v.checkpoint;
  if (!validSessionId(session) || typeof seq !== "number" || !Number.isInteger(seq) || seq < 1) return null;
  const paths = v.paths;
  if (paths.length === 0 || !paths.every((p): p is string => typeof p === "string")) return null;
  return { session, seq, tool: toolName, paths };
}

const baseName = (p: string) => p.split("/").filter(Boolean).pop() ?? p;

/** One sentence for the member about an undo. */
export function describeUndo(o: UndoOutcome): string {
  if (!o.ok) {
    if (o.kind === "conflict" && o.conflicts.length > 0) {
      const which = o.conflicts.map((c) => c.path).join(", ");
      return `Not undone: ${which} changed after the agent's edit, so nothing was restored. Undo would overwrite those newer changes.`;
    }
    return `Not undone: ${o.reason ?? "the agent sidecar refused"}`;
  }
  if (o.undone.length === 0) return "Nothing left to undo.";
  const what = o.restored.length === 1 ? baseName(o.restored[0]) : `${o.restored.length} files`;
  const pruned = o.prunedThrough !== null ? ` Older changes, up to step ${o.prunedThrough}, were pruned and could not be undone.` : "";
  return `Undone: restored ${what}.${pruned}`;
}
