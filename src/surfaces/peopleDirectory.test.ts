// CONNECT-S0 acceptance — the People directory aggregation is real (derived from group rosters + faces),
// excludes self, dedups across shared groups, and never fabricates a name or a person (Rule 1).
import { describe, it, expect } from "vitest";
import { buildPeopleDirectory, filterPeople } from "./peopleDirectory";

const SELF = "0x00000000000000000000000000000000000000me";
const groups = [
  { id: "g-design", name: "Design" },
  { id: "g-ops", name: "Ops" },
];
const rosterByGroup = {
  "g-design": [
    { address: SELF, role: "owner" },
    { address: "0xAA", role: "admin" },
    { address: "0xBB", role: "member" },
  ],
  "g-ops": [
    { address: SELF, role: "member" },
    { address: "0xAA", role: "member" }, // 0xAA shared across BOTH groups
  ],
};
const faces = [{ address: "0xaa", network: "x", handle: "dana" }]; // 0xAA has a verified X face

describe("CONNECT-S0 — People directory aggregation", () => {
  it("AC1: unions distinct roster members across groups and excludes self", () => {
    const people = buildPeopleDirectory(groups, rosterByGroup, faces, SELF);
    const addrs = people.map((p) => p.address.toLowerCase());
    expect(addrs).toContain("0xaa");
    expect(addrs).toContain("0xbb");
    expect(addrs).not.toContain(SELF.toLowerCase()); // never in your own directory
    expect(people).toHaveLength(2);
  });

  it("AC2: a resolved address carries its verified face; an unresolved one has none (no fabrication)", () => {
    const people = buildPeopleDirectory(groups, rosterByGroup, faces, SELF);
    const aa = people.find((p) => p.address.toLowerCase() === "0xaa")!;
    const bb = people.find((p) => p.address.toLowerCase() === "0xbb")!;
    expect(aa.face).toEqual({ network: "x", handle: "dana" });
    expect(bb.face).toBeNull(); // renders as a short address, never an invented name
  });

  it("AC3: a person shared across two groups appears once, listing both with their role", () => {
    const people = buildPeopleDirectory(groups, rosterByGroup, faces, SELF);
    const aa = people.find((p) => p.address.toLowerCase() === "0xaa")!;
    expect(aa.groups).toHaveLength(2);
    expect(aa.groups.find((g) => g.id === "g-design")!.role).toBe("admin");
    expect(aa.groups.find((g) => g.id === "g-ops")!.role).toBe("member");
    // faced + most-shared person sorts first
    expect(people[0].address.toLowerCase()).toBe("0xaa");
  });

  it("AC4: no groups (or no shared members) yields an empty directory, not fabricated rows", () => {
    expect(buildPeopleDirectory([], {}, [], SELF)).toEqual([]);
    expect(buildPeopleDirectory(groups, { "g-design": [{ address: SELF, role: "owner" }] }, [], SELF)).toEqual([]);
  });

  it("AC5: filter matches by handle, address, or shared group name", () => {
    const people = buildPeopleDirectory(groups, rosterByGroup, faces, SELF);
    expect(filterPeople(people, "dana").map((p) => p.address.toLowerCase())).toEqual(["0xaa"]);
    expect(filterPeople(people, "0xbb").map((p) => p.address.toLowerCase())).toEqual(["0xbb"]);
    expect(filterPeople(people, "ops").map((p) => p.address.toLowerCase())).toEqual(["0xaa"]); // only 0xAA is in Ops
    expect(filterPeople(people, "")).toHaveLength(2); // empty query = unchanged
  });
});
