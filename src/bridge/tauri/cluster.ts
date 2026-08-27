// CX bridge impl — cluster (C-20), TAURI. Owned by lane s4 (CX-S4) after S0.
// S0.2 stub: honest Unavailable. CX-S4 wires the hybrid Noise-identity + gossipsub cluster.
import type { ClusterDomain } from "../domains";
import { Unavailable } from "../types";

export const tauriCluster: ClusterDomain = {
  async status() {
    throw new Unavailable("cluster", "status");
  },
  async join() {
    throw new Unavailable("cluster", "join");
  },
  async peers() {
    throw new Unavailable("cluster", "peers");
  },
  async shareFile() {
    throw new Unavailable("cluster", "shareFile");
  },
  async leave() {
    throw new Unavailable("cluster", "leave");
  },
};
