// citrate-core — federation link builders for the "open ↗" affordances.
//
// The catalog/tutorials carry either a bare host (e.g. "scan.citrate.ai") or an
// Atlas-relative path (e.g. "atlas/tutorials/first-validator"). These helpers
// turn those into full https URLs the shell opens in the system browser. The
// destination RP runs its own OIDC login today; a real signed-in handoff
// (shared-authority SSO) is work-order WO-6.

/** Atlas docs base — the known-deployed host. WO-8: DGX to confirm the canonical
 * domain (a *.citrate.ai custom domain may replace this preview host later). */
export const ATLAS_BASE = "https://citrate-atlas.vercel.app";
/** CitrateScan explorer base (services seed: scan.citrate.ai). */
export const SCAN_BASE = "https://scan.citrate.ai";

/**
 * Normalize a catalog `url`/`path`/`docs` value to a full https URL:
 * - already-absolute `https://…` → unchanged
 * - `atlas/<rest>` → `${ATLAS_BASE}/<rest>`
 * - bare host `scan.citrate.ai` → `https://scan.citrate.ai`
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
