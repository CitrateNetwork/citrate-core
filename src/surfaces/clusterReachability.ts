// V60 MESH-4: how a peer's live state reads in the Cluster screen. Source: the cluster daemon's
// `peers` answer (`cluster_peers`), whose `link` says whether an online peer is reached directly or
// through a Citrate relay. An older daemon sends no `link`; the label then says only "online".
import type { ClusterPeer } from "../bridge/domains";

export function peerStateLabel(p: ClusterPeer): string {
  if (!p.online) return "authorized · offline";
  if (p.link === "direct") return "online · direct";
  if (p.link === "relayed") return "online · through a relay";
  return "online";
}
