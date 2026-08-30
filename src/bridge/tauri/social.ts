// CX bridge impl — social identity (Connections · social discovery), TAURI.
//
// SCAFFOLD (per ADR-2026-08-30 implementation surface): the seam exists; the Rust `social_*`
// commands (OAuth loopback-PKCE reusing connections.rs, the device-local + server-blind binding
// store, and the wallet-signed IdentityBinding through the ceremony) land in the follow-up sprint.
// Until then: status() is honestly EMPTY (there are no links yet — nothing is fabricated, Rule 1),
// and every mutating op reports honest Unavailable so the UI shows "pending backend", never a fake
// link. Rule 3 holds — when wired, the WALLET signs the binding challenge; no sidecar signs.
import type { LinkedIdentity, SocialDomain } from "../domains";
import { Unavailable } from "../types";

export const tauriSocial: SocialDomain = {
  async status(): Promise<LinkedIdentity[]> {
    // No binding store wired yet → there are genuinely no links. Honest-empty, not an error card.
    return [];
  },
  async start(): Promise<LinkedIdentity> {
    throw new Unavailable("social", "start");
  },
  async bindingChallenge(): Promise<{ message: string; nonce: string }> {
    throw new Unavailable("social", "bindingChallenge");
  },
  async verify(): Promise<LinkedIdentity> {
    throw new Unavailable("social", "verify");
  },
  async setVisibility(): Promise<LinkedIdentity> {
    throw new Unavailable("social", "setVisibility");
  },
  async disconnect(): Promise<void> {
    throw new Unavailable("social", "disconnect");
  },
};
