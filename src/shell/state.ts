// =====================================================================
// citrate-core — state model
// Ported 1:1 from the design's inline <script type="text/x-dc"> state
// model (design/CitrateCore.dc.html): constructor + freshState(pid).
// This is the ONE state object the whole shell renders from.
//
// Data source — this is prototype/sim state. Every surface names its real
// source in a "Data source —" caption; wiring replaces the sim, not the UI
// (Rule 1). The single genuinely-live datum (chain-40204 block height) is
// wired separately in the Dashboard via wagmi useBlockNumber.
// =====================================================================

import type { PendingWithdrawal } from "../bridge/domains";
import type { Person } from "../surfaces/peopleDirectory";
import type { CeremonyView } from "../bridge/types";
import { BRIDGE_MODE } from "../bridge/mode";

export const STORAGE_KEY = "citrate-core-proto-v2";

// tier gating order
// Tier ranking for paid-feature gating (isPaidEntitlementActive). Includes BOTH
// the app's internal tiers (free/pilot/enterprise) AND the authority's tier
// vocabulary that /userinfo folds in verbatim (public/commercial/commercial.kyc/
// academic/confidential). ADR-2026-07-25 (payment-as-sybil) grants `commercial`
// to a paid-but-unverified member — it MUST rank above free or the S5 grant poll
// never settles for them (the reported "stuck on step 5"). commercial.kyc is the
// KYC-verified upgrade; confidential is the top enterprise-equivalent.
export const RANK: Record<string, number> = {
  free: 0,
  public: 0,
  pilot: 1,
  commercial: 1,
  "commercial.kyc": 1,
  academic: 1,
  enterprise: 2,
  confidential: 2,
};

export const ORIGIN_COLORS: Record<string, string> = {
  "user wallet action": "#5a8205",
  "node-agent": "#1b4965",
  "chat agent": "#5d60c9",
  "micro-app": "#a8497a",
  // Keyless agent runtimes (CX-S6). Each proposes; the human disposes at the ceremony.
  "agent:hermes": "#5d60c9",
  "agent:openclaw": "#5d60c9",
  "agent:grok": "#5d60c9",
};

export interface Persona {
  id: string;
  name: string;
  initials: string;
  label: string;
  tier: string;
  role: string;
  org: string | null;
  fresh: boolean;
  blurb: string;
  email: string;
}

export const PERSONAS: Record<string, Persona> = {
  p1: { id: "p1", name: "Dana Okafor", initials: "DO", label: "P1 · The Joiner", tier: "pilot", role: "member", org: null, fresh: true, blurb: "Fresh install — walks S0–S6.", email: "dana.okafor@fastmail.com" },
  p2: { id: "p2", name: "Marcus Bell", initials: "MB", label: "P2 · The Operator", tier: "pilot", role: "operator", org: null, fresh: false, blurb: "Onboarded, node validating, deep telemetry.", email: "mbell@protonmail.com" },
  p3: { id: "p3", name: "Priya Anand", initials: "PA", label: "P3 · The Builder", tier: "pilot", role: "builder", org: null, fresh: false, blurb: "SDKs, gateway key, agents on the socket.", email: "priya@lattice.dev" },
  p4: { id: "p4", name: "R. Calloway", initials: "RC", label: "P4 · The Org Seat", tier: "enterprise", role: "org-seat", org: "BA-7", fresh: false, blurb: "Org seat — org-scoped doors render.", email: "r.calloway@boeing.com" },
};

// ------- deterministic address / hash helpers (verbatim from design) -------
export function makeAddr(seed: string): string {
  let h = 2166136261 >>> 0;
  for (let i = 0; i < seed.length; i++) {
    h = Math.imul(h ^ seed.charCodeAt(i), 16777619) >>> 0;
  }
  let out = "";
  for (let k = 0; k < 5; k++) {
    h = Math.imul(h ^ (h >>> 13), 0x5bd1e995) >>> 0;
    out += h.toString(16).padStart(8, "0");
  }
  return "0x" + out.slice(0, 40);
}
export function makeHash(): string {
  const c = "0123456789abcdef";
  let s = "0x";
  for (let i = 0; i < 64; i++) s += c[(Math.random() * 16) | 0];
  return s;
}
export function short(h: string | null | undefined): string {
  return h ? h.slice(0, 6) + "…" + h.slice(-4) : "—";
}

/**
 * BC-1.3 (F1) — format a decimal wei string (18-decimals SALT) to a whole-SALT,
 * thousands-grouped display string (e.g. "32000000000000000000000" → "32,000").
 * Truncates the fractional part (membership grants are whole-SALT). Returns "—"
 * for a null/unparseable input so the settled card NEVER shows a fabricated or
 * malformed number (Rule 1: display only what really read). BigInt so a 32k-SALT
 * grant (far beyond JS safe-int range) is exact.
 */
export function fmtSaltFromWei(wei: string | null | undefined): string {
  if (wei == null) return "—";
  let n: bigint;
  try {
    n = BigInt(wei);
  } catch {
    return "—";
  }
  const salt = n / 10n ** 18n; // truncate to whole SALT
  return salt.toLocaleString("en-US");
}
export function nodeLabel(n: string): string {
  return (
    { off: "off", prov: "provisioning", syncing: "syncing", synced: "synced", paused: "paused", validating: "validating", stalled: "sync stalled", error: "error" } as Record<string, string>
  )[n] || n;
}

// ---------------------- state shapes ----------------------
export interface ChatMsg {
  id: string;
  who: string;
  text: string;
  chips: { label: string; status: string }[];
  streaming: boolean;
}
export interface Activity {
  id: string;
  kind: string;
  amount: string;
  hash: string;
  ts: number;
  /**
   * Q-E.2 (C-5) — the REAL receipt status passed through from the indexer:
   * `1` succeeded, `0` reverted/failed, `null`/absent = pending or unknown.
   * Optimistic local rows (addActivity) omit it (undefined → "pending"). Lets the
   * Activity table mark a failed tx instead of rendering it identically to a
   * success (the bug this closes). Never fabricated (Rule 1).
   */
  status?: number | null;
  /** Q-E.2 — the classified direction ("in"/"out"/"self") from the indexer. */
  direction?: string;
}
export interface LogLine {
  t: string;
  line: string;
  id: number;
}
export interface PeerRow {
  id: string;
  dir: string;
  lat: string;
}
export interface Crash {
  when: string;
  note: string;
}
export interface Pin {
  cid: string;
  bond: number;
  cadH: number;
  nextIn: number;
  last: string;
}
export interface JournalPage {
  id: string;
  title: string;
  kind: "daily" | "page";
  pinned: boolean;
  blocks: string[];
}
export interface CerRow {
  k: string;
  v: string;
}
export interface CerSpec {
  id?: string;
  origin: string;
  requester: string;
  title: string;
  rows: CerRow[];
  cost: string;
  sponsor: string;
  sponsorColor: string;
  warning?: string;
  chainless?: boolean;
  apply?: (hash: string) => void;
}

/**
 * Q-E.1 (@rule8, P0) — the PENDING wallet-review state. A money action
 * (send/stake/withdraw-request/withdraw-claim/claim) builds its ceremony via the
 * bridge and STOPS here: it holds the decoded `CeremonyView` (what will be
 * signed — action / destination / cost, or an undecodable raw-mode warning) for
 * a HUMAN to Approve or Reject. Only an explicit approve calls signing.broadcast
 * (the human-in-the-loop gate). `label` names the action for the modal header;
 * `spendSummary` is an optional plain-language line the caller can pass (the
 * amount the user typed) alongside the decoded view. Never carries key material.
 */
export interface WalletReview {
  kind: "send" | "stake" | "withdraw-request" | "withdraw-claim" | "claim" | "wallet-link" | "agent" | "social";
  label: string;
  view: CeremonyView;
  spendSummary?: string;
  /** True once the user has ticked the raw-mode ack (undecodable calldata). */
  rawAck: boolean;
  /**
   * Called after the review resolves (approved=true on a successful broadcast, false on
   * reject/failure). Used by an AGENT-originated chain effect (CX-S6.3) to release the keyless
   * sidecar's blocked action via `hermes_resolve` only after the human decided at the ceremony.
   */
  onResolved?: (approved: boolean) => void | Promise<void>;
}

export interface AppState {
  persona: string;
  tier: string;
  org: string | null;
  /**
   * CORE-A3 entitlement engine — the `citrate_role` claim (member / operator /
   * builder / org-seat). In a Tauri build this comes from the live /userinfo
   * entitlement claim; in web-dev from the sim persona. Gates Settings RBAC.
   */
  citrateRole: string;
  /**
   * ALF cooperative membership flag (ALF-ND). True iff the `citrate_role` claim is
   * `alf_member` — a CLAIM, never tier-derived (DGX_HANDOFF §4.3). Gates the ALF surface + nav.
   */
  alfMember: boolean;
  /**
   * CORE-A3 identity — the REAL signed-in user, folded from the live /userinfo
   * claims (`sub`, `email`, `wallet_address`). In a Tauri build these are the
   * source of truth for every identity surface (Sidebar, Settings account/RBAC,
   * greeting); the sim `persona` is ONLY a web-dev affordance and must never
   * surface once a real user is signed in (Rule 1 — no prototype identity shown
   * to a real account). `signedIn` gates the switch: true ⇒ render auth identity,
   * false ⇒ fall back to the sim persona. `authName`/`authInitials` are derived
   * from the email local-part (the authority issues no display-name claim).
   */
  signedIn: boolean;
  authSub: string | null;
  authEmail: string | null;
  authName: string | null;
  authInitials: string | null;
  /**
   * Q-A.4b item 10 — did `walletAddr` come from a REAL `wallet_address` claim
   * (folded by applyAuthStatus), or is it the deterministic persona `makeAddr`
   * seed? For a SIGNED-IN user this must be true before any identity surface (S4
   * onboarding) renders the address — otherwise the persona hash is a fabricated
   * wallet and the UI shows "—"/pending instead (Rule 1). Not persisted (identity
   * is re-derived from a live session only). Irrelevant in the web-dev/persona
   * path (signedIn=false), where the labelled persona address is the honest preview.
   */
  walletFromClaim: boolean;
  /**
   * BC-6.3 — the REAL entitlement expiry, folded verbatim from the /userinfo
   * `expires_at` claim (bridge `AuthStatus.expiresAt`; an ISO datetime or unix
   * seconds). Settings "Memberships & billing" renders THIS (an honest "—" when
   * null/absent) — never a hardcoded date (Rule 1). `null` until a signed-in
   * session folds a claim that carries an expiry.
   */
  authExpiresAt: string | null;
  entitlement: "active" | "expiring" | "grace" | "lapsed";
  stage: "s0" | "s1" | "s2" | "s3" | "s4" | "s5" | "s6" | "done";
  s1: "idle" | "waiting" | "attest" | "done";
  s2: "none" | "pending" | "verified" | "failed" | "review";
  s3: "idle" | "paying" | "settled";
  /**
   * The bounded S3 grant poll ran out of budget while still "paying".
   *
   * The poll used to just stop, leaving a spinner with no exit: `s3 === "paying"`
   * renders only a loader, so a member whose grant had not landed yet had NO way
   * to resume waiting and no explanation. That is a dead end, and it is reachable
   * in NORMAL operation — core-membership's grant reconciler runs on a 10-minute
   * cron with a 6-HOUR backoff after a denial, so a grant can legitimately land
   * long after any client-side budget we are willing to spend. Surfacing this flag
   * lets S3 say so honestly and offer a re-check that costs nothing (it re-reads
   * the chain; it never re-opens checkout, so it can never double-charge).
   */
  s3PollExhausted: boolean;
  s5: "idle" | "verifying" | "settling" | "settled";
  s5c: number;
  s5n: number;
  /**
   * BC-1.3 (F1 — Rule 1 display honesty) — the REAL staked principal the settled
   * S5 card renders, as a decimal wei string read from
   * `MembershipStakeVault.attributedStake(member)` via the grant-status read (tauri)
   * or the honest sim `grantStatus` (web-dev). Set ONLY when the grant genuinely
   * lands (both poll paths), never fabricated. `null`/`"0"` until then. The card
   * formats wei→SALT from this — it never shows a hardcoded "32,000".
   */
  s5StakeWei: string | null;
  /**
   * M-2.2/M-2.3 — a plain-language description of the member's bond escrow
   * (`describeBond`), e.g. "Staked · unlocks at block 300,000". Derived ONLY
   * from the real grant-status read; `null` until the grant lands. Replaces the
   * hardcoded "mainnet release policy" line, which described a release flag that
   * no longer exists — the member's own lock + KYC gates govern release now.
   */
  s5BondStatus: string | null;
  kycOutcome: "verified" | "failed" | "review";
  route: string;
  wTab: string;
  nTab: string;
  cTab: string;
  sSec: string;
  coach: number;
  coachDone: boolean;
  demoOpen: boolean;
  toast: string | null;
  height: number;
  peers: number;
  lastCp: number;
  finAge: number;
  node: "off" | "prov" | "syncing" | "synced" | "paused" | "validating" | "stalled" | "error";
  /**
   * What the MEMBER wants the node to do, as distinct from what it is doing.
   *
   * `node` is observed state and is rewritten every 2s by the poller, so it can
   * never express intent: after a quit/restart it read "off" and nothing started
   * the node again — a member's validator silently stayed down until they found
   * the button (observed 2026-08-06). This field is the intent, so launch can
   * auto-start WITHOUT overriding a deliberate stop.
   */
  nodeIntent: "run" | "stopped";
  syncPct: number;
  hb: number;
  cpu: number;
  ram: number;
  logs: LogLine[];
  peerRows: PeerRow[];
  crashes: Crash[];
  liquid: number;
  selfStake: number;
  /**
   * The member's REAL validator bond in the `ValidatorRegistry` (SALT), read from
   * live chain state (`pubkeyOfStaker(bond)` → `stakeOf`) via refreshNode / the grant
   * poll. This is the ONLY signal that the member is a staked validator: under the
   * bond-clone model (2026-08-05) the membership grant deploys + funds the member's
   * `MemberBond` clone with the 32k, and the member ACTIVATES it via
   * `MemberBond.activate` (the clone then bonds into the registry) — until then this
   * is 0 even though `hasGrant` is true. The node's "validating" status gates on this,
   * NOT on `hasGrant` (a granted-but-unactivated member is `synced`, not validating —
   * Rule 1: a funded bond is not the same as an activated one).
   */
  bondedStake: number;
  /**
   * CORE WP2 — the wallet's PENDING (unclaimed) LiquidStakingPool withdrawals,
   * read from live chain state (getLogs WithdrawalRequested + withdrawals(id) +
   * block_number) via refreshPendingWithdrawals(). NOT persisted — the source of
   * truth is chain, re-read on Wallet mount + after a settle. A fresh wallet
   * honestly reads [] (Rule 1 — never fabricated).
   */
  pendingWithdrawals: PendingWithdrawal[];
  hasGrant: boolean;
  hasSbt: boolean;
  earnVal: number;
  earnPin: number;
  earnComp: number;
  earnToday: number;
  claimable: number;
  /**
   * CORE-C2 — where `claimable` currently comes from. "chain" once the real
   * `ContributionAccounting.claimable(addr)` eth_call has resolved (Rule 11 data
   * source); "sim" for the prototype value before/without a live read. The
   * Earning tab captions the claimable card with this so a real read is never
   * confused with the prototype number (Rule 1).
   */
  earnSource: "chain" | "sim";
  activity: Activity[];
  justSigned: string | null;
  queue: CerSpec[];
  cerPhase: "review" | "busy" | "done";
  cerStep: number;
  cerHash: string;
  /** Q-E.1 (@rule8, P0) — the pending wallet money-action review (null = none). */
  walletReview: WalletReview | null;
  chatMsgs: ChatMsg[];
  chatStatus: "ready" | "thinking" | "streaming" | "tool";
  chatBackend: "gateway" | "local";
  storageMode: "lexical" | "dl" | "semantic";
  modelPct: number;
  /**
   * CORE-BC-3 — the local Gemma model lifecycle for the onboarding S6.5 step.
   * `modelState` is the honest, bridge-derived status: it becomes "ready" ONLY
   * after a real SHA-256 verify (never fabricated). `modelDownloadedBytes`/
   * `modelTotalBytes` trace to REAL bytes on disk (Tauri) or the preview animation
   * (web-dev). `modelSkipped` marks that the member chose to run chat on the
   * gateway/demo instead — an honest opt-out, not a failure. NOT persisted config
   * (runtime status, refreshed from the real bridge).
   */
  modelState: "notPresent" | "downloading" | "verifying" | "ready" | "error";
  modelDownloadedBytes: number;
  modelTotalBytes: number;
  modelError: string | null;
  modelSkipped: boolean;
  sel: string | null;
  panX: number;
  panY: number;
  pollIn: number;
  dl: Record<string, { st: string; pct: number }>;
  gwKey: string | null;
  gwKeyFull: string | null;
  rpc: "local" | "public";
  net: "testnet" | "local";
  cpuCap: number;
  autolock: number;
  /**
   * CORE-A2 runtime custody lock state (NOT persisted). In a Tauri build the
   * store refreshes this from the real `custody_status`; in web-dev it is a sim
   * UI shim. `"unknown"` before the first status read.
   */
  custodyLock: "locked" | "unlocked" | "unknown";
  sigPolicy: "hitl" | "allow";
  channel: "stable" | "beta";
  telemetry: boolean;
  updState: "idle" | "checking" | "current";
  dataDir: string;
  /**
   * CORE-D3.C — the core-membership base URL (the S3 checkout opens
   * `{coreMembershipUrl}/checkout`). Persisted config field; overridable to a
   * preview/prod domain. Mirrors `AppConfig.coreMembershipUrl`.
   */
  coreMembershipUrl: string;
  walletAddr: string;
  /**
   * This device's CUSTODY EOA (from `wallet.balances().address`) — the address
   * that actually signs, as opposed to `walletAddr`, which is the authority's
   * `wallet_address` CLAIM. They differ until the member links this wallet, and
   * that difference is exactly what strands a validator bond: the money path grants
   * to the claim (the clone's `onlyMember`), the activation is sent from the custody
   * EOA. Empty until a real balances() read lands (never fabricated).
   */
  custodyAddr: string;
  socketPath: string;
  deviceId: string;
  s1c: number;
  pins: Pin[];
  jPages: JournalPage[];
  jSel: string | null;
  jEditing: boolean;
  jMicOn: boolean;
  jInterim: string;
  jExportOpen: boolean;
  connections: Record<string, boolean>;
  /**
   * CORE-AI1 (@rule8) — the DEFAULT AI provider route id (a non-secret id like
   * "openai" / "gateway" / "custom"). The PROVIDER KEY is NOT here and NOT
   * persisted: it lives sealed in the OS keyring (Rust), never in AppState or
   * localStorage. `aiKeys` was REMOVED — the webview holds no key (invariant 2).
   * Whether a route is actually usable is read live via bridge.chat.providerStatus.
   */
  aiDefault: string;
  aiEdit: string | null;
  sponsorUnits: number;
  blocksProposed: number;
  graphQ: string;
  dataReady: boolean;
  s6ready?: boolean;
  /**
   * CORE-C3 runtime memory graph (NOT persisted). In a Tauri build the Storage
   * surface fetches this from the REAL mcp_serve daemon via
   * `bridge.memory.constellation()`; nodes/links/tenant-totals come from the
   * per-user encrypted store — never fabricated (Rule 1). `memGraphState`
   * reflects the fetch honestly: "idle" before the first read, "loading" while
   * in flight, "ready" once real nodes arrive, "unavailable" when the daemon is
   * not running / the socket is unreachable (the UI then shows an honest
   * empty/offline state, never a sim graph).
   */
  memGraph?: MemGraph;
  memGraphState: "idle" | "loading" | "ready" | "unavailable";
  /**
   * CONNECT-S0 — the People directory: everyone you share a group with, derived live from your group
   * rosters + verified faces (never fabricated — Rule 1). `peopleState` reflects the fetch honestly:
   * "loading" before/while reading, "ready" once the real aggregation lands, "unavailable" if groups
   * can't be read (the UI shows an honest empty/offline state, never invented people).
   */
  people: Person[];
  peopleState: "loading" | "ready" | "unavailable";
  /**
   * Q-A.4a runtime memory-daemon status (NOT persisted, never fabricated). Read
   * from the REAL `memory_status()` on Storage mount / after a Start. `memDaemon`
   * is the honest supervisor state ("idle" before the first read, "running" when
   * the daemon reports it, "offline" when unreachable/not started, "error" on a
   * start failure — e.g. the mem-mcp binary is not bundled yet → BinaryNotFound).
   * `memSocketPath` is the REAL socket path the daemon reports (null until known —
   * NEVER the old client-invented `~/.citrate/core/memory/<persona>.sock`
   * constant). `memSemantic` mirrors the daemon's `semantic` flag: the bge
   * embedding model is bundled WITH the daemon, so semantic search is available
   * ONLY when the daemon reports `semantic: true` (never a fabricated download).
   */
  memDaemon: "idle" | "running" | "offline" | "error";
  memDaemonError: string | null;
  memSocketPath: string | null;
  memSemantic: boolean;
}

/** One tenant's real node count + its parsed nodes, from the memory daemon. */
export interface MemTenant {
  tenant: string;
  totalInTenant: number;
}
/** A laid-out memory-graph node (deterministic layout over the real store). */
export interface MemGraphNode {
  id: string;
  label: string;
  tenant: string;
  kind: string;
  detail: string;
  x: number;
  y: number;
  z: number;
}
/** The real memory constellation: laid-out nodes + tenant totals. Links are
 * derived deterministically from tenant adjacency (edges are a per-node fetch). */
export interface MemGraph {
  nodes: MemGraphNode[];
  links: [string, string][];
  tenants: MemTenant[];
}

function greeting(P: Persona): ChatMsg {
  // Personalize ONLY with a real (or web-dev persona) first name. An empty name
  // — the packaged app before sign-in — greets neutrally, never a mock name.
  const first = P.name.split(" ")[0];
  const hello = first ? "Welcome back, " + first + ". " : "Welcome. ";
  return {
    id: "g0",
    who: "Agent",
    text:
      hello +
      "I read your node, wallet, and memory graph — and anything I want to write comes back to you for approval. Ask me about your staking position, earnings, or the network.",
    chips: [],
    streaming: false,
  };
}
export { greeting };

function seedActivity(items: string[]): Activity[] {
  const now = Date.now();
  return items.map((it, i) => {
    const [kind, amount] = it.split("|");
    return { id: "seed" + i, kind, amount, hash: makeHash(), ts: now - (i + 1) * 3600e3 * (6 + i * 9) };
  });
}

export function freshState(pid: string): AppState {
  const P = PERSONAS[pid] || PERSONAS.p1;
  const first = P.name.split(" ")[0].toLowerCase().replace(/[^a-z]/g, "");
  const s: AppState = {
    persona: pid,
    tier: P.fresh ? "free" : P.tier,
    org: P.org,
    citrateRole: P.role,
    alfMember: false, // set true only by applyAuthStatus when the alf_member claim is present
    // Real identity is empty until a live sign-in folds /userinfo claims in
    // (applyAuthStatus). Until then the sim persona is the display fallback.
    signedIn: false,
    authSub: null,
    authEmail: null,
    authName: null,
    authInitials: null,
    walletFromClaim: false,
    authExpiresAt: null,
    entitlement: "active",
    stage: P.fresh ? "s0" : "done",
    s1: "idle",
    s2: "none",
    s3: "idle",
    s3PollExhausted: false,
    s5: "idle",
    s5c: 0,
    s5n: 0,
    s5StakeWei: null,
    s5BondStatus: null,
    kycOutcome: "verified",
    route: "dashboard",
    wTab: "overview",
    nTab: "ops",
    cTab: "apps",
    sSec: "account",
    coach: -1,
    coachDone: !P.fresh,
    demoOpen: false,
    toast: null,
    height: 131204 + ((Math.random() * 40) | 0),
    peers: 0,
    lastCp: 131200,
    finAge: 8,
    node: "off",
    // Default RUN: a fresh install should bring its node up on first launch.
    nodeIntent: "run",
    syncPct: 0,
    hb: 4,
    cpu: 0,
    ram: 0,
    logs: [],
    peerRows: [],
    crashes: [],
    liquid: 0,
    selfStake: 0,
    bondedStake: 0,
    pendingWithdrawals: [],
    hasGrant: false,
    hasSbt: false,
    earnVal: 0,
    earnPin: 0,
    earnComp: 0,
    earnToday: 0,
    claimable: 0,
    earnSource: "sim",
    activity: [],
    justSigned: null,
    queue: [],
    cerPhase: "review",
    cerStep: 0,
    cerHash: "",
    walletReview: null,
    chatMsgs: [],
    chatStatus: "ready",
    chatBackend: "gateway",
    storageMode: "lexical",
    modelPct: 0,
    modelState: "notPresent",
    modelDownloadedBytes: 0,
    modelTotalBytes: 5_335_289_824,
    modelError: null,
    modelSkipped: false,
    sel: null,
    panX: 0,
    panY: 0,
    pollIn: 20,
    dl: {},
    gwKey: null,
    gwKeyFull: null,
    rpc: "local",
    net: "testnet",
    cpuCap: 50,
    autolock: 30,
    // Sim default: the prototype presents an unlocked, provisioned vault so the
    // Keys-&-security section still renders. A Tauri build overwrites this from
    // the real custody_status on load.
    custodyLock: "unlocked",
    sigPolicy: "hitl",
    channel: "stable",
    telemetry: false,
    updState: "idle",
    dataDir: "~/.citrate/core",
    coreMembershipUrl: "https://core-membership.vercel.app",
    walletAddr: makeAddr(P.name),
    custodyAddr: "",
    socketPath: "~/.citrate/core/memory/" + first + ".sock",
    deviceId: "dev_" + makeAddr(P.name + "::device").slice(2, 12),
    s1c: 0,
    pins: [],
    jPages: [],
    jSel: null,
    jEditing: false,
    jMicOn: false,
    jInterim: "",
    jExportOpen: false,
    connections: {},
    // AI provider KEYS live in the OS keyring (Rust), never in AppState. Only the
    // non-secret default route id is kept here (AI1, invariant 2).
    aiDefault: "gateway",
    aiEdit: null,
    sponsorUnits: 4,
    blocksProposed: 0,
    graphQ: "",
    dataReady: true,
    memGraphState: "idle",
    people: [],
    peopleState: "loading",
    memDaemon: "idle",
    memDaemonError: null,
    memSocketPath: null,
    memSemantic: false,
  };
  const today = new Date().toISOString().slice(0, 10);
  if (P.fresh) {
    s.jPages = [
      {
        id: "d-" + today,
        title: today,
        kind: "daily",
        pinned: false,
        blocks: [
          "Welcome — this journal is local and yours. One bullet per line, [[Page name]] to link.",
          "Agents write here only with your approval, every time.",
        ],
      },
    ];
  } else {
    s.jPages = [
      {
        id: "d-" + today,
        title: today,
        kind: "daily",
        pinned: false,
        blocks: [
          "Node validated through the night — [[Validator runbook]] holds.",
          "@agent claim batched at 09:12 · 12.41 SALT — approved by you",
          "  follow up: raise the bond on kq4e before the next window",
          "Reviewed [[Agent worklog]] — indexing is ahead of schedule.",
        ],
      },
      {
        id: "p-runbook",
        title: "Validator runbook",
        kind: "page",
        pinned: true,
        blocks: [
          "Keep stake ≥ 32,000 — eligibility is a hard floor.",
          "The heartbeat is slashing protection. Never kill -9 the agent mid-job.",
          "Claims batch until ≥ 5 SALT to avoid dust gas.",
          "Pause before OS upgrades; resume re-attests the machine.",
        ],
      },
      {
        id: "p-worklog",
        title: "Agent worklog",
        kind: "page",
        pinned: false,
        blocks: [
          "@agent 2026-07-10 · researched x402 settlement flows for the marketplace brief",
          "@agent 2026-07-11 · indexed the 40204 contract catalog into chain-facts",
          "@agent drafts live here — off-chain, local, recallable via [[" + today + "]]",
        ],
      },
    ];
  }
  s.jSel = s.jPages[0].id;
  if (!P.fresh) {
    s.hasGrant = true;
    s.hasSbt = true;
    s.pins = [
      { cid: "bafybeigd4x…kq4e", bond: 120, cadH: 6, nextIn: 4520, last: "attested" },
      { cid: "bafybeif7ta…m2rw", bond: 80, cadH: 6, nextIn: 12980, last: "attested" },
      { cid: "bafybeih0pz…x9c3", bond: 200, cadH: 12, nextIn: 31000, last: "pending" },
    ];
    if (pid === "p3") {
      s.pins = s.pins.slice(0, 1);
      s.connections = { github: true, hf: true, notion: true };
    }
    if (pid === "p2") {
      s.sponsorUnits = 3;
      s.blocksProposed = 412;
    }
    if (pid === "p4") {
      s.blocksProposed = 908;
      s.connections = { outlook: true, planner: true };
    }
    if (pid === "p2") {
      s.node = "validating";
      s.peers = 23;
      s.liquid = 142.63;
      s.claimable = 9.41;
      s.earnVal = 96.42;
      s.earnPin = 21.7;
      s.earnComp = 9.31;
      s.earnToday = 3.86;
      s.selfStake = 2500;
      s.crashes = [{ when: "2026-07-06 03:12 · exit 137", note: "OOM under compaction — restarted with backoff in 4s; heartbeat continuity held." }];
      s.activity = seedActivity(["Claim rewards|+12.41 SALT", "Add stake|−2,500.00 SALT", "Send|−40.00 SALT", "Grant + stake ceremony|32,000 SALT"]);
    } else if (pid === "p3") {
      s.node = "off";
      s.liquid = 58.02;
      s.claimable = 2.14;
      s.earnVal = 30.11;
      s.earnPin = 4.02;
      s.earnComp = 6.4;
      s.gwKey = "cgk_9f31•••••••••••••7f2a";
      s.activity = seedActivity(["Claim rewards|+8.90 SALT", "Grant + stake ceremony|32,000 SALT"]);
    } else if (pid === "p4") {
      s.node = "validating";
      s.peers = 26;
      s.liquid = 301.44;
      s.claimable = 18.92;
      s.earnVal = 240.5;
      s.earnPin = 51.2;
      s.earnComp = 22.8;
      s.earnToday = 5.12;
      s.selfStake = 8000;
      s.activity = seedActivity(["Claim rewards|+22.05 SALT", "Add stake|−8,000.00 SALT", "Grant + stake ceremony|32,000 SALT"]);
    }
    // Packaged app (tauri): never seed the sim persona's name into the greeting.
    // Real identity folds in on sign-in; until then greet neutrally (Rule 1).
    s.chatMsgs = [greeting(BRIDGE_MODE === "tauri" ? { ...P, name: "" } : P)];
  }
  return s;
}

// keys persisted (verbatim from design save())
export const PERSIST_KEYS: (keyof AppState)[] = [
  "persona", "tier", "org", "citrateRole", "entitlement", "stage", "s2", "s3", "s5", "s5n", "hasGrant", "hasSbt",
  // NOTE: the real-identity fields (signedIn/authSub/authEmail/authName/
  // authInitials/authExpiresAt) are DELIBERATELY NOT persisted — HIPAA sign-out-by-default.
  // Every launch starts signed-out; identity is re-derived only from a live
  // authority session (refreshAuth → auth.status), never from disk. No account
  // PII is written to localStorage.
  "liquid", "selfStake", "bondedStake", "earnVal", "earnPin", "earnComp", "earnToday", "claimable", "activity",
  "node", "nodeIntent", "syncPct", "peers", "gwKey", "rpc", "net", "cpuCap", "autolock", "sigPolicy", "channel",
  "telemetry", "storageMode", "coachDone", "dataDir", "coreMembershipUrl", "s5StakeWei", "s5BondStatus", "walletAddr", "socketPath",
  "kycOutcome", "chatBackend", "crashes", "wTab", "nTab", "cTab", "sSec", "route", "deviceId",
  // NOTE: `aiKeys` is REMOVED (AI1) — provider keys live in the OS keyring, never
  // localStorage (invariant 2). Only the non-secret `aiDefault` route id persists.
  "pins", "jPages", "jSel", "connections", "aiDefault", "sponsorUnits", "blocksProposed",
];

export function loadState(): AppState {
  let saved: Partial<AppState> | null = null;
  try {
    saved = JSON.parse(localStorage.getItem(STORAGE_KEY) || "null");
  } catch {
    saved = null;
  }
  const base = freshState((saved && saved.persona) || "p1");
  if (saved) Object.assign(base, saved, { queue: [], toast: null, demoOpen: false, chatStatus: "ready" });
  return base;
}
