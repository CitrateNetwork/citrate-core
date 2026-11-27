// L1-WP01 CH-1 (characterization, citrate-core a826e441). LQ registry: surface "harness and
// provider selection".
//
// Pins what `Store.rebuildProvider` (src/shell/store.ts) chooses today for each combination of
// the local inference state, the member's chat route, the sidecar loop switch, whether Hermes
// answers, and a preferred cloud model: the provider kind, the no-tools reason, whether the
// Hermes status was read at all, and the chat header kind. Kinds and reasons only, never label
// copy. WP09 (chat over ACP, provider kind "acp") changes this table and names CH-1.
import { describe, it, expect, vi, beforeEach } from "vitest";

type Route = "ask_first" | "local_only";
type Cloud = { providerId: string; model: string } | null;

const h = {
  route: "ask_first" as Route,
  inference: "ready",
  running: false,
  statusError: null as string | null,
  preferred: null as Cloud,
};
const statusMock = vi.fn(async () => {
  if (h.statusError) throw new Error(h.statusError);
  return { running: h.running, skills: 0, pendingApprovals: 0 };
});

vi.mock("../../../../bridge", () => ({
  bindSimHost: () => {},
  bridge: {
    mode: "tauri",
    chat: {
      providerStatus: async () => [],
      inferenceState: async () => h.inference,
      inferLocal: async () => "a plain local reply",
      route: async () => ({ route: h.route, loadError: null }),
    },
    providers: { preferredGet: async () => ({ preferred: h.preferred, loadError: null }), status: async () => [] },
    model: { serveStart: async () => {}, serveStop: async () => {} },
    agentHarness: {
      status: () => statusMock(),
      sessionOpen: async () => "s-1",
      sessionOpenCloud: async () => "s-1",
      sessionSend: async () => {},
      sessionEvents: async () => ({ events: [], lastSeq: 0, busy: false, pendingCoreCalls: [] }),
      sessionStop: async () => {},
      sessionClose: async () => {},
      skills: async () => [],
      pendingApprovals: async () => [],
      registrySkills: async () => [],
    },
    escalation: { endpoints: async () => [] },
  },
}));
vi.mock("../../../../bridge/mode", () => ({ BRIDGE_MODE: "tauri", assertSimAllowed: () => {} }));

import { Store } from "../../../../shell/store";

const CLOUD: Cloud = { providerId: "openai", model: "gpt-x" };

interface Row {
  inference: string;
  route: Route;
  loop: boolean;
  up: boolean | "unreadable";
  cloud: Cloud;
  provider: string;
  down: "stopped" | "loop_off" | "unreadable" | null;
  read: boolean;
  header: "local" | "real" | "demo";
}

// The table at a826e441.
const ROWS: Row[] = [
  // The local model serves.
  { inference: "ready", route: "ask_first", loop: true, up: true, cloud: null, provider: "sidecar", down: null, read: true, header: "local" },
  { inference: "ready", route: "ask_first", loop: true, up: true, cloud: CLOUD, provider: "sidecar-cloud", down: null, read: true, header: "real" },
  { inference: "ready", route: "ask_first", loop: true, up: false, cloud: null, provider: "local", down: "stopped", read: true, header: "local" },
  { inference: "ready", route: "ask_first", loop: true, up: false, cloud: CLOUD, provider: "local", down: "stopped", read: true, header: "local" },
  { inference: "ready", route: "ask_first", loop: true, up: "unreadable", cloud: null, provider: "local", down: "unreadable", read: true, header: "local" },
  { inference: "ready", route: "ask_first", loop: false, up: true, cloud: null, provider: "local", down: "loop_off", read: false, header: "local" },
  { inference: "ready", route: "ask_first", loop: false, up: true, cloud: CLOUD, provider: "local", down: "loop_off", read: false, header: "local" },
  { inference: "ready", route: "local_only", loop: true, up: true, cloud: CLOUD, provider: "sidecar", down: null, read: true, header: "local" },
  { inference: "ready", route: "local_only", loop: true, up: false, cloud: null, provider: "local", down: "stopped", read: true, header: "local" },
  { inference: "ready", route: "local_only", loop: false, up: false, cloud: null, provider: "local", down: "loop_off", read: false, header: "local" },
  // A local model is set up but its server is not running.
  { inference: "local-stopped", route: "ask_first", loop: true, up: true, cloud: null, provider: "local-stopped", down: null, read: false, header: "local" },
  { inference: "local-stopped", route: "ask_first", loop: true, up: true, cloud: CLOUD, provider: "sidecar-cloud", down: null, read: true, header: "real" },
  { inference: "local-stopped", route: "ask_first", loop: true, up: false, cloud: CLOUD, provider: "local-stopped", down: null, read: true, header: "local" },
  { inference: "local-stopped", route: "local_only", loop: true, up: true, cloud: CLOUD, provider: "local-stopped", down: null, read: false, header: "local" },
  // No local model and nothing configured.
  { inference: "demo", route: "ask_first", loop: true, up: true, cloud: null, provider: "demo", down: null, read: false, header: "demo" },
  { inference: "demo", route: "local_only", loop: true, up: true, cloud: null, provider: "local-stopped", down: null, read: false, header: "local" },
  { inference: "demo", route: "ask_first", loop: true, up: true, cloud: CLOUD, provider: "sidecar-cloud", down: null, read: true, header: "real" },
];

function label(r: Row): string {
  return `${r.inference} ${r.route} loop=${r.loop} up=${String(r.up)} cloud=${r.cloud ? "yes" : "no"}`;
}

beforeEach(() => {
  localStorage.clear();
  statusMock.mockClear();
});

describe("CH-1: rebuildProvider chooses the same provider for the same inputs", () => {
  for (const row of ROWS) {
    it(label(row), async () => {
      h.route = row.route;
      h.inference = row.inference;
      h.running = row.up === true;
      h.statusError = row.up === "unreadable" ? "hermes control: connection refused" : null;
      h.preferred = row.cloud;
      const store = new Store();
      vi.spyOn(store, "toast").mockImplementation(() => {});
      store.setState({ chatMsgs: [], chatStatus: "ready", aiDefault: "gateway", hermesSidecarLoop: row.loop });
      statusMock.mockClear();
      await store.rebuildProvider();
      const down = (store as unknown as { sidecarDown: { why: string } | null }).sidecarDown;
      expect({
        provider: store.provider?.kind,
        down: down?.why ?? null,
        read: statusMock.mock.calls.length > 0,
        header: store.state.chatProviderKind,
      }).toEqual({ provider: row.provider, down: row.down, read: row.read, header: row.header });
    });
  }

  it("the table covers every provider kind rebuildProvider builds today", () => {
    const kinds = new Set(ROWS.map((r) => r.provider));
    expect([...kinds].sort()).toEqual(["demo", "local", "local-stopped", "sidecar", "sidecar-cloud"]);
  });
});
