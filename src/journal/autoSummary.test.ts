// HUP-S10.4 (US-10.4 AC1) — the optional once-a-day journal summary is OFF by default, runs at most
// once per journal day after its hour when the member turns it on, and a finished daemon run is
// written into its day's entry by the app.
import { describe, it, expect, beforeEach, vi } from "vitest";
import { store, AUTO_SUMMARY_UTC_HOUR } from "../shell/store";
import { freshState } from "../shell/state";
import { HERMES_SUMMARY_HEADER } from "./dailyEntry";

const DAY = "2026-10-04";
const at = (h: number) => Date.parse(`${DAY}T${String(h).padStart(2, "0")}:30:00Z`);

beforeEach(() => {
  vi.restoreAllMocks();
  vi.spyOn(store, "save").mockImplementation(() => {});
  store.setState({ jPages: [], activity: [], journalAutoSummary: false, journalAutoSummaryDay: null });
});

describe("the once-a-day summary", () => {
  it("is off by default on a fresh install", () => {
    const s = freshState("p1");
    expect(s.journalAutoSummary).toBe(false);
    expect(s.journalAutoSummaryDay).toBeNull();
  });

  it("does nothing while off", async () => {
    expect(await store.maybeAutoDailySummary(at(23))).toBe(false);
    expect(store.state.jPages).toEqual([]);
  });

  it("when on, waits for its hour, writes once, and not again that day", async () => {
    store.setJournalAutoSummary(true);
    expect(AUTO_SUMMARY_UTC_HOUR).toBe(23);
    expect(await store.maybeAutoDailySummary(at(22))).toBe(false);
    expect(await store.maybeAutoDailySummary(at(23))).toBe(true);
    const entry = store.state.jPages.find((p) => p.id === "d-" + DAY);
    expect(entry?.blocks).toContain(HERMES_SUMMARY_HEADER);
    expect(store.state.journalAutoSummaryDay).toBe(DAY);
    expect(await store.maybeAutoDailySummary(at(23) + 60_000)).toBe(false);
    expect(store.state.jPages.find((p) => p.id === "d-" + DAY)?.blocks.filter((b) => b === HERMES_SUMMARY_HEADER)).toHaveLength(1);
  });
});

describe("a finished daemon run in the journal", () => {
  it("lands in the day entry of its end time, without the run's reply", () => {
    store.recordDaemonRunInJournal({ name: "Node digest", outcome: "answered", tokens: 720, tokenSource: "measured", endedMs: at(9) });
    expect(store.state.jPages).toEqual([{ id: "d-" + DAY, title: DAY, kind: "daily", pinned: false, blocks: ["@daemon Node digest answered at 09:30 UTC, 720 tokens (measured)"] }]);
  });
});
