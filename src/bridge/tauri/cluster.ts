// CX bridge impl — cluster (C-20), TAURI. Owned by lane s4 (CX-S4).
//
// CL-S2: the cluster is now DAEMON-BACKED. citrate-core spawns the citrate-cluster sidecar and the
// `cluster_*` commands route over its UDS socket — the daemon holds the membership (admission via
// cluster-core) + the co-pinned shared-file set, feeding on the group roster (from the comms daemon).
// status/peers return REAL daemon state; `online` reflects live mesh connectivity (0 until peers are
// actually connected — no fabricated peers, Rule 1). shareFile announces a co-pin over the mesh.
import { invoke } from "./invoke";
import type {
  ClusterDomain,
  ClusterMemberDevices,
  ClusterPeer,
  ClusterStatus,
  DeviceLinks,
  DeviceRevokePrepared,
} from "../domains";
import type { CeremonyView } from "../types";

export const tauriCluster: ClusterDomain = {
  status(groupId): Promise<ClusterStatus> {
    return invoke<ClusterStatus>("cluster_status", { group: groupId });
  },
  peers(groupId): Promise<ClusterPeer[]> {
    return invoke<ClusterPeer[]>("cluster_peers", { group: groupId });
  },
  async join(groupId) {
    await invoke("cluster_join", { group: groupId });
  },
  async shareFile(groupId, cid) {
    await invoke("cluster_share_file", { group: groupId, cid });
  },
  async leave(groupId) {
    await invoke("cluster_leave", { group: groupId });
  },
  // HUP-S8.1 — per-device keys + DeviceLink. The wallet signature goes through the ceremony
  // (linkDeviceRequest opens it, linkDeviceApprove completes it after the person approves).
  devices(groupId): Promise<ClusterMemberDevices[]> {
    return invoke<ClusterMemberDevices[]>("cluster_devices", { group: groupId });
  },
  myDevices(): Promise<DeviceLinks> {
    return invoke<DeviceLinks>("device_links");
  },
  linkDeviceRequest(label): Promise<CeremonyView> {
    return invoke<CeremonyView>("device_link_request", { label });
  },
  linkDeviceApprove(id, rawAck): Promise<DeviceLinks> {
    return invoke<DeviceLinks>("device_link_approve", { id, rawAck });
  },
  async linkDeviceReject(id) {
    await invoke("device_link_reject", { id });
  },
  revokeDevicePrepare(device): Promise<DeviceRevokePrepared> {
    return invoke<DeviceRevokePrepared>("device_link_revoke_prepare", { device });
  },
  revokeDevice(confirmId): Promise<DeviceLinks> {
    return invoke<DeviceLinks>("device_link_revoke", { confirmId });
  },
  exportDeviceLink(): Promise<string> {
    return invoke<string>("device_link_export");
  },
  importDeviceLink(code): Promise<DeviceLinks> {
    return invoke<DeviceLinks>("device_link_import", { code });
  },
};
