// HUP-S2.2 (US-2.2 AC3) — every command run is kept for the Activity monitor, capped, newest last,
// including runs reported after a stopped turn ended.
import { describe, it, expect, beforeEach } from "vitest";
import { turnActivity, beginTurn, endTurn, commandRan, IDLE_ACTIVITY, MAX_RUN_ROWS } from "./turnActivity";

const run = (callId: string) => ({ kind: "command_run" as const, callId, tool: "shell_run", status: "completed", summary: "ok", exitCode: 0, durationMs: 5, timedOut: false, sandbox: "macOS Seatbelt" });

describe("turn activity: command runs", () => {
  beforeEach(() => turnActivity.set({ ...IDLE_ACTIVITY }));

  it("records a run with its facts", () => {
    beginTurn("sidecar", "Hermes", 1);
    commandRan(run("c1"), 7);
    expect(turnActivity.get().runs).toEqual([{ callId: "c1", tool: "shell_run", status: "completed", summary: "ok", exitCode: 0, durationMs: 5, timedOut: false, sandbox: "macOS Seatbelt", at: 7 }]);
  });

  it("keeps a run reported while a stopped turn drains", () => {
    beginTurn("sidecar", "Hermes", 1);
    endTurn("stopped", 2);
    commandRan(run("late"), 3);
    expect(turnActivity.get().runs.map((r) => r.callId)).toEqual(["late"]);
  });

  it("caps the list and starts fresh with the next turn", () => {
    beginTurn("sidecar", "Hermes", 1);
    for (let i = 0; i < MAX_RUN_ROWS + 3; i++) commandRan(run("c" + i), i);
    const runs = turnActivity.get().runs;
    expect(runs.length).toBe(MAX_RUN_ROWS);
    expect(runs[runs.length - 1].callId).toBe("c" + (MAX_RUN_ROWS + 2));
    beginTurn("sidecar", "Hermes", 99);
    expect(turnActivity.get().runs).toEqual([]);
  });
});
