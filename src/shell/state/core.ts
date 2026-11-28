// =====================================================================
// citrate-core: an AppState part (L0-WP06)
//
// L0 (the spine: node, wallet and validator reads, custody, config, storage and memory, the
// journal, routes, the ceremony queue and the wallet review gate).
//
// L0-WP06 (lanes/L0_INTEGRATION_SEAMS.md 3.4). Owned by L0 (.agentile/v60-ownership.map).
// These fields moved here from state.ts verbatim, each with its freshState default and its
// PERSIST_SCOPE entry. state.ts composes AppState, freshState and PERSIST_SCOPE from every part;
// a field lives in exactly one part (src/shell/seams/statePartition.test.ts).
//
// New 0.6.0 state does not go here: it goes into a slice registered with registerSlice
// (src/shell/slices/registry.ts). This part changes when one of its legacy fields moves or retires.
// The persona seeding in freshState (the P2 to P4 sim values) stays in state.ts.
// =====================================================================

import { personaFor, makeAddr } from "./persona";
import type { Activity, LogLine, Pin, JournalPage, CerSpec, WalletReview, KnowledgeImportState, MemGraph } from "../state";
import type { ActivationStatusView, NodeCrashLoopView } from "../../bridge/domains";
import type { SyncProgress } from "../syncProgress";
import type { ValidatorActivation, ValidatorCounts } from "../chainTruth";
import { currentNetKey, type ArchivedNetwork, type NetRead, type NetworkNotice, type PersistScope } from "../netScope";

export interface CoreState {
  persona: string;
  route: string;
  wTab: string;
  cTab: string;
  sSec: string;
  toast: string | null;
  /**
   * The local node's `eth_blockNumber`. 0 means no reading yet (UI truth contract §2: the
   * call returns 0x0 on error, so 0 is never shown as a height). There is no finality-age
   * field: checkpoint finality is not running on 40204 (contract §4), so nothing may show one.
   */
  height: number;
  peers: number;
  /**
   * 0.5.2 RC52 — sync progress and catch-up ETA (syncProgress.ts): this node's height against the
   * profile RPC's public head, with the measured net catch-up rate. Folded by Store.refreshNode;
   * null until the first poll. Observed, never persisted (not in PERSIST_KEYS).
   */
  syncProgress: SyncProgress | null;
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
  /**
   * SCL-S0.6 — set while the node start is refused because another process holds the chain
   * database or a node port: the holder (pid, path) and the action, from `node_status.blocked`.
   * Observed state, never persisted.
   */
  nodeBlocked: string | null;
  /**
   * 0.5.7: set while the node keeps exiting on start with the same error (`node_status.crashLoop`,
   * the node's own stderr line). The supervisor keeps retrying meanwhile. Observed state, never
   * persisted.
   */
  nodeCrashLoop: NodeCrashLoopView | null;
  /** Why block production is held, from `node_status.miningHold`. Observed state, never persisted. */
  miningHold: string | null;
  /** CORE-N4: the node's own syncState, from `node_status.syncState` (citrate_nodeIdentity);
   *  null when the node reports none. Observed state, never persisted. */
  nodeSyncState: { state: string; reason: string | null; forkDepth: number | null; appliedHeight: number | null; highestSeen: number | null } | null;
  /** CORE-N4: head heights the node's peers announced, from `node_status.peerTips`; null when the
   *  node does not expose them. Observed state, never persisted. */
  peerTips: number[] | null;
  syncPct: number;
  logs: LogLine[];
  /**
   * K3: when the node_status answer behind `node` / `height` / `peers` was read, set by the poll
   * that committed them (ms since epoch); null = not read this session. Node › Status reads its
   * freshness from this. Observed, never persisted.
   */
  nodeStatusAt: number | null;
  liquid: number;
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
   * True once a `ValidatorRegistry.stakeOf` read (the grant-status read) has returned THIS
   * session. Until then `bondedStake` is a default or a persisted value, not a reading, so the
   * staked figure is shown as "—" (UI truth contract rule 2: unknown is not zero). Not persisted.
   */
  bondedStakeRead: boolean;
  /** RC52: the chain reads network-scoped values still wait for after a genesis switch. */
  netUnread: NetRead[];
  /** RC52: the genesis-switch notice, shown on Node until dismissed. */
  networkNotice: NetworkNotice | null;
  /** RC52: previous networks' archived values (read from the archive at load, not persisted). */
  netArchived: ArchivedNetwork[];
  /** RC52: the network this state's chain reads belong to (netScope.currentNetKey); every save is
   *  stamped with it. Not persisted as a field. */
  netKey: string;
  /**
   * C1-V V8 — the member's validator rewards, from `ValidatorRegistry.rewardsOf(pubkey)` only
   * (UI truth contract §1 Rewards): `totalWei` is the total, `claimableWei` is `claimableNow`, and
   * the difference is maturing. null = not a registered validator, or not read yet. There is no
   * per-day or per-source split: no contract exposes one, so none is shown.
   */
  rewards: { totalWei: string; claimableWei: string } | null;
  /** #391 — why `rewards` is null when a read was attempted (e.g. not the member's key); null = no error. */
  rewardsError: string | null;
  /**
   * C1-V (contract v1.1 §1) — this node's key in the registry: `isActive(pubkey)`,
   * `validatorInfo(pubkey)` status + actEpoch, and `currentEpoch()`. The ONLY input to the
   * validator words; stake amounts never are. null = not read
   * (no registered stake yet, or the read failed). Observed, never persisted.
   */
  validatorActivation: ValidatorActivation | null;
  /** C1-V (contract v1.1 §2) — registered / eligible / pending counts of the live registry; null = not read. */
  validatorCounts: ValidatorCounts | null;
  /**
   * `MemberBond.activated()` from the grant read (#348 review). After a full exit the registry stake
   * reads 0 but the bond can still claim the key's matured rewards, so this (with the stake) is
   * what decides whether `rewardsOf` is read. null = not read. Observed, never persisted.
   */
  bondActivated: boolean | null;
  /**
   * C1-V5 — `MemberBond.pubkey()` from the grant read: the member's validator key once the bond has
   * activated (kept after an exit). null = not read, not activated, or no bond. Observed, never persisted.
   */
  bondPubkey: string | null;
  /**
   * `MemberBond.isUnlocked()` from the grant read (owner decision 2026-10-19 / chain#331: activation
   * only inside the lock). null = not read. Observed, never persisted.
   */
  bondUnlocked: boolean | null;
  /**
   * Contract v1.2 §1 (#362 B-1) — `MemberBond.isKycVerified()` from the grant read: the gate the bond's
   * `claimRewards()` checks (reverts KycRequired otherwise). null = not read, or no bond. Observed,
   * never persisted. The identity provider's kyc_status claim is never a substitute for it.
   */
  bondKycVerified: boolean | null;
  /** C1-V V6 — seconds per block measured over the last 100 blocks (contract §2); null = unmeasured. */
  blockSecs: number | null;
  /** C1-V V7 — `ValidatorRegistry.minStake()` in wei (contract §2: read it, never hard-code 32,000). */
  minStakeWei: string | null;
  /**
   * C1-V — the member's validator key, the one the registry reads and the producer check use (C1-V5):
   * `MemberBond.pubkey()` once the bond has activated, else this node's `proposer.key`. Observed,
   * never persisted.
   */
  validatorPubkey: string | null;
  /** C1-V — distinct proposer keys sampled over the last 100 blocks (chainTruth); null = not sampled. */
  producers: { head: number; window: number; proposers: string[] } | null;
  /** Why the last producer sample failed, shown with "—". */
  producersError: string | null;
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
  storageMode: "lexical" | "dl" | "semantic";
  sel: string | null;
  panX: number;
  panY: number;
  pollIn: number;
  dl: Record<string, { st: string; pct: number }>;
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
  channel: "stable" | "beta";
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
  pins: Pin[];
  jPages: JournalPage[];
  jSel: string | null;
  jEditing: boolean;
  jMicOn: boolean;
  jInterim: string;
  jExportOpen: boolean;
  /** HUP-S10.4 (US-10.4 AC1) — write today's Hermes summary into the journal automatically once a
   *  day. Off by default; the member turns it on in the Journal. Persisted. */
  journalAutoSummary: boolean;
  /** The journal day (UTC `YYYY-MM-DD`) the automatic summary last ran for, or null. Persisted. */
  journalAutoSummaryDay: string | null;
  sponsorUnits: number;
  blocksProposed: number;
  graphQ: string;
  dataReady: boolean;
  /**
   * Join-8 (S7 · Activate validator). The hash of a sent MemberBond.activate. NOT persisted here:
   * the durable record is #347's fsync'd submitted-tx record in Rust, written before the send, and
   * the activation read returns it (`activation.recordedTx`) so a relaunch resumes to "Pending".
   * Cleared once the chain shows the bond activated, or by Forget / Try again (which also mark the
   * durable record forgotten).
   */
  s7TxHash: string | null;
  /** Join-8 — when `s7TxHash` was recorded (ms since epoch; from the durable record after a relaunch). */
  s7TxAt: number | null;
  /** Join-8 — the step's last chain read (NOT persisted: chain is the truth, re-read on launch). */
  activation: ActivationStatusView | null;
  /** Join-8 — why the last read failed (the last honest read is kept), or null. */
  activationError: string | null;
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
  /**
   * HUP-S3.1 runtime state of the first-run knowledge-corpus import (NOT persisted). `done`/`total`
   * are the importer's own per-tenant progress counts; `message` is the skip reason or the failure
   * text, shown as-is. Counts are never fabricated: they come only from the importer.
   */
  knowledgeImport: KnowledgeImportState;
}

export function freshCore(pid: string): CoreState {
  const P = personaFor(pid);
  const first = P.name.split(" ")[0].toLowerCase().replace(/[^a-z]/g, "");
  return {
    persona: pid,
    route: "dashboard",
    wTab: "overview",
    cTab: "apps",
    sSec: "account",
    toast: null,
    // No reading until the node is polled (contract rule 2): never a seeded height.
    height: 0,
    peers: 0,
    syncProgress: null,
    node: "off",
    // Default RUN: a fresh install should bring its node up on first launch.
    nodeIntent: "run",
    nodeBlocked: null,
    nodeCrashLoop: null,
    miningHold: null,
    nodeSyncState: null,
    peerTips: null,
    syncPct: 0,
    logs: [],
    nodeStatusAt: null,
    liquid: 0,
    bondedStake: 0,
    bondedStakeRead: false,
    netUnread: [],
    networkNotice: null,
    netArchived: [],
    netKey: currentNetKey(),
    rewards: null,
    rewardsError: null,
    validatorActivation: null,
    validatorCounts: null,
    bondActivated: null,
    bondPubkey: null,
    bondUnlocked: null,
    bondKycVerified: null,
    blockSecs: null,
    minStakeWei: null,
    validatorPubkey: null,
    producers: null,
    producersError: null,
    claimable: 0,
    earnSource: "sim",
    activity: [],
    justSigned: null,
    queue: [],
    cerPhase: "review",
    cerStep: 0,
    cerHash: "",
    walletReview: null,
    s7TxHash: null,
    s7TxAt: null,
    activation: null,
    activationError: null,
    storageMode: "lexical",
    sel: null,
    panX: 0,
    panY: 0,
    pollIn: 20,
    dl: {},
    rpc: "local",
    net: "testnet",
    cpuCap: 50,
    autolock: 30,
    // Sim default: the prototype presents an unlocked, provisioned vault so the
    // Keys-&-security section still renders. A Tauri build overwrites this from
    // the real custody_status on load.
    custodyLock: "unlocked",
    channel: "stable",
    updState: "idle",
    dataDir: "~/.citrate/core",
    coreMembershipUrl: "https://core-membership.vercel.app",
    walletAddr: makeAddr(P.name),
    custodyAddr: "",
    socketPath: "~/.citrate/core/memory/" + first + ".sock",
    deviceId: "dev_" + makeAddr(P.name + "::device").slice(2, 12),
    pins: [],
    jPages: [],
    jSel: null,
    jEditing: false,
    jMicOn: false,
    jInterim: "",
    jExportOpen: false,
    journalAutoSummary: false,
    journalAutoSummaryDay: null,
    sponsorUnits: 4,
    blocksProposed: 0,
    graphQ: "",
    dataReady: true,
    memGraphState: "idle",
    memDaemon: "idle",
    memDaemonError: null,
    memSocketPath: null,
    memSemantic: false,
    knowledgeImport: { state: "idle", tenant: null, done: 0, total: 0, nodesAdded: 0, message: null },
  };
}

const D = "device" as const;
const N = "network" as const;
export const CORE_PERSIST_SCOPE = {
  persona: D,
  liquid: N, bondedStake: N, claimable: N, activity: N,
  nodeIntent: D, rpc: D, net: D, cpuCap: D, autolock: D, channel: D,
  storageMode: D, dataDir: D, coreMembershipUrl: D,
  walletAddr: D, socketPath: D,
  wTab: D, cTab: D, sSec: D, route: D, deviceId: D,
  pins: D, jPages: D, jSel: D, journalAutoSummary: D, journalAutoSummaryDay: D,
  sponsorUnits: N, blocksProposed: N,
  // The reads still owed on this network after a switch, and the switch notice (kept until dismissed).
  netUnread: N, networkNotice: D,
} satisfies Partial<Record<keyof CoreState, PersistScope>>;
