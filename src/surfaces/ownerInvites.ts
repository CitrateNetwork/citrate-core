// The owner's outstanding invites and incoming claim requests for one group (Groups, roster tab).
// Data: bridge.invites.list / pollClaims (the app's pending.json and the relay claims inbox).
import { bridge } from "../bridge";
import type { InviteClaim, PendingInvite } from "../bridge/domains";

export interface OwnerInviteSetters {
  invites: (v: PendingInvite[]) => void;
  requests: (v: InviteClaim[]) => void;
}

/** Load `groupId`'s invites and requests. `isCurrent(groupId)` is asked after each answer. */
export async function loadOwnerInvites(groupId: string, _isCurrent: (groupId: string) => boolean, set: OwnerInviteSetters): Promise<void> {
  try {
    set.invites(await bridge.invites.list(groupId));
  } catch {
    /* honest: none / relay unavailable */
  }
  // CONNECT-S1 — poll the server-blind claims-inbox for incoming requests (no DM-back). Honest-empty
  // if the relay/daemon can't be reached; never a fabricated request (Rule 1).
  try {
    set.requests(await bridge.invites.pollClaims(groupId));
  } catch {
    set.requests([]);
  }
}
