// =====================================================================
// HUP-S2.4 — approval cards generated from tool annotations.
//
// A card is what the member reads before deciding on an agent action. It always leads with ONE
// plain-language summary line (verb from the tool's `effect` annotation), then the facts in the
// form that fits the effect:
//   • diff    — a file write: the line diff between what is on disk and what would be written;
//   • chain   — a chain call: the SignatureCeremony decoder's own view (action / to / cost / chain /
//               origin). Undecodable calldata stays raw and is never shown as decoded;
//   • command — a command: the exact argv, one argument per entry (never re-joined or re-split),
//               plus, for a held shell_run (HUP-S2.2), the program, timeout and OS sandbox rows;
//   • fields  — anything else: the call's arguments by name.
// Cards are pure data; the existing SignatureCeremony and wallet-review modal render them.
// =====================================================================
import type { CeremonyView } from "../bridge/types";
import type { ToolAnnotation } from "./toolAnnotations";
import type { ShellPendingView } from "../bridge/domains";

export interface CardRow {
  k: string;
  v: string;
}

export interface DiffLine {
  op: "context" | "add" | "remove";
  text: string;
}

export type ApprovalCard =
  | { kind: "diff"; tool: string; summary: string; path: string; created: boolean; lines: DiffLine[] }
  | { kind: "chain"; tool: string; summary: string; raw: boolean; rows: CardRow[] }
  | { kind: "command"; tool: string; summary: string; argv: string[]; cwd?: string; rows?: CardRow[] }
  | { kind: "fields"; tool: string; summary: string; rows: CardRow[] };

/** A call that needs the member's explicit decision (the sidecar marked it `hic: "required"`). */
export interface HicRequirement {
  reason: string;
}

const VERB: Record<ToolAnnotation["effect"], string> = {
  none: "Hermes wants to read",
  write: "Hermes wants to change something",
  spend: "Hermes wants to move value",
  sign: "Hermes wants your signature",
};

/** The one-line, plain-language summary that heads every card. */
export function summaryLine(ann: ToolAnnotation | null, detail: string): string {
  return (ann ? VERB[ann.effect] : "Hermes wants to act") + ": " + detail;
}

const MAX_VALUE = 600;
function clip(v: string): string {
  return v.length > MAX_VALUE ? v.slice(0, MAX_VALUE) + " … (" + v.length + " characters)" : v;
}
function plural(n: number, word: string): string {
  return n + " " + word + (n === 1 ? "" : "s");
}

/** Above this many line pairs the LCS table is skipped: the diff shows all removed then all added. */
const LCS_CELL_LIMIT = 250_000;

/** A line diff (LCS). Bounded: very large inputs fall back to remove-all / add-all. */
export function lineDiff(before: string, after: string): DiffLine[] {
  const a = before === "" ? [] : before.split("\n");
  const b = after === "" ? [] : after.split("\n");
  if (a.length * b.length > LCS_CELL_LIMIT) {
    return [...a.map((text) => ({ op: "remove" as const, text })), ...b.map((text) => ({ op: "add" as const, text }))];
  }
  const n = a.length;
  const m = b.length;
  const lcs: number[][] = Array.from({ length: n + 1 }, () => new Array<number>(m + 1).fill(0));
  for (let i = n - 1; i >= 0; i--) {
    for (let j = m - 1; j >= 0; j--) {
      lcs[i][j] = a[i] === b[j] ? lcs[i + 1][j + 1] + 1 : Math.max(lcs[i + 1][j], lcs[i][j + 1]);
    }
  }
  const out: DiffLine[] = [];
  let i = 0;
  let j = 0;
  while (i < n && j < m) {
    if (a[i] === b[j]) {
      out.push({ op: "context", text: a[i] });
      i++;
      j++;
    } else if (lcs[i + 1][j] >= lcs[i][j + 1]) {
      out.push({ op: "remove", text: a[i++] });
    } else {
      out.push({ op: "add", text: b[j++] });
    }
  }
  while (i < n) out.push({ op: "remove", text: a[i++] });
  while (j < m) out.push({ op: "add", text: b[j++] });
  return out;
}

/** A file-write card: the diff between `before` (on disk; "" when absent) and `after`. */
export function diffCard(tool: string, ann: ToolAnnotation | null, path: string, before: string, after: string): ApprovalCard {
  const lines = lineDiff(before, after);
  const added = lines.filter((l) => l.op === "add").length;
  const removed = lines.filter((l) => l.op === "remove").length;
  const created = before === "";
  const detail = created
    ? `create ${path} (${plural(added, "line")} added)`
    : `edit ${path} (${plural(added, "line")} added, ${removed} removed)`;
  return { kind: "diff", tool, summary: summaryLine(ann, detail), path, created, lines };
}

/**
 * A chain card from the SignatureCeremony decoder's pending view. `costFallback` fills an empty
 * decoded cost (for example "network gas"). Undecodable calldata keeps only the facts that do not
 * depend on decoding.
 */
export function chainCard(tool: string, ann: ToolAnnotation | null, view: CeremonyView, costFallback = "—"): ApprovalCard {
  // A view without a decoded action is treated like undecodable calldata: never shown as decoded.
  if (view.requiresRawAck || !view.decoded) {
    return {
      kind: "chain",
      tool,
      raw: true,
      summary: summaryLine(ann, `a transaction on chain ${view.chainId} whose calldata could not be decoded`),
      rows: [
        { k: "Chain", v: String(view.chainId) },
        { k: "Origin", v: view.origin },
      ],
    };
  }
  const d = view.decoded;
  return {
    kind: "chain",
    tool,
    raw: false,
    summary: summaryLine(ann, `${d.action || "a transaction"} on chain ${view.chainId}`),
    rows: [
      { k: "Action", v: d.action || "—" },
      { k: "To", v: d.destination || "—" },
      { k: "Cost", v: d.cost || costFallback },
      { k: "Chain", v: String(view.chainId) },
      { k: "Origin", v: view.origin },
    ],
  };
}

/** The argv of a call, only when the arguments carry a non-empty array of strings. */
export function argvOf(args: Record<string, unknown>): string[] | null {
  const v = args.argv;
  if (!Array.isArray(v) || v.length === 0 || !v.every((x) => typeof x === "string")) return null;
  return v as string[];
}

/** A command card: the exact argv, one entry per argument. */
export function commandCard(tool: string, ann: ToolAnnotation | null, argv: string[], cwd?: string): ApprovalCard {
  return {
    kind: "command",
    tool,
    summary: summaryLine(ann, `run a command: ${argv[0]}` + (cwd ? ` in ${cwd}` : "")),
    argv: argv.slice(),
    ...(cwd ? { cwd } : {}),
  };
}

/** A card listing the call's arguments by name. */
export function fieldsCard(tool: string, ann: ToolAnnotation | null, args: Record<string, unknown>, detail: string): ApprovalCard {
  const rows = Object.entries(args).map(([k, v]) => ({ k, v: clip(typeof v === "string" ? v : JSON.stringify(v)) }));
  return { kind: "fields", tool, summary: summaryLine(ann, detail), rows };
}

/** The card for a call with no more specific shape: argv when present, otherwise its fields. */
export function cardForCall(tool: string, ann: ToolAnnotation | null, args: Record<string, unknown>): ApprovalCard {
  const argv = argvOf(args);
  if (argv) return commandCard(tool, ann, argv, typeof args.cwd === "string" ? args.cwd : undefined);
  return fieldsCard(tool, ann, args, `call ${tool}`);
}

/** HUP-S2.2 — why every shell_run needs the member (shown on the HIC banner). */
export const SHELL_RUN_HIC_REASON =
  "Hermes wants to run a command on this computer. Every command needs your explicit decision (HIC), and it runs only in the folder and sandbox shown.";

/**
 * HUP-S2.2 (US-2.2 AC2) — the card for a held shell_run: the exact argv (one entry per argument),
 * the canonical folder, the program that will run, the timeout and the OS sandbox it runs in.
 */
export function shellRunCard(p: ShellPendingView): ApprovalCard {
  const base = commandCard(p.tool, { effect: "write", trust: "untrusted" }, p.argv, p.cwd);
  const sandbox = p.sandbox.enforced ? p.sandbox.summary : `not enforced: ${p.sandbox.summary}`;
  const rows: CardRow[] = [
    { k: "Program", v: p.resolvedProgram },
    { k: "Timeout", v: `${p.timeoutSecs} s` },
    { k: "Sandbox", v: clip(sandbox) },
    { k: "Network", v: p.sandbox.network },
    { k: "Can write", v: clip(p.sandbox.writable.join("; ")) },
  ];
  return base.kind === "command" ? { ...base, rows } : base;
}
