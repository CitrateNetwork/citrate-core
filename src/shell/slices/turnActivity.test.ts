// HUP-S7.6 — the live record of the current agent turn that the Activity monitor shows. Every
// value is recorded from the real send path (store.sendChat callbacks); nothing is estimated.
import { describe, it, expect, beforeEach } from "vitest";
import { turnActivity, beginTurn, notePhase, noteStep, toolStarted, toolFinished, markStopping, endTurn, IDLE_ACTIVITY, MAX_TOOL_ROWS } from "./turnActivity";

beforeEach(() => turnActivity.set(IDLE_ACTIVITY));

describe("HUP-S7.6 turn activity", () => {
  it("starts idle with nothing invented", () => {
    const a = turnActivity.get();
    expect(a.state).toBe("idle");
    expect(a.startedAt).toBeNull();
    expect(a.step).toBeNull();
    expect(a.tools).toEqual([]);
  });

  it("records a turn from begin to answered", () => {
    beginTurn("local", "local model", 1000);
    notePhase("thinking");
    noteStep(1);
    toolStarted("c1", "node_status", 1200);
    expect(turnActivity.get().currentTool).toBe("node_status");
    toolFinished("c1", true, 1500);
    notePhase("streaming");
    endTurn("answered", 2000);
    const a = turnActivity.get();
    expect(a).toMatchObject({ state: "idle", providerKind: "local", startedAt: 1000, endedAt: 2000, step: 1, outcome: "answered", currentTool: null, phase: null });
    expect(a.tools).toEqual([{ id: "c1", name: "node_status", state: "done", startedAt: 1200, endedAt: 1500 }]);
  });

  it("a new turn clears the previous turn's tools and steps", () => {
    beginTurn("local", "l", 1);
    noteStep(3);
    toolStarted("c1", "x", 2);
    endTurn("answered", 3);
    beginTurn("local", "l", 10);
    expect(turnActivity.get()).toMatchObject({ state: "running", step: null, tools: [], outcome: null, startedAt: 10, endedAt: null });
  });

  it("stopping marks running tools abandoned at the end, and the outcome is stopped", () => {
    beginTurn("sidecar", "s", 0);
    toolStarted("c1", "group_invite", 5);
    markStopping();
    expect(turnActivity.get().state).toBe("stopping");
    endTurn("stopped", 9);
    const a = turnActivity.get();
    expect(a.outcome).toBe("stopped");
    expect(a.tools[0]).toMatchObject({ state: "abandoned", endedAt: 9 });
  });

  it("a failed tool is recorded as failed", () => {
    beginTurn("local", "l", 0);
    toolStarted("c1", "x", 1);
    toolFinished("c1", false, 2);
    expect(turnActivity.get().tools[0].state).toBe("failed");
  });

  it("ignores events when no turn is running (late callbacks of a stopped turn)", () => {
    noteStep(4);
    toolStarted("c1", "x", 1);
    notePhase("tool");
    expect(turnActivity.get()).toEqual(IDLE_ACTIVITY);
  });

  it("keeps only the most recent tool rows", () => {
    beginTurn("local", "l", 0);
    for (let i = 0; i < MAX_TOOL_ROWS + 5; i++) toolStarted("c" + i, "t", i);
    const tools = turnActivity.get().tools;
    expect(tools.length).toBe(MAX_TOOL_ROWS);
    expect(tools[tools.length - 1].id).toBe("c" + (MAX_TOOL_ROWS + 4));
  });

  it("steps only move forward", () => {
    beginTurn("local", "l", 0);
    noteStep(2);
    noteStep(1);
    expect(turnActivity.get().step).toBe(2);
  });
});
