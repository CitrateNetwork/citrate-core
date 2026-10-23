// =====================================================================
// citrate-core — rendezvous slice (GROW-S2 / Join 9, task M5)
//
// The latest relay-rendezvous views from the member daemon: the invitee's joins and the inviter's
// invites. Fed by the `join://state` event (the Rust tick emits it at app start, every ≤ 30 s and
// after a join) and by `refresh()` (a tick on demand). Views only; no secrets. When a join reaches
// the group, the group list (groupsSlice) is reloaded so the row can say Joined.
//
// `lastKnown*` keeps, per join / invite, the last view that wasn't `relay_unreachable`, so the UI can
// show "last known: Waiting… (time)" and never turn an unreachable relay into "expired" (§11).
// A failed tick changes nothing here: the last views stay.
// =====================================================================
import { createSlice } from "./createSlice";
import { bridge } from "../../bridge";
import { groupsSlice, refreshGroups } from "./groups";
import { reachedStep } from "../../privacy/joinFunnel";
import type { InviteViewDto, JoinViewDto, RendezvousViews } from "../../bridge/domains";

export interface RendezvousState {
  joins: JoinViewDto[];
  invites: InviteViewDto[];
  lastKnownJoin: Record<string, JoinViewDto>;
  lastKnownInvite: Record<string, InviteViewDto>;
  /** When the app last got views from the daemon (ms, app clock); 0 = never. The freshness rule
   *  (§11) uses it for a view with no relay answer yet. */
  receivedAt: number;
  /** A cancel / forget / retry in flight, by group or lookup. */
  busy: string | null;
  /** The last cancel / forget that failed, or null. */
  error: JoinActionError | null;
}

/** A failed invitee action on one join: which group, and the words to show under its row. */
export interface JoinActionError {
  group: string;
  text: string;
}

export const rendezvousSlice = createSlice<RendezvousState>({
  joins: [],
  invites: [],
  lastKnownJoin: {},
  lastKnownInvite: {},
  receivedAt: 0,
  busy: null,
  error: null,
});

/** Fold a fresh set of views in (pure on the slice state; exported for tests). */
export function applyViews(s: RendezvousState, v: RendezvousViews, at: number = Date.now()): Partial<RendezvousState> {
  const lastKnownJoin = { ...s.lastKnownJoin };
  for (const j of v.joins) if (j.state !== "relay_unreachable") lastKnownJoin[j.group] = j;
  const lastKnownInvite = { ...s.lastKnownInvite };
  for (const i of v.invites) if (i.state !== "relay_unreachable") lastKnownInvite[i.lookup] = i;
  return { joins: v.joins, invites: v.invites, lastKnownJoin, lastKnownInvite, receivedAt: at };
}

/** Whether these views mean the group list is out of date (pure; exported for tests): a join newly
 *  reached `joined` or `taken`, a `joined` group still isn't in `listed`, or a `taken` group isn't
 *  listed and the last list read failed (so non-membership isn't confirmed). The row only says
 *  Joined once the group is listed (§11), so the list has to be reloaded for it to get there. Every
 *  join surface reads `groupsSlice` (the daemon's `listGroups`), the list this refresh fills. */
export function needsMembershipRefresh(prev: JoinViewDto[], next: JoinViewDto[], listed: string[], lastListFailed = false): boolean {
  return next.some((j) => {
    if (j.state !== "joined" && j.state !== "taken") return false;
    const before = prev.find((p) => p.group === j.group);
    if (before?.state !== j.state) return true;
    if (listed.includes(j.group)) return false;
    return j.state === "joined" || lastListFailed;
  });
}

/** Whether the last group-list read this slice asked for failed. */
let lastListFailed = false;

export function ingest(v: RendezvousViews): void {
  const prev = rendezvousSlice.get().joins;
  rendezvousSlice.set((s) => applyViews(s, v));
  const listed = groupsSlice.get().groups.map((g) => g.id);
  if (needsMembershipRefresh(prev, v.joins, listed, lastListFailed)) {
    void refreshGroups().then(() => {
      const g = groupsSlice.get();
      lastListFailed = g.error !== null;
      // 0.5.2 Join funnel: a join that completed in the background is the join_group step.
      const listedNow = g.groups.map((x) => x.id);
      if (v.joins.some((j) => (j.state === "joined" || j.state === "taken") && listedNow.includes(j.group))) {
        reachedStep("join_group");
      }
    });
  }
}

let unlisten: (() => void) | null = null;

/** Subscribe once to `join://state`. Safe to call repeatedly. */
export async function startRendezvousFeed(): Promise<void> {
  if (unlisten) return;
  try {
    unlisten = await bridge.rendezvous.onState(ingest);
  } catch {
    /* no event channel (web preview): the slice stays empty */
  }
}

/** Run a tick now (Groups open, Retry). A failure leaves the last views in place. */
export async function refreshRendezvous(): Promise<void> {
  try {
    ingest(await bridge.rendezvous.tick());
  } catch {
    /* daemon not started / older daemon: keep the last views, show nothing new */
  }
}

/** Fold one join view (the answer to cancel) into the list. */
function upsertJoin(view: JoinViewDto): void {
  rendezvousSlice.set((s) => {
    const joins = s.joins.some((j) => j.group === view.group)
      ? s.joins.map((j) => (j.group === view.group ? view : j))
      : [...s.joins, view];
    // A cancel answer is one join's view, not a full tick: keep the tick's receivedAt.
    return { ...applyViews(s, { joins, invites: s.invites }), receivedAt: s.receivedAt };
  });
}

export async function cancelJoin(group: string): Promise<void> {
  rendezvousSlice.set({ busy: group, error: null });
  try {
    upsertJoin(await bridge.rendezvous.cancel(group));
  } catch (e) {
    // Not confirmed is not withdrawn: keep the last confirmed state and say the request may still go
    // through (the daemon may have the cancel persisted, or never got it).
    rendezvousSlice.set({
      error: { group, text: `Couldn't confirm the cancel: ${e instanceof Error ? e.message : String(e)}. Your request may still be accepted.` },
    });
  } finally {
    rendezvousSlice.set({ busy: null });
  }
}

export async function forgetJoin(group: string): Promise<void> {
  rendezvousSlice.set({ busy: group, error: null });
  try {
    await bridge.rendezvous.forget(group);
    rendezvousSlice.set((s) => ({ joins: s.joins.filter((j) => j.group !== group) }));
  } catch (e) {
    rendezvousSlice.set({ error: { group, text: `Couldn't dismiss this request: ${e instanceof Error ? e.message : String(e)}` } });
  } finally {
    rendezvousSlice.set({ busy: null });
  }
}
