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
  };
}
