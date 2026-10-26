// GROW-S1 task 12 — the invitee path in the store. A v2 link (`citrate://join/<code>#K` or
// `https://citrate.ai/join/<code>#K`) is handed straight to Rust (pending_join_set) and never kept in
// webview state, localStorage or a log. Nothing is resolved on arrival; the member's Review click
// resolves, and only their explicit Join redeems. The legacy `citrate://invite` link still works.
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { store } from "./store";
import { bridge } from "../bridge";
import { STORAGE_KEY } from "./state";
import { locate } from "./places";
import type { JoinResolved, PendingJoinView } from "../bridge/domains";

const CODE = "Ab3dE7gh";
// The link-key fixture: bytes 00..0f as unpadded base64url ("AAEC…Dw"). Built at runtime so the
// secret scanner doesn't read a test constant as a credential.
const KEY = btoa(String.fromCharCode(...Array.from({ length: 16 }, (_, i) => i))).replace(/=+$/, "");
const HTTPS = `https://citrate.ai/join/${CODE}#${KEY}`;
const SCHEME = `citrate://join/${CODE}#${KEY}`;

const held: PendingJoinView = { hasKey: true, storedAt: 1, expiresAt: 2_000_000_000, persisted: true };
const resolved: JoinResolved = {
  code: CODE,
  kind: "direct",
  clusterName: "Garden club",
  goal: "storage",
  inviter: "@dana",
  inviterVerified: true,
  exp: "2100-01-01T00:00:00.000Z",
  actionable: true,
  keyMissing: false,
};

const flush = () => new Promise((r) => setTimeout(r, 0));

type Spy = ReturnType<typeof vi.spyOn>;
let spies: Record<string, Spy>;
let logs: Spy[];

function everythingLogged(): string {
  return logs.flatMap((s) => s.mock.calls.flat()).map((a) => (typeof a === "string" ? a : JSON.stringify(a))).join("\n");
}

function assertNoSecretAnywhere() {
  const state = JSON.stringify(store.state);
  expect(state).not.toContain(KEY);
  expect(state).not.toContain(`#${KEY}`);
  expect(state).not.toContain(CODE);
  const saved = localStorage.getItem(STORAGE_KEY) ?? "";
  expect(saved).not.toContain(KEY);
  expect(saved).not.toContain(CODE);
  const logged = everythingLogged();
  expect(logged).not.toContain(KEY);
  expect(logged).not.toContain(CODE);
}

describe("v2 invite deep links (task 12)", () => {
  beforeEach(() => {
    vi.useRealTimers();
    localStorage.clear();
    store.setState({ pendingInvite: null, pendingJoin: null, pendingPairLink: null, route: "home", stage: "done", signedIn: true });
    spies = {
      set: vi.spyOn(bridge.join, "pendingSet").mockResolvedValue(held),
      get: vi.spyOn(bridge.join, "pendingGet").mockResolvedValue(null),
      clear: vi.spyOn(bridge.join, "pendingClear").mockResolvedValue(undefined),
      resolve: vi.spyOn(bridge.join, "pendingResolve").mockResolvedValue(resolved),
      redeem: vi.spyOn(bridge.join, "redeem").mockResolvedValue(undefined),
      legacyRedeem: vi.spyOn(bridge.invites, "redeem").mockResolvedValue(undefined),
    };
    logs = (["log", "info", "warn", "error", "debug"] as const).map((m) => vi.spyOn(console, m).mockImplementation(() => {}));
  });
  afterEach(() => {
    vi.restoreAllMocks();
    store.setState({ pendingJoin: null, pendingInvite: null });
  });

  it.each([HTTPS, SCHEME])("%s goes to Rust as-is and is not resolved on arrival", async (url) => {
    store.handleDeepLink(url);
    await flush();
    expect(spies.set).toHaveBeenCalledWith(url);
    expect(spies.resolve).not.toHaveBeenCalled();
    expect(spies.redeem).not.toHaveBeenCalled();
    expect(store.state.pendingJoin).toMatchObject({ phase: "waiting", hasKey: true, persisted: true });
    expect(store.state.pendingInvite).toBeNull();
    expect(locate(store.state.route).place).toBe("groups");
    store.save();
    await new Promise((r) => setTimeout(r, 450));
    assertNoSecretAnywhere();
  });

  it("during first-run setup the link waits for onboarding instead of opening Groups", async () => {
    store.setState({ stage: "s2", route: "onboarding" });
    store.handleDeepLink(SCHEME);
    await flush();
    expect(store.state.pendingJoin?.phase).toBe("waiting");
    expect(store.state.route).toBe("onboarding");
    expect(spies.resolve).not.toHaveBeenCalled();
  });

  it("Review resolves once, then Join redeems that code and clears the waiting invite", async () => {
    store.handleDeepLink(HTTPS);
    await flush();
    await store.reviewPendingJoin();
    expect(spies.resolve).toHaveBeenCalledTimes(1);
    expect(spies.redeem).not.toHaveBeenCalled();
    expect(store.state.pendingJoin).toMatchObject({ phase: "ready", resolved: { clusterName: "Garden club", actionable: true } });
    await store.redeemPendingJoin();
    expect(spies.redeem).toHaveBeenCalledWith(CODE);
    expect(store.state.pendingJoin).toBeNull();
    assertNoSecretAnywhere();
  });

  it("a failed Join keeps the reviewed invite so the member can press Join again", async () => {
    spies.redeem.mockRejectedValueOnce(new Error("comms daemon not running"));
    store.handleDeepLink(HTTPS);
    await flush();
    await store.reviewPendingJoin();
    await store.redeemPendingJoin();
    expect(store.state.pendingJoin).toMatchObject({ phase: "ready" });
    expect(store.state.pendingJoin?.error).toMatch(/try again/i);
    await store.redeemPendingJoin();
    expect(spies.redeem).toHaveBeenCalledTimes(2);
    expect(store.state.pendingJoin).toBeNull();
  });

  it("Join does nothing for an invite that isn't actionable (public or keyless)", async () => {
    spies.resolve.mockResolvedValue({ ...resolved, kind: "referral", actionable: false });
    store.handleDeepLink(HTTPS);
    await flush();
    await store.reviewPendingJoin();
    await store.redeemPendingJoin();
    expect(spies.redeem).not.toHaveBeenCalled();
  });

  it("Cancel wipes it in Rust", async () => {
    store.handleDeepLink(HTTPS);
    await flush();
    await store.cancelPendingJoin();
    expect(spies.clear).toHaveBeenCalled();
    expect(store.state.pendingJoin).toBeNull();
  });

  it("a resolve error is shown honestly and keeps no secret", async () => {
    spies.resolve.mockRejectedValue(new Error("join: invite not found"));
    store.handleDeepLink(HTTPS);
    await flush();
    await store.reviewPendingJoin();
    expect(store.state.pendingJoin).toMatchObject({ phase: "error" });
    expect(store.state.pendingJoin?.error).toMatch(/expired|revoked/i);
    assertNoSecretAnywhere();
  });

  it("a malformed v2 link is refused by Rust and nothing is kept", async () => {
    spies.set.mockRejectedValue(new Error("join: invalid invite request"));
    store.handleDeepLink(`citrate://join/${CODE}#short`);
    await flush();
    expect(store.state.pendingJoin).toBeNull();
    expect(JSON.stringify(store.state)).not.toContain("#short");
  });

  it("on start-up a waiting invite held in Rust is picked up (no code, no key)", async () => {
    spies.get.mockResolvedValue(held);
    await store.refreshPendingJoin();
    expect(store.state.pendingJoin).toMatchObject({ phase: "waiting", hasKey: true });
    expect(spies.resolve).not.toHaveBeenCalled();
  });

  it("a pasted link is the member acting: it is held and, once set up, reviewed", async () => {
    await store.acceptInviteInput(`${CODE}#${KEY}`);
    expect(spies.set).toHaveBeenCalledWith(`${CODE}#${KEY}`);
    expect(spies.resolve).toHaveBeenCalledTimes(1);
    assertNoSecretAnywhere();
  });

  it("a pasted link during setup is held but not resolved yet", async () => {
    store.setState({ stage: "s1", signedIn: false });
    await store.acceptInviteInput(HTTPS);
    expect(spies.set).toHaveBeenCalled();
    expect(spies.resolve).not.toHaveBeenCalled();
  });

  it("the legacy citrate://invite link still lands on the Groups Join banner", () => {
    const legacy = "citrate://invite?g=grp_1&t=tok123&k=02ab";
    store.handleDeepLink(legacy);
    expect(store.state.pendingInvite?.url).toBe(legacy);
    expect(locate(store.state.route).place).toBe("groups");
    expect(spies.set).not.toHaveBeenCalled();
  });

  it("Join is offered from the wallet step through the end of setup, including s7 (Activate validator)", () => {
    for (const stage of ["s4", "s5", "s6", "s7", "done"]) {
      store.setState({ stage: stage as never, signedIn: true });
      expect(store.canJoinNow(), stage).toBe(true);
    }
    for (const stage of ["s0", "s1", "s2", "s3"]) {
      store.setState({ stage: stage as never, signedIn: true });
      expect(store.canJoinNow(), stage).toBe(false);
    }
    store.setState({ stage: "s7" as never, signedIn: false });
    expect(store.canJoinNow()).toBe(false);
  });

  it("a Review answer for an invite that was replaced meanwhile is dropped", async () => {
    let answer!: (r: JoinResolved) => void;
    spies.resolve.mockImplementationOnce(() => new Promise<JoinResolved>((res) => (answer = res)));
    store.handleDeepLink(HTTPS);
    await flush();
    const review = store.reviewPendingJoin();
    expect(store.state.pendingJoin?.phase).toBe("resolving");
    spies.set.mockResolvedValueOnce({ ...held, hasKey: false });
    store.handleDeepLink("citrate://join/Zz9yX8wv");
    await flush();
    answer(resolved);
    await review;
    expect(store.state.pendingJoin).toMatchObject({ phase: "waiting", hasKey: false });
    expect(store.state.pendingJoin?.resolved).toBeUndefined();
    await store.redeemPendingJoin();
    expect(spies.redeem).not.toHaveBeenCalled();
  });

  it("a Review failure for an invite that was replaced meanwhile doesn't mark the new one", async () => {
    let fail!: (e: Error) => void;
    spies.resolve.mockImplementationOnce(() => new Promise<JoinResolved>((_, rej) => (fail = rej)));
    store.handleDeepLink(HTTPS);
    await flush();
    const review = store.reviewPendingJoin();
    store.handleDeepLink("citrate://join/Zz9yX8wv");
    await flush();
    fail(new Error("join: invite not found"));
    await review;
    expect(store.state.pendingJoin).toMatchObject({ phase: "waiting" });
    expect(store.state.pendingJoin?.error).toBeUndefined();
  });

  it("a Cancel while Review is in flight stays cancelled", async () => {
    let answer!: (r: JoinResolved) => void;
    spies.resolve.mockImplementationOnce(() => new Promise<JoinResolved>((res) => (answer = res)));
    store.handleDeepLink(HTTPS);
    await flush();
    const review = store.reviewPendingJoin();
    await store.cancelPendingJoin();
    answer(resolved);
    await review;
    expect(store.state.pendingJoin).toBeNull();
  });

  it("a Join that finishes after a new invite arrived leaves the new one waiting", async () => {
    let done!: () => void;
    spies.redeem.mockImplementationOnce(() => new Promise<void>((res) => (done = res)));
    store.handleDeepLink(HTTPS);
    await flush();
    await store.reviewPendingJoin();
    const join = store.redeemPendingJoin();
    store.handleDeepLink("citrate://join/Zz9yX8wv");
    await flush();
    done();
    await join;
    expect(spies.redeem).toHaveBeenCalledWith(CODE);
    expect(store.state.pendingJoin).toMatchObject({ phase: "waiting" });
  });

  it("a Cancel while the link is still being stored stays cancelled, and Rust clears after the store", async () => {
    let stored!: (v: PendingJoinView) => void;
    spies.set.mockImplementationOnce(() => new Promise<PendingJoinView>((res) => (stored = res)));
    store.handleDeepLink(HTTPS);
    await flush();
    const cancel = store.cancelPendingJoin();
    await flush();
    expect(spies.clear, "clear waits for the set in flight").not.toHaveBeenCalled();
    stored(held);
    await cancel;
    expect(spies.clear).toHaveBeenCalledTimes(1);
    expect(spies.set.mock.invocationCallOrder[0]).toBeLessThan(spies.clear.mock.invocationCallOrder[0]);
    expect(store.state.pendingJoin).toBeNull();
  });

  it("two links arriving together are stored one after the other, and the later one is shown", async () => {
    let first!: (v: PendingJoinView) => void;
    spies.set.mockImplementationOnce(() => new Promise<PendingJoinView>((res) => (first = res)));
    spies.set.mockResolvedValueOnce({ ...held, hasKey: false });
    store.handleDeepLink(HTTPS);
    store.handleDeepLink("citrate://join/Zz9yX8wv");
    await flush();
    expect(spies.set, "the second waits for the first").toHaveBeenCalledTimes(1);
    first(held);
    await flush();
    await flush();
    expect(spies.set).toHaveBeenCalledTimes(2);
    expect(spies.set.mock.calls.map((c) => c[0])).toEqual([HTTPS, "citrate://join/Zz9yX8wv"]);
    expect(store.state.pendingJoin).toMatchObject({ phase: "waiting", hasKey: false });
  });

  it("a malformed link arriving right after a good one leaves the good one shown", async () => {
    spies.set.mockResolvedValueOnce(held);
    spies.set.mockRejectedValueOnce(new Error("join: invalid invite request"));
    store.handleDeepLink(HTTPS);
    store.handleDeepLink(`citrate://join/${CODE}#short`);
    await flush();
    await flush();
    expect(store.state.pendingJoin).toMatchObject({ phase: "waiting", hasKey: true });
  });

  it("the start-up check doesn't clear an invite that arrived while it ran", async () => {
    let none!: (v: PendingJoinView | null) => void;
    spies.get.mockImplementationOnce(() => new Promise<PendingJoinView | null>((res) => (none = res)));
    const refresh = store.refreshPendingJoin();
    store.handleDeepLink(SCHEME);
    await flush();
    expect(store.state.pendingJoin?.phase).toBe("waiting");
    none(null);
    await refresh;
    expect(store.state.pendingJoin).toMatchObject({ phase: "waiting", hasKey: true });
  });

  it.each([
    "join: invite links are temporarily unavailable",
    "join: could not reach the invite service",
    "join: too many invite requests, try again later",
    "join: the waiting invite changed while it was being checked, review it again",
  ])("a temporary Review failure (%s) keeps the invite and Review can be retried", async (msg) => {
    spies.resolve.mockRejectedValueOnce(new Error(msg));
    store.handleDeepLink(HTTPS);
    await flush();
    await store.reviewPendingJoin();
    expect(store.state.pendingJoin).toMatchObject({ phase: "error", retryable: true });
    expect(spies.clear).not.toHaveBeenCalled();
    await store.reviewPendingJoin();
    expect(spies.resolve).toHaveBeenCalledTimes(2);
    expect(spies.set).toHaveBeenCalledTimes(1);
    expect(store.state.pendingJoin).toMatchObject({ phase: "ready", resolved: { clusterName: "Garden club" } });
    expect(store.state.pendingJoin?.retryable).toBeFalsy();
  });

  it.each(["join: invite not found", "join: the invite could not be verified"])("a final Review failure (%s) is not offered as a retry", async (msg) => {
    spies.resolve.mockRejectedValueOnce(new Error(msg));
    store.handleDeepLink(HTTPS);
    await flush();
    await store.reviewPendingJoin();
    expect(store.state.pendingJoin).toMatchObject({ phase: "error" });
    expect(store.state.pendingJoin?.retryable).toBeFalsy();
  });

  it("a Cancel the device keychain couldn't finish says the invite may come back", async () => {
    spies.clear.mockRejectedValueOnce(new Error("keyring unavailable"));
    store.handleDeepLink(HTTPS);
    await flush();
    await store.cancelPendingJoin();
    expect(store.state.pendingJoin).toBeNull();
    expect(store.state.toast).toMatch(/keychain/i);
    expect(store.state.toast).toMatch(/restart/i);
    assertNoSecretAnywhere();
  });

  it("pendingJoin is not a persisted key", async () => {
    const { PERSIST_KEYS } = await import("./state");
    expect(PERSIST_KEYS).not.toContain("pendingJoin");
    expect(PERSIST_KEYS).not.toContain("pendingInvite");
  });
});
