// 0.5.4 (owner 2026-11-14): "a warning pops up saying it isn't running when it is."
// The chat provider was built once, before Hermes started, and nothing rebuilt it when Hermes came
// up, so every message toasted "Hermes is not running". Here: Hermes starting after the provider
// was built moves chat onto Hermes (no notice), and the notice appears only from a fresh read.
import { describe, it, expect, vi, beforeEach } from "vitest";

const h = { running: false, statusError: null as string | null };
const statusMock = vi.fn(async () => {
  if (h.statusError) throw new Error(h.statusError);
  return { running: h.running, skills: 0, pendingApprovals: 0 };
});
const inferLocalMock = vi.fn(async () => "a plain local reply");
const sessionOpenMock = vi.fn(async () => "s-1");

vi.mock("../bridge", () => ({
  bindSimHost: () => {},
  bridge: {
    mode: "tauri",
    chat: {
      providerStatus: async () => [],
      inferenceState: async () => "ready",
      inferLocal: (...a: unknown[]) => inferLocalMock(...(a as [])),
      route: async () => ({ route: "ask_first", loadError: null }),
    },
    providers: { preferredGet: async () => ({ preferred: null, loadError: null }), status: async () => [] },
    model: { serveStart: async () => {}, serveStop: async () => {} },
    agentHarness: {
      status: () => statusMock(),
      sessionOpen: (...a: unknown[]) => sessionOpenMock(...(a as [])),
      sessionSend: async () => {},
      sessionEvents: async () => ({ events: [{ seq: 1, event: { type: "final", content: "from Hermes" } }, { seq: 2, event: { type: "done", outcome: "answered" } }], lastSeq: 2, busy: false, pendingCoreCalls: [] }),
      sessionStop: async () => {},
      sessionClose: async () => {},
      skills: async () => [],
      pendingApprovals: async () => [],
      registrySkills: async () => [],
    },
    escalation: { endpoints: async () => [] },
  },
}));
vi.mock("../bridge/mode", () => ({ BRIDGE_MODE: "tauri", assertSimAllowed: () => {} }));

import { Store } from "./store";
import { agentSlice } from "./slices/agent";
import { SIDECAR_DOWN_NOTICE, SIDECAR_LOOP_OFF_NOTICE, sidecarUnreadableNotice } from "../agent/sidecarLiveness";

const flush = async (n = 40) => {
  for (let i = 0; i < n; i++) await new Promise((r) => setTimeout(r, 0));
};

async function freshStore(loop = true): Promise<{ store: Store; toasts: string[] }> {
  const store = new Store();
  const toasts: string[] = [];
  vi.spyOn(store, "toast").mockImplementation((t: string) => void toasts.push(t));
  store.setState({ chatMsgs: [], chatStatus: "ready", aiDefault: "gateway", hermesSidecarLoop: loop });
  await store.rebuildProvider();
  return { store, toasts };
}

function setHermesRow(state: "off" | "running"): void {
  agentSlice.set((s) => ({ attached: s.attached.map((r) => (r.id === "hermes" ? { ...r, state } : r)) }));
}

beforeEach(() => {
  h.running = false;
  h.statusError = null;
  statusMock.mockClear();
  inferLocalMock.mockClear();
  sessionOpenMock.mockClear();
  setHermesRow("off");
  localStorage.clear();
});

describe("0.5.4: the Hermes-not-running notice reflects the real state", () => {
  it("Hermes started after the provider was built: the next message goes to Hermes, no warning", async () => {
    const { store, toasts } = await freshStore();
    expect(store.provider?.kind).toBe("local");
    h.running = true; // started from Agent › Start or Node › Services, without the chat knowing
    await store.sendChat("hello");
    await flush();
    expect(sessionOpenMock).toHaveBeenCalledTimes(1);
    expect(inferLocalMock).not.toHaveBeenCalled();
    expect(toasts).not.toContain(SIDECAR_DOWN_NOTICE);
    expect(store.provider?.kind).toBe("sidecar");
  });

  it("the Agent slice seeing Hermes start rebuilds the chat provider at once", async () => {
    const { store } = await freshStore();
    store.followHermes();
    expect(store.provider?.kind).toBe("local");
    h.running = true;
    setHermesRow("running");
    await flush();
    expect(store.provider?.kind).toBe("sidecar");
    h.running = false;
    setHermesRow("off");
    await flush();
    expect(store.provider?.kind).toBe("local");
  });

  it("Hermes really not running: the warning is shown, from a fresh read at send time", async () => {
    const { store, toasts } = await freshStore();
    const before = statusMock.mock.calls.length;
    await store.sendChat("hello");
    await flush();
    expect(statusMock.mock.calls.length).toBeGreaterThan(before);
    expect(toasts).toContain(SIDECAR_DOWN_NOTICE);
    expect(inferLocalMock).toHaveBeenCalledTimes(1);
  });

  it("a status read that fails never says 'not running'", async () => {
    h.statusError = "hermes control: connection refused";
    const { store, toasts } = await freshStore();
    await store.sendChat("hello");
    await flush();
    expect(toasts).not.toContain(SIDECAR_DOWN_NOTICE);
    expect(toasts).toContain(sidecarUnreadableNotice("hermes control: connection refused"));
  });

  it("the loop switched off says that, not that Hermes is down, and reads nothing", async () => {
    h.running = true;
    const { store, toasts } = await freshStore(false);
    const before = statusMock.mock.calls.length;
    await store.sendChat("hello");
    await flush();
    expect(statusMock.mock.calls.length).toBe(before);
    expect(toasts).toContain(SIDECAR_LOOP_OFF_NOTICE);
    expect(toasts).not.toContain(SIDECAR_DOWN_NOTICE);
  });
});
