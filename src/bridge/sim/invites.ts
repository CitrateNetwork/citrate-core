// CX bridge impl — group invites (INVITE-S2 self-admit + CONNECT-S1 claim-back), SIM. Honest (Rule 1).
import type { InviteMinted, InvitesDomain, PendingInvite, ReferralEvent } from "../domains";
import type { SimHost } from "./index";

export function simInvites(_host: SimHost): InvitesDomain {
  return {
    async create(): Promise<InviteMinted> {
      // No relay in the web preview — minting a self-admit invite needs the desktop daemon.
      throw new Error("creating a group invite needs the desktop app");
    },
    async redeem(): Promise<void> {
      // No relay/MLS in the web preview — self-admit needs the desktop daemon.
      throw new Error("joining via an invite needs the desktop app");
    },
    async list(): Promise<PendingInvite[]> {
      return [];
    },
    async verifyConsume(): Promise<boolean> {
      return false;
    },
    async revoke(): Promise<void> {
      /* no-op */
    },
    async submitClaim(): Promise<void> {
      /* sim: no relay claims-inbox — honest no-op (nothing submitted), never a fake "sent" */
    },
    async pollClaims() {
      return []; // sim: no relay — honest-empty, never a fabricated request
    },
    async referralLog(): Promise<ReferralEvent[]> {
      return []; // sim: no local ledger in the web preview — honest-empty
    },
    async exportReferralLog(): Promise<string> {
      return "[]";
    },
  };
}
