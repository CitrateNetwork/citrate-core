// 0.5.4 Hermes × FL, WP-H2: the groups and cluster tools (gap analysis §5.1).
// Each tool is driven through runGroupClusterTool with its bridge seams spied; the store-level
// cases drive store.handleTool, the same entry point a Hermes turn uses.
import { describe, it, expect, vi, afterEach } from "vitest";
import { CLUSTER_ROUTE, GROUP_CLUSTER_TOOL_NAMES, MESH_NOT_SIGNED_OFF, runGroupClusterTool, type GroupClusterDeps } from "./groupClusterTools";
import { AGENT_TOOLS, AUTHORITY_LOWERING_AGENT_TOOLS, READ_ONLY_AGENT_TOOLS, type ToolCall } from "./harness";
import { annotationFor } from "./toolAnnotations";
import { store } from "../shell/store";
import { bridge } from "../bridge";
import type { MeshStatus } from "../bridge/domains";

const MEMBER = "0x" + "a".repeat(40);
const OWNER = "0x" + "b".repeat(40);
const LINK = "citrate://invite?g=g1&t=secret-token-value&k=pubkey&e=9999999999";

function deps(over: Partial<{ mesh: MeshStatus; decision: string }> = {}) {
  const mesh: MeshStatus = over.mesh ?? { on: false, source: "off", signedOff: false, note: "The cross-machine mesh is off." };
  const d = {
    invites: {
      preview: vi.fn(async () => ({ group: "g1", groupName: "Night shift", inviter: "Dana", expiresAt: 9999999999, expired: false, requested: false })),
      request: vi.fn(async () => ({ id: "r1", group: "g1", groupName: "Night shift", inviter: "Dana", requestedAt: 1, expiresAt: 2, state: "waiting" as never, note: "" })),
      list: vi.fn(async () => [{ id: "0123456789abcdef", group: "g1", link: LINK, label: "@dana", createdAt: 1, expiresAt: 2, approving: "" }]),
      revoke: vi.fn(async () => ({ notified: false })),
    },
    groups: {
      roster: vi.fn(async () => [
        { address: OWNER, role: "owner" as const },
        { address: MEMBER, role: "member" as const },
      ]),
      assignRole: vi.fn(async () => {}),
      offboard: vi.fn(async () => {}),
    },
    cluster: {
      status: vi.fn(async () => ({ groupId: "g1", online: 1, total: 3, sharedFiles: ["bafy1"] })),
      peers: vi.fn(async () => [{ address: "0xpeer", online: true, link: "relayed" as const }]),
      devices: vi.fn(async () => [{ member: MEMBER, role: "member", online: true, devices: [{ device: "0xdev", index: 1, label: "desk", issuedAt: 1, online: true }] }]),
      meshStatus: vi.fn(async () => mesh),
      setMeshEnabled: vi.fn(async (on: boolean) => ({ ...mesh, on })),
    },
    ask: vi.fn(async () => over.decision ?? "approved"),
    go: vi.fn(),
    toast: vi.fn(),
  };
  return d satisfies GroupClusterDeps;
}

const run = (name: (typeof GROUP_CLUSTER_TOOL_NAMES)[number], args: Record<string, unknown>, d: GroupClusterDeps) =>
  runGroupClusterTool(name, args, annotationFor(name), d);

afterEach(() => vi.restoreAllMocks());

describe("WP-H2: every groups and cluster tool is offered, annotated and classified", () => {
  it("is in AGENT_TOOLS with an annotation; reads are on the read-only list, revoke on the lowers-authority list", () => {
    const names = AGENT_TOOLS.map((t) => t.function.name as string);
    for (const n of GROUP_CLUSTER_TOOL_NAMES) {
      expect(names).toContain(n);
      expect(annotationFor(n), n).not.toBeNull();
    }
    expect([...READ_ONLY_AGENT_TOOLS].filter((n) => (GROUP_CLUSTER_TOOL_NAMES as readonly string[]).includes(n)).sort()).toEqual(
      ["cluster_add_machine_open", "cluster_read", "group_invites_list"],
    );
    expect([...AUTHORITY_LOWERING_AGENT_TOOLS]).toEqual(["group_invite_revoke"]);
  });
});

describe("group_join_propose", () => {
  it("a link core can't read is 'We couldn't find that invite', with no card and no request", async () => {
    const d = deps();
    d.invites.preview.mockRejectedValueOnce(new Error("that is not a Citrate invite link"));
    const out = await run("group_join_propose", { link: "https://evil.example" }, d);
    expect(out.result).toMatch(/^We couldn't find that invite: that is not a Citrate invite link/);
    expect(d.ask).not.toHaveBeenCalled();
    expect(d.invites.request).not.toHaveBeenCalled();
  });

  it("an expired link or one already used to ask sends nothing", async () => {
    const d = deps();
    d.invites.preview.mockResolvedValueOnce({ group: "g1", groupName: "", inviter: "", expiresAt: 1, expired: true, requested: false });
    expect((await run("group_join_propose", { link: LINK }, d)).result).toMatch(/expired/);
    d.invites.preview.mockResolvedValueOnce({ group: "g1", groupName: "", inviter: "", expiresAt: 9, expired: false, requested: true });
    expect((await run("group_join_propose", { link: LINK }, d)).result).toMatch(/already asked/);
    expect(d.ask).not.toHaveBeenCalled();
    expect(d.invites.request).not.toHaveBeenCalled();
  });

  it("asks on a card naming the group and inviter; a decline sends no request", async () => {
    const d = deps({ decision: "declined" });
    const out = await run("group_join_propose", { link: LINK }, d);
    expect(d.ask).toHaveBeenCalledTimes(1);
    const [spec, card] = d.ask.mock.calls[0] as unknown as [{ rows: { k: string; v: string }[] }, { tool: string }];
    expect(spec.rows.map((r) => r.v).join(" ")).toContain("Night shift");
    expect(spec.rows.map((r) => r.v).join(" ")).toContain("Dana");
    expect(spec.rows.map((r) => r.v).join(" ")).toMatch(/only after they approve/);
    expect(card.tool).toBe("group_join_propose");
    expect(d.invites.request).not.toHaveBeenCalled();
    expect(out.result).toMatch(/declined; no request was sent/);
  });

  it("on Approve it only ASKS to join, and never echoes the link or its token", async () => {
    const d = deps();
    const out = await run("group_join_propose", { link: LINK }, d);
    expect(d.invites.request).toHaveBeenCalledWith(LINK);
    expect(out.status).toBe("approved");
    expect(out.result).toMatch(/inviter approves or declines/);
    expect(out.result).not.toContain("secret-token-value");
    expect(out.result).not.toContain("citrate://invite");
    // The group and inviter came from someone else's link: fenced.
    expect(out.result).toContain("<<<UNTRUSTED");
  });
});

describe("group_invites_list and group_invite_revoke", () => {
  it("lists ids, labels and expiry only; the link never reaches the model", async () => {
    const d = deps();
    const out = await run("group_invites_list", { group: "g1" }, d);
    expect(out.result).toContain("0123456789abcdef");
    expect(out.result).toContain("@dana");
    expect(out.result).not.toContain("citrate://invite");
    expect(out.result).not.toContain("secret-token-value");
  });

  it("revoke applies at once with no card (it lowers authority) and calls only revoke", async () => {
    const d = deps();
    const out = await run("group_invite_revoke", { group: "g1", id: "0123456789abcdef" }, d);
    expect(d.ask).not.toHaveBeenCalled();
    expect(d.invites.revoke).toHaveBeenCalledWith("g1", "0123456789abcdef");
    expect(out.result).toMatch(/no longer works/);
    expect(d.toast).toHaveBeenCalledWith(expect.not.stringMatching(/approve/i));
  });

  it("revoke with no id revokes nothing; a core error is passed on", async () => {
    const d = deps();
    expect((await run("group_invite_revoke", { group: "g1" }, d)).result).toMatch(/nothing was revoked/);
    d.invites.revoke.mockRejectedValueOnce(new Error("no open invite with that id"));
    expect((await run("group_invite_revoke", { group: "g1", id: "x" }, d)).result).toBe("The invite was not revoked: no open invite with that id");
  });
});

describe("group_role_propose", () => {
  it("refuses an address not on the roster, the owner, an unknown role and a no-op, all without a card", async () => {
    const d = deps();
    expect((await run("group_role_propose", { group: "g1", member: "0x" + "c".repeat(40), role: "admin" }, d)).result).toMatch(/not on this group's roster/);
    expect((await run("group_role_propose", { group: "g1", member: OWNER, role: "admin" }, d)).result).toMatch(/ownership transfer/);
    expect((await run("group_role_propose", { group: "g1", member: MEMBER, role: "owner" }, d)).result).toMatch(/admin, member, guest or offboard/);
    expect((await run("group_role_propose", { group: "g1", member: MEMBER, role: "member" }, d)).result).toMatch(/already member/);
    expect((await run("group_role_propose", { group: "g1", member: "dana", role: "admin" }, d)).result).toMatch(/0x and 40 hex/);
    expect(d.ask).not.toHaveBeenCalled();
    expect(d.groups.assignRole).not.toHaveBeenCalled();
  });

  it("an unreadable roster proposes nothing", async () => {
    const d = deps();
    d.groups.roster.mockRejectedValueOnce(new Error("comms daemon not running"));
    const out = await run("group_role_propose", { group: "g1", member: MEMBER, role: "admin" }, d);
    expect(out.result).toMatch(/roster could not be read.*comms daemon not running/);
    expect(d.ask).not.toHaveBeenCalled();
  });

  it("a role change asks on a card showing old and new role; Approve sends the comms-key grant", async () => {
    const d = deps();
    const out = await run("group_role_propose", { group: "g1", member: MEMBER.toUpperCase().replace("0X", "0x"), role: "Admin" }, d);
    const spec = d.ask.mock.calls[0][0] as unknown as { title: string; rows: { k: string; v: string }[] };
    expect(spec.title).toBe("Change a member's role");
    expect(spec.rows.find((r) => r.k === "Role")?.v).toBe("member → admin");
    expect(d.groups.assignRole).toHaveBeenCalledWith("g1", MEMBER, "admin");
    expect(out.result).toMatch(/now admin/);
  });

  it("offboard says it cannot be undone; a decline offboards nobody", async () => {
    const d = deps({ decision: "declined" });
    await run("group_role_propose", { group: "g1", member: MEMBER, role: "offboard" }, d);
    const spec = d.ask.mock.calls[0][0] as unknown as { title: string; rows: { k: string; v: string }[] };
    expect(spec.title).toBe("Offboard a member");
    expect(spec.rows.find((r) => r.k === "Undo")?.v).toMatch(/cannot be undone/);
    expect(d.groups.offboard).not.toHaveBeenCalled();
  });

  it("the daemon's refusal is passed on word for word", async () => {
    const d = deps();
    d.groups.assignRole.mockRejectedValueOnce(new Error("Role changes need the 0.6.0 comms daemon; the one this app runs is older."));
    const out = await run("group_role_propose", { group: "g1", member: MEMBER, role: "guest" }, d);
    expect(out.result).toBe("The role did not change: Role changes need the 0.6.0 comms daemon; the one this app runs is older.");
  });
});

describe("cluster_read", () => {
  it("reads status, peers, devices and mesh from the cluster daemon, fenced", async () => {
    const d = deps();
    const out = await run("cluster_read", { group: "g1" }, d);
    expect(d.cluster.status).toHaveBeenCalledWith("g1");
    expect(out.result).toContain("<<<UNTRUSTED");
    expect(out.result).toContain('"online":1');
    expect(out.result).toContain('"total":3');
    expect(out.result).toContain('"link":"relayed"');
    expect(out.result).toContain('"label":"desk"');
    expect(out.result).toContain('"signedOff":false');
  });

  it("a part that fails is unknown with its reason, never an empty list", async () => {
    const d = deps();
    d.cluster.peers.mockRejectedValueOnce(new Error("cluster daemon not bundled"));
    const out = await run("cluster_read", { group: "g1" }, d);
    expect(out.result).toContain('"peers":{"unknown":"cluster daemon not bundled"}');
    expect(out.result).not.toContain('"peers":[]');
    expect(out.result).toContain('"total":3');
  });

  it("without a group it reads only the mesh", async () => {
    const d = deps();
    const out = await run("cluster_read", {}, d);
    expect(d.cluster.status).not.toHaveBeenCalled();
    expect(out.result).toMatch(/^Cross-machine mesh:/);
  });
});

describe("cluster_mesh_propose", () => {
  it("before the CL-S4 sign-off, on is refused truthfully with no card and no switch", async () => {
    const d = deps();
    const out = await run("cluster_mesh_propose", { enabled: true }, d);
    expect(out.result).toBe(MESH_NOT_SIGNED_OFF);
    expect(out.result).toContain("CL-S4");
    expect(d.ask).not.toHaveBeenCalled();
    expect(d.cluster.setMeshEnabled).not.toHaveBeenCalled();
  });

  it("after the sign-off, on asks on a card; a decline leaves it off", async () => {
    const d = deps({ mesh: { on: false, source: "off", signedOff: true, note: "You turned the cross-machine mesh off." }, decision: "declined" });
    const out = await run("cluster_mesh_propose", { enabled: true }, d);
    expect(d.ask).toHaveBeenCalledTimes(1);
    expect(d.cluster.setMeshEnabled).not.toHaveBeenCalled();
    expect(out.result).toMatch(/stays off/);
  });

  it("after the sign-off, an approved on turns it on", async () => {
    const d = deps({ mesh: { on: false, source: "off", signedOff: true, note: "" } });
    await run("cluster_mesh_propose", { enabled: "true" }, d);
    expect(d.cluster.setMeshEnabled).toHaveBeenCalledWith(true);
  });

  it("off needs no card: already off changes nothing, an operator setting is named, a default-on mesh turns off", async () => {
    let d = deps();
    expect((await run("cluster_mesh_propose", { enabled: false }, d)).result).toMatch(/already off/);
    expect(d.cluster.setMeshEnabled).not.toHaveBeenCalled();
    d = deps({ mesh: { on: true, source: "operator", signedOff: false, note: "" } });
    expect((await run("cluster_mesh_propose", { enabled: false }, d)).result).toMatch(/CITRATE_CLUSTER_LISTEN/);
    expect(d.cluster.setMeshEnabled).not.toHaveBeenCalled();
    d = deps({ mesh: { on: true, source: "default", signedOff: true, note: "" } });
    await run("cluster_mesh_propose", { enabled: false }, d);
    expect(d.cluster.setMeshEnabled).toHaveBeenCalledWith(false);
    expect(d.ask).not.toHaveBeenCalled();
  });

  it("an unclear switch changes nothing", async () => {
    const d = deps();
    expect((await run("cluster_mesh_propose", {}, d)).result).toMatch(/nothing changed/);
    expect(d.cluster.meshStatus).not.toHaveBeenCalled();
  });
});

describe("cluster_add_machine_open", () => {
  it("only navigates to Groups › Cluster and returns no link or code", async () => {
    const d = deps();
    const out = await run("cluster_add_machine_open", {}, d);
    expect(d.go).toHaveBeenCalledWith(CLUSTER_ROUTE);
    expect(CLUSTER_ROUTE).toBe("groups/cluster");
    expect(out.result).toMatch(/never to you/);
    expect(out.result).not.toMatch(/citrate-cluster:\/\/|fleet:\/\//);
  });
});

describe("store.handleTool routes the WP-H2 tools", () => {
  const call = (name: string, args: Record<string, unknown>): ToolCall => ({ id: "c1", name, arguments: JSON.stringify(args) });

  it("group_invite_revoke lowers authority: no approval, only the revoke command", async () => {
    const sig = vi.spyOn(store, "requestSig").mockResolvedValue("approved");
    const revoke = vi.spyOn(bridge.invites, "revoke").mockResolvedValue({ notified: false });
    const create = vi.spyOn(bridge.invites, "create");
    const out = await store.handleTool(call("group_invite_revoke", { group: "g1", id: "abc" }), "m1", () => {});
    expect(sig).not.toHaveBeenCalled();
    expect(revoke).toHaveBeenCalledWith("g1", "abc");
    expect(create).not.toHaveBeenCalled();
    expect(out).toMatch(/Revoked the invite/);
  });

  it("group_role_propose asks once (self-gated), even when the call is HIC-required", async () => {
    const sig = vi.spyOn(store, "requestSig").mockResolvedValue("declined");
    vi.spyOn(bridge.groups, "roster").mockResolvedValue([{ address: MEMBER, role: "member" }]);
    const assign = vi.spyOn(bridge.groups, "assignRole").mockResolvedValue();
    await store.handleTool(call("group_role_propose", { group: "g1", member: MEMBER, role: "admin" }), "m1", () => {}, { hic: "required", hicReason: "read untrusted" });
    expect(sig).toHaveBeenCalledTimes(1);
    expect((sig.mock.calls[0][0] as { hic?: unknown }).hic).toBeTruthy();
    expect(assign).not.toHaveBeenCalled();
  });

  it("cluster_mesh_propose on, before the sign-off, opens no card through the store either", async () => {
    const sig = vi.spyOn(store, "requestSig").mockResolvedValue("approved");
    vi.spyOn(bridge.cluster, "meshStatus").mockResolvedValue({ on: false, source: "off", signedOff: false, note: "" });
    const set = vi.spyOn(bridge.cluster, "setMeshEnabled");
    const out = await store.handleTool(call("cluster_mesh_propose", { enabled: true }), "m1", () => {});
    expect(out).toBe(MESH_NOT_SIGNED_OFF);
    expect(sig).not.toHaveBeenCalled();
    expect(set).not.toHaveBeenCalled();
  });
});
