// HUP-S0.4 / S0.7 — agent text integrity and failed turns.
//  - S0.4: streamed tokens keep their markdown (store.ts used to strip every `**`).
//  - S0.7: a failed or timed-out turn shows inline with Retry; it used to vanish (console only).
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { store } from "./store";
import type { ChatProvider } from "../agent/harness";

const fakeProvider = (send: ChatProvider["send"]): ChatProvider =>
  ({ kind: "local", label: "test", send }) as unknown as ChatProvider;

beforeEach(() => {
  store.setState({ chatMsgs: [], chatStatus: "ready" });
});
afterEach(() => {
  vi.restoreAllMocks();
});

describe("HUP-S0.4 streamed markdown is preserved", () => {
  it("keeps ** in streamed tokens", async () => {
    store.provider = fakeProvider(async ({ callbacks }) => {
      callbacks.onStatus?.("streaming");
      callbacks.onToken?.("**Staking** locks ");
      callbacks.onToken?.("SALT");
      callbacks.onStatus?.("done");
    });
    await store.sendChat("what is staking?");
    const asst = store.state.chatMsgs.find((m) => m.who !== "You");
    expect(asst?.text).toBe("**Staking** locks SALT");
  });
});

describe("HUP-S0.7 failed turns are visible and retryable", () => {
  it("a thrown turn leaves an inline error with the text to retry", async () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    store.provider = fakeProvider(async () => {
      throw new Error('command "ai_chat_local_tools" timed out after 330000ms');
    });
    await store.sendChat("deploy my token");
    const asst = store.state.chatMsgs.find((m) => m.who !== "You");
    expect(asst, "the failed turn must not vanish").toBeTruthy();
    expect(asst?.error).toMatch(/timed out/);
    expect(asst?.retryText).toBe("deploy my token");
    expect(asst?.streaming).toBe(false);
    expect(store.state.chatStatus).toBe("ready");
  });

  it("an errored turn is never sent back to the model as context", async () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    store.provider = fakeProvider(async () => {
      throw new Error("boom");
    });
    await store.sendChat("first");
    const seen: string[][] = [];
    store.provider = fakeProvider(async ({ messages }) => {
      seen.push(messages.map((m) => m.content));
    });
    await store.sendChat("second");
    expect(seen[0]).not.toContain("");
  });

  it("retryChat replaces the failed pair and sends the same text again", async () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    store.provider = fakeProvider(async () => {
      throw new Error("boom");
    });
    await store.sendChat("hello");
    const failed = store.state.chatMsgs.find((m) => m.error)!;
    const send = vi.fn(async ({ callbacks }: Parameters<ChatProvider["send"]>[0]) => {
      callbacks.onToken?.("hi");
    });
    store.provider = fakeProvider(send as unknown as ChatProvider["send"]);
    await store.retryChat(failed.id);
    expect(send).toHaveBeenCalledTimes(1);
    const texts = store.state.chatMsgs.map((m) => `${m.who}:${m.text}`);
    expect(texts).toEqual(["You:hello", "Agent:hi"]);
    expect(store.state.chatMsgs.some((m) => m.error)).toBe(false);
  });
});
