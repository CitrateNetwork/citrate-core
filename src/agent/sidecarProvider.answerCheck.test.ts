// #681: when the sidecar's final-answer check corrects an answer it already streamed, the corrected
// final replaces the streamed text (it is never shown beside it), and the chat says so.
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { createSidecarProvider, type SidecarSessionApi } from "./sidecarProvider";
import type { ChatProvider, TurnActivityEvent } from "./harness";
import { store } from "../shell/store";
import { DEPLOY_CLAIM_CHIP } from "./deployClaims";

const FAB = "Your contract is deployed at 0x52908400098527886E0F7030069857D2E4169EE7.";
const FIXED = "No contract is deployed yet. The deploy is waiting for your approval in Citrate Core.";

function api(): SidecarSessionApi {
  const page = {
    events: [
      { seq: 1, event: { type: "step_start", step: 1 } },
      { seq: 2, event: { type: "assistant_delta", step: 1, text: FAB } },
      { seq: 3, event: { type: "answer_check", findings: ["removed a deploy claim at 0x5290…"] } },
      { seq: 4, event: { type: "final", content: FIXED } },
      { seq: 5, event: { type: "done", outcome: "answered" } },
    ],
    lastSeq: 5,
    busy: false,
  };
  return { open: vi.fn(async () => "s1"), send: vi.fn(async () => undefined), events: vi.fn(async () => page), toolResult: vi.fn(async () => undefined), stop: vi.fn(async () => undefined) };
}

describe("#681 the sidecar's corrected answer replaces what it streamed", () => {
  it("the provider rewrites the streamed text and reports the check", async () => {
    const tokens: string[] = [];
    const rewrites: [string, string][] = [];
    const acts: TurnActivityEvent[] = [];
    const p = createSidecarProvider(api(), () => "p", () => []);
    const r = await p.send({
      messages: [{ role: "user", content: "deploy" }],
      callbacks: { onStatus: vi.fn(), onToken: (t) => tokens.push(t), onToolCall: vi.fn(async () => ""), onActivity: (e) => acts.push(e), onRewrite: (a, b) => rewrites.push([a, b]) },
    });
    expect(tokens.join("")).toBe(FAB);
    expect(rewrites).toEqual([[FAB, FIXED]]);
    expect(acts).toContainEqual({ kind: "answer_check", findings: ["removed a deploy claim at 0x5290…"] });
    expect(r.content).toBe(FIXED);
  });

  it("without onRewrite the corrected answer is appended (an older caller), never lost", async () => {
    const tokens: string[] = [];
    const p = createSidecarProvider(api(), () => "p", () => []);
    await p.send({ messages: [{ role: "user", content: "deploy" }], callbacks: { onStatus: vi.fn(), onToken: (t) => tokens.push(t), onToolCall: vi.fn(async () => "") } });
    expect(tokens.join("")).toBe(FAB + "\n\n" + FIXED);
  });

  describe("in the chat", () => {
    beforeEach(() => store.setState({ chatMsgs: [], chatStatus: "ready", chatThreadId: "t681-" + Math.random().toString(36).slice(2) }));
    afterEach(() => {
      store.provider = null;
      vi.restoreAllMocks();
    });
    it("the reply shows only the corrected answer, with the answer-check chip, and core's check keeps it", async () => {
      store.provider = { ...createSidecarProvider(api(), () => "p", () => []), kind: "sidecar" } as ChatProvider;
      await store.sendChat("deploy my token");
      const m = store.state.chatMsgs.find((x) => x.who !== "You")!;
      expect(m.text).toBe(FIXED);
      expect(m.text).not.toContain("0x5290");
      expect(m.chips.filter((c) => c.label === DEPLOY_CLAIM_CHIP)).toHaveLength(1);
    });
  });
});
