// =====================================================================
// citrate-core — cluster slice (CX-S4.1 + Pass-1 redesign, lane s4)
//
// State for a group's cluster — the peer set is the group roster (the RBAC→network boundary;
// canonical derivation is Rust `cluster::allowed_peers`). Actions call bridge.cluster (join /
// leave / shareFile) + bridge.storage.list (so a member shares a FILE they already have, not a
// raw CID they had to find). Errors are CAUGHT into `error`, never thrown at render (Rule 1) — an
// un-provisioned daemon surfaces honestly. `joined` is a session truth for the join/leave toggle;
// live connectivity (peers.online) is real daemon state. Owned by lane s4.
// =====================================================================
import { createSlice } from "./createSlice";
import { bridge } from "../../bridge";
import type {
  ClusterMemberDevices,
  ClusterPeer,
  ClusterStatus,
  DeviceLinks,
  DeviceRevokePrepared,
  Group,
  MeshStatus,
  PinRow,
} from "../../bridge/domains";
import { groupsSlice, reloadMessages } from "./groups";
import { ingestDeviceLinkMessages, shareDeviceLinks } from "../../fleet/deviceLinkShare";

export interface ClusterState {
  /** Your groups, for the picker. */
  groups: Group[];
  /** The selected group's id, or null. */
  selectedId: string | null;
  /** The selected group's cluster status, or null. */
  status: ClusterStatus | null;
  /** The selected group's cluster peers (the authorized set; online is live). */
  peers: ClusterPeer[];
  /** Your own pinned files — the source for "share a file with the group". */
  myFiles: PinRow[];
  /** Session set of clusters you have joined (drives the join/leave view). */
  joined: string[];
  /** A load is in flight. */
  loading: boolean;
  /** A join is in flight. */
  joining: boolean;
  /** A shareFile is in flight (the cid), or null. */
  sharing: string | null;
  /** The last user-facing error, or null when clear. */
  error: string | null;
  /** HUP-S8.1: this machine's device key + the device links it knows (null until loaded). */
  myDevices: DeviceLinks | null;
  /** HUP-S8.1: the selected group's members with their linked devices (live). */
  memberDevices: ClusterMemberDevices[];
  /** HUP-S8.1: a device revoke is in flight (the device address), or null. */
  revoking: string | null;
  /** HUP-S8.4 prep: whether the cross-machine mesh transport is on, and why (null until loaded). */
  mesh: MeshStatus | null;
  /** A removal core prepared and the member has not confirmed yet. */
  revokePrepared: DeviceRevokePrepared | null;
}

const initial: ClusterState = {
  groups: [],
  selectedId: null,
  status: null,
  peers: [],
  myFiles: [],
  joined: [],
  loading: false,
  joining: false,
  sharing: null,
  error: null,
  myDevices: null,
  memberDevices: [],
  revoking: null,
  mesh: null,
  revokePrepared: null,
};

export const clusterSlice = createSlice<ClusterState>(initial);

const message = (e: unknown): string =>
  e instanceof Error ? e.message : typeof e === "string" ? e : String(e);

/** Load your groups (the cluster picker). Honest-empty on a sim / un-provisioned bridge. */
export async function loadClusterGroups(): Promise<void> {
  try {
    const groups = await bridge.groups.list();
    clusterSlice.set({ groups, error: null });
  } catch (e) {
    clusterSlice.set({ error: message(e) });
  }
}

/** Select a group and load its cluster status + peers (the authorized member set). */
export async function selectClusterGroup(groupId: string): Promise<void> {
  clusterSlice.set({ selectedId: groupId, status: null, peers: [], loading: true, error: null });
  try {
    const [status, peers] = await Promise.all([
      bridge.cluster.status(groupId),
      bridge.cluster.peers(groupId),
    ]);
    if (clusterSlice.get().selectedId === groupId) {
      clusterSlice.set({ status, peers, loading: false });
    }
  } catch (e) {
    if (clusterSlice.get().selectedId === groupId) {
      clusterSlice.set({ loading: false, error: message(e) });
    }
  }
}

/**
 * HUP-S8.1 — load this machine's device key address and the links it knows. Never mints a key.
 * Errors are caught into `error` (an un-provisioned keyring surfaces honestly).
 */
export async function loadMyDevices(): Promise<void> {
  try {
    const myDevices = await bridge.cluster.myDevices();
    clusterSlice.set({ myDevices });
  } catch (e) {
    clusterSlice.set({ error: message(e) });
  }
}

/**
 * HUP-S8.1 — load the selected group's members with their devices. A daemon that predates device
 * links has no `devices` op: that is not an error for the rest of the surface, so it reads as an
 * empty list (the peers list still shows every authorized address).
 */
export async function loadMemberDevices(groupId: string): Promise<void> {
  try {
    const memberDevices = await bridge.cluster.devices(groupId);
    if (clusterSlice.get().selectedId === groupId) clusterSlice.set({ memberDevices });
  } catch {
    if (clusterSlice.get().selectedId === groupId) clusterSlice.set({ memberDevices: [] });
  }
}

/** HUP-S8.1 — step 1 of removing one of your devices: core mints the confirmation for it. */
export async function prepareRevokeDevice(device: string): Promise<void> {
  clusterSlice.set({ error: null, revokePrepared: null });
  try {
    const revokePrepared = await bridge.cluster.revokeDevicePrepare(device);
    clusterSlice.set({ revokePrepared });
  } catch (e) {
    clusterSlice.set({ error: message(e) });
  }
}

/** Drop a prepared removal without revoking anything. */
export function cancelRevokeDevice(): void {
  clusterSlice.set({ revokePrepared: null });
}

/** HUP-S8.1 — you confirmed: revoke the prepared device (permanent for that device key), then refresh. */
export async function revokeMyDevice(): Promise<void> {
  const prepared = clusterSlice.get().revokePrepared;
  if (!prepared) return;
  clusterSlice.set({ revoking: prepared.device, error: null, revokePrepared: null });
  try {
    const myDevices = await bridge.cluster.revokeDevice(prepared.confirmId);
    clusterSlice.set({ myDevices, revoking: null });
    // Tell the other members of your groups, so their nodes drop the device too.
    void shareMyDeviceLinks();
    const id = clusterSlice.get().selectedId;
    if (id) await Promise.all([selectClusterGroup(id), loadMemberDevices(id)]);
  } catch (e) {
    clusterSlice.set({ revoking: null, error: message(e) });
  }
}

/**
 * HUP-S8.1 — add another of your own devices from the code it showed (pairing by copy/paste until
 * the fleet wizard's QR pairing lands). Core verifies all three signatures and that the device is
 * yours before storing it; the mesh picks it up on the next roster update.
 */
export async function importDeviceCode(code: string): Promise<boolean> {
  clusterSlice.set({ error: null });
  try {
    const myDevices = await bridge.cluster.importDeviceLink(code.trim());
    clusterSlice.set({ myDevices });
    void shareMyDeviceLinks();
    const id = clusterSlice.get().selectedId;
    if (id) await loadMemberDevices(id);
    return true;
  } catch (e) {
    clusterSlice.set({ error: message(e) });
    return false;
  }
}

/** HUP-S8.1 — this machine's link code (to paste on another of your devices), or null + error. */
export async function exportDeviceCode(): Promise<string | null> {
  try {
    return await bridge.cluster.exportDeviceLink();
  } catch (e) {
    clusterSlice.set({ error: message(e) });
    return null;
  }
}

/**
 * HUP-S8.1 follow-on: share your device links and revocations with every group you are in (a hidden
 * control message over the end-to-end encrypted relay). Core sends nothing when you never linked a
 * device, and nothing to a group that already has this exact set. Best effort; never throws.
 */
export async function shareMyDeviceLinks(): Promise<number> {
  try {
    const groups = await bridge.groups.list();
    return await shareDeviceLinks(
      groups.map((g) => g.id),
      {
        offer: (g) => bridge.cluster.deviceLinksShareOffer(g),
        mark: (g, d) => bridge.cluster.deviceLinksMarkShared(g, d),
        send: (g, body) => bridge.groups.send(g, body),
      },
    );
  } catch {
    return 0;
  }
}

/**
 * HUP-S8.1 follow-on: pick up other members' device links for a group. Drains the group's mailbox
 * into its retained history (the same drain the Groups chat does, so no message is lost), then hands
 * the newest share of each member to core, which verifies it. Best effort; never throws.
 */
export async function syncPeerDeviceLinks(groupId: string): Promise<{ links: number; revocations: number }> {
  try {
    await reloadMessages(groupId);
    const history = groupsSlice.get().history[groupId] ?? [];
    return await ingestDeviceLinkMessages(history, (sender, body) => bridge.cluster.deviceLinksIngest(sender, body));
  } catch {
    return { links: 0, revocations: 0 };
  }
}

/** HUP-S8.4 prep: load the mesh transport status (honest: off until the operator or sign-off). */
export async function loadMeshStatus(): Promise<void> {
  try {
    clusterSlice.set({ mesh: await bridge.cluster.meshStatus() });
  } catch {
    clusterSlice.set({ mesh: null });
  }
}

/** Reload just the status + peers for the current selection. */
export async function reloadCluster(): Promise<void> {
  const id = clusterSlice.get().selectedId;
  if (id) await selectClusterGroup(id);
}

/** Load your own pinned files — used to share a file with the group without typing a CID. */
export async function loadMyFiles(): Promise<void> {
  try {
    const myFiles = await bridge.storage.list();
    clusterSlice.set({ myFiles, error: null });
  } catch (e) {
    // Honest: an un-provisioned kubo surfaces here; the picker shows its error, not a fake list.
    clusterSlice.set({ error: message(e) });
  }
}

/** Join a group's cluster — contributes your storage and keeps you in sync. */
export async function joinCluster(groupId: string): Promise<void> {
  clusterSlice.set({ joining: true, error: null });
  try {
    await bridge.cluster.join(groupId);
    clusterSlice.set((s) => ({ joining: false, joined: s.joined.includes(groupId) ? s.joined : [...s.joined, groupId] }));
    await selectClusterGroup(groupId);
  } catch (e) {
    clusterSlice.set({ joining: false, error: message(e) });
  }
}

/** Leave a group's cluster. */
export async function leaveCluster(groupId: string): Promise<void> {
  clusterSlice.set({ error: null });
  try {
    await bridge.cluster.leave(groupId);
    clusterSlice.set((s) => ({ joined: s.joined.filter((id) => id !== groupId) }));
    await selectClusterGroup(groupId);
  } catch (e) {
    clusterSlice.set({ error: message(e) });
  }
}

/**
 * Share a file the user already has with the group (co-pin its CID across the roster). The caller
 * passes a CID resolved from a picked/added file — never a hand-typed one for a normal user.
 */
export async function shareClusterFile(groupId: string, cid: string): Promise<void> {
  if (!cid) return;
  clusterSlice.set({ sharing: cid, error: null });
  try {
    await bridge.cluster.shareFile(groupId, cid);
    clusterSlice.set({ sharing: null });
    await selectClusterGroup(groupId);
  } catch (e) {
    clusterSlice.set({ sharing: null, error: message(e) });
  }
}

/**
 * Add a local file to IPFS, then share it with the group — the "Browse / drop a file" path so a
 * member never has to know what a CID is. Tauri-only (needs a real filesystem path); returns the
 * new CID or throws honestly.
 */
export async function addAndShareFile(groupId: string, path: string): Promise<void> {
  clusterSlice.set({ sharing: path, error: null });
  try {
    const { cid } = await bridge.storage.add(path);
    await bridge.cluster.shareFile(groupId, cid);
    clusterSlice.set({ sharing: null });
    await Promise.all([loadMyFiles(), selectClusterGroup(groupId)]);
  } catch (e) {
    clusterSlice.set({ sharing: null, error: message(e) });
  }
}

/** Whether the user has joined the given cluster this session. */
export function isJoined(state: ClusterState, groupId: string | null): boolean {
  return !!groupId && state.joined.includes(groupId);
}

/** The display label for a group — its name if present, else a short id. */
export function clusterGroupLabel(g: Group): string {
  return g.name || `${g.id.slice(0, 10)}…`;
}
