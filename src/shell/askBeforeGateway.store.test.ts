// SCL-S7.5a (D-16 extended, US-6.2 AC5, RT-12) — ask before sending to the gateway when the local
// model is configured but its server is not running. The Rust selector now reports that case as
// "local-stopped" (never a route). The store must then:
//   - never send the prompt to the gateway without the member's explicit choice for that message;
//   - at launch, while the app's own start of the local server is pending, wait (bounded) and
//     send locally once it is ready, without asking;
//   - on "restart", restart the local server and answer locally (nothing goes to the gateway);
//   - on "gateway", send exactly that one message to the gateway; the next message asks again;
//   - with no gateway key, behave as before (nothing to ask, nothing sent to a gateway).
//
// The bridge and BRIDGE_MODE are mocked so the Store's tauri branch runs headless (the same
// pattern as onS5Begin.test.ts); timers are fake so the bounded waits are driven explicitly.
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

const route = { inference: "local-stopped", gatewayConfigured: true };
const inferToolsMock = vi.fn(async (..._a: unknown[]) => JSON.stringify({ role: "assistant", content: "answer from the gateway" }));
const inferLocalMock = vi.fn(async (..._a: unknown[]) => "answer from the local model");
const serveStartMock = vi.fn(async () => {
  // The supervisor spawns the server: from now on the selector reports it as running.
  route.inference = "ready";
});
const serveStopMock = vi.fn(async () => {});
const inferenceStateMock = vi.fn(async (_gw: boolean) => route.inference);

vi.mock("../bridge", () => ({
  bindSimHost: () => {},
  bridge: {
    mode: "tauri",
    chat: {
      providerStatus: async () => [{ id: "gateway", configured: route.gatewayConfigured }],
      inferenceState: (gw: boolean) => inferenceStateMock(gw),
      inferTools: (...a: unknown[]) => inferToolsMock(...a),
      inferLocal: (...a: unknown[]) => inferLocalMock(...a),
    },
    model: {
      serveStart: () => serveStartMock(),
      serveStop: () => serveStopMock(),
    },
    agentHarness: { status: async () => ({ running: false }) },
    escalation: { endpoints: async () => [] },
  },
}));

vi.mock("../bridge/mode", () => ({
  BRIDGE_MODE: "tauri",
  assertSimAllowed: () => {},
}));

import { Store, LOCAL_START_WAIT_MS } from "./store";

type Internals = { launchLocalStartPending: boolean };

async function settle(ms = 0): Promise<void> {
  await vi.advanceTimersByTimeAsync(ms);
  for (let i = 0; i < 5; i++) await Promise.resolve();
}

/** Run until the predicate holds (bounded), advancing fake time in small steps. */
async function until(pred: () => boolean, maxMs = 10_000): Promise<void> {
  for (let t = 0; t < maxMs && !pred(); t += 50) await settle(50);
}

async function freshStore(): Promise<Store> {
  const store = new Store();
  store.setState({ chatMsgs: [], chatStatus: "ready", aiDefault: "gateway", hermesSidecarLoop: false });
  await store.rebuildProvider();
  return store;
}

const agentReplies = (s: Store) => s.state.chatMsgs.filter((m) => m.who !== "You");

describe("SCL-S7.5a: ask before sending to the gateway when the local model is not running", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    route.inference = "local-stopped";
    route.gatewayConfigured = true;
    inferToolsMock.mockClear();
    inferLocalMock.mockClear();
    serveStartMock.mockClear();
    serveStopMock.mockClear();
    inferenceStateMock.mockClear();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("holds the message and asks; no gateway request is made without the explicit choice", async () => {
    const store = await freshStore();
    void store.sendChat("summarize my week");
    await until(() => store.state.chatRouteHold?.phase === "ask");
    expect(store.state.chatRouteHold?.phase).toBe("ask");
    // The member's message is shown, nothing has been answered, and nothing was sent anywhere.
    expect(store.state.chatMsgs.map((m) => m.text)).toEqual(["summarize my week"]);
    expect(store.state.chatRouteHold?.msgId).toBe(store.state.chatMsgs[0].id);
    // Time passing never turns the question into a silent route.
    await settle(LOCAL_START_WAIT_MS * 2);
    expect(inferToolsMock).not.toHaveBeenCalled();
    expect(inferLocalMock).not.toHaveBeenCalled();
    expect(store.state.chatRouteHold?.phase).toBe("ask");
    // Cancel sends nothing and frees the chat.
    store.answerLocalModelAsk("cancel");
    await until(() => store.state.chatStatus === "ready");
    expect(inferToolsMock).not.toHaveBeenCalled();
    expect(store.state.chatRouteHold).toBeNull();
    expect(store.state.chatStatus).toBe("ready");
  });

  it('"gateway" sends exactly that one message to the gateway; the next message asks again', async () => {
    const store = await freshStore();
    void store.sendChat("first question");
    await until(() => store.state.chatRouteHold?.phase === "ask");
    store.answerLocalModelAsk("gateway");
    await until(() => store.state.chatStatus === "ready" && agentReplies(store).length === 1);
    expect(inferToolsMock).toHaveBeenCalledTimes(1);
    expect(String(inferToolsMock.mock.calls[0][1])).toContain("first question");
    expect(inferLocalMock).not.toHaveBeenCalled();
    expect(agentReplies(store)[0].text).toContain("answer from the gateway");
    expect(store.state.chatRouteHold).toBeNull();

    // The choice covered only that message: the next one is held and asked about again.
    void store.sendChat("second question");
    await until(() => store.state.chatRouteHold?.phase === "ask");
    expect(store.state.chatRouteHold?.phase).toBe("ask");
    await settle(1000);
    expect(inferToolsMock).toHaveBeenCalledTimes(1);
    store.answerLocalModelAsk("cancel");
    await until(() => store.state.chatStatus === "ready");
  });

  it('"restart" restarts the local server and answers locally; nothing goes to the gateway', async () => {
    const store = await freshStore();
    void store.sendChat("what is my node height?");
    await until(() => store.state.chatRouteHold?.phase === "ask");
    store.answerLocalModelAsk("restart");
    await until(() => store.state.chatStatus === "ready" && agentReplies(store).length === 1);
    expect(serveStartMock).toHaveBeenCalledTimes(1);
    expect(inferLocalMock).toHaveBeenCalledTimes(1);
    expect(String(inferLocalMock.mock.calls[0][0])).toContain("what is my node height?");
    expect(inferToolsMock).not.toHaveBeenCalled();
    expect(agentReplies(store)[0].text).toContain("answer from the local model");
    expect(store.state.chatRouteHold).toBeNull();
  });

  it('"restart" that does not bring the server up fails honestly and never falls back to the gateway', async () => {
    serveStartMock.mockImplementationOnce(async () => {
      throw new Error("llama-server binary not bundled");
    });
    const store = await freshStore();
    void store.sendChat("hello");
    await until(() => store.state.chatRouteHold?.phase === "ask");
    store.answerLocalModelAsk("restart");
    await until(() => store.state.chatStatus === "ready" && agentReplies(store).length === 1, LOCAL_START_WAIT_MS + 5000);
    expect(inferToolsMock).not.toHaveBeenCalled();
    expect(inferLocalMock).not.toHaveBeenCalled();
    const reply = agentReplies(store)[0];
    expect(reply.error).toMatch(/local model did not start/);
    expect(reply.error).toMatch(/nothing was sent to the gateway/);
    expect(reply.retryText).toBe("hello");
  });

  it("at launch, waits for the app's own start of the local server and then sends locally, without asking", async () => {
    const store = await freshStore();
    (store as unknown as Internals).launchLocalStartPending = true;
    void store.sendChat("good morning");
    await until(() => store.state.chatRouteHold?.phase === "waiting");
    expect(store.state.chatRouteHold?.phase).toBe("waiting");
    await settle(3000);
    // Still waiting: not asked, nothing sent.
    expect(store.state.chatRouteHold?.phase).toBe("waiting");
    expect(inferToolsMock).not.toHaveBeenCalled();
    // The app's own start lands (what serveLocalAndRoute does at launch).
    route.inference = "ready";
    (store as unknown as Internals).launchLocalStartPending = false;
    await until(() => store.state.chatStatus === "ready" && agentReplies(store).length === 1);
    expect(inferLocalMock).toHaveBeenCalledTimes(1);
    expect(inferToolsMock).not.toHaveBeenCalled();
    expect(serveStartMock).not.toHaveBeenCalled();
    expect(store.state.chatRouteHold).toBeNull();
  });

  it("the launch wait is bounded: if the start never lands it asks, still without sending to the gateway", async () => {
    const store = await freshStore();
    (store as unknown as Internals).launchLocalStartPending = true;
    void store.sendChat("anyone there?");
    await until(() => store.state.chatRouteHold?.phase === "waiting");
    await settle(LOCAL_START_WAIT_MS + 2000);
    expect(store.state.chatRouteHold?.phase).toBe("ask");
    expect(inferToolsMock).not.toHaveBeenCalled();
    expect(inferLocalMock).not.toHaveBeenCalled();
    store.answerLocalModelAsk("cancel");
    await until(() => store.state.chatStatus === "ready");
  });

  it("a local server that came back on its own is used directly, without asking", async () => {
    const store = await freshStore();
    route.inference = "ready"; // the supervisor recovered after the provider was chosen
    void store.sendChat("still there?");
    await until(() => store.state.chatStatus === "ready" && agentReplies(store).length === 1);
    expect(inferLocalMock).toHaveBeenCalledTimes(1);
    expect(inferToolsMock).not.toHaveBeenCalled();
    expect(store.state.chatRouteHold).toBeNull();
  });

  it("with no gateway key nothing changes: no question, nothing sent to a gateway", async () => {
    route.gatewayConfigured = false;
    route.inference = "demo"; // what Rust reports for a stopped local model with no gateway key
    const store = await freshStore();
    void store.sendChat("hi");
    await until(() => store.state.chatStatus === "ready" && agentReplies(store).length === 1);
    expect(store.state.chatRouteHold).toBeNull();
    expect(inferToolsMock).not.toHaveBeenCalled();
    expect(store.state.chatProviderKind).toBe("demo");
  });
});
