// #691: Steer now steers the running turn in place when Hermes can take it; otherwise (an older
// Hermes, or the run already finishing) it falls back to stop-then-send and says so.
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { store } from "./store";
import type { ChatProvider, SteerResult, SendOpts } from "../agent/harness";

let callbacks: SendOpts["callbacks"] | null = null;
let finish: () => void = () => undefined;
const sent: string[] = [];

function provider(steer: (t: string) => Promise<SteerResult>): ChatProvider {
  return {
    kind: "sidecar",
    label: "test",
    steer,
    send: ({ messages, callbacks: cb, signal }) => {
      sent.push(messages[messages.length - 1].content);
      callbacks = cb;
      cb.onStatus("thinking");
      if (sent.length > 1) {
        cb.onToken("next turn");
        return Promise.resolve({ role: "assistant", content: "" });
      }
      return new Promise((resolve, reject) => {
        finish = () => {
          cb.onToken("Weeknights only.");
          resolve({ role: "assistant", content: "" });
        };
        signal?.addEventListener("abort", () => reject(new Error("stopped by you")));
      });
    },
  } as unknown as ChatProvider;
}

beforeEach(() => {
  sent.length = 0;
  callbacks = null;
  store.setState({ chatMsgs: [], chatStatus: "ready", chatQueue: [], chatThreadId: "tsteer-" + Math.random().toString(36).slice(2) });
});
afterEach(() => {
  vi.restoreAllMocks();
  store.provider = null;
});

describe("#691 steering in place", () => {
  it("the steer joins the running turn: shown as your steer, taken when Hermes says so, nothing stopped", async () => {
    const steer = vi.fn(async () => ({ ok: true as const, waiting: 1 }));
    store.provider = provider(steer);
    const stop = vi.spyOn(store, "stopAgentTurn");
    const turn = store.sendChat("Put my GPU to work on rounds at night.");
    await vi.waitFor(() => expect(callbacks).not.toBeNull());
    await store.steerChat("also skip Fridays");
    expect(steer).toHaveBeenCalledWith("also skip Fridays");
    expect(stop).not.toHaveBeenCalled();
    const you = store.state.chatMsgs.filter((m) => m.who === "You");
    expect(you.map((m) => [m.text, m.steer])).toEqual([
      ["Put my GPU to work on rounds at night.", undefined],
      ["also skip Fridays", "sent"],
    ]);
    // The steer sits before the reply it steers.
    const order = store.state.chatMsgs.map((m) => m.who);
    expect(order).toEqual(["You", "You", "Agent"]);
    callbacks!.onActivity?.({ kind: "steered", step: 2, text: "also skip Fridays" });
    expect(store.state.chatMsgs[1].steer).toBe("taken");
    finish();
    await turn;
    const reply = store.state.chatMsgs.find((m) => m.who === "Agent")!;
    expect(reply.run?.steps.some((s) => s.kind === "steer" && s.detail === "also skip Fridays")).toBe(true);
    expect(sent).toEqual(["Put my GPU to work on rounds at night."]);
    expect(store.state.chatQueue).toEqual([]);
  });

  it("a steer accepted but not taken before the run ended goes next as a new message", async () => {
    store.provider = provider(async () => ({ ok: true, waiting: 1 }));
    const turn = store.sendChat("Network status");
    await vi.waitFor(() => expect(callbacks).not.toBeNull());
    await store.steerChat("and peers");
    finish();
    await turn;
    await vi.waitFor(() => expect(sent).toEqual(["Network status", "and peers"]));
    // Never shown as taken: the waiting steer left, and the message went as an ordinary one.
    expect(store.state.chatMsgs.filter((m) => m.steer)).toEqual([]);
  });

  it("when Hermes cannot take it, the run stops and the message goes next, with a toast", async () => {
    store.provider = provider(async () => ({ ok: false, reason: "the bundled Hermes is too old to take messages mid-run" }));
    const toast = vi.spyOn(store, "toast");
    const turn = store.sendChat("Network status");
    await vi.waitFor(() => expect(callbacks).not.toBeNull());
    await store.steerChat("and peers");
    await turn;
    expect(toast.mock.calls.some((c) => String(c[0]).includes("Hermes could not take your message mid-run (the bundled Hermes is too old"))).toBe(true);
    await vi.waitFor(() => expect(sent).toEqual(["Network status", "and peers"]));
  });

  it("race 1: Hermes takes the steer before the POST returns: it is shown taken and never sent again", async () => {
    let resolveSteer: (r: SteerResult) => void = () => undefined;
    store.provider = provider(() => new Promise<SteerResult>((r) => (resolveSteer = r)));
    const turn = store.sendChat("Network status");
    await vi.waitFor(() => expect(callbacks).not.toBeNull());
    const steering = store.steerChat("and peers");
    // The sidecar takes it at the next step boundary; the event poll delivers that first.
    callbacks!.onActivity?.({ kind: "steered", step: 2, text: "and peers" });
    resolveSteer({ ok: true, waiting: 0 });
    await steering;
    expect(store.state.chatMsgs.filter((m) => m.who === "You").map((m) => [m.text, m.steer])).toEqual([
      ["Network status", undefined],
      ["and peers", "taken"],
    ]);
    finish();
    await turn;
    await vi.waitFor(() => expect(store.state.chatStatus).toBe("ready"));
    expect(sent).toEqual(["Network status"]);
    expect(store.state.chatQueue).toEqual([]);
  });

  it("race 2: the conversation changes while the POST is pending: nothing moves to it, and its run is not stopped", async () => {
    let resolveSteer: (r: SteerResult) => void = () => undefined;
    store.provider = provider(() => new Promise<SteerResult>((r) => (resolveSteer = r)));
    vi.spyOn(store, "rebuildProvider").mockResolvedValue();
    const orig = store.state.chatThreadId;
    const turn = store.sendChat("Network status");
    await vi.waitFor(() => expect(callbacks).not.toBeNull());
    const steering = store.steerChat("private text for the first chat");
    // The run ends and the member moves to another conversation before the POST answers.
    finish();
    await turn;
    expect(store.newChat()).toBe(true);
    const other = store.state.chatThreadId;
    const stop = vi.spyOn(store, "stopAgentTurn");
    resolveSteer({ ok: false, reason: "the run is finishing", code: 409 } as unknown as SteerResult);
    await steering;
    expect(stop).not.toHaveBeenCalled();
    // Nothing of the first chat's text is queued in, shown in, or sent to the second.
    expect(store.state.chatQueue.filter((q) => q.threadId === other)).toEqual([]);
    expect(store.state.chatMsgs.some((m) => m.text.includes("private text"))).toBe(false);
    expect(sent).toEqual(["Network status"]);
    expect(store.state.chatQueue.filter((q) => q.threadId === orig).map((q) => q.text)).toEqual(["private text for the first chat"]);
  });

  it("race 2b: accepted by the sidecar for a run that has since ended: not queued again", async () => {
    let resolveSteer: (r: SteerResult) => void = () => undefined;
    store.provider = provider(() => new Promise<SteerResult>((r) => (resolveSteer = r)));
    const turn = store.sendChat("Network status");
    await vi.waitFor(() => expect(callbacks).not.toBeNull());
    const steering = store.steerChat("and peers");
    finish();
    await turn;
    resolveSteer({ ok: true, waiting: 1 });
    await steering;
    await vi.waitFor(() => expect(store.state.chatStatus).toBe("ready"));
    // The run ended without taking it: it goes next exactly once, in its own conversation.
    await vi.waitFor(() => expect(sent).toEqual(["Network status", "and peers"]));
    expect(store.state.chatQueue).toEqual([]);
  });
});
