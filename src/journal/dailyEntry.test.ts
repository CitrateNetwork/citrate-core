// HUP-S10.4 — the daily entry flow (one entry per day, editable) and the
// "what Hermes did today" summary built only from real local records.
import { describe, it, expect } from "vitest";
import type { Activity, JournalPage } from "../shell/state";
import {
  HERMES_SUMMARY_HEADER,
  NO_HERMES_ACTIVITY_LINE,
  applyHermesSummary,
  dailyId,
  ensureDailyEntry,
  hermesDayLines,
} from "./dailyEntry";

const TODAY = "2026-10-01";
const NOON_TODAY = Date.parse("2026-10-01T12:00:00Z");
const YESTERDAY = Date.parse("2026-09-30T12:00:00Z");

function page(id: string, blocks: string[], kind: JournalPage["kind"] = "page"): JournalPage {
  return { id, title: id.startsWith("d-") ? id.slice(2) : id, kind, pinned: false, blocks };
}

describe("ensureDailyEntry — one entry per day", () => {
  it("creates today's daily entry when it is missing and puts it first", () => {
    const pages = [page("p-notes", ["a"])];
    const r = ensureDailyEntry(pages, TODAY);
    expect(r.created).toBe(true);
    expect(r.id).toBe("d-" + TODAY);
    expect(r.pages[0]).toEqual({ id: "d-" + TODAY, title: TODAY, kind: "daily", pinned: false, blocks: [] });
    expect(r.pages.slice(1)).toEqual(pages);
  });

  it("returns the existing entry untouched (same array) when today already exists", () => {
    const pages = [page("p-notes", ["a"]), page("d-" + TODAY, ["kept"], "daily")];
    const r = ensureDailyEntry(pages, TODAY);
    expect(r.created).toBe(false);
    expect(r.id).toBe("d-" + TODAY);
    expect(r.pages).toBe(pages);
  });

  it("never creates a second entry for the same day across repeated calls", () => {
    let pages: JournalPage[] = [];
    for (let i = 0; i < 3; i++) pages = ensureDailyEntry(pages, TODAY).pages;
    expect(pages.filter((p) => p.id === dailyId(TODAY))).toHaveLength(1);
  });

  it("rejects a malformed day string instead of minting a bogus page id", () => {
    expect(() => ensureDailyEntry([], "10/01/2026")).toThrow(/YYYY-MM-DD/);
    expect(() => dailyId("")).toThrow(/YYYY-MM-DD/);
  });
});

describe("hermesDayLines — real local records only", () => {
  it("is empty when nothing happened today (never invents activity)", () => {
    expect(hermesDayLines({ pages: [], activity: [], today: TODAY })).toEqual([]);
  });

  it("lists today's approved @agent journal notes from today's entry only", () => {
    const pages = [
      page("d-" + TODAY, ["my own note", "@agent indexed the contract catalog", "@prompt not an agent write"], "daily"),
      page("d-2026-09-30", ["@agent yesterday's note"], "daily"),
      page("p-worklog", ["@agent named-page note is not dated today"]),
    ];
    const lines = hermesDayLines({ pages, activity: [], today: TODAY });
    expect(lines).toEqual(["Journal note you approved: indexed the contract catalog"]);
  });

  it("lists wallet activity recorded today with its real status, skipping demo seed rows and other days", () => {
    const activity: Activity[] = [
      { id: "a1", kind: "Claim rewards", amount: "+1.00 SALT", hash: "0x1", ts: NOON_TODAY, status: 1 },
      { id: "a2", kind: "Send", amount: "-2.00 SALT", hash: "0x2", ts: NOON_TODAY, status: 0 },
      { id: "a3", kind: "Add stake", amount: "-3.00 SALT", hash: "0x3", ts: NOON_TODAY },
      { id: "a4", kind: "Old", amount: "x", hash: "0x4", ts: YESTERDAY, status: 1 },
      { id: "seed0", kind: "Seeded demo row", amount: "y", hash: "0x5", ts: NOON_TODAY, status: 1 },
    ];
    const lines = hermesDayLines({ pages: [], activity, today: TODAY });
    expect(lines).toEqual([
      "Wallet activity (any, not only Hermes): Claim rewards · +1.00 SALT (confirmed)",
      "Wallet activity (any, not only Hermes): Send · -2.00 SALT (failed)",
      "Wallet activity (any, not only Hermes): Add stake · -3.00 SALT (pending)",
    ]);
  });

  it("ignores lines inside an earlier summary block so a re-run does not count itself", () => {
    const blocks = applyHermesSummary(["@agent real note"], ["Journal note you approved: real note"]);
    const lines = hermesDayLines({ pages: [page("d-" + TODAY, blocks, "daily")], activity: [], today: TODAY });
    expect(lines).toEqual(["Journal note you approved: real note"]);
  });
});

describe("summary wording stays honest about its sources", () => {
  it("does not credit Hermes with every wallet row, and names the limited sources when empty", () => {
    const lines = hermesDayLines({ pages: [], activity: [{ id: "w1", kind: "Send", amount: "1", hash: "0x", ts: NOON_TODAY, status: 1 }], today: TODAY });
    expect(lines[0]).toMatch(/not only Hermes/);
    expect(NO_HERMES_ACTIVITY_LINE).toMatch(/approved Hermes notes or wallet activity/);
  });
});

describe("applyHermesSummary — editable, idempotent block", () => {
  it("appends a header with indented lines", () => {
    const out = applyHermesSummary(["mine"], ["Wallet activity: Send · 1 (pending)"]);
    expect(out).toEqual(["mine", HERMES_SUMMARY_HEADER, "  Wallet activity: Send · 1 (pending)"]);
  });

  it("says plainly when there is nothing to report", () => {
    const out = applyHermesSummary([], []);
    expect(out).toEqual([HERMES_SUMMARY_HEADER, "  " + NO_HERMES_ACTIVITY_LINE]);
  });

  it("replaces an earlier summary instead of stacking a second one, keeping the member's own lines", () => {
    const first = applyHermesSummary(["before"], ["Wallet activity: A · 1 (pending)"]);
    const edited = first.concat(["after"]);
    const second = applyHermesSummary(edited, ["Wallet activity: B · 2 (confirmed)"]);
    expect(second).toEqual(["before", "after", HERMES_SUMMARY_HEADER, "  Wallet activity: B · 2 (confirmed)"]);
    expect(second.filter((b) => b === HERMES_SUMMARY_HEADER)).toHaveLength(1);
  });
});
