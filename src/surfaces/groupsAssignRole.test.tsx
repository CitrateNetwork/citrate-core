// #495 / GC-2 — Groups › Assign role. A role change is a `grantRole` request: the 0.6.0
// comms-member-daemon signs the grant with this device's comms key and sends it to the group,
// where every member checks it against the group owner and its roster. Only the owner makes admins
// (Admin gates money-adjacent surfaces), so the role control is the owner's. Red-first.
import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Groups } from "./Groups";
import { ROLE_CHANGES_NEED_NEWER_DAEMON, canChangeRoles } from "./groupRoles";
import { bridge } from "../bridge";
import { freshState } from "../shell/state";
import { groupsSlice, assignRole } from "../shell/slices/groups";
import type { Store } from "../shell/store";
import type { Group, GroupMember } from "../bridge/domains";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const ME = "0x1111111111111111111111111111111111111111";
const THEM = "0x2222222222222222222222222222222222222222";
const ROSTER: GroupMember[] = [
  { address: ME, role: "owner" },
  { address: THEM, role: "member" },
];
const GROUP: Group = { id: "g-495", name: "Team", owner: ME, kind: "channel", members: ROSTER };

const SRC = join(__dirname, "..", "..");
const read = (p: string) => readFileSync(join(SRC, p), "utf8");

/** The body of `pub async fn groups_assign_role` in comms.rs, up to the closing brace. */
function rustAssignRoleBody(): string {
  const rs = read("src-tauri/src/comms.rs");
  const start = rs.indexOf("pub async fn groups_assign_role");
  expect(start, "groups_assign_role is still a command in comms.rs").toBeGreaterThan(-1);
  const end = rs.indexOf("\n}\n", start);
  return rs.slice(start, end);
}

let host: HTMLDivElement;
let root: Root;
let requestSig: ReturnType<typeof vi.fn>;

beforeEach(() => {
  vi.restoreAllMocks();
  vi.spyOn(bridge.groups, "list").mockResolvedValue([GROUP]);
  vi.spyOn(bridge.groups, "roster").mockResolvedValue(ROSTER);
  vi.spyOn(bridge.groups, "messages").mockResolvedValue([]);
  groupsSlice.set({ groups: [GROUP], selectedId: GROUP.id, roster: ROSTER, names: { [GROUP.id]: "Team" }, busyMember: null, error: null });
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  requestSig = vi.fn(async () => "approved");
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  groupsSlice.set({ groups: [], selectedId: null, roster: [], names: {}, busyMember: null, error: null });
});

async function openPeopleTab(): Promise<void> {
  const store = {
    toast: () => {},
    identity: () => ({ wallet: "" }),
    refreshPeople: async () => {},
    requestSig,
  } as unknown as Store;
  const s = { ...freshState("p1"), commsAddr: ME };
  await act(async () => root.render(<Groups store={store} s={s} />));
  const tab = [...host.querySelectorAll("button")].find((b) => b.textContent === "People");
  expect(tab, "the People tab").toBeDefined();
  await act(async () => tab!.click());
}
const roleButton = () =>
  [...host.querySelectorAll("button")].find((b) => /Make admin|Make member/.test(b.textContent ?? "")) as HTMLButtonElement | undefined;

describe("Assign role sends a grant the daemon signs with the comms key (#495, GC-2)", () => {
  it("the owner sees Make admin, and confirming sends the role change", async () => {
    const send = vi.spyOn(bridge.groups, "assignRole").mockResolvedValue();
    await openPeopleTab();
    const b = roleButton();
    expect(b, "the owner gets the role control").toBeDefined();
    expect(b!.disabled).toBe(false);
    await act(async () => b!.click());
    expect(requestSig).toHaveBeenCalledTimes(1);
    const sheet = requestSig.mock.calls[0][0];
    expect(JSON.stringify(sheet)).toContain("comms key");
    expect(sheet.origin).not.toMatch(/wallet/i);
    await act(async () => sheet.apply());
    expect(send).toHaveBeenCalledWith(GROUP.id, THEM, "admin");
  });

  it("only the owner changes roles", () => {
    expect(canChangeRoles("owner")).toBe(true);
    for (const r of ["admin", "member", "guest", "agent"] as const) expect(canChangeRoles(r)).toBe(false);
  });

  it("an older daemon's refusal is shown in plain words", async () => {
    vi.spyOn(bridge.groups, "assignRole").mockRejectedValue(new Error(ROLE_CHANGES_NEED_NEWER_DAEMON));
    await assignRole(GROUP.id, THEM, "admin");
    expect(groupsSlice.get().error).toBe(ROLE_CHANGES_NEED_NEWER_DAEMON);
    expect(groupsSlice.get().busyMember).toBeNull();
  });

  it("the sim bridge makes no simulated role change", async () => {
    if (bridge.mode !== "sim") return;
    await expect(bridge.groups.assignRole(GROUP.id, THEM, "admin")).rejects.toThrow(/desktop app/);
  });

  it("the UI reason for an older daemon is the Rust reason, word for word", () => {
    const m = read("src-tauri/src/comms.rs").match(/ROLE_GRANTS_NEED_NEWER_DAEMON: &str =\s*"([^"]+)"/);
    expect(m?.[1]).toBe(ROLE_CHANGES_NEED_NEWER_DAEMON);
  });

  it("groups_assign_role routes the request instead of refusing", () => {
    expect(rustAssignRoleBody()).toMatch(/\broute\(/);
  });

  it("no copy claims the grant is enforced at the relay", () => {
    for (const p of ["src/surfaces/Groups.tsx", "src/shell/slices/groups.ts", "src/bridge/domains.ts"]) {
      expect(read(p), p).not.toMatch(/enforced at the relay/i);
    }
  });
});
