// K2 (0.5.2): "Local model only" is a real stored setting (Rust chat_route.rs) with two values.
//   local_only: chat never routes to the gateway. A stopped local model offers only to start it;
//               with no local model the message is not sent and the chat says how to fix it.
//   ask_first (default): today's ask-before-the-gateway, and the silent case is closed: a gateway
//               key with no local model asks before each message instead of sending.
// The bridge and BRIDGE_MODE are mocked so the Store's desktop branch runs headless (the same
// pattern as askBeforeGateway.store.test.ts).
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

const route = { inference: "ask-gateway", gatewayConfigured: true, chatRoute: "ask_first" as "ask_first" | "local_only", statusFails: false };
// K2: core mints a one-time consent for each held message the member may send to the gateway.
const consentPrepareMock = vi.fn(async (providerId: unknown, _message: unknown) => {
  if (consent.fail) throw new Error("consent unavailable");
  return { consentId: "cc_" + ++consent.n, providerId, expiresAtMs: 0 };
});
const consent = { n: 0, fail: false };
const inferToolsMock = vi.fn(async (..._a: unknown[]) => JSON.stringify({ role: "assistant", content: "answer from the gateway" }));
const inferLocalMock = vi.fn(async (..._a: unknown[]) => "answer from the local model");
const serveStartMock = vi.fn(async () => {
  route.inference = "ready";
});
const setRouteMock = vi.fn(async (r: "ask_first" | "local_only") => {
  route.chatRoute = r;
  return { route: r, loadError: null };
});

vi.mock("../bridge", () => ({
  bindSimHost: () => {},
  bridge: {
    mode: "tauri",
    chat: {
      providerStatus: async () => {
        // An unavailable OS keyring or an undecodable saved provider record.
        if (route.statusFails) throw new Error("provider records unreadable");
        return [{ id: "gateway", configured: route.gatewayConfigured }];
      },
      inferenceState: async () => route.inference,
      inferTools: (...a: unknown[]) => inferToolsMock(...a),
      consentPrepare: (...a: unknown[]) => consentPrepareMock(...a),
      inferLocal: (...a: unknown[]) => inferLocalMock(...a),
      route: async () => ({ route: route.chatRoute, loadError: null }),
      setRoute: (r: "ask_first" | "local_only") => setRouteMock(r),
    },
    model: { serveStart: () => serveStartMock(), serveStop: async () => {} },
    agentHarness: { status: async () => ({ running: false }) },
    escalation: { endpoints: async () => [] },
  },
}));
vi.mock("../bridge/mode", () => ({ BRIDGE_MODE: "tauri", assertSimAllowed: () => {} }));

import { Store, pickChatProviderKind, routeAskOptions } from "./store";

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
const agentReplies = (s: Store) => s.state.chatMsgs.filter((m) => m.who !== "You");
const gw = [{ id: "gateway", configured: true }];
const STATES = ["ready", "local-stopped", "local-fallback", "gateway-only", "downloading", "no-model", "demo", "ask-gateway", "local-only", "local-only-stopped", undefined];

describe("chat route: pure routing per value", () => {
  it("never picks a silent gateway route, for either value and any state", () => {
    for (const r of ["ask_first", "local_only"] as const) {
      for (const st of STATES) {
        expect(pickChatProviderKind(gw, "gateway", "tauri", st, r)).not.toBe("real");
      }
    }
  });

  it("ask_first: local when ready; asks when stopped, when only a gateway key exists, and while downloading", () => {
    expect(pickChatProviderKind(gw, "gateway", "tauri", "ready", "ask_first")).toBe("local");
    expect(pickChatProviderKind(gw, "gateway", "tauri", "local-stopped", "ask_first")).toBe("ask");
    expect(pickChatProviderKind(gw, "gateway", "tauri", "ask-gateway", "ask_first")).toBe("ask");
    // An older core that still says gateway-only / downloading: asked, never sent silently.
    expect(pickChatProviderKind(gw, "gateway", "tauri", "gateway-only", "ask_first")).toBe("ask");
    expect(pickChatProviderKind(gw, "gateway", "tauri", "downloading", "ask_first")).toBe("ask");
    expect(pickChatProviderKind([], "gateway", "tauri", "demo", "ask_first")).toBe("demo");
    expect(routeAskOptions("local-stopped", "ask_first")).toEqual({ restart: true, gateway: true });
    expect(routeAskOptions("ask-gateway", "ask_first")).toEqual({ restart: false, gateway: true });
  });

  it("local_only: local when ready; a stopped model offers only a restart; no model offers nothing remote", () => {
    expect(pickChatProviderKind(gw, "gateway", "tauri", "ready", "local_only")).toBe("local");
    expect(pickChatProviderKind(gw, "gateway", "tauri", "local-only-stopped", "local_only")).toBe("ask");
    expect(pickChatProviderKind(gw, "gateway", "tauri", "local-only", "local_only")).toBe("ask");
    // An older core that does not know the route yet: the app still keeps it local.
    expect(pickChatProviderKind(gw, "gateway", "tauri", "gateway-only", "local_only")).toBe("ask");
    expect(pickChatProviderKind(gw, "gateway", "tauri", "local-stopped", "local_only")).toBe("ask");
    expect(routeAskOptions("local-only-stopped", "local_only")).toEqual({ restart: true, gateway: false });
    expect(routeAskOptions("local-stopped", "local_only")).toEqual({ restart: true, gateway: false });
    expect(routeAskOptions("local-only", "local_only")).toEqual({ restart: false, gateway: false });
    expect(routeAskOptions("gateway-only", "local_only")).toEqual({ restart: false, gateway: false });
  });
});

// STD (Codex P2 on #372 at 33410a0): local_only never falls back to the demo agent.
describe("chat route: local only never uses the demo agent", () => {
  it("tripwire: under local_only every non-ready state holds the message and offers nothing remote", () => {
    for (const remote of [[], gw]) {
      for (const st of STATES) {
        if (st === "ready") continue;
        expect(pickChatProviderKind(remote, "gateway", "tauri", st, "local_only")).toBe("ask");
        expect(routeAskOptions(st, "local_only", remote.length > 0).gateway).toBe(false);
      }
    }
  });
});

describe("chat route: the store", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    route.inference = "ask-gateway";
    route.gatewayConfigured = true;
    route.chatRoute = "ask_first";
    route.statusFails = false;
    inferToolsMock.mockClear();
    inferLocalMock.mockClear();
    serveStartMock.mockClear();
    setRouteMock.mockClear();
  });
  afterEach(() => vi.useRealTimers());

  it("ask_first, gateway key and no local model: the message is held and asked about, never sent silently", async () => {
    const store = await freshStore();
    expect(store.state.chatRoute).toBe("ask_first");
    void store.sendChat("hello");
    await until(() => store.state.chatRouteHold?.phase === "ask");
    expect(store.state.chatRouteHold?.options).toEqual({ restart: false, gateway: true });
    await settle(30_000);
    expect(inferToolsMock).not.toHaveBeenCalled();
    // A choice that is not offered does nothing.
    store.answerLocalModelAsk("restart");
    await settle(100);
    expect(store.state.chatRouteHold?.phase).toBe("ask");
    expect(serveStartMock).not.toHaveBeenCalled();
    store.answerLocalModelAsk("gateway");
    await until(() => store.state.chatStatus === "ready" && agentReplies(store).length === 1);
    expect(inferToolsMock).toHaveBeenCalledTimes(1);
  });

  it("local_only, no local model: not sent, with a clear 'start the local model' message; nothing goes to the gateway", async () => {
    route.chatRoute = "local_only";
    route.inference = "local-only";
    const store = await freshStore();
    void store.sendChat("hello");
    await until(() => store.state.chatStatus === "ready" && agentReplies(store).length === 1);
    expect(store.state.chatRouteHold).toBeNull();
    const reply = agentReplies(store)[0];
    expect(reply.error).toContain("Local only");
    expect(reply.error).toContain("start the local model");
    expect(inferToolsMock).not.toHaveBeenCalled();
  });

  it("local_only, local model stopped: offers only the restart; the gateway answer does nothing", async () => {
    route.chatRoute = "local_only";
    route.inference = "local-only-stopped";
    const store = await freshStore();
    void store.sendChat("hello");
    await until(() => store.state.chatRouteHold?.phase === "ask");
    expect(store.state.chatRouteHold?.options).toEqual({ restart: true, gateway: false });
    store.answerLocalModelAsk("gateway");
    await settle(100);
    expect(inferToolsMock).not.toHaveBeenCalled();
    expect(store.state.chatRouteHold?.phase).toBe("ask");
    store.answerLocalModelAsk("restart");
    await until(() => store.state.chatStatus === "ready" && agentReplies(store).length === 1);
    expect(serveStartMock).toHaveBeenCalledTimes(1);
    expect(inferLocalMock).toHaveBeenCalledTimes(1);
    expect(inferToolsMock).not.toHaveBeenCalled();
  });

  for (const inference of ["demo", "downloading", "no-model"]) {
    it(`local_only, no key, core reports "${inference}": not sent with the local-only message; no demo reply, no tool run`, async () => {
      route.chatRoute = "local_only";
      route.inference = inference;
      route.gatewayConfigured = false;
      const store = await freshStore();
      expect(store.provider?.kind).not.toBe("demo");
      void store.sendChat("what is my balance?");
      await until(() => store.state.chatStatus === "ready" && agentReplies(store).length === 1);
      const replies = agentReplies(store);
      expect(replies).toHaveLength(1);
      expect(replies[0].error).toContain("Local only");
      expect(replies[0].text).toBe("");
      expect(store.state.chatRouteHold).toBeNull();
      expect(inferToolsMock).not.toHaveBeenCalled();
      expect(inferLocalMock).not.toHaveBeenCalled();
    });
  }

  // Codex P2 on #372 at 7143b4e: the route is read and kept even when the provider records cannot be read.
  for (const inference of ["local-only", "demo", "no-model"]) {
    it(`local_only, provider records unreadable, core reports "${inference}": not sent; no demo reply, no tool run`, async () => {
      route.chatRoute = "local_only";
      route.inference = inference;
      route.statusFails = true;
      const store = await freshStore();
      expect(store.state.chatRoute).toBe("local_only");
      expect(store.provider?.kind).not.toBe("demo");
      void store.sendChat("what is my balance?");
      await until(() => store.state.chatStatus === "ready" && agentReplies(store).length === 1);
      const replies = agentReplies(store);
      expect(replies).toHaveLength(1);
      expect(replies[0].error).toContain("Local only");
      expect(replies[0].text).toBe("");
      expect(store.state.chatRouteHold).toBeNull();
      expect(inferToolsMock).not.toHaveBeenCalled();
      expect(inferLocalMock).not.toHaveBeenCalled();
    });
  }

  for (const chatRoute of ["local_only", "ask_first"] as const) {
    it(`${chatRoute}, provider records unreadable, healthy local model: chat uses the local model`, async () => {
      route.chatRoute = chatRoute;
      route.inference = "ready";
      route.statusFails = true;
      const store = await freshStore();
      expect(store.provider?.kind).not.toBe("demo");
      void store.sendChat("hello");
      await until(() => store.state.chatStatus === "ready" && agentReplies(store).length === 1);
      expect(inferLocalMock).toHaveBeenCalledTimes(1);
      expect(inferToolsMock).not.toHaveBeenCalled();
    });
  }

  it("tripwire: under local_only, no inference state and no provider-status outcome builds the demo agent", async () => {
    route.chatRoute = "local_only";
    for (const statusFails of [false, true]) {
      for (const gatewayConfigured of [false, true]) {
        for (const st of STATES) {
          route.statusFails = statusFails;
          route.gatewayConfigured = gatewayConfigured;
          route.inference = st ?? "demo";
          const store = await freshStore();
          expect(store.provider?.kind, `${st} statusFails=${statusFails} key=${gatewayConfigured}`).not.toBe("demo");
        }
      }
    }
  });

  it("ask_first, provider records unreadable, no local model: nothing remote is offered", async () => {
    route.chatRoute = "ask_first";
    route.inference = "local-stopped";
    route.statusFails = true;
    const store = await freshStore();
    void store.sendChat("hello");
    await until(() => store.state.chatRouteHold?.phase === "ask");
    expect(store.state.chatRouteHold?.options).toEqual({ restart: true, gateway: false });
    store.answerLocalModelAsk("gateway");
    await settle(100);
    expect(inferToolsMock).not.toHaveBeenCalled();
  });

  it("setChatRoute stores the choice through chat_route_set and re-routes at once", async () => {
    const store = await freshStore();
    await store.setChatRoute("local_only");
    expect(setRouteMock).toHaveBeenCalledWith("local_only");
    expect(store.state.chatRoute).toBe("local_only");
    // The next launch reads it back from core.
    const next = await freshStore();
    expect(next.state.chatRoute).toBe("local_only");
  });
});
