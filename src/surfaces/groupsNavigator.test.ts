// CONNECT-S4 — role navigator derivation tests. The navigator badges YOUR role per group from live
// roster data (keyed on your comms address), filters "where I'm admin", and never fabricates a role.
import { describe, it, expect } from "vitest";
import { buildRoleNavigator, managedGroups, navigatorSummary } from "./groupsNavigator";
import type { GroupRole } from "../bridge/domains";

const SELF = "0xme";
const groups = [
  { id: "g1", name: "Design", kind: "channel" as const },
  { id: "g2", name: "Ops", kind: "forum" as const },
  { id: "g3", name: "Alumni", kind: "channel" as const },
];
const roster = (rows: [string, GroupRole][]) => rows.map(([address, role]) => ({ address, role }));

describe("buildRoleNavigator — CONNECT-S4", () => {
  it("badges my role per group from the roster, keyed on my comms address", () => {
    const rows = buildRoleNavigator(
      groups,
      {
        g1: roster([[SELF, "owner"], ["0xa", "member"]]),
        g2: roster([["0xb", "owner"], [SELF, "admin"]]),
        g3: roster([["0xc", "owner"], [SELF, "member"]]),
      },
      SELF,
    );
    expect(rows.find((r) => r.id === "g1")?.myRole).toBe("owner");
    expect(rows.find((r) => r.id === "g2")?.myRole).toBe("admin");
    expect(rows.find((r) => r.id === "g3")?.myRole).toBe("member");
  });

  it("is case-insensitive on the address match", () => {
    const rows = buildRoleNavigator(groups.slice(0, 1), { g1: roster([["0xME", "admin"]]) }, "0xme");
    expect(rows[0].myRole).toBe("admin");
  });

  it("gives a null role (never a guess) when the roster hasn't loaded", () => {
    const rows = buildRoleNavigator(groups.slice(0, 1), {}, SELF);
    expect(rows[0].myRole).toBeNull();
    expect(rows[0].iManage).toBe(false);
  });

  it("iManage is true for owner/admin, false for member", () => {
    const rows = buildRoleNavigator(
      groups,
      {
        g1: roster([[SELF, "owner"]]),
        g2: roster([[SELF, "admin"]]),
        g3: roster([[SELF, "member"]]),
      },
      SELF,
    );
    expect(rows.find((r) => r.id === "g1")?.iManage).toBe(true);
    expect(rows.find((r) => r.id === "g2")?.iManage).toBe(true);
    expect(rows.find((r) => r.id === "g3")?.iManage).toBe(false);
  });

  it("treats a group created THIS session as manage-capable even before its roster keys my seat", () => {
    const rows = buildRoleNavigator(groups.slice(0, 1), {}, SELF, {}, ["g1"]);
    expect(rows[0].iManage).toBe(true);
    expect(rows[0].myRole).toBeNull(); // still honest: no roster role yet
  });

  it("prefers the session name over the DTO name", () => {
    const rows = buildRoleNavigator([{ id: "g1", name: "grp_raw", kind: "channel" }], { g1: roster([[SELF, "owner"]]) }, SELF, { g1: "My Studio" });
    expect(rows[0].name).toBe("My Studio");
  });

  it("orders managed groups first, then by role rank, then name", () => {
    const rows = buildRoleNavigator(
      groups,
      {
        g1: roster([[SELF, "member"]]), // Design, member
        g2: roster([[SELF, "admin"]]), // Ops, admin
        g3: roster([[SELF, "owner"]]), // Alumni, owner
      },
      SELF,
    );
    // owner (Alumni) then admin (Ops) then member (Design)
    expect(rows.map((r) => r.id)).toEqual(["g3", "g2", "g1"]);
  });
});

describe("managedGroups / navigatorSummary — CONNECT-S4", () => {
  const rows = buildRoleNavigator(
    groups,
    { g1: roster([[SELF, "owner"]]), g2: roster([[SELF, "admin"]]), g3: roster([[SELF, "member"]]) },
    SELF,
  );
  it("managedGroups keeps only the ones I run", () => {
    expect(managedGroups(rows).map((r) => r.id).sort()).toEqual(["g1", "g2"]);
  });
  it("navigatorSummary counts managed vs total", () => {
    expect(navigatorSummary(rows)).toEqual({ managed: 2, total: 3 });
  });
});
