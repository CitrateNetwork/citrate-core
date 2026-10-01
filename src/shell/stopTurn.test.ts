// HUP-S7.6 — the Stop path the Activity monitor drives. Stop returns the chat to ready at once,
// runs no further tool calls for the stopped turn, ends a sidecar turn through the session's own
// stop route, and stops the in-app loop before its next model request or tool call.
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { store } from "./store";
import { turnActivity, IDLE_ACTIVITY } from "./slices/turnActivity";
import { createAgentProvider, TurnStopped, type ChatProvider, type SendOpts } from "../agent/harness";
import { createSidecarProvider, type SidecarSessionApi } from "../agent/sidecarProvider";

const fakeProvider = (send: ChatProvider["send"], kind = "local"): ChatProvider =>
  ({ kind, label: "test", send }) as unknown as ChatProvider;
const flush = () => new Promise((r) => setTimeout(r, 0));

beforeEach(() => {
  store.setState({ chatMsgs: [], chatStatus: "ready" });
  turnActivity.set(IDLE_ACTIVITY);
});
afterEach(() => {
  vi.restoreAllMocks();
});

describe("HUP-S7.6 store.stopAgentTurn", () => {
  it("is a no-op when nothing is running", () => {
    expect(() => store.stopAgentTurn()).not.toThrow();
    expect(store.state.chatStatus).toBe("ready");
  });

  it("a hung turn ends at once on Stop: chat ready, reply marked stopped, activity stopped", async () => {
    let seen: AbortSignal | undefined;
    store.provider = fakeProvider(({ signal, callbacks }) => {
      seen = signal;
      callbacks.onStatus("thinking");
      return new Promise(() => {}); // never resolves (a wedged model request)
    });
    const turn = store.sendChat("hello");
    await flush();
    expect(store.state.chatStatus).toBe("thinking");
    expect(turnActivity.get().state).toBe("running");
    store.stopAgentTurn();
    await turn;
    expect(seen?.aborted).toBe(true);
    expect(store.state.chatStatus).toBe("ready");
    const asst = store.state.chatMsgs.find((m) => m.who !== "You");
    expect(asst?.error).toMatch(/stopped/i);
    expect(asst?.streaming).toBe(false);
    expect(turnActivity.get()).toMatchObject({ state: "idle", outcome: "stopped" });
  });

  it("after Stop, a late tool call from the stopped turn is not run", async () => {
    let late: ((r: string) => void) | null = null;
    let lateCall: Promise<string> | null = null;
    store.provider = fakeProvider(({ callbacks }) => {
      callbacks.onStatus("thinking");
      return new Promise((res) => {
        late = () => {
          lateCall = callbacks.onToolCall({ id: "c1", name: "group_invite", arguments: "{}" });
          res({ role: "assistant", content: "" });
        };
      });
    });
    const spy = vi.spyOn(store, "handleTool");
    const turn = store.sendChat("invite");
    await flush();
    store.stopAgentTurn();
    await turn;
    (late as unknown as () => void)();
    const result = await (lateCall as unknown as Promise<string>);
    expect(result).toMatch(/stopped/i);
    expect(spy).not.toHaveBeenCalled();
  });

  it("a normal turn records the steps and tools it really ran", async () => {
    store.provider = fakeProvider(async ({ callbacks }) => {
      callbacks.onStatus("thinking");
      callbacks.onActivity?.({ kind: "step", step: 1 });
      callbacks.onStatus("tool");
      await callbacks.onToolCall({ id: "c1", name: "navigate", arguments: '{"route":"wallet"}' });
      callbacks.onStatus("streaming");
      callbacks.onToken("done");
      callbacks.onStatus("done");
      return { role: "assistant", content: "done" };
    });
    await store.sendChat("go to wallet");
    const a = turnActivity.get();
    expect(a.outcome).toBe("answered");
    expect(a.step).toBe(1);
    expect(a.tools.map((t) => [t.name, t.state])).toEqual([["navigate", "done"]]);
  });

  it("a failed turn is recorded as failed", async () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    store.provider = fakeProvider(async () => {
      throw new Error("timeout");
    });
    await store.sendChat("x");
    expect(turnActivity.get().outcome).toBe("failed");
  });
});

describe("HUP-S7.6 in-app loop honours Stop", () => {
  it("stops before the next model request once aborted", async () => {
    const ac = new AbortController();
    const infer = vi.fn(async () => {
      ac.abort(); // the member presses Stop while the first request is in flight
      return JSON.stringify({ role: "assistant", content: null, tool_calls: [{ id: "c1", function: { name: "node_status", arguments: "{}" } }] });
    });
    const p = createAgentProvider("x", () => ({}) as never, infer);
    const onToolCall = vi.fn(async () => "ok");
    const opts: SendOpts = { messages: [{ role: "user", content: "hi" }], signal: ac.signal, callbacks: { onStatus: vi.fn(), onToken: vi.fn(), onToolCall } };
    await expect(p.send(opts)).rejects.toBeInstanceOf(TurnStopped);
    expect(infer).toHaveBeenCalledTimes(1);
    expect(onToolCall).not.toHaveBeenCalled();
  });

  it("a Stop that lands as a tool is about to run keeps it from running", async () => {
    const ac = new AbortController();
    const p = createAgentProvider("x", () => ({}) as never, async () =>
      JSON.stringify({ role: "assistant", content: null, tool_calls: [{ id: "c1", function: { name: "group_invite", arguments: "{}" } }] }),
    );
    const onToolCall = vi.fn(async () => "ran");
    const onStatus = vi.fn((st: string) => { if (st === "tool") ac.abort(); });
    await expect(p.send({ messages: [{ role: "user", content: "hi" }], signal: ac.signal, callbacks: { onStatus, onToken: vi.fn(), onToolCall } })).rejects.toBeInstanceOf(TurnStopped);
    expect(onToolCall).not.toHaveBeenCalled();
  });

  it("a turn already stopped makes no model request", async () => {
    const ac = new AbortController();
    ac.abort();
    const infer = vi.fn(async () => JSON.stringify({ role: "assistant", content: "x" }));
    const p = createAgentProvider("x", () => ({}) as never, infer);
    await expect(p.send({ messages: [{ role: "user", content: "hi" }], signal: ac.signal, callbacks: { onStatus: vi.fn(), onToken: vi.fn(), onToolCall: vi.fn(async () => "r") } })).rejects.toBeInstanceOf(TurnStopped);
    expect(infer).not.toHaveBeenCalled();
  });

  it("stops revealing the answer once stopped", async () => {
    const ac = new AbortController();
    const p = createAgentProvider("x", () => ({}) as never, async () => JSON.stringify({ role: "assistant", content: "one two three four five six" }));
    const onToken = vi.fn(() => ac.abort());
    await expect(p.send({ messages: [{ role: "user", content: "hi" }], signal: ac.signal, callbacks: { onStatus: vi.fn(), onToken, onToolCall: vi.fn(async () => "r") } })).rejects.toBeInstanceOf(TurnStopped);
    expect(onToken).toHaveBeenCalledTimes(1);
  });

  it("reports each model round as a step", async () => {
    const replies = [
      { role: "assistant", content: null, tool_calls: [{ id: "c1", function: { name: "node_status", arguments: "{}" } }] },
      { role: "assistant", content: "ok" },
    ];
    const p = createAgentProvider("x", () => ({}) as never, async () => JSON.stringify(replies.shift()));
    const onActivity = vi.fn();
    await p.send({ messages: [{ role: "user", content: "hi" }], callbacks: { onStatus: vi.fn(), onToken: vi.fn(), onToolCall: vi.fn(async () => "r"), onActivity } });
    expect(onActivity.mock.calls.map((c) => c[0])).toEqual([{ kind: "step", step: 1 }, { kind: "step", step: 2 }]);
  });
});

describe("HUP-S7.6 sidecar loop honours Stop", () => {
  function api(pages: { events: { seq: number; event: Record<string, unknown> }[]; lastSeq: number; busy: boolean }[], onEvents?: (n: number) => void) {
    const q = pages.slice();
    let n = 0;
    const calls: string[] = [];
    const a: SidecarSessionApi = {
      open: vi.fn(async () => "s1"),
      send: vi.fn(async (_id: string, t: string) => { calls.push("send:" + t); }),
      events: vi.fn(async (_id: string, after: number) => { onEvents?.(++n); calls.push("events:" + after); return q.shift() ?? { events: [], lastSeq: after, busy: false }; }),
      toolResult: vi.fn(async (_i: string, c: string) => { calls.push("result:" + c); }),
      stop: vi.fn(async () => { calls.push("stop"); }),
    };
    return { a, calls };
  }

  it("Stop calls the session's stop route, drains to done, and runs no tool call", async () => {
    const ac = new AbortController();
    const { a, calls } = api(
      [
        { events: [{ seq: 1, event: { type: "step_start", step: 1 } }], lastSeq: 1, busy: true },
        { events: [{ seq: 2, event: { type: "tool_call", step: 1, host: "core", call: { id: "c1", name: "group_invite", arguments: "{}" } } }, { seq: 3, event: { type: "done", outcome: "stopped" } }], lastSeq: 3, busy: false },
      ],
      (n) => { if (n === 1) ac.abort(); },
    );
    const onToolCall = vi.fn(async () => "ran");
    const p = createSidecarProvider(a, () => "p", () => []);
    await expect(p.send({ messages: [{ role: "user", content: "hi" }], signal: ac.signal, callbacks: { onStatus: vi.fn(), onToken: vi.fn(), onToolCall } })).rejects.toBeInstanceOf(TurnStopped);
    expect(a.stop).toHaveBeenCalledWith("s1");
    expect(onToolCall).not.toHaveBeenCalled();
    expect(calls).not.toContain("result:c1");
  });

  it("the next turn starts after the stopped one drained, in a fresh session", async () => {
    const ac = new AbortController();
    const { a, calls } = api(
      [
        { events: [{ seq: 1, event: { type: "step_start", step: 1 } }], lastSeq: 1, busy: true },
        { events: [{ seq: 2, event: { type: "done", outcome: "stopped" } }], lastSeq: 2, busy: false },
        { events: [{ seq: 3, event: { type: "final", content: "fresh" } }, { seq: 4, event: { type: "done", outcome: "answered" } }], lastSeq: 4, busy: false },
      ],
      (n) => { if (n === 1) ac.abort(); },
    );
    const p = createSidecarProvider(a, () => "p", () => []);
    const cbs = () => ({ onStatus: vi.fn(), onToken: vi.fn(), onToolCall: vi.fn(async () => "r") });
    const first = p.send({ messages: [{ role: "user", content: "one" }], signal: ac.signal, callbacks: cbs() });
    const second = p.send({ messages: [{ role: "user", content: "two" }], callbacks: cbs() });
    await expect(first).rejects.toBeInstanceOf(TurnStopped);
    const out = await second;
    expect(out.content).toBe("fresh");
    expect(calls.indexOf("send:two")).toBeGreaterThan(calls.indexOf("stop"));
    // The stopped turn drained to its `done` (events after seq 1) before the next turn began, and
    // the next turn did not reuse the stopped session (its stop switch stays on).
    expect(calls.indexOf("events:1")).toBeLessThan(calls.indexOf("send:two"));
    expect(a.open).toHaveBeenCalledTimes(2);
  });

  // The sidecar's per-session stop switch is one-way: once a session is stopped, every later turn
  // in it ends at once with outcome "stopped". A fake that behaves the same way.
  function stickyStopApi(onSend?: (text: string) => void) {
    let opened = 0;
    const stopped = new Set<string>();
    const sent = new Map<string, number>();
    const a: SidecarSessionApi = {
      open: vi.fn(async () => "s" + ++opened),
      send: vi.fn(async (id: string, text: string) => { sent.set(id, (sent.get(id) ?? 0) + 1); onSend?.(text); }),
      events: vi.fn(async (id: string, after: number) => {
        // Each turn in this fake writes two events, so turn n starts after seq 2(n-1).
        const base = ((sent.get(id) ?? 1) - 1) * 2;
        if (after >= base + 2) return { events: [], lastSeq: after, busy: false };
        const evs = stopped.has(id)
          ? [{ seq: base + 1, event: { type: "step_start", step: 0 } }, { seq: base + 2, event: { type: "done", outcome: "stopped" } }]
          : [{ seq: base + 1, event: { type: "final", content: "fresh answer" } }, { seq: base + 2, event: { type: "done", outcome: "answered" } }];
        return { events: evs, lastSeq: base + 2, busy: false };
      }),
      toolResult: vi.fn(async () => {}),
      stop: vi.fn(async (id: string) => { stopped.add(id); }),
    };
    return a;
  }

  it("after a Stop the next turn gets a real answer, not the stopped session's empty one", async () => {
    const ac = new AbortController();
    const a = stickyStopApi((text) => { if (text === "one") ac.abort(); });
    const p = createSidecarProvider(a, () => "p", () => []);
    const cbs = () => ({ onStatus: vi.fn(), onToken: vi.fn(), onToolCall: vi.fn(async () => "r") });
    await p.send({ messages: [{ role: "user", content: "warm up" }], callbacks: cbs() });
    // The member presses Stop while turn "one" runs; the session's stop route is called.
    await expect(p.send({ messages: [{ role: "user", content: "one" }], signal: ac.signal, callbacks: cbs() })).rejects.toBeInstanceOf(TurnStopped);
    expect(a.stop).toHaveBeenCalledWith("s1");
    const out = await p.send({ messages: [{ role: "user", content: "two" }], callbacks: cbs() });
    expect(out.content).toBe("fresh answer");
  });

  it("a turn the member did not stop that ends 'stopped' is shown as a failure, never an empty answer", async () => {
    const a = stickyStopApi();
    const p = createSidecarProvider(a, () => "p", () => []);
    const cbs = () => ({ onStatus: vi.fn(), onToken: vi.fn(), onToolCall: vi.fn(async () => "r") });
    await p.send({ messages: [{ role: "user", content: "warm up" }], callbacks: cbs() });
    await a.stop("s1"); // stopped from elsewhere (the sidecar's own stop or the global stop)
    await expect(p.send({ messages: [{ role: "user", content: "two" }], callbacks: cbs() })).rejects.toThrow(/stopped/i);
    // ...and the turn after that runs in a fresh session.
    const out = await p.send({ messages: [{ role: "user", content: "three" }], callbacks: cbs() });
    expect(out.content).toBe("fresh answer");
    expect(a.open).toHaveBeenCalledTimes(2);
  });

  it("reports step_start as a step", async () => {
    const { a } = api([
      { events: [{ seq: 1, event: { type: "step_start", step: 1 } }, { seq: 2, event: { type: "step_start", step: 2 } }, { seq: 3, event: { type: "final", content: "x" } }, { seq: 4, event: { type: "done", outcome: "answered" } }], lastSeq: 4, busy: false },
    ]);
    const onActivity = vi.fn();
    const p = createSidecarProvider(a, () => "p", () => []);
    await p.send({ messages: [{ role: "user", content: "hi" }], callbacks: { onStatus: vi.fn(), onToken: vi.fn(), onToolCall: vi.fn(async () => "r"), onActivity } });
    expect(onActivity.mock.calls.map((c) => c[0])).toEqual([{ kind: "step", step: 1 }, { kind: "step", step: 2 }]);
  });
});
