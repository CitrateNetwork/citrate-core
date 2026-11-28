// citrate-core: bridge domain `auth`, moved verbatim from src/bridge/domains.ts by L0-WP05
// (planset v060). src/bridge/domains.ts re-exports it, so imports from "../bridge/domains" keep
// working. The owning lane is the row for this path in .agentile/v60-ownership.map.

import type { AuthStatus } from "../types";

// ---- the eight seam domains -----------------------------------------
// Their sim impls delegate to the prototype Store (1:1 UI preserved); their
// Tauri impls throw `Unavailable` until their own later phase.

// ---- auth (A3 — real OIDC loopback-PKCE) ----------------------------
// Every method returns the claim-derived AuthStatus (or void) — NEVER a token
// (ADV-8). In Tauri these invoke the real Rust OIDC commands; in sim they drive
// the prototype persona flow (guarded out of packaged builds).
export interface AuthDomain {
  /** Current claim-derived status (signedIn + entitlement flags). */
  status(): Promise<AuthStatus>;
  /** Run the loopback-PKCE sign-in; opens the system browser. Returns status. */
  login(): Promise<AuthStatus>;
  /**
   * rc.5 FIX 2 — abort the in-flight browser sign-in (the Cancel button). The pending
   * `login()` then rejects with "auth: sign-in cancelled". Resolves true when an
   * attempt was waiting.
   */
  cancelLogin(): Promise<boolean>;
  /** Live /userinfo entitlement re-check (the federation RP rule). */
  userinfo(): Promise<AuthStatus>;
  /** Silent refresh from the vaulted refresh token (survives restart). */
  refresh(): Promise<AuthStatus>;
  /** Revoke the refresh token + clear the vault slot + wipe the session. */
  logout(): Promise<void>;
  /** Open the KYC flow in the browser (S2). Status is then read via userinfo. */
  kycStart(): Promise<void>;
}
