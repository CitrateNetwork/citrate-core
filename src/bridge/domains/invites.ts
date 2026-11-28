// citrate-core: bridge domain `invites`, moved verbatim from src/bridge/domains.ts by L0-WP05
// (planset v060). src/bridge/domains.ts re-exports it, so imports from "../bridge/domains" keep
// working. The owning lane is the row for this path in .agentile/v60-ownership.map.

// ── Group invites (D-12): one flow, request and approve. The inviter makes a one-time link; whoever
// opens it asks to join; the inviter approves (they are added) or declines. Nothing joins from the
// link alone. Mirrors src-tauri/src/invites.rs. No private key ever crosses `invoke`.
/** An invite this device made. */
export interface InviteView {
  /** Public id (a hash prefix of the token). */
  id: string;
  group: string;
  /** The link to share: citrate://invite?… */
  link: string;
  /** Who it's for, a label only ("" when none). */
  label: string;
  /** Unix seconds. */
  createdAt: number;
  expiresAt: number;
  /** The requester being added now ("" when none). */
  approving: string;
}
/** A request waiting for the inviter. */
export interface IncomingRequest {
  inviteId: string;
  group: string;
  /** The comms address the request names (lowercase). What the request says: not checked
   *  against the requester's relay session, so the inviter confirms it's them before approving. */
  address: string;
  /** The label of the link it came in on ("" when the link has none). */
  inviteLabel: string;
  /** It is being approved now. */
  approving: boolean;
}
/** What checking a held approval against the roster found. */
export type HeldCheck = "not_held" | "added" | "released" | "expired";
/** Whether the requester(s) were told about a decline or revoke (best-effort, through the relay). */
export interface InviteNotice {
  notified: boolean;
}
/** What the "Join <group>?" screen shows for a link. Nothing is sent to read it. */
export interface InvitePreview {
  group: string;
  /** From the link, display only ("" when none). */
  groupName: string;
  /** From the link, display only ("" when none). */
  inviter: string;
  expiresAt: number;
  expired: boolean;
  /** This device already asked to join with this link. */
  requested: boolean;
}
export type InviteRequestState = "waiting" | "joined" | "declined" | "revoked" | "expired";
/** A request this device sent. */
export interface MyInviteRequest {
  id: string;
  group: string;
  groupName: string;
  inviter: string;
  requestedAt: number;
  expiresAt: number;
  state: InviteRequestState;
  /** Why the state couldn't be checked just now ("" when it was). */
  note: string;
}

export interface InvitesDomain {
  /** Make a one-time invite link for a group (owner or admin). `label` says who it's for (may be
   *  ""); `inviter` names the inviter on the invitee's screen (may be ""). */
  create(group: string, label: string, inviter: string): Promise<InviteView>;
  /** The group's open invites made on this device. Rejects when the saved invites can't be read. */
  list(group: string): Promise<InviteView[]>;
  /** Revoke an invite (also Cancel on a held approval): no request under it can be approved any more. */
  revoke(group: string, id: string): Promise<InviteNotice>;
  /** The requests waiting on the group's invites. Rejects when the relay can't be asked. */
  requests(group: string): Promise<IncomingRequest[]>;
  /** Approve a request: add the requester (Welcome) and use up the invite. */
  approve(group: string, id: string, address: string): Promise<void>;
  /** Settle a held approval by the roster (Check again on the invite row). */
  check(group: string, id: string): Promise<HeldCheck>;
  /** Decline a request. */
  decline(group: string, id: string, address: string): Promise<InviteNotice>;
  /** Read a link for the Join screen. Sends nothing. */
  preview(link: string): Promise<InvitePreview>;
  /** Ask to join with a link (the invitee's Join). */
  request(link: string): Promise<MyInviteRequest>;
  /** Where this device's requests stand; a request that was approved finishes its join here. */
  myRequests(): Promise<MyInviteRequest[]>;
  /** Forget a request this device sent. */
  dismiss(id: string): Promise<void>;
}
