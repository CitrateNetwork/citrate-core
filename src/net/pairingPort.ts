// L7-WP03: what the member is told about the pairing port. Source: the pairing offer from
// `fleet_pair_create` (`port`, `portFallback`; core binds TCP 4210 on private addresses only).
import type { PairOffer } from "../bridge/tauri/fleet";

/** The plain-language port line for a pairing offer, or null when core did not say (older core). */
export function pairingPortNote(o: Pick<PairOffer, "port" | "portFallback">): string | null {
  if (typeof o.port !== "number") return null;
  if (o.portFallback) {
    return `Port ${o.port} (TCP) is taken on this machine, so this link uses another port, shown in its addresses. If the other machine can't connect, allow Citrate Core through the firewall for private networks.`;
  }
  return `Pairing uses port ${o.port} (TCP), on your private networks only. If the other machine can't connect, allow Citrate Core through the firewall for private networks.`;
}
