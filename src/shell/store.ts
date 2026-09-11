// =====================================================================
// citrate-core — imperative store
// Ports the DCLogic class methods from design/CitrateCore.dc.html:
// the 600ms sim tick, ceremony queue + resolvers, chat send loop, journal
// operations, node lifecycle. React binds via useStore() (see below).
//
// Kept imperative (one mutable state object + subscriber notify) to match
// the prototype's setState-with-updater semantics exactly. A React reducer
// would fight the timer/resolver flow; this is the honest 1:1.
// =====================================================================
import { useSyncExternalStore } from "react";
import {
  Activity,
  AppState,
  CerSpec,
  ChatMsg,
  LogLine,
  Persona,
  PERSONAS,
  PERSIST_KEYS,
  RANK,
  STORAGE_KEY,
  freshState,
  greeting,
  loadState,
  makeHash,
  WalletReview,
} from "./state";
import type { CeremonyView } from "../bridge/types";
import { NODE_LOG_TEMPLATES } from "../data/seed";
import { createDemoProvider, createLocalProvider, createAgentProvider, ChatProvider, ToolCall } from "../agent/harness";
import { canSelect, resolveActive, type ModelChoice } from "../agent/modelRouter";
import { formatJournalForAgent } from "../agent/journalRead";
import { validateNewSkill, runPrompt } from "../agent/userSkills";
import type { GrantStatus, GroupRole, MemoryResult } from "../bridge/domains";
import { bindSimHost, bridge } from "../bridge";
import { BRIDGE_MODE } from "../bridge/mode";
import { layoutGraph } from "./memGraph";
import { buildPeopleDirectory } from "../surfaces/peopleDirectory";
import { buildRoleNavigator } from "../surfaces/groupsNavigator";
import { parseJoinLink, resolveJoinCode, parseClaimLink } from "../surfaces/referral";
import { groupsSlice } from "./slices/groups";

/** Q-E.1 — plain-language labels for the sim "settles only in desktop" toast. */
const WALLET_ACTION_LABELS: Record<WalletReview["kind"], string> = {
  send: "Send",
  stake: "Stake",
  "withdraw-request": "Withdrawal request",
  "withdraw-claim": "Withdrawal claim",
  claim: "Claim",
  "wallet-link": "Wallet link",
  agent: "Agent action",
  social: "Verify identity",
  deploy: "Deploy contract",
};

type Updater = Partial<AppState> | ((s: AppState) => Partial<AppState>);

/**
 * A3-03 — is an entitlement `expiresAt` claim in the past (or anomalous)?
 * - ABSENT (null/undefined/empty) → NOT expired: the authority legitimately may
 *   not send an expiry; the hard id_token `exp` gate in Rust still bounds the
 *   session.
 * - PRESENT but UNPARSEABLE → treated as EXPIRED (fail-closed): a malformed
 *   expiry on a T1 gating surface is an anomaly, not a licence to keep the tier.
 * - PRESENT + in the past → expired.
 * Accepts an ISO date/datetime OR a unix-epoch numeric string. The authority nests
 * the entitlement expiry as epoch-MILLISECONDS (citrate-identity: `new Date(row).
 * getTime()`), and the Rust seam forwards it verbatim as a digit string — so a bare
 * `Number(x) * 1000` (assuming seconds) mis-scaled a real ms expiry to ~year 58000
 * and NEVER expired. Disambiguate by magnitude: a value already in ms range
 * (>= 1e12, i.e. any time after 2001) is used as-is; a smaller value is seconds.
 */
export function isExpiredClaim(expiresAt: string | null | undefined): boolean {
  if (!expiresAt) return false;
  let ms: number;
  if (/^\d+$/.test(expiresAt)) {
    const n = Number(expiresAt);
    ms = n >= 1e12 ? n : n * 1000; // ms already vs unix-seconds
  } else {
    ms = Date.parse(expiresAt); // ISO date/datetime
  }
  if (!Number.isFinite(ms)) return true; // fail-closed on an unparseable value
  return ms < Date.now();
}

/**
 * CORE-D3.C — has the S3 checkout entitlement actually landed?
 *
 * The onboarding "Pay" step advances ONLY when `/userinfo` shows the REAL grant
 * (Rule 1 — the tauri path never fakes a settled membership). "Landed" means the
 * folded claim shows a PAID tier (rank past `free`/public — the commercial.kyc
 * grant) AND the entitlement reads `active`. A still-`free`/`public` tier, or a
 * lapsed/grace entitlement, is NOT settled — the poll keeps waiting.
 *
 * This is the single decision point the poll uses, so it is unit-tested against
 * the exact folded AppState the entitlement engine produces.
 */
export function isPaidEntitlementActive(st: { tier: string; entitlement: string }): boolean {
  const rank = RANK[st.tier];
  // Unknown tiers are treated as unpaid (fail-closed): only a KNOWN paid tier
  // (rank > free) with an ACTIVE entitlement counts as the grant having landed.
  if (rank === undefined || rank <= RANK.free) return false;
  return st.entitlement === "active";
}

/**
 * BC-1.3 — the validator stake requirement in wei (== 32,000 SALT == 32000e18).
 * A granted member's `MembershipStakeVault.attributedStake(member)` reads at least
 * this. Kept as a BigInt so the comparison never loses precision (a 32k-SALT grant
 * far exceeds JS's safe integer range).
 */
export const VALIDATOR_STAKE_REQUIREMENT_WEI = 32000n * 10n ** 18n;

/**
 * BC-1.3 — is the 32,000-SALT membership grant GENUINELY on-chain?
 *
 * The S5 grant leg settles ONLY when the member holds the SBT AND at least the
 * validator stake requirement is attributable to them on chain (Rule 1 — read from
 * chain, never a fabricated settlement).
 *
 * THE VAULT IS THE ANSWER AGAIN (M-2, citrate-chain #141). This gate has been
 * widened twice, both times because the model moved the principal somewhere the
 * gate could not see:
 *   - ADR 2026-07-27 funded the member's OWN EOA, so a just-granted member held
 *     the 32k as NATIVE balance and the vault read 0 — added `nativeBalanceWei`;
 *   - after they self-bonded it moved to the registry — added `bondedStakeWei`.
 * Settling on the vault alone left a correctly-granted member polling "still
 * settling" forever (the 2026-07-28 stall): working, indistinguishable from broken.
 *
 * Under M-2 the principal has exactly ONE home again — the member's MemberBond
 * escrow — and the vault attributes it at GRANT time and keeps attributing it. So
 * `attributedStake` is durable across the whole lifecycle and both extra legs go.
 *
 * They would in fact be WRONG to keep. The registry leg stays 0 from the grant
 * until the member runs the activation ceremony, which can be days; the native leg
 * never fills at all, because M-2 sends the member only ~0.05 SALT of gas. Keeping
 * either would be reading a number that no longer carries the meaning it did.
 *
 * `hasSbt` remains a hard precondition.
 *
 * Wei values are decimal strings parsed as BigInt so a value beyond JS number range
 * is exact. A malformed value fails closed — never a fabricated grant.
 */
export function isGrantOnChain(g: { attributedStakeWei: string; hasSbt: boolean }): boolean {
  if (!g.hasSbt) return false;
  let staked: bigint;
  try {
    staked = BigInt(g.attributedStakeWei);
  } catch {
    return false; // fail-closed on an unparseable value — never fabricate a grant
  }
  return staked >= VALIDATOR_STAKE_REQUIREMENT_WEI;
}

/**
 * How the dashboard describes a member's bond (M-2.2 / M-2.3). Pure, so the
 * wording is testable without a render.
 *
 * Rule 1: every branch names a real on-chain fact. "Staked" is never claimed for a
 * member whose escrow does not exist, and an unlock is never advertised as
 * withdrawable while KYC is outstanding — the chain enforces both gates, and the UI
 * must not imply otherwise.
 */
export function describeBond(g: {
  bondDeployed: boolean;
  unlockBlock: number | null;
  isUnlocked: boolean;
  isKycVerified: boolean;
  hasValidator: boolean;
}): string {
  if (!g.bondDeployed) return "Not yet staked";
  if (!g.isUnlocked) {
    const at = g.unlockBlock === null ? "—" : g.unlockBlock.toLocaleString();
    return `Staked · unlocks at block ${at}`;
  }
  // Unlocked. Owner decision A.5: eligible, never automatic — so "can", not "has".
  // A.6: KYC supersedes the lock, so an unverified member is NOT told their funds
  // are available.
  if (!g.isKycVerified) return "Unlocked · KYC required to withdraw";
  return g.hasValidator ? "Unlocked · validating · can withdraw" : "Unlocked · can withdraw";
}

/**
 * The onboarding patch implied by a REAL on-chain grant, or `null` when the grant
 * is not (yet) genuinely on chain.
 *
 * Pure so the settle DECISION is testable without a chain, a bridge, or a mocked
 * `BRIDGE_MODE` — the same reason `isGrantOnChain` / `describeBond` / `mapNodeState`
 * are pure. It requires EXACTLY the evidence `pollGrant` requires (attributed stake
 * on chain AND a minted SBT) and returns the SAME fields from the SAME values, so
 * the launch-time reconcile and the in-flight poll can never disagree.
 *
 * Returning `null` (rather than an empty patch) keeps "no grant" a distinct,
 * non-mutating outcome — a member mid-flow is never nudged forward (Rule 1).
 */
export function reconciledGrantPatch(g: GrantStatus): Partial<AppState> | null {
  if (!isGrantOnChain(g) || !g.hasSbt) return null;
  return {
    s3: "settled",
    s5: "settled",
    s5n: 32000,
    s5StakeWei: g.attributedStakeWei,
    s5BondStatus: describeBond(g),
    hasGrant: true,
    hasSbt: g.hasSbt,
  };
}

/**
 * Membership-renew SAFEGUARD (Luke, 2026-09-11). A paid member reopened the app and
 * was wrongly shown "renew": the `/userinfo` entitlement re-check on reopen came back
 * THIN for their session (signed in, but no tier claim), so `applyAuthStatus` folded
 * `tier:free + entitlement:lapsed` — even though their on-chain membership SBT and
 * staked grant were fully present. The on-chain SBT is the AUTHORITATIVE membership
 * proof (the `/userinfo` entitlement is a *derived mirror* of it, per
 * citrate-identity/src/entitlements.ts), so a real member must never be pushed to
 * renew because of a transient claim.
 *
 * Returns a patch restoring an ACTIVE paid membership from the on-chain grant, or
 * `null`. Regression guards (all must hold):
 *  - the current entitlement is `lapsed` — nothing to correct otherwise;
 *  - the lapse is NOT from an explicit PAST expiry (`isExpiredClaim(authExpiresAt)`):
 *    a genuinely elapsed year is a REAL lapse and MUST still show renew — this only
 *    rescues the missing/thin-claim case;
 *  - the grant is genuinely on chain AND the SBT is minted (`isGrantOnChain && hasSbt`)
 *    — a live 40204 read, never a client assertion, so a never-paid free account
 *    (no SBT) is never elevated.
 * The restored tier is the paid-member baseline `commercial` (rank > free); a
 * commercial.kyc upgrade still arrives from the next live claim.
 */
export function entitlementSafeguardFromChain(
  st: { tier: string; entitlement: string; authExpiresAt: string | null },
  g: GrantStatus,
): Partial<AppState> | null {
  if (st.entitlement !== "lapsed") return null;
  if (isExpiredClaim(st.authExpiresAt)) return null; // a real expiry — honor the lapse
  if (!isGrantOnChain(g) || !g.hasSbt) return null; // no on-chain membership proof
  return { tier: "commercial", entitlement: "active" };
}


/**
 * W3.3 — render mem hits as plain text the agent feeds back to the model. Honest
 * emptiness when a tenant has no match (Rule 1 — the model is told there's nothing
 * rather than being left to invent Citrate facts). The hit `title` carries the
 * authored content (the `Title › Section` breadcrumb + body from docs_ingest).
 */
export function formatMemoryHits(res: MemoryResult): string {
  if (!res.hits.length) {
    return `No results in the ${res.tenant} memory (${res.totalInTenant} nodes total). Do not fabricate; tell the member nothing was found.`;
  }
  return res.hits.map((h, i) => `[${i + 1}] ${h.title}`).join("\n\n");
}

/**
 * Derive a display name + initials from an email local-part. The authority
 * issues NO display-name claim, so `larry@citrate.ai` → { name: "Larry",
 * initials: "LA" } and `ada.lovelace@x.io` → { name: "Ada Lovelace", initials:
 * "AL" }. Used by applyAuthStatus to render a real signed-in user's identity;
 * exported so the derivation is unit-tested independently of the auth plumbing.
 */
export function deriveIdentityFromEmail(email: string): { name: string; initials: string } {
  const local = email.split("@")[0];
  const words = local.split(/[._+-]+/).filter(Boolean);
  const name = words.map((w) => w.charAt(0).toUpperCase() + w.slice(1)).join(" ") || email;
  const fromWords = words.length > 1 ? words.map((w) => w.charAt(0)).join("") : local.slice(0, 2);
  const initials = (fromWords || local.slice(0, 2)).slice(0, 2).toUpperCase();
  return { name, initials };
}

/**
 * Q-A.2/Q-B.2 — convert the REAL streamed node log lines (bridge `NodeLogLine`:
 * `{ ts, stream, line }`) into the app's `LogLine` shape (`{ t, line, id }`) the
 * Node LOG panel renders. `ts` (unix ms) → a HH:MM:SS stamp; the id is the ring
 * index (stable within a snapshot). The tail is capped at 14 (the panel's window)
 * so a 500-line ring does not flood the DOM; the NEWEST lines are kept. This is
 * what replaces the fabricated NODE_LOG_TEMPLATES path in a packaged build —
 * these are REAL node stdout/stderr lines (Rule 1), never a template.
 */
export function foldNodeLogs(lines: { ts: number; stream: "out" | "err"; line: string }[]): LogLine[] {
  const tail = lines.slice(-14);
  return tail.map((l, i) => {
    const d = new Date(l.ts);
    const t =
      String(d.getHours()).padStart(2, "0") +
      ":" +
      String(d.getMinutes()).padStart(2, "0") +
      ":" +
      String(d.getSeconds()).padStart(2, "0");
    return { t, line: l.line, id: i };
  });
}

/**
 * Map the REAL node supervisor state (bridge.node.status → node.rs `map_state`:
 * stopped/starting/running/restarting/failed) onto the app's node lifecycle
 * enum. A `running` node is `syncing` until fully synced, then `validating` when
 * its stake meets the threshold, else `synced`. This is the seam that replaces
 * the sim `tick()` node state in a Tauri build.
 */
export function mapNodeState(state: string, syncPct: number, staked: number): AppState["node"] {
  switch (state) {
    case "starting":
    case "restarting":
      return "prov";
    case "failed":
      return "error";
    case "running":
      if (syncPct < 100) return "syncing";
      return staked >= 32000 ? "validating" : "synced";
    case "stopped":
    default:
      return "off";
  }
}

/**
 * CORE-AI1 / BC-3.2 — the pure LOCAL → GATEWAY → DEMO provider SELECTION rule,
 * extracted so it is unit-tested independently of the store singleton + bridge.
 *
 * Priority (Rule 1 — the route is HONEST, it never claims a provider it can't call):
 *  1. **local** — ONLY in the Tauri build AND the Rust inference state is `ready`
 *     (the local model is verified AND `llama-server` is HEALTHY). If the local
 *     server isn't healthy the Rust state is NOT `ready`, so we fall through — we
 *     never fabricate a local reply against a down server.
 *  2. **real** (gateway) — the current default id is actually CONFIGURED (its key
 *     is sealed in the OS keyring).
 *  3. **demo** — the honest built-in agent (web preview has no keyring/local model,
 *     so it always lands here).
 *
 * `inferenceState` is the kebab string from `model_inference_state` (Rust); only
 * `"ready"` means the local server is actually serving.
 */
export function pickChatProviderKind(
  statuses: { id: string; configured: boolean }[],
  aiDefault: string,
  mode: "sim" | "tauri",
  inferenceState?: string,
): "local" | "real" | "demo" {
  if (mode !== "tauri") return "demo";
  // 1) LOCAL: only when the real Rust state says the model is ready AND the
  //    llama-server is healthy — any other state falls through (honest).
  if (inferenceState === "ready") return "local";
  // 2) GATEWAY (real): the default provider id has a sealed key.
  if (statuses.some((p) => p.id === aiDefault && p.configured)) return "real";
  // 3) DEMO: the honest built-in agent.
  return "demo";
}

/**
 * CORE item 4 — fold the REAL indexed tx history (`real`, from CitrateScan) into
 * the current activity list (`current`, which may hold OPTIMISTIC just-sent rows
 * from `addActivity`), DEDUPED by hash. The real indexed list is authoritative:
 * any optimistic/local row whose hash now appears in `real` is dropped so a
 * settled tx renders exactly once. Optimistic rows NOT yet indexed are kept, in
 * front, so a just-sent tx stays visible until the indexer catches up. Rows are
 * capped at 24 (the prototype cap). This is idempotent when `real === current`
 * (the sim path echoes state.activity), so re-merging never duplicates.
 */
export function mergeActivity(real: Activity[], current: Activity[]): Activity[] {
  const realHashes = new Set(real.map((a) => a.hash));
  // Optimistic/local rows the indexer hasn't returned yet (dedupe by hash) — but
  // never re-add a row that IS in the real list (that would double it).
  const pendingLocal = current.filter((a) => !realHashes.has(a.hash));
  return pendingLocal.concat(real).slice(0, 24);
}

export class Store {
  state: AppState;
  private subs = new Set<() => void>();
  private snap: AppState;
  private cid = 0;
  private mid = 0;
  private uskSeq = 0;
  private resolvers: Record<string, (v: string) => void> = {};
  private timer: ReturnType<typeof setInterval> | null = null;
  private nodeTimer: ReturnType<typeof setInterval> | null = null;
  /** Epoch ms of the last `grantStatus` bond read — see the throttle in refreshNode. */
  private lastBondReadAt = 0;
  private nodeWatchdog: ReturnType<typeof setInterval> | null = null;
  /** True while an outage has already been reported, so we warn once, not every 30s. */
  private nodeOutageNotified = false;
  private modelTimer: ReturnType<typeof setInterval> | null = null;
  private nodeStarting = false;
  // In-flight guards — a slow daemon/RPC call must not let the 2s pollers or a re-mounted surface
  // stack concurrent calls that queue behind a blocked one (a pinwheel amplifier). Each async loop
  // skips its tick while the previous is still running.
  private peopleRefreshing = false;
  private nodeRefreshing = false;
  private modelRefreshing = false;
  /** Sync-stall detection: the highest height we've seen advance, and when it last froze (0 = not
   *  frozen). A frozen height while still behind the tip, with peers connected, means the node's
   *  import pipeline has stalled — surfaced as the honest "stalled" state instead of a stuck "syncing". */
  private nodeStallLastHeight = -1;
  private nodeStallSince = 0;
  /** Bounded auto-recovery budget for a stalled sync. Only reset on reaching FULL sync, so we can't
   *  restart-loop forever against a chain-side wedge that advances only a few blocks per restart. */
  private nodeStallCycles = 0;
  private nodeStalledNotified = false;
  // W1.x — true once the producer has been armed this session (the node respawns
  // with --mine and mints proposer.key). Gate for `maybeAutoBond` step 1.
  private validatorArmed = false;
  // W1.3 — true once the onboarding S6 auto bond-activation ceremony has been opened this
  // session, so `maybeAutoBond` does not re-open the wallet review every 2s poll.
  private autoBondAttempted = false;
  private _saveT: ReturnType<typeof setTimeout> | null = null;
  private _toastT: ReturnType<typeof setTimeout> | null = null;
  private _s1t: ReturnType<typeof setTimeout> | null = null;
  provider: ChatProvider | null = null;
  // element refs (imperative, like the design)
  chatScrollEl: HTMLElement | null = null;
  chatInputEl: HTMLInputElement | null = null;
  jChatScrollEl: HTMLElement | null = null;
  jChatInputEl: HTMLInputElement | null = null;

  constructor() {
    this.state = loadState();
    this.snap = this.state;
    this.provider = createDemoProvider(() => this.snapshot());
    // Bind the sim adapter to this Store so the bridge (in sim mode) reads and
    // writes the live prototype state — the 1:1 UI is preserved (CORE-A1 A1.2).
    bindSimHost({
      getState: () => this.state,
      patch: (u) => this.setState(u),
    });
  }

  // ---- React binding ----
  subscribe = (cb: () => void): (() => void) => {
    this.subs.add(cb);
    return () => this.subs.delete(cb);
  };
  getSnapshot = (): AppState => this.snap;

  setState(u: Updater): void {
    const patch = typeof u === "function" ? u(this.state) : u;
    this.state = { ...this.state, ...patch };
    this.snap = this.state;
    this.subs.forEach((cb) => cb());
  }

  start(): void {
    if (this.timer) return;
    this.timer = setInterval(() => this.tick(), 600);
    // CORE-A2 — pull the real custody lock state into the Keys-&-security
    // section. In a Tauri build this reads the live vault (custody_status); in
    // web-dev it is the sim shim. Failures leave the field "unknown" (honest).
    void this.refreshCustody();
    // CORE-A3 — fold the live entitlement claim into the engine on launch. In a
    // Tauri build this reads the real /userinfo-derived status (silent if signed
    // out); in web-dev it reads the sim persona. Honest no-op on failure.
    void this.refreshAuth();
    // CORE-AI1 — select the chat provider: a REAL OpenAI-compatible provider if
    // the default id is configured (key sealed in the OS keyring), else the honest
    // built-in demo agent. Web-dev has no keyring, so this always resolves to demo.
    void this.rebuildProvider();
    // CORE (Phase 1) — in a Tauri build, poll the REAL node vitals (height/peers/
    // syncPct/state) from the supervisor + local RPC every 2s and fold them into
    // AppState, so the Node surface + Sidebar render live numbers instead of the
    // sim tick(). No-op in web-dev (the sim tick drives those there).
    if (BRIDGE_MODE === "tauri") {
      void this.refreshNode();
      this.nodeTimer = setInterval(() => void this.refreshNode(), 2000);
      // AUTOSTART. `startNode()` was reachable ONLY from a button, so after any
      // quit/restart/crash a member's node stayed down until they went looking for
      // it — silently not syncing, not producing, not earning. Bring it up on every
      // launch UNLESS the member explicitly stopped it (nodeIntent === "stopped").
      // Deferred a tick so the first refreshNode() can observe an already-running
      // supervisor and we don't double-spawn (node.rs is idempotent, but this keeps
      // the UI out of a spurious "prov").
      setTimeout(() => {
        if (this.state.nodeIntent !== "run") return;
        if (this.state.node !== "off" && this.state.node !== "error") return;
        this.startNode();
      }, 1500);
      // AUTOCONNECT the memory daemon on launch (like the node) so the knowledge graph is live +
      // SEEDED on open, not only after the member clicks Start on the Storage surface. startMemoryDaemon
      // is idempotent + honest (an offline daemon surfaces its error) and seeds the constellation once
      // connected. Deferred so the node/wallet vitals it seeds from are already folded.
      setTimeout(() => void this.startMemoryDaemon(), 2500);
      // RESUME S3 FROM CHAIN TRUTH. `s3` is not persisted, so every launch re-enters
      // S3 as `idle` — a screen whose only control is "Check out · $48". A member
      // whose grant landed while the app was closed would therefore be invited to
      // pay a second time for a membership they already hold. Ask the chain once on
      // launch; a granted member lands on Continue instead of a checkout button.
      if (this.state.stage === "s3") void this.settleS3IfAlreadyGranted();
      // WATCHDOG. The supervisor retries a crashed node with backoff and then gives
      // up; nothing told the member. Poll the gap between intent and reality and say
      // so ONCE per outage, with the real reason from the node's own crash record.
      this.nodeWatchdog = setInterval(() => void this.checkNodeLiveness(), 30_000);
      // BC-3 — fold the REAL local-model status on launch so the S6.5 step (and
      // Settings) show a resumed download / an already-verified model honestly.
      // AND actually RESUME it: a prior session that was mid-download leaves the
      // status "downloading" (a `.part` on disk) but NOTHING re-drives the fetch on
      // the next launch — so the bar froze at its last % forever with no network
      // activity (observed 2026-08-05). If we resolve to "downloading" and the
      // member hasn't skipped, kick the streamed (Range-resumable) fetch again.
      void this.refreshModel().then(() => {
        if (this.state.modelState === "downloading" && !this.state.modelSkipped) {
          this.startModelDownload();
        }
      });
      // Seamless device-bound unlock FIRST (passphrase-less model): a fresh launch
      // starts with a LOCKED in-memory session, and there is no user passphrase to
      // enter, so nothing else re-unlocks the vault. Every wallet read below is gated
      // on an unlocked vault (address/balances do an in-process custody_get), so
      // without this they fail closed and the wallet appears broken. Run the wallet
      // reads AFTER the unlock attempt resolves (best-effort; a reset keychain leaves
      // the honest locked state).
      // Replace the persona-derived placeholder id with the REAL device fingerprint
      // (sha256 of the device-bound custody pubkey). Honest no-op on failure: the
      // labelled placeholder stands rather than a wrong claim about this machine.
      void bridge.wallet
        .deviceId()
        .then((id) => {
          if (id) {
            this.setState({ deviceId: id });
            this.save();
          }
        })
        .catch(() => {
          /* no vault yet (or web-dev) — leave the existing id untouched */
        });
      // INDEPENDENT of the custody chain below. The grant is a permissionless chain
      // read that needs no vault, no session and no unlock — chaining it behind
      // custodyEnsureUnlocked() meant a vault that never settles silently suppressed
      // it, stranding a paid member with a fully-granted on-chain position.
      void this.reconcileOnboardingFromChain();
      void this.custodyEnsureUnlocked().finally(() => {
        // Re-establish the signed-in session from the vaulted OIDC refresh token so
        // a restart keeps the member's paid tier instead of dropping to signed-out
        // → public/free. The refresh token is a custody slot, so this MUST run after
        // the unlock above (a locked vault fails the read closed).
        void this.resumeSession();
        // Real wallet balances (native liquid + claimable) — folded once on launch;
        // the Wallet surface also refreshes on mount + after a settled ceremony.
        void this.refreshWallet();
        // WP2 — the real pending-withdrawal queue (chain-sourced), folded on launch.
        void this.refreshPendingWithdrawals();
        // Item 4 — the real indexed tx history (CitrateScan txlist), folded on launch.
        void this.refreshActivity();
      });
    }
  }

  /**
   * Refresh the runtime custody lock state from the bridge. Reads real vault
   * status in a Tauri build; the sim shim in web-dev. Never throws — an
   * unavailable/failed read is reported honestly as `"unknown"`.
   */
  async refreshCustody(): Promise<void> {
    try {
      const st = await bridge.custody.status();
      this.setState({
        custodyLock: st.unlocked ? "unlocked" : "locked",
        // config.autolock is the single source of truth; keep UI in sync.
        autolock: st.autolockMins,
      });
    } catch {
      this.setState({ custodyLock: "unknown" });
    }
  }

  /**
   * Q-A.4a item 2 — read the REAL memory-daemon status (`memory_status()`) and
   * fold the REAL socket path + `semantic` flag + supervisor state into AppState.
   * Never fabricated: the old client-invented socket-path constant is replaced by
   * the daemon-reported path; `semantic` drives the Storage "semantic available"
   * state ONLY when the daemon reports it. A transport failure (daemon not running)
   * is HONEST → memDaemon "offline", memSocketPath null, memSemantic false. Never
   * throws. Returns the read state so callers can gate a constellation refresh.
   */
  async refreshMemoryStatus(): Promise<"running" | "offline"> {
    try {
      const st = await bridge.memory.status();
      const running = st.state === "running";
      this.setState({
        memDaemon: running ? "running" : "offline",
        memDaemonError: null,
        memSocketPath: st.socketPath || null,
        memSemantic: !!st.semantic,
      });
      return running ? "running" : "offline";
    } catch {
      // Honest: the daemon is not running / unreachable — no fabricated path/flag.
      this.setState({ memDaemon: "offline", memSocketPath: null, memSemantic: false });
      return "offline";
    }
  }

  /**
   * Q-A.4a item 3 — START the memory daemon. Nothing else calls
   * `bridge.memory.start()`, so the graph is otherwise permanently offline. On a
   * fresh dev machine the mem-mcp binary isn't bundled yet (WO-2), so this
   * HONESTLY errors `BinaryNotFound` — that's a real error, not a fabrication, and
   * it is surfaced (memDaemon "error" + the coarse message). On success it re-reads
   * the status and re-runs the constellation fetch so the graph loads.
   */
  async startMemoryDaemon(): Promise<void> {
    this.setState({ memDaemon: "idle", memDaemonError: null });
    try {
      await bridge.memory.start();
    } catch (err) {
      // Honest failure (e.g. BinaryNotFound — the binary isn't bundled yet).
      this.setState({ memDaemon: "error", memDaemonError: String((err as Error).message ?? err) });
      return;
    }
    const state = await this.refreshMemoryStatus();
    if (state === "running") {
      await this.refreshConstellation();
      void this.seedMemoryGraph();
    }
  }

  /**
   * Populate the memory graph the moment the daemon connects, so the constellation has REAL content
   * on open instead of an empty canvas: seed network/node/stake facts into the constellation tenants
   * (`chain-state` + `personal`) AND preload the bundled Citrate docs (`citrate-docs`). Both are gated
   * + idempotent in Rust (skip a tenant that is already seeded, or when the daemon is lexical-only), so
   * this is safe to call on every connect. Refreshes the constellation if anything was authored. Every
   * fact is composed Rust-side from real app state — never fabricated (Rule 1).
   */
  async seedMemoryGraph(): Promise<void> {
    const s = this.state;
    const facts = {
      chainId: 40204,
      nodeState: s.node,
      height: s.height,
      peers: s.peers,
      walletAddr: s.walletAddr,
      hasGrant: !!s.hasGrant,
      grantStakedSalt: s.hasGrant && s.s5StakeWei ? Math.round(Number(BigInt(s.s5StakeWei)) / 1e18) : 0,
      bondStatus: s.s5BondStatus || "",
      hasSbt: !!s.hasSbt,
    };
    let changed = false;
    try {
      const r = await bridge.memory.seedContext(facts);
      if (r.authored > 0) changed = true;
    } catch {
      /* honest no-op: daemon race / unavailable (web preview) — never a fabricated node */
    }
    try {
      const r = await bridge.memory.ingestDocs();
      if (!r.skipped && r.chunks > 0) changed = true;
    } catch {
      /* honest no-op */
    }
    if (changed) await this.refreshConstellation();
  }

  /**
   * Q-A.4a item 4/5 — fetch the REAL constellation (personal + chain-state tenants)
   * from the mem-mcp daemon and fold the laid-out graph into AppState. A transport
   * failure is HONEST (`memGraphState: "unavailable"`), never seed data (Rule 1).
   * `q` (optional) routes through the REAL `bridge.memory.search(tenant, q)` RPC
   * so the graph reflects a real daemon-side search, not only the client filter.
   */
  async refreshConstellation(q?: string): Promise<void> {
    this.setState({ memGraphState: "loading" });
    try {
      const tenants =
        q && q.trim().length >= 2
          ? await Promise.all(
              ["personal", "chain-state"].map((t) => bridge.memory.search(t, q.trim())),
            )
          : await bridge.memory.constellation();
      const rawNodes = tenants.flatMap((t) =>
        t.hits.map((h) => ({ id: h.id, label: h.title, tenant: t.tenant, kind: h.kind })),
      );
      const graph = layoutGraph(
        tenants.map((t) => ({ tenant: t.tenant, totalInTenant: t.totalInTenant })),
        rawNodes,
      );
      this.setState({ memGraph: graph, memGraphState: "ready" });
    } catch {
      // Honest: the daemon is not running / unreachable. Show offline, not seed.
      this.setState({ memGraph: undefined, memGraphState: "unavailable" });
    }
  }

  /**
   * CORE-AI1 (@rule8) — (re)select the chat provider from the live AI config.
   *
   * Reads `bridge.chat.providerStatus()` (Tauri: the OS-keyring-sealed provider
   * metadata, NEVER the key). If the current `aiDefault` id is CONFIGURED, chat
   * runs through the REAL provider — `createRealProvider` calls the Rust `ai_chat`
   * command, which reads the STORED baseURL for that id and POSTs from Rust (the
   * key never touches the webview; the webview never supplies the URL). Otherwise
   * chat falls back to the honest built-in demo agent (Rule 1 — never a fabricated
   * "gateway" reply). Web-dev has no keyring, so this always resolves to the demo.
   * Called on launch and after a provider config change (Settings), so the active
   * provider tracks the config. Never throws — a failed status read keeps the demo.
   */
  async rebuildProvider(): Promise<void> {
    try {
      const statuses = await bridge.chat.providerStatus();
      const def = this.state.aiDefault;
      // BC-3.2 — is a gateway key configured? (used both for the local-fallback
      // routing computed in Rust AND for the gateway leg of the selection here.)
      const gatewayConfigured = statuses.some((p) => p.id === def && p.configured);
      // BC-3.2 — the HONEST local-vs-gateway routing state, computed in Rust from
      // the real model status + serve health. Only "ready" means the local server
      // is actually serving. A failed read (web shim) resolves to "demo" — never a
      // fabricated "ready" (Rule 1).
      let inferenceState = "demo";
      try {
        inferenceState = await bridge.chat.inferenceState(gatewayConfigured);
      } catch {
        inferenceState = "demo";
      }
      const kind = pickChatProviderKind(statuses, def, BRIDGE_MODE, inferenceState);
      if (kind === "local") {
        // REAL local inference against the healthy llama-server (Rust-owned URL).
        this.provider = createLocalProvider(() => this.snapshot(), (msgs, ctx) =>
          bridge.chat.inferLocal(msgs, ctx),
        );
        return;
      }
      if (kind === "real") {
        // W3.3 — the gateway runs the AGENTIC tool loop (memory recall/search +
        // navigate + approval-gated writes). Tool calls execute through handleTool.
        this.provider = createAgentProvider(def, () => this.snapshot(), (pid, msgs, tools, ctx) =>
          bridge.chat.inferTools(pid, msgs, tools, ctx),
        );
        return;
      }
      // No local server + no configured default (or web-dev): the honest demo agent.
      this.provider = createDemoProvider(() => this.snapshot());
    } catch {
      // Honest no-op: providerStatus unavailable (web shim / failed read) — keep
      // the built-in demo agent rather than a fabricated provider.
      this.provider = createDemoProvider(() => this.snapshot());
    }
  }

  /**
   * CORE-AI1 (@rule8) — seal an AI provider's {baseURL, model, apiKey} in the OS
   * keyring via the bridge (Rust binds the key to its https baseURL). The key is
   * handed to Rust ONCE and never stored in AppState/localStorage (aiKeys is gone).
   * On success the id becomes the default route and the provider is rebuilt so chat
   * uses it immediately. Web-dev has no keyring — honest toast, no fake seal.
   */
  async aiSetProvider(providerId: string, baseURL: string, model: string, apiKey: string): Promise<void> {
    if (BRIDGE_MODE !== "tauri") {
      this.toast("Provider keys seal in the OS keyring — desktop app only (no keyring in web preview).");
      return;
    }
    try {
      await bridge.chat.setProvider(providerId, baseURL, model, apiKey);
      this.setState({ aiDefault: providerId, aiEdit: null });
      await this.rebuildProvider();
      this.toast("Provider key sealed in the OS keyring — chat now uses it.");
      this.save();
    } catch (err) {
      this.toast("Could not save the provider — " + String((err as Error).message ?? err));
    }
  }

  /**
   * CORE-AI1 (@rule8) — delete a provider's sealed config via the bridge, then
   * rebuild the provider (falls back to the demo agent if the default was cleared).
   */
  async aiClearProvider(providerId: string): Promise<void> {
    if (BRIDGE_MODE !== "tauri") {
      this.setState({ aiEdit: null });
      return;
    }
    try {
      await bridge.chat.clearProvider(providerId);
      await this.rebuildProvider();
      this.toast("Provider key removed from the keyring.");
      this.save();
    } catch (err) {
      this.toast("Could not remove the provider — " + String((err as Error).message ?? err));
    }
  }

  /** CORE-AI1 — set the default provider route + rebuild (no key involved). */
  async aiSetDefault(providerId: string): Promise<void> {
    this.setState({ aiDefault: providerId });
    await this.rebuildProvider();
    this.save();
  }

  /** Unlock the custody vault, then refresh lock state. */
  async custodyUnlock(passphrase: string): Promise<void> {
    await bridge.custody.unlock(passphrase);
    await this.refreshCustody();
  }

  /**
   * Seamless device-bound unlock (passphrase-less model). Re-provisions + unlocks
   * the vault from the OS-keyring device passphrase — the Settings "Unlock" control
   * and app-launch path both call this. There is no user passphrase to type, so a
   * locked vault (auto-lock or fresh launch) can ONLY be recovered this way; without
   * it the wallet reads gated on the vault appear broken. Best-effort: a reset
   * keychain fails closed and leaves the honest "locked" state.
   */
  async custodyEnsureUnlocked(): Promise<void> {
    try {
      await bridge.custody.ensureUnlocked();
    } catch {
      // Reset keychain / device secret unavailable — fail closed. `refreshCustody`
      // below folds the honest locked state; callers read `store.state.custodyLock`
      // rather than a thrown error (the Settings control toasts off that state).
    } finally {
      await this.refreshCustody();
    }
  }

  /** Lock the custody vault, then refresh lock state. */
  async custodyLock(): Promise<void> {
    await bridge.custody.lock();
    await this.refreshCustody();
  }

  // ---------- CORE-A3 auth (real OIDC in Tauri; sim persona in web-dev) ----
  /**
   * Pull the live claim-derived auth status from the bridge and fold it into the
   * entitlement engine (tier/org/role/entitlement) + the KYC S2 state. In a
   * Tauri build this reads the real /userinfo entitlement claim; in web-dev it
   * reads the sim persona. Never throws — an unavailable/failed read leaves the
   * prior state untouched (honest).
   */
  async refreshAuth(): Promise<void> {
    try {
      const st = await bridge.auth.status();
      this.applyAuthStatus(st);
    } catch {
      /* honest no-op: the auth domain reported unavailable / not signed in */
    }
  }

  /**
   * Re-establish the signed-in session on LAUNCH from the vaulted OIDC refresh
   * token (A3), so a restart keeps the paid tier instead of dropping the member to
   * signed-out → public/free. `bridge.auth.status()` reads only the in-memory
   * session, which is empty on a fresh process; `bridge.auth.refresh()` silently
   * mints a fresh session from the refresh token sealed in the custody vault. That
   * read REQUIRES an unlocked vault (the token is a custody slot), so this must run
   * AFTER `custodyEnsureUnlocked`. A never-signed-in user (no token) or a declined
   * refresh fails gracefully and stays honestly signed out. After a successful
   * refresh we re-check the LIVE entitlement (`authUserinfo`) — the federation RP
   * rule — so the current tier (e.g. a just-raised membership) is folded, not a
   * stale claim.
   */
  async resumeSession(): Promise<void> {
    let refreshed = false;
    try {
      const st = await bridge.auth.refresh();
      this.applyAuthStatus(st);
      refreshed = true;
    } catch {
      // No vaulted refresh token (never signed in) or the refresh was declined —
      // stay signed out honestly rather than fabricate a session.
    }
    // Live /userinfo re-check once the session is back, so the freshest entitlement
    // (tier/expiry) is folded. Skipped when there is no session to re-check.
    // WRAPPED: a rejection here must not skip the chain reconcile below. These are
    // independent — the grant is a permissionless chain read that does not need a
    // live session, and letting an auth hiccup suppress it is exactly how a paid
    // member stays stranded (observed 2026-08-05).
    try {
      if (refreshed) await this.authUserinfo();
      else await this.refreshAuth();
    } catch {
      /* honest no-op: entitlement display may lag; the chain read below still runs */
    }
    // The onboarding step state is LOCAL; the grant is on CHAIN. Re-sync them.
    await this.reconcileOnboardingFromChain();
  }

  /**
   * Fold the REAL on-chain grant into the onboarding step state at launch.
   *
   * WHY THIS EXISTS. `s3`/`s5` were advanced ONLY by `pollMembership`/`pollGrant`,
   * which are in-memory, run solely while their step is the active one, and stop
   * at a ~5-minute cap. Nothing re-read the chain afterwards. So a member who paid
   * and whose treasury grant fully landed — SBT minted, MemberBond clone deployed
   * and funded with 32,000 SALT — was stranded permanently if the app restarted or
   * the cap elapsed during checkout: the chain said "member", the app said `s3:
   * idle / hasGrant: false`, and onboarding could not be completed or re-entered.
   * Observed 2026-08-05 on the first fresh-email E2E (member
   * `0xE14d2F9d…`, bond `0xD9FF524e…`).
   *
   * This is a RECONCILE, not a shortcut: it settles on exactly the same evidence
   * `pollGrant` requires — `isGrantOnChain(grant) && grant.hasSbt`, both live 40204
   * reads — and sets the same fields from the same values. A member without a real
   * grant is untouched, and a read failure is an honest no-op (Rule 1). It never
   * advances S6/activation, which still needs the human ceremony.
   */
  private async reconcileOnboardingFromChain(): Promise<void> {
    if (BRIDGE_MODE !== "tauri") return; // web-dev sim has no chain to reconcile against
    const member = this.identity().wallet;
    if (!member) return; // not signed in / no claim yet
    let grant: Awaited<ReturnType<typeof bridge.membership.grantStatus>>;
    try {
      grant = await bridge.membership.grantStatus(member);
    } catch {
      return; // transient RPC error — keep the last honest state, never fabricate
    }
    // Membership-renew safeguard FIRST — a real on-chain SBT must not be overridden by
    // a thin /userinfo claim into a false "renew". This runs even when onboarding is
    // already settled, because the wrongful lapse happens post-onboarding, on reopen.
    const guard = entitlementSafeguardFromChain(this.state, grant);
    if (guard) {
      this.setState(guard);
      this.save();
    }
    // Onboarding-step reconcile: only needed while the flow is not already settled.
    if (this.state.hasGrant && this.state.s3 === "settled" && this.state.s5 === "settled") return;
    const patch = reconciledGrantPatch(grant);
    if (!patch) return; // no real grant: leave the flow exactly as it was
    this.setState(patch);
    this.save();
  }

  /**
   * CORE (Phase 1) — pull the REAL node vitals from the bridge and fold them into
   * AppState. `bridge.node.status()` (Tauri) hits the supervisor + local node RPC
   * (eth_blockNumber / net_peerCount) — height/peers are live 40204 truth, never
   * fabricated. A user-initiated `paused` is respected (not overwritten). Failure
   * leaves the last honest values untouched — no sim fallback (Rule 1).
   *
   * Q-A.2/Q-B.2 — ALSO folds the REAL streamed node log lines (`bridge.node.logs`
   * → the Rust `node_logs` ring of stdout+stderr) into `s.logs`, so the Node LOG
   * panel shows live node output in a packaged build. This is the REAL stream —
   * the fabricated NODE_LOG_TEMPLATES path is web-dev/sim ONLY (it lives inside
   * the `BRIDGE_MODE === "sim"` branch of `tick()` and never runs here). A stopped
   * node returns [] honestly; a failed logs read leaves the last real tail.
   */
  async refreshNode(): Promise<void> {
    if (this.state.node === "paused") return; // respect an explicit pause
    if (this.nodeRefreshing) return; // in-flight guard — skip this 2s tick if the last is still running
    this.nodeRefreshing = true;
    try {
      const st = await bridge.node.status();
      // REAL validator bond (SALT) from the ValidatorRegistry (pubkeyOfStaker(bond)
      // → stakeOf, via grantStatus, keyed on the member's MemberBond clone). This —
      // NOT `hasGrant` — is what makes a member a validator: the membership grant
      // deploys + funds the clone (bond-clone model, 2026-08-05), and the member
      // activates it via `MemberBond.activate`, whose clone then bonds into the
      // registry. Reading `hasGrant ? 32000` here was the Rule-1 lie that let a
      // funded-but-unactivated member read as "validating" with 0 actually bonded.
      // Read the bond only while the node is running and we are still WAITING for it
      // (< 32k); once bonded the value is stable, so we stop re-reading it every 2s.
      let bonded = this.state.bondedStake;
      // THROTTLED. `grantStatus` is NINE sequential public-RPC eth_calls (~2.4 s of
      // network). refreshNode ticks every 2 s, so reading it every tick meant the
      // reads overlapped continuously — 27 calls/min per member against the shared
      // 40204 RPC, for a value that only changes once (at activation). Re-read at
      // most every BOND_READ_MS; the vitals fold below still runs every tick off the
      // fast LOCAL node RPC (~0.3 ms), so height/peers/sync stay live.
      const BOND_READ_MS = 15_000;
      const dueForBondRead = Date.now() - this.lastBondReadAt >= BOND_READ_MS;
      // Read grantStatus (public 40204 RPC, node-independent) when we still need the validator bond
      // OR when the attributed grant hasn't been reconciled yet. The latter is the fix for a grant
      // signed THIS session: the launch-time reconcile (reconcileOnboardingFromChain) ran BEFORE the
      // grant existed, and this was the only other read — but it folded ONLY the validator bond (0
      // pre-activation), so the 32k attributed stake never surfaced until a relaunch.
      let grantPatch: Partial<AppState> | null = null;
      const needBond = st.state === "running" && bonded < 32000;
      const needGrant = !this.state.hasGrant; // attributed grant not yet reconciled this session
      if ((needBond || needGrant) && dueForBondRead) {
        this.lastBondReadAt = Date.now();
        try {
          const g = await bridge.membership.grantStatus(this.identity().wallet);
          bonded = g.bondedStakeWei ? Number(BigInt(g.bondedStakeWei)) / 1e18 : 0;
          // Surface the ATTRIBUTED grant (hasGrant + s5StakeWei=32k + bond status incl. the unlock
          // block), DISTINCT from the validator bond above — so a granted-but-not-yet-activated member
          // sees their locked 32k, not "0 staked". Null when the grant isn't genuinely on chain (Rule 1).
          grantPatch = reconciledGrantPatch(g);
        } catch {
          /* honest no-op: keep the last real values, never fabricate */
        }
      }
      const staked = bonded + this.state.selfStake;
      // RE-ARM AFTER RESTART. `validatorArmed` is in-memory, and arming is what
      // respawns the node with `--mine --coinbase`. So an ALREADY-BONDED validator
      // came back from every app restart as a plain follower: synced, healthy, and
      // silently producing nothing — the member only finds out by noticing rewards
      // never move (observed 2026-08-06 after a reinstall, with 32,000 SALT bonded
      // and proposer.key already minted).
      //
      // `arm_mining_if_synced` re-checks the tip and no-ops without a coinbase or
      // when behind, so this is safe to attempt on the poll; it stops once armed.
      if (!this.validatorArmed && bonded >= 32000 && (st.state === "running" || st.state === "starting")) {
        void bridge.node
          .armMining()
          .then((armed) => {
            if (armed) {
              this.validatorArmed = true;
              this.toast("Block production re-armed — your validator is producing again.");
            }
          })
          .catch(() => {
            /* honest no-op: surfaced by the node vitals, never a fabricated arm */
          });
      }
      const patch: Partial<AppState> = {
        height: st.height,
        peers: st.peers,
        syncPct: st.syncPct,
        bondedStake: bonded,
        // There is NO real finality/checkpoint-age source from the node yet
        // (WO-1 adds citrate_getDagStats). Mark it unavailable (< 0) rather than
        // leaving the fabricated seed value — displays render "—" (Rule 1).
        finAge: -1,
      };
      // Merge the reconciled grant (hasGrant + s5StakeWei + s5BondStatus) so the 32k attributed
      // stake surfaces on the Dashboard/Wallet tiles + the model context live, without a relaunch.
      // The grant surfaces THIS tick if grantPatch sets hasGrant and it wasn't set before — the
      // launch-time memory seed ran before the grant reconciled, so re-seed the personal/stake facts.
      const grantJustSurfaced = !!grantPatch && !this.state.hasGrant;
      if (grantPatch) Object.assign(patch, grantPatch);
      // While a start is in flight, a transient "stopped" poll (supervisor not
      // yet registered during spawn) must NOT demote the optimistic "prov" back
      // to "off" — that caused a prov→off→prov flicker. Still fold height/peers.
      // Sync-stall overlay (honest, Rule 1): the node can report "running" while its block-import
      // pipeline is wedged — height FROZEN, still behind the tip, peers connected (a known chain-side
      // wedge). Surface a distinct "stalled" state rather than a "syncing…" that never advances.
      // Detection lives here (we own the per-tick height); recovery is in the 30s watchdog. Peers==0
      // is a connectivity problem, not a stall — left as syncing.
      let nextNode = mapNodeState(st.state, st.syncPct, staked);
      if (nextNode === "syncing") {
        if (st.height > this.nodeStallLastHeight) {
          this.nodeStallLastHeight = st.height; // advanced — reset the freeze window
          this.nodeStallSince = 0;
        } else if (st.peers > 0) {
          const NODE_STALL_MS = 60_000; // height frozen this long while behind + peered = stalled
          if (this.nodeStallSince === 0) this.nodeStallSince = Date.now();
          else if (Date.now() - this.nodeStallSince >= NODE_STALL_MS) nextNode = "stalled";
        }
      } else if (nextNode === "synced" || nextNode === "validating") {
        // Fully caught up — clear the entire stall budget (a fresh session on any later dip).
        this.nodeStallLastHeight = -1;
        this.nodeStallSince = 0;
        this.nodeStallCycles = 0;
        this.nodeStalledNotified = false;
      }
      if (!this.nodeStarting) patch.node = nextNode;
      // Q-A.2/Q-B.2 — fold the REAL streamed node log tail. Best-effort: a failed
      // logs read must not clobber the vitals fold above, so it is caught
      // separately (the last real tail stands). NEVER a fabricated template here.
      try {
        const rawLogs = await bridge.node.logs();
        patch.logs = foldNodeLogs(rawLogs);
      } catch {
        /* honest no-op: keep the last real log tail, never a fabricated one */
      }
      this.setState(patch);
      // Re-seed the memory graph's personal/stake facts the moment the grant first surfaces (the
      // launch-time seed ran before the grant reconciled, so the personal tenant was skipped then).
      if (grantJustSurfaced && this.state.memDaemon === "running") void this.seedMemoryGraph();
      // W1.3 — auto-initiate the validator bond activation during onboarding S6 once
      // the node is running (proposer.key minted) and the member is granted + linked
      // but not yet activated. Rule 3 forbids auto-approve, so this only OPENS the
      // wallet review; the human still approves the MemberBond.activate ceremony once.
      this.maybeAutoBond();
    } catch {
      /* honest no-op: a failed poll keeps the last real values, never a sim number */
    } finally {
      this.nodeRefreshing = false;
    }
  }

  /**
   * W1.3 (@rule8 · Rule 3) — auto-open the validator bond-activation ceremony during
   * onboarding S6. Under the bond-clone model (2026-08-05) the membership grant
   * deploys + funds the member's `MemberBond` clone with the 32k; the member must
   * then ACTIVATE it via `MemberBond.activate(pubkey, sig)` on the clone (from their
   * own EOA, the clone's `onlyMember`), which makes the clone bond into the registry.
   * Until activation the 32k just sits in the clone and the node is never a real
   * validator (the gap the pre-fix flow left open). This fires the EXISTING
   * `activateValidator` path — it builds the ceremony and STOPS for explicit human
   * approval (no auto-approve).
   *
   * Fires at most once per session (`autoBondAttempted`): if the member dismisses
   * the review, S6 keeps its manual "bond" affordance rather than re-opening the
   * modal every 2s. Guards fail closed — a not-ready node / unlinked wallet / not-yet-
   * granted member simply does not trigger (and `node_register_validator`'s own
   * on-chain guards — bond deployed + funded, not already activated — fail closed
   * again behind it).
   */
  private maybeAutoBond(): void {
    if (BRIDGE_MODE !== "tauri") return;
    const s = this.state;
    if (s.stage !== "s6") return; // only during node ignition; the dashboard has its own button
    if (s.bondedStake >= 32000) return; // already a bonded validator — nothing to do
    if (!this.walletIsLinked()) return; // S4: the EOA must be the member's wallet_address
    if (!s.hasGrant) return; // the grant must have deployed + funded the bond clone (settled S5)

    // STEP 1 — ARM the producer so the node mints `proposer.key` (the validator
    // identity activation needs). The node runs as a plain follower and NEVER mints
    // the key on its own; arming respawns it with `--mine --coinbase`, which does.
    // Gate on the node being "synced" (mapNodeState requires syncPct==100) — and
    // `arm_mining_if_synced` re-checks the network tip on the backend, so a premature
    // call is a safe no-op. Retry each 2s poll until it actually arms (returns true);
    // only THEN advance to activation, so we never open a doomed ceremony against a
    // missing key.
    if (!this.validatorArmed) {
      if (s.node === "synced" || s.node === "validating") {
        void bridge.node.armMining().then((armed) => {
          if (armed) this.validatorArmed = true;
        });
      }
      return; // wait for the arm + the proposer.key mint before activating
    }

    // STEP 2 — the producer is armed and proposer.key is (being) minted. Auto-open
    // the MemberBond.activate ceremony. `activateValidator` returns false WITHOUT
    // consuming the attempt if the key isn't on disk yet (respawn still in flight),
    // so the poll retries until it opens. Rule 3: this only OPENS the review — the
    // human approves once.
    if (s.walletReview) return; // a review is already open
    if (this.autoBondAttempted) return; // opened once this session
    void this.activateValidator().then((opened) => {
      if (opened) this.autoBondAttempted = true;
    });
  }

  /**
   * CORE — pull REAL wallet balances (native `liquid` SALT via eth_getBalance +
   * the real `claimable` via ContributionAccounting + the real SELF-stake via
   * LiquidStakingPool.balanceOf) and fold them into AppState. `staked` from the
   * bridge is the self-stake only; the displayed staked = (grant ? 32000 : 0) +
   * selfStake, so we fold it into `selfStake`. Failure leaves the last honest
   * values untouched — no fabricated number (Rule 1).
   */
  async refreshWallet(): Promise<void> {
    try {
      const b = await bridge.wallet.balances();
      const patch: Partial<AppState> = { liquid: b.liquid };
      // The address that actually signs. Folded so the UI can tell whether this
      // device's wallet is the one the authority serves as `wallet_address` —
      // if they differ, a validator bond would be paid somewhere unspendable.
      if (b.address) patch.custodyAddr = b.address;
      if (b.claimable >= 0) {
        patch.claimable = b.claimable;
        // Only claim a "chain" data source for a REAL read (Tauri). In web-dev the
        // sim bridge echoes AppState, so labeling it chain-sourced would be a
        // fabricated caption (Rule 1).
        if (BRIDGE_MODE === "tauri") patch.earnSource = "chain";
      }
      // b.staked >= 0 ⇒ a grounded self-stake read (balanceOf) — fold it. The UI
      // adds the vaulted grant on top (hasGrant ? 32000 : 0). A negative value
      // would mean "not grounded" — keep the local value (defensive; the real and
      // sim bridges both return >= 0 today).
      if (b.staked >= 0) patch.selfStake = b.staked;
      this.setState(patch);
    } catch {
      /* honest no-op */
    }
  }

  /**
   * CORE item 4 — pull the wallet's REAL indexed 40204 tx history from CitrateScan
   * and fold it into `state.activity`. `bridge.wallet.activity()` (Tauri) reads the
   * public `txlist` endpoint (data source: citrate-explorer /api/v1); the sim shim
   * echoes the prototype `s().activity`. A fresh address / not-provisioned index
   * honestly reads [] — never fabricated (Rule 1). A failure leaves the last honest
   * list untouched (no fabricated rows).
   *
   * Optimistic just-sent entries (addActivity) are DEDUPED by hash against the
   * fetched list: any local row whose hash now appears in the real indexed list is
   * dropped in favour of the canonical indexed entry, so a settled tx never shows
   * twice.
   */
  async refreshActivity(): Promise<void> {
    try {
      const real = await bridge.wallet.activity();
      this.setState((s) => ({ activity: mergeActivity(real, s.activity) }));
    } catch {
      /* honest no-op: keep the last real/optimistic list, never a fabricated one */
    }
  }

  /** Fold a claim-derived AuthStatus into AppState (the entitlement engine). */
  private applyAuthStatus(st: {
    signedIn: boolean;
    sub?: string | null;
    email?: string | null;
    walletAddr?: string | null;
    tier: string | null;
    org: string | null;
    role: string | null;
    kycStatus: string | null;
    expiresAt?: string | null;
  }): void {
    if (!st.signedIn) return;
    const patch: Partial<AppState> = {};
    // Fold the REAL identity so no prototype persona ever surfaces to a signed-in
    // user (Rule 1). The authority issues no display-name claim, so derive a
    // display name + initials from the email local part.
    patch.signedIn = true;
    patch.authSub = st.sub ?? null;
    patch.authEmail = st.email ?? null;
    if (st.email) {
      const d = deriveIdentityFromEmail(st.email);
      patch.authName = d.name;
      patch.authInitials = d.initials;
    }
    // Q-A.4b item 10 — fold the REAL wallet only from a genuine wallet_address
    // claim, and RECORD that provenance. A signed-in user with no wallet claim
    // keeps walletFromClaim=false, so the S4 onboarding card shows "—"/pending
    // rather than the fabricated persona `makeAddr` hash (Rule 1).
    if (st.walletAddr) {
      patch.walletAddr = st.walletAddr;
      patch.walletFromClaim = true;
    } else {
      patch.walletFromClaim = false;
    }
    // BC-6.3: fold the REAL entitlement expiry verbatim (the Settings billing card
    // renders THIS, an honest "—" when absent — never a hardcoded date). Folded in
    // every branch below, including the expired one (it explains WHY the tier
    // lapsed). `??` normalizes undefined → null.
    patch.authExpiresAt = st.expiresAt ?? null;
    // A3-03: enforce the entitlement expiry at the DECISION point — a claim whose
    // `expiresAt` is in the past is downgraded to the free tier + lapsed, so no
    // gated surface stays unlocked on a stale claim. A valid future expiry keeps
    // the claimed tier and marks the entitlement active.
    if (isExpiredClaim(st.expiresAt)) {
      patch.tier = "free";
      patch.entitlement = "lapsed";
    } else if (st.tier) {
      patch.tier = st.tier;
      patch.entitlement = "active";
    } else {
      // Signed in but the authority granted NO tier — fail closed to free so an
      // ungranted session can never inherit a prior session's persisted paid
      // tier (tier/entitlement are persisted keys).
      patch.tier = "free";
      patch.entitlement = "lapsed";
    }
    patch.org = st.org;
    // The authority may omit the citrate_role claim for a plain member; a
    // signed-in paid account is a "member" by default (never the persona's role).
    patch.citrateRole = st.role || "member";
    // ALF cooperative membership is a CLAIM (DGX_HANDOFF §4.3): alf_member is a
    // citrate_role minted by the authority, never tier-derived. Gates the ALF surface.
    patch.alfMember = patch.citrateRole === "alf_member";
    // KYC claim → the S2 seam's five states (none/pending/verified/failed/review).
    if (st.kycStatus === "verified") patch.s2 = "verified";
    else if (st.kycStatus === "pending") patch.s2 = "pending";
    else if (st.kycStatus === "failed") patch.s2 = "failed";
    else if (st.kycStatus === "review") patch.s2 = "review";
    this.setState(patch);
  }

  /** Run the real loopback-PKCE sign-in (Tauri), then fold in the claim. */
  async authLogin(): Promise<void> {
    const st = await bridge.auth.login();
    this.applyAuthStatus(st);
    // The login result may carry only id_token claims (no email/wallet). Fold
    // the live /userinfo immediately so the real identity (email/wallet/tier)
    // is complete on the first render — never a persona fallback for a signed-in
    // user. Best-effort: keep the login-folded state if /userinfo is unavailable.
    try {
      await this.authUserinfo();
    } catch {
      /* honest no-op — the login-folded claim stands */
    }
    this.save();
  }

  /** Live /userinfo entitlement re-check, folded into the entitlement engine. */
  async authUserinfo(): Promise<void> {
    const st = await bridge.auth.userinfo();
    this.applyAuthStatus(st);
  }

  /** Sign out: revoke + clear the vault slot + wipe the session, then reset. */
  async authLogout(): Promise<void> {
    await bridge.auth.logout();
    this.setState({
      tier: "free",
      // Coherent free/no-membership state — matches the expired-claim path
      // (tier:free + entitlement:lapsed); leaving a prior "active" would render a
      // false membership badge on a signed-out/free account (Rule 1).
      entitlement: "lapsed",
      citrateRole: "member",
      alfMember: false, // clear the ALF flag on sign-out — a signed-out account is not an ALF member (Rule 1)
      org: null,
      signedIn: false,
      authSub: null,
      authEmail: null,
      authName: null,
      authInitials: null,
      walletFromClaim: false,
    });
    this.save();
  }

  /** Open KYC in the browser; S2 status then arrives via userinfo polling. */
  async kycStart(): Promise<void> {
    // Same hazard as "Manage account", but on the compliance path: `kyc_start`
    // opened a BARE authority URL, and the authority resolves identity from the
    // BROWSER's cookie. A member whose browser held a different Citrate session
    // was sent to verify THAT account — the exact failure seen 2026-08-06, where a
    // test member's KYC opened the admin's already-verified account.
    //
    // Route it through the identity-aware opener so we at least hint the intended
    // account and say plainly whose page this is. A server-side authenticated
    // hand-off is the real fix (see handoffs/IDENTITY_ACCOUNT_HANDOFF_2026-08-06).
    await this.openAuthorityPage("https://auth.citrate.ai/kyc/start", "identity verification");
  }

  /**
   * Open an external federation link (Almanac docs/tutorials, CitrateScan, a service
   * webapp) in the system browser. `url` must be https. The destination RP runs
   * its own OIDC login today; a real signed-in handoff (shared-authority SSO) is
   * work-order WO-6. Honest toast on failure — never a silent dead click.
   */
  async openExternal(url: string): Promise<void> {
    try {
      await bridge.shell.openExternal(url);
    } catch (err) {
      this.toast("Could not open the link — " + String((err as Error).message ?? err));
    }
  }

  /**
   * Open an AUTHORITY page (account, KYC) for the account THIS APP is signed in as.
   *
   * The app's session lives in its own vault; the system browser has a SEPARATE
   * cookie session. `auth.citrate.ai` resolves identity from the BROWSER cookie
   * (`/kyc/start` reads `provider.Session.get`), so opening a bare URL hands the
   * member whichever account their browser happens to hold:
   *
   *   - no browser session  -> 401 dead end
   *   - a DIFFERENT account -> the page silently opens as that other member
   *
   * Observed 2026-08-06: signed into the app as a test member, "Manage account"
   * opened the ADMIN account. On the KYC path that is not cosmetic — it is
   * verifying the wrong identity on a compliance-and-money flow.
   *
   * The app cannot assert its identity to the authority today: `/kyc/start` takes
   * only `level` and `return_to`, with no bearer and no subject. The real fix is a
   * server-side authenticated hand-off (a one-time, sub-bound link minted with the
   * app's access token). Until that exists we do the two things we honestly can:
   * pass `login_hint` so a browser with NO session lands on the right account, and
   * NAME the account we intend, so a mismatch is visible rather than silent.
   */
  async openAuthorityPage(url: string, what: string): Promise<void> {
    const email = this.state.authEmail || "";
    let target = url;
    if (email) {
      const u = new URL(url);
      // Helps only when the browser has no session; it cannot override one.
      u.searchParams.set("login_hint", email);
      target = u.toString();
    }
    this.toast(
      email
        ? `Opening ${what} for ${email}. If your browser is signed into a different Citrate account, sign out there first — the page follows the browser's session, not the app's.`
        : `Opening ${what} in your browser.`,
    );
    await this.openExternal(target);
  }

  /**
   * Open the REAL core-membership checkout (Settings → Billing "Renew"). Uses the
   * same wired path as onboarding S3 (bridge.membership.checkout → the checkout
   * popup). Web-dev has no popup — honest message. Entitlement refresh then
   * arrives via the normal /userinfo poll after settlement.
   */
  async renewMembership(): Promise<void> {
    if (BRIDGE_MODE !== "tauri") {
      this.toast("Renewal opens the checkout in the desktop app.");
      return;
    }
    try {
      // Pass the signed-in email as login_hint so the browser checkout targets THIS account.
      await bridge.membership.checkout(this.state.authEmail ?? undefined);
    } catch (err) {
      this.toast("Could not open checkout — " + String((err as Error).message ?? err));
    }
  }
  stop(): void {
    if (this.timer) clearInterval(this.timer);
    this.timer = null;
    if (this.nodeTimer) clearInterval(this.nodeTimer);
    this.nodeTimer = null;
    if (this.nodeWatchdog) clearInterval(this.nodeWatchdog);
    this.nodeWatchdog = null;
  }

  // ---------- helpers ----------
  persona(): Persona {
    return PERSONAS[this.state.persona] || PERSONAS.p1;
  }

  /**
   * The identity every surface renders — the REAL signed-in user when a live
   * session exists, else the sim persona (web-dev only). This is the single
   * seam that stops prototype identity (Dana Okafor et al.) from ever showing
   * to a real, signed-in account. Name/initials derive from the email local
   * part because the authority issues no display-name claim; the wallet, sub,
   * email, tier, role and org come straight from the folded /userinfo claims.
   */
  identity(): {
    name: string;
    initials: string;
    email: string;
    sub: string;
    wallet: string;
    tier: string;
    role: string;
    org: string | null;
    real: boolean;
  } {
    const s = this.state;
    if (s.signedIn) {
      // A signed-in user NEVER renders the sim persona (Rule 1) — even if
      // /userinfo has not folded the email yet or failed. Fall back to a neutral
      // real placeholder derived from the claims we do have, never Dana et al.
      const email = s.authEmail || "";
      return {
        name: s.authName || email || "Member",
        initials: s.authInitials || (email ? email.slice(0, 2).toUpperCase() : "M"),
        email: email || "—",
        sub: s.authSub || "—",
        wallet: s.walletAddr,
        tier: s.tier,
        role: s.citrateRole || "member",
        org: s.org,
        real: true,
      };
    }
    // Not signed in. The sim persona (Dana Okafor et al.) is a WEB-DEV preview
    // affordance ONLY (state.ts) and must NEVER render in the packaged app. In a
    // Tauri build a not-signed-in view (e.g. "Explore free" before auth) shows a
    // neutral, obviously-not-real Guest — never the named mock (Rule 1).
    if (BRIDGE_MODE === "tauri") {
      return {
        name: "Guest",
        initials: "G",
        email: "—",
        sub: "—",
        wallet: s.walletAddr,
        tier: s.tier,
        role: s.citrateRole || "member",
        org: s.org,
        real: false,
      };
    }
    const P = this.persona();
    return {
      name: P.name,
      initials: P.initials,
      email: P.email,
      sub: "usr_2af4c19e…" + P.initials.toLowerCase(),
      wallet: s.walletAddr,
      tier: s.tier,
      role: s.citrateRole || P.role,
      org: s.org,
      real: false,
    };
  }

  save(): void {
    if (this._saveT) clearTimeout(this._saveT);
    this._saveT = setTimeout(() => {
      const s = this.state;
      const keep: Record<string, unknown> = {};
      for (const k of PERSIST_KEYS) keep[k] = s[k];
      try {
        localStorage.setItem(STORAGE_KEY, JSON.stringify(keep));
      } catch {
        /* ignore */
      }
    }, 400);
  }

  snapshot() {
    const s = this.state;
    // The membership grant stakes SALT into a time-locked (≈1yr), recoverable validator bond. That
    // stake is REAL and locked but is NOT the same as the validator bond (which is 0 until the member
    // approves the activation ceremony). The model must be able to reconcile "0 spendable but staked",
    // so expose both figures + the human-readable bond status (which carries the unlock block), rather
    // than the old single `staked` that only reflected the validator bond and read 0 for a fresh member.
    const grantStakedSalt = s.hasGrant && s.s5StakeWei ? Math.round(Number(BigInt(s.s5StakeWei)) / 1e18) : 0;
    return {
      height: s.height,
      peers: s.peers,
      finalityAge: Math.round(s.finAge),
      nodeState: nodeLabelLocal(s.node),
      // "staked" now reflects the locked membership grant (falling back to the validator bond), so the
      // model no longer sees 0 for a granted member.
      staked: (grantStakedSalt || s.bondedStake) + s.selfStake,
      liquid: s.liquid,
      claimable: s.claimable,
      earningsToday: s.earnToday,
      walletAddr: s.walletAddr,
      tier: s.tier,
      membership: {
        grantStakedSalt, // the recoverable grant, locked (e.g. 32000)
        bondStatus: s.s5BondStatus || (s.hasGrant ? "granted" : "none"), // e.g. "Staked · unlocks at block N"
        validatorBondedSalt: s.bondedStake, // 0 until the member approves the activation ceremony
        locked: grantStakedSalt > 0 && s.bondedStake < grantStakedSalt,
        note: "The membership grant stakes SALT into a time-locked (about a year), recoverable validator bond. Locked stake is not spendable and is separate from the liquid balance; the bond only starts validating after the member approves the activation ceremony.",
      },
    };
  }

  toast(text: string): void {
    if (this._toastT) clearTimeout(this._toastT);
    this.setState({ toast: text });
    this._toastT = setTimeout(() => this.setState({ toast: null }), 3000);
  }
  copy(text: string, note?: string): void {
    try {
      if (navigator.clipboard) navigator.clipboard.writeText(text);
    } catch {
      /* ignore */
    }
    this.toast(note || "Copied");
  }
  /**
   * CONNECT-S0 — build the People directory from LIVE data: every group's roster + verified faces,
   * excluding your own comms address. Pure aggregation (`buildPeopleDirectory`) over real bridge reads
   * — never fabricated. Honest states: "loading" while reading, "ready" with the derived list (possibly
   * empty), "unavailable" if groups can't be read. A face read failure degrades to addresses, not fake
   * names.
   */
  async refreshPeople(): Promise<void> {
    if (this.peopleRefreshing) return; // in-flight guard — don't stack concurrent refreshes
    this.peopleRefreshing = true;
    if (this.state.peopleState !== "ready") this.setState({ peopleState: "loading" });
    try {
      const groups = await bridge.groups.list();
      const rosterByGroup: Record<string, { address: string; role: GroupRole }[]> = {};
      const addrs = new Set<string>();
      // Read every group's roster IN PARALLEL, not one-after-another — a group whose roster won't
      // read contributes nothing, honestly. This turns (1 + N) serial daemon round-trips into 1 + N
      // concurrent, so many groups or a slow daemon can't serialize into a long stall.
      const rosters = await Promise.all(
        groups.map((g) =>
          bridge.groups
            .roster(g.id)
            .then((roster) => ({ id: g.id, roster: roster.map((m) => ({ address: m.address, role: m.role })) }))
            .catch(() => ({ id: g.id, roster: [] as { address: string; role: GroupRole }[] })),
        ),
      );
      for (const r of rosters) {
        rosterByGroup[r.id] = r.roster;
        r.roster.forEach((m) => addrs.add(m.address));
      }
      let self = "";
      try {
        self = await bridge.groups.selfAddress();
      } catch {
        self = (this.identity().wallet || this.state.walletAddr || "").toLowerCase(); // seam fallback (S5 closes it)
      }
      let faces: { address: string; network: string; handle: string }[] = [];
      try {
        faces = await bridge.social.resolve([...addrs]);
      } catch {
        /* faces unavailable → people render as addresses, never invented names */
      }
      const people = buildPeopleDirectory(
        groups.map((g) => ({ id: g.id, name: g.name })),
        rosterByGroup,
        faces,
        self,
      );
      // CONNECT-S4 — the role navigator is derived in the SAME pass from the same rosters + self.
      // Session names (groups created/renamed this session) win over the DTO name to match the Groups
      // rail; those ids are also manage-capable pre-roster (the iCreated seam), keyed on comms self.
      const names = groupsSlice.get().names;
      const myGroups = buildRoleNavigator(
        groups.map((g) => ({ id: g.id, name: g.name, kind: g.kind })),
        rosterByGroup,
        self,
        names,
        Object.keys(names),
      );
      this.setState({ people, myGroups, peopleState: "ready" });
    } catch {
      this.setState({ people: [], myGroups: [], peopleState: "unavailable" });
    } finally {
      this.peopleRefreshing = false;
    }
  }

  go(route: string): void {
    this.setState({ route });
    try {
      if ((location.hash || "").replace(/^#\//, "") !== route) location.hash = "#/" + route;
    } catch {
      /* ignore */
    }
    this.save();
  }

  /**
   * GROW-S1 — handle a `citrate://join/...` deep-link handed to the app by the OS (the one-tap
   * cold-start hand-off from the web join page). Parses the display context, stashes it as
   * `pendingInvite`, and routes to Groups, which surfaces it and — for a full invite-token link —
   * redeems it. Tolerant of a `citrate://` OR `https://citrate.ai/join/...` URL (parseJoinLink handles
   * both). A malformed URL is ignored (no crash, no fabricated invite — Rule 1).
   */
  handleDeepLink(url: string): void {
    if (!url) return;
    // PHONEPAY-S4 — a `citrate://claim?…` onboarding link takes precedence over the
    // join/invite handling below. The params are untrusted HINTS ONLY (A8): we stash
    // the email + order hint to steer sign-in and detect a wrong-account mismatch, land
    // the buyer in onboarding, and let the authority re-auth + server reconciler do the
    // actual admission. NOTHING here grants a membership.
    const claim = parseClaimLink(url);
    if (claim) {
      this.setState({ pendingClaim: { email: claim.email ?? null, orderHint: claim.order ?? null, raw: url } });
      if (!this.state.signedIn) {
        // Land at sign-in so the seat binds to the right identity. The App auth-gate
        // permits s0/s1 for a signed-out session; anything past sign-in still requires
        // a live authority session, so a claim link can never skip auth.
        this.setState({ stage: "s1", s1: "idle" });
        this.go("onboarding");
      } else {
        // Already signed in: don't move a member backward. If the paid-with email
        // doesn't match this session, say so plainly; otherwise reassure that the
        // grant lands server-side. The mismatch banner also renders in onboarding.
        const mm = this.claimAccountMismatch();
        if (mm) {
          this.toast(`You paid as ${mm.paidAs}, but you're signed in as ${mm.signedInAs}. Sign in with the account you paid with to claim this membership.`);
        } else {
          this.toast("Signed in — your membership will appear here once the grant lands on chain.");
        }
      }
      this.save();
      return;
    }
    const parts = parseJoinLink(url);
    // GROW-S1b — an opaque short code: resolve + VERIFY the EdDSA signature before trusting anything,
    // then show the invite. Land on Groups immediately with a "resolving…" banner so it's responsive.
    if (parts.code) {
      this.setState({ pendingInvite: { url, resolving: true } });
      this.go("groups");
      void resolveJoinCode(parts.code)
        .then((r) =>
          this.setState({
            pendingInvite: { url, clusterId: r.clusterId, clusterName: r.clusterName, inviterHandle: r.inviterHandle, goal: r.goal },
          }),
        )
        .catch(() => this.setState({ pendingInvite: { url, unresolved: true } }));
      return;
    }
    // Self-contained link — act only if it's join-shaped (has a cluster or referrer); else ignore.
    if (!parts.clusterId && !parts.inviter && !parts.inviterHandle) return;
    this.setState({
      pendingInvite: {
        url,
        clusterId: parts.clusterId,
        clusterName: parts.clusterName,
        inviterHandle: parts.inviterHandle,
        goal: parts.goal,
      },
    });
    this.go("groups");
  }

  /** Clear the pending deep-link invite once Groups has consumed it. */
  clearPendingInvite(): void {
    this.setState({ pendingInvite: null });
  }

  /** Clear the pending claim hint once onboarding has surfaced it (or on sign-out). */
  clearPendingClaim(): void {
    this.setState({ pendingClaim: null });
  }

  /**
   * PHONEPAY-S4 — the wrong-account guard. When a claim link carried a paid-with email
   * and the user is signed in as a DIFFERENT identity, return the two emails so the UI
   * can say "you paid as X, you're signed in as Y". Null when there's no claim, no email
   * hint, we're not signed in yet, or the accounts match (case/space-insensitive). This
   * is a DISPLAY safeguard only — it never blocks the authority, which is the real gate.
   */
  claimAccountMismatch(): { paidAs: string; signedInAs: string } | null {
    const paid = this.state.pendingClaim?.email?.trim().toLowerCase();
    if (!paid || !this.state.signedIn) return null;
    const here = (this.state.authEmail ?? "").trim().toLowerCase();
    if (!here || here === paid) return null;
    return { paidAs: this.state.pendingClaim!.email!.trim(), signedInAs: this.state.authEmail!.trim() };
  }

  // ---------- sim tick (verbatim logic) ----------
  tick(): void {
    this.setState((s) => {
      const u: Partial<AppState> = {};
      const running = s.node !== "off" && s.node !== "prov";
      // NODE VITALS SIMULATION — web-dev ONLY. In a Tauri build height/peers/
      // syncPct/node-state come from the REAL node via `refreshNode`
      // (bridge.node.status → node.rs → local RPC eth_blockNumber/net_peerCount),
      // and cpu/ram/logs/peer-rows are honestly blank rather than fabricated
      // (Rule 1). This whole block is the source of the implausible always-
      // climbing height + steady ~24 peers a packaged build must never show.
      if (BRIDGE_MODE === "sim") {
        u.height = s.height + (Math.random() < 0.85 ? 1 : 2);
        u.finAge = s.finAge + 0.6;
        if (u.height - s.lastCp >= 50) {
          u.lastCp = u.height;
          u.finAge = 0;
        }
        const target = running ? 24 : 0;
        let peers = s.peers;
        if (peers < target) peers += Math.ceil(Math.random() * 3);
        else if (peers > target) peers -= Math.ceil(Math.random() * 4);
        else if (running && Math.random() < 0.15) peers += Math.random() < 0.5 ? 1 : -1;
        u.peers = Math.max(0, Math.min(32, peers));
        if (s.node === "syncing") {
          const np = Math.min(100, s.syncPct + 5 + Math.random() * 9);
          u.syncPct = np;
          if (np >= 100) {
            // SIM-ONLY preview: the sim has no real ValidatorRegistry, so it treats
            // a granted member's principal as the bond and folds it into
            // `bondedStake` too, keeping the fabricated node state and the
            // `snapshot().staked` vitals (= bondedStake + selfStake) coherent. The
            // REAL tauri path reads the genuine registry bond in refreshNode.
            const simBond = s.hasGrant ? 32000 : 0;
            u.bondedStake = simBond;
            u.node = simBond + s.selfStake >= 32000 ? "validating" : "synced";
            if (s.stage === "s6") u.s6ready = true;
          }
        }
        if (s.node === "validating" && Math.random() < 0.018) u.blocksProposed = s.blocksProposed + 1;
        if (s.node === "validating") {
          const dv = 0.004 + Math.random() * 0.005,
            dp = 0.0009 + Math.random() * 0.0006,
            dc = 0.0004 + Math.random() * 0.0004;
          u.earnVal = s.earnVal + dv;
          u.earnPin = s.earnPin + dp;
          u.earnComp = s.earnComp + dc;
          u.earnToday = s.earnToday + dv + dp + dc;
          u.claimable = s.claimable + (dv + dp + dc) * 0.85;
        }
        if (running) {
          u.hb = (s.hb + 0.6) % 30;
          u.cpu = s.node === "paused" ? 2 + Math.random() * 2 : (s.node === "validating" ? 18 : 11) + Math.random() * 12;
          u.ram = (s.node === "validating" ? 780 : 540) + Math.random() * 140;
          if (Math.random() < 0.55) {
            const T = NODE_LOG_TEMPLATES;
            const line = T[(Math.random() * T.length) | 0]
              .replace(/\{h\}/g, String(u.height))
              .replace(/\{peers\}/g, String(u.peers))
              .replace(/\{r\}/g, String((u.height! / 50) | 0));
            const d = new Date();
            const t =
              String(d.getHours()).padStart(2, "0") + ":" + String(d.getMinutes()).padStart(2, "0") + ":" + String(d.getSeconds()).padStart(2, "0");
            u.logs = s.logs.concat([{ t, line, id: this.mid++ }]).slice(-14);
          }
          if (!s.peerRows.length || (u.height! % 60 === 0)) u.peerRows = this.makePeers(u.peers!);
        }
      }
      if (s.s5 === "settling") {
        // BC-1.3: the counter is a PURE animation now — it NEVER settles S5 or sets
        // hasGrant/hasSbt (Rule 1). Settlement comes ONLY from the real grant read:
        // the TAURI path settles via pollGrant (the on-chain attributedStake + SBT +
        // entitlement); the web-dev SIM path settles via pollGrantSim (the honest
        // sim grantStatus derived from the persona/AppState) — never a blind timer.
        u.s5n = Math.min(32000, s.s5n + 6800);
      }
      // Q-A.4a item 1: the fabricated semantic-search "download" tick is GONE.
      // The bge embedding model is bundled WITH the mem-mcp daemon (not a UI
      // download), so semantic availability is a REAL read from memory_status()
      // (state.memSemantic), never a Math.random() progress bar claiming a
      // "sha256 verified" file that was never fetched (Rule 1).
      u.pollIn = s.pollIn <= 0.6 ? 20 : s.pollIn - 0.6;
      if (running && s.pins.length) {
        u.pins = s.pins.map((p) => {
          const nx = p.nextIn - 0.6;
          if (nx <= 0) return { ...p, nextIn: p.cadH * 3600, last: p.last === "pending" ? "pending" : "attested" };
          return { ...p, nextIn: nx };
        });
      }
      return u;
    });
  }

  makePeers(n: number) {
    const rows = [];
    const cities = ["fra1", "sgp1", "nyc3", "ams2", "tok1", "sfo2"];
    for (let i = 0; i < Math.min(5, Math.max(2, (n / 5) | 0)); i++) {
      rows.push({
        id: "16Uiu2HA" + Math.random().toString(36).slice(2, 8) + "…" + cities[i % cities.length],
        dir: Math.random() < 0.5 ? "in" : "out",
        lat: ((18 + Math.random() * 120) | 0) + " ms",
      });
    }
    return rows;
  }

  // ---------- ceremony ----------
  requestSig(spec: CerSpec): Promise<string> {
    return new Promise((resolve) => {
      const id = "cer" + ++this.cid;
      this.resolvers[id] = resolve;
      this.setState((s) => ({
        queue: s.queue.concat([{ id, ...spec }]),
        cerPhase: s.queue.length ? s.cerPhase : "review",
        cerStep: 0,
      }));
    });
  }
  finishCer(result: string): void {
    const s = this.state;
    const head = s.queue[0];
    if (!head) return;
    const res = this.resolvers[head.id!];
    delete this.resolvers[head.id!];
    this.setState({ queue: s.queue.slice(1), cerPhase: "review", cerStep: 0 });
    if (res) res(result);
  }
  approveCer(): void {
    const head = this.state.queue[0];
    if (!head) return;
    const chainless = head.chainless;
    this.setState({ cerPhase: "busy", cerStep: 0, cerHash: makeHash() });
    const step = (n: number, ms: number) => setTimeout(() => this.setState({ cerStep: n }), ms);
    step(1, 650);
    step(2, 1350);
    setTimeout(() => this.setState({ cerPhase: "done" }), chainless ? 1200 : 2050);
    setTimeout(
      () => {
        if (head.apply) head.apply(this.state.cerHash);
        this.finishCer("approved");
        this.save();
      },
      chainless ? 2300 : 3300,
    );
  }
  addActivity(kind: string, amount: string, hash?: string): void {
    this.setState((s) => ({
      activity: [{ id: "a" + Date.now(), kind, amount, hash: hash || makeHash(), ts: Date.now() }].concat(s.activity).slice(0, 24),
      justSigned: kind,
    }));
    setTimeout(() => this.setState({ justSigned: null }), 2200);
  }

  // ---------- chat ----------
  async sendChat(text: string): Promise<void> {
    text = (text || "").trim();
    if (!text || this.state.chatStatus !== "ready" || !this.provider) return;
    const userMsg: ChatMsg = { id: "m" + ++this.mid, who: "You", text, chips: [], streaming: false };
    this.setState((s) => ({ chatMsgs: s.chatMsgs.concat([userMsg]), chatStatus: "thinking" }));
    if (this.chatInputEl) this.chatInputEl.value = "";
    const asstId = "m" + ++this.mid;
    let started = false;
    const ensure = () => {
      if (started) return;
      started = true;
      this.setState((s) => ({ chatMsgs: s.chatMsgs.concat([{ id: asstId, who: "Agent", text: "", chips: [], streaming: true }]) }));
    };
    const patch = (fn: (m: ChatMsg) => ChatMsg) =>
      this.setState((s) => ({ chatMsgs: s.chatMsgs.map((m) => (m.id === asstId ? fn(m) : m)) }));
    try {
      await this.provider.send({
        messages: this.state.chatMsgs
          .filter((m) => !m.streaming)
          .map((m) => ({ role: m.who === "You" ? "user" : "assistant", content: m.text }))
          .concat([{ role: "user", content: text }]),
        callbacks: {
          onStatus: (st) => {
            if (st === "streaming") ensure();
            this.setState({ chatStatus: st === "done" || st === "error" ? "ready" : (st as AppState["chatStatus"]) });
          },
          onToken: (tk) => {
            ensure();
            patch((m) => ({ ...m, text: m.text + tk.replace(/\*\*/g, "") }));
            this.scrollChat();
          },
          onToolCall: (call) => this.handleTool(call, asstId, ensure),
        },
      });
    } catch (e) {
      console.error(e);
    }
    patch((m) => ({ ...m, streaming: false }));
    this.setState({ chatStatus: "ready" });
    this.scrollChat();
    this.save();
  }
  scrollChat(): void {
    requestAnimationFrame(() => {
      if (this.chatScrollEl) this.chatScrollEl.scrollTop = this.chatScrollEl.scrollHeight;
      if (this.jChatScrollEl) this.jChatScrollEl.scrollTop = this.jChatScrollEl.scrollHeight;
    });
  }

  async handleTool(call: ToolCall, asstId: string, ensure: () => void): Promise<string> {
    let args: Record<string, string> = {};
    try {
      args = JSON.parse(call.arguments || "{}");
    } catch {
      /* ignore */
    }
    let status = "ok";
    let result = "ok";
    if (call.name === "memory_assert") {
      // Rule 1 / Q-A.4a item 8: the demo agent has NO reachable memory daemon (mem-mcp
      // isn't bundled yet), so an approval here does NOT durably write anything. Show
      // the real approval ceremony, but the copy must not imply a durable write
      // occurred — it is a preview of the write the real agent would queue.
      const r = await this.requestSig({
        origin: "chat agent",
        requester: "dashboard agent · tool memory_assert",
        title: "Approve a memory write (demo — not durable)",
        chainless: true,
        rows: [
          { k: "Assertion", v: "“" + (args.fact || "") + "”" },
          { k: "Tenant", v: "personal · your capability grant" },
          { k: "Store", v: this.state.memSocketPath ?? "memory daemon offline — nothing is written" },
        ],
        cost: "none — demo mode does not write to the memory graph",
        sponsor: "no chain transaction",
        sponsorColor: "var(--tx-3)",
      });
      status = r;
      result = r;
    } else if (call.name === "journal_append") {
      const entry = args.entry || "work note";
      const today = new Date().toISOString().slice(0, 10);
      const r = await this.requestSig({
        origin: "chat agent",
        requester: "dashboard agent · tool journal_append",
        title: "Write to your journal",
        chainless: true,
        rows: [
          { k: "Entry", v: "“" + entry + "”" },
          { k: "Page", v: today + " · daily note" },
          { k: "Store", v: "local journal — off-chain, encrypted at rest" },
        ],
        cost: "none — local journal write",
        sponsor: "no chain transaction",
        sponsorColor: "var(--tx-3)",
      });
      if (r === "approved") {
        this.setState((st) => {
          const pages = st.jPages.slice();
          let pg = pages.find((p) => p.id === "d-" + today);
          if (!pg) {
            pg = { id: "d-" + today, title: today, kind: "daily", pinned: false, blocks: [] };
            pages.unshift(pg);
          }
          const upd = { ...pg, blocks: pg.blocks.concat(["@agent " + entry]) };
          return { jPages: pages.map((p) => (p.id === upd.id ? upd : p)) };
        });
        this.save();
      }
      status = r;
      result = r;
    } else if (call.name === "app_navigate") {
      const route = ["wallet", "node", "storage", "comms", "commissary", "settings", "dashboard"].indexOf(args.route) >= 0 ? args.route : "dashboard";
      this.go(route);
      result = "ok";
    } else if (call.name === "chain_read") {
      result = JSON.stringify(this.snapshot());
    } else if (call.name === "memory_search") {
      // W3.3 — REAL semantic search over the mem graph (docs or personal). Returns
      // real hits or honest emptiness (Rule 1 — never a fabricated Citrate fact).
      const tenant = args.tenant === "personal" ? "personal" : "citrate-docs";
      try {
        const res = await bridge.memory.search(tenant, args.query || "", 6);
        result = formatMemoryHits(res);
      } catch (e) {
        result = "memory search unavailable: " + (e instanceof Error ? e.message : String(e));
      }
    } else if (call.name === "memory_recall") {
      const tenant = args.tenant === "personal" ? "personal" : "citrate-docs";
      try {
        const res = await bridge.memory.recall(tenant, 6);
        result = formatMemoryHits(res);
      } catch (e) {
        result = "memory recall unavailable: " + (e instanceof Error ? e.message : String(e));
      }
    } else if (call.name === "journal_read") {
      // WP4.2 — READ the local journal (read-only, immediate). Honest-empty on no
      // pages / no match; never fabricates entries (Rule 1).
      const today = new Date().toISOString().slice(0, 10);
      result = formatJournalForAgent(this.state.jPages || [], args.page, today);
    } else if (call.name === "docs_link") {
      result = "ok";
    }
    const label =
      call.name.replace("_", ".") +
      (args.query ? " · " + args.query : "") +
      (status === "approved" ? " · approved" : status === "declined" ? " · declined" : " ✓");
    ensure();
    this.setState((s) => ({
      chatMsgs: s.chatMsgs.map((m) => (m.id === asstId ? { ...m, chips: m.chips.concat([{ label, status }]) } : m)),
    }));
    return result;
  }

  // ---------- Hermes P5 — user skills (prompt-skills) ----------
  /**
   * Add a member-authored prompt-skill (validated + normalized by the pure core).
   * Persists in local state (PERSIST_KEYS) like the journal — no chain, no key. On a
   * validation failure it toasts the honest reason and adds nothing (Rule 1).
   * Returns whether it was added, so a form can clear itself only on success.
   */
  addUserSkill(name: string, instruction: string, description = ""): boolean {
    const r = validateNewSkill(name, instruction, description, this.state.userSkills);
    if (!r.ok) {
      this.toast(r.error);
      return false;
    }
    const id = "usk-" + ++this.uskSeq + "-" + this.state.userSkills.length;
    this.setState((s) => ({ userSkills: s.userSkills.concat([{ id, ...r.skill }]) }));
    this.save();
    this.toast(`Added your "${r.skill.name}" skill.`);
    return true;
  }

  /** Remove a user skill by id. */
  removeUserSkill(id: string): void {
    this.setState((s) => ({ userSkills: s.userSkills.filter((k) => k.id !== id) }));
    this.save();
  }

  /**
   * Run a user skill: send its instruction to the chat against the ACTIVE model
   * (the router's Gemma / gateway / local backend). It is a prompt, not code — any
   * chain action the model then proposes still stops at the SignatureCeremony
   * (Rule 3 holds by construction; nothing here signs). Navigates to the dashboard
   * chat so the member sees the run.
   */
  runUserSkill(id: string): void {
    const skill = this.state.userSkills.find((k) => k.id === id);
    if (!skill) return;
    if (this.state.route !== "dashboard") this.go("dashboard");
    void this.sendChat(runPrompt(skill));
  }

  // ---------- journal capture ----------
  appendCapture(prefix: string): void {
    const txt = (this.jChatInputEl ? this.jChatInputEl.value : "").trim();
    if (!txt) return this.toast("Nothing to save — speak or type first");
    const st0 = this.state;
    const selId = st0.jSel || (st0.jPages[0] && st0.jPages[0].id);
    if (!selId) return;
    this.setState((st) => ({
      jInterim: "",
      jPages: st.jPages.map((p) => (p.id === selId ? { ...p, blocks: p.blocks.concat([prefix + txt]) } : p)),
    }));
    if (this.jChatInputEl) this.jChatInputEl.value = "";
    this.toast(prefix ? "Saved as a runnable prompt on this page" : "Saved to the page");
    this.save();
  }

  // ---------- journal export ----------
  private esc(t: string): string {
    return String(t).replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
  }
  private download(name: string, content: string, mime: string): void {
    const blob = new Blob([content], { type: mime });
    const a = document.createElement("a");
    a.href = URL.createObjectURL(blob);
    a.download = name;
    document.body.appendChild(a);
    a.click();
    a.remove();
    setTimeout(() => URL.revokeObjectURL(a.href), 3000);
  }
  private journalHtml(page: { title: string; blocks: string[] }): string {
    const items = page.blocks
      .map((b) => {
        const lead = (b.match(/^\s*/) || [""])[0].length;
        let t = b.trim();
        let tag = "";
        if (t.indexOf("@agent ") === 0) {
          t = t.slice(7);
          tag = '<span style="font-size:10px;color:#5d60c9;border:1px solid #5d60c9;border-radius:99px;padding:0 6px;margin-right:6px;">agent</span>';
        }
        if (t.indexOf("@prompt ") === 0) {
          t = t.slice(8);
          tag = '<span style="font-size:10px;color:#5a8205;border:1px solid #5a8205;border-radius:99px;padding:0 6px;margin-right:6px;">prompt</span>';
        }
        return '<li style="margin-left:' + Math.min(3, Math.floor(lead / 2)) * 22 + 'px;margin-bottom:6px;">' + tag + this.esc(t) + "</li>";
      })
      .join("");
    return (
      '<!DOCTYPE html><html><head><meta charset="utf-8"><title>' +
      this.esc(page.title) +
      "</title></head>" +
      '<body style="font-family:Georgia,serif;color:#0e0f0c;max-width:640px;margin:48px auto;line-height:1.6;">' +
      '<div style="font-size:11px;letter-spacing:.14em;text-transform:uppercase;color:#84867f;font-family:monospace;">Citrate journal · local · off-chain</div>' +
      '<h1 style="font-family:Helvetica,Arial,sans-serif;font-weight:500;">' +
      this.esc(page.title) +
      "</h1>" +
      '<ul style="list-style:disc;padding-left:20px;">' +
      items +
      "</ul></body></html>"
    );
  }
  exportJournal(fmt: string): void {
    const s = this.state;
    const page = s.jPages.find((p) => p.id === s.jSel) || s.jPages[0];
    if (!page) return;
    this.setState({ jExportOpen: false });
    const fname = page.title.replace(/[^\w-]+/g, "_");
    const mdLines = page.blocks.map((b) => {
      const lead = (b.match(/^\s*/) || [""])[0].length;
      return "  ".repeat(Math.min(3, Math.floor(lead / 2))) + "- " + b.trim();
    });
    if (fmt === "md") {
      this.download(fname + ".md", "# " + page.title + "\n\n" + mdLines.join("\n") + "\n", "text/markdown");
      this.toast("Markdown exported");
    } else if (fmt === "txt") {
      this.download(fname + ".txt", page.title + "\n\n" + page.blocks.map((b) => b.trim()).join("\n") + "\n", "text/plain");
      this.toast("Plain text exported");
    } else if (fmt === "doc") {
      this.download(fname + ".doc", this.journalHtml(page), "application/msword");
      this.toast("Word document exported");
    } else if (fmt === "pdf") {
      const f = document.createElement("iframe");
      f.style.cssText = "position:fixed;right:100%;width:800px;height:1000px;";
      document.body.appendChild(f);
      f.srcdoc = this.journalHtml(page);
      f.onload = () => {
        try {
          f.contentWindow!.focus();
          f.contentWindow!.print();
          this.toast("Print dialog opened — save as PDF");
        } catch {
          this.toast("Print blocked here — exported Markdown instead");
          this.exportJournal("md");
        }
        setTimeout(() => f.remove(), 60000);
      };
    }
  }

  // ---------- node lifecycle ----------
  /**
   * Notice when the node is DOWN but the member wanted it up, and say so.
   *
   * The supervisor already restarts a crashed node with backoff, but after the cap
   * it gives up silently: `node.rs` writes a crash record and stops. A validator
   * can therefore sit stopped indefinitely — not syncing, not producing, not
   * earning — while the app looks idle. Nothing surfaced that (2026-08-06: nine
   * `LOCK: Resource temporarily unavailable` crashes, no user-visible sign).
   *
   * One attempt to bring it back, then an HONEST toast naming the real reason from
   * the node's own crash record (Rule 1 — never "something went wrong"). Warned
   * once per outage so a persistent failure does not spam every 30s; the latch
   * clears when the node is observed running again.
   */
  private async checkNodeLiveness(): Promise<void> {
    if (BRIDGE_MODE !== "tauri") return;
    if (this.state.nodeIntent !== "run") return; // member stopped it deliberately
    if (this.nodeStarting) return; // a spawn is already in flight
    // Stalled sync (running but frozen below the tip): bounded auto-recovery, then honest surrender.
    // Each cycle stops + respawns the node (keeping nodeIntent "run" so it comes back). The budget
    // resets only on FULL sync, so a chain-side wedge that advances a few blocks per restart can't
    // spin here forever — after the budget we stop restarting and tell the truth.
    if (this.state.node === "stalled") {
      const NODE_STALL_MAX_CYCLES = 3;
      if (this.nodeStallCycles < NODE_STALL_MAX_CYCLES) {
        this.nodeStallCycles++;
        this.toast(`Sync stalled — restarting the node (${this.nodeStallCycles}/${NODE_STALL_MAX_CYCLES})…`);
        try {
          await bridge.node.stop();
        } catch {
          /* honest no-op: proceed to respawn regardless of the stop result */
        }
        this.setState({ node: "off" }); // reflect the stop so the idempotent startNode proceeds
        this.nodeStallSince = 0; // fresh detection window for the restarted node
        this.nodeStallLastHeight = -1;
        this.startNode();
      } else if (!this.nodeStalledNotified) {
        this.nodeStalledNotified = true;
        this.toast(
          "Sync is stuck and restarting hasn't helped — this is a known network-side issue the team is addressing. The node stays up and keeps retrying.",
        );
      }
      return;
    }
    const down = this.state.node === "off" || this.state.node === "error";
    if (!down) {
      this.nodeOutageNotified = false; // recovered — re-arm the warning
      return;
    }
    if (this.nodeOutageNotified) return;
    this.nodeOutageNotified = true;
    // Why did it stop? The node's crash record is the real answer.
    let reason = "";
    try {
      const rec = await bridge.node.lastCrash();
      if (rec?.reason) reason = " — " + rec.reason;
    } catch {
      /* honest no-op: no crash record available, warn without a reason */
    }
    this.toast("Node is not running" + reason + ". Restarting…");
    this.startNode();
  }

  startNode(): void {
    // IDEMPOTENT START (onboarding-snag fix). The launch autostart already brings the node
    // up, so a second startNode() — from the S5 provisioning step or a user tap — would ask
    // the supervisor to spawn an already-live node and get `AlreadyRunning`, which used to
    // surface as a scary "Could not start the node". If the node is already up or spawning,
    // this is a no-op: just fold the live state (height/peers/sync) and let onboarding proceed.
    if (
      BRIDGE_MODE === "tauri" &&
      (this.nodeStarting || (this.state.node !== "off" && this.state.node !== "error"))
    ) {
      this.setState({ nodeIntent: "run" });
      void this.refreshNode();
      return;
    }
    this.setState({ node: "prov", nodeIntent: "run", syncPct: 0, logs: [], peerRows: [] });
    if (BRIDGE_MODE === "tauri") {
      // Spawn the REAL supervised node (bridge.node.start → node.rs). The 2s
      // poller (refreshNode) then folds live state/height/peers as it boots +
      // syncs. `nodeStarting` guards the poller from demoting the optimistic
      // "prov" to "off" during the spawn window. On failure (e.g. the node
      // binary is not bundled) show an HONEST error, never a faked sync (Rule 1).
      this.nodeStarting = true;
      // Start the bundled IPFS daemon alongside the node (fire-and-forget; the node
      // reaches it on :5001 for artifact/model ops — block production does not need
      // it, so a failure here never blocks the node).
      void bridge.node.startIpfs().catch(() => {
        /* honest no-op: IPFS unavailable (dev/web) — the node still produces */
      });
      void bridge.node
        .start()
        .then(() => {
          this.nodeStarting = false;
          return this.refreshNode();
        })
        .catch((e) => {
          this.nodeStarting = false;
          // Surface the REAL failure. Tauri rejects a Rust `Err(String)` as a
          // plain string (not an Error), so an `instanceof Error` gate would
          // swallow the actual cause (e.g. "No space left on device", RocksDB
          // LOCK held) behind a generic line — a Rule-1 lie about why it failed.
          const reason =
            e instanceof Error ? e.message : typeof e === "string" && e.trim() ? e : "supervisor unavailable";
          // AlreadyRunning is NOT a failure — the node is already up (a race past the
          // idempotent guard above). Fold the live state and proceed; never scare the
          // user with "could not start" when the node is in fact running.
          if (/already running/i.test(reason)) {
            void this.refreshNode();
            return;
          }
          this.setState({ node: "error" });
          this.toast("Could not start the node: " + reason);
        });
      return;
    }
    setTimeout(() => {
      this.setState({ node: "syncing", syncPct: 2, peerRows: this.makePeers(8) });
      this.save();
    }, 1500);
  }

  /** Stop the node. Tauri: release the real supervisor (bridge.node.stop). */
  stopNode(): void {
    // Explicit member action — remember it so launch does NOT restart the node.
    this.setState({ node: "off", nodeIntent: "stopped", peers: 0, logs: [], syncPct: 0, peerRows: [] });
    if (BRIDGE_MODE === "tauri") {
      // Only claim the supervisor was released once the real stop RESOLVES — the
      // optimistic "off" reflects intent, but the toast must not overstate.
      void bridge.node
        .stop()
        .then(() => {
          this.toast("Node stopped — supervisor released");
          return this.refreshNode();
        })
        .catch(() => {
          this.toast("Stop requested — the supervisor releases on the next poll");
        });
      this.save();
      return;
    }
    this.toast("Node stopped — supervisor released");
    this.save();
  }

  // ---------- CORE-BC-3: local model (download + verify + serve) ----------

  /** Fold the REAL model status from the bridge into AppState. `ready` is EARNED
   * only by a real verify (Rule 1) — the bridge never fabricates it. A failed
   * poll keeps the last honest values. */
  async refreshModel(): Promise<void> {
    if (this.modelRefreshing) return; // in-flight guard — skip this 2s tick if the last is still running
    this.modelRefreshing = true;
    try {
      const st = await bridge.model.status();
      const patch: Partial<AppState> = { modelState: st.state, modelError: null };
      if (st.state === "downloading") {
        patch.modelDownloadedBytes = st.downloadedBytes;
        patch.modelTotalBytes = st.totalBytes;
      } else if (st.state === "error") {
        patch.modelError = st.msg;
      }
      this.setState(patch);
      // If the local model is verified-Ready, SPAWN the llama-server sidecar so
      // chat actually uses on-device Gemma. Previously serveStart was only called
      // right after a fresh verify (onboarding), so a normal launch with an
      // already-verified model left the server down → inference fell back to the
      // canned demo provider. Idempotent (serveStart returns AlreadyRunning if up);
      // best-effort (a missing/failed llama-server binary is caught, chat stays on
      // its honest fallback). Re-select the provider once the server is healthy.
      if (BRIDGE_MODE === "tauri" && st.state === "ready") {
        try {
          await bridge.model.serveStart();
          await this.rebuildProvider();
        } catch {
          /* honest no-op: no local server (binary missing / spawn failed) → chat
             stays on the gateway/demo fallback rather than a fabricated answer */
        }
      }
    } catch {
      /* honest no-op: a failed poll keeps the last real status, never a sim number */
    } finally {
      this.modelRefreshing = false;
    }
  }

  /** Start (or resume) the STREAMED model download, then poll `model.status()`
   * like `startNode` polls the node. On completion it auto-verifies; `ready` is
   * only ever reached from a real verify. An honest error surfaces on failure —
   * chat then falls back to the gateway/demo (never a fake "verified"). */
  startModelDownload(): void {
    this.setState({ modelState: "downloading", modelError: null });
    this.save();
    // Poll the real status every 2s so the progress bar reflects real bytes.
    if (!this.modelTimer) this.modelTimer = setInterval(() => void this.refreshModel(), 2000);
    void bridge.model
      .download()
      .then(async () => {
        // Download resolved: verify (streams the file through SHA-256). Only a
        // matching hash reaches "ready".
        await this.refreshModel();
        await this.verifyModel();
      })
      .catch((e) => {
        this.setState({ modelState: "error", modelError: e instanceof Error ? e.message : "download failed" });
        this.toast("Model download failed — " + (e instanceof Error ? e.message : "unknown") + ". Chat will use the gateway.");
        this.stopModelPoll();
        this.save();
      });
  }

  /** Stream-verify the downloaded model (SHA-256 == the pinned hash). On a match
   * the model becomes `ready`; on a mismatch the file is quarantined and an
   * honest error surfaces (never a fabricated "verified"). */
  async verifyModel(): Promise<void> {
    this.setState({ modelState: "verifying" });
    try {
      await bridge.model.verify();
      await this.refreshModel(); // reads the EARNED `ready` from the bridge
      if (this.state.modelState === "ready") {
        this.toast("Local model verified — SHA-256 matched. Chat can run on-device.");
        // Best-effort: spin up the llama-server sidecar so chat routes locally.
        void bridge.model.serveStart().catch(() => {/* honest: gateway fallback until bundled */});
      }
      this.stopModelPoll();
    } catch (e) {
      this.setState({ modelState: "error", modelError: e instanceof Error ? e.message : "verify failed" });
      this.toast("Model verify failed — " + (e instanceof Error ? e.message : "checksum mismatch") + ". Chat will use the gateway.");
      this.stopModelPoll();
    }
    this.save();
  }

  /**
   * Hermes ModelRouter (P0/WP0.4) — persist the member's model selection. `choices` is the
   * live router list the surface built (local + gateway [+ registry]). Fail-closed on a
   * phantom id (INV-Router-3): a selection that is not an enumerated choice is ignored, so
   * the active can never point at a fabricated model. The caller (the chat surface) also
   * switches the SERVED local model for a local pick (modelsSlice.selectModel restarts
   * llama-server); the gateway is the always-ready default, so picking it needs no restart.
   */
  selectModel(id: string, choices: ModelChoice[]): void {
    if (!canSelect(id, choices)) return; // phantom id — never persist a non-choice
    this.setState({ activeModelId: id });
    this.save();
  }

  /**
   * The model the send path actually resolves (INV-Router-2): the active choice iff it is
   * READY, else the always-ready gateway terminal. Pure over the passed `choices` so the
   * chat never routes to a not-ready (still-downloading / not-pulled) model.
   */
  routerActive(choices: ModelChoice[]): ModelChoice {
    return resolveActive(this.state.activeModelId, choices);
  }

  /** Honestly SKIP the local model: chat routes to the gateway/demo. This is an
   * opt-out, not a failure — no fabricated "ready". */
  skipModel(): void {
    this.stopModelPoll();
    this.setState({ modelSkipped: true });
    this.toast("Skipped local model — chat runs on the gateway (or demo). You can download it later in Settings.");
    this.save();
  }

  /** Stop the model status poller (called on complete/skip/error). */
  private stopModelPoll(): void {
    if (this.modelTimer) {
      clearInterval(this.modelTimer);
      this.modelTimer = null;
    }
  }

  /**
   * CORE-C2 — pull the REAL claimable from the chain and fold it into state.
   * Reads `ContributionAccounting.claimable(vaultAddress)` via `eth_call` on
   * 40204 through the bridge (data source: eth_call, Rule 11). On success the
   * claimable becomes a chain value (`earnSource: "chain"`); a fresh node
   * honestly reads 0. There is NO per-source breakdown from chain — the sim
   * `earnVal`/`earnPin`/`earnComp` are left as-is and the Earning tab labels them
   * as off-chain estimates (Rule 1 / I-3: no fabricated on-chain split).
   *
   * Failures (locked vault, RPC blip) are swallowed to a toast — the tab keeps
   * showing the last honest value rather than a fabricated one.
   */
  async refreshEarnings(): Promise<void> {
    // W1.4 FIRST, for a REGISTERED validator. `node_validator_earnings` reads
    // `rewardsOf(proposerPubkey)` on the ValidatorRegistry — the actual accrual for
    // producing blocks. It has existed since W1.4 and had NO caller: the app showed
    // `ContributionAccounting.claimable` instead, which node.rs itself documents as
    // "the wrong read" for a validator. A bonded member therefore saw 0.00 earnings
    // no matter how many blocks they proposed.
    //
    // Only meaningful once the bond is the registry's staker; before that it
    // honestly returns zeros, so we fall through to the contribution read rather
    // than pin a validator's display at zero.
    if (this.state.bondedStake >= 32000) {
      try {
        const v = await bridge.node.validatorEarnings();
        const total = Number(BigInt(v.totalWei)) / 1e18;
        const claimable = Number(BigInt(v.claimableWei)) / 1e18;
        this.setState({ earnVal: total, claimable, earnSource: "chain" });
        this.save();
        return;
      } catch {
        /* fall through to the contribution read — never fabricate */
      }
    }
    try {
      const e = await bridge.agent.earnings();
      // Wei string → SALT number for the prototype's numeric display.
      const salt = Number(BigInt(e.claimableWei)) / 1e18;
      this.setState({ claimable: salt, earnSource: "chain" });
      this.save();
    } catch (err) {
      // Honest: no fabricated fallback. Keep the prior value, note the source.
      this.toast("Claimable unavailable — " + String((err as Error).message ?? err));
    }
  }

  /**
   * CORE-C2-F-1 (@rule8) — the REAL Claim. This REPLACES the old sim-ceremony
   * `apply` that toasted "Claimed — balance updated from chain" and locally did
   * `setState({liquid:+amt, claimable:0})` (a fabricated settlement — Rule 1 / I-3).
   *
   * It drives `bridge.agent.claim()`, which reads the REAL on-chain claimable and
   * either reports an honest "nothing to claim" (0) or mints a REAL pending
   * ceremony (the `claimRewards()` intent). In the Tauri build that ceremony is then
   * approved through the ONE human-in-the-loop signer via `signing.broadcast`
   * (B1.4 → a real 40204 tx); the balance changes ONLY when the real tx settles and
   * we re-read `claimable` from chain. In web-dev (sim) there is no key and no chain,
   * so a claim CANNOT settle — we say so honestly and never fake a balance change.
   */
  /**
   * KYC gates getting SALT **out**, not taking part.
   *
   * THE MODEL (ADR-2026-07-25 + the 2026-07-28 handoff plan). Participation is
   * un-gated: a paid member is provisioned and can run the node, produce blocks and
   * accrue earnings with no KYC at all. Verification is the qualifier for VALUE
   * LEAVING — claiming rewards and withdrawing stake.
   *
   * Re-checks LIVE (`/userinfo`) rather than trusting the cached claim: a stale
   * local `s2` must never authorize a payout, and an unreachable authority REFUSES
   * rather than falling through on the stale value.
   *
   * HONEST LIMIT — READ THIS BEFORE CALLING IT ENFORCEMENT. This is an app-side
   * product default, NOT a gate. The member holds their own key and can call
   * `claimRewards` / `requestWithdrawal` directly against the 40204 contracts,
   * bypassing this entirely. It is beta-acceptable for a known member set; it is
   * not a compliance control. The contract-side gate (a KYC attestation the pool
   * and registry honour) is a separate deliverable.
   */
  async assertPayoutKyc(action: string): Promise<boolean> {
    try {
      await this.authUserinfo();
    } catch {
      // Could not confirm — fail closed. Never authorize a payout on a claim we
      // could not re-read.
      this.toast(action + " needs a verified identity, and we could not reach the authority to confirm. Try again in a moment.");
      return false;
    }
    if (this.state.s2 === "verified") return true;
    const detail =
      this.state.s2 === "pending" || this.state.s2 === "review"
        ? "Your verification is still in review."
        : this.state.s2 === "failed"
          ? "Your last verification did not pass."
          : "Verify your identity to withdraw.";
    this.toast(action + " requires identity verification. " + detail + " Your earnings keep accruing in the meantime.");
    return false;
  }

  async claimRewards(): Promise<void> {
    // Payout gate: KYC is required to take SALT out (participation is not gated).
    if (!(await this.assertPayoutKyc("Claiming earnings"))) return;
    let res: Awaited<ReturnType<typeof bridge.agent.claim>>;
    try {
      res = await bridge.agent.claim();
    } catch (err) {
      this.toast("Claim unavailable — " + String((err as Error).message ?? err));
      return;
    }
    if (res.kind === "nothing") {
      this.toast("No claimable earnings — nothing to claim.");
      return;
    }
    // Q-E.1 (@rule8, P0) — a REAL pending ceremony exists (res.view). STOP: surface
    // the decoded claimRewards() intent for human approval; nothing broadcasts
    // until a person clicks Approve (no more self-broadcast from code).
    this.openWalletReview("claim", "Claim rewards", res.view);
  }

  /**
   * CORE (@rule8) — a REAL native SALT transfer. Builds the pending ceremony
   * (bridge.wallet.send → wallet_send), then approves + broadcasts the real
   * EIP-155 tx on 40204 (B1.4). NEVER mutates the local balance and NEVER
   * fabricates a hash (unlike the old sim ceremony) — the balance re-reads from
   * chain after settle (refreshWallet). Mirrors claimRewards(). Web-dev cannot
   * settle — honest message, no fake transfer.
   */
  async walletSend(to: string, amountWei: string): Promise<void> {
    let view: Awaited<ReturnType<typeof bridge.wallet.send>>;
    try {
      view = await bridge.wallet.send(to, amountWei);
    } catch (err) {
      this.toast("Send unavailable — " + String((err as Error).message ?? err));
      return;
    }
    // Q-E.1 (@rule8, P0) — STOP here. Build the pending review; a HUMAN must see
    // the decoded intent and click Approve before anything broadcasts. No signing
    // happens from code.
    this.openWalletReview("send", "Send SALT", view);
  }

  /**
   * CORE (@rule8) — a REAL LiquidStakingPool stake (`deposit()`). Builds the
   * pending ceremony (bridge.wallet.stake → wallet_stake), then approves +
   * broadcasts the real 40204 deposit tx (B1.4). NEVER mutates the local balance
   * and NEVER fabricates a hash — the staked figure re-reads from chain after
   * settle (refreshWallet → balanceOf). Mirrors walletSend. Web-dev cannot settle
   * — honest message, no fake stake. `amountWei` is a decimal wei string.
   */
  async walletStake(amountWei: string): Promise<void> {
    let view: Awaited<ReturnType<typeof bridge.wallet.stake>>;
    try {
      view = await bridge.wallet.stake(amountWei);
    } catch (err) {
      this.toast("Stake unavailable — " + String((err as Error).message ?? err));
      return;
    }
    // Q-E.1 (@rule8, P0) — STOP: surface the decoded deposit for human approval.
    this.openWalletReview("stake", "Add stake", view);
  }

  /**
   * Hermes P3 / WP3.2 — deploy a compiled contract. Builds the pending creation-tx
   * ceremony (bridge.contracts.deploy → contract_deploy), then STOPS at the human gate:
   * the decoded "Deploy contract" intent shows at the WalletReviewModal and only on
   * approve does signing.broadcast sign + send the real 40204 creation tx (B1.4). The
   * bytecode is caller-supplied + compiled — nothing fabricates contract code (Rule 1),
   * nothing signs from code (Rule 3). `input` mirrors ContractDeployInput.
   */
  async deployContract(input: {
    bytecodeHex: string;
    constructorArgsHex?: string;
    valueWei?: string;
    gas?: number;
  }): Promise<void> {
    let view: Awaited<ReturnType<typeof bridge.contracts.deploy>>;
    try {
      view = await bridge.contracts.deploy(input);
    } catch (err) {
      this.toast("Deploy unavailable — " + String((err as Error).message ?? err));
      return;
    }
    // STOP: a human sees the decoded creation tx (raw init code) and approves it.
    this.openWalletReview("deploy", "Deploy contract", view);
  }

  /**
   * W1.3 (@rule8) — activate the member's node as a block-producing validator
   * (bond-clone model). Builds the pending `MemberBond.activate(pubkey, sig)` ceremony
   * (bridge.node.registerValidator → node_register_validator: reads the live nonce,
   * signs the register digest with the node's `proposer.key`, staker = the member's
   * bond CLONE), then STOPS and surfaces the decoded tx for human approval. The tx is
   * sent from the member EOA to the clone and carries NO value — the 32k already lives
   * in the clone. On approval it broadcasts (B1.4): the clone forwards its principal
   * into ValidatorRegistry, the node enters the active set, and it begins producing +
   * earning the subsidy. Only meaningful once the node is synced + its proposer.key is
   * minted AND the bond is deployed + funded; a not-ready node (or web-dev) errors
   * honestly — never a fabricated activation.
   */
  async activateValidator(): Promise<boolean> {
    // ARM FIRST. The node runs as a plain follower and never mints `proposer.key`
    // on its own — arming respawns it with `--mine --coinbase`, which does. Only
    // `maybeAutoBond` armed, and that is gated on `stage === "s6"`, so a member who
    // had FINISHED onboarding (stage "done") and pressed Activate on the dashboard
    // hit "proposer key not available yet" forever: nothing in that path ever armed
    // the producer (observed 2026-08-06 with 32,000 SALT already funded on chain).
    //
    // `arm_mining_if_synced` re-checks the network tip on the backend and no-ops if
    // the node is not caught up or has no coinbase, so calling it here is safe and
    // idempotent. The key appears a few seconds later on respawn, so we do NOT open
    // a ceremony on this pass — we say what is happening and let the caller retry.
    if (!this.validatorArmed) {
      let armed = false;
      try {
        armed = await bridge.node.armMining();
      } catch {
        /* honest no-op: surfaced by the registerValidator error below */
      }
      if (armed) {
        this.validatorArmed = true;
        this.toast("Block production armed — minting your validator key, then activation opens.");
        return false; // retry once the key lands
      }
    }
    let view: Awaited<ReturnType<typeof bridge.node.registerValidator>>;
    try {
      view = await bridge.node.registerValidator();
    } catch (err) {
      // Honest failure — most often "proposer key not available yet" while the
      // armed node is still respawning/minting it. Return false so the auto-bond
      // caller does NOT consume its one-shot attempt and simply retries next poll.
      this.toast("Validator activation unavailable — " + String((err as Error).message ?? err));
      return false;
    }
    this.openWalletReview("stake", "Activate validator · bond 32,000 SALT", view, "32,000 SALT validator bond");
    return true;
  }

  /**
   * Bind THIS device's custody wallet to the member's Citrate identity.
   *
   * WHY IT COMES FIRST. The authority mints a `wallet_address` claim for every
   * member; until a wallet is linked that claim is the counterfactual smart-wallet
   * address, which no private key can spend from. The membership money path grants to
   * THAT address (it becomes the `MemberBond` clone's `member`/`onlyMember`), while
   * the bond activation (`MemberBond.activate`) is sent from this device's custody
   * EOA — so if they differ, the clone is bound to an address this device cannot sign
   * as and `activate` reverts `onlyMember`. Linking makes the two the same address.
   *
   * Signs nothing here: this builds the pending personal_sign ceremony and STOPS,
   * exactly like every other wallet action. The human sees the authority's
   * challenge message verbatim and approves it. No funds move.
   */
  async linkWallet(): Promise<void> {
    // ROOT-CAUSE FIX (the "Wallet link unavailable" saga): a fresh install has an
    // uninitialized + LOCKED custody vault and NO minted wallet, so linkRequest()
    // — and every wallet read (balances/address) — failed closed, and the grant
    // fell back to a derived placeholder address. Provision the device wallet
    // FIRST: seamless + device-bound (auto keyring passphrase, no user passphrase;
    // security boundary = the OS login). ensureReady() init+unlocks the vault and
    // mints the wallet silently, returning the real custody EOA. We stamp it into
    // custodyAddr so walletIsLinked() has the custody side even before the first
    // balances read lands. Idempotent — a no-op on an already-provisioned device.
    try {
      const { address } = await bridge.wallet.ensureReady();
      if (address) this.setState({ custodyAddr: address });
    } catch (err) {
      this.toast("Wallet setup unavailable — " + String((err as Error).message ?? err));
      return;
    }
    let view: Awaited<ReturnType<typeof bridge.wallet.linkRequest>>;
    try {
      view = await bridge.wallet.linkRequest();
    } catch (err) {
      this.toast("Wallet link unavailable — " + String((err as Error).message ?? err));
      return;
    }
    this.openWalletReview("wallet-link", "Link this wallet to your Citrate identity", view, "no funds move");
  }

  /**
   * True when the authority already serves THIS device's custody wallet as the
   * member's `wallet_address`. Only meaningful once both reads have landed — an
   * unknown either side reads false (never an optimistic yes).
   */
  walletIsLinked(): boolean {
    const claim = (this.state.walletAddr || "").toLowerCase();
    const custody = (this.state.custodyAddr || "").toLowerCase();
    return claim !== "" && custody !== "" && claim === custody;
  }

  /**
   * CORE WP2 — pull the wallet's REAL pending withdrawals from chain and fold
   * them into AppState. `bridge.wallet.pendingWithdrawals()` (Tauri) reads the
   * live on-chain queue (getLogs WithdrawalRequested + withdrawals(id) +
   * block_number); the sim shim returns [] (no chain). A fresh wallet honestly
   * reads []. Failure leaves the last honest list untouched — no fabricated rows.
   */
  async refreshPendingWithdrawals(): Promise<void> {
    try {
      const list = await bridge.wallet.pendingWithdrawals();
      this.setState({ pendingWithdrawals: list });
    } catch {
      /* honest no-op: keep the last real list, never a fabricated one */
    }
  }

  /**
   * CORE WP2 (@rule8) — a REAL withdraw of self-added stake. Builds the pending
   * `requestWithdrawal(shares)` ceremony (bridge.wallet.requestWithdrawal →
   * wallet_request_withdrawal; the SALT→shares conversion is done in Rust from
   * live reads), then approves + broadcasts the real 40204 tx (B1.4). It burns
   * shares into the ~7-day queue; the SALT is claimable later via
   * walletClaimWithdrawal. NEVER mutates a local balance or fabricates a hash —
   * balances + the pending list re-read from chain after settle. Mirrors
   * walletStake. Web-dev cannot settle — honest message. `amountWei` = SALT (wei).
   */
  async walletRequestWithdrawal(amountWei: string): Promise<void> {
    // Payout gate: withdrawing STAKE takes SALT out, so it needs KYC too.
    if (!(await this.assertPayoutKyc("Withdrawing stake"))) return;
    let view: Awaited<ReturnType<typeof bridge.wallet.requestWithdrawal>>;
    try {
      view = await bridge.wallet.requestWithdrawal(amountWei);
    } catch (err) {
      this.toast("Withdraw unavailable — " + String((err as Error).message ?? err));
      return;
    }
    // Q-E.1 (@rule8, P0) — STOP: surface the decoded withdrawal for human approval.
    this.openWalletReview("withdraw-request", "Request withdrawal", view);
  }

  /**
   * CORE WP2 (@rule8) — claim a MATURED withdrawal. Builds the pending
   * `claimWithdrawal(id)` ceremony (bridge.wallet.claimWithdrawal →
   * wallet_claim_withdrawal), then approves + broadcasts the real 40204 tx (B1.4).
   * The ~7-day (50,400-block) delay is enforced ON-CHAIN — a too-early claim
   * reverts (we never fabricate an early settlement). NEVER mutates a local
   * balance or fabricates a hash — balances + pending list re-read from chain.
   * Web-dev cannot settle — honest message. `id` = the decimal request id.
   */
  async walletClaimWithdrawal(id: string): Promise<void> {
    // The matured leg still moves SALT to the member — gate it as well, so a
    // request made while verified cannot be collected after a revocation.
    if (!(await this.assertPayoutKyc("Claiming a matured withdrawal"))) return;
    let view: Awaited<ReturnType<typeof bridge.wallet.claimWithdrawal>>;
    try {
      view = await bridge.wallet.claimWithdrawal(id);
    } catch (err) {
      this.toast("Claim unavailable — " + String((err as Error).message ?? err));
      return;
    }
    // Q-E.1 (@rule8, P0) — STOP: surface the decoded claim for human approval.
    this.openWalletReview("withdraw-claim", "Claim withdrawal", view);
  }

  /**
   * ADR-2026-08-30 (D3) — verify a linked social identity: open a ceremony over the wallet-signed
   * IdentityBinding, show it at the review gate, and on approve record the binding (flips verified).
   * Rule 3: the wallet signs at the ceremony; nothing signs here. `onDone` refreshes the surface
   * after either outcome.
   */
  async verifySocial(network: "x" | "linkedin" | "discord", onDone?: () => void): Promise<void> {
    let view: CeremonyView;
    try {
      view = await bridge.social.verifyRequest(network);
    } catch (err) {
      this.toast("Couldn't start verification — " + String((err as Error).message ?? err));
      return;
    }
    this.openWalletReview("social", "Verify your " + network + " identity", view, undefined, () => onDone?.());
  }

  /**
   * ADR-2026-08-30 (D1) — share your verified, group-visible identity bindings to every group you're
   * in, over the ciphertext-only relay (server-blind). Each binding rides as a control message peers
   * ingest + hide; they recover-verify it before showing your face. Best-effort; safe to call often.
   */
  async shareSocialBindings(): Promise<void> {
    try {
      const groups = await bridge.groups.list();
      if (!groups.length) return;
      const nets = ["discord", "x", "linkedin"] as const;
      const payloads = (
        await Promise.all(nets.map((n) => bridge.social.exportBinding(n).catch(() => null)))
      ).filter((p): p is NonNullable<typeof p> => !!p);
      if (!payloads.length) return;
      const { SOCIAL_BINDING_MSG_PREFIX } = await import("../bridge/domains");
      for (const g of groups) {
        for (const p of payloads) {
          try {
            await bridge.groups.send(g.id, SOCIAL_BINDING_MSG_PREFIX + JSON.stringify(p));
          } catch {
            /* best-effort per group */
          }
        }
      }
    } catch {
      /* best-effort: no groups / not linked / relay down */
    }
  }

  // ---------- Q-E.1 (@rule8, P0) — wallet review gate ----------
  /**
   * Set the pending wallet-review state from a freshly-built ceremony view and
   * STOP. This is the human-in-the-loop gate: nothing broadcasts until a person
   * sees the decoded intent (WalletReviewModal) and clicks Approve. NEVER signs
   * or broadcasts here. Undecodable calldata (view.requiresRawAck) starts with
   * rawAck=false so Approve is blocked until the explicit raw-mode ack.
   */
  openWalletReview(kind: WalletReview["kind"], label: string, view: CeremonyView, spendSummary?: string, onResolved?: WalletReview["onResolved"]): void {
    this.setState({ walletReview: { kind, label, view, spendSummary, rawAck: false, onResolved } });
  }

  /**
   * CX-S6.3 — the human gate for an agent-proposed action. A CHAIN effect is bridged into a real
   * pending ceremony (hermes_bridge_pending) and shown at the Signature Ceremony (WalletReviewModal);
   * on approve it signs+broadcasts, then releases the sidecar via hermes_resolve(true). A code/shell
   * effect (or a chain head that didn't bridge) is gated by the review ceremony and released the same
   * way. Nothing signs here (Rule 3); the sidecar holds no key.
   */
  async reviewAgentApproval(ap: { id: string; kind: "code" | "chain" | "shell"; summary: string }, onDone?: () => void): Promise<void> {
    if (ap.kind === "chain") {
      let view: CeremonyView | null = null;
      try {
        view = await bridge.agentHarness.bridgePending();
      } catch (err) {
        this.toast("Couldn't prepare the agent's action for review — " + String((err as Error).message ?? err));
        return;
      }
      if (view) {
        this.openWalletReview("agent", ap.summary, view, undefined, async (approved) => {
          try {
            await bridge.agentHarness.resolve(approved);
          } catch {
            /* best-effort: the sidecar head unblocks on its own timeout if this fails */
          }
          onDone?.();
        });
        return;
      }
      // No bridged view (head wasn't a chain effect) — fall through to the confirm gate.
    }
    // code / shell (or an unbridged chain head): confirm at the ceremony, then release the sidecar.
    const outcome = await this.requestSig({
      origin: "agent:hermes",
      requester: "agent runtime",
      title: ap.summary,
      rows: [
        { k: "Kind", v: ap.kind },
        { k: "Effect", v: ap.kind === "code" ? "a code change on your machine" : ap.kind === "shell" ? "a shell command on your machine" : "an on-chain action" },
      ],
      cost: "—",
      sponsor: "you approve · one action",
      sponsorColor: "var(--ok)",
      chainless: true,
    });
    try {
      await bridge.agentHarness.resolve(outcome === "approved");
    } catch {
      /* best-effort */
    }
    onDone?.();
  }

  /** Toggle the raw-mode ack for an undecodable-calldata review (gates Approve). */
  setWalletReviewRawAck(rawAck: boolean): void {
    const r = this.state.walletReview;
    if (!r) return;
    this.setState({ walletReview: { ...r, rawAck } });
  }

  /**
   * Approve the pending wallet review → broadcast the deferred ceremony. This is
   * the ONE place a wallet money action reaches signing.broadcast, and only after
   * an explicit human click. Fails closed when undecodable calldata has not been
   * raw-acked (never broadcasts blind). On success: refresh balances/activity; on
   * failure: honest toast + release the pending ceremony. `rawAck` may be passed
   * explicitly (tests / the modal checkbox) or read from the review state.
   */
  async approveWalletReview(rawAck?: boolean): Promise<void> {
    const r = this.state.walletReview;
    if (!r) return;
    const ack = rawAck ?? r.rawAck;
    // Fail closed: undecodable calldata requires the explicit raw-mode ack before
    // ANY broadcast (T2 — no signing blind). Keep the review open.
    if (r.view.requiresRawAck && !ack) {
      this.toast("Undecodable calldata — tick “I understand this is raw” to approve.");
      return;
    }
    // A wallet LINK is not a transaction: it is a personal_sign whose proof is
    // POSTed to the authority. It must never reach signing.broadcast, which would
    // try to send a tx. Route it to the dedicated command, which signs, submits,
    // and returns only the bound address (the signature stays in-process).
    if (r.kind === "wallet-link") {
      try {
        const res = await bridge.wallet.linkApprove(r.view.id, ack);
        this.setState({ walletReview: null });
        this.addActivity(r.label, "no funds moved", "");
        // `linked` and `canonical` are different facts and the difference is the
        // whole bug: the link is durable the moment the proof is accepted, but
        // the authority only serves THIS address as `wallet_address` if it is
        // also canonical. Claiming "the authority now pays you" off `linked`
        // alone is how a member ends up blocked while being told it worked.
        if (res.canonical) {
          this.toast("Wallet linked — the authority now pays " + res.address.slice(0, 10) + "….");
        } else {
          this.toast(
            "Wallet linked, but the payout address did not move. Your earlier wallet is " +
              "still the one on file — contact support before paying in.",
          );
        }
        // Re-read the claim so the UI reflects the new binding rather than
        // asserting it: walletIsLinked() must be earned by a real read.
        await this.authUserinfo();
        await this.refreshWallet();
        this.save();
      } catch (err) {
        try {
          await bridge.wallet.linkReject(r.view.id);
        } catch {
          /* best-effort cleanup */
        }
        this.setState({ walletReview: null });
        this.toast("Wallet not linked — " + String((err as Error).message ?? err));
      }
      return;
    }
    // A SOCIAL identity verification (ADR D3) is a personal_sign, not a tx: the wallet signs the
    // IdentityBinding at the ceremony and the binding is recorded — it must never reach
    // signing.broadcast. Route it to the dedicated command, which signs, records, and flips verified.
    if (r.kind === "social") {
      try {
        const li = await bridge.social.verifyApprove(r.view.id, ack);
        this.setState({ walletReview: null });
        this.toast(`Verified — your ${li.network} identity is now bound to your wallet.`);
        await r.onResolved?.(true);
      } catch (err) {
        try {
          await bridge.social.verifyForget(r.view.id);
        } catch {
          /* best-effort cleanup */
        }
        this.setState({ walletReview: null });
        this.toast("Not verified — " + String((err as Error).message ?? err));
        await r.onResolved?.(false);
      }
      return;
    }
    // In web-dev there is no key/chain; signing.broadcast throws honestly. Keep the
    // review open so the flow is truthful (no fabricated settlement, Rule 1).
    if (BRIDGE_MODE !== "tauri") {
      try {
        await bridge.signing.broadcast(r.view.id, ack);
      } catch (err) {
        this.toast(WALLET_ACTION_LABELS[r.kind] + " settles only in the desktop app — " + String((err as Error).message ?? err));
      }
      // Release the pending sim ceremony and clear the review (nothing settled).
      try {
        await bridge.signing.reject(r.view.id);
      } catch {
        /* best-effort — sim broadcast already consumed it */
      }
      this.setState({ walletReview: null });
      // Nothing settled in the web preview → the sidecar effect does not proceed.
      await r.onResolved?.(false);
      return;
    }
    // Tauri: sign + broadcast the real 40204 tx, then re-read balances from chain.
    try {
      const result = await bridge.signing.broadcast(r.view.id, ack);
      this.addActivity(r.label, r.view.decoded.cost || r.view.decoded.action, result.txHash);
      this.toast(r.label + " broadcast — tx " + result.txHash.slice(0, 10) + "…; balance updates when it settles.");
      this.setState({ walletReview: null });
      if (r.kind === "claim") await this.refreshEarnings();
      else await this.refreshWallet();
      // Activating the bond (MemberBond.activate) makes the clone bond its principal
      // into the ValidatorRegistry — re-read the REAL bond now so the node flips to
      // "validating" immediately instead of waiting for the next 2s poll.
      if (r.kind === "stake") await this.refreshNode();
      if (r.kind === "withdraw-request" || r.kind === "withdraw-claim") await this.refreshPendingWithdrawals();
      await this.refreshActivity();
      this.save();
      // CX-S6.3 — an agent-originated chain effect: release the sidecar now that it's broadcast.
      await r.onResolved?.(true);
    } catch (err) {
      // Honest failure — release the pending ceremony so it isn't left dangling.
      try {
        await bridge.signing.reject(r.view.id);
      } catch {
        /* best-effort cleanup */
      }
      this.setState({ walletReview: null });
      this.toast(r.label + " not settled — " + String((err as Error).message ?? err));
      // The tx didn't settle → tell the sidecar to abort, not proceed.
      await r.onResolved?.(false);
    }
  }

  /**
   * Reject the pending wallet review → broadcast NOTHING, release the ceremony,
   * clear the review. The human declined; no signature is produced.
   */
  async rejectWalletReview(): Promise<void> {
    const r = this.state.walletReview;
    if (!r) return;
    this.setState({ walletReview: null });
    try {
      // The link path has its own reject: it also drops the one-time challenge
      // nonce, so a declined link cannot be resumed with a stale nonce.
      if (r.kind === "wallet-link") await bridge.wallet.linkReject(r.view.id);
      else await bridge.signing.reject(r.view.id);
      // A declined social verification also drops its pending-bind entry (nonce is one-time).
      if (r.kind === "social") await bridge.social.verifyForget(r.view.id);
    } catch {
      /* best-effort — the ceremony may already be gone */
    }
    this.toast(r.label + " declined — nothing was signed.");
    // CX-S6.3 — an agent-originated effect the human declined: tell the sidecar to abort.
    await r.onResolved?.(false);
  }

  /**
   * Honest outcome for a value action whose REAL on-chain path is NOT wired yet.
   * Today this is bonded pinning. (The WITHDRAW leg — LiquidStakingPool
   * `requestWithdrawal` → `claimWithdrawal` — is now wired via
   * walletRequestWithdrawal / walletClaimWithdrawal, WP2.) NEVER fabricates a hash
   * or mutates a balance (Rule 1/3). Web-dev: settles only in the desktop app.
   */
  settleUnwired(action: string): void {
    if (BRIDGE_MODE === "tauri") {
      this.toast(action + " isn't wired to a real 40204 transaction yet — no funds moved. (Grounded contract pending.)");
    } else {
      this.toast(action + " settles only in the desktop app — no key or chain in web preview.");
    }
  }

  openMicroApp(a: { name?: string; capabilities?: string[] }): void {
    // No isolated capability-scoped webview is spawned yet — the old path toasted
    // "opened" but nothing happened. Honest until the micro-app runtime lands.
    this.toast((a.name || "This micro-app") + " isn't wired to an isolated window yet — capability-scoped micro-apps land in a later build.");
  }

  // ---------- onboarding transitions ----------
  onJoin(): void {
    this.setState({ stage: "s1" });
    this.save();
  }
  onExplore(): void {
    this.setState({ stage: "done", tier: "free", coachDone: true, chatMsgs: [greeting({ ...this.persona(), name: this.identity().real ? this.identity().name : "" })] });
    this.save();
  }
  /** S3 — choose the FREE tier: enter the app WITHOUT a paid membership (no stake, no SBT). Mirrors
   *  onExplore (the S0 free path) so a user who reached the membership step can still decline the paid
   *  seat instead of being funneled into it. The paid membership stays available later (Settings →
   *  Billing → Renew opens the same checkout). No payment, no fabricated grant (Rule 1). */
  onS3Free(): void {
    this.setState({ stage: "done", tier: "free", coachDone: true, chatMsgs: [greeting({ ...this.persona(), name: this.identity().real ? this.identity().name : "" })] });
    this.toast("You're on the Free tier — wallet, agent, chain reads, and marketplace view. Upgrade to membership anytime in Settings → Billing.");
    this.save();
  }
  /** Enterprise · Contact us — submit a qualified sales lead to core-membership. Returns `{ ok }`, or
   *  `{ ok: false, error }` with the backend's honest message (Rule 1 — never a fabricated "received").
   *  No money, no grant; the record just prepares sales for the call. */
  async submitEnterpriseLead(lead: {
    org: string;
    email: string;
    contact?: string;
    seats?: string;
    workload?: string;
    timeline?: string;
    notes?: string;
  }): Promise<{ ok: boolean; error?: string }> {
    try {
      await bridge.membership.enterpriseLead(lead);
      return { ok: true };
    } catch (err) {
      return { ok: false, error: String((err as Error)?.message ?? err) };
    }
  }
  onS1Start(): void {
    this.setState({ s1: "waiting" });
    // Tauri: drive the REAL loopback-PKCE sign-in. The system browser opens; on
    // success the entitlement claim folds in and S1 completes. A failure/cancel
    // returns to idle (honest — no faked success).
    if (BRIDGE_MODE === "tauri") {
      void this.authLogin()
        .then(() => {
          this.setState({ s1: "done" });
          this.save();
        })
        .catch(() => {
          this.setState({ s1: "idle" });
        });
      return;
    }
    // Web-dev sim: the prototype attestation animation (no real auth).
    this._s1t = setTimeout(() => {
      this.setState({ s1: "attest", s1c: 0 });
      setTimeout(() => this.setState({ s1c: 1 }), 700);
      setTimeout(() => this.setState({ s1c: 2 }), 1500);
      setTimeout(() => this.setState({ s1c: 3 }), 2300);
      setTimeout(() => {
        this.setState({ s1: "done" });
        this.save();
      }, 2800);
    }, 2300);
  }
  onS1Cancel(): void {
    if (this._s1t) clearTimeout(this._s1t);
    this.setState({ s1: "idle" });
  }
  onS2Start(): void {
    this.setState({ s2: "pending" });
    // Tauri: open the REAL KYC flow in the browser, then poll /userinfo for the
    // kyc_status claim change (none→pending→verified/failed/review). The S2 UI
    // drives entirely off the real claim.
    if (BRIDGE_MODE === "tauri") {
      void this.kycStart().catch(() => {
        /* browser open failed — stay pending; the user can retry */
      });
      this.pollKyc();
      return;
    }
    // Web-dev sim: resolve to the persona's scripted outcome after a beat.
    setTimeout(() => {
      this.setState({ s2: this.state.kycOutcome });
      this.save();
    }, 2600);
  }

  /**
   * Poll the live /userinfo entitlement for a KYC status change while S2 is
   * pending (Tauri only). Stops once the claim resolves to a terminal state.
   */
  private pollKyc(): void {
    if (BRIDGE_MODE !== "tauri") return;
    const tick = () => {
      if (this.state.s2 !== "pending") return; // resolved or navigated away
      void this.authUserinfo().finally(() => {
        if (this.state.s2 === "pending") setTimeout(tick, 5000);
        else this.save();
      });
    };
    setTimeout(tick, 5000);
  }
  onS3Pay(): void {
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
      if (!this.walletIsLinked()) {
        this.toast("Link your wallet first — approve the link, then tap Pay again.");
        void this.linkWallet();
        return;
      }
      // ★ MONEY GUARD ★ — never open checkout for a member who ALREADY holds the
      // grant. Settlement is chain truth, and the client had no way to observe an
      // existing grant: `settled` was reachable ONLY through `pollMembership`, which
      // ran ONLY from this handler. So a member who was granted while the app was
      // closed (or after its poll gave up) came back to S3 `idle`, whose only
      // control is "Check out in your browser · $48" — and paying again is the one
      // thing that cannot help, because the membership is one-per-sub and the second
      // order can never be granted. Observed 2026-08-15: a member reached three
      // stranded `paid` orders this way. Read the chain BEFORE spending money.
      void (async () => {
        if (await this.settleS3IfAlreadyGranted()) return;
        this.setState({ s3: "paying" });
        // login_hint = the signed-in email → the browser checkout binds to THIS account.
        void bridge.membership.checkout(this.state.authEmail ?? undefined).catch(() => {
          // Popup open failed (headless / user cancel at OS level) — stay "paying";
          // the poll below simply never settles. The user can retry (S3 re-enterable).
        });
        this.pollMembership();
      })();
      return;
    }
    // Web-dev sim: keep the prototype's fake settle so onboarding still walks.
    // (Guarded: the sim membership.checkout is a no-op; the tier flip is sim-only.)
    this.setState({ s3: "paying" });
    setTimeout(() => {
      this.setState({ s3: "settled", tier: "pilot" });
      this.save();
    }, 2500);
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
  private pollMembership(attempt = 0): void {
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
      if (this.state.s3 !== "paying") return; // resolved or navigated away
      const member = this.identity().wallet;
      // Read the REAL on-chain grant; fold /userinfo for display (never the decision).
      void Promise.all([bridge.membership.grantStatus(member), this.authUserinfo()])
        .then(([grant]) => {
          if (this.state.s3 !== "paying") return;
          // ONLY the genuine on-chain grant settles S3 — never the pre-payment
          // KYC entitlement (Rule 1).
          if (isGrantOnChain(grant) && grant.hasSbt) {
            this.setState({ s3: "settled", s3PollExhausted: false });
            this.save();
            return;
          }
          // Not yet granted — keep polling until the bounded cap.
          if (attempt + 1 < MAX_ATTEMPTS) this.pollMembership(attempt + 1);
          // At the cap we stop polling; S3 stays "paying" (re-enterable) and we SAY SO
          // rather than leaving a spinner that will never resolve. Never faked.
          else this.setState({ s3PollExhausted: true });
        })
        .catch(() => {
          // Transient RPC/read error — retry until the cap; never fabricate a settle.
          if (this.state.s3 === "paying" && attempt + 1 < MAX_ATTEMPTS) this.pollMembership(attempt + 1);
          else if (this.state.s3 === "paying") this.setState({ s3PollExhausted: true });
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
   * see was "Check out in your browser · $48".
   */
  recheckMembership(): void {
    if (this.state.s3 !== "paying") return;
    this.setState({ s3PollExhausted: false });
    this.pollMembership();
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
  async settleS3IfAlreadyGranted(): Promise<boolean> {
    if (BRIDGE_MODE !== "tauri") return false;
    if (this.state.s3 === "settled") return true;
    try {
      const grant = await bridge.membership.grantStatus(this.identity().wallet);
      if (isGrantOnChain(grant) && grant.hasSbt) {
        this.setState({
          s3: "settled",
          s3PollExhausted: false,
          hasGrant: true,
          hasSbt: grant.hasSbt,
        });
        this.save();
        return true;
      }
    } catch {
      // Honest no-op: an unreadable chain is not evidence of a grant.
    }
    return false;
  }
  onS5Begin(): void {
    this.setState({ s5: "verifying", s5c: 0 });
    setTimeout(() => this.setState({ s5c: 1 }), 800);
    setTimeout(() => this.setState({ s5c: 2 }), 1600);
    setTimeout(() => this.setState({ s5c: 3 }), 2400);
    setTimeout(() => {
      // Enter the honest "settling / waiting for the on-chain grant" state and
      // start the bounded poll that settles S5 ONLY from the REAL chain read.
      this.setState({ s5: "settling", s5n: 0 });
      if (BRIDGE_MODE === "tauri") this.pollGrant();
      else this.pollGrantSim();
    }, 3300);
  }

  /**
   * BC-1.3 — the web-dev SIM S5 settle. The sim has NO real 40204, so it derives an
   * HONEST grant status from the persona/AppState via `bridge.membership.grantStatus`
   * (granted only for a paid+active sim member) and settles S5 from THAT — not a
   * blind timer (Rule 1's shape carried into the preview). Bounded like pollGrant.
   */
  private pollGrantSim(attempt = 0): void {
    if (BRIDGE_MODE === "tauri") return;
    const MAX_ATTEMPTS = 20;
    const tick = () => {
      if (this.state.s5 !== "settling") return;
      void bridge.membership
        .grantStatus(this.identity().wallet)
        .then((grant) => {
          if (this.state.s5 !== "settling") return;
          if (isGrantOnChain(grant) && grant.hasSbt) {
            // s5StakeWei carries the sim-derived attributedStake (honest for the
            // preview — grantStatus derives it from the paid persona, not a real
            // chain read; the card renders it, never a hardcoded 32,000).
            this.setState({
              s5: "settled",
              s5n: 32000,
              s5StakeWei: grant.attributedStakeWei,
              // M-2.2/M-2.3: the lock + KYC state, straight from the bond.
              s5BondStatus: describeBond(grant),
              hasGrant: true,
              hasSbt: grant.hasSbt,
            });
            this.save();
            return;
          }
          if (attempt + 1 < MAX_ATTEMPTS) this.pollGrantSim(attempt + 1);
        })
        .catch(() => {
          if (this.state.s5 === "settling" && attempt + 1 < MAX_ATTEMPTS) this.pollGrantSim(attempt + 1);
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
  private pollGrant(attempt = 0): void {
    if (BRIDGE_MODE !== "tauri") return;
    const MAX_ATTEMPTS = 60; // ~5 min at 5s cadence
    const tick = () => {
      if (this.state.s5 !== "settling") return; // resolved or navigated away
      const member = this.identity().wallet;
      // Read the REAL on-chain grant (attributedStake + SBT) and re-check the live
      // entitlement leg. Both must be real for S5 to settle (Rule 1).
      void Promise.all([bridge.membership.grantStatus(member), this.authUserinfo()])
        .then(([grant]) => {
          if (this.state.s5 !== "settling") return;
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
            this.setState({
              s5: "settled",
              s5n: 32000,
              s5StakeWei: grant.attributedStakeWei,
              // M-2.2/M-2.3: the lock + KYC state, straight from the bond.
              s5BondStatus: describeBond(grant),
              hasGrant: true,
              hasSbt: grant.hasSbt,
            });
            this.save();
            return;
          }
          // Not yet fully settled — keep polling until the bounded cap. Never set
          // hasGrant/hasSbt without the real grant.
          if (attempt + 1 < MAX_ATTEMPTS) this.pollGrant(attempt + 1);
          // At the cap we stop; S5 stays "settling" (re-enterable), never faked.
        })
        .catch(() => {
          // A transient RPC/read error — retry until the cap; never fabricate.
          if (this.state.s5 === "settling" && attempt + 1 < MAX_ATTEMPTS) this.pollGrant(attempt + 1);
        });
    };
    setTimeout(tick, 5000);
  }
  onEnter(): void {
    const s = this.state;
    this.setState({
      stage: "done",
      coach: s.coachDone ? -1 : 0,
      chatMsgs: s.chatMsgs.length ? s.chatMsgs : [greeting({ ...this.persona(), name: this.identity().real ? this.identity().name : "" })],
    });
    this.save();
  }

  // ---------- demo panel ----------
  selectPersona(pid: string): void {
    const ns = freshState(pid);
    ns.demoOpen = true;
    this.setState(ns);
    this.save();
  }
  resetProto(): void {
    try {
      localStorage.removeItem(STORAGE_KEY);
    } catch {
      /* ignore */
    }
    const ns = freshState("p1");
    this.setState(ns);
    this.toast("Prototype reset — fresh install");
  }
}

function nodeLabelLocal(n: string): string {
  return (
    { off: "off", prov: "provisioning", syncing: "syncing", synced: "synced", paused: "paused", validating: "validating", error: "error" } as Record<string, string>
  )[n] || n;
}

// One shared store instance for the app.
export const store = new Store();

export function useStore(): AppState {
  return useSyncExternalStore(store.subscribe, store.getSnapshot, store.getSnapshot);
}
