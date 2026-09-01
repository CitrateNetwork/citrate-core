// CONNECT-S4 — the groups & clusters role navigator (pure, testable).
//
// A single list of every group you're in, badged with YOUR role in each, so you can see at a glance
// where you're an owner/admin vs a member and jump straight to it. It is derived from the SAME live
// data the People directory reads (each group's roster + your own comms address) — never fabricated
// (Rule 1). Your role is read from the roster (the relay's source of truth), keyed on your comms
// address; a group whose roster hasn't loaded contributes a null role (shown as "—"), never a guessed
// one. `iCreated` (a group made THIS session) counts as manage-capable even before its roster keys
// your seat — the same seam the Groups surface papers over — but it never asserts an authorization the
// relay wouldn't (RBAC is still enforced there).

import type { Group, GroupRole } from "../bridge/domains";

export interface GroupRoleRow {
  id: string;
  /** Display name (session `names` map wins over the frozen DTO's name, matching the Groups rail). */
  name: string;
  kind: Group["kind"];
  /** Your role in this group, read from its roster (keyed on your comms address). null = roster not
   *  loaded yet → the UI shows "—", never a fabricated role. */
  myRole: GroupRole | null;
  /** You can manage this group (owner/admin, or you created it this session). Drives "where I'm admin". */
  iManage: boolean;
}

const RANK: Record<string, number> = { owner: 0, admin: 1, member: 2, guest: 3, agent: 4 };

/**
 * Build the role navigator from live data. `rosterByGroup[groupId]` is that group's `(address, role)`
 * roster (same map the People directory builds); `selfAddr` is your comms address (roster key);
 * `sessionNames` is the session id→name map (created-this-session groups) so names match the Groups
 * rail; `createdIds` are groups you created this session (manage-capable even pre-roster). Deterministic
 * order: manage-capable first, then by role rank, then by name — so the groups you run surface at top.
 */
export function buildRoleNavigator(
  groups: { id: string; name: string; kind: Group["kind"] }[],
  rosterByGroup: Record<string, { address: string; role: GroupRole }[]>,
  selfAddr: string,
  sessionNames: Record<string, string> = {},
  createdIds: string[] = [],
): GroupRoleRow[] {
  const self = (selfAddr || "").toLowerCase();
  const created = new Set(createdIds);
  const rows: GroupRoleRow[] = groups.map((g) => {
    const roster = rosterByGroup[g.id];
    const seat = roster?.find((m) => m.address.toLowerCase() === self);
    const myRole = seat ? seat.role : null;
    const iCreated = created.has(g.id);
    const iManage = myRole === "owner" || myRole === "admin" || iCreated;
    return { id: g.id, name: sessionNames[g.id] || g.name || g.id, kind: g.kind, myRole, iManage };
  });
  return rows.sort((a, b) => {
    if (a.iManage !== b.iManage) return a.iManage ? -1 : 1;
    const ra = a.myRole ? RANK[a.myRole] ?? 9 : 9;
    const rb = b.myRole ? RANK[b.myRole] ?? 9 : 9;
    if (ra !== rb) return ra - rb;
    return a.name.toLowerCase().localeCompare(b.name.toLowerCase());
  });
}

/** The groups you can manage — owner/admin, or created this session. This is the "where I'm admin"
 *  filter (owner is a strict superset of admin authority, so it's included). */
export function managedGroups(rows: GroupRoleRow[]): GroupRoleRow[] {
  return rows.filter((r) => r.iManage);
}

/** Count by role bucket for a compact summary line ("2 you run · 5 total"). Honest over a partial
 *  list: rows with a null role count only toward `total`. */
export function navigatorSummary(rows: GroupRoleRow[]): { managed: number; total: number } {
  return { managed: rows.filter((r) => r.iManage).length, total: rows.length };
}
