// =====================================================================
// citrate-core — which network the webview's saved state belongs to (RC52)
//
// Values read from a chain (grant, bond, balances, claimable, tx history)
// belong to one genesis. Every save is stamped with the genesis of the active
// network profile (`netGenesis`). A save of another genesis, or one whose
// stamp can't be read, is archived on load under that genesis and its
// network-scoped values are reset and marked "not read yet"; they are never
// shown as current. This install's own values (wallet identity, settings,
// journal, Hermes) are kept. Which persisted key is which: PERSIST_SCOPE in
// state.ts, pinned by persistScope.test.ts.
//
// A 0.5.1 save has no stamp. 0.5.1 ran only on the alpha-r1005 genesis
// (UPGRADE_051_052 §2.10: its node marker equals that profile's genesis), so an
// unstamped save is read as alpha-r1005's.
// =====================================================================
import { ACTIVE_PROFILE, ACTIVE_PROFILE_ID, PROFILES, expectedGenesis, type NetworkProfile } from "../network/profile";

/** network: read from one chain, so kept only for its genesis. device: this install's own. */
export type PersistScope = "network" | "device";

/** A chain read a network-scoped value waits for after a switch, until it is made. */
export type NetRead = "grant" | "balances" | "activity";

/** Every read a switch marks as not made yet. */
export const NET_READS: readonly NetRead[] = ["grant", "balances", "activity"];

/** The field a save records its network in (the active profile's genesis at save time). */
export const NET_STAMP_FIELD = "netGenesis";

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
  /** False when the previous network's values could not be archived on this device. */
  archived: boolean;
}

/** The key network-scoped state is saved under: the active profile's genesis, or `unset:<id>`. */
export function currentNetKey(p: NetworkProfile | null = ACTIVE_PROFILE): string {
  return expectedGenesis(p) ?? `unset:${p?.id ?? ACTIVE_PROFILE_ID}`;
}

/** The network an unstamped (0.5.1) save belongs to: the alpha-r1005 genesis. */
const UNSTAMPED_SAVE_NET: string | null = expectedGenesis(PROFILES.find((p) => p.id === "alpha-r1005") ?? null);

const GENESIS = /^0x[0-9a-f]{64}$/;
const UNSET = /^unset:[A-Za-z0-9_-]+$/;

/**
 * The network a save belongs to: its stamp, the alpha-r1005 genesis for an unstamped save, or
 * null when the stamp is there but not a network key (unknown network, never the current one).
 */
export function savedNetKey(saved: Record<string, unknown>): string | null {
  if (!(NET_STAMP_FIELD in saved)) return UNSTAMPED_SAVE_NET;
  const v = saved[NET_STAMP_FIELD];
  if (typeof v !== "string") return null;
  if (UNSET.test(v)) return v;
  const k = v.toLowerCase();
  return GENESIS.test(k) ? k : null;
}

function isArchived(v: unknown): v is ArchivedNetwork {
  if (!v || typeof v !== "object") return false;
  const a = v as Record<string, unknown>;
  return (
    (a.genesis === null || typeof a.genesis === "string") &&
    typeof a.archivedAt === "number" &&
    !!a.values &&
    typeof a.values === "object"
  );
}

/** The parsed archive, or "damaged" when the stored text is not one. Missing = empty. */
function parseArchive(): ArchivedNetwork[] | "damaged" {
  const raw = localStorage.getItem(NET_ARCHIVE_KEY);
  if (raw === null) return [];
  try {
    const v = JSON.parse(raw) as { v?: unknown; entries?: unknown };
    if (v && v.v === 1 && Array.isArray(v.entries) && v.entries.every(isArchived)) return v.entries;
  } catch {
    /* falls through to damaged */
  }
  return "damaged";
}

/** The archived previous networks, oldest first (empty when none, unreadable or unavailable). */
export function readNetArchive(): ArchivedNetwork[] {
  try {
    const a = parseArchive();
    return a === "damaged" ? [] : a;
  } catch {
    return [];
  }
}

/**
 * Append one previous network's values to the archive. Append only: earlier entries are kept, and
 * a damaged archive is moved aside (`<key>.damaged-<ms>`) before a new one is written, never
 * overwritten. Returns false when the archive could not be written (storage unavailable or full).
 */
export function appendNetArchive(entry: ArchivedNetwork): boolean {
  try {
    let entries = parseArchive();
    if (entries === "damaged") {
      const raw = localStorage.getItem(NET_ARCHIVE_KEY) ?? "";
      localStorage.setItem(`${NET_ARCHIVE_KEY}.damaged-${entry.archivedAt}`, raw);
      entries = [];
    }
    localStorage.setItem(NET_ARCHIVE_KEY, JSON.stringify({ v: 1, entries: [...entries, entry] }));
    return true;
  } catch {
    return false;
  }
}

/** A short form of a network key for display: `0x1dcfc490…75f1`, or the key itself. */
export function shortNet(k: string | null): string {
  if (k === null) return "not recorded";
  return GENESIS.test(k) ? `${k.slice(0, 10)}…${k.slice(-4)}` : k;
}
