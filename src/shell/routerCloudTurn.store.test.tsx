// Owner report (0.5.3): an OpenAI key connected in Settings › Models could not be used from the
// Hermes model router. Picking a BYOK model in the chat's router must (1) store it in core as the
// preferred model (`providers_preferred_set`) and (2) run the next Hermes turn on it through the core
// model proxy: the sidecar session opens with `hermes_session_open_cloud` (a keyless proxy route),
// never the local session, and no key crosses the bridge. Picking the local model again goes back.
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import type { ModelChoice } from "../agent/modelRouter";

const st = { preferred: null as { providerId: string; model: string } | null };
const calls: string[] = [];
const preferredSet = vi.fn(async (c: { providerId: string; model: string } | null) => {
  st.preferred = c;
  return { preferred: c, loadError: null };
});
const sessionOpen = vi.fn(async (..._a: unknown[]) => {
  calls.push("open-local");
  return "s-local";
});
const sessionOpenCloud = vi.fn(async (..._a: unknown[]) => {
  calls.push("open-cloud");
  return "s-cloud";
});
const sessionSend = vi.fn(async (id: string, text: string) => {
  calls.push(`send:${id}:${text}`);
});
let seq = 0;
const sessionEvents = vi.fn(async (_id: string, after: number) => {
  if (after >= Number.MAX_SAFE_INTEGER - 1) return { events: [], lastSeq: seq, busy: false };
  const s1 = ++seq;
  const s2 = ++seq;
  return { events: [{ seq: s1, event: { type: "final", content: "answer from gpt-4o" } }, { seq: s2, event: { type: "done", outcome: "answered" } }], lastSeq: s2, busy: false };
});

vi.mock("../bridge", () => ({
  bindSimHost: () => {},
  bridge: {
    mode: "tauri",
    chat: {
      providerStatus: async () => [],
      inferenceState: async () => "ready",
      inferTools: async () => "{}",
      consentPrepare: async () => null,
      consentDrop: async () => {},
      inferLocal: async () => "local reply",
      route: async () => ({ route: "ask_first", loadError: null }),
    },
    providers: {
      preferredGet: async () => ({ preferred: st.preferred, loadError: null }),
      preferredSet: (c: { providerId: string; model: string } | null) => preferredSet(c),
      status: async () => [
        {
          id: "openai", name: "OpenAI", kind: "vendor", verified: true, baseUrl: "https://api.openai.com/v1", host: "api.openai.com",
          adapter: "openai-compatible", keyConfigured: true, keySetMs: 1, keyPage: null, revokePage: null, confirmed: true, halt: null,
          caps: { dailyTokens: null, monthlyTokens: null, dailyMicros: null, monthlyMicros: null }, approvedModels: [], listedModels: ["gpt-4o"], prices: {}, models: [],
        },
      ],
    },
    model: { serveStart: async () => {}, serveStop: async () => {} },
    agentHarness: {
      status: async () => ({ running: true }),
      sessionOpen: (...a: unknown[]) => sessionOpen(...a),
      sessionOpenCloud: (...a: unknown[]) => sessionOpenCloud(...a),
      sessionSend: (id: string, text: string) => sessionSend(id, text),
      sessionEvents: (id: string, after: number) => sessionEvents(id, after),
      sessionToolResult: async () => {},
      sessionStop: async () => {},
      sessionClose: async () => {},
      shellPending: async () => [],
      mcpPending: async () => [],
    },
    escalation: { endpoints: async () => [] },
  },
}));
vi.mock("../bridge/mode", () => ({ BRIDGE_MODE: "tauri", assertSimAllowed: () => {} }));

import { Store } from "./store";
import { routerChoicesFor } from "../agent/modelRouterSources";
import { cloudChoiceId } from "../agent/modelRouter";
import { bridge } from "../bridge";

async function settle(ms = 0): Promise<void> {
  await vi.advanceTimersByTimeAsync(ms);
  for (let i = 0; i < 5; i++) await Promise.resolve();
}
async function until(pred: () => boolean, maxMs = 10_000): Promise<void> {
  for (let t = 0; t < maxMs && !pred(); t += 50) await settle(50);
}

async function routerChoices(): Promise<ModelChoice[]> {
  const cloud = await bridge.providers.status();
  return routerChoicesFor({ local: [{ id: "loc1", source: "hf", repo: "r", file: "gemma.gguf", sizeBytes: 1, sha256: "a", kind: "gguf" }], registry: [], cloud });
}

describe("router pick → a Hermes turn on the chosen BYOK model, through the core proxy", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    st.preferred = null;
    calls.length = 0;
    seq = 0;
    for (const f of [preferredSet, sessionOpen, sessionOpenCloud, sessionSend, sessionEvents]) f.mockClear();
    try {
      localStorage.clear();
    } catch {
      /* no storage */
    }
  });
  afterEach(() => vi.useRealTimers());

  it("picking OpenAI · gpt-4o stores it in core and the next turn opens a cloud (proxied) session", async () => {
    const store = new Store();
    store.setState({ chatMsgs: [], chatStatus: "ready", hermesSidecarLoop: true });
    await store.rebuildProvider();
    const choices = await routerChoices();
    const id = cloudChoiceId("openai", "gpt-4o");
    expect(choices.some((c) => c.id === id)).toBe(true);

    expect(await store.selectModel(id, choices)).toBeNull();
    expect(preferredSet).toHaveBeenCalledWith({ providerId: "openai", model: "gpt-4o" });
    expect(store.state.hermesCloudModel).toEqual({ providerId: "openai", model: "gpt-4o" });
    expect(store.routerActive(choices).id).toBe(id);
    await until(() => store.activeProviderKind() === "sidecar-cloud");
    expect(store.activeProviderKind()).toBe("sidecar-cloud");

    void store.sendChat("hello");
    await until(() => calls.some((c) => c.startsWith("send:")));
    expect(sessionOpenCloud).toHaveBeenCalledTimes(1);
    expect(sessionOpen).not.toHaveBeenCalled();
    expect(calls).toContain("send:s-cloud:hello");
    // Keyless: nothing core was handed for the session carries a key.
    expect(JSON.stringify(sessionOpenCloud.mock.calls)).not.toMatch(/sk-[A-Za-z0-9]/);
  });

  it("picking the local model again clears the cloud choice and the turn runs locally", async () => {
    st.preferred = { providerId: "openai", model: "gpt-4o" };
    const store = new Store();
    store.setState({ chatMsgs: [], chatStatus: "ready", hermesSidecarLoop: true });
    await store.rebuildProvider();
    expect(store.activeProviderKind()).toBe("sidecar-cloud");
    const choices = await routerChoices();

    expect(await store.selectModel("loc1", choices)).toBeNull();
    expect(preferredSet).toHaveBeenLastCalledWith(null);
    expect(store.state.hermesCloudModel).toBeNull();
    await until(() => store.activeProviderKind() === "sidecar");
    void store.sendChat("hi");
    await until(() => calls.some((c) => c.startsWith("send:")));
    expect(sessionOpen).toHaveBeenCalled();
    expect(sessionOpenCloud).not.toHaveBeenCalled();
  });

  it("core's refusal leaves the selection unchanged and is returned", async () => {
    preferredSet.mockImplementationOnce(async () => {
      throw new Error("OpenAI is paused");
    });
    const store = new Store();
    store.setState({ chatMsgs: [], chatStatus: "ready", hermesSidecarLoop: true });
    await store.rebuildProvider();
    const choices = await routerChoices();
    expect(await store.selectModel(cloudChoiceId("openai", "gpt-4o"), choices)).toBe("OpenAI is paused");
    expect(store.state.hermesCloudModel).toBeNull();
    expect(store.state.activeModelId).not.toBe(cloudChoiceId("openai", "gpt-4o"));
  });
});
