// K2 (0.5.2): a Hermes restart is reported as done only once core confirms it. stopRuntime and
// startRuntime catch their own errors (they feed the Agent page), so restartHermesConfirmed checks
// the harness status after each step and returns a typed result naming the step that failed.
import { describe, it, expect, vi, afterEach, beforeEach } from "vitest";
import { bridge } from "../../bridge";
import { agentSlice, restartHermesConfirmed } from "./agent";

afterEach(() => vi.restoreAllMocks());
beforeEach(() => {
  agentSlice.set({ busy: false, error: null, attached: agentSlice.get().attached.map((r) => ({ ...r, state: "running", error: null })) });
});

const status = (running: boolean) => ({ running, skills: 0, pendingApprovals: 0 });

describe("restartHermesConfirmed", () => {
  it("stop fails: Hermes is still running, the result names the stop step and start is never called", async () => {
    vi.spyOn(bridge.agentHarness, "stop").mockRejectedValue(new Error("the sidecar did not exit"));
    vi.spyOn(bridge.agentHarness, "status").mockResolvedValue(status(true));
    const start = vi.spyOn(bridge.agentHarness, "start").mockResolvedValue(undefined);
    const r = await restartHermesConfirmed();
    expect(r).toEqual({ ok: false, stage: "stop", error: "the sidecar did not exit" });
    expect(start).not.toHaveBeenCalled();
  });

  it("stop fails without an error from core: still reported, never as done", async () => {
    vi.spyOn(bridge.agentHarness, "stop").mockResolvedValue(undefined);
    vi.spyOn(bridge.agentHarness, "status").mockResolvedValue(status(true));
    vi.spyOn(bridge.agentHarness, "skills").mockResolvedValue([]);
    vi.spyOn(bridge.agentHarness, "pendingApprovals").mockResolvedValue([]);
    vi.spyOn(bridge.agentHarness, "registrySkills").mockResolvedValue([]);
    const r = await restartHermesConfirmed();
    expect(r.ok).toBe(false);
    if (!r.ok) expect(r.stage).toBe("stop");
  });

  it("start fails: Hermes is left stopped, the result names the start step with core's error", async () => {
    let running = true;
    vi.spyOn(bridge.agentHarness, "stop").mockImplementation(async () => {
      running = false;
    });
    vi.spyOn(bridge.agentHarness, "status").mockImplementation(async () => status(running));
    vi.spyOn(bridge.agentHarness, "registrySkills").mockResolvedValue([]);
    vi.spyOn(bridge.agentHarness, "start").mockRejectedValue(new Error("llama-server is not running"));
    const r = await restartHermesConfirmed();
    expect(r).toEqual({ ok: false, stage: "start", error: "llama-server is not running" });
  });

  it("a status read that fails after stop is a failure, not a success", async () => {
    vi.spyOn(bridge.agentHarness, "stop").mockResolvedValue(undefined);
    vi.spyOn(bridge.agentHarness, "status").mockRejectedValue(new Error("status unavailable"));
    vi.spyOn(bridge.agentHarness, "registrySkills").mockResolvedValue([]);
    const start = vi.spyOn(bridge.agentHarness, "start").mockResolvedValue(undefined);
    const r = await restartHermesConfirmed();
    expect(r).toEqual({ ok: false, stage: "stop", error: "status unavailable" });
    expect(start).not.toHaveBeenCalled();
  });

  it("stop then start, confirmed running: ok", async () => {
    let running = true;
    vi.spyOn(bridge.agentHarness, "stop").mockImplementation(async () => {
      running = false;
    });
    vi.spyOn(bridge.agentHarness, "start").mockImplementation(async () => {
      running = true;
    });
    vi.spyOn(bridge.agentHarness, "status").mockImplementation(async () => status(running));
    vi.spyOn(bridge.agentHarness, "skills").mockResolvedValue([]);
    vi.spyOn(bridge.agentHarness, "pendingApprovals").mockResolvedValue([]);
    vi.spyOn(bridge.agentHarness, "registrySkills").mockResolvedValue([]);
    expect(await restartHermesConfirmed()).toEqual({ ok: true });
  });
});
