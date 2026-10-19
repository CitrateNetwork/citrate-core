// CX bridge impl — v2 invite links (GROW-S1 task 10), TAURI. Thin wrappers over the Rust
// `join_*` commands; all crypto, signature checks and service calls happen in Rust.
import { invoke } from "./invoke";
import type { JoinCodeEntry, JoinDomain, JoinMinted, JoinMintRequest, JoinRedeemResult, JoinResolved } from "../domains";

export const tauriJoin: JoinDomain = {
  mint(request: JoinMintRequest, inviteLink?: string): Promise<JoinMinted> {
    return invoke<JoinMinted>("join_mint", { request, inviteLink: inviteLink ?? null });
  },
  attach(code: string): Promise<JoinMinted> {
    return invoke<JoinMinted>("join_attach", { code });
  },
  resolve(link: string): Promise<JoinResolved> {
    return invoke<JoinResolved>("join_resolve", { link });
  },
  redeem(code: string): Promise<JoinRedeemResult> {
    return invoke<JoinRedeemResult>("join_redeem", { code });
  },
  async revoke(code: string): Promise<void> {
    await invoke("join_revoke", { code });
  },
  list(): Promise<JoinCodeEntry[]> {
    return invoke<JoinCodeEntry[]>("join_list");
  },
};
