// Owner report (0.5.4, 2026-11-17): with terminal commands on and a BYOK cloud model chosen for
// Hermes (no local model running, the default "ask first" route), Hermes answered that it has no
// terminal access. Each held message the member sent to the cloud model ran in the app's own tool
// loop (core-hosted tools only, usage session "webview-chat"), not in the Hermes sidecar session
// that hosts shell_run, folder files, web and MCP. The member's per-message choice (K2) still
// gates the send; once made, the message must go to Hermes's cloud session.
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

const calls: string[] = [];
const consentPrepare = vi.fn(async (providerId: unknown, _m: unknown) => ({ consentId: "cc_1", providerId, expiresAtMs: 0 }));
const consentDrop = vi.fn(async (_id: unknown) => {});
const inferTools = vi.fn(async (..._a: unknown[]) => JSON.stringify({ role: "assistant", content: "app loop answer" }));
const sessionOpen = vi.fn(async (..._a: unknown[]) => {
  calls.push("open-local");
  return "s-local";
});
let opened = 0;
const sessionOpenCloud = vi.fn(async (..._a: unknown[]) => {
  calls.push("open-cloud");
  opened += 1;
  return opened === 1 ? "s-cloud" : `s-cloud-${opened}`;
});
const sessionClose = vi.fn(async (id: string) => {
  calls.push(`close:${id}`);
});
const sessionSend = vi.fn(async (id: string, text: string) => {
  calls.push(`send:${id}:${text}`);
});
let seq = 0;
const sessionEvents = vi.fn(async (_id: string, after: number) => {
  if (after >= Number.MAX_SAFE_INTEGER - 1) return { events: [], lastSeq: seq, busy: false };
  const s1 = ++seq;
  const s2 = ++seq;
  return { events: [{ seq: s1, event: { type: "final", content: "answer from Hermes on gpt-4o" } }, { seq: s2, event: { type: "done", outcome: "answered" } }], lastSeq: s2, busy: false };
});
const st = {
  running: true,
  route: "ask_first" as "ask_first" | "local_only",
  preferred: { providerId: "openai", model: "gpt-4o" } as { providerId: string; model: string } | null,
};
const preferredSet = vi.fn(async (c: { providerId: string; model: string } | null) => {
  st.preferred = c;
  return { preferred: c, loadError: null };
});

vi.mock("../bridge", () => ({
  bindSimHost: () => {},
  bridge: {
    mode: "tauri",
    chat: {
      providerStatus: async () => [],
      // No local model serving; a remote route is configured (the preferred cloud model).
      inferenceState: async (remote: boolean) => (remote ? "ask-gateway" : "demo"),
      inferTools: (...a: unknown[]) => inferTools(...a),
      consentPrepare: (...a: unknown[]) => consentPrepare(...a),
      consentDrop: (id: unknown) => consentDrop(id),
      inferLocal: async () => "local reply",
      route: async () => ({ route: st.route, loadError: null }),
    },
    providers: {
      preferredGet: async () => ({ preferred: st.preferred, loadError: null }),
      preferredSet: (c: { providerId: string; model: string } | null) => preferredSet(c),
      status: async () => [],
    },
    model: { serveStart: async () => {}, serveStop: async () => {} },
    agentHarness: {
      status: async () => ({ running: st.running }),
      sessionOpen: (...a: unknown[]) => sessionOpen(...a),
      sessionOpenCloud: (...a: unknown[]) => sessionOpenCloud(...a),
      sessionSend: (id: string, text: string) => sessionSend(id, text),
      sessionEvents: (id: string, after: number) => sessionEvents(id, after),
      sessionToolResult: async () => {},
      sessionStop: async () => {},
      sessionClose: (id: string) => sessionClose(id),
      shellPending: async () => [],
      mcpPending: async () => [],
    },
    escalation: { endpoints: async () => [] },
  },
}));
vi.mock("../bridge/mode", () => ({ BRIDGE_MODE: "tauri", assertSimAllowed: () => {} }));

import { Store } from "./store";

async function settle(ms = 0): Promise<void> {
  await vi.advanceTimersByTimeAsync(ms);
  for (let i = 0; i < 5; i++) await Promise.resolve();
}
async function until(pred: () => boolean, maxMs = 10_000): Promise<void> {
  for (let t = 0; t < maxMs && !pred(); t += 50) await settle(50);
}
async function freshStore(loop = true): Promise<Store> {
  const store = new Store();
  store.setState({ chatMsgs: [], chatStatus: "ready", aiDefault: "gateway", hermesSidecarLoop: loop });
  await store.rebuildProvider();
  return store;
}

describe("a held message sent to the preferred cloud model runs in Hermes's sidecar session", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    calls.length = 0;
    seq = 0;
    opened = 0;
    st.running = true;
    st.route = "ask_first";
    st.preferred = { providerId: "openai", model: "gpt-4o" };
    for (const f of [consentPrepare, consentDrop, inferTools, sessionOpen, sessionOpenCloud, sessionSend, sessionEvents, sessionClose, preferredSet]) f.mockClear();
    try {
      localStorage.clear();
    } catch {
      /* no storage */
    }
  });
  afterEach(() => vi.useRealTimers());

  it("asks first, then sends through the sidecar cloud session (where shell_run lives), not the app's own loop", async () => {
    const store = await freshStore();
    expect(store.activeProviderKind()).toBe("sidecar-cloud");
    void store.sendChat("run git status in my project");
    // K2 unchanged: nothing goes out before the member's choice for this message.
    await until(() => store.state.chatRouteHold?.phase === "ask");
    expect(store.state.chatRouteHold?.options?.gateway).toBe(true);
    expect(sessionSend).not.toHaveBeenCalled();
    expect(inferTools).not.toHaveBeenCalled();

    store.answerLocalModelAsk("gateway");
    await until(() => calls.some((c) => c.startsWith("send:")));
    expect(sessionOpenCloud).toHaveBeenCalledTimes(1);
    expect(sessionOpen).not.toHaveBeenCalled();
    expect(calls).toContain("send:s-cloud:run git status in my project");
    expect(inferTools).not.toHaveBeenCalled();
    // The consent minted for the app-loop send is not used by the sidecar session: it is dropped.
    await until(() => consentDrop.mock.calls.length > 0);
    expect(consentDrop).toHaveBeenCalledWith("cc_1");
  });

  it("with Hermes not running, the member's choice still sends through the app's own loop", async () => {
    st.running = false;
    const store = await freshStore();
    void store.sendChat("hello");
    await until(() => store.state.chatRouteHold?.phase === "ask");
    store.answerLocalModelAsk("gateway");
    await until(() => inferTools.mock.calls.length > 0);
    expect(inferTools.mock.calls[0][4]).toBe("cc_1");
    expect(sessionSend).not.toHaveBeenCalled();
  });

  it("declining the held message sends nothing anywhere", async () => {
    const store = await freshStore();
    void store.sendChat("hello");
    await until(() => store.state.chatRouteHold?.phase === "ask");
    store.answerLocalModelAsk("cancel");
    await settle(1_000);
    expect(sessionSend).not.toHaveBeenCalled();
    expect(inferTools).not.toHaveBeenCalled();
  });
  /** Send one held message and answer the card with `choice`; resolves once the chat is ready. */
  async function sendHeld(store: Store, text: string, choice: "gateway" | "cancel" = "gateway"): Promise<void> {
    void store.sendChat(text);
    await until(() => store.state.chatRouteHold?.phase === "ask");
    store.answerLocalModelAsk(choice);
    await until(() => store.state.chatStatus === "ready" && store.state.chatRouteHold === null);
    await settle(100);
  }

  it("a second held message continues the same Hermes session (one open, the follow-up keeps its context)", async () => {
    const store = await freshStore();
    await sendHeld(store, "first");
    await sendHeld(store, "now commit that");
    expect(calls.filter((c) => c === "open-cloud")).toEqual(["open-cloud"]);
    expect(calls).toContain("send:s-cloud:first");
    expect(calls).toContain("send:s-cloud:now commit that");
    expect(sessionClose).not.toHaveBeenCalled();
    expect(inferTools).not.toHaveBeenCalled();
  });

  it("a real change (another preferred cloud model) closes the session it replaces", async () => {
    const store = await freshStore();
    await sendHeld(store, "first");
    expect(await store.setHermesCloudModel({ providerId: "openai", model: "gpt-5" })).toBeNull();
    await until(() => sessionClose.mock.calls.length > 0);
    expect(sessionClose).toHaveBeenCalledWith("s-cloud");
    await sendHeld(store, "second");
    expect(calls).toContain("send:s-cloud-2:second");
  });

  it("Hermes stopping while the card is open: the member's choice goes to the app's own loop with its consent", async () => {
    const store = await freshStore();
    void store.sendChat("hello");
    await until(() => store.state.chatRouteHold?.phase === "ask");
    st.running = false;
    store.answerLocalModelAsk("gateway");
    await until(() => inferTools.mock.calls.length > 0);
    expect(inferTools.mock.calls[0][4]).toBe("cc_1");
    expect(sessionSend).not.toHaveBeenCalled();
  });

  it("with Hermes's loop switched off in Settings, the preferred cloud model runs in the app's own loop", async () => {
    const store = await freshStore(false);
    expect(store.activeProviderKind()).not.toBe("sidecar-cloud");
    await sendHeld(store, "hello");
    expect(inferTools).toHaveBeenCalled();
    expect(inferTools.mock.calls[0][4]).toBe("cc_1");
    expect(sessionOpenCloud).not.toHaveBeenCalled();
    expect(sessionSend).not.toHaveBeenCalled();
  });
});
