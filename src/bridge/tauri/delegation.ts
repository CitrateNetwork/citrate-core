// Bridge impl — the Cognition (Devin) delegate (D-11, ADR P-8), TAURI. Every command runs in Rust
// `cognition.rs`: core checks the proposed scope and returns the exact prompt (`prepare`), sends
// only what the member approved with the hash the card showed (`approve`), and reads the session
// live (`status`), all through the core model proxy. Nothing here carries a key.
import { invoke } from "./invoke";
import type { DelegationDomain, DelegationScope, DelegationSession, DelegationStarted, DelegationView } from "../domains";

export const tauriDelegation: DelegationDomain = {
  prepare(scope: DelegationScope) {
    return invoke<DelegationView>("delegation_prepare", { scope });
  },
  async decline(delegationId: string) {
    await invoke("delegation_decline", { delegationId });
  },
  approve(delegationId: string, scopeHash: string) {
    return invoke<DelegationStarted>("delegation_approve", { delegationId, scopeHash });
  },
  status(delegationId: string) {
    return invoke<DelegationSession>("delegation_status", { delegationId });
  },
};
