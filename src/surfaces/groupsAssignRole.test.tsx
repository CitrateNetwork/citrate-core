// #495 — Groups › Assign role. The pinned comms-member-daemon accepts `assignRole` only with a signed
// RoleAssertion (`{group, assertion}`), and nothing in core or the daemon signs one yet, so every
// press failed with "bad request: missing field `assertion`" while the confirm sheet claimed "a signed
// RoleAssertion, enforced at the relay". Until a signer exists the action is shown as not available,
// with a plain reason, and no request is sent. Written red-first against 90dce48a.
import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Groups } from "./Groups";
import { ROLE_CHANGES_AVAILABLE, ROLE_CHANGES_NOT_AVAILABLE } from "./groupRoles";
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

describe("Assign role is not available until role grants are signed (#495)", () => {
  it("shows Make admin as not available, with the plain reason", async () => {
    await openPeopleTab();
    const b = roleButton();
    expect(b, "the role control is still drawn for a manager").toBeDefined();
    expect(b!.disabled).toBe(true);
    expect(b!.getAttribute("aria-disabled")).toBe("true");
    expect(host.textContent).toContain(ROLE_CHANGES_NOT_AVAILABLE);
  });

  it("pressing it opens no confirm sheet and sends no request", async () => {
    const send = vi.spyOn(bridge.groups, "assignRole");
    await openPeopleTab();
    await act(async () => roleButton()!.click());
    expect(requestSig).not.toHaveBeenCalled();
    expect(send).not.toHaveBeenCalled();
  });

  it("the slice refuses with the reason and never calls the bridge", async () => {
    const send = vi.spyOn(bridge.groups, "assignRole");
    await assignRole(THEM, "admin");
    expect(send).not.toHaveBeenCalled();
    expect(groupsSlice.get().error).toBe(ROLE_CHANGES_NOT_AVAILABLE);
    expect(groupsSlice.get().busyMember).toBeNull();
  });

  it("the sim bridge refuses the same way the app does (no simulated role change)", async () => {
    if (bridge.mode !== "sim") return;
    await expect(bridge.groups.assignRole(GROUP.id, THEM, "admin")).rejects.toThrow(ROLE_CHANGES_NOT_AVAILABLE);
  });

  it("drops the claim that a signed RoleAssertion is enforced at the relay", () => {
    for (const p of ["src/surfaces/Groups.tsx", "src/shell/slices/groups.ts", "src/bridge/domains.ts"]) {
      expect(read(p), p).not.toMatch(/signed\s+RoleAssertion/i);
    }
  });
});

describe("tripwire: Assign role stays not available while the app has no role-grant signer", () => {
  it("the UI flag matches groups_assign_role: refusing in Rust means not available in the UI", () => {
    const body = rustAssignRoleBody();
    const rustRefuses = body.includes("role_grants_available()") && !/\broute\(/.test(body);
    // Re-enable Assign role only together with a groups_assign_role that sends a signed assertion.
    expect(ROLE_CHANGES_AVAILABLE).toBe(!rustRefuses);
  });

  it("the UI reason is the Rust reason, word for word", () => {
    const m = read("src-tauri/src/comms.rs").match(/ROLE_GRANTS_NOT_AVAILABLE: &str =\s*"([^"]+)"/);
    expect(m?.[1]).toBe(ROLE_CHANGES_NOT_AVAILABLE);
  });

  it("no surface calls assignRole without going through the availability gate", () => {
    const groups = read("src/surfaces/Groups.tsx");
    // The role control is only wired to the confirm sheet behind ROLE_CHANGES_AVAILABLE.
    expect(groups).toContain("ROLE_CHANGES_AVAILABLE");
    const slice = read("src/shell/slices/groups.ts");
    const fn = slice.slice(slice.indexOf("export async function assignRole"));
    const gate = fn.indexOf("ROLE_CHANGES_AVAILABLE");
    const call = fn.indexOf("bridge.groups.assignRole");
    expect(gate, "assignRole checks the gate").toBeGreaterThan(-1);
    expect(gate).toBeLessThan(call);
  });
});
