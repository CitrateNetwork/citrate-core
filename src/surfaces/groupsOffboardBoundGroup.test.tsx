// #389 — Groups › Offboard is irreversible (it rotates the group secret), so it must act only on the
// group it was proposed for. The member actions take that group explicitly; they never read whichever
// group happens to be selected when the approval finally runs. If the selection moved between the
// proposal and the approval, the offboard is refused and nothing is sent.
import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Groups } from "./Groups";
import { bridge } from "../bridge";
import { freshState } from "../shell/state";
import { groupsSlice, offboardMember, addMemberToGroup } from "../shell/slices/groups";
import type { Store } from "../shell/store";
import type { CerSpec } from "../shell/state";
import type { Group, GroupMember } from "../bridge/domains";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const ME = "0x1111111111111111111111111111111111111111";
const THEM = "0x2222222222222222222222222222222222222222";
const ROSTER: GroupMember[] = [
  { address: ME, role: "owner" },
  { address: THEM, role: "member" },
];
const A: Group = { id: "g-389-a", name: "Alpha", owner: ME, kind: "channel", members: ROSTER };
const B: Group = { id: "g-389-b", name: "Bravo", owner: ME, kind: "channel", members: ROSTER };

const SRC = join(__dirname, "..", "..");
const read = (p: string) => readFileSync(join(SRC, p), "utf8");

let host: HTMLDivElement;
let root: Root;
let proposals: CerSpec[];
let toasts: string[];

beforeEach(() => {
  vi.restoreAllMocks();
  vi.spyOn(bridge.groups, "list").mockResolvedValue([A, B]);
  vi.spyOn(bridge.groups, "roster").mockResolvedValue(ROSTER);
  vi.spyOn(bridge.groups, "messages").mockResolvedValue([]);
  groupsSlice.set({ groups: [A, B], selectedId: A.id, roster: ROSTER, names: { [A.id]: "Alpha", [B.id]: "Bravo" }, busyMember: null, error: null });
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  proposals = [];
  toasts = [];
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  groupsSlice.set({ groups: [], selectedId: null, roster: [], names: {}, busyMember: null, error: null });
});

/** Render Groups on the People tab with group A selected; approvals are captured, not run. */
async function openPeopleTab(): Promise<void> {
  const store = {
    toast: (m: string) => toasts.push(m),
    identity: () => ({ wallet: "" }),
    refreshPeople: async () => {},
    requestSig: vi.fn(async (spec: CerSpec) => {
      proposals.push(spec);
      return "pending";
    }),
  } as unknown as Store;
  const s = { ...freshState("p1"), commsAddr: ME };
  await act(async () => root.render(<Groups store={store} s={s} />));
  const tab = [...host.querySelectorAll("button")].find((b) => b.textContent === "People");
  expect(tab, "the People tab").toBeDefined();
  await act(async () => tab!.click());
}
const offboardButton = () => [...host.querySelectorAll("button")].find((b) => b.textContent === "Offboard") as HTMLButtonElement | undefined;

/** Propose an offboard of THEM while group A is selected; returns the captured approval. */
async function proposeOffboardOnA(): Promise<CerSpec> {
  await openPeopleTab();
  const b = offboardButton();
  expect(b, "Offboard is drawn for a manager").toBeDefined();
  await act(async () => b!.click());
  expect(proposals).toHaveLength(1);
  return proposals[0];
}

describe("Offboard acts only on the group it was proposed for (#389)", () => {
  it("race: propose on A, switch to B, approve: never offboards from B, and refuses", async () => {
    const off = vi.spyOn(bridge.groups, "offboard").mockResolvedValue(undefined);
    const spec = await proposeOffboardOnA();
    await act(async () => groupsSlice.set({ selectedId: B.id })); // the member switched groups before approving
    await act(async () => spec.apply?.());
    expect(off).not.toHaveBeenCalledWith(B.id, expect.anything());
    expect(off, "a changed selection refuses: nothing is sent").not.toHaveBeenCalled();
    expect(toasts.join("\n")).toMatch(/selected group changed/i);
  });

  it("approved with A still selected, it offboards from A", async () => {
    const off = vi.spyOn(bridge.groups, "offboard").mockResolvedValue(undefined);
    const spec = await proposeOffboardOnA();
    await act(async () => spec.apply?.());
    expect(off).toHaveBeenCalledTimes(1);
    expect(off).toHaveBeenCalledWith(A.id, THEM);
  });

  it("the approval names the group it acts on", async () => {
    const spec = await proposeOffboardOnA();
    expect(spec.rows).toContainEqual({ k: "Group", v: "Alpha" });
  });

  it("the slice offboards the group it is given, not the selected group", async () => {
    const off = vi.spyOn(bridge.groups, "offboard").mockResolvedValue(undefined);
    groupsSlice.set({ selectedId: B.id });
    await offboardMember(A.id, THEM);
    expect(off).toHaveBeenCalledWith(A.id, THEM);
  });

  it("the slice adds to the group it is given, not the selected group", async () => {
    const add = vi.spyOn(bridge.groups, "addMember").mockResolvedValue(undefined);
    groupsSlice.set({ selectedId: B.id });
    await addMemberToGroup(A.id, THEM);
    expect(add).toHaveBeenCalledWith(A.id, THEM);
  });

  it("after acting on A, B's shown roster is not replaced with A's", async () => {
    vi.spyOn(bridge.groups, "offboard").mockResolvedValue(undefined);
    const roster = vi.spyOn(bridge.groups, "roster").mockResolvedValue([{ address: ME, role: "owner" }]);
    groupsSlice.set({ selectedId: B.id, roster: [] });
    await offboardMember(A.id, THEM);
    expect(roster).not.toHaveBeenCalledWith(B.id);
    expect(groupsSlice.get().roster).toEqual([]);
  });

  it("a failed offboard reports the error and clears the busy row", async () => {
    vi.spyOn(bridge.groups, "offboard").mockRejectedValue(new Error("relay unavailable"));
    await offboardMember(A.id, THEM);
    expect(groupsSlice.get().error).toContain("relay unavailable");
    expect(groupsSlice.get().busyMember).toBeNull();
  });
});

// Tripwire: the member actions never decide their group from the selection.
describe("tripwire: member actions take an explicit group", () => {
  it("offboardMember, assignRole and addMemberToGroup take the group first and never read selectedId", () => {
    const src = read("src/shell/slices/groups.ts");
    for (const f of ["offboardMember", "assignRole", "addMemberToGroup"]) {
      const at = src.indexOf(`export async function ${f}(groupId: string`);
      expect(at, `${f}(groupId: string, …)`).toBeGreaterThan(-1);
      const rest = src.slice(at);
      const body = rest.slice(0, rest.indexOf("\n}\n"));
      expect(body, f).not.toMatch(/selectedId\s*;/);
      expect(body, f).not.toMatch(/=\s*groupsSlice\.get\(\)\.selectedId/);
    }
  });

  it("every Groups call of a member action passes a group id first", () => {
    const src = read("src/surfaces/Groups.tsx");
    const calls = [...src.matchAll(/\b(offboardMember|assignRole|addMemberToGroup|doOffboard|doRole)\(([^()]*)\)/g)].filter(
      (m) => !/^\s*groupId: string/.test(m[2]),
    );
    // 7 since D-12: the two claim-approval adds moved into Rust (group_invite_approve takes the group).
    expect(calls.length).toBeGreaterThanOrEqual(7);
    for (const m of calls) expect(m[2].split(",").length, m[0]).toBeGreaterThanOrEqual(2);
  });
});
