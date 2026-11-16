// =====================================================================
// citrate-core — Groups › Chat › Alerts (0.5.4 FL design, board 02).
//
// An alert points at the screen where you act. It never carries an approve button (HIC standard:
// approvals live in the thread or the review sheet, rendered by core). Every alert comes from data
// this device holds:
//   draft    a Hermes round draft in a group where you are an owner or admin (group messages)
//   answer   Hermes answered a message you sent (group messages)
//   invite   someone asked to join with an invite you made (the relay's claims inbox, invites.rs)
// Rounds and cluster alerts come from round events and cluster changes; neither has a source on
// this network today, so none are made up.
// =====================================================================
import type { GroupMessage, GroupRole, IncomingRequest } from "../../bridge/domains";
import { decodeHermes, messageRef } from "./groupEnvelope";

export interface GroupAlert {
  key: string;
  kind: "draft" | "answer" | "invite";
  /** The chip word. */
  tag: "HIC-1" | "Hermes" | "Invite";
  text: string;
  groupId: string;
  /** Where the alert opens. */
  route: "groups/chat" | "groups/members";
}

export interface AlertInput {
  groups: { id: string; name: string }[];
  history: Record<string, GroupMessage[]>;
  /** Your role per group id (null when not known). */
  roles: Record<string, GroupRole | null>;
  /** Waiting requests per group id you manage. */
  requests: Record<string, IncomingRequest[]>;
  me: string;
  /** Refs of the Hermes answers this device posted (groupHermes.ownAnswers). */
  ownAnswers?: Set<string>;
}

const short = (a: string) => (a.length > 12 ? `${a.slice(0, 6)}…${a.slice(-4)}` : a);

export function groupAlerts(i: AlertInput): GroupAlert[] {
  const out: GroupAlert[] = [];
  const me = i.me.toLowerCase();
  for (const g of i.groups) {
    const msgs = i.history[g.id] ?? [];
    const role = i.roles[g.id] ?? null;
    const mine = new Set(msgs.filter((m) => m.sender.toLowerCase() === me).map((m) => messageRef(m.sender, m.body)));
    for (const m of msgs) {
      const h = decodeHermes(m.body);
      if (!h) continue;
      const ref = messageRef(m.sender, m.body);
      if (h.draft && (role === "owner" || role === "admin")) {
        const verified = m.sender.toLowerCase() === me && (i.ownAnswers?.has(ref) ?? false);
        out.push(
          verified
            ? { key: "d" + g.id + ref, kind: "draft", tag: "HIC-1", text: `${g.name} has a round draft waiting for an owner or admin to review: ${h.draft.title}`, groupId: g.id, route: "groups/chat" }
            : { key: "d" + g.id + ref, kind: "draft", tag: "Hermes", text: `${short(m.sender)} posted a round draft as a Hermes answer in ${g.name} (not verified): ${h.draft.title}`, groupId: g.id, route: "groups/chat" },
        );
      }
      if (mine.has(h.to) && m.sender.toLowerCase() !== me) {
        out.push({ key: "a" + g.id + ref, kind: "answer", tag: "Hermes", text: `${short(m.sender)} posted a Hermes answer to your message in ${g.name} (not verified)`, groupId: g.id, route: "groups/chat" });
      }
    }
    for (const r of i.requests[g.id] ?? []) {
      out.push({ key: "i" + g.id + r.inviteId + r.address, kind: "invite", tag: "Invite", text: `${short(r.address)} asked to join ${g.name}${r.inviteLabel ? ` (invite for ${r.inviteLabel})` : ""}`, groupId: g.id, route: "groups/members" });
    }
  }
  // Newest last in history, so show the most recent first.
  return out.reverse();
}
