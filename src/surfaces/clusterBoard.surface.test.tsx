// 0.5.4 FL design, board 04 (Groups › Cluster): one test per StateMatrix "Cluster" state, rendered
// against the real slices with the bridge and the fleet commands stubbed. Every value on screen comes
// from a stubbed source; nothing is drawn that the source did not say.
import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Cluster, NOT_REPORTED_REASON } from "./Cluster";
import { bridge } from "../bridge";
import { freshState } from "../shell/state";
import { clusterSlice } from "../shell/slices/cluster";
import { groupsSlice } from "../shell/slices/groups";
import { sampleReport } from "../shell/slices/tierTestReport";
import type { Store } from "../shell/store";
import type { ClusterPeer, Group, MeshStatus } from "../bridge/domains";
import type { FleetApi, FleetDevice } from "../bridge/tauri/fleet";
import { NO_ROUNDS_REASON, GIB } from "./clusterBoard";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const GROUP: Group = { id: "g-ol", name: "Orchard Lab", owner: "0x1", kind: "forum", members: [] };
const MESH_ON: MeshStatus = { on: true, source: "default", signedOff: true, note: "On after sign-off." };
const MESH_OFF: MeshStatus = { on: false, source: "off", signedOff: true, note: "Off on this machine." };
const UNSIGNED: MeshStatus = { on: false, source: "off", signedOff: false, note: "Waiting for sign-off." };

let host: HTMLDivElement;
let root: Root;

function fleet(over: Partial<FleetApi> = {}): FleetApi {
  return {
    probe: async () => ({ device: { deviceId: "d", label: "Studio", tier: "T2", role: "heavy" }, tier: sampleReport({ facts: { os: "linux", arch: "x86_64", totalRamBytes: 128 * GIB, unifiedMemory: false, gpuVramBytes: 24 * GIB, diskFreeBytes: null } }) }),
    setLabel: vi.fn(),
    roster: async () => [] as FleetDevice[],
    setDiscovery: async (enabled) => ({ enabled }),
    browse: async () => [],
    createLink: async () => ({ link: "citrate://pair?c=abc", qr: { size: 2, rows: ["10", "01"] }, expiresAt: Date.now() / 1000 + 600, hints: [] }),
    inspectLink: vi.fn(),
    joinLink: vi.fn(),
    tailscale: vi.fn(),
    ...over,
  } as FleetApi;
}

function stubBridge(peers: ClusterPeer[], mesh: MeshStatus) {
  vi.spyOn(bridge.groups, "list").mockResolvedValue([GROUP]);
  vi.spyOn(bridge.groups, "messages").mockResolvedValue([]);
  vi.spyOn(bridge.cluster, "status").mockResolvedValue({ groupId: GROUP.id, online: peers.filter((p) => p.online).length, total: peers.length, sharedFiles: [] });
  vi.spyOn(bridge.cluster, "peers").mockResolvedValue(peers);
  vi.spyOn(bridge.cluster, "devices").mockResolvedValue([]);
  vi.spyOn(bridge.cluster, "myDevices").mockResolvedValue({ thisDevice: null, links: [], revoked: [] });
  vi.spyOn(bridge.cluster, "meshStatus").mockResolvedValue(mesh);
  vi.spyOn(bridge.cluster, "deviceLinksShareOffer").mockResolvedValue(null);
}

async function mount(api: FleetApi, desktop = true) {
  const store = { toast: () => {}, linkThisDevice: vi.fn(), clearPendingPairLink: vi.fn() } as unknown as Store;
  await act(async () => root.render(<Cluster store={store} s={freshState("p1")} fleet={api} desktop={desktop} />));
  // Let the loads settle.
  for (let i = 0; i < 4; i++) await act(async () => new Promise((r) => setTimeout(r, 0)));
}

const text = () => host.textContent ?? "";
const byTest = (id: string) => host.querySelector(`[data-testid="${id}"]`);
const button = (label: string) => [...document.querySelectorAll("button")].find((b) => b.textContent === label) as HTMLButtonElement | undefined;

beforeEach(() => {
  vi.restoreAllMocks();
  groupsSlice.set({ groups: [GROUP], selectedId: GROUP.id });
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  groupsSlice.set({ groups: [], selectedId: null });
  clusterSlice.set({ groups: [], selectedId: null, peers: [], status: null, mesh: null, memberDevices: [], error: null });
});

describe("Groups › Cluster board (StateMatrix: Cluster)", () => {
  it("follows the group open in Groups and titles it", async () => {
    stubBridge([], MESH_ON);
    await mount(fleet());
    expect(host.querySelector("h1")?.textContent).toBe("Orchard Lab cluster");
  });

  it("Empty: one machine, no peers yet", async () => {
    stubBridge([], MESH_ON);
    await mount(fleet());
    expect(byTest("cluster-empty")?.textContent).toContain("One machine, no peers yet");
    expect(host.querySelector("svg[role=img]")?.getAttribute("aria-label")).toBe("Cluster map: 1 device, this machine, no peers yet.");
  });

  it("Running: mesh on draws solid and dashed links and the switch is usable", async () => {
    stubBridge([{ address: "0xaa", online: true, link: "direct" }, { address: "0xbb", online: true, link: "relayed" }], MESH_ON);
    await mount(fleet());
    expect(host.querySelector('[data-edge="0xaa"]')?.getAttribute("data-style")).toBe("solid");
    expect(host.querySelector('[data-edge="0xbb"]')?.getAttribute("data-style")).toBe("dashed");
    const sw = byTest("cluster-mesh-switch") as HTMLInputElement;
    expect(sw.checked).toBe(true);
    expect(sw.disabled).toBe(false);
    expect(byTest("cluster-mesh-off")).toBeNull();
  });

  it("Failure: mesh off says so and draws no live edges", async () => {
    stubBridge([{ address: "0xaa", online: true, link: "direct" }], MESH_OFF);
    await mount(fleet());
    expect(byTest("cluster-mesh-off")?.textContent).toContain("Mesh off on this device. Other machines can't reach it");
    expect(host.querySelector("[data-edge]")).toBeNull();
  });

  it("the mesh switch is never a fake toggle: before sign-off it is disabled with its reason", async () => {
    stubBridge([], UNSIGNED);
    await mount(fleet());
    const sw = byTest("cluster-mesh-switch") as HTMLInputElement;
    expect(sw.disabled).toBe(true);
    expect(text()).toContain("once its security review is signed off");
  });

  it("Failure: probe failed names the reason and offers the probe again", async () => {
    stubBridge([], MESH_ON);
    let calls = 0;
    await mount(fleet({ probe: async () => { calls++; throw new Error("driver did not answer in 10 s"); } }));
    const e = byTest("cluster-eligibility")!;
    expect(e.textContent).toContain("Not known");
    expect(e.textContent).toContain("The hardware probe failed: driver did not answer in 10 s");
    expect(byTest("cap-accel")?.textContent).toContain("—");
    await act(async () => button("Run the probe again")!.click());
    expect(calls).toBe(2);
  });

  it("Unavailable: members' machines are dotted and their hardware is — with the reason", async () => {
    stubBridge([{ address: "0xaa", online: true, link: "direct" }], MESH_ON);
    await mount(fleet());
    expect(host.querySelector('[data-node="0xaa"]')?.getAttribute("data-reported")).toBe("no");
    expect(byTest("cap-not-reported")?.textContent).toContain("1 not reported");
    const peerBtn = [...host.querySelectorAll('[role=group] button')].find((b) => b.textContent?.startsWith("0xaa")) as HTMLButtonElement;
    await act(async () => peerBtn.click());
    expect(peerBtn.getAttribute("aria-pressed")).toBe("true");
    expect(byTest("cluster-selected")?.textContent).toContain(NOT_REPORTED_REASON);
  });

  it("Today: eligibility is Not known because Group rounds are not live, with this machine's receipt", async () => {
    stubBridge([], MESH_ON);
    await mount(fleet());
    const e = byTest("cluster-eligibility")!;
    expect(e.getAttribute("data-kind")).toBe("unknown");
    expect(e.textContent).toContain(NO_ROUNDS_REASON);
    expect(byTest("cap-reported")?.textContent).toContain("1");
    expect(byTest("cap-accel")?.textContent).toContain("24 GB");
    expect(byTest("cluster-selected")?.textContent).toContain("GPU, 24 GB memory, 128 GB RAM");
  });

  it("0.6.0 fixture: a round's requirements give Eligible or a why-not per miss", async () => {
    stubBridge([], MESH_ON);
    const store = { toast: () => {}, linkThisDevice: vi.fn(), clearPendingPairLink: vi.fn() } as unknown as Store;
    await act(async () => root.render(<Cluster store={store} s={freshState("p1")} fleet={fleet()} desktop requirements={{ round: "round 16", minAcceleratorBytes: 48 * GIB }} />));
    for (let i = 0; i < 4; i++) await act(async () => new Promise((r) => setTimeout(r, 0)));
    expect(byTest("cluster-eligibility")?.textContent).toContain("needs 48 GB GPU memory for round 16 (this machine has 24 GB)");
  });

  it("outside the desktop app, Add a machine is off and says why", async () => {
    stubBridge([], MESH_ON);
    await mount(fleet(), false);
    expect(button("Add a machine")?.disabled).toBe(true);
    expect(text()).toContain("Adding a machine needs the desktop app.");
  });

  it("copy follows the rules: no exclamation marks, no em dashes in sentences, no earn", async () => {
    stubBridge([{ address: "0xaa", online: false }], MESH_OFF);
    await mount(fleet());
    expect(text()).not.toMatch(/!|\bearn|users\b|USD/i);
    expect(text()).not.toMatch(/\w —|— \w(?!\(not)/);
  });
});
