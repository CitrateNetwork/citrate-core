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
// =====================================================================
import type { Activity, JournalPage } from "../shell/state";

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
export function hermesDayLines(input: { pages: JournalPage[]; activity: Activity[]; today: string }): string[] {
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
  return lines;
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
