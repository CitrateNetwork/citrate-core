// citrate-core — federation link builders for the "open ↗" affordances.
//
// The catalog/tutorials carry either a bare host (e.g. "explorer.citrate.ai") or
// an Atlas-relative path (e.g. "atlas/tutorials/first-validator"). These helpers
// turn those into full https URLs the shell opens in the system browser. The
// destination RP runs its own OIDC login today; a real signed-in handoff
// (shared-authority SSO) is work-order WO-6.

/** Atlas docs base — the canonical federation docs host. */
export const ATLAS_BASE = "https://docs.citrate.ai";
/** CitrateScan explorer base (canonical host). */
export const SCAN_BASE = "https://explorer.citrate.ai";

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

/** A CitrateScan transaction link for a tx hash. */
export const scanTxUrl = (hash: string): string => `${SCAN_BASE}/tx/${hash}`;
/** A CitrateScan address link for a wallet address. */
export const scanAddrUrl = (addr: string): string => `${SCAN_BASE}/address/${addr}`;
