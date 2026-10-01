// =====================================================================
// citrate-core — the Activity monitor's undo panel (HUP-S2.9)
//
// What the monitor shows about the agent session's recent file changes: the steps the sidecar's
// checkpoint store lists (newest first), whether undo is available and why not, whether an undo is
// running, and the last result in words. Built in the main window from the agentUndo slice; the
// pop-out validates every panel it receives and only asks the main window to undo.
// =====================================================================
import { validSessionId } from "../agent/fileChanges";

export const MAX_PANEL_STEPS = 100;
const STEP_STATUSES = ["prepared", "committed", "interrupted", "undone"];

export interface UndoPanelStep {
  seq: number;
  status: string;
  /** Paths relative to the granted folder. */
  paths: string[];
}

export interface UndoPanel {
  /** The agent session the steps belong to, or null before any agent file change. */
  session: string | null;
  enabled: boolean;
  /** Why undo is not available, or null. */
  note: string | null;
  busy: boolean;
  steps: UndoPanelStep[];
  last: { ok: boolean; text: string } | null;
}

const isObj = (v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null && !Array.isArray(v);
const isPosInt = (v: unknown): v is number => typeof v === "number" && Number.isInteger(v) && v > 0;

function isStep(v: unknown): v is UndoPanelStep {
  return (
    isObj(v) &&
    isPosInt(v.seq) &&
    typeof v.status === "string" &&
    STEP_STATUSES.includes(v.status) &&
    Array.isArray(v.paths) &&
    v.paths.every((p) => typeof p === "string")
  );
}

export function isUndoPanel(v: unknown): v is UndoPanel {
  if (!isObj(v)) return false;
  if (!(v.session === null || validSessionId(v.session))) return false;
  if (typeof v.enabled !== "boolean" || typeof v.busy !== "boolean") return false;
  if (!(v.note === null || typeof v.note === "string")) return false;
  if (!Array.isArray(v.steps) || v.steps.length > MAX_PANEL_STEPS || !v.steps.every(isStep)) return false;
  if (!(v.last === null || (isObj(v.last) && typeof v.last.ok === "boolean" && typeof v.last.text === "string"))) return false;
  return true;
}

/** A positive step number, or null for "the whole session". */
export function isUndoTarget(v: unknown): v is number | null {
  return v === null || isPosInt(v);
}
