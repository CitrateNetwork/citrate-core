// CX bridge impl — v2 invite links (GROW-S1 task 10), TAURI. Thin wrappers over the Rust
// `join_*` commands; all crypto, signature checks and service calls happen in Rust.
import { invoke } from "./invoke";
import type { JoinCodeEntry, JoinDomain, JoinMinted, JoinMintRequest, JoinQr, JoinResolved, PendingJoinView } from "../domains";

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
  async redeem(code: string): Promise<void> {
    await invoke("join_redeem", { code });
  },
  async revoke(code: string): Promise<void> {
    await invoke("join_revoke", { code });
  },
  list(): Promise<JoinCodeEntry[]> {
    return invoke<JoinCodeEntry[]>("join_list");
  },
  pendingSet(link: string): Promise<PendingJoinView> {
    return invoke<PendingJoinView>("pending_join_set", { link });
  },
  pendingGet(): Promise<PendingJoinView | null> {
    return invoke<PendingJoinView | null>("pending_join_get");
  },
  async pendingClear(): Promise<void> {
    await invoke("pending_join_clear");
  },
  pendingResolve(): Promise<JoinResolved> {
    return invoke<JoinResolved>("pending_join_resolve");
  },
  shareQr(link: string): Promise<JoinQr> {
    return invoke<JoinQr>("join_share_qr", { link });
  },
};
