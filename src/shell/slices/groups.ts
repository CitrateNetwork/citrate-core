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
  /** The selected group's messages, oldest first (the retained history for that group). */
  messages: GroupMessage[];
  /** Per-group retained history. `bridge.groups.messages` DRAINS the daemon mailbox, so each poll
   *  returns only NEW messages and the daemon does not keep them — the app is the source of truth for
   *  history. We accumulate here (deduped, capped) + echo the member's own sends, and persist locally
   *  so a re-select or restart shows the conversation instead of an empty room. */
  history: Record<string, GroupMessage[]>;
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

// Retained chat history lives on the member's own device (server-blind): the daemon DRAINS its mailbox
// on Poll, so if we do not keep what we drain, a re-select or restart shows an empty room. We cap per
// group to bound growth and persist across restarts.
const HISTORY_KEY = "citrate.groups.history.v1";
const HISTORY_CAP = 500;

function loadHistory(): Record<string, GroupMessage[]> {
  try {
    const raw = typeof localStorage !== "undefined" ? localStorage.getItem(HISTORY_KEY) : null;
    const parsed = raw ? JSON.parse(raw) : {};
    return parsed && typeof parsed === "object" ? (parsed as Record<string, GroupMessage[]>) : {};
  } catch {
    return {};
  }
}

function saveHistory(history: Record<string, GroupMessage[]>): void {
  try {
    if (typeof localStorage !== "undefined") localStorage.setItem(HISTORY_KEY, JSON.stringify(history));
  } catch {
    /* quota / unavailable — retention degrades to in-session only, never throws at the caller */
  }
}

/** Stable identity for dedup: prefer the daemon-assigned id, else sender+ts+body. */
const msgKey = (m: GroupMessage): string => m.id || `${m.sender}|${m.ts}|${m.body}`;

/**
 * Merge freshly-drained (or optimistic self) messages into a group's retained history, dedup by key,
 * keep oldest-first order, cap length, persist, and reflect into `messages` if that group is selected.
 */
function mergeIntoHistory(groupId: string, incoming: GroupMessage[]): void {
  if (!incoming.length) return;
  const s = groupsSlice.get();
  const prior = s.history[groupId] ?? [];
  const seen = new Set(prior.map(msgKey));
  const fresh = incoming.filter((m) => !seen.has(msgKey(m)));
  if (!fresh.length) return;
  const merged = prior
    .concat(fresh)
    .sort((a, b) => (a.ts || 0) - (b.ts || 0))
    .slice(-HISTORY_CAP);
  const history = { ...s.history, [groupId]: merged };
  saveHistory(history);
  const patch: Partial<GroupsState> = { history };
  if (s.selectedId === groupId) patch.messages = merged;
  groupsSlice.set(patch);
}

// The member's own comms address, cached — used to attribute an optimistic echo of a sent message so
// the sender sees it immediately (the relay delivers only to OTHER members, never back to the sender).
let selfAddrCache: string | null = null;
async function selfAddress(): Promise<string> {
  if (selfAddrCache) return selfAddrCache;
  try {
    selfAddrCache = await bridge.groups.selfAddress();
  } catch {
    selfAddrCache = "you";
  }
  return selfAddrCache;
}

const initial: GroupsState = {
  groups: [],
  selectedId: null,
  roster: [],
  messages: [],
  history: loadHistory(),
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

/** Select a group and load its roster + messages. Shows retained history immediately, then drains. */
export async function selectGroup(groupId: string): Promise<void> {
  // Show any retained history for this group at once — never flash an empty room while the drain runs.
  const retained = groupsSlice.get().history[groupId] ?? [];
  groupsSlice.set({ selectedId: groupId, roster: [], messages: retained, error: null });
  try {
    const [roster, drained] = await Promise.all([
      bridge.groups.roster(groupId),
      bridge.groups.messages(groupId),
    ]);
    // Ignore a stale load if the user moved on before it resolved.
    if (groupsSlice.get().selectedId === groupId) {
      groupsSlice.set({ roster });
      mergeIntoHistory(groupId, drained);
    }
  } catch (e) {
    if (groupsSlice.get().selectedId === groupId) groupsSlice.set({ error: message(e) });
  }
}

/** Send a message to the selected group. Echoes it locally at once, then relays + drains. */
export async function sendMessage(body: string): Promise<void> {
  const groupId = groupsSlice.get().selectedId;
  const text = body.trim();
  if (!groupId || !text) return;
  groupsSlice.set({ sending: true, error: null });
  // Optimistic echo: the relay delivers a message only to OTHER members, never back to the sender,
  // so the sender's own message must be shown locally or it appears to vanish.
  const me = await selfAddress();
  const ts = Date.now();
  const optimistic: GroupMessage = {
    // ts in the id keeps two identical messages distinct (otherwise dedup would collapse them to one).
    id: `local-${me}-${ts}`,
    groupId,
    sender: me,
    body: text,
    ts,
  };
  mergeIntoHistory(groupId, [optimistic]);
  try {
    await bridge.groups.send(groupId, text);
    groupsSlice.set({ sending: false });
    await reloadMessages(groupId);
  } catch (e) {
    groupsSlice.set({ sending: false, error: message(e) });
  }
}

/** Drain new messages for a group and fold them into retained history. No-op if selection moved on. */
export async function reloadMessages(groupId: string): Promise<void> {
  try {
    const drained = await bridge.groups.messages(groupId);
    mergeIntoHistory(groupId, drained);
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
