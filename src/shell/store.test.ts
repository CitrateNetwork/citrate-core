// CORE-A3 A3-03 — entitlement-expiry enforcement. The `expiresAt` claim gates
// whether a signed-in session keeps its tier: a past (or anomalous) expiry
// downgrades to free/lapsed at the DECISION point, not just in the UI. This is
// the frontend half of the entitlement engine; the Rust id_token `exp` guard is
// the hard backstop (oidc::tests).
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { isExpiredClaim, isPaidEntitlementActive, deriveIdentityFromEmail, mapNodeState, mergeActivity, pickChatProviderKind, foldNodeLogs, store } from "./store";
import { PERSIST_KEYS, freshState } from "./state";
import { bridge } from "../bridge";
import type { CeremonyView } from "../bridge/types";

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
    store.setState({ walletReview: null });
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
    store.setState({ walletReview: null });
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
    store.setState({ walletReview: null, liquid: 100, selfStake: 100 });
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
