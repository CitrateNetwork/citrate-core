// =====================================================================
// citrate-core — Groups › Members: who may change whose role (0.5.4 FL design, board 03).
//
// Pure, so the table, the ceremony and the tests read one rule. The rule is the comms-member-daemon's
// (citrate-comms `grant_role` and `check_role_grant`, checked again by every member that receives the
// grant): an owner or admin assigns roles; only the owner makes or unmakes an admin; nobody changes
// the owner's role here (that is an ownership transfer); an agent seat belongs to its sponsor path.
// Offboarding is a membership change (RemoveMember), which owners and admins hold; an admin is
// offboarded by the owner only. The daemon still decides: a refusal it sends is shown as is.
//
// Data source (Rule 7): the group roster from the comms-member-daemon (`groups_roster`), keyed on
// comms addresses, with the role grants of #637 applied.
// =====================================================================
import type { GroupRole } from "../../bridge/domains";

/** The member-facing word for a role. The daemon's `guest` can read and not post: "read-only". */
export function roleWord(role: GroupRole | string): string {
  if (role === "guest") return "read-only";
  return role;
}

export type RoleTarget = Exclude<GroupRole, "owner" | "agent">;

export interface RoleAction {
  /** The button label. */
  label: "Make admin" | "Make member" | "Read-only" | "Offboard";
  /** The role the grant sets, or "removed" for an offboard. */
  to: RoleTarget | "removed";
  /** Offboarding cannot be undone. */
  danger: boolean;
}

export interface RoleActions {
  actions: RoleAction[];
  /** Why there are no actions, when there are none. */
  none: string | null;
}

const OFFBOARD: RoleAction = { label: "Offboard", to: "removed", danger: true };

/**
 * The actions `myRole` may take on a member holding `target`. `isSelf` is that member being you.
 * A null `myRole` (the roster has not keyed your seat yet) offers nothing.
 */
export function roleActions(myRole: GroupRole | null, target: GroupRole, isSelf: boolean): RoleActions {
  if (isSelf) return { actions: [], none: "you" };
  if (target === "owner") return { actions: [], none: "owner" };
  if (target === "agent") return { actions: [], none: "agent seat, managed by its sponsor" };
  if (myRole === "owner") {
    if (target === "admin") return { actions: [make("member"), make("guest"), OFFBOARD], none: null };
    if (target === "member") return { actions: [make("admin"), make("guest"), OFFBOARD], none: null };
    return { actions: [make("member"), make("admin"), OFFBOARD], none: null };
  }
  if (myRole === "admin") {
    if (target === "admin") return { actions: [], none: "only the owner changes an admin" };
    if (target === "member") return { actions: [make("guest"), OFFBOARD], none: null };
    return { actions: [make("member"), OFFBOARD], none: null };
  }
  return { actions: [], none: "owners and admins assign roles" };
}

function make(to: RoleTarget): RoleAction {
  if (to === "admin") return { label: "Make admin", to, danger: false };
  if (to === "member") return { label: "Make member", to, danger: false };
  return { label: "Read-only", to, danger: false };
}

/** Owners and admins invite and add people (the relay checks AddMember). */
export function canInvite(myRole: GroupRole | null): boolean {
  return myRole === "owner" || myRole === "admin";
}

/**
 * Your role in a group. The durable source is the comms-keyed navigator row (`navRole`, from every
 * group's roster matched on your comms address); then your seat in the loaded roster; then the
 * group's owner field. Null when none of them knows yet, never a guessed role.
 */
export function myRoleIn(
  navRole: GroupRole | null | undefined,
  roster: { address: string; role: GroupRole }[],
  owner: string,
  me: string,
): GroupRole | null {
  if (navRole) return navRole;
  const self = me.toLowerCase();
  if (!self) return null;
  const seat = roster.find((r) => r.address.toLowerCase() === self);
  if (seat) return seat.role;
  if (owner && owner.toLowerCase() === self) return "owner";
  return null;
}

/** The ceremony title for an action on `who`. */
export function ceremonyTitle(a: RoleAction, who: string): string {
  if (a.to === "removed") return `Offboard ${who}`;
  if (a.to === "admin") return `Make ${who} an admin`;
  if (a.to === "member") return `Make ${who} a member`;
  return `Make ${who} read-only`;
}

/** Comms addresses compare without `0x` and case (the cluster daemon drops the prefix). */
export const sameAddress = (a: string, b: string): boolean =>
  a.replace(/^0x/i, "").toLowerCase() === b.replace(/^0x/i, "").toLowerCase();

export interface MemberPresence {
  /** "2 devices", or "—" when the cluster daemon has not reported this member. */
  devices: string;
  /** "online" / "not connected" from the live cluster state, or "—". */
  seen: string;
  /** The reason a "—" is shown, when it is. */
  why: string | null;
}

/**
 * A member's devices and presence from the cluster daemon's live answer (`cluster_devices`).
 * `devices` is null when that read has not succeeded (not joined, daemon not running): every
 * member then shows "—" with the reason, never zero devices.
 */
export function presenceOf(
  address: string,
  devices: { member: string; online: boolean; devices: { online: boolean }[] }[] | null,
  unreadReason: string,
): MemberPresence {
  if (!devices) return { devices: "—", seen: "—", why: unreadReason };
  const row = devices.find((d) => sameAddress(d.member, address));
  if (!row) return { devices: "—", seen: "—", why: "not reported by the group's cluster" };
  const n = row.devices.length;
  const on = row.online || row.devices.some((d) => d.online);
  return { devices: n === 0 ? "no linked devices" : `${n} device${n === 1 ? "" : "s"}`, seen: on ? "online" : "not connected", why: null };
}
