// K1a-2 review (#509): Node › Services controls run through each service's owner path, and that
// path's answer reaches the row.
// 1. A Failed service can be recovered from the tab: Reset runs the owner's stop, which releases
//    the failed supervisor, and only then is Start offered.
// 2. A refused Start, Stop or Reset resolves to the refusal reason (the owner paths used to swallow
//    it into a toast or slice state), and the tab shows it on the row.
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import type { ServiceEntry, ServicesSnapshot } from "../bridge/domains";

const m = vi.hoisted(() => ({
  nodeStart: vi.fn(async () => {}),
  nodeStop: vi.fn(async () => {}),
  memStart: vi.fn(async () => {}),
  memStop: vi.fn(async () => {}),
  hermesStart: vi.fn(async () => {}),
  hermesStop: vi.fn(async () => {}),
  hermesStatus: vi.fn(async () => ({ running: false, skills: 0, pendingApprovals: 0 })),
  snapshot: vi.fn(async (): Promise<ServicesSnapshot> => ({ takenAtUnixMs: 1, services: [] })),
}));

vi.mock("../bridge", () => ({
  bindSimHost: () => {},
  isUnavailable: () => false,
  bridge: {
    mode: "tauri",
    node: {
      start: () => m.nodeStart(),
      stop: () => m.nodeStop(),
      startIpfs: async () => {},
      status: async () => ({ state: "stopped", height: 0, peers: 0, syncPct: 0 }),
      logs: async () => [],
    },
    memory: {
      start: () => m.memStart(),
      stop: () => m.memStop(),
      status: async () => ({ state: "stopped", socketPath: null, semantic: false }),
    },
    agentHarness: {
      start: () => m.hermesStart(),
      stop: () => m.hermesStop(),
      status: () => m.hermesStatus(),
      skills: async () => [],
      pendingApprovals: async () => [],
      registrySkills: async () => [],
    },
    flRounds: { trajectoryStatus: async () => Promise.reject(new Error("not in this test")) },
    services: { snapshot: () => m.snapshot() },
  },
}));
vi.mock("../bridge/mode", () => ({ BRIDGE_MODE: "tauri", assertSimAllowed: () => {} }));
vi.mock("../bridge/tauri/invoke", async (orig) => ({
  ...(await orig<typeof import("../bridge/tauri/invoke")>()),
  invoke: vi.fn(async () => Promise.reject(new Error("not in this test"))),
}));

import { Store } from "../shell/store";
import { agentSlice } from "../shell/slices/agent";
import { NodeServices, SERVICE_CONTROLS } from "./NodeServices";
import { freshState } from "../shell/state";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function storeOff(): Store {
  const store = new Store();
  store.setState({ node: "off", nodeIntent: "stopped" });
  vi.spyOn(store, "importKnowledge").mockResolvedValue();
  return store;
}

function hermesOff() {
  agentSlice.set({ busy: false, error: null, attached: agentSlice.get().attached.map((r) => (r.id === "hermes" ? { ...r, state: "off", error: null } : r)) });
}

beforeEach(() => {
  for (const f of Object.values(m)) f.mockReset();
  m.hermesStatus.mockResolvedValue({ running: false, skills: 0, pendingApprovals: 0 });
  hermesOff();
});

describe("owner paths resolve to their refusal reason (null when nothing refused)", () => {
  it("node: a refused Start resolves to the owner's reason; an accepted one to null", async () => {
    const c = SERVICE_CONTROLS.node!(storeOff());
    m.nodeStart.mockRejectedValueOnce("the node binary is not bundled");
    await expect(c.start()).resolves.toBe("the node binary is not bundled");
    m.nodeStart.mockResolvedValueOnce(undefined);
    await expect(SERVICE_CONTROLS.node!(storeOff()).start()).resolves.toBeNull();
  });

  it("node: a Start refused because a supervisor still holds the slot is a refusal, not a silent success", async () => {
    m.nodeStart.mockRejectedValueOnce("node already running");
    await expect(SERVICE_CONTROLS.node!(storeOff()).start()).resolves.toBe("node already running");
  });

  it("node: Reset runs the owner's stop (it releases the failed supervisor) and a refused one says why", async () => {
    const c = SERVICE_CONTROLS.node!(storeOff());
    expect(typeof c.reset).toBe("function");
    m.nodeStop.mockResolvedValueOnce(undefined);
    await expect(c.reset()).resolves.toBeNull();
    expect(m.nodeStop).toHaveBeenCalledTimes(1);
    m.nodeStop.mockRejectedValueOnce("the supervisor is shutting down");
    await expect(c.stop()).resolves.toBe("the supervisor is shutting down");
  });

  it("memory: a refused Start and a refused Stop resolve to their reasons", async () => {
    const c = SERVICE_CONTROLS.memory!(storeOff());
    m.memStart.mockRejectedValueOnce(new Error("a memory backup or restore is in progress"));
    await expect(c.start()).resolves.toBe("a memory backup or restore is in progress");
    m.memStop.mockRejectedValueOnce(new Error("stop failed"));
    await expect(c.reset()).resolves.toBe("stop failed");
  });

  it("Hermes: a refused Start and a refused Stop resolve to their reasons", async () => {
    const c = SERVICE_CONTROLS.hermes!(storeOff());
    m.hermesStart.mockRejectedValueOnce(new Error("port 4711 is in use"));
    await expect(c.start()).resolves.toBe("port 4711 is in use");
    hermesOff();
    m.hermesStop.mockRejectedValueOnce(new Error("stop failed"));
    await expect(c.stop()).resolves.toBe("stop failed");
  });
});

describe("the tab shows a refusal on the row", () => {
  let root: Root | null = null;
  let host: HTMLDivElement | null = null;
  afterEach(() => {
    act(() => root?.unmount());
    host?.remove();
    root = null;
    host = null;
  });

  const nodeRow = (over: Partial<ServiceEntry>): ServiceEntry => ({
    id: "node",
    label: "Node",
    owner: "src-tauri/src/node.rs",
    prerequisites: ["the node binary is bundled"],
    probe: "Process exit only",
    readySignal: "None",
    logBudgetLines: 500,
    logsCommand: "node_logs",
    controls: true,
    observation: "notSupervised",
    phase: null,
    readiness: null,
    lastTransition: null,
    restarts: null,
    lastCrash: null,
    quarantine: null,
    ...over,
  });

  async function mount(store: Store) {
    host = document.createElement("div");
    document.body.appendChild(host);
    root = createRoot(host);
    await act(async () => {
      root?.render(<NodeServices store={store} s={freshState("p1")} />);
    });
  }

  async function click(control: string) {
    const btn = host?.querySelector(`[data-control="${control}"]`) as HTMLButtonElement | null;
    expect(btn, `${control} offered`).not.toBeNull();
    await act(async () => {
      btn?.click();
    });
    await act(async () => {
      await new Promise((r) => setTimeout(r, 0));
    });
  }

  it("a refused Start shows the owner's reason on the node row", async () => {
    m.snapshot.mockResolvedValue({ takenAtUnixMs: 1, services: [nodeRow({})] });
    m.nodeStart.mockRejectedValueOnce("the node binary is not bundled");
    await mount(storeOff());
    await click("start");
    const row = host?.querySelector('[data-service="node"]');
    expect(row?.querySelector('[role="alert"]')?.textContent).toBe("Start refused: the node binary is not bundled");
  });

  it("a Failed node is recovered from the tab: Reset releases it, then Start is offered", async () => {
    m.snapshot.mockResolvedValue({
      takenAtUnixMs: 1,
      services: [nodeRow({ observation: "observed", phase: "failed", readiness: "notAssessed", restarts: 9 })],
    });
    await mount(storeOff());
    // The owner's stop releases the supervisor; the next snapshot has no supervisor in the slot.
    m.nodeStop.mockImplementationOnce(async () => {
      m.snapshot.mockResolvedValue({ takenAtUnixMs: 2, services: [nodeRow({})] });
    });
    await click("reset");
    expect(m.nodeStop).toHaveBeenCalledTimes(1);
    expect(host?.querySelector('[data-service="node"] [role="alert"]')).toBeNull();
    expect(host?.querySelector('[data-service="node"] [data-control="start"]')).not.toBeNull();
  });
});
