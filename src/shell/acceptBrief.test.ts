// HUP-S1.4 — an accepted brief lands in the chat thread as a card and in persisted state; nothing
// is started from it.
import { describe, it, expect, beforeEach } from "vitest";
import { store } from "./store";
import { PERSIST_KEYS, freshState } from "./state";
import type { Brief } from "../bridge/domains";

const BRIEF: Brief = {
  track: "code",
  goal: "fix the parser",
  constraints: [{ id: "lang", ask: "Which language?", answer: "rust", from_default: true }],
  persona: "Builder",
  skills: ["repo-read"],
  workflow: "code-change",
  workflow_available: false,
  ships_in: "0.5.0",
  gates: ["tests pass"],
};

describe("store.acceptBrief", () => {
  beforeEach(() => {
    store.setState({ ...freshState("p1"), chatMsgs: [], chatStatus: "ready" });
  });

  it("adds the brief to the thread as a card and keeps it in persisted state", () => {
    store.acceptBrief(BRIEF, "# Brief\n\n**Goal:** fix the parser");
    const msgs = store.state.chatMsgs;
    expect(msgs).toHaveLength(1);
    expect(msgs[0].brief).toEqual(BRIEF);
    expect(msgs[0].text).toContain("fix the parser");
    expect(msgs[0].chips.map((c) => c.label).join(" ")).toMatch(/nothing (is )?built/i);
    expect(store.state.hermesBrief?.brief).toEqual(BRIEF);
    expect(PERSIST_KEYS).toContain("hermesBrief");
  });

  it("does not start a turn or a build", () => {
    store.acceptBrief(BRIEF, "# Brief");
    expect(store.state.chatStatus).toBe("ready");
    expect(store.state.chatMsgs.some((m) => m.who === "You")).toBe(false);
  });

  it("a fresh state has no brief", () => {
    expect(freshState("p1").hermesBrief).toBeNull();
  });
});
