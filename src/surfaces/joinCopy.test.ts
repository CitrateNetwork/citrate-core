// GROW-S2 (Join 9, M5) — every JoinView / InviteView state maps to its contract §11 copy, word for
// word. relay_unreachable shows the last known state and never "expired" or a failure.
import { describe, it, expect } from "vitest";
import type { InviteViewDto, JoinStateWord, JoinViewDto, InviteStateWord } from "../bridge/domains";
import { FRESH_MS, inviteCopy, joinCopy, readyToJoinCopy, shortJoiner, type JoinCopyContext } from "./joinCopy";
import { applyViews, type RendezvousState } from "../shell/slices/rendezvous";

const fmtDate = (ms: number) => `D${ms}`;
const fmtTime = (ms: number) => `T${ms}`;

function jv(state: JoinStateWord, over: Partial<JoinViewDto> = {}): JoinViewDto {
  return { group: "g1", name: "Aperture", state, reason: null, expiresAt: 100, graceUntil: 200, updatedAt: 50, ...over };
}
// `updatedAt` is 50 in the fixtures; "now" 10 s later is fresh.
const NOW = 10_050;
function ctx(over: Partial<JoinCopyContext> = {}): JoinCopyContext {
  return { cluster: "Aperture", groupListed: false, fmtDate, fmtTime, now: NOW, receivedAt: NOW, ...over };
}
function iv(state: InviteStateWord, over: Partial<InviteViewDto> = {}): InviteViewDto {
  return {
    lookup: "ll",
    group: "g1",
    state,
    joiner: "0x12aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaab",
    expiresAt: 300,
    updatedAt: 40,
    reason: null,
    dropped: 0,
    ...over,
  };
}

describe("invitee copy (§11)", () => {
  it("ready to join, before any JoinView", () => {
    expect(readyToJoinCopy("Aperture").title).toBe("Ready to join Aperture");
  });

  it("maps each state to its word", () => {
    const cases: [JoinStateWord, Partial<JoinViewDto>, Partial<JoinCopyContext>, string][] = [
      ["requesting", {}, {}, "Asking to join…"],
      ["waiting", {}, { inviter: "Larry" }, "Waiting for Larry to let you in · until D200"],
      ["waiting", {}, {}, "Waiting for the person who invited you to let you in · until D200"],
      ["joining", {}, {}, "Joining Aperture…"],
      ["joined", {}, { groupListed: true }, "Joined Aperture"],
      ["expired", {}, {}, "This invite has expired"],
      ["welcome_expired", {}, {}, "This invite wasn't picked up in time. Ask for a new one."],
      ["revoked", {}, {}, "This invite was withdrawn"],
      ["taken", {}, {}, "Someone else already used this invite"],
      ["taken", {}, { groupListed: true }, "Joined Aperture"],
      ["dropped", { reason: "bad_binding" }, { inviter: "Larry" }, "Larry's app couldn't add you · your device identity didn't verify"],
      ["dropped", { reason: "bad_mac" }, { inviter: "Larry" }, "Larry's app couldn't add you · the request didn't verify; ask for a new invite"],
      ["dropped", { reason: "bad_key_package" }, { inviter: "Larry" }, "Larry's app couldn't add you · the request didn't verify; ask for a new invite"],
      ["dropped", { reason: "already_member" }, {}, "Already a member"],
      ["withdrawn", {}, {}, "You cancelled this request"],
      ["unknown", { reason: "no_rendezvous" }, {}, "We couldn't find that invite"],
      ["unknown", { reason: "purged" }, {}, "We couldn't find that invite"],
      ["relay_unreachable", {}, {}, "Couldn't check your invite · relay unreachable"],
      ["relay_unsupported", {}, {}, "Invites need an update on our side"],
      ["reply_unverified", {}, {}, "The reply to your invite couldn't be verified"],
      ["failed", { reason: "mls" }, {}, "Couldn't finish joining · MLS join error"],
      ["failed", { reason: "already_delivered" }, {}, "Couldn't finish joining · ask for a new invite"],
    ];
    for (const [state, v, c, title] of cases) expect(joinCopy(jv(state, v), ctx(c)).title, state).toBe(title);
  });

  it("waiting says you can close Citrate and never says offline", () => {
    const c = joinCopy(jv("waiting"), ctx());
    expect(c.sub).toBe("Their Citrate adds you the next time it's open. You can close Citrate.");
    expect(JSON.stringify(c).toLowerCase()).not.toContain("offline");
  });

  it("joined needs the group in the daemon's group list (no optimistic joined)", () => {
    expect(joinCopy(jv("joined"), ctx({ groupListed: false })).title).not.toContain("Joined");
  });

  it("relay_unreachable shows the last known state and never expired or failed", () => {
    const c = joinCopy(jv("relay_unreachable"), ctx({ lastKnown: jv("waiting", { updatedAt: 77 }) }));
    expect(c.lastKnown).toBe("last known: Waiting… (T77)");
    expect(c.retry).toBe(true);
    for (const prior of ["expired", "failed", "revoked", "taken", "welcome_expired"] as JoinStateWord[]) {
      const d = joinCopy(jv("relay_unreachable"), ctx({ lastKnown: jv(prior) }));
      const text = `${d.title} ${d.lastKnown ?? ""}`.toLowerCase();
      expect(text, prior).not.toContain("expired");
      expect(text, prior).not.toContain("withdrawn");
      expect(d.lastKnown, prior).toBeUndefined();
    }
  });

  it("final failures offer no Retry; recoverable ones do", () => {
    for (const r of ["group_mismatch", "binding_rejected", "domain_mismatch", "too_large", "quota_exceeded", "already_delivered"]) {
      expect(joinCopy(jv("failed", { reason: r }), ctx()).retry, r).toBe(false);
    }
    expect(joinCopy(jv("failed", { reason: "mls" }), ctx()).retry).toBe(true);
  });

  it("unknown is never worded as expired", () => {
    expect(joinCopy(jv("unknown", { reason: "purged" }), ctx()).title.toLowerCase()).not.toContain("expired");
  });

  it("cancel is offered only while the request is pending", () => {
    expect(joinCopy(jv("requesting"), ctx()).cancel).toBe(true);
    expect(joinCopy(jv("waiting"), ctx()).cancel).toBe(true);
    expect(joinCopy(jv("joining"), ctx()).cancel).toBeFalsy();
  });
});

describe("inviter copy (§11)", () => {
  it("shortens addresses as 0x12…ab", () => {
    expect(shortJoiner("0x12aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaab")).toBe("0x12…ab");
  });

  it("maps each state to its word", () => {
    const cases: [InviteStateWord, Partial<InviteViewDto>, string][] = [
      ["open", { joiner: null }, "Not used yet · expires D300"],
      ["adding", {}, "Adding 0x12…ab…"],
      ["added", {}, "Added 0x12…ab · waiting for them to open Citrate"],
      ["picked_up", {}, "Picked up by 0x12…ab"],
      ["welcome_expired", {}, "Not picked up · Remove 0x12…ab?"],
      ["expired", {}, "Expired"],
      ["revoked", {}, "Withdrawn"],
      ["failed", { reason: "too_large" }, "Couldn't add · the group is too big for an invite-link admit today"],
      ["failed", { reason: "not_a_member" }, "Couldn't add · you are no longer in this group"],
      ["relay_unreachable", {}, "Couldn't check · relay unreachable"],
      ["unknown", { reason: "purged" }, "No longer on the relay · check the member list"],
      ["unknown", { reason: "legacy_redeemed" }, "No longer on the relay · check the member list"],
    ];
    for (const [state, v, title] of cases) expect(inviteCopy(iv(state, v), { fmtDate, fmtTime, now: NOW, receivedAt: NOW }).title, state).toBe(title);
  });

  it("welcome_expired offers Remove", () => {
    expect(inviteCopy(iv("welcome_expired"), { fmtDate, fmtTime, now: NOW, receivedAt: NOW }).remove).toBe(true);
  });

  it("relay_unreachable shows the last known state, never expired or picked up", () => {
    const c = inviteCopy(iv("relay_unreachable"), { fmtDate, fmtTime, now: NOW, receivedAt: NOW, lastKnown: iv("added") });
    expect(c.lastKnown).toBe("last known: Added 0x12…ab · waiting for them to open Citrate (T40)");
    const d = inviteCopy(iv("relay_unreachable"), { fmtDate, fmtTime, now: NOW, receivedAt: NOW, lastKnown: iv("expired") });
    expect(d.lastKnown).toBeUndefined();
    expect(d.title.toLowerCase()).not.toContain("expired");
  });

  it("unknown is never shown as expired or picked up", () => {
    const t = inviteCopy(iv("unknown", { reason: "purged" }), { fmtDate, fmtTime, now: NOW, receivedAt: NOW }).title.toLowerCase();
    expect(t).not.toContain("expired");
    expect(t).not.toContain("picked up");
  });
});

describe("freshness (§9 / §11: a state is live only while updated_at is ≤ 60 s old)", () => {
  it("exposes the §9 threshold", () => {
    expect(FRESH_MS).toBe(60_000);
  });

  it("a stale Waiting reads as relay unreachable with the last known state", () => {
    const c = joinCopy(jv("waiting", { updatedAt: 1_000 }), ctx({ now: 1_000 + 60_001 }));
    expect(c.title).toBe("Couldn't check your invite · relay unreachable");
    expect(c.lastKnown).toBe("last known: Waiting… (T1000)");
    expect(c.retry).toBe(true);
    expect(c.cancel).toBeFalsy();
  });

  it("exactly 60 s old is still fresh", () => {
    expect(joinCopy(jv("waiting", { updatedAt: 1_000 }), ctx({ now: 61_000 })).title).toContain("Waiting for");
  });

  it("applies to every live invitee state, never to a terminal one", () => {
    const old = { updatedAt: 1 };
    for (const st of ["waiting", "joining", "reply_unverified"] as JoinStateWord[]) {
      expect(joinCopy(jv(st, old), ctx({ now: 100_000 })).title, st).toBe("Couldn't check your invite · relay unreachable");
    }
    for (const st of ["joined", "expired", "revoked", "taken", "withdrawn", "unknown", "failed"] as JoinStateWord[]) {
      expect(joinCopy(jv(st, old), ctx({ now: 100_000, groupListed: true })).title, st).not.toContain("relay unreachable");
    }
  });

  it("a requesting join with no relay answer yet goes stale on when the app last heard from the daemon", () => {
    const v = jv("requesting", { updatedAt: null });
    expect(joinCopy(v, ctx({ now: 70_000, receivedAt: 69_000 })).title).toBe("Asking to join…");
    const c = joinCopy(v, ctx({ now: 70_000, receivedAt: 5_000 }));
    expect(c.title).toBe("Couldn't check your invite · relay unreachable");
    expect(c.lastKnown).toBe("last known: Asking to join…");
  });

  it("a stale view never reads as expired", () => {
    const c = joinCopy(jv("waiting", { updatedAt: 1 }), ctx({ now: 10_000_000 }));
    expect(`${c.title} ${c.lastKnown}`.toLowerCase()).not.toContain("expired");
  });

  it("inviter rows: stale open / adding / added read as unreachable with the last known state", () => {
    const now = 40 + 60_001;
    const c = inviteCopy(iv("added"), { fmtDate, fmtTime, now, receivedAt: now });
    expect(c.title).toBe("Couldn't check · relay unreachable");
    expect(c.lastKnown).toBe("last known: Added 0x12…ab · waiting for them to open Citrate (T40)");
    for (const st of ["open", "adding"] as InviteStateWord[]) {
      expect(inviteCopy(iv(st), { fmtDate, fmtTime, now, receivedAt: now }).title, st).toBe("Couldn't check · relay unreachable");
    }
    for (const st of ["picked_up", "expired", "revoked", "unknown", "welcome_expired"] as InviteStateWord[]) {
      expect(inviteCopy(iv(st), { fmtDate, fmtTime, now, receivedAt: now }).title, st).not.toContain("relay unreachable");
    }
  });
});

describe("rendezvous slice: last known", () => {
  const empty: RendezvousState = { joins: [], invites: [], lastKnownJoin: {}, lastKnownInvite: {}, receivedAt: 0, busy: null, error: null };

  it("keeps the last non-unreachable view across an unreachable tick", () => {
    const a = { ...empty, ...applyViews(empty, { joins: [jv("waiting")], invites: [iv("added")] }) };
    const b = { ...a, ...applyViews(a, { joins: [jv("relay_unreachable")], invites: [iv("relay_unreachable")] }) };
    expect(b.joins[0].state).toBe("relay_unreachable");
    expect(b.lastKnownJoin.g1.state).toBe("waiting");
    expect(b.lastKnownInvite.ll.state).toBe("added");
  });

  it("records when the app last heard from the daemon", () => {
    const a = { ...empty, ...applyViews(empty, { joins: [], invites: [] }, 1234) };
    expect(a.receivedAt).toBe(1234);
  });
});
