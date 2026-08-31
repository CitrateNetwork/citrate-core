// CX bridge impl — group claimable invites (ADR D4), SIM. Honest-empty (Rule 1).
import type { InviteMinted, InvitesDomain, PendingInvite } from "../domains";
import type { SimHost } from "./index";

export function simInvites(_host: SimHost): InvitesDomain {
  return {
    async create(group: string): Promise<InviteMinted> {
      // No relay in the web preview — return an honest, clearly-local link with no token store.
      return { token: "", link: `citrate://invite?g=${group}&t=` };
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
  };
}
