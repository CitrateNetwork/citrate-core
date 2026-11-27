// L1-WP01 CH-2 (characterization, citrate-core a826e441). LQ registry: surface "Hermes start and
// sessions".
//
// Pins the launch order inside `Store.start()` (src/shell/store.ts) in the desktop app: the
// Hermes terminal spawn setting is synced first, then the chat provider is chosen, then chat
// starts following Hermes's run state; and a second start() does nothing. WP08 (Hermes
// auto-starts at launch) adds its step through the L0 seam after syncHermesTerminal and names CH-2.
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

vi.mock("../../bridge", () => ({
  bindSimHost: () => {},
  bridge: {
    mode: "tauri",
    chat: {
      providerStatus: async () => [],
      inferenceState: async () => "demo",
      inferLocal: async () => "",
      route: async () => ({ route: "ask_first", loadError: null }),
    },
    providers: { preferredGet: async () => ({ preferred: null, loadError: null }), status: async () => [] },
    model: { serveStart: async () => {}, serveStop: async () => {} },
    agentHarness: {
      status: async () => ({ running: false, skills: 0, pendingApprovals: 0 }),
      skills: async () => [],
      pendingApprovals: async () => [],
      registrySkills: async () => [],
    },
    escalation: { endpoints: async () => [] },
    services: { onAlert: async () => () => {} },
    wallet: { deviceId: async () => null },
  },
}));
vi.mock("../../bridge/mode", () => ({ BRIDGE_MODE: "tauri", assertSimAllowed: () => {} }));
vi.mock("../../shell/slices/hermesSetup", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../shell/slices/hermesSetup")>()),
  runHermesSelfCheck: async () => ({ items: [], missingRequired: 0, firstRunDefaults: [], install: null, complete: true, error: null, checkedAt: null }),
  setupNotice: () => null,
}));

import { Store } from "../../shell/store";

// Everything else start() kicks off is stubbed, so only the order of the three steps is observed.
const STUBBED = [
  "refreshCustody",
  "refreshAuth",
  "syncLocalSkills",
  "refreshNode",
  "startNode",
  "startMemoryDaemon",
  "settleS3IfAlreadyGranted",
  "installEntitlementFocusRecheck",
  "checkNodeLiveness",
  "loadModelOptions",
  "refreshModel",
  "reconcileOnboardingFromChain",
  "custodyEnsureUnlocked",
  "resumeSession",
  "refreshWallet",
  "refreshActivity",
  "toast",
] as const;
const OBSERVED = ["syncHermesTerminal", "rebuildProvider", "followHermes"] as const;

type Spyable = Record<string, (...a: unknown[]) => unknown>;

function rigged(): { store: Store; order: string[] } {
  const store = new Store();
  const order: string[] = [];
  const s = store as unknown as Spyable;
  for (const m of STUBBED) {
    vi.spyOn(s, m).mockImplementation(() => (m === "installEntitlementFocusRecheck" || m === "toast" || m === "startNode" ? undefined : Promise.resolve()));
  }
  for (const m of OBSERVED) {
    vi.spyOn(s, m).mockImplementation(() => {
      order.push(m);
      return m === "followHermes" ? undefined : Promise.resolve();
    });
  }
  return { store, order };
}

beforeEach(() => {
  vi.useFakeTimers();
});
afterEach(() => {
  vi.clearAllTimers();
  vi.useRealTimers();
  vi.restoreAllMocks();
});

describe("CH-2: launch order in start()", () => {
  it("syncs the Hermes terminal setting, then builds the chat provider, then follows Hermes", () => {
    const { store, order } = rigged();
    store.start();
    expect(order).toEqual(["syncHermesTerminal", "rebuildProvider", "followHermes"]);
    store.stop();
  });

  it("a second start() while running does nothing", () => {
    const { store, order } = rigged();
    store.start();
    store.start();
    expect(order).toEqual(["syncHermesTerminal", "rebuildProvider", "followHermes"]);
    store.stop();
  });

  it("start() after stop() runs the launch steps again in the same order", () => {
    const { store, order } = rigged();
    store.start();
    store.stop();
    store.start();
    expect(order).toEqual(["syncHermesTerminal", "rebuildProvider", "followHermes", "syncHermesTerminal", "rebuildProvider", "followHermes"]);
    store.stop();
  });
});
