// CX bridge impl — group claimable invites (ADR D4), TAURI.
import { invoke } from "@tauri-apps/api/core";
import type { InviteClaim, InviteMinted, InvitesDomain, PendingInvite } from "../domains";

export const tauriInvites: InvitesDomain = {
  create(group: string, forHandle: string): Promise<InviteMinted> {
    return invoke<InviteMinted>("group_invite_create", { group, forHandle });
  },
  list(group: string): Promise<PendingInvite[]> {
    return invoke<PendingInvite[]>("group_invites", { group });
  },
  verifyConsume(group: string, token: string): Promise<boolean> {
    return invoke<boolean>("group_invite_verify_consume", { group, token });
  },
  async revoke(group: string, token: string): Promise<void> {
    await invoke("group_invite_revoke", { group, token });
  },
  async submitClaim(link: string): Promise<void> {
    await invoke("group_invite_submit_claim", { link });
  },
  pollClaims(group: string): Promise<InviteClaim[]> {
    return invoke<InviteClaim[]>("group_invite_poll_claims", { group });
  },
};
