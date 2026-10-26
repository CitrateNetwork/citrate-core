// citrate-core — federation link builders for the "open ↗" affordances.
//
// The catalog/tutorials carry either a bare host (e.g. "explorer.citrate.ai") or
// an Atlas-relative path (e.g. "atlas/tutorials/first-validator"). These helpers
// turn those into full https URLs the shell opens in the system browser. The
// destination RP runs its own OIDC login today; a real signed-in handoff
// (shared-authority SSO) is work-order WO-6.

import { ACTIVE_PROFILE, type NetworkProfile } from "../network/profile";

/** Atlas docs base — the canonical federation docs host. */
export const ATLAS_BASE = "https://docs.citrate.ai";

/**
 * Normalize a catalog `url`/`path`/`docs` value to a full https URL:
 * - already-absolute `https://…` → unchanged
 * - `atlas/<rest>` → `${ATLAS_BASE}/<rest>`
 * - bare host `explorer.citrate.ai` → `https://explorer.citrate.ai`
 */
export function federationUrl(pathOrHost: string): string {
  const v = (pathOrHost || "").trim();
  if (/^https:\/\//i.test(v)) return v;
  if (v.startsWith("atlas/")) return `${ATLAS_BASE}/${v.slice("atlas/".length)}`;
  return `https://${v.replace(/^\/+/, "")}`;
}

/**
 * The explorer of a network profile (default: the active one), or null when it has none. RC52: links
 * follow the network the values were read on; a value from another network gets no link, since this
 * build does not know that network's explorer.
 */
function explorerOf(p: NetworkProfile | null): string | null {
  const u = p?.explorerUrl;
  return typeof u === "string" && /^https:\/\/\S+$/.test(u) ? u.replace(/\/+$/, "") : null;
}

/** A CitrateScan transaction link for a tx hash on a profile's network, or null without an explorer. */
export const scanTxUrl = (hash: string, p: NetworkProfile | null = ACTIVE_PROFILE): string | null => {
  const base = explorerOf(p);
  return base === null ? null : `${base}/tx/${hash}`;
};
/** A CitrateScan address link on a profile's network, or null without an explorer. */
export const scanAddrUrl = (addr: string, p: NetworkProfile | null = ACTIVE_PROFILE): string | null => {
  const base = explorerOf(p);
  return base === null ? null : `${base}/address/${addr}`;
};
