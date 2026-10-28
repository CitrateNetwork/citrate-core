// D-11 (P-5): with a preferred cloud model stored in core, a held message offers that model (not
// the gateway preset) and the member's choice sends it there through core (provider id
// "preferred", the consent minted for it); under Local model only nothing remote is offered.
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";

const st = { chatRoute: "ask_first" as "ask_first" | "local_only", preferred: { providerId: "openai", model: "gpt-4o" } as { providerId: string; model: string } | null };
const consentPrepareMock = vi.fn(async (providerId: unknown, _m: unknown) => ({ consentId: "cc_1", providerId, expiresAtMs: 0 }));
const inferToolsMock = vi.fn(async (..._a: unknown[]) => JSON.stringify({ role: "assistant", content: "answer from the preferred model" }));

vi.mock("../bridge", () => ({
  bindSimHost: () => {},
  bridge: {
    mode: "tauri",
    chat: {
      providerStatus: async () => [],
      inferenceState: async (remote: boolean) => (remote ? "ask-gateway" : "demo"),
      inferTools: (...a: unknown[]) => inferToolsMock(...a),
      consentPrepare: (...a: unknown[]) => consentPrepareMock(...a),
      consentDrop: async () => {},
      inferLocal: async () => "local",
      route: async () => ({ route: st.chatRoute, loadError: null }),
    },
    providers: { preferredGet: async () => ({ preferred: st.preferred, loadError: null }) },
    model: { serveStart: async () => {}, serveStop: async () => {} },
    agentHarness: { status: async () => ({ running: false }) },
    escalation: { endpoints: async () => [] },
  },
}));
vi.mock("../bridge/mode", () => ({ BRIDGE_MODE: "tauri", assertSimAllowed: () => {} }));

import { Store, PREFERRED_CHAT_ID } from "./store";
import { LocalModelAsk } from "../components/LocalModelAsk";

async function settle(ms = 0): Promise<void> {
  await vi.advanceTimersByTimeAsync(ms);
  for (let i = 0; i < 5; i++) await Promise.resolve();
}
async function until(pred: () => boolean, maxMs = 10_000): Promise<void> {
  for (let t = 0; t < maxMs && !pred(); t += 50) await settle(50);
}
async function freshStore(): Promise<Store> {
  const store = new Store();
  store.setState({ chatMsgs: [], chatStatus: "ready", aiDefault: "gateway", hermesSidecarLoop: false });
  await store.rebuildProvider();
  return store;
}

describe("chat on the preferred cloud model (D-11)", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    st.chatRoute = "ask_first";
    st.preferred = { providerId: "openai", model: "gpt-4o" };
    consentPrepareMock.mockClear();
    inferToolsMock.mockClear();
  });
  afterEach(() => vi.useRealTimers());

  it("mirrors core's setting, offers that model for a held message, and sends only on the member's choice", async () => {
    const store = await freshStore();
    expect(store.state.hermesCloudModel).toEqual({ providerId: "openai", model: "gpt-4o" });
    void store.sendChat("hello");
    await until(() => store.state.chatRouteHold?.phase === "ask");
    expect(store.state.chatRouteHold?.options).toEqual({ restart: false, gateway: true, remoteLabel: "openai · gpt-4o" });
    expect(consentPrepareMock).toHaveBeenCalledWith(PREFERRED_CHAT_ID, "hello");
    expect(inferToolsMock).not.toHaveBeenCalled();
    store.answerLocalModelAsk("gateway");
    await until(() => inferToolsMock.mock.calls.length > 0);
    expect(inferToolsMock.mock.calls[0][0]).toBe(PREFERRED_CHAT_ID);
    expect(inferToolsMock.mock.calls[0][4]).toBe("cc_1");
  });

  it("under Local model only nothing remote is offered, preferred model or not", async () => {
    st.chatRoute = "local_only";
    const store = await freshStore();
    void store.sendChat("hello");
    await settle(1_000);
    expect(store.state.chatRouteHold?.options?.gateway ?? false).toBe(false);
    expect(consentPrepareMock).not.toHaveBeenCalled();
    expect(inferToolsMock).not.toHaveBeenCalled();
  });

  it("the held-message card names the preferred model", () => {
    const html = renderToStaticMarkup(<LocalModelAsk phase="ask" onChoose={() => {}} options={{ restart: false, gateway: true, remoteLabel: "openai · gpt-4o" }} />);
    expect(html).toContain("Send this message to openai · gpt-4o this time");
  });
});
