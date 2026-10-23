// GROW-S2 (Join 9, M5) review: a join that completes after the group list was loaded refreshes the
// group list, so the row turns into "Joined ⟨cluster⟩" and the group appears where the member is.
import { describe, it, expect, vi, afterEach } from "vitest";
import { readFileSync } from "node:fs";
import { join as joinPath } from "node:path";
import { bridge } from "../../bridge";
import type { Group, JoinViewDto } from "../../bridge/domains";
import { groupsSlice, refreshGroups } from "./groups";
import { cancelJoin, forgetJoin, ingest, rendezvousSlice } from "./rendezvous";
import { renderToStaticMarkup } from "react-dom/server";
import { JoinRequests } from "../../surfaces/JoinRequests";
import { OnboardingJoins } from "../../onboarding/Onboarding";

const join = (state: JoinViewDto["state"], group = "g1"): JoinViewDto => ({
  group,
  name: "Aperture",
  state,
  reason: null,
  expiresAt: null,
  graceUntil: null,
  updatedAt: 1,
});
const group = (id: string): Group => ({ id, name: "Aperture", owner: "0xo", kind: "channel", members: [] });
const flush = () => new Promise((r) => setTimeout(r, 0));

afterEach(() => {
  vi.restoreAllMocks();
  groupsSlice.set({ groups: [], error: null });
  rendezvousSlice.set({ joins: [], invites: [], lastKnownJoin: {}, lastKnownInvite: {}, receivedAt: 0, busy: null, error: null });
});

describe("join completion refreshes the group list", () => {
  it("waiting → joined after the initial group-list load reloads the list", async () => {
    const list = vi.spyOn(bridge.groups, "list").mockResolvedValueOnce([]).mockResolvedValue([group("g1")]);
    await refreshGroups(); // Groups opened while the request waits
    ingest({ joins: [join("waiting")], invites: [] });
    await flush();
    expect(list).toHaveBeenCalledTimes(1);
    ingest({ joins: [join("joined")], invites: [] }); // the background tick's join://state
    await flush();
    expect(list).toHaveBeenCalledTimes(2);
    expect(groupsSlice.get().groups.map((g) => g.id)).toEqual(["g1"]);
  });

  it("a taken join (already a member) refreshes too", async () => {
    const list = vi.spyOn(bridge.groups, "list").mockResolvedValue([group("g1")]);
    ingest({ joins: [join("waiting")], invites: [] });
    ingest({ joins: [join("taken")], invites: [] });
    await flush();
    expect(list).toHaveBeenCalledTimes(1);
  });

  it("a tick that changes nothing for a listed join doesn't reload", async () => {
    const list = vi.spyOn(bridge.groups, "list").mockResolvedValue([group("g1")]);
    ingest({ joins: [join("waiting")], invites: [] });
    ingest({ joins: [join("waiting")], invites: [] });
    await flush();
    expect(list).not.toHaveBeenCalled();
    ingest({ joins: [join("joined")], invites: [] });
    await flush();
    ingest({ joins: [join("joined")], invites: [] });
    await flush();
    expect(list).toHaveBeenCalledTimes(1);
  });

  it("a joined group the list still lacks is retried on the next tick (never an optimistic Joined)", async () => {
    const list = vi.spyOn(bridge.groups, "list").mockResolvedValueOnce([]).mockResolvedValue([group("g1")]);
    ingest({ joins: [join("joined")], invites: [] });
    await flush();
    expect(groupsSlice.get().groups).toEqual([]);
    ingest({ joins: [join("joined")], invites: [] });
    await flush();
    expect(list).toHaveBeenCalledTimes(2);
    expect(groupsSlice.get().groups.map((g) => g.id)).toEqual(["g1"]);
  });



});

// Codex round 3 (P2): a cancel or dismiss that fails is said under its row, and the last confirmed
// state stays. A failed cancel is never shown as withdrawn: the request may still be accepted.
describe("failed cancel and dismiss", () => {
  it("a rejected cancel keeps the last confirmed state and says the cancel wasn't confirmed", async () => {
    vi.spyOn(bridge.groups, "list").mockResolvedValue([]);
    ingest({ joins: [join("waiting")], invites: [] });
    vi.spyOn(bridge.rendezvous, "cancel").mockRejectedValue(new Error("comms daemon did not respond within 15000 ms"));
    await cancelJoin("g1");
    const s = rendezvousSlice.get();
    expect(s.joins.map((j) => j.state)).toEqual(["waiting"]);
    expect(s.busy).toBeNull();
    expect(s.error?.group).toBe("g1");
    expect(s.error?.text).toContain("Couldn't confirm the cancel");
    expect(s.error?.text).toContain("may still");
  });

  it("a rejected dismiss keeps the row and says so", async () => {
    vi.spyOn(bridge.groups, "list").mockResolvedValue([]);
    ingest({ joins: [join("expired")], invites: [] });
    vi.spyOn(bridge.rendezvous, "forget").mockRejectedValue(new Error("daemon down"));
    await forgetJoin("g1");
    const s = rendezvousSlice.get();
    expect(s.joins).toHaveLength(1);
    expect(s.error).toEqual({ group: "g1", text: "Couldn't dismiss this request: daemon down" });
  });

  it("the row renders the error, and both surfaces pass it", () => {
    const html = renderToStaticMarkup(
      <JoinRequests
        joins={[join("waiting")]}
        lastKnown={{}}
        listedGroups={[]}
        busy={null}
        receivedAt={1}
        now={1}
        onRetry={() => {}}
        onCancel={() => {}}
        onForget={() => {}}
        error={{ group: "g1", text: "Couldn't confirm the cancel: x. Your request may still be accepted." }}
      />,
    );
    expect(html).toContain("Couldn&#x27;t confirm the cancel");
    for (const f of ["../../surfaces/Groups.tsx", "../../onboarding/Onboarding.tsx"]) {
      expect(readFileSync(joinPath(__dirname, f), "utf8"), f).toContain("error={rv.error}");
    }
  });
});

// Codex round 4 (P2): a membership check that failed is retried, and onboarding reads the same list
// the retry gate reads.
describe("membership checks after a failed list", () => {
  it("an unchanged taken join retries after a failed group list, then shows the group", async () => {
    const list = vi.spyOn(bridge.groups, "list").mockRejectedValueOnce(new Error("daemon busy")).mockResolvedValue([group("g1")]);
    ingest({ joins: [join("waiting")], invites: [] });
    ingest({ joins: [join("taken")], invites: [] });
    await flush();
    expect(groupsSlice.get().groups).toEqual([]);
    ingest({ joins: [join("taken")], invites: [] }); // the next tick, unchanged
    await flush();
    expect(list).toHaveBeenCalledTimes(2);
    expect(groupsSlice.get().groups.map((g) => g.id)).toEqual(["g1"]);
  });

  it("a taken join whose list succeeded without the group isn't re-checked every tick", async () => {
    const list = vi.spyOn(bridge.groups, "list").mockResolvedValue([]);
    ingest({ joins: [join("waiting")], invites: [] });
    ingest({ joins: [join("taken")], invites: [] });
    await flush();
    ingest({ joins: [join("taken")], invites: [] });
    await flush();
    expect(list).toHaveBeenCalledTimes(1);
  });

  it("onboarding says Joined from the same group list the refresh fills", () => {
    rendezvousSlice.set({ joins: [join("joined")] });
    groupsSlice.set({ groups: [group("g1")] });
    expect(renderToStaticMarkup(<OnboardingJoins />)).toContain("Joined Aperture");
    groupsSlice.set({ groups: [] });
    expect(renderToStaticMarkup(<OnboardingJoins />)).not.toContain("Joined Aperture");
  });
});

// #390: rendezvous_cancel runs a tick before it answers, so a join://state event during the call is
// at least as new as the cancel answer. The answer never overwrites it.
describe("cancel answers and newer events", () => {
  it("a newer event that arrives before the cancel resolves wins", async () => {
    vi.spyOn(bridge.groups, "list").mockResolvedValue([group("g1")]);
    ingest({ joins: [join("waiting")], invites: [] });
    vi.spyOn(bridge.rendezvous, "cancel").mockImplementation(async () => {
      ingest({ joins: [join("joined")], invites: [] }); // the command's own tick confirmed the join
      return join("waiting"); // the cancel's earlier answer
    });
    await cancelJoin("g1");
    expect(rendezvousSlice.get().joins.map((j) => j.state)).toEqual(["joined"]);
  });

  it("with no event in between, the cancel answer is applied", async () => {
    vi.spyOn(bridge.groups, "list").mockResolvedValue([]);
    ingest({ joins: [join("waiting")], invites: [] });
    vi.spyOn(bridge.rendezvous, "cancel").mockResolvedValue(join("withdrawn"));
    await cancelJoin("g1");
    expect(rendezvousSlice.get().joins.map((j) => j.state)).toEqual(["withdrawn"]);
  });
});
