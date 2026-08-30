// CX bridge impl — social identity (Connections · social discovery), TAURI.
//
// The LINK flow is wired to the real Rust social_* commands (public-client loopback-PKCE, per
// ADR-2026-08-30): status / start (OAuth ownership proof → keyring-sealed token → device-local
// record) / setVisibility / disconnect. The token NEVER crosses this boundary. `verified` (the
// wallet-signed IdentityBinding through the ceremony, D3) is the follow-up — bindingChallenge/verify
// stay honest Unavailable until then. Rule 3 holds: when wired, the WALLET signs; no sidecar signs.
import { invoke } from "@tauri-apps/api/core";
import type { LinkedIdentity, SocialDomain, SocialNetwork, SocialVisibility } from "../domains";
import { Unavailable } from "../types";

export const tauriSocial: SocialDomain = {
  status(): Promise<LinkedIdentity[]> {
    return invoke<LinkedIdentity[]>("social_status");
  },
  start(network: SocialNetwork): Promise<LinkedIdentity> {
    return invoke<LinkedIdentity>("social_start", { network });
  },
  async bindingChallenge(): Promise<{ message: string; nonce: string }> {
    throw new Unavailable("social", "bindingChallenge");
  },
  async verify(): Promise<LinkedIdentity> {
    throw new Unavailable("social", "verify");
  },
  setVisibility(network: SocialNetwork, visibility: SocialVisibility): Promise<LinkedIdentity> {
    return invoke<LinkedIdentity>("social_set_visibility", { network, visibility });
  },
  async disconnect(network: SocialNetwork): Promise<void> {
    await invoke("social_disconnect", { network });
  },
};
