// =====================================================================
// HUP-S10.4 — the member's daily journal entry.
//
// One entry per day: the daily page id is `d-YYYY-MM-DD` (the same id the
// journal_append tool writes to), so "open today" either selects the existing
// page or creates it once. The entry is an ordinary journal page, so it is
// edited with the existing editor.
//
// "What Hermes did today" is assembled ONLY from records already on this
// device (Rule 1): the `@agent` bullets the member approved into today's entry
// and the wallet activity rows recorded today. Demo seed rows are skipped.
// When there is nothing, the summary says so; it never invents activity. The
// summary is a plain, editable block the member can change or delete.
//
// US-10.4 AC1 (2026-10-04) adds three more local sources, each with an honest
// line when it has nothing or cannot be read (Rule 1):
//   - metering: the Hermes sidecar's daily report (`hermes_metering_daily`):
//     turns, sessions, outcomes, verifier results, reported tokens, top tools
//   - daemons: the daemon run log in the metering folder (`daemon_runs_between`)
//   - memory: the most recent facts in the personal memory tenant
//     (`memory recall personal`). The memory store keeps no dates, so these are
//     labelled "most recent", never "today".
// `@daemon` bullets (one per finished daemon run, written by the app) are left
// out of the summary: the daemon source already lists those runs.
// =====================================================================
import type { Activity, JournalPage } from "../shell/state";
import type { DailyResponse } from "./meteringView";
import type { DaemonRunEntry } from "../daemons/api";
import type { MemoryHit } from "../bridge/domains";

/** Header bullet that opens the summary block (the block's lines follow, indented). */
export const HERMES_SUMMARY_HEADER = "What Hermes did today (from local records on this device):";
/** The honest line used when no local record mentions Hermes today. */
export const NO_HERMES_ACTIVITY_LINE = "No approved Hermes notes or wallet activity are recorded on this device today.";

const DAY_RE = /^\d{4}-\d{2}-\d{2}$/;
const INDENT = "  ";

/** The journal page id for a day. Throws on anything that is not `YYYY-MM-DD`. */
export function dailyId(day: string): string {
  if (!DAY_RE.test(day)) throw new Error(`daily entry day must be YYYY-MM-DD, got "${day}"`);
  return "d-" + day;
}

/**
 * Make sure today's entry exists. Returns the SAME `pages` array when it already
 * does (so callers can skip a state write), else a new array with the empty entry
 * first (newest daily first, matching the sidebar order).
 */
export function ensureDailyEntry(pages: JournalPage[], today: string): { pages: JournalPage[]; id: string; created: boolean } {
  const id = dailyId(today);
  if (pages.some((p) => p.id === id)) return { pages, id, created: false };
  const entry: JournalPage = { id, title: today, kind: "daily", pinned: false, blocks: [] };
  return { pages: [entry, ...pages], id, created: true };
}

/** Remove an existing summary block (header + its indented lines) from `blocks`. */
function withoutSummary(blocks: string[]): string[] {
  const out: string[] = [];
  let inSummary = false;
  for (const b of blocks) {
    if (b.trim() === HERMES_SUMMARY_HEADER) {
      inSummary = true;
      continue;
    }
    if (inSummary && b.startsWith(INDENT)) continue;
    inSummary = false;
    out.push(b);
  }
  return out;
}

function statusLabel(a: Activity): string {
  if (a.status === 1) return "confirmed";
  if (a.status === 0) return "failed";
  return "pending";
}

function utcDay(ts: number): string | null {
  if (!Number.isFinite(ts)) return null;
  return new Date(ts).toISOString().slice(0, 10);
}

/**
 * The summary lines for `today`, from local records only. Empty when nothing is
 * recorded (the caller renders {@link NO_HERMES_ACTIVITY_LINE}).
 */
export function hermesDayLines(input: { pages: JournalPage[]; activity: Activity[]; today: string; sources?: DaySources }): string[] {
  const id = dailyId(input.today);
  const lines: string[] = [];

  const entry = input.pages.find((p) => p.id === id);
  if (entry) {
    for (const b of withoutSummary(entry.blocks)) {
      const t = b.trim();
      if (t.startsWith("@agent ")) {
        const text = t.slice(7).trim();
        if (text) lines.push("Journal note you approved: " + text);
      }
    }
  }

  for (const a of input.activity || []) {
    if (a.id.startsWith("seed")) continue; // demo persona rows, not a real record
    if (utcDay(a.ts) !== input.today) continue;
    // Wallet rows are not tagged by who started them, so they are not credited to Hermes.
    lines.push(`Wallet activity (any, not only Hermes): ${a.kind} · ${a.amount} (${statusLabel(a)})`);
  }
  if (input.sources) lines.push(...sourceLines(input.sources));
  return lines;
}

// ---------------------------------------------------------------------
// US-10.4 AC1 — the metering, daemon and memory sources
// ---------------------------------------------------------------------

/** One source as read: its data, or why it could not be read. `null` = not read (web preview). */
export type SourceRead<T> = { ok: true; value: T } | { ok: false; why: string } | null;

export interface DaySources {
  metering: SourceRead<DailyResponse>;
  daemonRuns: SourceRead<{ runs: DaemonRunEntry[]; unreadable: number }>;
  memory: SourceRead<MemoryHit[]>;
}

/** Most daemon runs and memory facts listed in the summary. */
export const MAX_DAEMON_LINES = 12;
export const MAX_MEMORY_LINES = 5;
/** Longest memory fact title shown, in characters. */
const MAX_FACT_CHARS = 160;

/** The day's sidecar metering as summary lines (sessions, turns, outcomes, checks, tokens, tools). */
export function meteringLines(m: SourceRead<DailyResponse>): string[] {
  if (m === null) return []; // not read (the web preview has no sidecar): the summary is unchanged
  if (!m.ok) return [`Hermes metering: could not be read (${m.why}).`];
  const r = m.value.report;
  if (r.turns === 0) return [`Hermes metering: no Hermes turns recorded today (${m.value.source === "log" ? "metering log" : "this session only"}).`];
  const o = r.outcomes;
  const lines = [
    `Hermes metering: ${r.turns} turn${r.turns === 1 ? "" : "s"} in ${r.sessions} session${r.sessions === 1 ? "" : "s"}; ${o.answered} answered, ${o.stopped} stopped, ${o.step_limit} hit the step limit, ${o.failed} failed.`,
  ];
  const v = r.verification;
  if (v.passed + v.failed > 0) lines.push(`Checks on Hermes's work: ${v.passed} passed, ${v.failed} failed, ${v.unverified} turns not checked.`);
  lines.push(
    r.tokens.turns_reporting === 0
      ? "Tokens: unknown (the model did not report usage)."
      : `Tokens: ${r.tokens.tokens_in} in, ${r.tokens.tokens_out} out (reported for ${r.tokens.turns_reporting} of ${r.turns} turns).`,
  );
  const tools = Object.entries(r.tool_calls)
    .sort((a, b) => b[1].calls - a[1].calls || (a[0] < b[0] ? -1 : 1))
    .slice(0, 5);
  if (tools.length) {
    const hic = Object.values(r.tool_calls).reduce((n, t) => n + t.hic_required, 0);
    lines.push(`Tools used: ${tools.map(([n, t]) => `${n} x${t.calls}`).join(", ")}${hic ? `; ${hic} needed your explicit decision` : ""}.`);
  }
  if (r.tainted_turns) lines.push(`${r.tainted_turns} turn${r.tainted_turns === 1 ? "" : "s"} read untrusted content.`);
  return lines;
}

const OUTCOME_WORDS: Record<string, string> = {
  answered: "answered",
  failed: "failed",
  stopped: "stopped",
  timed_out: "passed its time limit",
  over_budget: "reached its token allowance",
};

/** The day's daemon runs as summary lines (from the run log). */
export function daemonLines(d: SourceRead<{ runs: DaemonRunEntry[]; unreadable: number }>): string[] {
  if (d === null) return [];
  if (!d.ok) return [`Daemon runs: could not be read (${d.why}).`];
  const out: string[] = [];
  if (d.value.runs.length === 0) out.push("Daemon runs: none today.");
  const shown = d.value.runs.slice(-MAX_DAEMON_LINES);
  for (const r of shown) {
    out.push(`Daemon run: ${r.name} ${OUTCOME_WORDS[r.outcome] ?? r.outcome}, ${r.tokens} tokens (${r.tokenSource}).`);
  }
  if (d.value.runs.length > shown.length) out.push(`(${d.value.runs.length - shown.length} earlier daemon runs not listed.)`);
  if (d.value.unreadable) out.push(`(${d.value.unreadable} daemon log line${d.value.unreadable === 1 ? "" : "s"} could not be read and were skipped.)`);
  return out;
}

/** The most recent personal-memory facts as summary lines (the store keeps no dates). */
export function memoryLines(m: SourceRead<MemoryHit[]>): string[] {
  if (m === null) return [];
  if (!m.ok) return [`Personal memory: could not be read (${m.why}).`];
  const facts = m.value.filter((h) => h.title.trim()).slice(0, MAX_MEMORY_LINES);
  if (facts.length === 0) return ["Personal memory: no facts stored yet."];
  return facts.map((h) => {
    const t = h.title.replace(/\s+/g, " ").trim();
    const title = t.length > MAX_FACT_CHARS ? t.slice(0, MAX_FACT_CHARS) + "…" : t;
    return `Recent personal memory (undated): ${title}${h.status && h.status !== "accepted" ? ` (${h.status})` : ""}`;
  });
}

/** Every source's lines, in a fixed order. */
export function sourceLines(src: DaySources): string[] {
  return [...meteringLines(src.metering), ...daemonLines(src.daemonRuns), ...memoryLines(src.memory)];
}

// ---------------------------------------------------------------------
// HUP-S10.3 — a finished daemon run, written into today's entry by the app
// ---------------------------------------------------------------------

/** Prefix of the app-written daemon run bullets. */
export const DAEMON_BULLET = "@daemon ";

/** The bullet for one finished daemon run: typed fields only, never the run's reply text. */
export function daemonBullet(r: Pick<DaemonRunEntry, "name" | "outcome" | "tokens" | "tokenSource" | "endedMs">): string {
  const at = new Date(r.endedMs).toISOString().slice(11, 16);
  const name = r.name.replace(/\s+/g, " ").trim().slice(0, 80);
  return `${DAEMON_BULLET}${name} ${OUTCOME_WORDS[r.outcome] ?? r.outcome} at ${at} UTC, ${r.tokens} tokens (${r.tokenSource})`;
}

/** Add a daemon run's bullet to the day's entry (created when missing). */
export function withDaemonBullet(pages: JournalPage[], day: string, bullet: string): JournalPage[] {
  const r = ensureDailyEntry(pages, day);
  return r.pages.map((p) => (p.id === r.id ? { ...p, blocks: p.blocks.concat([bullet]) } : p));
}

/**
 * Put the summary into an entry's blocks: any earlier summary block is replaced
 * (never stacked), the member's own bullets are kept in order, and the new block
 * goes at the end.
 */
export function applyHermesSummary(blocks: string[], lines: string[]): string[] {
  const body = lines.length ? lines : [NO_HERMES_ACTIVITY_LINE];
  return [...withoutSummary(blocks), HERMES_SUMMARY_HEADER, ...body.map((l) => INDENT + l)];
}
