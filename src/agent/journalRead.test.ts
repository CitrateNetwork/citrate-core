// Hermes P4 / WP4.2 — journal_read formatter tests. Pure, no DOM.
import { describe, it, expect } from "vitest";
import { formatJournalForAgent, type JournalPageLike } from "./journalRead";

const TODAY = "2026-09-11";

const pages: JournalPageLike[] = [
  { id: "d-2026-09-11", title: "2026-09-11", kind: "daily", blocks: ["node held overnight", "@agent staked 32k SALT", "  "] },
  { id: "p-ideas", title: "Ideas", kind: "page", blocks: ["@prompt draft the launch post", "ship P4"] },
];

describe("formatJournalForAgent", () => {
  it("empty journal is honest — never invents entries (Rule 1)", () => {
    expect(formatJournalForAgent([], undefined, TODAY)).toMatch(/empty/i);
  });

  it("no query → an index of pages plus today's note", () => {
    const out = formatJournalForAgent(pages, undefined, TODAY);
    expect(out).toContain("Ideas (page, 2 bullets)");
    expect(out).toContain("2026-09-11 (daily, 2 bullets)"); // blank bullet not counted
    expect(out).toContain(`Today (${TODAY})`);
    expect(out).toContain("node held overnight");
  });

  it("query matches a page title (case-insensitive) and strips markers with a tag", () => {
    const out = formatJournalForAgent(pages, "ideas", TODAY);
    expect(out).toContain("draft the launch post (prompt)");
    expect(out).toContain("- ship P4");
    expect(out).not.toContain("@prompt");
  });

  it("agent-authored bullets are tagged, blanks dropped", () => {
    const out = formatJournalForAgent(pages, "2026-09-11", TODAY);
    expect(out).toContain("staked 32k SALT (agent)");
    expect(out).not.toContain("@agent");
    expect(out).not.toMatch(/- +$/m); // no empty bullet
  });

  it("a non-matching query is an honest no-match, not a guess", () => {
    expect(formatJournalForAgent(pages, "nonexistent", TODAY)).toMatch(/no journal page matches/i);
  });

  it("no daily note today → says so", () => {
    const out = formatJournalForAgent([pages[1]], undefined, TODAY);
    expect(out).toMatch(/No note for today/i);
  });
});
