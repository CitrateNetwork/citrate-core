// =====================================================================
// citrate-core — which network the webview's saved state belongs to (RC52)
//
// Values read from a chain (grant, bond, balances, claimable, tx history)
// belong to one genesis. A save is stamped with the genesis of the active
// network profile; a save of another genesis is archived on load, never
// shown as current.
// =====================================================================
import { ACTIVE_PROFILE, ACTIVE_PROFILE_ID, expectedGenesis, type NetworkProfile } from "../network/profile";

/** network: read from one chain, so kept only for its genesis. device: this install's own. */
export type PersistScope = "network" | "device";

/** A chain read a network-scoped value waits for after a switch, until it is made. */
export type NetRead = "grant" | "balances" | "activity";

/** localStorage key of the previous networks' archived values. */
export const NET_ARCHIVE_KEY = "citrate-core-netarchive-v1";

/** One previous network's values, as they were when the switch archived them. */
export interface ArchivedNetwork {
  /** Its genesis, or null when the save did not record a readable one. */
  genesis: string | null;
  archivedAt: number;
  values: Record<string, unknown>;
}

/** The persistent switch notice the Node surface shows until it is dismissed. */
export interface NetworkNotice {
  at: number;
  /** The previous network's genesis (null when not recorded). */
  from: string | null;
  /** The network this build targets now. */
  to: string;
  /** The node's own notice (chain data reset for the new genesis), when it gave one. */
  node: string | null;
}

/** The key network-scoped state is saved under: the active profile's genesis, or `unset:<id>`. */
export function currentNetKey(p: NetworkProfile | null = ACTIVE_PROFILE): string {
  return expectedGenesis(p) ?? `unset:${p?.id ?? ACTIVE_PROFILE_ID}`;
}

/** The archived previous networks, oldest first. */
export function readNetArchive(): ArchivedNetwork[] {
  return [];
}
