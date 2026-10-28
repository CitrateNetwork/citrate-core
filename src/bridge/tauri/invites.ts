// CX bridge impl — group invites (D-12: request and approve), TAURI. Mirrors src-tauri/src/invites.rs.
import { invoke } from "./invoke";
import type { HeldCheck, IncomingRequest, InviteNotice, InvitePreview, InvitesDomain, InviteView, MyInviteRequest } from "../domains";

export const tauriInvites: InvitesDomain = {
  create(group, label, inviter) {
    return invoke<InviteView>("group_invite_create", { group, label, inviter });
  },
  list(group) {
    return invoke<InviteView[]>("group_invites", { group });
  },
  revoke(group, id) {
    return invoke<InviteNotice>("group_invite_revoke", { group, id });
  },
  requests(group) {
    return invoke<IncomingRequest[]>("group_invite_requests", { group });
  },
  async approve(group, id, address) {
    await invoke("group_invite_approve", { group, id, address });
  },
  check(group, id) {
    return invoke<HeldCheck>("group_invite_check", { group, id });
  },
  decline(group, id, address) {
    return invoke<InviteNotice>("group_invite_decline", { group, id, address });
  },
  preview(link) {
    return invoke<InvitePreview>("group_invite_preview", { link });
  },
  request(link) {
    return invoke<MyInviteRequest>("group_invite_request", { link });
  },
  myRequests() {
    return invoke<MyInviteRequest[]>("group_invite_my_requests");
  },
  async dismiss(id) {
    await invoke("group_invite_dismiss", { id });
  },
};
