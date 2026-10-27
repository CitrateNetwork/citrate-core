// CX bridge impl — group invites (D-12: request and approve), SIM. The web preview has no relay and
// no group daemon, so nothing here pretends to make, send or approve an invite (Rule 1).
import type { InvitesDomain } from "../domains";
import type { SimHost } from "./index";

const DESKTOP_ONLY = "Invites need the desktop app.";

export function simInvites(_host: SimHost): InvitesDomain {
  const refuse = async (): Promise<never> => {
    throw new Error(DESKTOP_ONLY);
  };
  return {
    create: refuse,
    list: async () => [],
    revoke: refuse,
    requests: refuse,
    approve: refuse,
    decline: refuse,
    preview: refuse,
    request: refuse,
    myRequests: async () => [],
    dismiss: async () => {},
  };
}
