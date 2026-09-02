// CX bridge impl — groups (C-19), TAURI. Owned by lane s3 (CX-S3).
//
// S3.4: real invokes of the CX-S3.2 groups_* commands. citrate-core spawns the comms-member-daemon
// (#161) and routes these over its UDS socket; the daemon returns lean tuples, which we map onto the
// frozen DTOs. Errors propagate verbatim (Rule 1) — e.g. "comms keyring error…" or "binary not
// bundled…" before the daemon is packaged — so the surface shows the honest state, never a fake room.
//
// Two frozen-contract gaps are HONORED, not faked, and tracked for an S0 amendment:
//   1. `Group` has no `name`; the daemon stores one and `list` returns it, but the DTO can't carry
//      it — so persistent channel names need an S0 `name` field. The slice keeps session names.
//   2. the domain has no `addMember`; owner-invite (the daemon's add_member) needs an S0 command.
import { invoke } from "./invoke";
import type { Group, GroupMember, GroupMessage, GroupRole, GroupsDomain } from "../domains";

/** (address, role) rows as the Rust groups_roster / groups_list tuples arrive in JS. */
type Pair = [string, string];

function toMembers(rows: Pair[]): GroupMember[] {
  return rows.map(([address, role]) => ({ address, role: role as GroupRole }));
}

export const tauriGroups: GroupsDomain = {
  async create(kind, name): Promise<Group> {
    const id = await invoke<string>("groups_create", { name });
    // Compose the frozen Group from the fresh roster (the daemon seeds the owner as sole member).
    const members = toMembers(await invoke<Pair[]>("groups_roster", { group: id }));
    const owner = members.find((m) => m.role === "owner")?.address ?? "";
    return { id, name, owner, kind, members };
  },
  async list(): Promise<Group[]> {
    const rows = await invoke<Pair[]>("groups_list"); // (id, name)
    return rows.map(([id, name]) => ({ id, name, owner: "", kind: "channel" as Group["kind"], members: [] }));
  },
  async selfAddress(): Promise<string> {
    return invoke<string>("groups_self_address");
  },
  async join(groupId) {
    await invoke("groups_join", { group: groupId });
  },
  async addMember(groupId, address) {
    await invoke("groups_add_member", { group: groupId, member: address });
  },
  async roster(groupId) {
    return toMembers(await invoke<Pair[]>("groups_roster", { group: groupId }));
  },
  async assignRole(groupId, address, role) {
    await invoke("groups_assign_role", { group: groupId, member: address, role });
  },
  async offboard(groupId, address) {
    await invoke("groups_offboard", { group: groupId, member: address });
  },
  async send(groupId, body) {
    await invoke("groups_send", { group: groupId, text: body });
  },
  async messages(groupId): Promise<GroupMessage[]> {
    const rows = await invoke<Pair[]>("groups_messages", { group: groupId }); // (sender, body)
    // The lean command carries no id/ts; index gives a stable key, ts=0 (the surface renders order,
    // not wall-clock — an honest limitation until the daemon surfaces timestamps).
    return rows.map(([sender, body], i) => ({ id: `${groupId}:${i}`, groupId, sender, body, ts: 0 }));
  },
  async relayStatus(): Promise<string> {
    // Bounded read; never force-starts the daemon (the command reads MANAGER or returns "idle").
    return invoke<string>("comms_relay_status");
  },
};
