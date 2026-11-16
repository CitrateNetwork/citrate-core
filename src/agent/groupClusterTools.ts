// =====================================================================
// 0.5.4 Hermes × FL, WP-H2: Hermes's groups and cluster tools (gap analysis §5.1).
//
//   group_join_propose       ask to join a group with an invite link the member pasted  (HIC-1 card)
//   group_invites_list       the group's open invites made on this device, no links     (read)
//   group_invite_revoke      revoke one of those invites                                (no card: lowers authority)
//   group_role_propose       change a member's role, or offboard them                  (HIC-1 card, never in an envelope)
//   cluster_read             the group's cluster: status, peers, devices, mesh on or off (read)
//   cluster_mesh_propose     turn the cross-machine mesh on (HIC-1) or off (no card)    (refuses on until CL-S4)
//   cluster_add_machine_open open Groups › Cluster, where Add a machine pairs a device   (navigate only, no seed)
//
// Data sources (Rule 7), all real core commands over the bridge:
//  - invites: src-tauri/src/invites.rs (group_invite_preview, group_invite_request, group_invites,
//    group_invite_revoke). A request only ASKS: the inviter approves or declines it in Groups.
//  - roles: src-tauri/src/comms.rs (groups_roster, groups_assign_role, groups_offboard); the
//    comms-member-daemon signs a role grant with this device's comms key and refuses what this
//    member may not do. Its refusal is passed on word for word.
//  - cluster: src-tauri/src/cluster.rs (cluster_status, cluster_peers, cluster_devices) and
//    src-tauri/src/cluster_mesh.rs (cluster_mesh_status, cluster_mesh_set_enabled). Core refuses the
//    member's mesh switch until the CL-S4 transport sign-off is recorded (TRANSPORT_SIGNED_OFF).
//
// Safety (HIC): Hermes never signs and never decides. Raising authority (a join request, a role
// change, an offboard, mesh on) waits for the member's Approve on a card; lowering it (revoking an
// invite, mesh off) applies at once. An invite link or a pairing link never comes back to the
// model: invite listings carry ids and labels only, and Add a machine shows its link and code on
// screen only. Text other people can write (group names, inviter names, device labels, roster
// addresses) reaches the model fenced as untrusted data.
// =====================================================================
import type { ApprovalCard } from "./approvalCards";
import { fieldsCard } from "./approvalCards";
import type { ToolAnnotation } from "./toolAnnotations";
import { fenceUntrusted } from "./untrusted";
import type { CerSpec } from "../shell/state";
import type {
  ClusterMemberDevices,
  ClusterPeer,
  ClusterStatus,
  GroupMember,
  InviteNotice,
  InvitePreview,
  InviteView,
  MeshStatus,
  MyInviteRequest,
} from "../bridge/domains";

export const GROUP_CLUSTER_TOOL_NAMES = [
  "group_join_propose",
  "group_invites_list",
  "group_invite_revoke",
  "group_role_propose",
  "cluster_read",
  "cluster_mesh_propose",
  "cluster_add_machine_open",
] as const;
export type GroupClusterToolName = (typeof GROUP_CLUSTER_TOOL_NAMES)[number];

export function isGroupClusterTool(name: string): name is GroupClusterToolName {
  return (GROUP_CLUSTER_TOOL_NAMES as readonly string[]).includes(name);
}

/** The roles a role change may give (comms.rs grant_role_request); owner and agent change elsewhere. */
export const GRANTABLE_ROLES = ["admin", "member", "guest"] as const;

/** The Groups › Cluster place (src/shell/places.ts). */
export const CLUSTER_ROUTE = "groups/cluster";

/** Core's refusal before the CL-S4 sign-off, in the member's words (cluster_mesh.rs). */
export const MESH_NOT_SIGNED_OFF =
  "The cross-machine mesh can't be turned on yet: its security review (CL-S4) is not signed off in this build. Nothing changed.";

export interface GroupClusterDeps {
  invites: {
    preview(link: string): Promise<InvitePreview>;
    request(link: string): Promise<MyInviteRequest>;
    list(group: string): Promise<InviteView[]>;
    revoke(group: string, id: string): Promise<InviteNotice>;
  };
  groups: {
    roster(group: string): Promise<GroupMember[]>;
    assignRole(group: string, address: string, role: string): Promise<void>;
    offboard(group: string, address: string): Promise<void>;
  };
  cluster: {
    status(group: string): Promise<ClusterStatus>;
    peers(group: string): Promise<ClusterPeer[]>;
    devices(group: string): Promise<ClusterMemberDevices[]>;
    meshStatus(): Promise<MeshStatus>;
    setMeshEnabled(enabled: boolean): Promise<MeshStatus>;
  };
  /** Ask the member (the SignatureCeremony queue). Resolves "approved" only on an explicit Approve. */
  ask: (spec: CerSpec, card: ApprovalCard) => Promise<string>;
  /** Move the app to a place. */
  go: (route: string) => void;
  /** A text-only notice (never carries an approve button). */
  toast: (text: string) => void;
}

export interface GroupClusterOutcome {
  /** "ok" for reads and no-card writes, or the member's decision on a card. */
  status: string;
  result: string;
}

const str = (v: unknown): string => (typeof v === "string" ? v.trim() : "");
const errText = (e: unknown): string => (e instanceof Error ? e.message : String(e));
const ADDRESS = /^0x[0-9a-fA-F]{40}$/;

function spec(tool: string, title: string, rows: { k: string; v: string }[]): CerSpec {
  return {
    origin: "chat agent",
    requester: "dashboard agent · tool " + tool,
    title,
    chainless: true,
    rows,
    cost: "none, no chain transaction",
    sponsor: "no chain transaction",
    sponsorColor: "var(--tx-3)",
  };
}

function declined(r: string, what: string): GroupClusterOutcome {
  return {
    status: r,
    result: r === "expired" ? `The approval expired because the run ended; ${what}` : `The member declined; ${what}`,
  };
}

/** Run one of the WP-H2 tools. `ann` is the tool's reviewed annotation (for its card). */
export async function runGroupClusterTool(
  name: GroupClusterToolName,
  args: Record<string, unknown>,
  ann: ToolAnnotation | null,
  d: GroupClusterDeps,
): Promise<GroupClusterOutcome> {
  switch (name) {
    case "group_join_propose":
      return joinPropose(args, ann, d);
    case "group_invites_list":
      return invitesList(args, d);
    case "group_invite_revoke":
      return inviteRevoke(args, d);
    case "group_role_propose":
      return rolePropose(args, ann, d);
    case "cluster_read":
      return clusterRead(args, d);
    case "cluster_mesh_propose":
      return meshPropose(args, ann, d);
    case "cluster_add_machine_open":
      d.go(CLUSTER_ROUTE);
      return {
        status: "ok",
        result:
          "Opened Groups › Cluster. Tell the member to press Add a machine there: core makes a one-time pairing link and a six-digit code, shown on their screen only and never to you, and both machines must approve the pairing.",
      };
  }
}

async function joinPropose(args: Record<string, unknown>, ann: ToolAnnotation | null, d: GroupClusterDeps): Promise<GroupClusterOutcome> {
  const link = str(args.link);
  if (!link) return { status: "ok", result: "Ask the member to paste the invite link they were sent; nothing was requested." };
  let p: InvitePreview;
  try {
    p = await d.invites.preview(link);
  } catch (e) {
    return { status: "ok", result: "We couldn't find that invite: " + errText(e) + ". Nothing was requested." };
  }
  if (p.expired) return { status: "ok", result: "That invite has expired. Ask whoever sent it for a new link; nothing was requested." };
  if (p.requested) {
    return { status: "ok", result: "This device already asked to join with that link. The inviter approves or declines it; Groups shows where it stands." };
  }
  const group = p.groupName || "this group";
  const r = await d.ask(
    spec("group_join_propose", "Ask to join a group", [
      { k: "Group", v: p.groupName ? "“" + p.groupName + "”" : p.group },
      { k: "Invited by", v: p.inviter || "not named in the link" },
      { k: "Effect", v: "your request goes to the inviter; you join only after they approve it" },
    ]),
    fieldsCard("group_join_propose", ann, { group: p.groupName || p.group, inviter: p.inviter || "(not named)" }, "ask to join " + group),
  );
  if (r !== "approved") return declined(r, "no request was sent.");
  try {
    const req = await d.invites.request(link);
    return {
      status: r,
      result:
        "Sent the request to join. The inviter approves or declines it; the member joins only after an approval, and Groups shows where it stands. " +
        fenceUntrusted("group from the invite", { group: req.groupName || req.group, inviter: req.inviter, state: req.state }),
    };
  } catch (e) {
    return { status: r, result: "The request to join was not sent: " + errText(e) };
  }
}

async function invitesList(args: Record<string, unknown>, d: GroupClusterDeps): Promise<GroupClusterOutcome> {
  const group = str(args.group);
  if (!group) return { status: "ok", result: "Name the group (its id from groups_list); nothing was read." };
  try {
    const list = await d.invites.list(group);
    if (list.length === 0) return { status: "ok", result: "This device has no open invites for that group." };
    // Ids and labels only: a link never reaches the model.
    const rows = list.map((i) => ({ id: i.id, for: i.label || "(no label)", expiresAt: new Date(i.expiresAt * 1000).toISOString(), beingApproved: i.approving !== "" }));
    return { status: "ok", result: fenceUntrusted("open invites made on this device", rows) };
  } catch (e) {
    return { status: "ok", result: "The invites could not be read: " + errText(e) };
  }
}

async function inviteRevoke(args: Record<string, unknown>, d: GroupClusterDeps): Promise<GroupClusterOutcome> {
  const group = str(args.group);
  const id = str(args.id);
  if (!group || !id) return { status: "ok", result: "Name the group and the invite id (from group_invites_list); nothing was revoked." };
  try {
    const n = await d.invites.revoke(group, id);
    d.toast("Invite revoked. Its link no longer works.");
    return {
      status: "ok",
      result: "Revoked the invite: its link no longer works and no request under it can be approved." + (n.notified ? " Someone who had asked with it was told." : ""),
    };
  } catch (e) {
    return { status: "ok", result: "The invite was not revoked: " + errText(e) };
  }
}

async function rolePropose(args: Record<string, unknown>, ann: ToolAnnotation | null, d: GroupClusterDeps): Promise<GroupClusterOutcome> {
  const group = str(args.group);
  const member = str(args.member);
  const role = str(args.role).toLowerCase();
  const offboard = role === "offboard";
  if (!group) return { status: "ok", result: "Name the group (its id from groups_list); nothing changed." };
  if (!ADDRESS.test(member)) return { status: "ok", result: "Name the member by their address from group_roster (0x and 40 hex digits); nothing changed." };
  if (!offboard && !(GRANTABLE_ROLES as readonly string[]).includes(role)) {
    return { status: "ok", result: "The role must be admin, member, guest or offboard; nothing changed." };
  }
  let roster: GroupMember[];
  try {
    roster = await d.groups.roster(group);
  } catch (e) {
    return { status: "ok", result: "The roster could not be read, so nothing was proposed: " + errText(e) };
  }
  const row = roster.find((m) => m.address.toLowerCase() === member.toLowerCase());
  if (!row) return { status: "ok", result: "That address is not on this group's roster; nothing changed." };
  if (row.role === "owner") return { status: "ok", result: "The owner's role changes only by an ownership transfer; nothing changed." };
  if (!offboard && row.role === role) return { status: "ok", result: `That member is already ${role}; nothing changed.` };
  const rows = offboard
    ? [
        { k: "Group", v: group },
        { k: "Member", v: row.address },
        { k: "Effect", v: "removes them from the roster and the group's encryption, so they can't read new messages" },
        { k: "Undo", v: "this cannot be undone; they need a new invite to come back" },
      ]
    : [
        { k: "Group", v: group },
        { k: "Member", v: row.address },
        { k: "Role", v: row.role + " → " + role },
        { k: "Signed by", v: "your comms key on this device; every member checks the grant" },
      ];
  const r = await d.ask(
    spec("group_role_propose", offboard ? "Offboard a member" : "Change a member's role", rows),
    fieldsCard(
      "group_role_propose",
      ann,
      { group, member: row.address, change: offboard ? "offboard (cannot be undone)" : row.role + " → " + role },
      offboard ? "offboard " + row.address + " from the group" : "make " + row.address + " " + role,
    ),
  );
  if (r !== "approved") return declined(r, "nothing changed.");
  try {
    if (offboard) {
      await d.groups.offboard(group, row.address);
      return { status: r, result: "Offboarded that member: they are off the roster and out of the group's encryption." };
    }
    await d.groups.assignRole(group, row.address, role);
    return { status: r, result: `Sent the role grant: that member is now ${role}, signed with this device's comms key.` };
  } catch (e) {
    return { status: r, result: (offboard ? "The member was not offboarded: " : "The role did not change: ") + errText(e) };
  }
}

/** One part of a cluster read: its value, or why it is unknown (never an invented empty list). */
async function part<T>(read: () => Promise<T>): Promise<{ ok: T } | { unknown: string }> {
  try {
    return { ok: await read() };
  } catch (e) {
    return { unknown: errText(e) };
  }
}

async function clusterRead(args: Record<string, unknown>, d: GroupClusterDeps): Promise<GroupClusterOutcome> {
  const group = str(args.group);
  const mesh = await part(() => d.cluster.meshStatus());
  const meshOut = "ok" in mesh ? { on: mesh.ok.on, source: mesh.ok.source, signedOff: mesh.ok.signedOff, note: mesh.ok.note } : { unknown: mesh.unknown };
  if (!group) {
    return {
      status: "ok",
      result: "Cross-machine mesh: " + JSON.stringify(meshOut) + ". Name a group (its id from groups_list) to read its cluster.",
    };
  }
  const [status, peers, devices] = await Promise.all([part(() => d.cluster.status(group)), part(() => d.cluster.peers(group)), part(() => d.cluster.devices(group))]);
  const out = {
    mesh: meshOut,
    status: "ok" in status ? { online: status.ok.online, total: status.ok.total, sharedFiles: status.ok.sharedFiles.length } : { unknown: status.unknown },
    peers:
      "ok" in peers
        ? peers.ok.map((p) => ({ address: p.address, online: p.online, ...(p.member ? { deviceOf: p.member } : {}), link: p.link ?? (p.online ? "not reported" : "offline") }))
        : { unknown: peers.unknown },
    devices:
      "ok" in devices
        ? devices.ok.map((m) => ({ member: m.member, role: m.role, online: m.online, devices: m.devices.map((x) => ({ label: x.label, index: x.index, online: x.online })) }))
        : { unknown: devices.unknown },
  };
  return { status: "ok", result: "Cluster of group " + group + " (from the cluster daemon): " + fenceUntrusted("cluster state", out) };
}

async function meshPropose(args: Record<string, unknown>, ann: ToolAnnotation | null, d: GroupClusterDeps): Promise<GroupClusterOutcome> {
  const raw = args.enabled;
  const enabled = raw === true || raw === "true" ? true : raw === false || raw === "false" ? false : null;
  if (enabled === null) return { status: "ok", result: "Say enabled true (turn the mesh on) or false (turn it off); nothing changed." };
  let now: MeshStatus;
  try {
    now = await d.cluster.meshStatus();
  } catch (e) {
    return { status: "ok", result: "The mesh status could not be read, so nothing changed: " + errText(e) };
  }
  if (!enabled) {
    // Lowering authority needs no card. Before the sign-off the member's switch does not exist yet.
    if (!now.on) return { status: "ok", result: "The cross-machine mesh is already off; nothing changed. " + now.note };
    if (now.source === "operator") {
      return { status: "ok", result: "An operator setting on this machine (CITRATE_CLUSTER_LISTEN) turned the mesh on; the app can't turn that off. Nothing changed." };
    }
    try {
      const s = await d.cluster.setMeshEnabled(false);
      d.toast("Cross-machine mesh turned off. It applies at the next cluster start.");
      return { status: "ok", result: "Turned the cross-machine mesh off; it applies at the next cluster start. " + s.note };
    } catch (e) {
      return { status: "ok", result: "The mesh was not turned off: " + errText(e) };
    }
  }
  if (!now.signedOff) return { status: "ok", result: MESH_NOT_SIGNED_OFF };
  if (now.on) return { status: "ok", result: "The cross-machine mesh is already on; nothing changed. " + now.note };
  const r = await d.ask(
    spec("cluster_mesh_propose", "Turn on the cross-machine mesh", [
      { k: "Effect", v: "this machine listens for the machines of your groups' members; the roster and device links decide who is let in" },
      { k: "When", v: "at the next cluster start" },
      { k: "Undo", v: "turn it off any time in Groups › Cluster" },
    ]),
    fieldsCard("cluster_mesh_propose", ann, { mesh: "on" }, "turn on the cross-machine mesh"),
  );
  if (r !== "approved") return declined(r, "the mesh stays off.");
  try {
    const s = await d.cluster.setMeshEnabled(true);
    return { status: r, result: "Turned the cross-machine mesh on; it applies at the next cluster start. " + s.note };
  } catch (e) {
    return { status: r, result: "The mesh was not turned on: " + errText(e) };
  }
}
