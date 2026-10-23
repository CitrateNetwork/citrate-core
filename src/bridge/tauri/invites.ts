// CX bridge impl — group invites (INVITE-S2 self-admit + CONNECT-S1 claim-back), TAURI.
import { invoke } from "./invoke";
import type { ConsumeOutcome, InviteClaim, InviteMinted, InvitesDomain, JoinRedeemResult, PendingInvite, ReferralEvent } from "../domains";

export const tauriInvites: InvitesDomain = {
  create(group: string, forHandle: string): Promise<InviteMinted> {
    return invoke<InviteMinted>("group_invite_create", { group, forHandle });
  },
  redeem(link: string): Promise<JoinRedeemResult> {
    return invoke<JoinRedeemResult>("group_invite_redeem", { link });
  },
  list(group: string): Promise<PendingInvite[]> {
    return invoke<PendingInvite[]>("group_invites", { group });
  },
  verifyConsume(group: string, token: string, address: string): Promise<ConsumeOutcome> {
    return invoke<ConsumeOutcome>("group_invite_verify_consume", { group, token, address });
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
  referralLog(): Promise<ReferralEvent[]> {
    return invoke<ReferralEvent[]>("group_referral_log");
  },
  exportReferralLog(): Promise<string> {
    return invoke<string>("group_referral_export");
  },
};
