// =====================================================================
// citrate-core — groups slice (CX-S3.4, lane s3)
//
// State for the Groups (chat) surface: the group list, the selected group's roster + messages, and
// transient busy/error flags. Actions call bridge.groups (the CX-S3.2 member-daemon seam) and fold
// results in. Errors are CAUGHT into `error`, never thrown at render — so an un-packaged daemon or an
// unreachable keyring surfaces honestly (Rule 1), never a fabricated room.
//
// `names` is a SESSION id->name map filled on create: the frozen `Group` DTO carries no name (the
// daemon has one; surfacing it across reloads needs an S0 `name` field), so names created this
// session are shown, and older groups fall back to a short id. Owned entirely by lane s3.
// =====================================================================
import { createSlice } from "./createSlice";
import { bridge } from "../../bridge";
import type { Group, GroupMember, GroupMessage, GroupRole } from "../../bridge/domains";

export interface GroupsState {
  /** The groups you are in. */
  groups: Group[];
  /** The selected group's id, or null. */
  selectedId: string | null;
  /** The selected group's roster. */
  roster: GroupMember[];
  /** The selected group's messages, oldest first. */
  messages: GroupMessage[];
  /** Session id->name (from create) — the DTO carries no name; see the module note. */
  names: Record<string, string>;
  /** A group create is in flight. */
  creating: boolean;
  /** A message send is in flight. */
  sending: boolean;
  /** The address of a per-member admin action in flight (assign/offboard), or null. */
  busyMember: string | null;
  /** The last user-facing error, or null when clear. */
  error: string | null;
}

const initial: GroupsState = {
  groups: [],
  selectedId: null,
  roster: [],
  messages: [],
  names: {},
  creating: false,
  sending: false,
  busyMember: null,
  error: null,
};

export const groupsSlice = createSlice<GroupsState>(initial);

const message = (e: unknown): string =>
  e instanceof Error ? e.message : typeof e === "string" ? e : String(e);

/** Load the group list. Honest-empty on a sim / un-provisioned bridge. */
export async function refreshGroups(): Promise<void> {
  try {
    const groups = await bridge.groups.list();
    groupsSlice.set({ groups, error: null });
  } catch (e) {
    groupsSlice.set({ error: message(e) });
  }
}

/** Create a group, remember the name for this session, then select it. */
export async function createGroup(kind: Group["kind"], name: string): Promise<void> {
  groupsSlice.set({ creating: true, error: null });
  try {
    const g = await bridge.groups.create(kind, name);
    groupsSlice.set((s) => ({ creating: false, names: { ...s.names, [g.id]: name } }));
    await refreshGroups();
    await selectGroup(g.id);
  } catch (e) {
    groupsSlice.set({ creating: false, error: message(e) });
  }
}

/** Select a group and load its roster + messages. */
export async function selectGroup(groupId: string): Promise<void> {
  groupsSlice.set({ selectedId: groupId, roster: [], messages: [], error: null });
  try {
    const [roster, messages] = await Promise.all([
      bridge.groups.roster(groupId),
      bridge.groups.messages(groupId),
    ]);
    // Ignore a stale load if the user moved on before it resolved.
    if (groupsSlice.get().selectedId === groupId) {
      groupsSlice.set({ roster, messages });
    }
  } catch (e) {
    if (groupsSlice.get().selectedId === groupId) groupsSlice.set({ error: message(e) });
  }
}

/** Send a message to the selected group, then reload its messages. */
export async function sendMessage(body: string): Promise<void> {
  const groupId = groupsSlice.get().selectedId;
  if (!groupId || !body.trim()) return;
  groupsSlice.set({ sending: true, error: null });
  try {
    await bridge.groups.send(groupId, body);
    groupsSlice.set({ sending: false });
    await reloadMessages(groupId);
  } catch (e) {
    groupsSlice.set({ sending: false, error: message(e) });
  }
}

/** Reload messages for a group (send follow-up / poll). No-op if the selection moved on. */
export async function reloadMessages(groupId: string): Promise<void> {
  try {
    const messages = await bridge.groups.messages(groupId);
    if (groupsSlice.get().selectedId === groupId) groupsSlice.set({ messages });
  } catch (e) {
    if (groupsSlice.get().selectedId === groupId) groupsSlice.set({ error: message(e) });
  }
}

/** Grant/change a member's role (owner-signed RoleAssertion, enforced at the relay). */
export async function assignRole(address: string, role: GroupRole): Promise<void> {
  const groupId = groupsSlice.get().selectedId;
  if (!groupId) return;
  groupsSlice.set({ busyMember: address, error: null });
  try {
    await bridge.groups.assignRole(groupId, address, role);
    groupsSlice.set({ busyMember: null });
    groupsSlice.set({ roster: await bridge.groups.roster(groupId) });
  } catch (e) {
    groupsSlice.set({ busyMember: null, error: message(e) });
  }
}

/** Remove a member (atomic offboard — drops all four planes in one epoch, ADR-001). */
export async function offboardMember(address: string): Promise<void> {
  const groupId = groupsSlice.get().selectedId;
  if (!groupId) return;
  groupsSlice.set({ busyMember: address, error: null });
  try {
    await bridge.groups.offboard(groupId, address);
    groupsSlice.set({ busyMember: null });
    groupsSlice.set({ roster: await bridge.groups.roster(groupId) });
  } catch (e) {
    groupsSlice.set({ busyMember: null, error: message(e) });
  }
}

/** Owner-invite a member (who has published a key package to the relay), then reload the roster. */
export async function addMemberToGroup(address: string): Promise<void> {
  const groupId = groupsSlice.get().selectedId;
  const addr = address.trim();
  if (!groupId || !addr) return;
  groupsSlice.set({ busyMember: addr, error: null });
  try {
    await bridge.groups.addMember(groupId, addr);
    groupsSlice.set({ busyMember: null });
    groupsSlice.set({ roster: await bridge.groups.roster(groupId) });
  } catch (e) {
    groupsSlice.set({ busyMember: null, error: message(e) });
  }
}

/** Join a group you were added to on a shared relay. */
export async function joinGroup(groupId: string): Promise<void> {
  groupsSlice.set({ error: null });
  try {
    await bridge.groups.join(groupId);
    await refreshGroups();
    await selectGroup(groupId);
  } catch (e) {
    groupsSlice.set({ error: message(e) });
  }
}

/** The display label for a group: its name (now on the DTO), else a session name, else a short id. */
export function groupLabel(state: GroupsState, g: Group): string {
  return g.name || state.names[g.id] || `${g.id.slice(0, 10)}…`;
}
