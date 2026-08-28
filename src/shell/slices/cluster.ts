// =====================================================================
// citrate-core — cluster slice (CX-S4.1, lane s4)
//
// State for the Cluster surface: pick one of your Groups, and see its cluster — the peer set is the
// group roster (the RBAC→network boundary; the canonical derivation is Rust `cluster::allowed_peers`).
// Actions call bridge.cluster (which composes the view from groups_roster) + bridge.groups.list for
// the picker. Errors are CAUGHT into `error`, never thrown at render (Rule 1) — an un-provisioned
// daemon surfaces honestly. Connectivity + shared files are S4.2 (libp2p), so peers read `online:false`.
// Owned entirely by lane s4.
// =====================================================================
import { createSlice } from "./createSlice";
import { bridge } from "../../bridge";
import type { ClusterPeer, ClusterStatus, Group } from "../../bridge/domains";

export interface ClusterState {
  /** Your groups, for the picker. */
  groups: Group[];
  /** The selected group's id, or null. */
  selectedId: string | null;
  /** The selected group's cluster status, or null. */
  status: ClusterStatus | null;
  /** The selected group's cluster peers (the authorized set; online is S4.2). */
  peers: ClusterPeer[];
  /** A load is in flight. */
  loading: boolean;
  /** The last user-facing error, or null when clear. */
  error: string | null;
}

const initial: ClusterState = {
  groups: [],
  selectedId: null,
  status: null,
  peers: [],
  loading: false,
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

/** The display label for a group. A short id for now; the DTO `name` field lands with the S0
 *  amendment (#163) and can replace this once merged. */
export function clusterGroupLabel(g: Group): string {
  return `${g.id.slice(0, 10)}…`;
}
