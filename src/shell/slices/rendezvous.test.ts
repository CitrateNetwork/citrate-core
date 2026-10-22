// GROW-S2 (Join 9, M5) review: a join that completes after the group list was loaded refreshes the
// group list, so the row turns into "Joined ⟨cluster⟩" and the group appears where the member is.
import { describe, it, expect, vi, afterEach } from "vitest";
import { readFileSync } from "node:fs";
import { join as joinPath } from "node:path";
import { bridge } from "../../bridge";
import type { Group, JoinViewDto } from "../../bridge/domains";
import { groupsSlice, refreshGroups } from "./groups";
import { ingest, onJoinCompleted, rendezvousSlice } from "./rendezvous";

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
  rendezvousSlice.set({ joins: [], invites: [], lastKnownJoin: {}, lastKnownInvite: {}, receivedAt: 0 });
});

describe("join completion refreshes the group list", () => {
  it("waiting → joined after the initial group-list load reloads the list and tells listeners", async () => {
    const list = vi.spyOn(bridge.groups, "list").mockResolvedValueOnce([]).mockResolvedValue([group("g1")]);
    const seen = vi.fn();
    const off = onJoinCompleted(seen);
    await refreshGroups(); // Groups opened while the request waits
    ingest({ joins: [join("waiting")], invites: [] });
    await flush();
    expect(list).toHaveBeenCalledTimes(1);
    expect(seen).not.toHaveBeenCalled();
    ingest({ joins: [join("joined")], invites: [] }); // the background tick's join://state
    await flush();
    expect(list).toHaveBeenCalledTimes(2);
    expect(groupsSlice.get().groups.map((g) => g.id)).toEqual(["g1"]);
    expect(seen).toHaveBeenCalledTimes(1);
    off();
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

  it("an unsubscribed listener isn't called", async () => {
    vi.spyOn(bridge.groups, "list").mockResolvedValue([group("g1")]);
    const seen = vi.fn();
    onJoinCompleted(seen)();
    ingest({ joins: [join("joined")], invites: [] });
    await flush();
    expect(seen).not.toHaveBeenCalled();
  });

  it("onboarding reloads its group list on completion and ticks on mount", () => {
    const src = readFileSync(joinPath(__dirname, "../../onboarding/Onboarding.tsx"), "utf8");
    const body = src.slice(src.indexOf("function OnboardingJoins"), src.indexOf("function ClaimBanner"));
    expect(body).toContain("onJoinCompleted(() => void store.refreshPeople())");
    expect(body).toContain("void refreshRendezvous();");
  });
});
