// citrate-core: bridge domain `cluster`, moved verbatim from src/bridge/domains.ts by L0-WP05
// (planset v060). src/bridge/domains.ts re-exports it, so imports from "../bridge/domains" keep
// working. The owning lane is the row for this path in .agentile/v60-ownership.map.

import type { CeremonyView } from "../types";

// ── C-20 group clusters: private P2P + shared files/compute (lane s4) ──
export interface ClusterPeer {
  address: string;
  online: boolean;
  /** HUP-S8.1: set when this peer is a linked device; the member it acts for. */
  member?: string;
  /** V60 MESH-4: how an online peer is reached, when the cluster daemon reports it. */
  link?: "direct" | "relayed";
}
/** HUP-S8.1: one linked device under a member, as the cluster daemon admits it. */
export interface ClusterDevice {
  device: string;
  index: number;
  label: string;
  issuedAt: number;
  online: boolean;
}
/** HUP-S8.1: a cluster member with its linked devices. */
export interface ClusterMemberDevices {
  member: string;
  role: string;
  online: boolean;
  devices: ClusterDevice[];
}
/** HUP-S8.1: a DeviceLink this machine knows (no signatures cross the bridge). */
export interface DeviceLinkView {
  device: string;
  member: string;
  wallet: string;
  index: number;
  label: string;
  issuedAt: number;
  thisDevice: boolean;
}
/** HUP-S8.1: this machine's device address (null before its key exists) + known links. */
/** HUP-S8.1: a prepared device removal (mirrors Rust `device_link::RevokePrepared`). */
export interface DeviceRevokePrepared {
  confirmId: string;
  /** Canonical device address (lowercase hex, no 0x). */
  device: string;
  statement: string;
  confirmBy: number;
}

export interface DeviceLinks {
  thisDevice: string | null;
  links: DeviceLinkView[];
  revoked: string[];
}
export interface ClusterStatus {
  groupId: string;
  online: number;
  total: number;
  sharedFiles: string[];
}
export interface ClusterDomain {
  status(groupId: string): Promise<ClusterStatus>;
  join(groupId: string): Promise<void>;
  peers(groupId: string): Promise<ClusterPeer[]>;
  /** Co-pin a CID across the Group roster. */
  shareFile(groupId: string, cid: string): Promise<void>;
  leave(groupId: string): Promise<void>;
  /** HUP-S8.1: the group's members with their linked devices (live). */
  devices(groupId: string): Promise<ClusterMemberDevices[]>;
  /** HUP-S8.1: this machine's device key address + the links it knows. Never mints a key. */
  myDevices(): Promise<DeviceLinks>;
  /** HUP-S8.1: open the wallet ceremony that links THIS machine. Signs nothing. */
  linkDeviceRequest(label: string): Promise<CeremonyView>;
  /** HUP-S8.1: the person approved; complete + store the link. */
  linkDeviceApprove(id: string, rawAck: boolean): Promise<DeviceLinks>;
  /** HUP-S8.1: the person declined; nothing was signed. */
  linkDeviceReject(id: string): Promise<void>;
  /**
   * HUP-S8.1: step 1 of removing a device of yours: what you confirm, and a one-shot id core
   * minted for that device (valid for two minutes). Signs nothing.
   */
  revokeDevicePrepare(device: string): Promise<DeviceRevokePrepared>;
  /** HUP-S8.1: you confirmed: revoke the device that `confirmId` names (permanent for that key). */
  revokeDevice(confirmId: string): Promise<DeviceLinks>;
  /** HUP-S8.1: this machine's signed link as a code to paste on another of YOUR devices. */
  exportDeviceLink(): Promise<string>;
  /** HUP-S8.1: add another of your own devices from its code (verified before it is stored). */
  importDeviceLink(code: string): Promise<DeviceLinks>;
  /**
   * HUP-S8.1 follow-on: the group message that shares YOUR links and revocations with `groupId`, or
   * null when there is nothing to share or this exact set was already sent there.
   */
  deviceLinksShareOffer(groupId: string): Promise<DeviceLinkShareOffer | null>;
  /** Record that the set with `digest` was sent to `groupId`. */
  deviceLinksMarkShared(groupId: string, digest: string): Promise<void>;
  /** Accept another member's share message (verified in core before anything is stored). */
  deviceLinksIngest(sender: string, body: string): Promise<DeviceLinkIngest>;
  /** HUP-S8.4 prep: whether the cross-machine mesh transport is on, and why. */
  meshStatus(): Promise<MeshStatus>;
  /**
   * V60 MESH-4: the member's on/off switch (`cluster_mesh_set_enabled`). Core refuses it until the
   * CL-S4 sign-off is recorded; a change applies the next time the cluster starts.
   */
  setMeshEnabled(enabled: boolean): Promise<MeshStatus>;
  /**
   * HUP-S8.4: this machine's link (or QR text) for `groupId`: where another member's machine can
   * reach it. Needs the cross-machine mesh; without it the daemon says so and nothing is invented.
   */
  groupSeed(groupId: string): Promise<ClusterGroupSeed>;
  /**
   * HUP-S8.4: dial the machines named in a group link another member shared. Resolves to how many
   * addresses are being dialed. A link says where to knock, nothing more: each machine is still
   * admitted only by the group's roster and DeviceLinks.
   */
  addGroupSeed(groupId: string, seed: string): Promise<number>;
}

/** HUP-S8.4: a group link (mirrors Rust `cluster::ClusterGroupSeedDto`). */
export interface ClusterGroupSeed {
  groupId: string;
  /** `citrate-cluster://seed?v=1&g=...&a=...`, short enough for a QR code. Locations only. */
  link: string;
  /** The multiaddrs in the link, each ending in this machine's peer id. */
  addrs: string[];
}

/** Group-message prefix of a DeviceLink share (a control message; never shown in chat). It starts
 *  with U+0001, like the social-binding sentinel, so nothing a member types can match it. */
export const DEVICE_LINKS_MSG_PREFIX = "\u0001cdlink1:";
export interface DeviceLinkShareOffer {
  body: string;
  digest: string;
}
export interface DeviceLinkIngest {
  links: number;
  revocations: number;
  refused: string[];
}
/** The cross-machine mesh transport policy (core `cluster_mesh.rs`). */
export interface MeshStatus {
  /** The cross-machine transport is on for this app run. */
  on: boolean;
  /** `operator`: CITRATE_CLUSTER_LISTEN is set; `default`: on by default after sign-off; `off`. */
  source: "operator" | "default" | "off";
  /** The CL-S4 transport sign-off is recorded in this build. */
  signedOff: boolean;
  /** Plain-language reason for the member. */
  note: string;
}
