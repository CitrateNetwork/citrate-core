// citrate-core: bridge domain `groups`, moved verbatim from src/bridge/domains.ts by L0-WP05
// (planset v060). src/bridge/domains.ts re-exports it, so imports from "../bridge/domains" keep
// working. The owning lane is the row for this path in .agentile/v60-ownership.map.

// ── C-19 groups: secure 1:1 + group comms, admin RBAC (lane s3) ──
export type GroupRole = "owner" | "admin" | "member" | "guest" | "agent";
export interface GroupMember {
  address: string;
  role: GroupRole;
}
export interface Group {
  id: string;
  /** Human name (the daemon stores it; `list` returns it). Amended in post-S0 (CX-S3.5). */
  name: string;
  owner: string;
  kind: "dm" | "channel" | "forum";
  members: GroupMember[];
}
export interface GroupMessage {
  id: string;
  groupId: string;
  sender: string;
  body: string;
  ts: number;
}
export interface GroupsDomain {
  create(kind: Group["kind"], name: string): Promise<Group>;
  list(): Promise<Group[]>;
  /** This member's own comms address (what the rosters key on) — used to exclude self from the People directory. */
  selfAddress(): Promise<string>;
  join(groupId: string): Promise<void>;
  /**
   * Owner-invite: add a member who has published a key package to the shared relay. The daemon
   * produces + publishes the MLS welcome; the invitee then `join`s. Amended post-S0 (CX-S3.5).
   */
  addMember(groupId: string, address: string): Promise<void>;
  roster(groupId: string): Promise<GroupMember[]>;
  /** Change a member's role: `groups_assign_role`, a `grantRole` request the comms daemon signs
   *  with this device's comms key (#637). The bundled daemon is older than 0.6.0, so it rejects
   *  with ROLE_CHANGES_NEED_NEWER_DAEMON (groupRoles.ts) until the daemon pin moves. */
  assignRole(groupId: string, address: string, role: GroupRole): Promise<void>;
  /** Atomic offboard — drops all four planes in one epoch (ADR-001). */
  offboard(groupId: string, address: string): Promise<void>;
  send(groupId: string, body: string): Promise<void>;
  messages(groupId: string): Promise<GroupMessage[]>;
  /**
   * Flag-A — the networked relay-link health for the status chip. One of:
   * `"idle"` (daemon not started) · `"local"` (in-process relay) · `"connecting"` (up, unconfirmed) ·
   * `"connected"` · `"reconnecting"` (link down, the daemon is redialing) · `"degraded"` (link down, ops failing). Reported, never a fake — a bounded read.
   */
  relayStatus(): Promise<string>;
}
