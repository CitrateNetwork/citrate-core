// L1-WP01 CH-3 (characterization, citrate-core a826e441). LQ registry: surface "Hermes start and
// sessions".
//
// Pins `startRuntime` (src/shell/slices/agent.ts) today: a start while one is in flight or
// Hermes already runs is a no-op with a plain reason, core's "already running" refusal counts as
// success, any other start error is reported once and polling stops, the start polls status until
// Hermes reports running, a start that never comes up ends "off" and retryable, and an unwired
// adapter or a runtime that is not attached never reaches core. WP08 (auto-start at launch)
// reuses this path and names CH-3.
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { bridge } from "../../../../bridge";
import { agentSlice, startRuntime, RUNTIME_OPTIONS } from "../../../../shell/slices/agent";
import { hermesRunningSlice } from "../../../../shell/slices/hermesRunning";
import type { FlTrajectoryStatus } from "../../../../bridge/domains";

const traj: FlTrajectoryStatus = { settings: { enabled: false, changedAtMs: null }, recordedFiles: 0, datasets: 0, appliesOnRestart: true, loadError: null };

function hermesRow() {
  return agentSlice.get().attached.find((r) => r.id === "hermes");
}

function rig(opts: { start?: () => Promise<void>; running?: () => boolean } = {}) {
  vi.spyOn(bridge.flRounds, "trajectoryStatus").mockResolvedValue(traj);
  vi.spyOn(bridge.agentHarness, "skills").mockResolvedValue([]);
  vi.spyOn(bridge.agentHarness, "pendingApprovals").mockResolvedValue([]);
  vi.spyOn(bridge.agentHarness, "registrySkills").mockResolvedValue([]);
  const start = vi.spyOn(bridge.agentHarness, "start").mockImplementation(opts.start ?? (async () => {}));
  const running = opts.running ?? (() => true);
  const status = vi.spyOn(bridge.agentHarness, "status").mockImplementation(async () => ({ running: running(), skills: 0, pendingApprovals: 0 }));
  return { start, status };
}

beforeEach(() => {
  hermesRunningSlice.set({ webSearch: null, learn: null, starting: 0 });
  agentSlice.set({
    busy: false,
    error: null,
    attached: agentSlice.get().attached.filter((r) => r.id === "hermes").map((r) => ({ ...r, state: "off", error: null })),
  });
});
afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});

describe("CH-3: startRuntime", () => {
  it("a start that comes up resolves null and leaves Hermes running", async () => {
    const { start } = rig();
    expect(await startRuntime("hermes")).toBeNull();
    expect(start).toHaveBeenCalledTimes(1);
    expect(hermesRow()?.state).toBe("running");
    expect(agentSlice.get().busy).toBe(false);
    expect(hermesRunningSlice.get().starting).toBe(0);
  });

  it("core's 'already running' refusal is success, not an error", async () => {
    const { start } = rig({
      start: async () => {
        throw new Error("Hermes is already running");
      },
    });
    expect(await startRuntime("hermes")).toBeNull();
    expect(start).toHaveBeenCalledTimes(1);
    expect(hermesRow()?.state).toBe("running");
    expect(hermesRow()?.error).toBeNull();
  });

  it("any other start error is reported once and status is never polled", async () => {
    const { start, status } = rig({
      start: async () => {
        throw new Error("binary not found");
      },
    });
    expect(await startRuntime("hermes")).toBe("binary not found");
    expect(start).toHaveBeenCalledTimes(1);
    expect(status).not.toHaveBeenCalled();
    expect(hermesRow()?.state).toBe("error");
    expect(hermesRow()?.error).toBe("binary not found");
    expect(agentSlice.get().busy).toBe(false);
  });

  it("a second start while one is in flight is a no-op with a reason", async () => {
    let release: () => void = () => {};
    const gate = new Promise<void>((r) => (release = r));
    const { start } = rig({ start: () => gate });
    const first = startRuntime("hermes");
    expect(await startRuntime("hermes")).toMatch(/already has Hermes starting or running/);
    release();
    expect(await first).toBeNull();
    expect(start).toHaveBeenCalledTimes(1);
  });

  it("a start while Hermes already runs is a no-op with a reason", async () => {
    const { start } = rig();
    agentSlice.set((s) => ({ attached: s.attached.map((r) => (r.id === "hermes" ? { ...r, state: "running" } : r)) }));
    expect(await startRuntime("hermes")).toMatch(/already has Hermes starting or running/);
    expect(start).not.toHaveBeenCalled();
  });

  it("polls until Hermes reports running", async () => {
    let reads = 0;
    vi.useFakeTimers();
    const { status } = rig({ running: () => ++reads >= 3 });
    const p = startRuntime("hermes");
    await vi.advanceTimersByTimeAsync(800 * 3);
    expect(await p).toBeNull();
    expect(status.mock.calls.length).toBeGreaterThanOrEqual(3);
    expect(hermesRow()?.state).toBe("running");
  });

  it("a start that never reports running ends off and retryable after 15 polls", async () => {
    vi.useFakeTimers();
    const { status } = rig({ running: () => false });
    const p = startRuntime("hermes");
    await vi.advanceTimersByTimeAsync(800 * 16);
    expect(await p).toBe("Hermes did not report running");
    expect(status).toHaveBeenCalledTimes(15);
    expect(hermesRow()?.state).toBe("off");
    expect(hermesRow()?.error).toMatch(/tap Start to retry/);
    expect(agentSlice.get().busy).toBe(false);
  });

  it("an unwired adapter or a runtime that is not attached never reaches core", async () => {
    const { start } = rig();
    expect(await startRuntime("grok")).toBe("no grok runtime is attached");
    const openclaw = RUNTIME_OPTIONS.find((o) => o.id === "openclaw");
    expect(openclaw?.wired).toBe(false);
    if (!openclaw) return;
    agentSlice.set((s) => ({ attached: [...s.attached, { ...openclaw, state: "off", error: null }] }));
    expect(await startRuntime("openclaw")).toMatch(/isn't available in this build yet/);
    expect(agentSlice.get().attached.find((r) => r.id === "openclaw")?.state).toBe("error");
    expect(start).not.toHaveBeenCalled();
  });
});
