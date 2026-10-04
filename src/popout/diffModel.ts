// =====================================================================
// citrate-core — the Code and diff pop-out's data (HUP-S5.4 over HUP-S2.9)
//
// What one checkpointed agent file change did, path by path, as Rust `hermes_checkpoint_diff`
// returns it (the sidecar's GET /checkpoints/:session/steps/:seq/diff). The "before" side is the
// snapshot the step took; the "after" side is the file on disk, shown only while it still holds what
// the step left there. Binary content and large files are described, never sent. Everything that
// crosses into the pop-out is checked here first (parseStepDiff), and a line diff is computed for
// text on both sides (lineDiff).
//
// Data source (Rule 7): the sidecar's checkpoint store, through Rust, called by the main window.
// =====================================================================
import { validSessionId } from "../agent/fileChanges";

export type DiffSide =
  | { kind: "absent" }
  | { kind: "text"; text: string }
  | { kind: "binary"; size: number }
  | { kind: "too_large"; size: number }
  | { kind: "symlink"; target: string }
  | { kind: "unavailable"; reason: string };

export interface FileDiff {
  path: string;
  before: DiffSide;
  after: DiffSide;
}

export interface StepDiff {
  ok: boolean;
  session: string;
  seq: number;
  status: string;
  files: FileDiff[];
  kind: string | null;
  reason: string | null;
}

/** Mirrors Rust `MAX_DIFF_FILES` and the sidecar's 256 KiB per side. */
export const MAX_DIFF_FILES = 200;
export const MAX_SIDE_CHARS = 256 * 1024;
const MAX_PATH = 4096;
const MAX_REASON = 500;
const STATUSES = ["", "prepared", "committed", "interrupted", "undone"];

const isObj = (v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null && !Array.isArray(v);
const isSize = (v: unknown): v is number => typeof v === "number" && Number.isInteger(v) && v >= 0;
const str = (v: unknown, max: number): v is string => typeof v === "string" && v.length <= max;

export function parseSide(raw: unknown): DiffSide | null {
  if (!isObj(raw)) return null;
  switch (raw.kind) {
    case "absent":
      return { kind: "absent" };
    case "text":
      return str(raw.text, MAX_SIDE_CHARS) ? { kind: "text", text: raw.text } : null;
    case "binary":
      return isSize(raw.size) ? { kind: "binary", size: raw.size } : null;
    case "too_large":
      return isSize(raw.size) ? { kind: "too_large", size: raw.size } : null;
    case "symlink":
      return str(raw.target, MAX_PATH) ? { kind: "symlink", target: raw.target } : null;
    case "unavailable":
      return str(raw.reason, MAX_REASON) ? { kind: "unavailable", reason: raw.reason } : null;
    default:
      return null;
  }
}

/** A step diff as received; null when anything is off (the pop-out shows an error instead). */
export function parseStepDiff(raw: unknown): StepDiff | null {
  if (!isObj(raw) || typeof raw.ok !== "boolean") return null;
  if (!validSessionId(raw.session) || !isSize(raw.seq) || raw.seq < 1) return null;
  if (typeof raw.status !== "string" || !STATUSES.includes(raw.status)) return null;
  if (!Array.isArray(raw.files) || raw.files.length > MAX_DIFF_FILES) return null;
  const files: FileDiff[] = [];
  for (const f of raw.files) {
    if (!isObj(f) || !str(f.path, MAX_PATH) || f.path.length === 0) return null;
    const before = parseSide(f.before);
    const after = parseSide(f.after);
    if (!before || !after) return null;
    files.push({ path: f.path, before, after });
  }
  const opt = (v: unknown) => (v === null || v === undefined ? null : str(v, MAX_REASON) ? v : undefined);
  const kind = opt(raw.kind);
  const reason = opt(raw.reason);
  if (kind === undefined || reason === undefined) return null;
  return { ok: raw.ok, session: raw.session, seq: raw.seq, status: raw.status, files, kind, reason };
}

// ---------------------------------------------------------------------------------------------
// Line diff
// ---------------------------------------------------------------------------------------------

export type DiffOp = "same" | "add" | "del";

export interface DiffLine {
  op: DiffOp;
  text: string;
  /** 1-based line number in the before text (null for an added line). */
  oldNo: number | null;
  /** 1-based line number in the after text (null for a removed line). */
  newNo: number | null;
}

/** Above this many comparison cells the middle is shown as removed then added, not aligned. */
export const MAX_COMPARE_CELLS = 2_000_000;

export function splitLines(s: string): string[] {
  if (s === "") return [];
  const lines = s.split("\n");
  if (lines[lines.length - 1] === "") lines.pop();
  return lines;
}

/** A line-by-line diff (longest common subsequence). `exact: false` when the changed middle was
 *  too large to align and is shown as a block removed then a block added. */
export function lineDiff(before: string, after: string): { lines: DiffLine[]; exact: boolean } {
  const a = splitLines(before);
  const b = splitLines(after);
  let pre = 0;
  while (pre < a.length && pre < b.length && a[pre] === b[pre]) pre++;
  let suf = 0;
  while (suf < a.length - pre && suf < b.length - pre && a[a.length - 1 - suf] === b[b.length - 1 - suf]) suf++;
  const out: DiffLine[] = [];
  for (let i = 0; i < pre; i++) out.push({ op: "same", text: a[i], oldNo: i + 1, newNo: i + 1 });
  const am = a.slice(pre, a.length - suf);
  const bm = b.slice(pre, b.length - suf);
  let exact = true;
  if (am.length * bm.length > MAX_COMPARE_CELLS) {
    exact = false;
    am.forEach((t, i) => out.push({ op: "del", text: t, oldNo: pre + i + 1, newNo: null }));
    bm.forEach((t, j) => out.push({ op: "add", text: t, oldNo: null, newNo: pre + j + 1 }));
  } else {
    const n = am.length;
    const m = bm.length;
    // lcs[i][j]: common length of am[i..] and bm[j..], flattened.
    const w = m + 1;
    const lcs = new Uint32Array((n + 1) * w);
    for (let i = n - 1; i >= 0; i--) {
      for (let j = m - 1; j >= 0; j--) {
        lcs[i * w + j] = am[i] === bm[j] ? lcs[(i + 1) * w + j + 1] + 1 : Math.max(lcs[(i + 1) * w + j], lcs[i * w + j + 1]);
      }
    }
    let i = 0;
    let j = 0;
    while (i < n || j < m) {
      if (i < n && j < m && am[i] === bm[j]) {
        out.push({ op: "same", text: am[i], oldNo: pre + i + 1, newNo: pre + j + 1 });
        i++;
        j++;
      } else if (j < m && (i === n || lcs[i * w + j + 1] >= lcs[(i + 1) * w + j])) {
        out.push({ op: "add", text: bm[j], oldNo: null, newNo: pre + j + 1 });
        j++;
      } else {
        out.push({ op: "del", text: am[i], oldNo: pre + i + 1, newNo: null });
        i++;
      }
    }
  }
  for (let k = 0; k < suf; k++) {
    const oi = a.length - suf + k;
    const ni = b.length - suf + k;
    out.push({ op: "same", text: a[oi], oldNo: oi + 1, newNo: ni + 1 });
  }
  return { lines: out, exact };
}

export function diffStats(lines: DiffLine[]): { added: number; removed: number } {
  let added = 0;
  let removed = 0;
  for (const l of lines) {
    if (l.op === "add") added++;
    else if (l.op === "del") removed++;
  }
  return { added, removed };
}

/** Groups of changed lines with `context` unchanged lines around each; unchanged runs between
 *  groups are left out. */
export function hunks(lines: DiffLine[], context = 3): DiffLine[][] {
  const keep = new Array<boolean>(lines.length).fill(false);
  lines.forEach((l, i) => {
    if (l.op === "same") return;
    for (let k = Math.max(0, i - context); k <= Math.min(lines.length - 1, i + context); k++) keep[k] = true;
  });
  const out: DiffLine[][] = [];
  let cur: DiffLine[] = [];
  lines.forEach((l, i) => {
    if (keep[i]) cur.push(l);
    else if (cur.length) {
      out.push(cur);
      cur = [];
    }
  });
  if (cur.length) out.push(cur);
  return out;
}

const kb = (n: number) => (n < 1024 ? `${n} bytes` : `${Math.round(n / 1024)} KB`);

/** One sentence for a side that is not text. */
export function describeSide(s: DiffSide, which: "before" | "after"): string {
  switch (s.kind) {
    case "absent":
      return which === "before" ? "The file did not exist before." : "The file was removed.";
    case "text":
      return "";
    case "binary":
      return `Binary content (${kb(s.size)}), not shown.`;
    case "too_large":
      return `Too large to show here (${kb(s.size)}).`;
    case "symlink":
      return `A link to ${s.target}.`;
    case "unavailable":
      return `Not shown: ${s.reason}.`;
  }
}
