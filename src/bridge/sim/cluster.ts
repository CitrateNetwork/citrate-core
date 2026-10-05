// CX bridge impl — cluster (C-20), SIM. Owned by lane s4 after S0. Honest-empty (Rule 1).
import type { ClusterDomain, ClusterStatus } from "../domains";
import type { SimHost } from "./index";

export function simCluster(_host: SimHost): ClusterDomain {
  return {
    async status(groupId): Promise<ClusterStatus> {
      return { groupId, online: 0, total: 0, sharedFiles: [] };
    },
    async join() {
      /* sim: no-op */
    },
    async peers() {
      return [];
    },
    async shareFile() {
      /* sim: no-op */
    },
    async leave() {
      /* sim: no-op */
    },
    // HUP-S8.1 — honest-empty: the sim has no device key, no wallet and no cluster daemon.
    async devices() {
      return [];
    },
    async myDevices() {
      return { thisDevice: null, links: [], revoked: [] };
    },
    async linkDeviceRequest() {
      throw new Error("Linking a device needs the Citrate Core desktop app.");
    },
    async linkDeviceApprove() {
      throw new Error("Linking a device needs the Citrate Core desktop app.");
    },
    async linkDeviceReject() {
      /* sim: nothing pending */
    },
    async revokeDevicePrepare() {
      throw new Error("Removing a device needs the Citrate Core desktop app.");
    },
    async revokeDevice() {
      throw new Error("Removing a device needs the Citrate Core desktop app.");
    },
    async exportDeviceLink() {
      throw new Error("Linking a device needs the Citrate Core desktop app.");
    },
    async importDeviceLink() {
      throw new Error("Linking a device needs the Citrate Core desktop app.");
    },
    // Nothing to share and nothing accepted: the sim holds no links.
    async deviceLinksShareOffer() {
      return null;
    },
    async deviceLinksMarkShared() {
      /* sim: nothing was sent */
    },
    async deviceLinksIngest() {
      return { links: 0, revocations: 0, refused: [] };
    },
    async meshStatus() {
      return {
        on: false,
        source: "off" as const,
        signedOff: false,
        note: "The preview has no cluster daemon.",
      };
    },
    // HUP-S8.4: no daemon, no address, so refuse rather than invent one.
    async groupSeed() {
      throw new Error("Group links need the Citrate Core desktop app.");
    },
    async addGroupSeed() {
      throw new Error("Group links need the Citrate Core desktop app.");
    },
  };
}
