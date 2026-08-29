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
import type { ClusterPeer, ClusterStatus, Group, PinRow } from "../../bridge/domains";

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
