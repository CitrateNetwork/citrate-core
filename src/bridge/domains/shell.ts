// citrate-core: bridge domain `shell`, moved verbatim from src/bridge/domains.ts by L0-WP05
// (planset v060). src/bridge/domains.ts re-exports it, so imports from "../bridge/domains" keep
// working. The owning lane is the row for this path in .agentile/v60-ownership.map.

/** Opening federation apps / docs / explorer links in the system browser. The
 * URL is opened as-is (the destination RP runs its own OIDC login); a real
 * signed-in handoff (shared-authority SSO) is a later work-order item (WO-6).
 * Only https URLs are opened — never an arbitrary scheme. */
export interface ShellDomain {
  openExternal(url: string): Promise<void>;
}
