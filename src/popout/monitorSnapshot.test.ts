// HUP-S7.6 — what the Activity monitor shows, built only from real app state. Where the app has no
// real number (token usage, gateway spend, an unprobed tier) the snapshot says "unknown" (null) and
// why; it never fills in an estimate (Rule 1).
import { describe, it, expect } from "vitest";
import { buildMonitorSnapshot, spendFor, contextFor, providerClass, waitingReason, formatElapsed, type MonitorInputs } from "./monitorSnapshot";
import { IDLE_ACTIVITY } from "../shell/slices/turnActivity";

const base: MonitorInputs = {
  activity: IDLE_ACTIVITY,
  providerKind: "local",
  providerLabel: "local model · llama-server · agentic",
  modelLabel: "Gemma 3 4B",
  modelId: "local:gemma",
  tier: "T2",
  localCtxTokens: 8192,
  now: 5000,
};

describe("HUP-S7.6 monitor snapshot", () => {
  it("classifies providers", () => {
    expect(providerClass("local")).toBe("local");
    expect(providerClass("sidecar")).toBe("local");
    expect(providerClass("real")).toBe("gateway");
    expect(providerClass("agent")).toBe("gateway");
    expect(providerClass("demo")).toBe("demo");
    expect(providerClass("something-new")).toBe("unknown");
  });

  it("spend is 0 for local and demo, unknown for the gateway (not metered in the app yet)", () => {
    expect(spendFor("local")).toMatchObject({ amount: 0 });
    expect(spendFor("sidecar")).toMatchObject({ amount: 0 });
    expect(spendFor("demo")).toMatchObject({ amount: 0 });
    expect(spendFor("agent").amount).toBeNull();
    expect(spendFor("agent").note).toMatch(/not metered/i);
    expect(spendFor("mystery").amount).toBeNull();
  });

  it("the context window is the local server's real --ctx-size; usage is unknown until the server reports it", () => {
    const local = contextFor("local", 8192);
    expect(local.windowTokens).toBe(8192);
    expect(local.usedTokens).toBeNull();
    expect(local.usedNote).toMatch(/has not reported token usage/i);
    expect(contextFor("sidecar", 8192).windowTokens).toBe(8192);
    expect(contextFor("local", null).windowTokens).toBeNull();
    expect(contextFor("agent", 8192).windowTokens).toBeNull();
    expect(contextFor("demo", 8192).windowTokens).toBeNull();
  });

  it("an idle snapshot carries the model, tier and no turn", () => {
    const s = buildMonitorSnapshot(base);
    expect(s.model).toEqual({ label: "Gemma 3 4B", id: "local:gemma" });
    expect(s.tier).toBe("T2");
    expect(s.turn.state).toBe("idle");
    expect(s.turn.startedAt).toBeNull();
    expect(s.at).toBe(5000);
  });

  it("a missing tier stays null (shown as unknown), never a default", () => {
    expect(buildMonitorSnapshot({ ...base, tier: null }).tier).toBeNull();
  });

  it("a running turn reports the provider it started on, not the current default", () => {
    const s = buildMonitorSnapshot({
      ...base,
      providerKind: "local",
      activity: { ...IDLE_ACTIVITY, state: "running", providerKind: "agent", providerLabel: "provider · x · agentic", phase: "thinking", startedAt: 1000 },
    });
    expect(s.provider).toEqual({ kind: "agent", class: "gateway", label: "provider · x · agentic" });
    expect(s.spend.amount).toBeNull();
    expect(s.turn.state).toBe("running");
  });

  it("explains why you are waiting", () => {
    expect(waitingReason({ ...IDLE_ACTIVITY })).toMatch(/idle/i);
    expect(waitingReason({ ...IDLE_ACTIVITY, state: "running", phase: "thinking" })).toMatch(/model/i);
    expect(waitingReason({ ...IDLE_ACTIVITY, state: "running", phase: "streaming" })).toMatch(/writing/i);
    expect(waitingReason({ ...IDLE_ACTIVITY, state: "running", phase: "tool", currentTool: "group_invite" })).toMatch(/group_invite/);
    expect(waitingReason({ ...IDLE_ACTIVITY, state: "stopping", phase: "tool" })).toMatch(/stopping/i);
  });

  it("formats elapsed time", () => {
    expect(formatElapsed(null, 10)).toBe("not started");
    expect(formatElapsed(0, 999)).toBe("0s");
    expect(formatElapsed(0, 61_000)).toBe("1m 01s");
    expect(formatElapsed(0, 3_725_000)).toBe("1h 02m 05s");
    expect(formatElapsed(5000, 1000)).toBe("0s");
  });
});

// HUP-S1.9 — the sidecar's worker processes (toolchain now, browser reserved) on the monitor.
import { isMonitorSnapshot, workerLine, type WorkerRow } from "./monitorSnapshot";

describe("HUP-S1.9 worker processes in the snapshot", () => {
  const toolchain: WorkerRow = {
    kind: "toolchain", state: "running", healthy: true, pid: 4242, restarts: 1,
    lastExit: "killed by signal 9", lastError: null, runningSinceMs: 1, detail: null,
  };
  const browser: WorkerRow = {
    kind: "browser", state: "not_built", healthy: null, pid: null, restarts: null,
    lastExit: null, lastError: null, runningSinceMs: null, detail: "the browser worker arrives with HUP-S5.1",
  };

  it("carries the rows as read and says each worker is its own process", () => {
    const s = buildMonitorSnapshot({ ...base, workers: [toolchain, browser] });
    expect(s.workers.rows).toEqual([toolchain, browser]);
    expect(s.workers.note).toMatch(/separate process/i);
    expect(isMonitorSnapshot(s)).toBe(true);
  });

  it("unread workers are unknown, an empty list means Hermes is not running", () => {
    const unread = buildMonitorSnapshot(base);
    expect(unread.workers.rows).toBeNull();
    expect(unread.workers.note).toMatch(/could not be read/i);
    expect(isMonitorSnapshot(unread)).toBe(true);
    const none = buildMonitorSnapshot({ ...base, workers: [] });
    expect(none.workers.rows).toEqual([]);
    expect(none.workers.note).toMatch(/not running/i);
  });

  it("a restart is stated with how the last process ended", () => {
    expect(workerLine(toolchain)).toBe("running, restarted 1 time (last exit: killed by signal 9)");
    expect(workerLine({ ...toolchain, restarts: 0, lastExit: null })).toBe("running");
    expect(workerLine({ ...toolchain, state: "failed", restarts: 5, lastError: "gave up after 5 restarts" })).toBe(
      "failed, restarted 5 times (last exit: killed by signal 9; gave up after 5 restarts)",
    );
    expect(workerLine(browser)).toBe("not built yet (the browser worker arrives with HUP-S5.1)");
    expect(workerLine({ ...toolchain, healthy: false })).toMatch(/not answering health checks/);
  });

  it("rejects a snapshot whose worker rows are malformed", () => {
    const s = buildMonitorSnapshot({ ...base, workers: [toolchain] });
    expect(isMonitorSnapshot({ ...s, workers: { rows: [{ kind: 1 }], note: "x" } })).toBe(false);
    expect(isMonitorSnapshot({ ...s, workers: undefined })).toBe(false);
  });
});
