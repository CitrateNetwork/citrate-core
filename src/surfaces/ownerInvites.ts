// The owner's outstanding invites and incoming claim requests for one group (Groups, roster tab).
// Data: bridge.invites.list / pollClaims (the app's pending.json and the relay claims inbox).
import { bridge } from "../bridge";
import type { InviteClaim, PendingInvite } from "../bridge/domains";

export interface OwnerInviteSetters {
  invites: (v: PendingInvite[]) => void;
  requests: (v: InviteClaim[]) => void;
}

/** Load `groupId`'s invites and requests; each answer is applied only while `isCurrent(groupId)`. */
export async function loadOwnerInvites(groupId: string, isCurrent: (groupId: string) => boolean, set: OwnerInviteSetters): Promise<void> {
  // #389 — an answer for a group that is no longer selected is dropped, so a switch mid-load never
  // shows (or lets Remove / Approve act on) the previous group's rows.
  try {
    const invites = await bridge.invites.list(groupId);
    if (isCurrent(groupId)) set.invites(invites);
  } catch {
    /* honest: none / relay unavailable */
  }
  // CONNECT-S1 — poll the server-blind claims-inbox for incoming requests (no DM-back). Honest-empty
  // if the relay/daemon can't be reached; never a fabricated request (Rule 1).
  try {
    const requests = await bridge.invites.pollClaims(groupId);
    if (isCurrent(groupId)) set.requests(requests);
  } catch {
    if (isCurrent(groupId)) set.requests([]);
  }
}
