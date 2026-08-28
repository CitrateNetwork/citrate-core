// CX bridge impl — cluster (C-20), TAURI. Owned by lane s4 (CX-S4).
//
// S4.1: the cluster's authorized peer set IS the group roster (the RBAC→network boundary — Rust
// `cluster::allowed_peers`). status/peers compose that view from the existing `groups_roster`
// command; every peer is reported `online: false` and `status.online: 0` because the live P2P
// transport (dialing, gossipsub connectivity, shared files) is S4.2 — we never fake a live peer
// (Rule 1). join/shareFile/leave invoke the Rust cluster commands, which honestly report "not wired
// yet (CX-S4.2 libp2p transport)".
import { invoke } from "@tauri-apps/api/core";
import type { ClusterDomain, ClusterPeer, ClusterStatus } from "../domains";

type Pair = [string, string]; // (address, role) from groups_roster

export const tauriCluster: ClusterDomain = {
  async status(groupId): Promise<ClusterStatus> {
    const roster = await invoke<Pair[]>("groups_roster", { group: groupId });
    // total = the authorized set; online = 0 until the S4.2 transport reports real connectivity.
    return { groupId, online: 0, total: roster.length, sharedFiles: [] };
  },
  async peers(groupId): Promise<ClusterPeer[]> {
    const roster = await invoke<Pair[]>("groups_roster", { group: groupId });
    return roster.map(([address]) => ({ address, online: false }));
  },
  async join(groupId) {
    // The mesh membership is derived from the roster (you're already a member if you're in the
    // group); actually dialing the mesh is S4.2. Honest error from the Rust command.
    await invoke("cluster_join", { group: groupId });
  },
  async shareFile(groupId, cid) {
    await invoke("cluster_share_file", { group: groupId, cid });
  },
  async leave(groupId) {
    await invoke("cluster_leave", { group: groupId });
  },
};
