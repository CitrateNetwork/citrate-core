// L0-WP07f: the onboarding transitions (S1 to S7 and entering the app: join, explore, the free path,
// the enterprise lead, KYC, the S3 checkout with its membership and signer-health polls, the S5
// grant poll) and the prototype demo panel (persona switch, reset), moved out of the Store class
// body into the L3 membership and onboarding lane. Each Store method of the same name is a one-line
// delegate to the function here, so every call site (`store.onS3Pay()`) is unchanged. The code is
// moved as it was: `this` became `store`, and the Store's private members are reached through
// storeInternals(store).
//
// Data sources (Rule 7): AppState (`stage`, `s1` to `s7`, the checkout and grant fields), the
// identity authority's claim (bridge.auth, refreshed through the Store), the membership checkout and
// signer health (bridge.membership: core-membership), the on-chain grant (bridge.membership
// .grantStatus, chain 40204) and, for the demo panel, the prototype personas and localStorage key
// (state.ts STORAGE_KEY). Unchanged.

import { bridge } from "../../../bridge";
import type { GrantStatus } from "../../../bridge/domains";
import { BRIDGE_MODE } from "../../../bridge/mode";
import { isGrantDelayed } from "../../../onboarding/grantDelay";
import { describeBond, isGrantOnChain } from "../../grantReads";
import { freshState, greeting, STORAGE_KEY } from "../../state";
import type { Store } from "../../store";
import { storeInternals } from "../../storeInternals";

export function onJoin(store: Store): void {
  store.setState({ stage: "s1" });
  store.save();
}

export function onExplore(store: Store): void {
  store.setState({ stage: "done", tier: "free", coachDone: true, chatMsgs: [greeting({ ...store.persona(), name: store.identity().real ? store.identity().name : "" })] });
  store.save();
}

/** S3 — "Run a node for free" (owner decision 2026-10-27, option a: the free path comes first).
 *  Continues onboarding on the Free tier WITHOUT a paid membership (no SBT, no grant, no bond):
 *  S4 wallet → S6 node ignition (S4 skips the grant step when there is no membership), then the
 *  app via onS6Continue → onEnter. No checkout, no payment, no fabricated grant (Rule 1). The
 *  paid membership stays available later (Settings → Billing opens the same checkout). */
export function onS3Free(store: Store): void {
  store.endS3Check();
  store.setState({ stage: "s4", tier: "free" });
  store.save();
}

/** Enterprise · Contact us — submit a qualified sales lead to core-membership. Returns `{ ok }`, or
 *  `{ ok: false, error }` with the backend's honest message (Rule 1 — never a fabricated "received").
 *  No money, no grant; the record just prepares sales for the call. */
export async function submitEnterpriseLead(
  _store: Store,
  lead: {
    org: string;
    email: string;
    contact?: string;
    seats?: string;
    workload?: string;
    timeline?: string;
    notes?: string;
  },
): Promise<{ ok: boolean; error?: string }> {
  try {
    await bridge.membership.enterpriseLead(lead);
    return { ok: true };
  } catch (err) {
    return { ok: false, error: String((err as Error)?.message ?? err) };
  }
}

export function onS1Start(store: Store): void {
  const internals = storeInternals(store);
  store.setState({ s1: "waiting" });
  // Tauri: drive the REAL loopback-PKCE sign-in. The system browser opens; on
  // success the entitlement claim folds in and S1 completes. A failure/cancel
  // returns to idle (honest — no faked success).
  if (BRIDGE_MODE === "tauri") {
    void store.authLogin()
      .then(() => {
        store.setState({ s1: "done" });
        store.save();
      })
      .catch(() => {
        store.setState({ s1: "idle" });
      });
    return;
  }
  // Web-dev sim: the prototype attestation animation (no real auth).
  internals._s1t = setTimeout(() => {
    store.setState({ s1: "attest", s1c: 0 });
    setTimeout(() => store.setState({ s1c: 1 }), 700);
    setTimeout(() => store.setState({ s1c: 2 }), 1500);
    setTimeout(() => store.setState({ s1c: 3 }), 2300);
    setTimeout(() => {
      store.setState({ s1: "done" });
      store.save();
    }, 2800);
  }, 2300);
}

export function onS1Cancel(store: Store): void {
  const internals = storeInternals(store);
  if (internals._s1t) clearTimeout(internals._s1t);
  // rc.5 FIX 2: the sign-in runs in the system browser, so closing the tab sends
  // nothing back. Cancel the Rust loopback wait too, so a late browser completion
  // cannot sign the member in behind a screen that says cancelled.
  if (BRIDGE_MODE === "tauri") void store.cancelSignIn();
  store.setState({ s1: "idle" });
}

export function onS2Start(store: Store): void {
  const internals = storeInternals(store);
  store.setState({ s2: "pending" });
  // Tauri: open the REAL KYC flow in the browser, then poll /userinfo for the
  // kyc_status claim change (none→pending→verified/failed/review). The S2 UI
  // drives entirely off the real claim.
  if (BRIDGE_MODE === "tauri") {
    void store.kycStart().catch(() => {
      /* browser open failed — stay pending; the user can retry */
    });
    internals.pollKyc();
    return;
  }
  // Web-dev sim: resolve to the persona's scripted outcome after a beat.
  setTimeout(() => {
    store.setState({ s2: store.state.kycOutcome });
    store.save();
  }, 2600);
}

/**
 * Poll the live /userinfo entitlement for a KYC status change while S2 is
 * pending (Tauri only). Stops once the claim resolves to a terminal state.
 */
export function pollKyc(store: Store): void {
  if (BRIDGE_MODE !== "tauri") return;
  const tick = () => {
    if (store.state.s2 !== "pending") return; // resolved or navigated away
    void store.authUserinfo().finally(() => {
      if (store.state.s2 === "pending") setTimeout(tick, 5000);
      else store.save();
    });
  };
  setTimeout(tick, 5000);
}

export function onS3Pay(store: Store): void {
  const internals = storeInternals(store);
  // Tauri: open the REAL core-membership checkout popup, then POLL the on-chain
  // membership grant. The money + grant happen ENTIRELY server-side
  // (core-membership → treasury-signer droplet); this app only opens the URL and
  // watches the chain. S3 advances ONLY when the REAL on-chain grant lands (SBT +
  // >=32k stake) — NOT the KYC entitlement (which a verified member holds before
  // paying), and NEVER a faked settle (Rule 1). See pollMembership for why.
  if (BRIDGE_MODE === "tauri") {
    // ORDERING GUARD (Rule 1; see wallet_link.rs). The grant funds + mints to the
    // authority's `wallet_address` claim. Until THIS device's custody EOA is
    // LINKED, that claim is the counterfactual smart-wallet address no key can
    // spend — so core-membership refuses (`grant_denied_no_member_wallet`) rather
    // than strand the 32k bond at an unspendable address. So LINK FIRST, then
    // check out. The link ceremony re-reads the claim on approval
    // (authUserinfo/refreshWallet), so walletIsLinked() flips true and the next
    // Pay proceeds. This mirrors how activation is gated on the link.
    if (internals.linkBeforeCheckout("Link your wallet first — approve the link, then tap Pay again.")) return;
    // ★ MONEY GUARD ★ — never open checkout for a member who ALREADY holds the
    // grant. Settlement is chain truth, and the client had no way to observe an
    // existing grant: `settled` was reachable ONLY through `pollMembership`, which
    // ran ONLY from this handler. So a member who was granted while the app was
    // closed (or after its poll gave up) came back to S3 `idle`, whose only
    // paid control is the checkout button — and paying again is the one
    // thing that cannot help, because the membership is one-per-sub and the second
    // order can never be granted. Observed 2026-08-15: a member reached three
    // stranded `paid` orders this way. Read the chain BEFORE spending money.
    //
    // ONE ATTEMPT AT A TIME (core#584). The read above takes time, and until it ends a second
    // Pay, Back, or choosing membership again could each still open a checkout. So: a second
    // call while a check runs is ignored; each attempt carries a token, and an attempt whose
    // token was superseded (Back ends it via endS3Check) or whose stage changed opens nothing.
    if (store.state.s3Checking) return;
    const attempt = ++internals.s3Attempt;
    const stage = store.state.stage;
    store.setState({ s3Checking: true });
    void (async () => {
      let settled: boolean;
      try {
        settled = await store.settleS3IfAlreadyGranted();
      } catch {
        // Not a read failure (that returns false inside): something unexpected. Open nothing,
        // and give the controls back so the member can try again.
        if (attempt === internals.s3Attempt) store.setState({ s3Checking: false });
        return;
      }
      if (attempt !== internals.s3Attempt) return; // ended by Back / Continue free; ignore the result
      store.setState({ s3Checking: false });
      if (settled || store.state.stage !== stage) return;
      internals.s3PayingSince = Date.now();
      store.setState({ s3: "paying", s3GrantDelayed: false, s3SignerReady: null, membershipCheckoutAt: internals.s3PayingSince });
      // login_hint = the signed-in email → the browser checkout binds to THIS account.
      void bridge.membership.checkout(store.state.authEmail ?? undefined).catch(() => {
        // Popup open failed (headless / user cancel at OS level) — stay "paying";
        // the poll below simply never settles. The user can retry (S3 re-enterable).
      });
      internals.s3PollGen++;
      internals.pollMembership();
      // FIX 1a: S3 settles on the chain grant; the claim can land later. Keep watching
      // the entitlement so the member never sits on a stale "lapsed" banner.
      store.startEntitlementPoll();
    })();
    return;
  }
  // Web-dev sim: keep the prototype's fake settle so onboarding still walks.
  // (Guarded: the sim membership.checkout is a no-op; the tier flip is sim-only.)
  store.setState({ s3: "paying" });
  setTimeout(() => {
    store.setState({ s3: "settled", tier: "pilot" });
    store.save();
  }, 2500);
}

/**
 * core#584 — end any running pre-checkout grant check: its result is ignored, so it opens no
 * checkout. Back on the member checkout and "Continue free" call this. Idempotent.
 */
export function endS3Check(store: Store): void {
  const internals = storeInternals(store);
  internals.s3Attempt++;
  if (store.state.s3Checking) store.setState({ s3Checking: false });
}

/**
 * CORE-D3.C — poll while S3 is "paying" (Tauri only), advancing to "settled"
 * ONLY once the REAL on-chain membership grant lands: `MembershipStakeVault`
 * attributes >= 32,000 SALT to the member AND the member holds the SBT
 * (`isGrantOnChain` + `hasSbt`, read permissionlessly from chain 40204).
 *
 * WHY the on-chain grant and NOT the entitlement claim: a KYC-verified principal
 * is auto-granted the `commercial.kyc` tier by the authority (→ the app's `pilot`
 * tier) BEFORE paying for a membership, so `isPaidEntitlementActive` is already
 * true pre-payment and would let a verified user walk past S3 unpaid. The on-chain
 * SBT + stake is the honest "this membership was purchased AND provisioned" marker
 * — it lands only after the member pays and the treasury-signer executes the grant,
 * so S3 can never settle before payment (Rule 1 — no fabricated settle). The live
 * /userinfo is still folded (for the tier/entitlement DISPLAY), but it is not the
 * settle decision.
 *
 * Bounded (mirrors pollGrant's cadence) so a never-completing checkout does not
 * poll forever: after the cap it leaves S3 re-enterable (still "paying") rather
 * than fabricating a settlement.
 */
export function pollMembership(store: Store, attempt = 0, gen = storeInternals(store).s3PollGen): void {
  const internals = storeInternals(store);
  if (BRIDGE_MODE !== "tauri") return;
  // CADENCE. The old budget was 60 × 5s = 5 minutes flat, which was SHORTER than
  // the server's own retry cadence: core-membership re-drives stranded grants on a
  // 10-minute cron (`vercel.json` crons → /api/cron/reconcile-grants). So whenever
  // the Stripe webhook's grant attempt failed — the case the reconciler exists to
  // rescue — the client had already given up before the FIRST retry could run, and
  // the member sat on a dead spinner while the system healed itself behind them.
  // Observed 2026-08-15: paid at 12:43, poll expired ~12:48, first eligible sweep
  // was later still.
  //
  // Now: fast for the checkout window (the happy path settles in seconds), then
  // slow and long enough to span several reconciler sweeps without hammering the
  // public RPC — grantStatus is nine sequential eth_calls.
  const FAST_ATTEMPTS = 24; // 2 min at 5s — the normal settle window
  const FAST_MS = 5_000;
  const SLOW_MS = 30_000;
  const MAX_ATTEMPTS = FAST_ATTEMPTS + 52; // + 26 min at 30s ≈ 28 min total
  const delay = attempt < FAST_ATTEMPTS ? FAST_MS : SLOW_MS;
  const tick = () => {
    if (store.state.s3 !== "paying") return; // resolved or navigated away
    if (gen !== internals.s3PollGen) return; // a newer poll (Check again) took over
    // The signer read is a second, read-only source that only decides what S3 SAYS while it
    // waits (the delayed-grant card); it never settles. Every 30 s: each 6th fast tick, every
    // slow tick. core-membership caches the verdict 15 s.
    if (attempt % 6 === 0 || attempt >= FAST_ATTEMPTS) void internals.readSignerHealth();
    const member = store.identity().wallet;
    // Read the REAL on-chain grant; fold /userinfo for display (never the decision).
    void Promise.all([bridge.membership.grantStatus(member), store.authUserinfo()])
      .then(([grant]) => {
        if (store.state.s3 !== "paying" || gen !== internals.s3PollGen) return;
        // ONLY the genuine on-chain grant settles S3 — never the pre-payment
        // KYC entitlement (Rule 1).
        if (isGrantOnChain(grant) && grant.hasSbt) {
          store.setState({ s3: "settled", s3PollExhausted: false, s3GrantDelayed: false });
          store.save();
          return;
        }
        internals.updateGrantDelayed();
        // Not yet granted — keep polling until the bounded cap.
        if (attempt + 1 < MAX_ATTEMPTS) internals.pollMembership(attempt + 1, gen);
        // At the cap we stop polling; S3 stays "paying" (re-enterable) and we SAY SO
        // rather than leaving a spinner that will never resolve. Never faked.
        else store.setState({ s3PollExhausted: true });
      })
      .catch(() => {
        // Transient RPC/read error — retry until the cap; never fabricate a settle.
        if (gen !== internals.s3PollGen) return;
        internals.updateGrantDelayed();
        if (store.state.s3 === "paying" && attempt + 1 < MAX_ATTEMPTS) internals.pollMembership(attempt + 1, gen);
        else if (store.state.s3 === "paying") store.setState({ s3PollExhausted: true });
      });
  };
  setTimeout(tick, delay);
}

/**
 * Re-check the membership grant after the bounded poll gave up — the exit from
 * what used to be a dead-end spinner.
 *
 * Chain-read ONLY. It restarts `pollMembership`, which settles S3 exclusively on
 * the real on-chain grant (`isGrantOnChain && hasSbt`). It does NOT re-open Stripe
 * checkout, so re-checking can never charge a member a second time — the exact
 * hazard that made the dead end dangerous, since the only control the member could
 * see was the checkout button.
 */
export function recheckMembership(store: Store): void {
  const internals = storeInternals(store);
  if (store.state.s3 !== "paying") return;
  // From the delayed-grant card the first poll is still running: end it, so one loop runs.
  internals.s3PollGen++;
  store.setState({ s3PollExhausted: false });
  void internals.readSignerHealth();
  internals.pollMembership();
}

/**
 * Read core-membership's grant-signer readiness (public `/api/health`, core-membership#86) into
 * `s3SignerReady`, then re-decide the delayed-grant card. A failed read changes nothing: no read
 * is not evidence either way (Rule 1). Never settles S3.
 */
export async function readSignerHealth(store: Store): Promise<void> {
  const internals = storeInternals(store);
  if (BRIDGE_MODE !== "tauri") return;
  try {
    const h = await bridge.membership.signerHealth();
    if (store.state.s3 !== "paying") return;
    if (h.signerReady !== store.state.s3SignerReady) store.setState({ s3SignerReady: h.signerReady });
    internals.updateGrantDelayed();
  } catch {
    /* unreachable or not core-membership: keep the last read */
  }
}

/** Re-decide the delayed-grant card from the checkout time and the last signer read. */
export function updateGrantDelayed(store: Store): void {
  const internals = storeInternals(store);
  const delayed =
    store.state.s3 === "paying" &&
    isGrantDelayed({ payingSinceMs: internals.s3PayingSince, nowMs: Date.now(), signerReady: store.state.s3SignerReady });
  if (delayed !== store.state.s3GrantDelayed) store.setState({ s3GrantDelayed: delayed });
}

/**
 * Settle S3 directly from chain truth when the member ALREADY holds the grant.
 *
 * The missing "resume from what is actually true" step. S3's only exit was the
 * Continue button behind `s3 === "settled"`, and the only writer of `settled` was
 * `pollMembership`, started only by `onS3Pay`. Nothing ever asked the simple
 * question "is this member already granted?" — so a member granted out-of-band
 * (reconciler sweep, operator remediation, or simply a grant that landed after
 * the app closed) was stranded on a screen whose only affordance was to pay again.
 *
 * Same evidence bar as the poll: `isGrantOnChain && hasSbt`, read permissionlessly
 * from chain 40204. Returns false — and changes nothing — on any read failure, so
 * a flaky RPC can never manufacture a settle (Rule 1).
 */
export async function settleS3IfAlreadyGranted(store: Store): Promise<boolean> {
  const internals = storeInternals(store);
  if (BRIDGE_MODE !== "tauri") return false;
  if (store.state.s3 === "settled") return true;
  const grant = await internals.readHeldGrant();
  if (!grant) return false;
  store.setState({
    s3: "settled",
    s3PollExhausted: false,
    s3GrantDelayed: false,
    hasGrant: true,
    hasSbt: grant.hasSbt,
  });
  store.save();
  return true;
}

/**
 * The membership grant this account already holds on chain (`isGrantOnChain && hasSbt`, read
 * permissionlessly from 40204), or null. Null on any read failure: an unreadable chain is not
 * evidence of a grant (Rule 1). Shared by every checkout's money guard.
 */
export async function readHeldGrant(store: Store): Promise<GrantStatus | null> {
  try {
    const grant = await bridge.membership.grantStatus(store.identity().wallet);
    return isGrantOnChain(grant) && grant.hasSbt ? grant : null;
  } catch {
    return null;
  }
}

/** Checkout ordering guard: true (and the link ceremony opened) when the wallet is not linked yet. */
export function linkBeforeCheckout(store: Store, message: string): boolean {
  if (store.walletIsLinked()) return false;
  store.toast(message);
  void store.linkWallet();
  return true;
}

export function onS5Begin(store: Store): void {
  const internals = storeInternals(store);
  store.setState({ s5: "verifying", s5c: 0 });
  setTimeout(() => store.setState({ s5c: 1 }), 800);
  setTimeout(() => store.setState({ s5c: 2 }), 1600);
  setTimeout(() => store.setState({ s5c: 3 }), 2400);
  setTimeout(() => {
    // Enter the honest "settling / waiting for the on-chain grant" state and
    // start the bounded poll that settles S5 ONLY from the REAL chain read.
    store.setState({ s5: "settling", s5n: 0 });
    if (BRIDGE_MODE === "tauri") internals.pollGrant();
    else internals.pollGrantSim();
  }, 3300);
}

/**
 * BC-1.3 — the web-dev SIM S5 settle. The sim has NO real 40204, so it derives an
 * HONEST grant status from the persona/AppState via `bridge.membership.grantStatus`
 * (granted only for a paid+active sim member) and settles S5 from THAT — not a
 * blind timer (Rule 1's shape carried into the preview). Bounded like pollGrant.
 */
export function pollGrantSim(store: Store, attempt = 0): void {
  const internals = storeInternals(store);
  if (BRIDGE_MODE === "tauri") return;
  const MAX_ATTEMPTS = 20;
  const tick = () => {
    if (store.state.s5 !== "settling") return;
    void bridge.membership
      .grantStatus(store.identity().wallet)
      .then((grant) => {
        if (store.state.s5 !== "settling") return;
        if (isGrantOnChain(grant) && grant.hasSbt) {
          // s5StakeWei carries the sim-derived attributedStake (honest for the
          // preview — grantStatus derives it from the paid persona, not a real
          // chain read; the card renders it, never a hardcoded 32,000).
          store.setState({
            s5: "settled",
            s5n: 32000,
            s5StakeWei: grant.attributedStakeWei,
            // M-2.2/M-2.3: the lock + KYC state, straight from the bond.
            s5BondStatus: describeBond(grant),
            hasGrant: true,
            hasSbt: grant.hasSbt,
          });
          store.save();
          return;
        }
        if (attempt + 1 < MAX_ATTEMPTS) internals.pollGrantSim(attempt + 1);
      })
      .catch(() => {
        if (store.state.s5 === "settling" && attempt + 1 < MAX_ATTEMPTS) internals.pollGrantSim(attempt + 1);
      });
  };
  setTimeout(tick, 700);
}

/**
 * BC-1.3 — poll the REAL on-chain grant while S5 is "settling", advancing to
 * "settled" ONLY when the 32,000-SALT grant is genuinely on-chain AND the SBT is
 * minted AND the /userinfo entitlement is paid+active (Rule 1 — no fabricated
 * settlement). `hasGrant`/`hasSbt` are set ONLY from the real read; a member whose
 * grant has not landed simply stays in the honest "still settling" state (S5 is
 * re-enterable) — the poll never fabricates a grant.
 *
 * Bounded (mirrors pollMembership's cadence) so a never-completing grant does not
 * poll forever: after the cap it stops but leaves S5 "settling" (re-enterable),
 * never faked. TAURI only — the web-dev SIM path keeps its own honest animation
 * (the tick loop) that derives settlement from the sim grantStatus.
 */
export function pollGrant(store: Store, attempt = 0): void {
  const internals = storeInternals(store);
  if (BRIDGE_MODE !== "tauri") return;
  const MAX_ATTEMPTS = 60; // ~5 min at 5s cadence
  const tick = () => {
    if (store.state.s5 !== "settling") return; // resolved or navigated away
    const member = store.identity().wallet;
    // Read the REAL on-chain grant (attributedStake + SBT) and re-check the live
    // entitlement leg. Both must be real for S5 to settle (Rule 1).
    void Promise.all([bridge.membership.grantStatus(member), store.authUserinfo()])
      .then(([grant]) => {
        if (store.state.s5 !== "settling") return;
        const grantOnChain = isGrantOnChain(grant);
        // "Stuck on step 5" fix: the ON-CHAIN grant — a minted membership SBT plus
        // attributed stake in the vault — is the DEFINITIVE, real proof the grant
        // landed (Rule 1: both are live chain reads). Settle S5 on that. The paid
        // ENTITLEMENT tier is an OFF-CHAIN refinement read from /userinfo that can
        // lag the on-chain grant (session-token snapshot / roster propagation);
        // gating the settle on it stranded members whose grant was already on
        // chain. We keep calling authUserinfo() each tick so the tier still
        // refines to commercial/commercial.kyc — it just no longer BLOCKS S5.
        if (grantOnChain && grant.hasSbt) {
          // hasGrant/hasSbt/s5StakeWei set ONLY from the real read; never before
          // it. s5StakeWei is the REAL attributedStake the settled card renders
          // (F1 — no hardcoded 32,000).
          store.setState({
            s5: "settled",
            s5n: 32000,
            s5StakeWei: grant.attributedStakeWei,
            // M-2.2/M-2.3: the lock + KYC state, straight from the bond.
            s5BondStatus: describeBond(grant),
            hasGrant: true,
            hasSbt: grant.hasSbt,
          });
          store.save();
          return;
        }
        // Not yet fully settled — keep polling until the bounded cap. Never set
        // hasGrant/hasSbt without the real grant.
        if (attempt + 1 < MAX_ATTEMPTS) internals.pollGrant(attempt + 1);
        // At the cap we stop; S5 stays "settling" (re-enterable), never faked.
      })
      .catch(() => {
        // A transient RPC/read error — retry until the cap; never fabricate.
        if (store.state.s5 === "settling" && attempt + 1 < MAX_ATTEMPTS) internals.pollGrant(attempt + 1);
      });
  };
  setTimeout(tick, 5000);
}

export function onEnter(store: Store): void {
  const s = store.state;
  store.setState({
    stage: "done",
    coach: s.coachDone ? -1 : 0,
    chatMsgs: s.chatMsgs.length ? s.chatMsgs : [greeting({ ...store.persona(), name: store.identity().real ? store.identity().name : "" })],
  });
  store.save();
}

export function selectPersona(store: Store, pid: string): void {
  const ns = freshState(pid);
  ns.demoOpen = true;
  store.setState(ns);
  store.save();
}

export function resetProto(store: Store): void {
  try {
    localStorage.removeItem(STORAGE_KEY);
  } catch {
    /* ignore */
  }
  const ns = freshState("p1");
  store.setState(ns);
  store.toast("Prototype reset — fresh install");
}
