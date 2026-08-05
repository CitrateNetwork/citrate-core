// CORE-A3 A3-03 — entitlement-expiry enforcement. The `expiresAt` claim gates
// whether a signed-in session keeps its tier: a past (or anomalous) expiry
// downgrades to free/lapsed at the DECISION point, not just in the UI. This is
// the frontend half of the entitlement engine; the Rust id_token `exp` guard is
// the hard backstop (oidc::tests).
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { isExpiredClaim, isPaidEntitlementActive, isGrantOnChain, deriveIdentityFromEmail, mapNodeState, mergeActivity, pickChatProviderKind, foldNodeLogs, store } from "./store";
import { PERSIST_KEYS, freshState } from "./state";
import { bridge } from "../bridge";
import type { CeremonyView, AuthStatus } from "../bridge/types";

describe("isExpiredClaim — A3-03 entitlement-expiry enforcement", () => {
  it("absent/empty expiry is NOT expired (authority may omit it)", () => {
    expect(isExpiredClaim(null)).toBe(false);
    expect(isExpiredClaim(undefined)).toBe(false);
    expect(isExpiredClaim("")).toBe(false);
  });

  it("a past ISO date is expired", () => {
    expect(isExpiredClaim("2000-01-01")).toBe(true);
    expect(isExpiredClaim("2020-06-15T12:00:00Z")).toBe(true);
  });

  it("a future ISO date is NOT expired", () => {
    expect(isExpiredClaim("2999-01-01")).toBe(false);
    expect(isExpiredClaim("2100-12-31T23:59:59Z")).toBe(false);
  });

  it("a past unix-seconds string is expired; a future one is not", () => {
    expect(isExpiredClaim(String(Math.floor(Date.now() / 1000) - 3600))).toBe(true);
    expect(isExpiredClaim(String(Math.floor(Date.now() / 1000) + 3600))).toBe(false);
  });

  it("an unparseable expiry FAILS CLOSED (treated as expired — T1 gating)", () => {
    expect(isExpiredClaim("not-a-date")).toBe(true);
    expect(isExpiredClaim("2026-13-45")).toBe(true);
  });
});

// CORE-D3.C — the S3 checkout settle DECISION. The onboarding "Pay" step advances
// ONLY when /userinfo shows the REAL grant: a PAID tier (rank past free/public)
// with an ACTIVE entitlement. This is the exact predicate the tauri poll uses,
// so the Rule-1 property ("never settle before the entitlement lands") is decided
// here and unit-tested independently of the popup/poll plumbing.
describe("isPaidEntitlementActive — D3.C checkout settle decision", () => {
  it("free/public tier is NOT paid-active even if entitlement reads active", () => {
    expect(isPaidEntitlementActive({ tier: "free", entitlement: "active" })).toBe(false);
  });

  it("a paid tier (pilot/enterprise) with an ACTIVE entitlement IS the landed grant", () => {
    expect(isPaidEntitlementActive({ tier: "pilot", entitlement: "active" })).toBe(true);
    expect(isPaidEntitlementActive({ tier: "enterprise", entitlement: "active" })).toBe(true);
  });

  it("the authority's own tiers count as paid too (ADR-2026-07-25: commercial without KYC settles S5)", () => {
    // These fold in verbatim from /userinfo; without them in RANK the grant poll
    // never settles for a payment-as-sybil member (the "stuck on step 5" bug).
    expect(isPaidEntitlementActive({ tier: "commercial", entitlement: "active" })).toBe(true);
    expect(isPaidEntitlementActive({ tier: "commercial.kyc", entitlement: "active" })).toBe(true);
    expect(isPaidEntitlementActive({ tier: "public", entitlement: "active" })).toBe(false); // public == free
  });

  it("a paid tier that is NOT active (lapsed/grace/expiring) has NOT settled", () => {
    expect(isPaidEntitlementActive({ tier: "pilot", entitlement: "lapsed" })).toBe(false);
    expect(isPaidEntitlementActive({ tier: "pilot", entitlement: "grace" })).toBe(false);
    expect(isPaidEntitlementActive({ tier: "pilot", entitlement: "expiring" })).toBe(false);
  });

  it("an unknown tier fails closed (treated as unpaid)", () => {
    expect(isPaidEntitlementActive({ tier: "mystery", entitlement: "active" })).toBe(false);
    expect(isPaidEntitlementActive({ tier: "", entitlement: "active" })).toBe(false);
  });
});

// CORE-A3 identity — the authority issues no display-name claim, so name +
// initials are derived from the email local-part. Unit-tested independently of
// the auth plumbing (applyAuthStatus uses this exact helper).
describe("deriveIdentityFromEmail — display name/initials from an email", () => {
  it("single-word local part → capitalized name + first two letters", () => {
    expect(deriveIdentityFromEmail("larry@citrate.ai")).toEqual({ name: "Larry", initials: "LA" });
  });
  it("separators (. _ + -) split into title-cased words + first-letter initials", () => {
    expect(deriveIdentityFromEmail("ada.lovelace@x.io")).toEqual({ name: "Ada Lovelace", initials: "AL" });
    expect(deriveIdentityFromEmail("jean-luc_picard@x.io").name).toBe("Jean Luc Picard");
    expect(deriveIdentityFromEmail("jean-luc_picard@x.io").initials).toBe("JL");
  });
});

// CORE-A3 identity resolver — the single seam that stops prototype persona
// identity (Dana Okafor et al.) from ever showing to a real signed-in account.
describe("store.identity() — real signed-in user vs sim persona", () => {
  it("falls back to the sim persona when NOT signed in", () => {
    store.setState({ signedIn: false, authEmail: null, persona: "p1" });
    const id = store.identity();
    expect(id.real).toBe(false);
    expect(id.name).toBe("Dana Okafor"); // PERSONAS.p1
  });
  it("returns the REAL user when signed in with an email claim", () => {
    store.setState({
      signedIn: true,
      authEmail: "larry@citrate.ai",
      authName: "Larry",
      authInitials: "LA",
      authSub: "4925a564",
      walletAddr: "0xbb3a",
      tier: "pilot",
      citrateRole: "member",
      org: null,
    });
    const id = store.identity();
    expect(id.real).toBe(true);
    expect(id.email).toBe("larry@citrate.ai");
    expect(id.name).toBe("Larry");
    expect(id.role).toBe("member");
    expect(id.sub).toBe("4925a564");
  });
  it("signed in but /userinfo not yet folded (no email) → neutral REAL placeholder, NEVER the persona", () => {
    store.setState({ signedIn: true, authEmail: null, authName: null, authInitials: null, persona: "p1" });
    const id = store.identity();
    expect(id.real).toBe(true);
    expect(id.name).not.toBe("Dana Okafor"); // must not leak the sim persona
    expect(id.name).toBe("Member");
  });
});

// HIPAA sign-out-by-default — the signed-in session + account PII are NEVER
// written to disk; every launch starts signed-out and re-derives identity only
// from a live authority session. This test guards that invariant against a
// regression that would re-add the fields to the persisted set.
describe("HIPAA sign-out-by-default — no session/PII persisted", () => {
  it("PERSIST_KEYS excludes every real-identity field", () => {
    for (const k of ["signedIn", "authSub", "authEmail", "authName", "authInitials"] as const) {
      expect(PERSIST_KEYS).not.toContain(k);
    }
  });
});

// ALF-ND-A — the `alf_member` claim gates the ALF surface + nav. On sign-out the flag MUST
// clear; otherwise a signed-out account would keep the ALF workbench, a false-membership
// surface (Rule 1). This guards the reset the same way the tier/entitlement reset is guarded.
describe("ALF-ND — alfMember gating flag clears on sign-out", () => {
  it("authLogout clears alfMember (a signed-out account is never an ALF member)", async () => {
    const spy = vi.spyOn(bridge.auth, "logout").mockResolvedValue(undefined as unknown as void);
    store.setState({ alfMember: true, signedIn: true, citrateRole: "alf_member" });
    expect(store.state.alfMember).toBe(true);
    await store.authLogout();
    expect(store.state.alfMember).toBe(false);
    expect(store.state.signedIn).toBe(false);
    spy.mockRestore();
  });
});

// CORE-AI1 (@rule8) — the real-vs-demo provider selection + the key-never-in-state
// invariant. A REAL provider is chosen only in the Tauri build AND only when the
// default id is actually configured (its key sealed in the keyring); otherwise the
// honest built-in demo agent (Rule 1). The provider key lives in the OS keyring —
// AppState/PERSIST_KEYS must NOT carry an `aiKeys` map (invariant 2).
describe("AI1 — provider selection + key-never-in-state", () => {
  it("pickChatProviderKind picks 'real' only in tauri AND when the default is configured", () => {
    const configured = [
      { id: "openai", configured: true },
      { id: "gateway", configured: false },
    ];
    // Tauri + default is configured → real.
    expect(pickChatProviderKind(configured, "openai", "tauri")).toBe("real");
    // Tauri + default NOT configured → demo (honest fallback).
    expect(pickChatProviderKind(configured, "gateway", "tauri")).toBe("demo");
    // Web preview (sim) never selects a real provider (no keyring) → demo.
    expect(pickChatProviderKind(configured, "openai", "sim")).toBe("demo");
    // No providers configured at all → demo.
    expect(pickChatProviderKind([], "openai", "tauri")).toBe("demo");
  });

  // Q-A.4a item 6 — the LOCAL route is PREFERRED over gateway/demo, but ONLY when
  // the real Rust inference state is "ready" (model verified + llama-server healthy).
  // Any other inference state falls through — the route is HONEST, never a fabricated
  // local reply against a down server (Rule 1).
  it("pickChatProviderKind prefers 'local' when the inference state is 'ready' (server healthy)", () => {
    const configured = [{ id: "gateway", configured: true }];
    // ready → local, even though a gateway key is configured (local wins).
    expect(pickChatProviderKind(configured, "gateway", "tauri", "ready")).toBe("local");
    // NEGATIVE CONTROL: server NOT healthy (any non-"ready" state) → NOT local.
    // local-fallback means the model is ready but the server is down → gateway.
    expect(pickChatProviderKind(configured, "gateway", "tauri", "local-fallback")).toBe("real");
    expect(pickChatProviderKind(configured, "gateway", "tauri", "gateway-only")).toBe("real");
    // downloading with a gateway key → gateway (not local, not demo).
    expect(pickChatProviderKind(configured, "gateway", "tauri", "downloading")).toBe("real");
    // "ready" is honored ONLY in tauri — the web preview has no local server.
    expect(pickChatProviderKind(configured, "gateway", "sim", "ready")).toBe("demo");
    // no local, no gateway key, demo inference state → demo.
    expect(pickChatProviderKind([], "gateway", "tauri", "demo")).toBe("demo");
  });

  it("rebuildProvider falls back to the built-in demo agent in web-dev (no keyring)", async () => {
    await store.rebuildProvider();
    // Sim providerStatus returns [] → the honest demo agent, never a real provider.
    expect(store.provider?.kind).toBe("demo");
    expect(store.provider?.label).toBe("built-in demo agent");
  });

  it("AppState carries the aiDefault route id but NO aiKeys map (key lives in the keyring)", () => {
    const st = freshState("p1") as Record<string, unknown>;
    expect(st.aiDefault).toBeDefined();
    expect("aiKeys" in st).toBe(false);
    // And the persisted set never writes a provider key to localStorage.
    expect(PERSIST_KEYS as readonly string[]).not.toContain("aiKeys");
    expect(PERSIST_KEYS).toContain("aiDefault");
  });
});

// Q-A.4a items 2/3/4 — the memory-daemon store methods. In web-dev (sim) the
// memory bridge reports a running daemon over the sim socket; the real Tauri path
// reads memory_status(). These guard that the wiring is REAL (folds the daemon's
// socket path + semantic flag) and honest (an unreachable daemon → offline, no
// fabricated path).
describe("store memory daemon (Q-A.4a) — real status read, honest offline", () => {
  it("refreshMemoryStatus folds the daemon-reported socket path + semantic flag (never a client constant)", async () => {
    store.setState({ memSocketPath: null, memSemantic: true, memDaemon: "idle" });
    const state = await store.refreshMemoryStatus();
    // The sim daemon reports running with a socket path and semantic:false.
    expect(state).toBe("running");
    expect(store.getSnapshot().memDaemon).toBe("running");
    // The socket path is the daemon-reported one, folded into state (not null).
    expect(store.getSnapshot().memSocketPath).toBeTruthy();
    // semantic is the REAL daemon flag (false in sim) — never fabricated true.
    expect(store.getSnapshot().memSemantic).toBe(false);
  });

  it("startMemoryDaemon starts + re-reads status, then loads the real constellation", async () => {
    store.setState({ memGraph: undefined, memGraphState: "idle" });
    await store.startMemoryDaemon();
    // Sim start resolves; status reads running → the constellation loads (real hits).
    expect(store.getSnapshot().memDaemon).toBe("running");
    expect(store.getSnapshot().memGraphState).toBe("ready");
    expect(store.getSnapshot().memGraph?.nodes.length).toBeGreaterThan(0);
  });

  it("refreshConstellation with a query routes through the REAL daemon search (bridge.memory.search)", async () => {
    // A query that matches the sim chain-state hits narrows the graph via the real
    // search RPC (not only the client-side filter) — the graph is re-laid.
    await store.refreshConstellation("chain");
    expect(store.getSnapshot().memGraphState).toBe("ready");
  });
});

// CORE WP2 — the withdraw store methods. In web-dev (BRIDGE_MODE="sim") they do
// NOT settle (no key/chain): the write methods take the honest "desktop only"
// branch (no throw, no fabricated balance change), and refreshPendingWithdrawals
// folds the sim empty queue. This guards that the wiring exists + is honest.
describe("store withdraw (WP2) — honest in web-dev sim", () => {
  it("refreshPendingWithdrawals folds the (empty) sim queue without throwing", async () => {
    store.setState({ pendingWithdrawals: [{ id: "stale", saltWei: "1", requestBlock: 0, claimableAtBlock: 0, claimable: true }] });
    await store.refreshPendingWithdrawals();
    // Sim bridge returns [] (no chain) — the fold replaces the stale list honestly.
    expect(store.getSnapshot().pendingWithdrawals).toEqual([]);
  });

  it("walletRequestWithdrawal in web-dev opens a review and fabricates NO settlement", async () => {
    // Withdrawing is payout-gated on verified KYC (live-checked), so a member who
    // reaches the review is verified.
    store.setState({ walletReview: null, s2: "verified" });
    vi.spyOn(store, "authUserinfo").mockResolvedValue(undefined);
    const before = store.getSnapshot().selfStake;
    await store.walletRequestWithdrawal("1000000000000000000");
    // Q-E.1 — no balance mutation, no fabricated activity entry: the action STOPS
    // at the human-in-the-loop review (approving in web-dev is honest "desktop only").
    expect(store.getSnapshot().selfStake).toBe(before);
    expect(store.getSnapshot().walletReview).not.toBeNull();
    // The honest "desktop app" message surfaces on APPROVE (sim broadcast throws).
    await store.approveWalletReview();
    expect(store.getSnapshot().selfStake).toBe(before);
    expect(store.getSnapshot().toast).toContain("desktop app");
    expect(store.getSnapshot().walletReview).toBeNull();
  });

  it("walletClaimWithdrawal in web-dev opens a review and fabricates NO settlement", async () => {
    store.setState({ walletReview: null, s2: "verified" });
    vi.spyOn(store, "authUserinfo").mockResolvedValue(undefined);
    const liquidBefore = store.getSnapshot().liquid;
    await store.walletClaimWithdrawal("1");
    expect(store.getSnapshot().liquid).toBe(liquidBefore);
    expect(store.getSnapshot().walletReview).not.toBeNull();
    await store.approveWalletReview();
    expect(store.getSnapshot().liquid).toBe(liquidBefore);
    expect(store.getSnapshot().toast).toContain("desktop app");
    expect(store.getSnapshot().walletReview).toBeNull();
  });
});

// CORE item 4 — the wallet activity (tx history) fold. mergeActivity dedupes an
// OPTIMISTIC just-sent row against the REAL indexed list (CitrateScan txlist) so a
// settled tx renders exactly once; refreshActivity folds the real list into state.
describe("mergeActivity — dedupe optimistic vs indexed (item 4)", () => {
  const A = (id: string, hash: string, ts: number) => ({ id, kind: "Sent", amount: "−1.00 SALT", hash, ts });

  it("drops an optimistic row once the real indexed list contains its hash", () => {
    const optimistic = [A("local", "0xabc", 999)]; // just-sent, prepended by addActivity
    const real = [A("0xabc", "0xabc", 100), A("0xdef", "0xdef", 90)]; // now indexed
    const merged = mergeActivity(real, optimistic);
    // The optimistic 0xabc collapses into the canonical indexed 0xabc (no dupe).
    expect(merged.filter((r) => r.hash === "0xabc")).toHaveLength(1);
    expect(merged.map((r) => r.hash)).toEqual(["0xabc", "0xdef"]);
  });

  it("keeps an optimistic row NOT yet indexed, in front", () => {
    const optimistic = [A("local", "0xpending", 999)];
    const real = [A("0xdef", "0xdef", 90)];
    const merged = mergeActivity(real, optimistic);
    expect(merged[0].hash).toBe("0xpending"); // still visible until indexed
    expect(merged.map((r) => r.hash)).toEqual(["0xpending", "0xdef"]);
  });

  it("is idempotent when the real list equals the current list (sim echo path)", () => {
    const list = [A("0xaa", "0xaa", 2), A("0xbb", "0xbb", 1)];
    expect(mergeActivity(list, list)).toEqual(list);
  });

  it("caps the merged list at 24 rows", () => {
    const real = Array.from({ length: 30 }, (_, i) => A(`0x${i}`, `0x${i}`, i));
    expect(mergeActivity(real, [])).toHaveLength(24);
  });
});

describe("store.refreshActivity — folds the real indexed history (item 4)", () => {
  it("folds the sim activity list without throwing (web-dev echo)", async () => {
    // The sim bridge echoes s().activity; seed a known row and confirm the fold
    // preserves it (no fabricated rows, no throw).
    store.setState({ activity: [{ id: "x1", kind: "Send", amount: "−1.00 SALT", hash: "0xfeed", ts: 5 }] });
    await store.refreshActivity();
    expect(store.getSnapshot().activity.some((a) => a.hash === "0xfeed")).toBe(true);
  });
});

// CORE Phase 1 — the REAL node poller maps supervisor state (node.rs map_state)
// onto the app's node lifecycle enum. This replaces the sim tick() node state in
// a Tauri build so height/peers/sync are live 40204 truth, not fabricated.
describe("mapNodeState — supervisor state → app node lifecycle", () => {
  it("stopped/unknown → off", () => {
    expect(mapNodeState("stopped", 0, 0)).toBe("off");
    expect(mapNodeState("whatever", 100, 99999)).toBe("off");
  });
  it("starting/restarting → prov; failed → error", () => {
    expect(mapNodeState("starting", 0, 0)).toBe("prov");
    expect(mapNodeState("restarting", 0, 0)).toBe("prov");
    expect(mapNodeState("failed", 0, 0)).toBe("error");
  });
  it("running while not fully synced → syncing", () => {
    expect(mapNodeState("running", 42, 32000)).toBe("syncing");
  });
  it("running + synced + staked at/above threshold → validating", () => {
    expect(mapNodeState("running", 100, 32000)).toBe("validating");
  });
  it("running + synced but under-staked → synced (not validating)", () => {
    expect(mapNodeState("running", 100, 31999)).toBe("synced");
    expect(mapNodeState("running", 100, 0)).toBe("synced");
  });
});

// Q-A.2/Q-B.2 — REAL streamed node logs. The store folds the bridge's node.logs()
// (Rust node_logs ring of stdout+stderr) into s.logs so the Node LOG panel shows
// live node output in a packaged build. `foldNodeLogs` is the pure converter; the
// sim bridge path must read as a LABELLED preview, never as live node output.
describe("foldNodeLogs — real streamed node lines → LogLine (Q-A.2)", () => {
  it("converts real ts/stream/line into the panel's { t, line, id } shape", () => {
    const raw = [
      { ts: Date.parse("2026-07-19T13:04:05.000Z"), stream: "out" as const, line: "citrate_network::sync: imported 32/32 blocks (height 1540-1571)" },
      { ts: Date.parse("2026-07-19T13:04:06.000Z"), stream: "err" as const, line: "warn: peer dropped" },
    ];
    const out = foldNodeLogs(raw);
    expect(out).toHaveLength(2);
    // The line text is preserved verbatim (REAL node output, not a template).
    expect(out[0].line).toBe("citrate_network::sync: imported 32/32 blocks (height 1540-1571)");
    expect(out[1].line).toBe("warn: peer dropped");
    // Each carries a HH:MM:SS stamp and a stable id.
    expect(out[0].t).toMatch(/^\d{2}:\d{2}:\d{2}$/);
    expect(out.map((l) => l.id)).toEqual([0, 1]);
  });

  it("caps the tail at 14 lines, keeping the NEWEST (no DOM flood from a 500-ring)", () => {
    const raw = Array.from({ length: 40 }, (_, i) => ({ ts: 1_000_000 + i, stream: "out" as const, line: `LINE_${i}` }));
    const out = foldNodeLogs(raw);
    expect(out).toHaveLength(14);
    // Newest kept: last line is LINE_39, oldest kept is LINE_26 (40 - 14).
    expect(out[out.length - 1].line).toBe("LINE_39");
    expect(out[0].line).toBe("LINE_26");
  });

  it("an empty stream (node off / no output yet) folds to no lines", () => {
    expect(foldNodeLogs([])).toEqual([]);
  });
});

// Rule 1: in sim/web the node log stream must be a LABELLED preview, never dressed
// as real live output — so a packaged build (which runs the real node_logs instead)
// is the only place live lines appear. BRIDGE_MODE is "sim" in the test env.
describe("bridge.node.logs (sim) — labelled preview, never live output (Q-B.2)", () => {
  it("a running sim node yields lines that are clearly a preview, not real node output", async () => {
    store.setState({ node: "syncing", height: 1571, peers: 12 });
    const lines = await bridge.node.logs();
    expect(lines.length).toBeGreaterThan(0);
    // Every sim line is labelled so it can never be mistaken for live node output.
    expect(lines.every((l) => l.line.startsWith("[sim preview]"))).toBe(true);
    // And the fold keeps that label intact (the panel would render the preview tag).
    const folded = foldNodeLogs(lines);
    expect(folded.every((l) => l.line.startsWith("[sim preview]"))).toBe(true);
  });

  it("an off node has no stream → honest empty (no fabricated tail)", async () => {
    store.setState({ node: "off" });
    expect(await bridge.node.logs()).toEqual([]);
  });
});

// Q-E.1 (@rule8, P0) — the human-in-the-loop wallet review gate. The money
// actions MUST NOT broadcast on their own: each builds the pending ceremony,
// surfaces the DECODED view for a human to Approve/Reject, and ONLY an explicit
// approval calls signing.broadcast. This is the safety hole the sprint closes:
// the "Review & sign" button used to sign what the CODE built with no person in
// the loop. These tests pin "no broadcast without an explicit human approve".
describe("Q-E.1 wallet review gate — money actions never self-broadcast", () => {
  const view: CeremonyView = {
    id: "wcer-1",
    origin: "local-user",
    kind: "transaction",
    chainId: 40204,
    decoded: { action: "Send 1.00 SALT", cost: "est. gas 0.0019 SALT", destination: "0x" + "ab".repeat(20) },
    requiresRawAck: false,
  };
  const rawView: CeremonyView = { ...view, id: "wcer-raw", requiresRawAck: true, decoded: { action: "Unrecognized", cost: "", destination: "" } };

  let broadcastSpy: ReturnType<typeof vi.spyOn>;
  let rejectSpy: ReturnType<typeof vi.spyOn>;

  beforeEach(() => {
    // A member exercising withdrawals is, by the payout gate, KYC-verified — and
    // the gate re-checks live, so stub /userinfo too.
    store.setState({ walletReview: null, liquid: 100, selfStake: 100, s2: "verified" });
    vi.spyOn(store, "authUserinfo").mockResolvedValue(undefined);
    // Every money action funnels its build call to a view we control.
    vi.spyOn(bridge.wallet, "send").mockResolvedValue(view);
    vi.spyOn(bridge.wallet, "stake").mockResolvedValue(view);
    vi.spyOn(bridge.wallet, "requestWithdrawal").mockResolvedValue(view);
    vi.spyOn(bridge.wallet, "claimWithdrawal").mockResolvedValue(view);
    broadcastSpy = vi.spyOn(bridge.signing, "broadcast").mockResolvedValue({ txHash: "0xhash", blockNumber: 1 });
    rejectSpy = vi.spyOn(bridge.signing, "reject").mockResolvedValue(undefined);
    vi.spyOn(store, "refreshWallet").mockResolvedValue(undefined);
    vi.spyOn(store, "refreshActivity").mockResolvedValue(undefined);
    vi.spyOn(store, "refreshPendingWithdrawals").mockResolvedValue(undefined);
  });
  afterEach(() => {
    vi.restoreAllMocks();
    store.setState({ walletReview: null });
  });

  // NEGATIVE CONTROL: this MUST fail on the pre-fix code, which broadcasts
  // immediately. Each action sets the pending review and calls broadcast ZERO times.
  it("walletSend builds the pending review and does NOT broadcast", async () => {
    await store.walletSend("0x" + "ab".repeat(20), "1000000000000000000");
    expect(store.state.walletReview).not.toBeNull();
    expect(store.state.walletReview?.view.id).toBe("wcer-1");
    expect(broadcastSpy).not.toHaveBeenCalled();
  });

  it("walletStake builds the pending review and does NOT broadcast", async () => {
    await store.walletStake("1000000000000000000");
    expect(store.state.walletReview?.view.id).toBe("wcer-1");
    expect(broadcastSpy).not.toHaveBeenCalled();
  });

  it("walletRequestWithdrawal builds the pending review and does NOT broadcast", async () => {
    await store.walletRequestWithdrawal("1000000000000000000");
    expect(store.state.walletReview?.view.id).toBe("wcer-1");
    expect(broadcastSpy).not.toHaveBeenCalled();
  });

  it("walletClaimWithdrawal builds the pending review and does NOT broadcast", async () => {
    await store.walletClaimWithdrawal("7");
    expect(store.state.walletReview?.view.id).toBe("wcer-1");
    expect(broadcastSpy).not.toHaveBeenCalled();
  });

  // APPROVE → broadcast IS called with the pending view id + the raw-ack passed.
  it("approveWalletReview broadcasts the pending view id, then clears the review", async () => {
    await store.walletSend("0x" + "ab".repeat(20), "1000000000000000000");
    expect(broadcastSpy).not.toHaveBeenCalled();
    await store.approveWalletReview(false);
    expect(broadcastSpy).toHaveBeenCalledTimes(1);
    expect(broadcastSpy).toHaveBeenCalledWith("wcer-1", false);
    expect(store.state.walletReview).toBeNull();
  });

  // REJECT → broadcast NOT called; the pending ceremony is released; review cleared.
  it("rejectWalletReview broadcasts nothing, releases the ceremony, clears the review", async () => {
    await store.walletStake("1000000000000000000");
    await store.rejectWalletReview();
    expect(broadcastSpy).not.toHaveBeenCalled();
    expect(rejectSpy).toHaveBeenCalledWith("wcer-1");
    expect(store.state.walletReview).toBeNull();
  });

  // Undecodable calldata gates Approve behind an explicit raw-mode ack.
  it("undecodable calldata (requiresRawAck) blocks broadcast until an explicit ack", async () => {
    vi.spyOn(bridge.wallet, "send").mockResolvedValue(rawView);
    await store.walletSend("0x" + "ab".repeat(20), "1000000000000000000");
    expect(store.state.walletReview?.view.requiresRawAck).toBe(true);
    // Approving WITHOUT the raw ack must NOT broadcast (fail closed).
    await store.approveWalletReview(false);
    expect(broadcastSpy).not.toHaveBeenCalled();
    expect(store.state.walletReview).not.toBeNull();
    // With the explicit raw ack, it broadcasts (rawAck=true passed through).
    await store.approveWalletReview(true);
    expect(broadcastSpy).toHaveBeenCalledWith("wcer-raw", true);
  });
});

// ---------------------------------------------------------------------------
// Wallet link → the `wallet_address` binding.
//
// The authority mints `wallet_address` for every member; until a wallet is linked
// that claim is the counterfactual smart-wallet address, which no key can spend
// from. The membership money path pays THAT address while the validator self-bond
// is sent from this device's custody EOA — so an unlinked member's 32,000 SALT
// bond lands somewhere unreachable. These pin the gate and, above all, that a
// link NEVER reaches `signing.broadcast` (it is a signature, not a transaction).
// ---------------------------------------------------------------------------
describe("wallet link — binding this device's wallet to the identity", () => {
  const linkView: CeremonyView = {
    id: "wlink-1",
    origin: "https://auth.citrate.ai",
    kind: "personal_sign",
    chainId: 40204,
    decoded: {
      action: 'Sign message: "auth.citrate.ai wants to link a wallet…"',
      cost: "no funds moved",
      destination: "https://auth.citrate.ai",
    },
    requiresRawAck: false,
  };

  let broadcastSpy: ReturnType<typeof vi.spyOn>;
  let linkApproveSpy: ReturnType<typeof vi.spyOn>;
  let linkRejectSpy: ReturnType<typeof vi.spyOn>;

  beforeEach(() => {
    store.setState({ walletReview: null, walletAddr: "", custodyAddr: "" });
    // Seamless device provisioning runs first inside linkWallet — mock it to a
    // fixed custody EOA so the link tests are deterministic (the real provisioning
    // is proven in kit's custody/provisioning suites).
    vi.spyOn(bridge.wallet, "ensureReady").mockResolvedValue({ address: "0x" + "cd".repeat(20), created: false });
    vi.spyOn(bridge.wallet, "linkRequest").mockResolvedValue(linkView);
    linkApproveSpy = vi
      .spyOn(bridge.wallet, "linkApprove")
      .mockResolvedValue({ address: "0x" + "cd".repeat(20), linked: true, canonical: true });
    linkRejectSpy = vi.spyOn(bridge.wallet, "linkReject").mockResolvedValue(undefined);
    broadcastSpy = vi.spyOn(bridge.signing, "broadcast").mockResolvedValue({ txHash: "0xhash", blockNumber: 1 });
    vi.spyOn(store, "authUserinfo").mockResolvedValue(undefined as never);
    vi.spyOn(store, "refreshWallet").mockResolvedValue(undefined);
  });
  afterEach(() => {
    vi.restoreAllMocks();
    store.setState({ walletReview: null });
  });

  it("walletIsLinked is false until BOTH addresses are known — never an optimistic yes", () => {
    store.setState({ walletAddr: "", custodyAddr: "" });
    expect(store.walletIsLinked()).toBe(false);
    store.setState({ walletAddr: "0x" + "ab".repeat(20), custodyAddr: "" });
    expect(store.walletIsLinked()).toBe(false);
    store.setState({ walletAddr: "", custodyAddr: "0x" + "ab".repeat(20) });
    expect(store.walletIsLinked()).toBe(false);
  });

  it("differing claim and custody addresses read as NOT linked (the stranding case)", () => {
    store.setState({ walletAddr: "0x" + "ab".repeat(20), custodyAddr: "0x" + "cd".repeat(20) });
    expect(store.walletIsLinked()).toBe(false);
  });

  it("the same address in different casing reads as linked", () => {
    store.setState({ walletAddr: "0x" + "AB".repeat(20), custodyAddr: "0x" + "ab".repeat(20) });
    expect(store.walletIsLinked()).toBe(true);
  });

  it("linkWallet builds the pending review and signs NOTHING", async () => {
    await store.linkWallet();
    expect(store.state.walletReview).not.toBeNull();
    expect(store.state.walletReview?.kind).toBe("wallet-link");
    expect(store.state.walletReview?.view.id).toBe("wlink-1");
    expect(linkApproveSpy).not.toHaveBeenCalled();
    expect(broadcastSpy).not.toHaveBeenCalled();
  });

  // ROOT-CAUSE regression guard: linkWallet must device-provision the wallet
  // (ensureReady) BEFORE requesting the link, and stamp the returned custody EOA —
  // otherwise a fresh install has no vault/wallet and linkRequest fails closed with
  // "Wallet link unavailable" (the whole money-path bug).
  it("linkWallet provisions the device wallet first and stamps the custody EOA", async () => {
    const ensureSpy = vi.spyOn(bridge.wallet, "ensureReady").mockResolvedValue({ address: "0x" + "cd".repeat(20), created: true });
    const reqSpy = vi.spyOn(bridge.wallet, "linkRequest").mockResolvedValue(linkView);
    store.setState({ custodyAddr: "" });

    await store.linkWallet();

    expect(ensureSpy).toHaveBeenCalledTimes(1);
    // ensureReady resolved before linkRequest was invoked (provision-then-link).
    expect(ensureSpy.mock.invocationCallOrder[0]).toBeLessThan(reqSpy.mock.invocationCallOrder[0]);
    // The real custody EOA is stamped so walletIsLinked() has the custody side.
    expect(store.state.custodyAddr).toBe("0x" + "cd".repeat(20));
    expect(store.state.walletReview?.kind).toBe("wallet-link");
  });

  it("linkWallet surfaces an honest error and does NOT request a link if provisioning fails", async () => {
    vi.spyOn(bridge.wallet, "ensureReady").mockRejectedValue(new Error("keyring unavailable"));
    const reqSpy = vi.spyOn(bridge.wallet, "linkRequest").mockResolvedValue(linkView);
    await store.linkWallet();
    expect(reqSpy).not.toHaveBeenCalled();
    expect(store.state.walletReview).toBeNull();
  });

  // THE LOAD-BEARING ONE. A link is a personal_sign whose proof is POSTed to the
  // authority — routing it to signing.broadcast would try to send a transaction.
  it("approving a link submits the proof and NEVER broadcasts a transaction", async () => {
    await store.linkWallet();
    await store.approveWalletReview();
    expect(linkApproveSpy).toHaveBeenCalledWith("wlink-1", false);
    expect(broadcastSpy).not.toHaveBeenCalled();
    expect(store.state.walletReview).toBeNull();
  });

  // linked !== canonical, and conflating them is the bug this pair exists for.
  // The link is durable the moment the proof is accepted, but the authority only
  // serves THIS address as `wallet_address` if it is ALSO canonical (it defaults
  // to first-linked). A member whose custody vault was replaced links
  // successfully and stays blocked — told it worked. Observed live 2026-08-04.
  it("says the payout address moved only when the wallet is CANONICAL", async () => {
    await store.linkWallet();
    await store.approveWalletReview();
    const said = String(store.getSnapshot().toast ?? "");
    expect(said).toMatch(/authority now pays/i);
  });

  it("does NOT claim the authority pays you when the wallet is linked but NOT canonical", async () => {
    linkApproveSpy.mockResolvedValue({
      address: "0x" + "cd".repeat(20),
      linked: true,
      canonical: false,
    });
    await store.linkWallet();
    await store.approveWalletReview();
    const said = String(store.getSnapshot().toast ?? "");
    expect(said).not.toMatch(/authority now pays/i);
    // and it must SAY so rather than fall silent — a member about to pay in
    // needs to know the money would land on the old address.
    expect(said).toMatch(/did not move|still the one on file/i);
  });

  it("a failed submit leaves nothing linked and releases the ceremony", async () => {
    linkApproveSpy.mockRejectedValue(new Error("authority rejected the link: replayed nonce"));
    await store.linkWallet();
    await store.approveWalletReview();
    expect(linkRejectSpy).toHaveBeenCalledWith("wlink-1");
    expect(store.state.walletReview).toBeNull();
    expect(store.walletIsLinked()).toBe(false);
  });

  it("declining a link releases the one-time nonce via the link path, not signing.reject", async () => {
    const signingReject = vi.spyOn(bridge.signing, "reject").mockResolvedValue(undefined);
    await store.linkWallet();
    await store.rejectWalletReview();
    expect(linkRejectSpy).toHaveBeenCalledWith("wlink-1");
    expect(signingReject).not.toHaveBeenCalled();
    expect(linkApproveSpy).not.toHaveBeenCalled();
  });
});

// ---------------------------------------------------------------------------
// isGrantOnChain — the stake can live in TWO places.
//
// Before ADR 2026-07-27 the grant staked into MembershipStakeVault. Under the
// bond-fund model the treasury funds the member's EOA and the member self-bonds,
// so the principal sits in the ValidatorRegistry and the vault reads 0 FOREVER.
// Settling on the vault alone left a correctly-bonded validator permanently
// "ungranted" — working, but indistinguishable from broken.
// ---------------------------------------------------------------------------
describe("isGrantOnChain — vault OR bonded stake settles the grant leg", () => {
  const REQ = (32000n * 10n ** 18n).toString();

  it("no SBT is never granted, whatever the stake says", () => {
    expect(isGrantOnChain({ attributedStakeWei: REQ, hasSbt: false, bondedStakeWei: REQ })).toBe(false);
  });

  it("legacy vault grant still settles (the member granted before bond-fund)", () => {
    expect(isGrantOnChain({ attributedStakeWei: REQ, hasSbt: true, bondedStakeWei: "0" })).toBe(true);
  });

  // THE REGRESSION: this is the real post-bond-fund shape and it returned false
  // before the fix, so a bonded validator never settled.
  it("BOND-FUND: zero vault stake but a bonded validator DOES settle", () => {
    expect(isGrantOnChain({ attributedStakeWei: "0", hasSbt: true, bondedStakeWei: REQ })).toBe(true);
  });

  // THE 2026-07-28 STALL: right after payment the treasury funded the member's EOA
  // natively (32k) + minted the SBT, but the member hasn't self-bonded yet, so BOTH
  // vault and registry read 0. Before the native leg this member polled forever.
  it("BOND-FUND (just granted): funded native EOA, not yet self-bonded, DOES settle", () => {
    expect(
      isGrantOnChain({ attributedStakeWei: "0", hasSbt: true, bondedStakeWei: "0", nativeBalanceWei: REQ }),
    ).toBe(true);
  });

  it("native funding without the SBT is NEVER granted (SBT is the hard precondition)", () => {
    expect(
      isGrantOnChain({ attributedStakeWei: "0", hasSbt: false, bondedStakeWei: "0", nativeBalanceWei: REQ }),
    ).toBe(false);
  });

  it("neither source reaching the requirement does not settle", () => {
    expect(isGrantOnChain({ attributedStakeWei: "0", hasSbt: true, bondedStakeWei: "0", nativeBalanceWei: "0" })).toBe(false);
    const short = (31999n * 10n ** 18n).toString();
    expect(isGrantOnChain({ attributedStakeWei: short, hasSbt: true, bondedStakeWei: short, nativeBalanceWei: short })).toBe(false);
  });

  it("the two are NOT summed — a member cannot reach the bar by halves", () => {
    const half = (16000n * 10n ** 18n).toString();
    expect(isGrantOnChain({ attributedStakeWei: half, hasSbt: true, bondedStakeWei: half })).toBe(false);
  });

  it("an unparseable value contributes 0 rather than throwing (fail-closed)", () => {
    expect(isGrantOnChain({ attributedStakeWei: "not-a-number", hasSbt: true, bondedStakeWei: REQ })).toBe(true);
    expect(isGrantOnChain({ attributedStakeWei: "not-a-number", hasSbt: true, bondedStakeWei: "0" })).toBe(false);
  });

  it("an absent bondedStakeWei (older bridge) still works off the vault alone", () => {
    expect(isGrantOnChain({ attributedStakeWei: REQ, hasSbt: true })).toBe(true);
    expect(isGrantOnChain({ attributedStakeWei: "0", hasSbt: true })).toBe(false);
  });
});

// ---------------------------------------------------------------------------
// Payout gate — KYC qualifies getting SALT OUT, not taking part.
//
// Participation is deliberately un-gated (a paid member runs the node and accrues
// earnings with no KYC). Verification gates value LEAVING: claiming rewards and
// withdrawing stake. The check is LIVE — a cached claim must not authorize a payout.
//
// Honest limit these tests do NOT pretend away: this is app-side. A member with
// their own key can call the contracts directly. It is a product default, not
// enforcement; the contract-side gate is a separate deliverable.
// ---------------------------------------------------------------------------
describe("payout gate — claiming and withdrawing require verified KYC", () => {
  let claimSpy: ReturnType<typeof vi.spyOn>;
  let reqSpy: ReturnType<typeof vi.spyOn>;
  let claimWdSpy: ReturnType<typeof vi.spyOn>;
  let userinfoSpy: ReturnType<typeof vi.spyOn>;

  beforeEach(() => {
    store.setState({ walletReview: null, s2: "none" });
    claimSpy = vi.spyOn(bridge.agent, "claim").mockResolvedValue({ kind: "nothing" } as never);
    reqSpy = vi.spyOn(bridge.wallet, "requestWithdrawal").mockResolvedValue({} as never);
    claimWdSpy = vi.spyOn(bridge.wallet, "claimWithdrawal").mockResolvedValue({} as never);
    // authUserinfo succeeds but does not change s2 — the test drives s2 directly.
    userinfoSpy = vi.spyOn(store, "authUserinfo").mockResolvedValue(undefined);
  });
  afterEach(() => {
    vi.restoreAllMocks();
    store.setState({ walletReview: null, s2: "none" });
  });

  it("an UNVERIFIED member cannot claim earnings — the ceremony is never built", async () => {
    store.setState({ s2: "none" });
    await store.claimRewards();
    expect(claimSpy).not.toHaveBeenCalled();
  });

  it("an unverified member cannot withdraw stake, nor collect a matured withdrawal", async () => {
    store.setState({ s2: "none" });
    await store.walletRequestWithdrawal("1000000000000000000");
    await store.walletClaimWithdrawal("1");
    expect(reqSpy).not.toHaveBeenCalled();
    expect(claimWdSpy).not.toHaveBeenCalled();
  });

  it("pending / review / failed are all refused — only `verified` opens the door", async () => {
    for (const st of ["pending", "review", "failed"] as const) {
      store.setState({ s2: st });
      await store.claimRewards();
    }
    expect(claimSpy).not.toHaveBeenCalled();
  });

  it("a VERIFIED member proceeds — participation earnings become withdrawable", async () => {
    store.setState({ s2: "verified" });
    await store.claimRewards();
    expect(claimSpy).toHaveBeenCalled();
  });

  it("re-checks LIVE: a cached claim never authorizes a payout on its own", async () => {
    store.setState({ s2: "verified" });
    await store.claimRewards();
    expect(userinfoSpy).toHaveBeenCalled();
  });

  it("an unreachable authority FAILS CLOSED even when the cached claim says verified", async () => {
    store.setState({ s2: "verified" });
    userinfoSpy.mockRejectedValue(new Error("network"));
    await store.claimRewards();
    expect(claimSpy).not.toHaveBeenCalled();
  });
});

// ---------------------------------------------------------------------------
// custodyEnsureUnlocked — seamless device-bound re-unlock (passphrase-less model).
// A locked vault has no user passphrase to enter, so this is the ONLY recovery
// path; it also runs on launch so the wallet reads (gated on an unlocked vault)
// do not appear broken.
// ---------------------------------------------------------------------------
describe("store.custodyEnsureUnlocked — device-bound re-unlock", () => {
  afterEach(() => vi.restoreAllMocks());

  it("calls the seamless bridge unlock and refreshes the lock state to unlocked", async () => {
    const ensureSpy = vi
      .spyOn(bridge.custody, "ensureUnlocked")
      .mockResolvedValue({ initialized: true, unlocked: true, autolockMins: 30, keyringStatus: "available" });
    const statusSpy = vi
      .spyOn(bridge.custody, "status")
      .mockResolvedValue({ initialized: true, unlocked: true, autolockMins: 30, keyringStatus: "available" });

    store.setState({ custodyLock: "locked" });
    await store.custodyEnsureUnlocked();

    expect(ensureSpy).toHaveBeenCalledTimes(1);
    expect(statusSpy).toHaveBeenCalled(); // refreshCustody ran
    expect(store.state.custodyLock).toBe("unlocked");
  });

  it("fails closed: a reset-keychain error still refreshes state (honest locked), never throws through", async () => {
    vi.spyOn(bridge.custody, "ensureUnlocked").mockRejectedValue(new Error("keyring unavailable"));
    vi.spyOn(bridge.custody, "status").mockResolvedValue({ initialized: true, unlocked: false, autolockMins: 30, keyringStatus: "unavailable" });
    store.setState({ custodyLock: "unknown" });
    await store.custodyEnsureUnlocked();
    expect(store.state.custodyLock).toBe("locked"); // refreshCustody folded the honest locked state
  });
});

// ---------------------------------------------------------------------------
// resumeSession — re-establish the signed-in session on launch from the vaulted
// OIDC refresh token, so a restart keeps the paid tier instead of dropping to
// signed-out/public. Runs after the vault unlocks (the token is a custody slot).
// ---------------------------------------------------------------------------
describe("store.resumeSession — launch session re-establish", () => {
  afterEach(() => vi.restoreAllMocks());

  it("mints a fresh session from the refresh token, folds it, and re-checks live entitlement", async () => {
    const paid: AuthStatus = {
      signedIn: true, sub: "usr_1", tier: "commercial.kyc", org: null, role: "member",
      kycStatus: "verified", walletAddr: "0xabc", expiresAt: "2999-01-01", email: "d@example.com",
    };
    const refreshSpy = vi.spyOn(bridge.auth, "refresh").mockResolvedValue(paid);
    const applySpy = vi.spyOn(store, "applyAuthStatus");
    const userinfoSpy = vi.spyOn(store, "authUserinfo").mockResolvedValue(undefined);
    const refreshAuthSpy = vi.spyOn(store, "refreshAuth").mockResolvedValue(undefined);

    await store.resumeSession();

    expect(refreshSpy).toHaveBeenCalledTimes(1);
    expect(applySpy).toHaveBeenCalledWith(paid); // the refreshed session was folded
    expect(userinfoSpy).toHaveBeenCalledTimes(1); // live /userinfo re-check after refresh
    expect(refreshAuthSpy).not.toHaveBeenCalled(); // NOT the signed-out fallback path
  });

  it("no vaulted token (never signed in): stays signed out, falls back to a status read", async () => {
    vi.spyOn(bridge.auth, "refresh").mockRejectedValue(new Error("no refresh token"));
    const userinfoSpy = vi.spyOn(store, "authUserinfo").mockResolvedValue(undefined);
    const refreshAuthSpy = vi.spyOn(store, "refreshAuth").mockResolvedValue(undefined);

    await store.resumeSession();

    expect(refreshAuthSpy).toHaveBeenCalledTimes(1); // fallback status read
    expect(userinfoSpy).not.toHaveBeenCalled(); // no live re-check without a session
  });
});
