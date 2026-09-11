// =====================================================================
// Hermes P4 / WP4.2 — journal read (the pure formatter behind the journal_read tool).
//
// Renders the member's LOCAL journal (daily notes + named pages) into plain text the
// agent can ground on. Read-only. Honest-empty (Rule 1): an empty journal / no match
// says so plainly — it never invents entries. `@agent` / `@prompt` bullet markers are
// surfaced as a light "(agent)" / "(prompt)" tag so provenance stays visible.
// =====================================================================

/** The journal page shape this reads (subset of the app's JournalPage). */
export interface JournalPageLike {
  id: string;
  title: string;
  kind: string; // "daily" | "page" | ...
  blocks: string[];
}

/** Strip a leading `@agent`/`@prompt` marker, returning the clean text + a tag. */
function bulletText(raw: string): { text: string; tag: string } {
  const t = raw.trim();
  if (t.startsWith("@agent ")) return { text: t.slice(7).trim(), tag: " (agent)" };
  if (t.startsWith("@prompt ")) return { text: t.slice(8).trim(), tag: " (prompt)" };
  return { text: t, tag: "" };
}

/** Render one page's non-empty bullets as a bulleted block (or an empty note). */
function renderPage(p: JournalPageLike): string {
  const bullets = p.blocks
    .map(bulletText)
    .filter((b) => b.text !== "")
    .map((b) => `- ${b.text}${b.tag}`);
  const header = `${p.title} (${p.kind})`;
  return bullets.length ? `${header}:\n${bullets.join("\n")}` : `${header}: (empty)`;
}

/**
 * Format the journal for the agent. With no `query`, return an index of pages plus
 * today's daily note. With a `query`, return the first page whose title contains it
 * (case-insensitive), else an honest "no match". `today` is passed in (the caller
 * supplies the date — the module stays free of Date.now for testability).
 */
export function formatJournalForAgent(
  pages: JournalPageLike[],
  query: string | undefined,
  today: string,
): string {
  if (!pages || pages.length === 0) {
    return "Your journal is empty — no pages yet.";
  }

  const q = (query ?? "").trim().toLowerCase();
  if (q) {
    const match = pages.find(
      (p) => p.title.toLowerCase().includes(q) || p.id.toLowerCase() === q,
    );
    return match ? renderPage(match) : `No journal page matches "${query}".`;
  }

  // Index: title · kind · bullet count, newest daily first is the caller's order.
  const index = pages
    .map((p) => {
      const n = p.blocks.filter((b) => b.trim() !== "").length;
      return `- ${p.title} (${p.kind}, ${n} bullet${n === 1 ? "" : "s"})`;
    })
    .join("\n");

  const daily = pages.find((p) => p.id === "d-" + today);
  const todayBlock = daily
    ? `\n\nToday (${today}):\n${renderPage(daily)}`
    : `\n\nNo note for today (${today}) yet.`;

  return `Journal pages:\n${index}${todayBlock}`;
}
