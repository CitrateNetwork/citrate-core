// FL design board 01, "Honest failure": each failure has its own sentence and a fix.
import { describe, it, expect } from "vitest";
import { failureCopy, hermesDownCopy } from "./chatFailure";

const BANNED = /\b(earn|earning|earnings|income|yield|HITL)\b|!|—/i;

describe("failureCopy", () => {
  it("names each case with its own sentence and fix", () => {
    const stopped = failureCopy("stopped by you");
    expect(stopped.kind).toBe("stopped");
    expect(stopped.actions).toEqual(["retry"]);
    expect(failureCopy("model request failed: error sending request for url (http://127.0.0.1:8080)").kind).toBe("model_down");
    expect(failureCopy("connection refused").actions).toEqual(["pick_model", "retry"]);
    expect(failureCopy("the agent session stopped responding").kind).toBe("hermes_lost");
    expect(failureCopy("the agent session stopped responding").actions).toContain("open_controls");
    expect(failureCopy("the local model is not running; nothing was sent to the gateway").kind).toBe("local_not_running");
    expect(failureCopy("not sent; you cancelled before choosing where it goes").kind).toBe("not_sent");
    const other = failureCopy("turn ended: max_steps");
    expect(other.kind).toBe("other");
    expect(other.sentence).toContain("turn ended: max_steps");
    expect(failureCopy("").sentence).toContain("no reason was reported");
  });

  it("keeps the copy rules: no banned words, no exclamation marks, no em dashes", () => {
    for (const e of ["stopped by you", "connection refused", "the agent session stopped responding", "local model is not running", "not sent", "x"]) {
      const c = failureCopy(e);
      expect(c.sentence + " " + c.fix).not.toMatch(BANNED);
    }
  });
});

describe("hermesDownCopy", () => {
  it("not running offers Start Hermes; loop off and unreadable point to Controls", () => {
    expect(hermesDownCopy({ why: "stopped" })).toMatchObject({ title: "Hermes is not running", action: "start_hermes" });
    expect(hermesDownCopy({ why: "loop_off" }).action).toBe("open_settings");
    const u = hermesDownCopy({ why: "unreadable", error: "ipc timeout" });
    expect(u.action).toBe("open_controls");
    expect(u.sentence).toContain("ipc timeout");
    for (const r of [{ why: "stopped" as const }, { why: "loop_off" as const }]) {
      const c = hermesDownCopy(r);
      expect(c.title + c.sentence).not.toMatch(BANNED);
    }
  });
});
