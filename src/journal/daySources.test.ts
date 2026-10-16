// HUP-S10.4 (US-10.4 AC1) — the daily entry merges the sidecar's metering, the daemon run log and
// recent personal-memory facts, each with an honest line when it has nothing or cannot be read.
import { describe, it, expect, vi } from "vitest";
import { loadDaySources, utcDayBounds } from "./daySources";
import {
  DAEMON_BULLET,
  HERMES_SUMMARY_HEADER,
  applyHermesSummary,
  daemonBullet,
  daemonLines,
  hermesDayLines,
  memoryLines,
  meteringLines,
  withDaemonBullet,
  type DaySources,
} from "./dailyEntry";
import type { DailyResponse } from "./meteringView";

const DAY = "2026-10-04";

function report(over: Partial<DailyResponse["report"]> = {}): DailyResponse {
  return {
    day: DAY,
    source: "log",
    persisted: true,
    markdown: "",
    notMeasured: [],
    report: {
      schema: 1,
      day: DAY,
      turns: 6,
      sessions: 2,
      outcomes: { answered: 5, stopped: 1, step_limit: 0, failed: 0, unknown: 0 },
      verification: { passed: 2, failed: 1, unverified: 3 },
      verified_success_bps: 6666,
      latency_ms: { p50: 900, p95: 4000, max: 5000 },
      tokens: { tokens_in: 12000, tokens_out: 800, turns_reporting: 6 },
      steps_total: 11,
      tool_calls: {
        node_status: { calls: 3, ok: 3, denied: 0, error: 0, hic_required: 0 },
        gsheets_append: { calls: 1, ok: 1, denied: 0, error: 0, hic_required: 1 },
      },
      verifiers: {},
      models: { "gemma-4-e4b": 6 },
      tainted_turns: 1,
      ...over,
    },
  };
}

describe("metering lines", () => {
  it("summarise sessions, outcomes, checks, tokens and tools", () => {
    expect(meteringLines({ ok: true, value: report() })).toEqual([
      "Hermes metering: 6 turns in 2 sessions; 5 answered, 1 stopped, 0 hit the step limit, 0 failed.",
      "Checks on Hermes's work: 2 passed, 1 failed, 3 turns not checked.",
      "Tokens: 12000 in, 800 out (reported for 6 of 6 turns).",
      "Tools used: node_status x3, gsheets_append x1; 1 needed your explicit decision.",
      "1 turn read untrusted content.",
    ]);
  });
  it("say unknown tokens, an empty day and an unreadable report plainly", () => {
    expect(meteringLines({ ok: true, value: report({ tokens: { tokens_in: 0, tokens_out: 0, turns_reporting: 0 } }) })).toContain("Tokens: unknown (the model did not report usage).");
    expect(meteringLines({ ok: true, value: report({ turns: 0 }) })).toEqual(["Hermes metering: no Hermes turns recorded today (metering log)."]);
    expect(meteringLines({ ok: false, why: "Hermes is not running" })).toEqual(["Hermes metering: could not be read (Hermes is not running)."]);
    expect(meteringLines(null)).toEqual([]);
  });
});

describe("daemon and memory lines", () => {
  const run = (over = {}) => ({ schema: 1, daemonId: "d1", runId: "r1", name: "Node digest", startedMs: 0, endedMs: 1, tokens: 720, tokenSource: "measured" as const, outcome: "answered" as const, ...over });
  it("list each run with its token source", () => {
    expect(daemonLines({ ok: true, value: { runs: [run(), run({ runId: "r2", outcome: "over_budget", tokenSource: "estimated", tokens: 6000 })], unreadable: 1 } })).toEqual([
      "Daemon run: Node digest answered, 720 tokens (measured).",
      "Daemon run: Node digest reached its token allowance, 6000 tokens (estimated).",
      "(1 daemon log line could not be read and were skipped.)",
    ]);
    expect(daemonLines({ ok: true, value: { runs: [], unreadable: 0 } })).toEqual(["Daemon runs: none today."]);
    expect(daemonLines({ ok: false, why: "denied" })).toEqual(["Daemon runs: could not be read (denied)."]);
  });
  it("label memory facts as recent and undated", () => {
    expect(memoryLines({ ok: true, value: [{ id: "1", kind: "fact", title: "Prefers  metric\nunits", status: "accepted" }, { id: "2", kind: "fact", title: "Works on Citrate", status: "proposed" }] })).toEqual([
      "Recent personal memory (undated): Prefers metric units",
      "Recent personal memory (undated): Works on Citrate (proposed)",
    ]);
    expect(memoryLines({ ok: true, value: [] })).toEqual(["Personal memory: no facts stored yet."]);
    expect(memoryLines({ ok: false, why: "memory is off" })).toEqual(["Personal memory: could not be read (memory is off)."]);
  });
});

describe("the daily summary block", () => {
  it("merges the sources after the approved notes and leaves @daemon bullets out", () => {
    const pages = [{ id: "d-" + DAY, title: DAY, kind: "daily" as const, pinned: false, blocks: ["@agent shipped the widget", DAEMON_BULLET + "Node digest answered at 09:00 UTC, 720 tokens (measured)"] }];
    const sources: DaySources = { metering: { ok: true, value: report({ turns: 0 }) }, daemonRuns: { ok: true, value: { runs: [], unreadable: 0 } }, memory: { ok: true, value: [] } };
    const lines = hermesDayLines({ pages, activity: [], today: DAY, sources });
    expect(lines).toEqual([
      "Journal note you approved: shipped the widget",
      "Hermes metering: no Hermes turns recorded today (metering log).",
      "Daemon runs: none today.",
      "Personal memory: no facts stored yet.",
    ]);
    const blocks = applyHermesSummary(pages[0].blocks, lines);
    expect(blocks.slice(0, 2)).toEqual(pages[0].blocks);
    expect(blocks[2]).toBe(HERMES_SUMMARY_HEADER);
  });
});

describe("daemon run bullets in the journal", () => {
  it("carry typed fields only and land in the run's day entry", () => {
    const b = daemonBullet({ name: "Node\n digest", outcome: "timed_out", tokens: 12, tokenSource: "estimated", endedMs: Date.parse("2026-10-04T09:05:00Z") });
    expect(b).toBe("@daemon Node digest passed its time limit at 09:05 UTC, 12 tokens (estimated)");
    const pages = withDaemonBullet([], DAY, b);
    expect(pages).toEqual([{ id: "d-" + DAY, title: DAY, kind: "daily", pinned: false, blocks: [b] }]);
    expect(withDaemonBullet(pages, DAY, b)[0].blocks).toEqual([b, b]);
  });
});

describe("loadDaySources", () => {
  it("reads each source on its own; one failure leaves the others", async () => {
    const invoke = vi.fn(async (cmd: string, args: Record<string, unknown>) => {
      if (cmd === "hermes_metering_daily") throw new Error("Hermes is not running");
      if (cmd === "daemon_runs_between") return { runs: [], unreadable: 0, args };
      throw new Error("unexpected " + cmd);
    });
    const s = await loadDaySources({ mode: "tauri", invoke: invoke as never, recallPersonal: async () => ({ tenant: "personal", totalInTenant: 1, hits: [{ id: "1", kind: "fact", title: "x" }] }) }, DAY);
    expect(s.metering).toEqual({ ok: false, why: "Hermes is not running" });
    expect(invoke).toHaveBeenCalledWith("daemon_runs_between", { fromMs: Date.parse(DAY + "T00:00:00Z"), toMs: Date.parse(DAY + "T00:00:00Z") + 86_400_000 });
    expect(s.daemonRuns?.ok).toBe(true);
    expect(s.memory).toEqual({ ok: true, value: [{ id: "1", kind: "fact", title: "x" }] });
  });
  it("reads nothing in the web preview", async () => {
    const invoke = vi.fn();
    const recall = vi.fn();
    expect(await loadDaySources({ mode: "sim", invoke, recallPersonal: recall }, DAY)).toEqual({ metering: null, daemonRuns: null, memory: null });
    expect(invoke).not.toHaveBeenCalled();
    expect(recall).not.toHaveBeenCalled();
  });
  it("refuses a malformed day", () => {
    expect(() => utcDayBounds("10/04/2026")).toThrow(/YYYY-MM-DD/);
  });
});
