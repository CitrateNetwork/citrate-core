// 0.5.4 FL design, board 03 (Groups › Members). One test per StateMatrix "Members" state:
// Empty (only you), Pending (grant signed, applying), Success (roster updated), Failure (grant
// refused), Unavailable (roster not read yet); plus the role rule the daemon enforces, the HIC-1
// review sheet, offboarding, invites by role, and devices / presence that are "—" with a reason.
import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { GroupMembers } from "./GroupMembers";
import { roleActions, roleWord, myRoleIn, presenceOf, ceremonyTitle } from "./memberRoles";
import { bridge } from "../../bridge";
import { freshState, type AppState } from "../../shell/state";
import { groupsSlice } from "../../shell/slices/groups";
import type { Store } from "../../shell/store";
import type { Group, GroupMember } from "../../bridge/domains";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const ME = "0x1111111111111111111111111111111111111111";
const ANA = "0x2222222222222222222222222222222222222222";
const RUI = "0x3333333333333333333333333333333333333333";
const OWNER_ROSTER: GroupMember[] = [
  { address: ME, role: "owner" },
  { address: ANA, role: "member" },
  { address: RUI, role: "admin" },
];
const GROUP: Group = { id: "g-members", name: "Orchard Lab", owner: ME, kind: "forum", members: OWNER_ROSTER };

let host: HTMLDivElement;
let root: Root;
let requestSig: ReturnType<typeof vi.fn>;
let toast: ReturnType<typeof vi.fn>;

function store(): Store {
  return {
    toast,
    go: () => {},
    identity: () => ({ wallet: "", name: "Me" }),
    refreshPeople: async () => {},
    requestSig,
  } as unknown as Store;
}
const state = (over: Partial<AppState> = {}): AppState => ({ ...freshState("p1"), commsAddr: ME, ...over });

beforeEach(() => {
  vi.restoreAllMocks();
  vi.spyOn(bridge.groups, "list").mockResolvedValue([GROUP]);
  vi.spyOn(bridge.groups, "roster").mockResolvedValue(OWNER_ROSTER);
  vi.spyOn(bridge.groups, "messages").mockResolvedValue([]);
  vi.spyOn(bridge.social, "resolve").mockResolvedValue([]);
  vi.spyOn(bridge.cluster, "devices").mockRejectedValue(new Error("cluster daemon not running"));
  vi.spyOn(bridge.invites, "list").mockResolvedValue([]);
  groupsSlice.set({ groups: [GROUP], selectedId: GROUP.id, roster: OWNER_ROSTER, names: {}, busyMember: null, error: null });
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  requestSig = vi.fn(async () => "approved");
  toast = vi.fn();
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  groupsSlice.set({ groups: [], selectedId: null, roster: [], names: {}, busyMember: null, error: null });
});

async function render(s: AppState = state()): Promise<void> {
  await act(async () => root.render(<GroupMembers store={store()} s={s} />));
}
const board = () => host.querySelector("[data-board=members]") as HTMLElement;
const row = (addr: string) => host.querySelector(`tr[data-member="${addr}"]`) as HTMLTableRowElement;
const button = (scope: ParentNode, label: string) =>
  [...scope.querySelectorAll("button")].find((b) => b.textContent === label) as HTMLButtonElement | undefined;

describe("who may change whose role (the comms daemon's rule)", () => {
  it("the owner can make admins, members and read-only, and offboard", () => {
    expect(roleActions("owner", "member", false).actions.map((a) => a.label)).toEqual(["Make admin", "Read-only", "Offboard"]);
    expect(roleActions("owner", "admin", false).actions.map((a) => a.label)).toEqual(["Make member", "Read-only", "Offboard"]);
    expect(roleActions("owner", "guest", false).actions.map((a) => a.label)).toEqual(["Make member", "Make admin", "Offboard"]);
  });
  it("an admin moves members and read-only members, never an admin", () => {
    expect(roleActions("admin", "member", false).actions.map((a) => a.label)).toEqual(["Read-only", "Offboard"]);
    expect(roleActions("admin", "guest", false).actions.map((a) => a.label)).toEqual(["Make member", "Offboard"]);
    expect(roleActions("admin", "admin", false)).toEqual({ actions: [], none: "only the owner changes an admin" });
    expect(roleActions("admin", "member", false).actions.some((a) => a.to === "admin")).toBe(false);
  });
  it("members, read-only members and an unknown role change nothing", () => {
    for (const r of ["member", "guest", "agent", null] as const) expect(roleActions(r, "member", false).actions).toEqual([]);
  });
  it("nobody changes the owner, an agent seat or themselves here", () => {
    expect(roleActions("owner", "owner", false).none).toBe("owner");
    expect(roleActions("owner", "agent", false).actions).toEqual([]);
    expect(roleActions("owner", "member", true).none).toBe("you");
  });
  it("the daemon's guest reads as read-only", () => {
    expect(roleWord("guest")).toBe("read-only");
    expect(roleWord("admin")).toBe("admin");
  });
  it("your role is unknown until a source knows it, never guessed", () => {
    expect(myRoleIn(null, [], "", ME)).toBeNull();
    expect(myRoleIn(null, OWNER_ROSTER, "", ME)).toBe("owner");
    expect(myRoleIn("admin", OWNER_ROSTER, "", ME)).toBe("admin");
  });
  it("ceremony titles name the person and the role", () => {
    expect(ceremonyTitle({ label: "Make admin", to: "admin", danger: false }, "Ana")).toBe("Make Ana an admin");
    expect(ceremonyTitle({ label: "Offboard", to: "removed", danger: true }, "Ana")).toBe("Offboard Ana");
  });
});

describe("devices and presence come from the cluster daemon", () => {
  it("an unread cluster is — with the reason, never zero devices", () => {
    expect(presenceOf(ANA, null, "The group's cluster did not answer")).toEqual({ devices: "—", seen: "—", why: "The group's cluster did not answer" });
  });
  it("a reported member shows their device count and live presence", () => {
    const d = [{ member: ANA.slice(2), online: false, devices: [{ online: true }, { online: false }] }];
    expect(presenceOf(ANA, d, "")).toEqual({ devices: "2 devices", seen: "online", why: null });
  });
  it("a member the cluster did not report is — with that reason", () => {
    expect(presenceOf(RUI, [], "").why).toBe("not reported by the group's cluster");
  });
});

describe("Groups › Members states (StateMatrix)", () => {
  it("Unavailable: the roster has not been read yet", async () => {
    vi.spyOn(bridge.groups, "roster").mockReturnValue(new Promise(() => {}));
    groupsSlice.set({ roster: [] });
    await render();
    expect(board().dataset.state).toBe("roster-unread");
    expect(host.textContent).toContain("Roster not read yet");
    expect(host.querySelector("table")).toBeNull();
  });

  it("Empty: only you", async () => {
    const solo: GroupMember[] = [{ address: ME, role: "owner" }];
    vi.spyOn(bridge.groups, "roster").mockResolvedValue(solo);
    groupsSlice.set({ roster: solo });
    await render();
    expect(board().dataset.state).toBe("only-you");
    expect(host.textContent).toContain("Only you so far");
    expect(row(ME).textContent).toContain("you");
  });

  it("no groups at all points to Groups › Chat", async () => {
    vi.spyOn(bridge.groups, "list").mockResolvedValue([]);
    groupsSlice.set({ groups: [], selectedId: null, roster: [] });
    await render();
    expect(board().dataset.state).toBe("no-groups");
    expect(button(host, "Go to Groups › Chat")).toBeDefined();
  });

  it("the roster table shows roles, and devices as — with the reason when the cluster is unread", async () => {
    await render();
    expect(board().dataset.state).toBe("roster");
    expect(row(ANA).textContent).toContain("member");
    expect(row(RUI).textContent).toContain("admin");
    expect(row(ANA).querySelectorAll("td")[1].textContent).toContain("—");
    expect(row(ANA).querySelectorAll("td")[1].getAttribute("title")).toMatch(/cluster did not answer: cluster daemon not running/);
  });

  it("Pending: a signed grant shows as applying until the roster shows it; Success: the roster updated", async () => {
    let resolveAssign: () => void = () => {};
    vi.spyOn(bridge.groups, "assignRole").mockReturnValue(new Promise<void>((r) => (resolveAssign = r)));
    await render();
    await act(async () => button(row(ANA), "Make admin")!.click());
    expect(requestSig).toHaveBeenCalledTimes(1);
    const sheet = requestSig.mock.calls[0][0];
    expect(sheet.title).toBe(`Make ${ANA.slice(0, 6)}…${ANA.slice(-4)} an admin`);
    expect(JSON.stringify(sheet.rows)).toContain("comms key (not your wallet)");
    expect(sheet.sponsor).toContain("HIC-1");
    expect(sheet.chainless).toBe(true);
    await act(async () => sheet.apply());
    expect(row(ANA).textContent).toContain("applying: admin");
    const updated: GroupMember[] = OWNER_ROSTER.map((m) => (m.address === ANA ? { ...m, role: "admin" } : m));
    vi.spyOn(bridge.groups, "roster").mockResolvedValue(updated);
    await act(async () => resolveAssign());
    expect(row(ANA).textContent).not.toContain("applying");
    expect(row(ANA).querySelectorAll("td")[3].textContent).toBe("admin");
    expect(toast).toHaveBeenCalledWith(expect.stringContaining("Signed and sent to the group"));
  });

  it("Failure: the daemon refuses the grant and says why; the row stops applying", async () => {
    vi.spyOn(bridge.groups, "assignRole").mockRejectedValue(new Error("only the owner demotes an Admin"));
    await render();
    await act(async () => button(row(RUI), "Make member")!.click());
    await act(async () => requestSig.mock.calls[0][0].apply());
    expect(host.querySelector("[role=alert]")?.textContent).toBe("only the owner demotes an Admin");
    expect(row(RUI).textContent).not.toContain("applying");
  });

  it("offboarding warns that it cannot be undone and removes the member", async () => {
    const off = vi.spyOn(bridge.groups, "offboard").mockResolvedValue();
    await render();
    await act(async () => button(row(ANA), "Offboard")!.click());
    const sheet = requestSig.mock.calls[0][0];
    expect(sheet.title).toMatch(/^Offboard /);
    expect(sheet.warning).toContain("cannot be undone");
    expect(sheet.warning).toContain("rotates the group secret");
    await act(async () => sheet.apply());
    expect(off).toHaveBeenCalledWith(GROUP.id, ANA);
  });

  it("declining the review sends nothing", async () => {
    const send = vi.spyOn(bridge.groups, "assignRole").mockResolvedValue();
    requestSig = vi.fn(async () => "rejected");
    await render();
    await act(async () => button(row(ANA), "Make admin")!.click());
    expect(send).not.toHaveBeenCalled();
  });

  it("a member sees no role actions and is told who can invite", async () => {
    const asMember: GroupMember[] = [
      { address: RUI, role: "owner" },
      { address: ME, role: "member" },
      { address: ANA, role: "member" },
    ];
    vi.spyOn(bridge.groups, "roster").mockResolvedValue(asMember);
    groupsSlice.set({ groups: [{ ...GROUP, owner: RUI }], roster: asMember });
    await render();
    expect(row(ANA).textContent).toContain("owners and admins assign roles");
    expect(row(ANA).querySelectorAll("button").length).toBe(0);
    expect(host.textContent).toContain("Only owners and admins can invite.");
    expect(host.querySelector("[data-testid=invite-panel]")).toBeNull();
  });

  it("an owner gets the single-use invite panel", async () => {
    await render();
    expect(host.querySelector("[data-testid=invite-panel]")).not.toBeNull();
    expect(host.textContent).toContain("Single use.");
    expect(host.textContent).toContain("They join as a member");
  });

  it("devices and presence render from the cluster daemon when it answers", async () => {
    vi.spyOn(bridge.cluster, "devices").mockResolvedValue([
      { member: ANA, role: "member", online: true, devices: [{ device: "d1", index: 0, label: "Laptop", issuedAt: 1, online: true }] },
    ]);
    await render();
    expect(row(ANA).querySelectorAll("td")[1].textContent).toBe("1 device");
    expect(row(ANA).querySelectorAll("td")[2].textContent).toBe("online");
  });

  it("copy follows the rules: no exclamation marks, members not users, HIC words", async () => {
    await render();
    const text = host.textContent ?? "";
    expect(text).not.toMatch(/!/);
    expect(text).not.toMatch(/\busers?\b/i);
    expect(text).not.toMatch(/HITL|—\s[a-z]/);
  });
});
