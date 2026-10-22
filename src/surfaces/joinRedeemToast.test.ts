// Review of 56edc33 — a citrate://invite link now joins through the relay rendezvous first, so
// "You're in" is said only when the legacy self-admit actually ran. A rendezvous join says the
// contract §11 word for the state the daemon reported.
import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { keepInviteAfter, redeemToast } from "./joinRedeemToast";
import type { JoinRedeemResult } from "../bridge/domains";

const rv = (state: string, reason: string | null = null): JoinRedeemResult => ({
  path: "rendezvous",
  join: { group: "g", name: "Aperture", state, reason, expiresAt: null, graceUntil: null, updatedAt: 1 },
});

describe("redeemToast", () => {
  it("legacy keeps the joined message", () => {
    expect(redeemToast({ path: "legacy" }, false)).toBe("You're in — welcome to the group.");
  });

  it("a rendezvous join is never announced as joined until the group is listed", () => {
    expect(redeemToast(rv("requesting"), false)).toBe("Asking to join…");
    expect(redeemToast(rv("waiting"), false)).toBe("Waiting for the person who invited you to let you in");
    expect(redeemToast(rv("joined"), false)).toBe("Joining Aperture…");
    expect(redeemToast(rv("joined"), true)).toBe("Joined Aperture");
    for (const s of ["requesting", "waiting", "joining"]) expect(redeemToast(rv(s), false)).not.toContain("You're in");
  });

  it("terminal and unreachable states use the §11 words", () => {
    expect(redeemToast(rv("taken"), false)).toBe("Someone else already used this invite");
    expect(redeemToast(rv("expired"), false)).toBe("This invite has expired");
    expect(redeemToast(rv("unknown", "purged"), false)).toBe("We couldn't find that invite");
    expect(redeemToast(rv("relay_unreachable"), false)).toBe("Couldn't check your invite · relay unreachable");
    expect(redeemToast(rv("dropped", "already_member"), false)).toBe("Already a member");
  });

  it("Groups uses it for the paste box and the pending banner", () => {
    const src = readFileSync(join(__dirname, "Groups.tsx"), "utf8");
    expect(src.match(/redeemToast\(/g)?.length ?? 0).toBeGreaterThanOrEqual(2);
    // The result is always read (never a bare `await bridge.invites.redeem(…)` followed by "You're in").
    expect(src).not.toMatch(/^\s*await bridge\.invites\.redeem\(/m);
    expect(src).not.toContain('store.toast("You\'re in');
  });
});

// Review round 2 (Codex P2 on 72db7f0): an answer with no definite outcome keeps the invite offered,
// the same rule as the Rust side's `decide_join` (forget = false), so pressing Join again is the retry.
describe("keepInviteAfter", () => {
  it("keeps the invite while requesting or when the relay was unreachable", () => {
    expect(keepInviteAfter(rv("requesting"))).toBe(true);
    expect(keepInviteAfter(rv("relay_unreachable"))).toBe(true);
  });

  it("drops it once the daemon has a definite answer, or the legacy self-admit ran", () => {
    for (const s of ["waiting", "joining", "joined", "taken", "expired", "revoked", "withdrawn", "dropped", "unknown", "welcome_expired", "reply_unverified"]) {
      expect(keepInviteAfter(rv(s)), s).toBe(false);
    }
    expect(keepInviteAfter({ path: "legacy" })).toBe(false);
  });

  it("the paste box and the pending banner only clear the invite when it isn't kept", () => {
    const src = readFileSync(join(__dirname, "Groups.tsx"), "utf8");
    const paste = src.slice(src.indexOf("const doRedeemLink"), src.indexOf("const doRole"));
    expect(paste).toMatch(/if \(!keepInviteAfter\(result\)\) \{\s*if \(redeemRef\.current\) redeemRef\.current\.value = "";\s*setRedeemOpen\(false\);/);
    const banner = src.slice(src.indexOf("const result = await bridge.invites.redeem(pi.url);"));
    expect(banner.slice(0, 600)).toContain("if (!keepInviteAfter(result)) store.clearPendingInvite();");
  });
});
