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

import type { Brief } from "../bridge/domains";
import type { CeremonyView } from "../bridge/types";
import type { ApprovalCard, HicRequirement } from "../agent/approvalCards";
import type { DeployGateRecord } from "../agent/deployGate";
import type { RunRecord } from "../agent/runTimeline";
import type { WorkerTerms, WorkerTermsReceipt } from "../fl/workerTerms";
import type { ChatAttachment } from "../agent/chatAttachments";
import type { RoundDraft } from "../fl/roundDraft";
import { BRIDGE_MODE } from "../bridge/mode";
import { personaFor, type Persona } from "./state/persona";
import { freshCore, CORE_PERSIST_SCOPE, type CoreState } from "./state/core";
import { freshL1, L1_PERSIST_SCOPE, type L1State } from "./state/l1";
import { freshL2, L2_PERSIST_SCOPE, type L2State } from "./state/l2";
import { freshL3, L3_PERSIST_SCOPE, type L3State } from "./state/l3";
import { freshL4, L4_PERSIST_SCOPE, type L4State } from "./state/l4";
import { freshL5, L5_PERSIST_SCOPE, type L5State } from "./state/l5";
import { freshL6, L6_PERSIST_SCOPE, type L6State } from "./state/l6";
import { freshL7, L7_PERSIST_SCOPE, type L7State } from "./state/l7";
import { freshL8, L8_PERSIST_SCOPE, type L8State } from "./state/l8";
import { freshL9, L9_PERSIST_SCOPE, type L9State } from "./state/l9";
import {
  appendNetArchive,
  currentNetKey,
  NET_READS,
  NET_STAMP_FIELD,
  readNetArchive,
  savedNetKey,
  type PersistScope,
} from "./netScope";

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

// The sim personas and the address helper live in ./state/persona.ts (a leaf the parts read).
export { makeAddr, PERSONAS, type Persona } from "./state/persona";

// ------- deterministic hash helper (verbatim from design) -------
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
    { off: "off", prov: "provisioning", syncing: "syncing", synced: "synced", paused: "paused", validating: "validator active", stalled: "sync stalled", error: "error" } as Record<string, string>
  )[n] || n;
}

// ---------------------- state shapes ----------------------
export interface ChatMsg {
  id: string;
  who: string;
  text: string;
  chips: { label: string; status: string }[];
  streaming: boolean;
  /** HUP-S0.7 — the turn failed (timeout, provider, transport). Shown inline with Retry; never
   *  sent back to the model as context. */
  error?: string;
  /** HUP-S0.7 — the user text to resend on Retry. */
  retryText?: string;
  /** rc.5 FIX 5 — the error is the contract-toolchain refusal: offer "Open toolchain settings". */
  toolchainHelp?: boolean;
  /** HUP-S1.4 — an accepted brief, shown as a card in the thread (text holds its markdown). */
  brief?: Brief;
  /** FL design board 01: the steps Hermes took for this reply, from the turn's real events
   *  (src/agent/runTimeline.ts). Absent on replies made before 0.5.4's timeline. */
  run?: RunRecord;
  /** Board 01: the model this reply ran on, as the chat header named it when the turn began. */
  model?: string;
  /** Board 01: a proposal core renders in the transcript for the member to decide (HIC-1). */
  proposal?: ChatProposal;
  /** Board 07: a round draft Hermes proposed; the member may open it in Open a round. */
  roundDraft?: ChatRoundDraft;
  /** #690: what the member attached to this message with @ (shown as chips; told to Hermes). */
  attachments?: ChatAttachment[];
}
/** Board 07: a round draft card. Only core writes it; only the member's click opens the draft. */
export interface ChatRoundDraft {
  id: string;
  draft: Partial<RoundDraft>;
  /** `opened`: the member handed it to Open a round. `expired`: restored from history or
   *  carried into a branch, so it can no longer be opened from here. */
  state: "pending" | "opened" | "declined" | "expired";
  at?: number;
}
/** Board 01: a worker-terms proposal card. Only core writes it; Hermes can only ask for one. */
export interface ChatProposal {
  id: string;
  kind: "worker_terms";
  terms: WorkerTerms;
  /** `expired`: restored from history or carried into a branch; it can no longer be confirmed
   *  (its terms were proposed in another session), so Hermes must propose again. */
  state: "pending" | "saving" | "confirmed" | "declined" | "failed" | "expired";
  /** The receipt core's storage returned on Confirm (what "Stored" may say comes only from it). */
  receipt?: WorkerTermsReceipt;
  /** When the member decided (ms since epoch). */
  at?: number;
  /** Why saving failed (shown with "Save failed, nothing changed"). */
  error?: string;
}
/** Board 01: a message the member typed while Hermes was working. `steer`: it goes as soon as the
 *  stopped run has ended; otherwise it waits for the run to finish. Never persisted. */
export interface QueuedChat {
  id: string;
  text: string;
  steer: boolean;
  /** #690: what was attached with @ when it was typed. */
  attachments?: ChatAttachment[];
  /** The conversation it was typed in. It is only ever sent there. */
  threadId: string;
}
/** HUP-S1.4 — the last brief the member accepted (persisted; nothing is built from it yet). */
export interface AcceptedBrief {
  brief: Brief;
  markdown: string;
  acceptedAt: number;
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
  /** HUP-S2.4 — the card generated from the tool's annotations (diff / chain / command / fields). */
  card?: ApprovalCard;
  /** HUP-S2.4 — set when the sidecar marked the call hic:"required": explicit decision only. */
  hic?: HicRequirement;
}

/**
 * Q-E.1 (@rule8, P0) — the PENDING wallet-review state. A money action
 * (send/stake/claim) builds its ceremony via the
 * bridge and STOPS here: it holds the decoded `CeremonyView` (what will be
 * signed — action / destination / cost, or an undecodable raw-mode warning) for
 * a HUMAN to Approve or Reject. Only an explicit approve calls signing.broadcast
 * (the human-in-the-loop gate). `label` names the action for the modal header;
 * `spendSummary` is an optional plain-language line the caller can pass (the
 * amount the user typed) alongside the decoded view. Never carries key material.
 */
/** C1-V3 — a signed transaction that was (or may have been) submitted, with no known outcome. */
export interface CeremonyOutcomeUnknown {
  kind: "outcome-unknown";
  /** The tx hash when the ceremony knows it; null when the failure carried none. */
  txHash: string | null;
  reason: string;
}

export interface WalletReview {
  kind: "send" | "stake" | "claim" | "wallet-link" | "device-link" | "agent" | "social" | "deploy" | "directory-publish" | "directory-revoke" | "contract-call" | "web-sign-in";
  label: string;
  view: CeremonyView;
  spendSummary?: string;
  /** True once the user has ticked the raw-mode ack (undecodable calldata). */
  rawAck: boolean;
  /**
   * True from the moment Approve hands the ceremony to the signer until the signer returns.
   * While set, the review cannot be declined (Reject / Escape are no-ops) and the modal shows
   * a signing state: the outcome is reported only once it is known, never assumed.
   */
  approving?: boolean;
  /**
   * Called after the review resolves (approved=true on a successful broadcast, false on
   * reject/failure). Used by an AGENT-originated chain effect (CX-S6.3) to release the keyless
   * sidecar's blocked action via `hermes_resolve` only after the human decided at the ceremony.
   */
  onResolved?: (approved: boolean, txHash?: string, outcome?: CeremonyOutcomeUnknown) => void | Promise<void>;
  /**
   * #366 F2 — a caution the member must see before approving, rendered as an alert on the review
   * (e.g. an identical payment submitted earlier with an unknown outcome, and its tx hash).
   */
  warning?: string;
  /**
   * Join-8 — when set, approval signs FIRST without sending (`signing.prepare`), calls this
   * SYNCHRONOUSLY with the signed tx's hash, and only then sends it (`signing.sendPrepared`,
   * which polls for the receipt before returning). A caller that must survive a quit at any
   * point after signing (S7 shows the activation tx as Pending) gets the hash here. The durable
   * record is written in Rust (#347's fsync'd submitted-tx record) by the send itself, before
   * the tx reaches the node; with no durable record nothing is sent.
   */
  onSigned?: (txHash: string) => void;
  /** Join-8 — with `onSigned`: nothing was sent (the signed tx expired in the ceremony, or the
   *  backend refused the send before submission, e.g. no durable record). */
  onNotSent?: () => void;
  /** HUP-S2.4 — the chain card (from the decoder's view) and, for a hic:"required" call, why. */
  card?: ApprovalCard;
  hic?: HicRequirement;
  /** HUP-S6.4 — for a contract deploy: the READY D-4 gate record core checked for this bytecode. */
  deployGate?: DeployGateRecord;
}

/**
 * The one state object the shell renders from, composed from one part per lane (L0-WP06,
 * lanes/L0_INTEGRATION_SEAMS.md 3.4). Each field, its freshState default and its persisted scope
 * live in exactly one part under ./state/. New 0.6.0 state goes into a registered slice
 * (slices/registry.ts), never into AppState.
 */
export interface AppState
  extends CoreState,
    L1State,
    L2State,
    L3State,
    L4State,
    L5State,
    L6State,
    L7State,
    L8State,
    L9State {}

/** The keys that more than one of `P` declares. */
type SharedKeys<P extends object[]> = P extends [infer H, ...infer R extends object[]]
  ? (keyof H & { [I in keyof R]: keyof R[I] }[number]) | SharedKeys<R>
  : never;
type NoSharedKeys<K extends never> = K;
/**
 * Compile time (L0-WP06): no field is declared in two parts. `extends` accepts a field declared
 * twice with one type, so this names it instead: tsc fails here with the shared key. Optional
 * fields included; statePartition.test.ts checks the same at run time.
 */
export type StatePartsAreDisjoint = NoSharedKeys<
  SharedKeys<[CoreState, L1State, L2State, L3State, L4State, L5State, L6State, L7State, L8State, L9State]>
>;

/** HUP-S3.1 — see `AppState.knowledgeImport`. */
export interface KnowledgeImportState {
  state: "idle" | "running" | "imported" | "skipped" | "failed";
  tenant: string | null;
  done: number;
  total: number;
  nodesAdded: number;
  message: string | null;
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
      "I read your node, wallet, and memory graph — and anything I want to write comes back to you for approval. Ask me about your staking position, validator rewards, or the network.",
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
  const P = personaFor(pid);
  const s: AppState = {
    ...freshCore(pid),
    ...freshL1(pid),
    ...freshL2(pid),
    ...freshL3(pid),
    ...freshL4(pid),
    ...freshL5(pid),
    ...freshL6(pid),
    ...freshL7(pid),
    ...freshL8(pid),
    ...freshL9(pid),
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
          "Stop the node from the Node page rather than killing it mid-job.",
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
      s.activity = seedActivity(["Claim rewards|+12.41 SALT", "Send|−40.00 SALT", "Membership grant ceremony|32,000 SALT"]);
    } else if (pid === "p3") {
      s.node = "off";
      s.liquid = 58.02;
      s.claimable = 2.14;
      s.activity = seedActivity(["Claim rewards|+8.90 SALT", "Membership grant ceremony|32,000 SALT"]);
    } else if (pid === "p4") {
      s.node = "validating";
      s.peers = 26;
      s.liquid = 301.44;
      s.claimable = 18.92;
      s.activity = seedActivity(["Claim rewards|+22.05 SALT", "Membership grant ceremony|32,000 SALT"]);
    }
    // Packaged app (tauri): never seed the sim persona's name into the greeting.
    // Real identity folds in on sign-in; until then greet neutrally (Rule 1).
    s.chatMsgs = [greeting(BRIDGE_MODE === "tauri" ? { ...P, name: "" } : P)];
  }
  return s;
}

// Every persisted key with its scope (RC52). `network`: read from one chain, so it belongs to that
// chain's genesis; a save of another genesis has these archived and reset on load (netScope.ts).
// `device`: this install's own (wallet identity, settings, journal, Hermes), kept across a switch.
// A new persisted key must be given a scope in its part's table (./state/<part>.ts,
// L<N>_PERSIST_SCOPE); persistScope.test.ts pins the whole table.
const N = "network" as const;

/**
 * L0-WP06: the order the 0.5.9 save wrote its keys in. The composed table keeps it, so a save of
 * the same state is the same bytes as before the split; a key added since follows, in part order.
 * Only L0 edits this list, and only to drop a key that is no longer persisted.
 */
const SAVE_ORDER_059: readonly string[] = [
  "persona", "tier", "org", "citrateRole", "entitlement", "stage", "s2", "s3", "s5", "s5n",
  "hasGrant", "hasSbt", "liquid", "bondedStake", "claimable", "activity", "nodeIntent", "rpc",
  "net", "cpuCap", "autolock", "channel", "telemetry", "telemetryConsentVersion", "storageMode",
  "coachDone", "dataDir", "coreMembershipUrl", "s5StakeWei", "s5BondStatus", "walletAddr",
  "commsAddr", "socketPath", "activeModelId", "kycOutcome", "chatBackend", "wTab", "cTab", "sSec",
  "route", "deviceId", "pins", "userSkills", "jPages", "jSel", "connections", "aiDefault",
  "hermesSidecarLoop", "hermesSidecarLoopDefaultApplied", "hermesTerminal",
  "hermesTerminalDefaultApplied", "hermesBrief", "hermesPersona", "customPersonas",
  "hermesReadAloud", "journalAutoSummary", "journalAutoSummaryDay", "sidebarCollapsed",
  "hermesRailCollapsed", "sponsorUnits", "blocksProposed", "netUnread", "networkNotice",
];

function inSaveOrder<T extends Record<string, PersistScope>>(scope: T): T {
  const rank = (k: string) => {
    const i = SAVE_ORDER_059.indexOf(k);
    return i < 0 ? SAVE_ORDER_059.length : i;
  };
  return Object.fromEntries(Object.entries(scope).sort(([a], [b]) => rank(a) - rank(b))) as T;
}

export const PERSIST_SCOPE = inSaveOrder({
  ...CORE_PERSIST_SCOPE,
  ...L1_PERSIST_SCOPE,
  ...L2_PERSIST_SCOPE,
  ...L3_PERSIST_SCOPE,
  ...L4_PERSIST_SCOPE,
  ...L5_PERSIST_SCOPE,
  ...L6_PERSIST_SCOPE,
  ...L7_PERSIST_SCOPE,
  ...L8_PERSIST_SCOPE,
  ...L9_PERSIST_SCOPE,
}) satisfies Partial<Record<keyof AppState, PersistScope>>;

// keys persisted (verbatim from design save(), plus the RC52 network fields)
export const PERSIST_KEYS = Object.keys(PERSIST_SCOPE) as (keyof typeof PERSIST_SCOPE)[];

/** The persisted keys that belong to one genesis. */
export const NETWORK_KEYS = PERSIST_KEYS.filter((k) => PERSIST_SCOPE[k] === N);

/** Observed node state: rewritten by every poll, so it is never persisted or restored. */
export const OBSERVED_NOT_PERSISTED: (keyof AppState)[] = ["node", "syncPct", "peers", "height", "nodeStatusAt"];

/** D-11 (P-9d): an older build persisted a gateway key display. Provider keys live only in the OS
 *  keychain, so these are retired, and a save that still holds them is rewritten without them. */
const LEGACY_GATEWAY_KEYS = ["gwKey", "gwKeyFull"] as const;

/** Keys earlier builds persisted that are retired, dropped from a save when it loads:
 *  - D-9 (2026-10-26): the liquid staking pool is retired, so its self-stake figure is not current.
 *  - C1-V V8: the off-chain earnings estimates (earnVal / earnPin / earnComp / earnToday).
 *  - K3: the 0.5.1 Node screen's own tab switch (nTab) and its seeded crash rows (crashes); the
 *    Node place's tabs are routes, and the last crash record is read from node_last_crash.
 *  - D-11 (P-9d): the gateway key display (`LEGACY_GATEWAY_KEYS`). */
export const RETIRED_KEYS: readonly string[] = ["selfStake", "earnVal", "earnPin", "earnComp", "earnToday", "nTab", "crashes", ...LEGACY_GATEWAY_KEYS];

export function loadState(netKey: string = currentNetKey()): AppState {
  let saved: Partial<AppState> | null = null;
  try {
    saved = JSON.parse(localStorage.getItem(STORAGE_KEY) || "null");
  } catch {
    saved = null;
  }
  if (saved !== null && (typeof saved !== "object" || Array.isArray(saved))) saved = null;
  const base = freshState((saved && saved.persona) || "p1");
  base.netKey = netKey;
  // C1-V V10: observed node state is never restored from disk. Older builds persisted it, so
  // drop it from a saved state; the first poll is the only source (contract rules 2 and 4).
  if (saved) for (const k of OBSERVED_NOT_PERSISTED) delete saved[k];
  // Upgrade review: a 0.5.1 save may still carry retired keys; never fold them into state.
  // D-11 (P-9d): a save holding a gateway key display is rewritten at once, not at the next save.
  const hadGatewayKey = !!saved && LEGACY_GATEWAY_KEYS.some((k) => k in (saved as object));
  if (saved) for (const k of RETIRED_KEYS) delete (saved as Record<string, unknown>)[k];
  if (saved && hadGatewayKey) {
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify(saved));
    } catch {
      /* storage unavailable: the next save writes PERSIST_KEYS only, which never include them */
    }
  }
  // RC52: values read from another genesis's chain are never restored as current. They are
  // archived under that genesis, reset to "not read yet", and the switch notice is raised.
  if (saved) {
    const raw = saved as Record<string, unknown>;
    const from = savedNetKey(raw);
    delete raw[NET_STAMP_FIELD];
    for (const k of ["netKey", "netArchived"]) delete raw[k];
    if (from !== netKey) {
      const values: Record<string, unknown> = {};
      for (const k of NETWORK_KEYS) {
        if (k in raw) values[k] = raw[k];
        delete raw[k];
      }
      const at = Date.now();
      const archived = appendNetArchive({ genesis: from, archivedAt: at, values });
      saved.netUnread = [...NET_READS];
      saved.networkNotice = { at, from, to: netKey, node: null, archived };
    }
  }
  if (saved) Object.assign(base, saved, { queue: [], toast: null, demoOpen: false, chatStatus: "ready", chatRouteHold: null, chatQueue: [], chatHermesDown: null });
  // CLAUDE.md Rule 3: there is no signature-policy setting (every signature goes through the
  // ceremony; the one exception is a budgeted web sign-in). An older build persisted one, so drop
  // whatever was stored rather than carry it in state.
  if (saved) delete (base as { sigPolicy?: unknown }).sigPolicy;
  // 0.5.2 Join: a telemetry consent stored without a disclosure-version marker was given before
  // `joinSteps` existed. Mark it now (version 1) so the one-time note runs and join steps stay
  // unrecorded until the member has read it.
  if (base.telemetry && typeof base.telemetryConsentVersion !== "number") base.telemetryConsentVersion = 1;
  // Owner decision 2026-10-01: the sidecar agent loop is on for members. Installs that saved the
  // old default (off) are switched on once; a member who turns it off afterwards stays off.
  if (saved && saved.hermesSidecarLoopDefaultApplied !== true) {
    base.hermesSidecarLoop = true;
    base.hermesSidecarLoopDefaultApplied = true;
  }
  // D-11: the preferred cloud model is core's setting; a value an older build stored is ignored.
  base.hermesCloudModel = null;
  // Owner decision 2026-10-19 (supersedes 2026-10-04): terminal commands are off by default for new
  // installs. A stored value is kept as it is (on or off); only an install with no stored value
  // takes the default from freshState. The old switch-on-once migration is retired.
  base.netArchived = readNetArchive();
  return base;
}
