// #389 — member actions (offboard, role, add) act on the group they were started for, never on the
// group that happens to be selected when an approval finally runs; and a group switch mid-load never
// shows or acts on the previous group's invite rows.
import { describe, it, expect, vi, afterEach } from "vitest";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { bridge } from "../../bridge";
import { addMemberToGroup, assignRole, groupsSlice, offboardMember } from "./groups";
import { loadOwnerInvites } from "../../surfaces/ownerInvites";
import type { PendingInvite } from "../../bridge/domains";

afterEach(() => {
  vi.restoreAllMocks();
  groupsSlice.set({ selectedId: null, roster: [], busyMember: null, error: null });
});

const pending = (group: string): PendingInvite => ({ group, token: "t", forHandle: "h", createdAt: 1 });

describe("member actions take the row's group", () => {
  it("Remove started on g1, approved after switching to g2, offboards from g1", async () => {
    const off = vi.spyOn(bridge.groups, "offboard").mockResolvedValue(undefined);
    const roster = vi.spyOn(bridge.groups, "roster").mockResolvedValue([]);
    groupsSlice.set({ selectedId: "g2" }); // the member switched groups before approving
    await offboardMember("g1", "0xa");
    expect(off).toHaveBeenCalledWith("g1", "0xa");
    expect(roster).not.toHaveBeenCalledWith("g2");
  });

  it("role changes and adds also use the group they were started for", async () => {
    const role = vi.spyOn(bridge.groups, "assignRole").mockResolvedValue(undefined);
    const add = vi.spyOn(bridge.groups, "addMember").mockResolvedValue(undefined);
    vi.spyOn(bridge.groups, "roster").mockResolvedValue([]);
    groupsSlice.set({ selectedId: "g2" });
    await assignRole("g1", "0xa", "admin");
    await addMemberToGroup("g1", "0xb");
    expect(role).toHaveBeenCalledWith("g1", "0xa", "admin");
    expect(add).toHaveBeenCalledWith("g1", "0xb");
  });

  it("the roster shown is reloaded only when the acted-on group is still selected", async () => {
    vi.spyOn(bridge.groups, "offboard").mockResolvedValue(undefined);
    vi.spyOn(bridge.groups, "roster").mockResolvedValue([{ address: "0xg1member", role: "member" } as never]);
    groupsSlice.set({ selectedId: "g2", roster: [] });
    await offboardMember("g1", "0xa");
    expect(groupsSlice.get().roster).toEqual([]);
  });
});

describe("owner invite list loads", () => {
  it("drops an answer that arrives after the member switched groups", async () => {
    let selected = "g1";
    let release: (v: PendingInvite[]) => void = () => {};
    vi.spyOn(bridge.invites, "list").mockReturnValue(new Promise((r) => (release = r)));
    vi.spyOn(bridge.invites, "pollClaims").mockResolvedValue([{ group: "g1", token: "t", address: "0xa" } as never]);
    const invites = vi.fn();
    const requests = vi.fn();
    const load = loadOwnerInvites("g1", (id) => id === selected, { invites, requests });
    selected = "g2"; // switch while g1's list is pending
    release([pending("g1")]);
    await load;
    expect(invites).not.toHaveBeenCalled();
    expect(requests).not.toHaveBeenCalled();
  });

  it("applies the answer when the group is still selected", async () => {
    vi.spyOn(bridge.invites, "list").mockResolvedValue([pending("g1")]);
    vi.spyOn(bridge.invites, "pollClaims").mockResolvedValue([]);
    const invites = vi.fn();
    const requests = vi.fn();
    await loadOwnerInvites("g1", () => true, { invites, requests });
    expect(invites).toHaveBeenCalledWith([pending("g1")]);
    expect(requests).toHaveBeenCalledWith([]);
  });
});

// Tripwire: every member action in Groups names its group, and the slice actions never read the
// selected group to decide which group to act on.
describe("explicit group ids (tripwire)", () => {
  it("Groups calls offboard / role / add with a group id first", () => {
    const src = readFileSync(join(__dirname, "../../surfaces/Groups.tsx"), "utf8");
    const calls = [...src.matchAll(/\b(offboardMember|assignRole|addMemberToGroup|doOffboard|doRole)\(([^()]*)\)/g)].filter(
      (m) => !/^\s*(groupId: string|address: string)/.test(m[2]),
    );
    expect(calls.length).toBeGreaterThanOrEqual(8);
    for (const m of calls) expect(m[2].split(",").length, m[0]).toBeGreaterThanOrEqual(2);
    expect(src).toContain("onRemove={(joiner) => doOffboard(pi.group, joiner)}");
  });

  it("the slice actions take the group as their first parameter and act on it", () => {
    const src = readFileSync(join(__dirname, "groups.ts"), "utf8");
    for (const f of ["offboardMember", "assignRole", "addMemberToGroup"]) {
      const at = src.indexOf(`export async function ${f}(groupId: string`);
      expect(at, `${f}(groupId: string, …)`).toBeGreaterThan(-1);
      const body = src.slice(at);
      const fn = body.slice(0, body.indexOf("\n}\n"));
      expect(fn, f).not.toMatch(/const groupId = groupsSlice\.get\(\)\.selectedId/);
    }
  });
});
