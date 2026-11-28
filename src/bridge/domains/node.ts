// citrate-core: bridge domain `node`, moved verbatim from src/bridge/domains.ts by L0-WP05
// (planset v060). src/bridge/domains.ts re-exports it, so imports from "../bridge/domains" keep
// working. The owning lane is the row for this path in .agentile/v60-ownership.map.

import type { CeremonyView } from "../types";

/**
 * Q-A.2/Q-B.2 — one captured line from the supervised node's stdout/stderr. In
 * Tauri this is a REAL line streamed from the node process (e.g.
 * `citrate_network::sync: Validated and imported 32/32 blocks …`); `ts` is unix
 * ms and `stream` is "out"|"err". Mirrors the Rust `LogLine` (serde camelCase).
 */
export interface NodeLogLine {
  ts: number;
  stream: "out" | "err";
  line: string;
}

/**
 * SCL-S0.6 — why the last node start was refused: another process holds the chain database or a
 * node port. Mirrors the Rust `NodeBlocker` (serde camelCase). `message` names the holder (pid,
 * path) and the action; it is what the UI shows.
 */
export interface NodeBlocker {
  resource: "chainDatabase" | "port";
  port?: number;
  pid?: number;
  path?: string;
  message: string;
}

/**
 * 0.5.7: the node keeps exiting on start with the same error (`node_status.crashLoop`, from the
 * supervisor's crash records: the node's own stderr). `kind` is "tooManyOpenFiles" for EMFILE.
 */
export interface NodeCrashLoopView {
  kind: "tooManyOpenFiles" | "other";
  /** The node's own error line from the latest crash. */
  line: string;
  /** Crashes in a row with this error. */
  count: number;
  firstAtUnixMs: number;
  lastAtUnixMs: number;
}

/** The `node_status` shape. */
export interface NodeStatusView {
  state: string;
  peers: number;
  height: number;
  syncPct: number;
  /** Set after a start reset the node's chain data for a new genesis (40204 reroll). */
  notice?: string;
  /** Set while the last start was refused by a holder of the chain database or a node port. */
  blocked?: NodeBlocker;
  /** Set while the block producer is armed. */
  miningArmed?: boolean;
  /** Why block production is held (the mining gate in node.rs); unset while armed. */
  miningHold?: string;
  /** CORE-N4: the node's own citrate_nodeIdentity.syncState; unset when the node has none. */
  syncState?: { state: string; reason: string | null; forkDepth: number | null; appliedHeight: number | null; highestSeen: number | null };
  /** CORE-N4: head heights the node's peers announced (net_peerInfo); unset when not exposed. */
  peerTips?: number[];
  /** 0.5.7: set while the node keeps exiting on start with the same error; unset otherwise. */
  crashLoop?: NodeCrashLoopView;
}

/** A settled exit's outcome as read from the ValidatorRegistry's logs (wei as decimal strings). */
export interface ExitOutcomeView {
  /**
   * "returned": the exit escrowed at least the principal, a Withdrawn paid at least that, and no
   * Slashed was logged under the key before it. "slashed": a Slashed was logged before the payout
   * (or there is none), or the exit escrowed less than the principal. "notRead": neither shown.
   */
  outcome: "returned" | "slashed" | "notRead";
  exitBlock: number | null;
  escrowedWei: string | null;
  paidOutWei: string | null;
  paidOutBlock: number | null;
  /** Sum of the Slashed penalties logged up to the payout; null when none was seen. */
  slashedWei: string | null;
  scannedFrom: number;
  scannedTo: number | null;
  reason: string | null;
}

/**
 * Join-8 — everything the "Activate validator" onboarding step reads, in one call. Each field
 * names its source (UI truth contract v1 §1/§2); a failed read is `null`, never 0 or false.
 * Wei as decimal strings. Mirrors the Rust `validator_activation::ActivationStatus`.
 */
export interface ActivationStatusView {
  /** `MembershipStakeVault.bondOf(member)` (CREATE2: present even before the bond exists). */
  bondAddress: string | null;
  /** Code at `bondAddress`. */
  bondDeployed: boolean | null;
  /** `MemberBond.activated()`. */
  bondActivated: boolean | null;
  /** `MemberBond.principal()`. */
  principalWei: string | null;
  /** `MemberBond.unlockBlock()` / `unlockTimestamp()` (unix seconds). */
  unlockBlock: number | null;
  unlockTimestamp: number | null;
  /** `MemberBond.isUnlocked()` as read (chain#331: activation reverts once true); null = unread. */
  bondUnlocked: boolean | null;
  /** `MemberBond.memberTokenId()`. */
  memberTokenId: string | null;
  /** `CitrateMemberSBT.isActive(memberTokenId)`. */
  membershipActive: boolean | null;
  /** The key the registry reads use: the bond's registered key once activated, else this node's. */
  validatorPubkey: string | null;
  pubkeySource: "bond" | "node" | null;
  /** This node has a proposer key on disk. */
  nodeKeyReady: boolean;
  /** This node's proposer key, when it has one. */
  nodePubkey: string | null;
  /** `MemberBond.pubkey()`: the key the bond registered with (null before activation). */
  bondPubkey: string | null;
  /** `ValidatorRegistry.validatorInfo(pubkey).status`: 0 None, 1 Active, 2 Exiting, 3 Slashed. */
  registryStatus: 0 | 1 | 2 | 3 | null;
  /** `validatorInfo(pubkey).bonded`: stake still bonded under the key (wei). */
  registryBondedWei: string | null;
  /**
   * `validatorInfo(pubkey).escrow`: unbonded stake the registry holds until the exit lock ends
   * (wei). The key stays Exiting after the payout, so bonded and escrow both "0" is what says
   * the exit paid out.
   */
  registryEscrowWei: string | null;
  /**
   * How a settled exit ended (Exiting, bonded and escrow both "0"), from the registry's logs under
   * the key: `UnbondInitiated`/`Evicted` (the exit), `Slashed`, `Withdrawn` (the payout). A
   * non-Byzantine slash keeps the key Exiting, so bonded and escrow at 0 alone do not say the
   * bond came back. Null when not a settled exit, or not read.
   */
  exitOutcome: ExitOutcomeView | null;
  /** `validatorInfo(pubkey).actEpoch`. */
  activationEpoch: number | null;
  /** `ValidatorRegistry.isActive(pubkey)`. */
  registryActive: boolean | null;
  /** `ValidatorRegistry.EPOCH()`. */
  epochBlocks: number | null;
  /**
   * `ValidatorRegistry.currentEpoch()`. `isActive` is already true at registration (chain#319), so
   * "Validator active" also needs `activationEpoch <= currentEpoch`.
   */
  currentEpoch: number | null;
  /** Public `eth_blockNumber` (`0x0` is read as unknown). */
  headBlock: number | null;
  /** Measured: timestamp delta over the last 100 public blocks, in ms per block. */
  blockTimeMs: number | null;
  /** This node's `eth_blockNumber` / `eth_syncing` (true while it returns an object). */
  localBlock: number | null;
  localSyncing: boolean | null;
  /**
   * `eth_syncing.highestBlock` while it returns an object: the highest height this node has
   * EVIDENCE for, which includes unverified gossip. A hint, never the network head (null = false
   * or not read).
   */
  localHighest: number | null;
  /** This node's `net_peerCount` (null = not read, never 0). */
  localPeers: number | null;
  /** `eth_getTransactionReceipt` for a sent activation (null when none was sent). */
  tx: { hash: string; included: boolean | null; blockNumber: number | null; success: boolean | null } | null;
  /**
   * The member's latest activation send to this bond as the durable submitted-tx record names it
   * (written and fsync'd in Rust BEFORE the send; #347). Null when none is recorded, or it was
   * forgotten. This, not localStorage, is how the step resumes to Pending after a restart.
   */
  recordedTx: { hash: string; atMs: number | null } | null;
}

export interface NodeDomain {
  status(): Promise<NodeStatusView>;
  start(): Promise<void>;
  stop(): Promise<void>;
  /**
   * W1.3 — register the member's node as a block-producing validator. Builds the
   * `registerValidator{value:32k}(pubkey, sig)` tx as a PENDING ceremony and
   * returns the decoded view for human approval (staker = the member EOA). Tauri →
   * `node_register_validator`; the sim shim is honestly Unavailable (no node/key).
   */
  registerValidator(): Promise<CeremonyView>;
  /**
   * Join-8 — the Activate-validator step's one read (pure; signs nothing). `memberAddress` is the
   * member wallet the bond was granted to; `txHash` a sent activation to look up. Tauri →
   * `validator_activation_status`; the sim shim is honestly Unavailable (no chain, no node).
   */
  activationStatus(memberAddress: string, txHash?: string | null): Promise<ActivationStatusView>;
  /**
   * Join-8 — Forget a recorded activation send (S7's Forget after 10 minutes unseen, or Try again
   * after a revert): marks it forgotten in the durable record so a restart does not bring it back.
   * Resolves true when the record held it; rejects when the write fails. Tauri →
   * `validator_activation_forget`; the sim shim is honestly Unavailable.
   */
  activationForget(txHash: string): Promise<boolean>;
  /**
   * The most recent supervised-node crash, or null if it has never crashed.
   * Used by the liveness watchdog to name the REAL reason a node stopped.
   */
  lastCrash(): Promise<{ reason: string; atUnixMs: number } | null>;
  /**
   * W1.4 — the REAL validator rewards: `rewardsOf(proposerPubkey)` on the
   * ValidatorRegistry, keyed on the node's own proposer key. A not-yet-registered
   * validator honestly returns zeros. This is the CORRECT source for validator
   * earnings; `ContributionAccounting.claimable` is a different accrual.
   */
  validatorEarnings(pubkey?: string): Promise<{ totalWei: string; claimableWei: string; proposerPubkey: string }>;
  /**
   * W1.x — arm the producer, consensus-gated on being synced. The node runs as a
   * plain follower and never mints its `proposer.key`; arming respawns it with
   * `--mine --coinbase`, which mints the key the bond activation needs. Returns
   * whether it armed on this call (idempotent — already-armed / not-synced → false).
   * Tauri → `node_arm_mining`; the sim shim is an honest no-op (returns false).
   */
  armMining(): Promise<boolean>;
  /**
   * Start the bundled IPFS (kubo) daemon that the node uses for artifact/model
   * pin/add on 127.0.0.1:5001. Idempotent; started alongside the node. Tauri →
   * `ipfs_start`; the sim shim is a no-op (no daemon in the web preview).
   */
  startIpfs(): Promise<void>;
  /**
   * Q-A.2/Q-B.2 — the REAL recent node log lines (streamed stdout+stderr from the
   * supervisor's bounded ring). In Tauri these are live node output; a stopped
   * node honestly returns [] (never a fabricated template — Rule 1). In sim/web
   * this returns the labelled preview template (clearly a design preview, guarded
   * out of packaged builds), so the web preview reads as a preview, not live.
   */
  logs(): Promise<NodeLogLine[]>;
  /**
   * CORE-N3 follow-up: the previous networks' chain data a genesis switch set aside in the node data
   * dir (`chain.<profile>.bak`), with sizes. Tauri → `node_old_network_data`; sim has none.
   */
  oldNetworkData(): Promise<OldNetworkDataView[]>;
  /**
   * Remove one of them, at the size the member confirmed. Rust refuses anything that is not a chain
   * backup in the node data dir (chain_backup.rs). Resolves to the bytes freed.
   */
  removeOldNetworkData(name: string, confirmedBytes: number): Promise<number>;
}

/** One set-aside chain database (Rust `chain_backup::ChainBackup`). */
export interface OldNetworkDataView {
  name: string;
  bytes: number;
  files: number;
}
