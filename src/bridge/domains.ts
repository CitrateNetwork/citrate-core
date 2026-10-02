// =====================================================================
// citrate-core — bridge domain contracts (CORE-A1 · A1.1)
//
// One typed interface per domain: auth, wallet, node, memory, chat,
// membership, commissary, comms, config. Each declares the async functions
// its surface needs — the shapes the current `store.*` methods already imply.
//
// A1 wires exactly ONE domain for real (`config`, the round-trip proof). The
// other eight are typed contracts whose sim impl delegates to the prototype
// Store and whose Tauri impl returns `Unavailable` (Rule 1) until a later
// phase flips it. The surfaces above these interfaces never change.
// =====================================================================
import type { DeployGateInputs, DeployGateLookup, DeployGateRecord } from "../agent/deployGate";
import type { VerifiedSourceView } from "../agent/verifiedSource";
import type { CheckpointList, UndoOutcome } from "../agent/fileChanges";
import type { LearnAcceptResult, LearnContent, LearnedMemory, LearnProposal, LearnStatus, WorkflowRunView, WorkflowSpec } from "../agent/learn";
import type {
  AppConfig,
  KeyringStatus,
  CustodyStatus,
  SlotInfo,
  AuthStatus,
  SignatureIntent,
  CeremonyView,
  Signature,
  BroadcastResult,
} from "./types";

/** C2-F-1 — the outcome of pressing Claim. Either a REAL pending ceremony the
 * human must approve via `signing.broadcast` (B1.4), or an honest "nothing to
 * claim" when the on-chain claimable is 0. NEVER a faked settlement. */
export type ClaimResult =
  | { kind: "ceremony"; view: CeremonyView }
  | { kind: "nothing"; claimableWei: string };

// ---- config (A1.4 — the genuinely-live domain) ----------------------
export interface ConfigDomain {
  /** Read the full persisted app config from the real on-disk store. */
  read(): Promise<AppConfig>;
  /** Persist a partial patch; returns the merged, now-persisted config. */
  write(patch: Partial<AppConfig>): Promise<AppConfig>;
  /** OS keyring availability, read from the platform (not fabricated). */
  keyringStatus(): Promise<KeyringStatus>;
}

// ---- custody (A2 — the OS-keyring vault) ----------------------------
// Status + slot METADATA only. This domain NEVER returns secret bytes to the
// frontend (ADV-8); reading a secret is an in-process Rust API, not an invoke.
export interface CustodyDomain {
  /** Vault status: initialized / unlocked / auto-lock / keyring (real in Tauri). */
  status(): Promise<CustodyStatus>;
  /** Initialize a fresh vault from a passphrase (mints the keyring master key). */
  init(passphrase: string): Promise<void>;
  /** Unlock the session with the passphrase. */
  unlock(passphrase: string): Promise<void>;
  /**
   * Seamless device-bound RE-UNLOCK (passphrase-less model). Re-provisions +
   * unlocks the vault from the OS-keyring device passphrase — no user passphrase.
   * Idempotent; safe to call on launch/resume and from the Settings "Unlock"
   * control. Returns the fresh status. This is what keeps an auto-locked vault (and
   * the wallet reads gated on it) from getting permanently stuck.
   */
  ensureUnlocked(): Promise<CustodyStatus>;
  /** Lock the session (drops + zeroizes the in-memory data key). */
  lock(): Promise<void>;
  /** Slot metadata only — never secret bytes. */
  listSlots(): Promise<SlotInfo[]>;
}

// ---- the eight seam domains -----------------------------------------
// Their sim impls delegate to the prototype Store (1:1 UI preserved); their
// Tauri impls throw `Unavailable` until their own later phase.

// ---- auth (A3 — real OIDC loopback-PKCE) ----------------------------
// Every method returns the claim-derived AuthStatus (or void) — NEVER a token
// (ADV-8). In Tauri these invoke the real Rust OIDC commands; in sim they drive
// the prototype persona flow (guarded out of packaged builds).
export interface AuthDomain {
  /** Current claim-derived status (signedIn + entitlement flags). */
  status(): Promise<AuthStatus>;
  /** Run the loopback-PKCE sign-in; opens the system browser. Returns status. */
  login(): Promise<AuthStatus>;
  /** Live /userinfo entitlement re-check (the federation RP rule). */
  userinfo(): Promise<AuthStatus>;
  /** Silent refresh from the vaulted refresh token (survives restart). */
  refresh(): Promise<AuthStatus>;
  /** Revoke the refresh token + clear the vault slot + wipe the session. */
  logout(): Promise<void>;
  /** Open the KYC flow in the browser (S2). Status is then read via userinfo. */
  kycStart(): Promise<void>;
}

// ---- signing (B1.2 — the ONE human-in-the-loop signing path) --------
// The gated Rust signer is reachable ONLY through this domain's approve. A
// caller (user / agent / micro-app) SUBMITS an intent; a human APPROVES a
// specific id; only then is a signature produced. No method returns key/seed/
// entropy material (I-2) — request returns a decoded view, approve a signature.
export interface SigningDomain {
  /** Submit an intent → a PENDING ceremony (id + decoded action). Signs NOTHING. */
  request(intent: SignatureIntent): Promise<CeremonyView>;
  /** Approve a SPECIFIC pending id (no "approve latest"/auto-approve). `rawAck`
   * MUST be true to approve undecodable calldata. Returns the signature hex. */
  approve(id: string, rawAck: boolean): Promise<Signature>;
  /** CORE-B1.4 — approve a SPECIFIC pending TRANSACTION id, sign the real EIP-155
   * tx with the vault key, broadcast it to the live 40204 RPC, and return the
   * real tx hash + block (never key material). Same explicit-id / raw-ack /
   * single-use / fail-closed invariants as `approve`. */
  broadcast(id: string, rawAck: boolean): Promise<BroadcastResult>;
  /** Reject a pending ceremony — consumes it, produces no signature. */
  reject(id: string): Promise<void>;
}

export interface WalletDomain {
  balances(): Promise<{ liquid: number; staked: number; claimable: number; address: string }>;
  /** Q-E.2 (C-5) — the indexed tx history. `status` (1 ok / 0 reverted / null
   * pending) + `direction` are passed through from the indexer so a failed tx can
   * be marked (they used to be stripped, making a revert look like a success). */
  activity(): Promise<{ id: string; kind: string; amount: string; hash: string; ts: number; status?: number | null; direction?: string }[]>;
  /** CORE (@rule8) — submit a native SALT transfer as a PENDING ceremony and
   * return the decoded view. Signs NOTHING; the human approves via
   * `signing.broadcast(view.id)` (B1.4 → a real 40204 tx). `amountWei` is a
   * decimal wei string. Mirrors `agent.claim` for the claimRewards() path. */
  send(to: string, amountWei: string): Promise<CeremonyView>;
  /** CORE (@rule8) — submit a LiquidStakingPool `deposit()` stake as a PENDING
   * ceremony and return the decoded view. Signs NOTHING; the human approves via
   * `signing.broadcast(view.id)` (B1.4 → a real 40204 deposit tx). `amountWei` is
   * a decimal wei string. Mirrors `send` for the transfer path. */
  stake(amountWei: string): Promise<CeremonyView>;
  /** CORE WP2 (@rule8) — submit a LiquidStakingPool `requestWithdrawal(shares)`
   * as a PENDING ceremony (burns stSALT shares into the ~7-day queue). The
   * SALT→shares conversion is done in Rust from LIVE reads (shares()+balanceOf()),
   * so `amountWei` is the SALT the user wants to withdraw. Signs NOTHING; approved
   * via `signing.broadcast`. Mirrors `stake`. */
  requestWithdrawal(amountWei: string): Promise<CeremonyView>;
  /** CORE WP2 (@rule8) — submit a LiquidStakingPool `claimWithdrawal(id)` for a
   * matured request as a PENDING ceremony. The ~7-day (50,400-block) delay is
   * enforced on-chain; a too-early claim reverts. Signs NOTHING; approved via
   * `signing.broadcast`. `id` is the decimal request id from `pendingWithdrawals`. */
  claimWithdrawal(id: string): Promise<CeremonyView>;
  /** CORE WP2 — the wallet's PENDING (unclaimed) withdrawals, read from live
   * chain state (getLogs WithdrawalRequested + withdrawals(id) + block_number).
   * A fresh wallet honestly returns []. Never fabricated (Rule 1). */
  pendingWithdrawals(): Promise<PendingWithdrawal[]>;
  /**
   * Seamlessly device-provision the custody vault and ensure the wallet exists,
   * returning its PUBLIC address (never key/seed). Idempotent — safe to call on
   * every launch; an already-provisioned device short-circuits to a cheap read.
   *
   * WHY IT MATTERS: this is the missing runtime step behind the "Wallet link
   * unavailable" dead-end. On a fresh install the custody vault was uninitialized
   * + locked and no wallet had been minted, so `linkRequest`/`balances`/`address`
   * all failed closed. `ensureReady` init+unlocks the vault under a device-bound
   * keyring passphrase (no user passphrase — the owner-approved model) and mints
   * the wallet silently, so there is a REAL EOA to link and fund. Call it after
   * sign-in and before the membership grant.
   *
   * Tauri → `wallet_ensure_ready` (real custody + wallet). The sim shim returns a
   * deterministic, clearly-labelled NON-CHAIN persona address (no vault/key in the
   * web preview).
   */
  ensureReady(): Promise<{ address: string; created: boolean }>;
  /**
   * A REAL device fingerprint: sha256 over the device-bound custody PUBLIC key.
   * Stable per install, non-secret, and NOT attestation (see provisioning.rs).
   */
  deviceId(): Promise<string>;
  /**
   * Bind THIS device's custody EOA to the member's Citrate identity, as a PENDING
   * ceremony. Signs NOTHING — the human approves via `wallet.linkApprove`.
   *
   * WHY IT MATTERS: until a wallet is linked, the authority's `wallet_address`
   * claim is the counterfactual smart-wallet address, which no private key can
   * spend from. The membership money path pays THAT address, while the validator
   * self-bond is sent from this custody EOA — so an unlinked member is funded
   * somewhere they cannot reach. Linking makes the two the same address.
   *
   * The proof is an EIP-191 signature over a one-time challenge from the
   * authority; the message is shown verbatim in the approval UI.
   */
  linkRequest(): Promise<CeremonyView>;
  /**
   * Approve a pending link ceremony: signs the challenge and submits the proof to
   * the authority. Returns the now-bound address. NEVER returns the signature —
   * it is consumed in-process (I-2). This is NOT `signing.broadcast`: no
   * transaction is sent and no funds move.
   */
  linkApprove(id: string, rawAck: boolean): Promise<{ address: string; linked: boolean; canonical: boolean }>;
  /** Decline a pending link: release the ceremony + drop the one-time nonce. */
  linkReject(id: string): Promise<void>;
}

/** CORE WP2 — a single pending (unclaimed) LiquidStakingPool withdrawal, all
 * fields from live on-chain state. `saltWei` is the payout; `claimableAtBlock` =
 * `requestBlock + 50400`; `claimable` is `currentBlock >= claimableAtBlock`. */
export interface PendingWithdrawal {
  id: string;
  saltWei: string;
  requestBlock: number;
  claimableAtBlock: number;
  claimable: boolean;
}

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

export interface NodeDomain {
  status(): Promise<{ state: string; peers: number; height: number; syncPct: number }>;
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
  validatorEarnings(): Promise<{ totalWei: string; claimableWei: string; proposerPubkey: string }>;
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
}

/**
 * CORE-BC-3 — the local Gemma model status. `state` is the honest, file-derived
 * lifecycle; `Ready` is EARNED only by a real SHA-256 verify (never mere presence,
 * Rule 1). The download/verify facts (bytes/pct) trace to real bytes on disk — no
 * fabricated progress. Mirrors the Rust `ModelStatus` (serde camelCase).
 */
export type ModelStatus =
  | { state: "notPresent" }
  | { state: "downloading"; downloadedBytes: number; totalBytes: number; pct: number }
  | { state: "verifying" }
  | { state: "ready" }
  | { state: "error"; msg: string };

/**
 * CORE-BC-3 (BC-3.1/3.2) — the local model domain. `status/download/verify` drive
 * the BC-3.1 download+verify; `serveStart` spawns the BC-3.2 llama-server sidecar
 * (fails closed unless the model is verified-Ready + the binary is bundled). In
 * Tauri these invoke the real Rust commands; the sim impl is honest in web-dev
 * (guarded by assertSimAllowed — a fake progress animation is web-dev ONLY, never
 * presented as real in the packaged app).
 */
export interface ModelDomain {
  /** The honest, file-derived status (Ready only after a real verify). */
  status(): Promise<ModelStatus>;
  /** STREAMED, resumable download of the pinned Gemma GGUF. Resolves on complete. */
  download(): Promise<void>;
  /** Stream the file through SHA-256; on a match the model becomes Ready. */
  verify(): Promise<void>;
  /** Spawn the llama-server sidecar on the verified-Ready model (fails closed). */
  serveStart(): Promise<void>;
}

// The node-agent under the SidecarSupervisor (CORE-C1.2). `status` returns the
// supervisor state + whether a bearer session exists — NEVER the bearer token.
// `start` spawns the daemon with a per-session OsRng bearer (handed via a 0600
// file); `stop` releases the supervisor and wipes the session bearer. The
// node-agent's unsigned signature requests route through the SignatureCeremony
// (origin "agent:node-agent") — it holds no keys and never signs directly.
export interface AgentDomain {
  status(): Promise<{ state: string; authed: boolean }>;
  start(): Promise<void>;
  stop(): Promise<void>;
  /**
   * CORE-C2 — the REAL on-chain claimable. Reads
   * `ContributionAccounting.claimable(vaultAddress)` via `eth_call` on 40204
   * (data source: eth_call, Rule 11) and returns the single real claimable in
   * wei of SALT + its data source. There is NO per-source breakdown field: the
   * contract exposes none, so the Earning tab's sim validation/pinning/compute
   * split is NOT fabricated for live reads (Rule 1 / I-3).
   */
  earnings(): Promise<{ claimableWei: string; walletAddress: string; contract: string }>;
  /**
   * CORE-C2-F-1 (@rule8) — the USER Claim button. Reads the REAL claimable
   * (`ContributionAccounting.claimable(addr)` eth_call), then EITHER bridges the
   * real `claimRewards()` intent into a PENDING ceremony the human approves via
   * `signing.broadcast` (B1.4 → a real 40204 tx), OR returns an honest "nothing to
   * claim" when the claimable is 0. It NEVER mutates a local balance or fabricates
   * a settled-claim hash (Rule 1). The Tauri impl invokes the `user_claim` command;
   * the sim impl is guarded out of packaged builds and never claims for real.
   */
  claim(): Promise<ClaimResult>;
}

// The citrate-memories mcp_serve daemon under the SidecarSupervisor (CORE-C3).
// recall/search/neighbors speak the daemon's JSON-RPC over its Unix socket and
// return REAL parsed nodes/edges from the per-user encrypted store — never a
// fabricated graph (Rule 1). `status` carries the supervisor state + socket path
// (a local path, not a secret); `constellation` recalls the personal +
// chain-state tenants for the Storage graph. `assert` (a signed WRITE) still
// routes through the SignatureCeremony (a later WP), so it stays a seam stub.
export interface MemoryHit {
  id: string;
  kind: string;
  title: string;
  status?: string;
}
export interface MemoryResult {
  tenant: string;
  totalInTenant: number;
  hits: MemoryHit[];
}
export interface MemoryNeighbor {
  direction: "out" | "in";
  kind: string;
  title: string;
  proposed: boolean;
}
export interface MemoryStatus {
  state: string;
  socketPath: string;
  semantic: boolean;
}
/** W3.2 — result of the first-run docs preload (or why it was skipped). */
export interface DocsIngestReport {
  docs: number;
  chunks: number;
  /** "not-semantic" | "not-running" | "already-seeded" | "empty-corpus" when nothing ran. */
  skipped?: string;
}

/** Live values the seed composes real facts from (structured, never free-form text). */
export interface SeedFacts {
  chainId: number;
  nodeState: string;
  height: number;
  peers: number;
  walletAddr: string;
  hasGrant: boolean;
  grantStakedSalt: number;
  bondStatus: string;
  hasSbt: boolean;
}
export interface SeedReport {
  authored: number;
  /** "not-semantic" | "not-running" | "already-seeded" when nothing was authored. */
  skipped?: string;
}
/** HUP-S3.1 — one progress line from the first-run knowledge-corpus import (mirrors the Rust
 *  `ImportLine`; the importer's JSON-lines contract lives in citrate-memories `mem_corpus::progress`). */
export type KnowledgeImportLine =
  | { event: "verified"; bundle_digest: string; tenants: number; nodes: number; edges: number }
  | { event: "tenant_start"; tenant: string; nodes: number; edges: number }
  | { event: "progress"; tenant: string; done: number; total: number }
  | { event: "tenant_skipped"; tenant: string; reason: string }
  | { event: "tenant_done"; tenant: string }
  | {
      event: "done";
      bundle_digest: string;
      embed_model: string;
      nodes_added: number;
      nodes_merged: number;
      edges_added: number;
      tenants_imported: string[];
      tenants_skipped: string[];
    }
  | { event: "error"; stage: string; message: string };

/** HUP-S3.1 — what the first-run knowledge import did. Counts come only from the importer. */
export interface KnowledgeImportReport {
  state: "imported" | "skipped" | "failed";
  /** "no-bundle" | "not-semantic" | "already-imported" | "in-progress" when nothing ran. */
  skipped?: string | null;
  error?: string | null;
  bundleDigest?: string | null;
  embedModel?: string | null;
  nodesAdded: number;
  edgesAdded: number;
  tenantsImported: string[];
  tenantsSkipped: string[];
}

export interface MemoryDomain {
  status(): Promise<MemoryStatus>;
  start(): Promise<void>;
  stop(): Promise<void>;
  assert(fact: string): Promise<"approved" | "declined">;
  recall(tenant: string, budget?: number): Promise<MemoryResult>;
  search(tenant: string, query: string, budget?: number): Promise<MemoryResult>;
  neighbors(tenant: string, idPrefix: string, budget?: number): Promise<MemoryNeighbor[]>;
  /** Recall the personal + chain-state tenants for the Storage constellation. */
  constellation(budget?: number): Promise<MemoryResult[]>;
  /** W3.2 — idempotent first-run preload of the bundled Citrate docs into the
   *  citrate-docs tenant. Gated in Rust (semantic + running + empty tenant). */
  ingestDocs(): Promise<DocsIngestReport>;
  /** Seed the constellation tenants with real network/node/stake facts on daemon-connect. */
  seedContext(facts: SeedFacts): Promise<SeedReport>;
  /** HUP-S3.1 — import the bundled knowledge corpus into the local store (first run; idempotent,
   *  verified against its manifest in Rust). Must run while the daemon is stopped; Rust stops and
   *  restarts a running daemon itself. `onProgress` receives each importer line. */
  importKnowledge(onProgress?: (line: KnowledgeImportLine) => void): Promise<KnowledgeImportReport>;
}

/** CORE-AI1 (@rule8) — non-secret status of a configured AI provider. Carries the
 * id, https baseURL, model, and a `configured` flag — NEVER the API key or the
 * Authorization header (invariant 1). Mirrors the Rust `ProviderStatus`. */
export interface AiProviderStatus {
  id: string;
  baseURL: string;
  model: string;
  configured: boolean;
  isDefault: boolean;
}

export interface ChatDomain {
  /** Which provider transport the harness routes to (real/demo). */
  backend(): Promise<{ kind: string; label: string }>;
  /**
   * CORE-AI1 (@rule8) — seal a provider's `{baseURL, model, apiKey}` in the OS
   * keyring, binding the key to its https baseURL. Returns nothing — the key is
   * NEVER returned (invariant 1). The Tauri impl invokes `ai_set_provider`; the
   * sim impl is honestly Unavailable (the web preview has no OS keyring).
   */
  setProvider(providerId: string, baseURL: string, model: string, apiKey: string): Promise<void>;
  /**
   * CORE-AI1 (@rule8) — non-secret status for the preset provider ids: `{id,
   * baseURL, model, configured, isDefault}`, NEVER the key. Drives the Settings
   * rows + the store's real-vs-demo provider selection.
   */
  providerStatus(): Promise<AiProviderStatus[]>;
  /** CORE-AI1 — delete a provider's sealed config (and clear the default if it
   * pointed here). */
  clearProvider(providerId: string): Promise<void>;
  /**
   * CORE-AI1 (@rule8) — REAL inference. The webview picks WHICH configured
   * provider id; Rust reads the STORED baseURL for that id and POSTs the OpenAI
   * `/v1/chat/completions` body with the sealed Bearer, returning the model
   * completion. The webview can NEVER supply the URL (exfil-binding, invariant 3).
   * The Tauri impl invokes `ai_chat`; the sim impl is honestly Unavailable.
   */
  infer(providerId: string, messagesJson: string, contextJson: string): Promise<string>;
  /**
   * W3.3 — one AGENTIC turn: like `infer`, but the request carries a `tools` spec
   * and the result is the assistant MESSAGE JSON (content and/or `tool_calls`) so
   * the frontend runs the tool loop. Rust reads the STORED baseURL + sealed key
   * (invariants 1 + 3). The Tauri impl invokes `ai_chat_tools`; the sim impl is
   * honestly Unavailable.
   */
  inferTools(
    providerId: string,
    messagesJson: string,
    toolsJson: string,
    contextJson: string,
  ): Promise<string>;
  /**
   * BC-3.2 — REAL LOCAL inference against the bundled `llama-server` (llama.cpp)
   * on the loopback endpoint, with NO api key. The endpoint is derived IN RUST
   * from the serve manager's loopback port — the webview supplies ONLY the
   * messages + context, NEVER a URL or host (exfil-binding). The Tauri impl
   * invokes `ai_chat_local`; the sim impl is honestly Unavailable (the web
   * preview has no llama-server). Local routing only happens when
   * `inferenceState()` reports the server is healthy — otherwise the store falls
   * through to gateway/demo (never a fabricated local reply, Rule 1).
   */
  inferLocal(messagesJson: string, contextJson: string): Promise<string>;
  /**
   * The AGENTIC local path: like `inferLocal` but attaches the `tools` spec and returns the
   * assistant MESSAGE JSON (content and/or tool_calls) so the Hermes tool loop runs on the bundled
   * local model, not just the gateway. Tauri invokes `ai_chat_local_tools`; sim is Unavailable.
   */
  inferLocalTools(messagesJson: string, toolsJson: string, contextJson: string): Promise<string>;
  /**
   * BC-3.2 — the HONEST inference-routing state (kebab): `ready` (local model +
   * healthy server → chat runs LOCALLY), `local-fallback`/`gateway-only` (route
   * to the gateway), `downloading`, `no-model`, or `demo`. Computed IN RUST from
   * the real model status + serve health + `gatewayConfigured` (a cgk_ key is
   * sealed). Drives the store's local-vs-gateway-vs-demo selection so the route
   * is honest — the frontend never claims a local model that isn't serving. The
   * Tauri impl invokes `model_inference_state`; the sim impl returns `demo`.
   */
  inferenceState(gatewayConfigured: boolean): Promise<string>;
}

/**
 * BC-1.3 — the REAL on-chain membership grant status the S5 (grant + stake)
 * ceremony settles from. Every field is a live 40204 read (Rule 1): the vault's
 * `attributedStake`/`attributedShares(member)` (wei as decimal strings, since a
 * 32,000-SALT grant exceeds JS number range) and `CitrateMemberSBT.balanceOf`
 * (`hasSbt`). A fresh/never-granted member reads 0 / 0 / false.
 */
export interface GrantStatus {
  attributedStakeWei: string;
  /**
   * `MembershipStakeVault.attributedPrincipal(member)`. M-2 replaced
   * `attributedShares` (an stSALT share count) with raw bonded principal —
   * there are no shares under bonding.
   */
  attributedPrincipalWei: string;
  hasSbt: boolean;
  /**
   * `MembershipStakeVault.bondOf(member)` — the member's MemberBond escrow.
   * ALWAYS present (CREATE2-deterministic), so it resolves even for a member who
   * has never been granted. `bondDeployed` is what says it is real.
   */
  bondAddress: string;
  bondDeployed: boolean;
  /**
   * `ValidatorRegistry.stakeOf(pubkeyOfStaker(bond))` — the principal actually
   * bonded in the registry. Zero until the member runs the activation ceremony,
   * so this is NOT the settle signal; attribution is.
   */
  bondedStakeWei: string;
  /**
   * `pubkeyOfStaker(bondOf(member)) != 0` — the member has ACTIVATED a validator.
   * NOTE the staker is the member's bond clone, not the member.
   */
  hasValidator: boolean;
  /** `MemberBond.unlockBlock()` — height leg of the 1-year lock; null pre-bond. */
  unlockBlock: number | null;
  /** `MemberBond.isUnlocked()` — either leg elapsed. Eligibility, not a payout. */
  isUnlocked: boolean;
  /** `MemberBond.isKycVerified()` — gates money OUT only, never participation. */
  isKycVerified: boolean;
}


/** The qualifying fields the Enterprise contact form collects. `org` + `email` are required; the rest
 *  help sales prep the call. No card/money fields — this is a lead, not a purchase. */
export interface EnterpriseLeadInput {
  org: string;
  email: string;
  contact?: string;
  seats?: string;
  workload?: string;
  timeline?: string;
  notes?: string;
}

export interface MembershipDomain {
  entitlement(): Promise<{ status: "active" | "expiring" | "grace" | "lapsed"; tier: string; expiresAt: string }>;
  /**
   * BC-1.3 (@rule8) — read the member's REAL on-chain grant status from 40204
   * (`MembershipStakeVault.attributedStake`/`attributedShares` + `CitrateMemberSBT
   * .balanceOf`). `memberAddress` is the AA smart-wallet the grant targets (the
   * OIDC `wallet_address` claim), NOT the custody EOA. A PURE READ — it signs
   * nothing. S5 settles the grant leg ONLY from this real read; a never-granted
   * member honestly reads 0 stake / no SBT (no fabricated settlement, Rule 1). The
   * Tauri impl invokes `membership_grant_status`; the sim impl derives from the
   * persona/AppState (granted only for a paid sim member — never a real chain read).
   */
  grantStatus(memberAddress: string): Promise<GrantStatus>;
  /**
   * CORE-D3.C — open the REAL core-membership checkout in an in-app popup
   * (`{coreMembershipUrl}/checkout`). Resolves once the popup is OPENED — it does
   * NOT report payment success. The money + entitlement grant happen server-side
   * (core-membership → droplet); the caller then polls `auth.userinfo()` until the
   * entitlement goes active (the same "open a flow then poll userinfo" pattern as
   * `kycStart`/`pollKyc`). The Tauri impl invokes `membership_checkout`; the sim
   * impl is guarded out of packaged builds (web-dev keeps its fake settle).
   */
  checkout(loginHint?: string): Promise<void>;
  /**
   * Enterprise · Contact us — POST a QUALIFIED sales lead to core-membership's `/api/enterprise/lead`
   * (NOT the money path: never grants, charges, or signs). Contact PII is validated + field-encrypted
   * server-side. Resolves on success; REJECTS with an honest message on a validation/availability
   * failure (Rule 1 — never a fabricated "received"). The Tauri impl invokes `membership_enterprise_lead`.
   */
  enterpriseLead(lead: EnterpriseLeadInput): Promise<void>;
  /**
   * BC-5.3 (T1 identity READ) — read the member's AUTHORITATIVE wholly-on-chain
   * SBT emblem for their OIDC `sub`. The post-reroll CitrateMemberSBT generates the
   * art on-chain: `tokenURI` returns a `data:application/json;base64,...` whose
   * `image` is a `data:image/svg+xml;base64,...`. This resolves the tokenId via
   * `isSubBound`/`tokenIdForSub(keccak256(sub))`, reads `tokenURI`, and returns the
   * decoded `image` data-URI — or `null` HONESTLY when the member has no SBT (the
   * caller then shows the local `sbtArt.ts` emblem as a labelled offline fallback,
   * never a fabricated on-chain mark — Rule 1). A PURE READ; signs nothing. The
   * Tauri impl invokes `sbt_token_uri`; the sim impl returns null (no real chain in
   * the web preview — the local fallback renders, honestly labelled).
   */
  sbtArt(sub: string): Promise<string | null>;
}

export interface CommissaryDomain {
  catalog(): Promise<{ name: string; capabilities: string[] }[]>;
}

export interface CommsDomain {
  connections(): Promise<Record<string, boolean>>;
}

/** One MCP connection's state (W4). Mirrors the Rust `ConnectionStatus`
 * (camelCase). NO token ever crosses this boundary — only connect facts. */
export interface ConnectionInfo {
  /** Internal service id: `github` | `gdrive` | `notion`. */
  service: string;
  connected: boolean;
  scope: string | null;
  connectedAt: number | null;
}

/** MCP connections (Google Drive / Notion / GitHub) via OAuth loopback-PKCE.
 * `start` opens the provider's authorize page in the SYSTEM browser and, on
 * success, seals the token in the OS-keyring-backed custody vault — the token
 * is never returned. Desktop-only: the sim preview reports honestly disconnected
 * and throws Unavailable on `start`. */
export interface ConnectionsDomain {
  /** Connect/disconnect state of all three services. */
  status(): Promise<ConnectionInfo[]>;
  /** Run the OAuth flow for one service; resolves with its new status. */
  start(service: string): Promise<ConnectionInfo>;
  /** Forget a service's sealed token. */
  disconnect(service: string): Promise<void>;
}

// ---- the full bridge surface ----------------------------------------
/** Opening federation apps / docs / explorer links in the system browser. The
 * URL is opened as-is (the destination RP runs its own OIDC login); a real
 * signed-in handoff (shared-authority SSO) is a later work-order item (WO-6).
 * Only https URLs are opened — never an arbitrary scheme. */
export interface ShellDomain {
  openExternal(url: string): Promise<void>;
}

export interface BridgeContract {
  readonly mode: "sim" | "tauri";
  shell: ShellDomain;
  config: ConfigDomain;
  custody: CustodyDomain;
  auth: AuthDomain;
  signing: SigningDomain;
  wallet: WalletDomain;
  node: NodeDomain;
  model: ModelDomain;
  agent: AgentDomain;
  memory: MemoryDomain;
  chat: ChatDomain;
  membership: MembershipDomain;
  commissary: CommissaryDomain;
  comms: CommsDomain;
  connections: ConnectionsDomain;
}

// =====================================================================
// CX — social-node domains (planset citrate-core-social). Frozen in CX-S0.2 and
// composed onto BridgeContract as `bridge: BridgeContract & CxBridge` (bridge/index.ts),
// so the existing monolith bridges are never edited. Each domain's impl lives in its own
// file owned by its lane (src/bridge/{tauri,sim}/<domain>.ts) — see .agentile/cx-ownership.map.
// Interfaces are contract-first (Rule 7): declared here BEFORE any implementation, and only
// changed via a serialized spine-PR (01_SCOPE §5.2).
// =====================================================================

/** A downloadable/local model, from Hugging Face, GitHub Releases, or the bundle (C-16). */
export interface ModelDescriptor {
  id: string;
  source: "hf" | "github" | "bundled";
  repo: string;
  file: string;
  revision?: string;
  sizeBytes: number;
  sha256: string;
  kind: "gguf" | "safetensors";
}

/** C-16 — model catalog & switcher (local + HF + GitHub). Wired in CX-S1. */
/** HUP-S0.3 — an interrupted catalog download (survives an app restart; resumable). */
export interface PartialDownload {
  /** The catalog id; `download(id)` resumes its `.part`. */
  id: string;
  file: string;
  downloadedBytes: number;
  totalBytes: number;
  /** Whole percent, 0–100. */
  pct: number;
}

export interface ModelsCatalogDomain {
  /** Locally-present, verified models. */
  local(): Promise<ModelDescriptor[]>;
  /** HUP-S0.3 — interrupted catalog downloads that can be resumed. */
  partials(): Promise<PartialDownload[]>;
  /** Search downloadable models from a connected source (HF Hub / GitHub Releases). */
  search(source: "hf" | "github", query: string): Promise<ModelDescriptor[]>;
  /** Download + verify a descriptor; resolves on Ready. `onProgress` (0–100) fires as the
   *  bytes stream, so a multi-GB GGUF shows a real progress bar instead of a silent block. */
  download(id: string, onProgress?: (pct: number) => void): Promise<void>;
  /** Switch the active local model (restarts llama-server -m). */
  select(id: string): Promise<void>;
  /** Hermes WP0.2b — the models registered on-chain in the ModelRegistry (40204). A PURE
   *  read (getAllModelHashes + getModel); NOT-yet-local, so the router marks them
   *  "download to use". Honest empty list on an unwired/failed read (never fabricated). */
  registry(): Promise<RegistryModel[]>;
  /** Hermes P3 / WP3.1 — propose registering a pulled+verified+pinned model on-chain
   *  (ModelRegistry.registerModel). Builds the calldata and submits a PENDING
   *  SignatureCeremony (Rule 3 — the human approves + broadcasts); nothing signs here.
   *  `ipfsCid` MUST be a real pinned CID (the contract requires it) — an empty CID
   *  rejects up-front. On confirm, the model appears in `registry()`. */
  register(input: RegisterModelInput): Promise<void>;
}

/** The fields ModelRegistry.registerModel needs (Hermes P3 / WP3.1). */
export interface RegisterModelInput {
  name: string;
  framework: string;
  version: string;
  /** A real pinned IPFS CID for the weights — required by the contract. */
  ipfsCid: string;
  sizeBytes: number;
  /** Per-inference price in wei (0 for free). */
  inferencePrice: number;
  description: string;
  license: string;
  tags: string[];
}

/** A model from the on-chain ModelRegistry (Hermes WP0.2b). Mirrors the Rust RegistryModel. */
export interface RegistryModel {
  /** The `0x`-hex modelHash — the stable id. */
  id: string;
  /** Human name. */
  name: string;
  /** `0x`-hex owner address. */
  owner: string;
  /** IPFS CID of the model weights. */
  ipfsCid: string;
}

// ── C-17 storage & pinning file store (lane s2) ──
export interface PinRow {
  cid: string;
  sizeBytes: number;
  bondSalt: string;
  pinState: "pinned" | "pinning" | "challenged" | "unpinned";
  addedAt: number;
}
export interface StorageDomain {
  /** Add a local file to IPFS; returns its CID. */
  add(path: string): Promise<{ cid: string; sizeBytes: number }>;
  /** Pin a CID with a SALT bond — ceremony-gated (D-18). */
  pin(cid: string, bondSalt: string): Promise<void>;
  list(): Promise<PinRow[]>;
  retrieve(cid: string): Promise<{ path: string }>;
  unpin(cid: string): Promise<void>;
}

// ── C-19 groups: secure 1:1 + group comms, admin RBAC (lane s3) ──
export type GroupRole = "owner" | "admin" | "member" | "guest" | "agent";
export interface GroupMember {
  address: string;
  role: GroupRole;
}
export interface Group {
  id: string;
  /** Human name (the daemon stores it; `list` returns it). Amended in post-S0 (CX-S3.5). */
  name: string;
  owner: string;
  kind: "dm" | "channel" | "forum";
  members: GroupMember[];
}
export interface GroupMessage {
  id: string;
  groupId: string;
  sender: string;
  body: string;
  ts: number;
}
export interface GroupsDomain {
  create(kind: Group["kind"], name: string): Promise<Group>;
  list(): Promise<Group[]>;
  /** This member's own comms address (what the rosters key on) — used to exclude self from the People directory. */
  selfAddress(): Promise<string>;
  join(groupId: string): Promise<void>;
  /**
   * Owner-invite: add a member who has published a key package to the shared relay. The daemon
   * produces + publishes the MLS welcome; the invitee then `join`s. Amended post-S0 (CX-S3.5).
   */
  addMember(groupId: string, address: string): Promise<void>;
  roster(groupId: string): Promise<GroupMember[]>;
  /** Grant/change a role — a signed RoleAssertion enforced at the relay. */
  assignRole(groupId: string, address: string, role: GroupRole): Promise<void>;
  /** Atomic offboard — drops all four planes in one epoch (ADR-001). */
  offboard(groupId: string, address: string): Promise<void>;
  send(groupId: string, body: string): Promise<void>;
  messages(groupId: string): Promise<GroupMessage[]>;
  /**
   * Flag-A — the networked relay-link health for the status chip. One of:
   * `"idle"` (daemon not started) · `"local"` (in-process relay) · `"connecting"` (up, unconfirmed) ·
   * `"connected"` · `"degraded"` (link down, ops failing). Reported, never a fake — a bounded read.
   */
  relayStatus(): Promise<string>;
}

// ── C-20 group clusters: private P2P + shared files/compute (lane s4) ──
export interface ClusterPeer {
  address: string;
  online: boolean;
  /** HUP-S8.1: set when this peer is a linked device; the member it acts for. */
  member?: string;
}
/** HUP-S8.1: one linked device under a member, as the cluster daemon admits it. */
export interface ClusterDevice {
  device: string;
  index: number;
  label: string;
  issuedAt: number;
  online: boolean;
}
/** HUP-S8.1: a cluster member with its linked devices. */
export interface ClusterMemberDevices {
  member: string;
  role: string;
  online: boolean;
  devices: ClusterDevice[];
}
/** HUP-S8.1: a DeviceLink this machine knows (no signatures cross the bridge). */
export interface DeviceLinkView {
  device: string;
  member: string;
  wallet: string;
  index: number;
  label: string;
  issuedAt: number;
  thisDevice: boolean;
}
/** HUP-S8.1: this machine's device address (null before its key exists) + known links. */
export interface DeviceLinks {
  thisDevice: string | null;
  links: DeviceLinkView[];
  revoked: string[];
}
export interface ClusterStatus {
  groupId: string;
  online: number;
  total: number;
  sharedFiles: string[];
}
export interface ClusterDomain {
  status(groupId: string): Promise<ClusterStatus>;
  join(groupId: string): Promise<void>;
  peers(groupId: string): Promise<ClusterPeer[]>;
  /** Co-pin a CID across the Group roster. */
  shareFile(groupId: string, cid: string): Promise<void>;
  leave(groupId: string): Promise<void>;
  /** HUP-S8.1: the group's members with their linked devices (live). */
  devices(groupId: string): Promise<ClusterMemberDevices[]>;
  /** HUP-S8.1: this machine's device key address + the links it knows. Never mints a key. */
  myDevices(): Promise<DeviceLinks>;
  /** HUP-S8.1: open the wallet ceremony that links THIS machine. Signs nothing. */
  linkDeviceRequest(label: string): Promise<CeremonyView>;
  /** HUP-S8.1: the person approved; complete + store the link. */
  linkDeviceApprove(id: string, rawAck: boolean): Promise<DeviceLinks>;
  /** HUP-S8.1: the person declined; nothing was signed. */
  linkDeviceReject(id: string): Promise<void>;
  /** HUP-S8.1: revoke a device of yours (permanent for that device key). */
  revokeDevice(device: string): Promise<DeviceLinks>;
  /** HUP-S8.1: this machine's signed link as a code to paste on another of YOUR devices. */
  exportDeviceLink(): Promise<string>;
  /** HUP-S8.1: add another of your own devices from its code (verified before it is stored). */
  importDeviceLink(code: string): Promise<DeviceLinks>;
}

// ── C-21 train-together: group federated training (lane s5) ──
export type RoundPhase = "idle" | "open" | "aggregating" | "committed" | "settled";
export interface RoundStatus {
  groupId: string;
  round: number;
  phase: RoundPhase;
  participants: number;
}
export interface RewardInfo {
  round: number;
  weight: string;
  salt: string;
}
export interface TrainingDomain {
  start(groupId: string): Promise<void>;
  status(groupId: string): Promise<RoundStatus>;
  /** Lease -> local DiLoCo train -> submit a Q16 pseudo-gradient. */
  contribute(groupId: string): Promise<void>;
  reward(groupId: string): Promise<RewardInfo>;
  /** Claim the member's SALT reward — ceremony-gated (D-18/D-23). */
  claim(groupId: string): Promise<void>;
}

// ── HUP-S9.4: Hermes plans, explains and starts federated rounds; LoRA eval gate ──
// Shapes mirror src-tauri/src/fl_rounds.rs (serde camelCase). Data source: the compute-pool
// training-coordinator `GET /v1/status` at the configured URL; there is no default coordinator.
export type FlCapability = "probe" | "federated" | "h01";
export interface FlRoundProposal {
  requires: FlCapability;
  loraRank: number;
  maxTrajectories: number;
  leaseHours: number;
}
export type FlSettlement = "shadow" | "live" | "unknown";
export type FlPoolPhase = "noWork" | "open" | "running" | "complete";
export interface FlCoordinatorStatus {
  pending: number;
  leased: number;
  done: number;
  quarantined: number;
  workers: number;
  settlement: FlSettlement;
  phase: FlPoolPhase;
}
export type FlCoordinatorView =
  | { state: "notConfigured" }
  | { state: "unreachable"; url: string; reason: string }
  | { state: "live"; url: string; status: FlCoordinatorStatus };
export interface FlRoundExplanation {
  data: string;
  compute: string;
  reward: string;
  privacy: string;
  status: string;
}
export interface FlRoundPlan {
  planHash: string;
  createdAtMs: number;
  coordinator: FlCoordinatorView;
  proposal: FlRoundProposal;
  baseModel: string;
  device: { tier: string | null; accelerator: boolean | null };
  explain: FlRoundExplanation;
  canStart: boolean;
  blockers: string[];
}
export interface FlStartReceipt {
  planHash: string;
  coordinatorUrl: string;
  authorizedAtMs: number;
  /** Always false in this build: the device training worker is not bundled (HUP-S9.1/S9.2). */
  trainingStarted: boolean;
  note: string;
}
export interface FlStartRecord {
  planHash: string;
  coordinatorUrl: string;
  requires: FlCapability;
  authorizedAtMs: number;
}
export interface FlCoordinatorConfig {
  url: string | null;
  source: "env" | "settings" | "invalid" | "none";
  settingsUrl: string | null;
  note: string | null;
}
export interface FlAdapterGateRequest {
  adapterPath: string;
  expectedSha256: string;
  baseToolsPath: string;
  candidateToolsPath: string;
  baseQaPath?: string;
  candidateQaPath?: string;
}
export interface FlMetricDelta {
  metric: string;
  base: number | null;
  candidate: number | null;
  improvement: number | null;
}
export interface FlAdapterGateRecord {
  adapterSha256: string;
  adapterPath: string;
  baseModel: string;
  decidedAtMs: number;
  decision: {
    verdict: "ACCEPT" | "REJECT";
    reasons: string[];
    metrics: FlMetricDelta[];
    compositeBase: number;
    compositeCandidate: number;
  };
}
export interface FlOverview {
  config: FlCoordinatorConfig;
  starts: FlStartRecord[];
  gates: FlAdapterGateRecord[];
  activeAdapter: string | null;
  storeError: string | null;
}
export interface FlRoundsDomain {
  overview(): Promise<FlOverview>;
  /** Persist (or clear, with null) the coordinator base URL. Core validates it. */
  setCoordinator(url: string | null): Promise<FlCoordinatorConfig>;
  /** Read the coordinator and build the plan + plain-words explanation. Read-only. */
  plan(proposal?: FlRoundProposal): Promise<FlRoundPlan>;
  lookupPlan(planHash: string): Promise<FlRoundPlan>;
  /** Record the member's HIC-1 approval of exactly this plan. Call only after the approval card. */
  start(planHash: string): Promise<FlStartReceipt>;
  gateAdapter(request: FlAdapterGateRequest): Promise<FlAdapterGateRecord>;
  /** Load an ACCEPTED adapter into llama-server (`--lora`). Returns the served copy's path. */
  loadAdapter(sha256: string): Promise<string>;
  unloadAdapter(): Promise<void>;
}

// ── C-22 agent/Hermes harness: skills, code, comms (lane s6) ──
export interface AgentSkill {
  name: string;
  description: string;
}
export interface AgentApproval {
  id: string;
  kind: "code" | "chain" | "shell";
  summary: string;
  /** A chain effect's real target + calldata (what would actually be signed). Absent for code/shell. */
  to?: string;
  data?: string;
}
export interface AgentHarnessStatus {
  running: boolean;
  skills: number;
  pendingApprovals: number;
}
/** A skill registered on-chain in the SkillRegistry (Hermes P2). Mirrors the Rust RegistrySkill. */
export interface RegistrySkill {
  /** The `0x`-hex skillHash — the stable id. */
  id: string;
  /** Human name (e.g. "hf-model-register"). */
  name: string;
  /** Semver. */
  version: string;
  /** IPFS CID of the WASM capsule manifest (empty for a not-yet-published skill). */
  manifestCid: string;
  /** Short description. */
  description: string;
  /** `0x`-hex owner address. */
  owner: string;
}

/** HUP-S4.3 — which MCP servers Hermes may use (core writes the sidecar's allowlist). Mirrors the
 *  Rust `McpSettings`. Both default off (default pending owner sign-off). */
export interface HermesMcpSettings {
  mem: boolean;
  scan: boolean;
}

/** HUP-S4.3 — one server row (Rust `McpServerView`). */
export interface HermesMcpServer {
  name: string;
  label: string;
  transport: string;
  enabled: boolean;
  available: boolean;
  detail: string;
}

/** HUP-S4.3 — the settings view (Rust `McpView`). */
export interface HermesMcpView {
  settings: HermesMcpSettings;
  servers: HermesMcpServer[];
  /** An allowlist file is in place for the next Hermes start. */
  configWritten: boolean;
  /** Hermes is running and reads the allowlist only at start. */
  restartRequired: boolean;
}

export interface AgentHarnessDomain {
  /** Sidecar a Hermes agent (keyless; every chain effect stays ceremony-gated). */
  start(): Promise<void>;
  status(): Promise<AgentHarnessStatus>;
  skills(): Promise<AgentSkill[]>;
  /** Hermes P2 — the skills registered on-chain in the SkillRegistry (40204). A PURE read
   *  (getAllSkillHashes + getSkill); this is what Hermes "ships with" instead of an empty set.
   *  Honest empty list on an unwired/failed read (never fabricated). */
  registrySkills(): Promise<RegistrySkill[]>;
  /** Run a skill/code task behind the mandatory HITL approval flow. */
  runSkill(name: string, argsJson: string): Promise<{ ok: boolean }>;
  pendingApprovals(): Promise<AgentApproval[]>;
  stop(): Promise<void>;
  /**
   * CX-S6.3 — bridge the sidecar's head CHAIN effect into a PENDING ceremony (signs nothing) and
   * return the decoded CeremonyView to show at the Signature Ceremony, or null when the head is a
   * code/shell effect or nothing is pending. The human then approves via signing.broadcast and the
   * caller must call `resolve(true, id)` to let the capsule proceed. PBA-L7b-003: `id` is the call the
   * member is reviewing; if the head is a different call this rejects with `STALE_APPROVAL`.
   */
  bridgePending(id: string): Promise<CeremonyView | null>;
  /** CX-S6.3 — resolve the sidecar's head effect after the human decided: true proceeds, false aborts.
   *  PBA-L7b-003: BOUND to the reviewed call `id` — if the sidecar's head is no longer that call the
   *  promise rejects with a message starting `STALE_APPROVAL` and NOTHING is resolved (re-review). */
  resolve(approve: boolean, id: string): Promise<void>;
  /** HUP-S1.1c — the sidecar-owned agent loop (ADR loop-in-sidecar). Open a session on the LOCAL
   *  model (endpoint + key are Rust-owned; the webview supplies only the prompt + tool specs, every
   *  tool runs in core through its approval gates). Returns the session id. */
  sessionOpen(systemPrompt: string, toolsJson: string, persona?: SessionPersonaChoice | null): Promise<string>;
  sessionSend(id: string, text: string): Promise<void>;
  /** Long-poll the session's events after sequence `after` (waits up to `waitMs` for new ones). */
  sessionEvents(id: string, after: number, waitMs: number): Promise<SessionEventsPage>;
  /** Hand back a core-hosted tool's result after core's own gates ran it. */
  sessionToolResult(id: string, callId: string, status: "ok" | "denied" | "error", content: string): Promise<void>;
  sessionStop(id: string): Promise<void>;
  /** HUP-S10.3 — open a session for a scheduled daemon run: the chat session body marked
   *  `unattended`, so every effectful call needs the member's explicit decision. Local model only. */
  sessionOpenUnattended(systemPrompt: string, toolsJson: string): Promise<string>;
  /** HUP-S10.3 — close a session (a finished daemon run). */
  sessionClose(id: string): Promise<void>;
  /** HUP-S1.4 — the interview tracks the sidecar serves (question sets, persona, skills, workflow,
   *  gates). Rejects when the sidecar isn't running. */
  tracks(): Promise<InterviewTrack[]>;
  /** HUP-S1.4 — answers → brief. Unanswered questions take their defaults (empty answers = "just use
   *  defaults"); no `track` lets the sidecar suggest one from the goal. A refusal (no track fits, an
   *  answer outside its choices) rejects with a message starting `BRIEF_REFUSED: `. Builds nothing. */
  briefCreate(track: string | null, goal: string, answers: Record<string, string>): Promise<BriefDraft>;
  /** HUP-S1.4 — validate a member-edited brief against its track (required gates and the workflow
   *  can't be edited away). A refusal rejects with `BRIEF_REFUSED: <reason>`. */
  briefCheck(brief: Brief): Promise<{ ok: boolean; markdown: string }>;
  /** HUP-S4.3 — the MCP servers Hermes may use (mem-mcp, CitrateScan). Rejects outside the app. */
  mcpSettings(): Promise<HermesMcpView>;
  /** HUP-S4.3 — save the member's choice; takes effect at the next Hermes start. */
  mcpSet(settings: HermesMcpSettings): Promise<HermesMcpView>;
  /** HUP-S2.9 — the agent session's checkpointed file changes (newest first). `enabled: false`
   *  with a note when the sidecar cannot undo. Rejects when the sidecar isn't running. */
  checkpoints(id: string): Promise<CheckpointList>;
  /** HUP-S2.9 — undo one agent file change. A refusal (a file changed since, a pruned step) resolves
   *  with `ok: false` and the reason; nothing was restored. */
  undoStep(id: string, seq: number): Promise<UndoOutcome>;
  /** HUP-S2.9 — undo every change of the session not undone yet (all or nothing). */
  undoSession(id: string): Promise<UndoOutcome>;
  /** HUP-S2.6 — record the member's answer on an approval card or a wallet review in core's HIC
   *  outbox (then in the decision records the nightly anchor covers). Resolves with the record id,
   *  or null in a build that keeps no decision records (web/dev); rejects when it could not be
   *  written. */
  recordDecision(kind: "ceremony.approval" | "agent.tool_approval", decision: "approved" | "denied", subject: string, reason: string): Promise<number | null>;
  /** HUP-S3.4 — run a declarative, verifier-judged workflow in a session; returns the run id. */
  workflowRun(sessionId: string, workflow: WorkflowSpec): Promise<string>;
  /** HUP-S3.4 — a workflow run's state and (when verified) its evidence. */
  workflowStatus(sessionId: string, runId: string): Promise<WorkflowRunView>;
  /** HUP-S3.4 — whether learning is on in the sidecar, and whether publishing is. */
  learnStatus(): Promise<LearnStatus>;
  /** HUP-S3.4 — proposals waiting for the member (`all`: every kept one). */
  learnProposals(all?: boolean): Promise<LearnProposal[]>;
  /** HUP-S3.4 — propose a skill or memory from a VERIFIED run of that session. */
  learnPropose(sessionId: string, runId: string, content: LearnContent): Promise<LearnProposal>;
  /** HUP-S3.4 — accept (HIC-1, recorded first). `acknowledged` names the conflicts accepted anyway.
   *  A refusal rejects with a message starting `LEARN_REFUSED: `. */
  learnAccept(id: string, acknowledged: string[]): Promise<LearnAcceptResult>;
  /** HUP-S3.4 — reject (recorded, final). */
  learnReject(id: string, reason: string): Promise<void>;
  /** HUP-S3.4 — the learned-memory ledger, with each memory's place in the memory graph. */
  learnMemories(): Promise<LearnedMemory[]>;
  /** HUP-S3.4 — store learned memories that are still waiting for the memory store. */
  learnStorePending(): Promise<LearnedMemory[]>;
  /** HUP-S3.4 — resolve a contradiction: keep one learned memory, set aside another (HIC-1; the
   *  sidecar records the decision before anything changes). Returns the updated ledger. A refusal
   *  rejects with a message starting `LEARN_REFUSED: `. */
  learnResolve(keep: string, retract: string): Promise<LearnedMemory[]>;
  /** HUP-S3.4 — publish a saved skill to the SkillRegistry (HIC-1 ceremony). Rejects with
   *  `PUBLISH_DISABLED: ` while publishing is off. */
  learnPublish(id: string, version: string): Promise<void>;
  /** HUP-S3.3 + S3.7 — the shipped personas with their prompt fragments (names are placeholders
   *  pending owner sign-off). Rejects when the sidecar isn't running. */
  personas(): Promise<HermesPersona[]>;
  /** HUP-S3.3 — the sidecar validates a member-defined persona and renders its fragment. A refusal
   *  rejects with a message starting `PERSONA_REFUSED: `. */
  personaCheck(persona: CustomPersonaInput): Promise<HermesPersona>;
  /** HUP-S3.3 — every track's workflow family, with `needs_tools` and, when this sidecar cannot
   *  run one, `unavailable` (why). */
  workflows(): Promise<TrackWorkflow[]>;
  /** HUP-S3.3 (US-3.3 AC2) — run a track's catalog workflow in a session by id. Rejects with a
   *  message starting `WORKFLOW_REFUSED: ` (unknown workflow, or a tool the session lacks). Read
   *  the run with `workflowStatus`. */
  trackWorkflowRun(sessionId: string, workflowId: string): Promise<TrackWorkflowStart>;
  /** HUP-S2.2 — the `shell_run` commands the sidecar holds for the member's decision in a session. */
  shellPending(sessionId: string): Promise<ShellPendingView[]>;
  /** HUP-S2.2 — allow or decline one held command. The decision carries the exact argv and folder
   *  the member was shown; the sidecar refuses it (rejects with `SHELL_DECISION_REFUSED: `) when
   *  they differ from what is waiting or nothing with that id waits any more. */
  shellDecide(sessionId: string, id: string, allow: boolean, argv: string[], cwd: string): Promise<void>;
}

/** HUP-S2.2 — the OS sandbox a held command would run in, as the sidecar describes it. */
export interface ShellSandboxSummary {
  /** "seatbelt" | "bwrap" | "none". */
  backend: string;
  enforced: boolean;
  /** "denied" | "allowed". */
  network: string;
  writable: string[];
  readable_extra: string[];
  /** One line for people. */
  summary: string;
}

/** HUP-S2.2 — one `shell_run` command waiting for the member (`GET /sessions/:id/shell/pending`). */
export interface ShellPendingView {
  id: string;
  callId: string;
  tool: string;
  hic: string;
  /** Exactly as proposed: the program name first, one entry per argument. */
  argv: string[];
  resolvedProgram: string;
  /** The canonical folder it runs in. */
  cwd: string;
  timeoutSecs: number;
  sandbox: ShellSandboxSummary;
  expiresInSecs: number;
}

/** HUP-S3.3 — the persona a sidecar session applies (skill allowlist + tool emphasis): a shipped
 *  persona by id, or a member-defined one. */
export type SessionPersonaChoice = { persona: string } | { customPersona: CustomPersonaInput };

/** HUP-S3.3 — what `POST /sessions/:id/track_workflows` answers. */
export interface TrackWorkflowStart {
  run_id: string;
  workflow_id: string;
  track: string;
  evidence: string;
}

// HUP-S3.3 + S3.7 — persona and track-workflow wire shapes. These mirror the sidecar's
// `agent-loop::personas` / `agent-loop::workflows` views verbatim (snake_case).
export interface HermesPersona {
  id: string;
  role: string;
  name: string;
  name_status: string;
  summary: string;
  voice: string;
  tone: string;
  style_rules: string[];
  default_track: string;
  default_workflow: string;
  tool_emphasis: string[];
  skills: string[];
  /** Optional voice id for the existing speech engine; null = the system voice. */
  tts_voice?: string | null;
  /** What the chat appends to its system prompt while this persona is active. */
  prompt_fragment: string;
  /** True while the shipped name is a placeholder (the UI says so). */
  name_pending_sign_off: boolean;
  custom: boolean;
  /** Allowlisted skills this sidecar has installed (absent for a custom persona's check). */
  skills_installed?: string[];
}
export interface CustomPersonaInput {
  id: string;
  name: string;
  summary: string;
  voice: string;
  tone: string;
  style_rules: string[];
  default_track: string;
  tool_emphasis: string[];
  skills: string[];
  tts_voice: string | null;
}
export interface TrackWorkflowStep {
  id: string;
  instruction: string;
  max_attempts: number;
  verifier_names: string[];
}
export interface TrackWorkflow {
  id: string;
  track: string;
  title: string;
  summary: string;
  is_default: boolean;
  /** "tool-report" (a tool's own report decides) or "answer-shape" (the answer's structure). */
  evidence: "tool-report" | "answer-shape" | string;
  tools: string[];
  /** Tools a passing run must call. */
  needs_tools?: string[];
  /** Why this sidecar cannot run the workflow (e.g. the toolchain is off); null = it can. */
  unavailable?: string | null;
  verifier_names: string[];
  steps: TrackWorkflowStep[];
}

// HUP-S1.4 — interviewer wire shapes. These mirror the sidecar's `agent-loop::interview` types
// verbatim (snake_case), so a brief round-trips webview → core → sidecar unchanged.
export interface InterviewQuestion {
  id: string;
  ask: string;
  /** Empty = free text; otherwise the answer must be one of these. */
  choices: string[];
  default: string;
}
export interface InterviewTrack {
  id: string;
  title: string;
  summary: string;
  persona: string;
  skills: string[];
  workflow: string;
  /** False until the workflow ships; the UI says so (Rule 1). */
  workflow_available: boolean;
  ships_in?: string | null;
  gates: string[];
  questions: InterviewQuestion[];
}
export interface BriefConstraint {
  id: string;
  ask: string;
  answer: string;
  from_default: boolean;
}
export interface Brief {
  track: string;
  goal: string;
  constraints: BriefConstraint[];
  persona: string;
  skills: string[];
  workflow: string;
  workflow_available: boolean;
  ships_in?: string | null;
  gates: string[];
}
export interface BriefDraft {
  brief: Brief;
  markdown: string;
}

/** HUP-S1.1c — one page of a sidecar session's event log. */
export interface SessionEventsPage {
  events: { seq: number; event: Record<string, unknown> }[];
  lastSeq: number;
  busy: boolean;
}

// ── Local instruction-skills (Hermes "write & run skills"). A skill is a markdown playbook the agent
// authors and stores on THIS device; running it loads the instructions back into the agent's loop so it
// carries the task out with its existing (ceremony-gated) tools. Local files only; signs nothing. ──
export interface LocalSkill {
  /** The human name as authored. */
  name: string;
  /** One-line description of what the skill does. */
  description: string;
  /** Stable filename slug (the id skill_run uses). */
  slug: string;
}
export interface AgentSkillsDomain {
  /** The skills the agent has authored on this device (honest-empty, never fabricated). */
  list(): Promise<LocalSkill[]>;
  /** Author a local instruction-skill. Local file only; signs/runs nothing. PBA-L7b-002: with
   *  `overwrite` false (the default) an existing skill is NOT replaced — the call rejects with an
   *  error whose message starts `SKILL_EXISTS`; replacing it needs the member's approval first. */
  write(name: string, description: string, instructions: string, overwrite?: boolean): Promise<LocalSkill>;
  /** The instruction body of an authored skill, by slug or name. */
  read(name: string): Promise<string>;
  /** Remove an authored skill (idempotent). */
  remove(name: string): Promise<void>;
}

// ── Social identity (Connections · social discovery). Privacy model: ADR-2026-08-30. ──
// The address↔identity binding is device-local + shared server-blind; NEVER on-chain by default;
// private by default; "verified" requires OAuth ownership + a wallet-signed IdentityBinding through
// the Signature Ceremony (Rule 3). The OAuth token seals in the OS keyring and NEVER crosses this
// bridge — only the LinkedIdentity connect facts do.
export type SocialNetwork = "x" | "linkedin" | "discord";
export type SocialVisibility = "private" | "groups";
export interface LinkedIdentity {
  network: SocialNetwork;
  /** The display handle from the verified OAuth account (e.g. "dana"). */
  handle: string;
  /** verified = OAuth ownership + wallet-signed IdentityBinding via the ceremony (ADR D3). */
  verified: boolean;
  /** private (default) | groups-only. Never public unless the explicit on-chain opt-in is taken (ADR D2). */
  visibility: SocialVisibility;
  linkedAt: number;
  /** #61 — published to the opt-in find-via-X directory (a separate, more public opt-in than visibility). */
  directoryPublished: boolean;
}

/** #61 — a live directory binding for an exact handle (find-via-X lookup). Mirrors the Rust DirectoryHit. */
export interface DirectoryHit {
  address: string;
  boundAt: number;
}

/** #61 — one directory search (typeahead) row. Mirrors the Rust DirectorySearchHit. */
export interface DirectorySearchHit {
  handle: string;
  address: string;
  displayName?: string;
}
/** A face a viewer may see for an address — a verified, group-visible handle (resolver output). */
export interface ResolvedIdentity {
  address: string;
  network: SocialNetwork;
  handle: string;
}
/** The shareable binding payload that rides a group message (server-blind). Carries no token. */
export interface ExportedBinding {
  network: SocialNetwork;
  handle: string;
  address: string;
  nonce: string;
  signature: string;
}
/** Control-message sentinel: a group message body starting with this carries an ExportedBinding
 *  (JSON follows). Clients ingest + hide these; they never render in the conversation. */
export const SOCIAL_BINDING_MSG_PREFIX = "cbind1:";
export interface SocialDomain {
  /** The user's current linked identities (device-local). Honest-empty when none/unwired. */
  status(): Promise<LinkedIdentity[]>;
  /** Begin the OAuth ownership proof (opens the system browser, loopback-PKCE). Returns the resulting
   *  UNVERIFIED, private link (handle from OAuth). The token seals in the keyring, never returned. */
  start(network: SocialNetwork): Promise<LinkedIdentity>;
  /** Open a ceremony over the wallet-signed IdentityBinding (ADR D3). Returns the CeremonyView the
   *  approval UI renders; signs NOTHING (the wallet signs at approve). */
  verifyRequest(network: SocialNetwork): Promise<CeremonyView>;
  /** Approve a SPECIFIC pending verification id → the wallet signs the binding at the ceremony and it
   *  is recorded; the link becomes verified. Never returns a signature. */
  verifyApprove(id: string, rawAck: boolean): Promise<LinkedIdentity>;
  /** Drop a pending verification the human rejected. */
  verifyForget(id: string): Promise<void>;
  /** Set a link's visibility (private | groups). Narrowing takes effect immediately (ADR D2). */
  setVisibility(network: SocialNetwork, visibility: SocialVisibility): Promise<LinkedIdentity>;
  /** Forget a link — drop the local record + tombstone to group members (ADR revocation). */
  disconnect(network: SocialNetwork): Promise<void>;
  /** Resolve member addresses → verified, group-visible faces this device knows (own + foreign). */
  resolve(addresses: string[]): Promise<ResolvedIdentity[]>;
  /** The shareable payload for a verified, group-visible link (null if none) — rides a group message. */
  exportBinding(network: SocialNetwork): Promise<ExportedBinding | null>;
  /** Accept a peer's binding from the relay. VERIFIES (sender==address + signature recovers) before
   *  storing; returns whether it was accepted. */
  ingestBinding(sender: string, binding: ExportedBinding): Promise<boolean>;
  /** #61 — opt-in PUBLISH a verified link to the find-via-X directory. Opens a ceremony over the
   *  directory-scoped statement the wallet signs (ADR D-7 exception; self-published only). Returns the
   *  CeremonyView; signs NOTHING (approve at directoryPublishApprove). Requires a verified link. */
  directoryPublishRequest(network: SocialNetwork): Promise<CeremonyView>;
  /** #61 — approve a pending publish → the wallet signs, the binding POSTs to the authority with the
   *  member's Bearer, and the link is marked published. Returns the updated link. Never a signature. */
  directoryPublishApprove(id: string, rawAck: boolean): Promise<LinkedIdentity>;
  /** #61 — open a ceremony to REVOKE (unpublish) a directory binding. Returns the CeremonyView. */
  directoryUnpublishRequest(network: SocialNetwork): Promise<CeremonyView>;
  /** #61 — approve a pending revoke → sign, tombstone at the authority, mark unpublished. */
  directoryUnpublishApprove(id: string, rawAck: boolean): Promise<LinkedIdentity>;
  /** #61 — drop a pending directory ceremony the human rejected. */
  directoryForget(id: string): Promise<void>;
  /** #61 — resolve an EXACT social handle to its published address, or null if nobody opted in. */
  directoryLookup(platform: SocialNetwork, handle: string): Promise<DirectoryHit | null>;
  /** #61 — typeahead over published handles (find-via-X). Honest-empty on no match. */
  directorySearch(platform: SocialNetwork, query: string): Promise<DirectorySearchHit[]>;
}

/**
 * The CX domain surface, composed onto the bridge alongside the legacy domains.
 * FROZEN (CX-S0.2): each domain's impl lives in its own lane-owned file; changing a
 * signature/DTO here requires a serialized spine-PR (01_SCOPE §5.2). Every entry is
 * additive — no existing interface above this block is modified.
 */
// ── Group claimable invites (ADR-2026-08-30 D4). Invite by @handle without a directory: the owner
// mints a claimable invite + shares its link over the platform DM; the invitee volunteers their
// address in a claim (consent); the owner verifies the one-time token and adds them the normal way.
// Citrate never resolves a handle to an address.
export interface PendingInvite {
  group: string;
  token: string;
  forHandle: string;
  createdAt: number;
  /** CONNECT-S1 — the full share link (with the sealed-invite key); "" for pre-S1 invites. */
  link?: string;
}
export interface InviteMinted {
  token: string;
  /** citrate://invite?g=<group>&t=<token> — the owner DMs this to the @handle on the platform. */
  link: string;
}
/** #73 — one entry in the device-local referral audit ledger. Mirrors the Rust `ReferralEvent`.
 *  The member's own copy of "invites I minted / groups I joined via one"; the relay's referral tally
 *  is authoritative for airdrop scoring. `tokenHash` (BLAKE3(token) hex) ties a row to that record. */
export interface ReferralEvent {
  /** `inviter` (I minted an invite) | `joiner` (I self-admitted via one). */
  role: string;
  /** `invited` | `joined` | `revoked`. */
  event: string;
  group: string;
  groupName: string;
  tokenHash: string;
  /** The handle the invite was labelled for (inviter rows only). */
  forHandle: string;
  /** Unix seconds. */
  ts: number;
}

export interface InvitesDomain {
  /** Mint a single-use, group-bound invite (labelled for a handle) + share link. No address
   *  resolution. INVITE-S2: publishes BLAKE3(token) + expiry to the relay so the invitee can
   *  self-admit; fails closed if the relay is unreachable. */
  create(group: string, forHandle: string): Promise<InviteMinted>;
  /** INVITE-S2 — INVITEE: SELF-ADMIT into the group from an invite link, in one click. Joins by MLS
   *  external commit with no owner action (even if the owner is offline). Honest error on a spent/
   *  expired/revoked token or an unreachable relay. */
  redeem(link: string): Promise<void>;
  /** The owner's outstanding invites for a group. */
  list(group: string): Promise<PendingInvite[]>;
  /** Verify + CONSUME a claim's one-time token against an outstanding invite. The caller then adds
   *  the volunteered address the normal way. Returns whether the token was valid. (Claim-back path.) */
  verifyConsume(group: string, token: string): Promise<boolean>;
  /** Drop an outstanding invite — also revokes it on the relay so a leaked link can no longer redeem. */
  revoke(group: string, token: string): Promise<void>;
  /** CONNECT-S1 — INVITEE: seal + submit a claim for an invite link to the relay's server-blind
   *  inbox (the claim-back fallback). The owner then sees it via `pollClaims`. */
  submitClaim(link: string): Promise<void>;
  /** CONNECT-S1 — OWNER: poll + open the sealed claims for a group's outstanding invites. */
  pollClaims(group: string): Promise<InviteClaim[]>;
  /** #73 — the device-local referral audit ledger (newest last). The member's own copy. */
  referralLog(): Promise<ReferralEvent[]>;
  /** #73 — the referral ledger as pretty JSON, for the member to save as their audit copy. */
  exportReferralLog(): Promise<string>;
}

/** A volunteered claim recovered from the server-blind inbox (owner-side). */
export interface InviteClaim {
  group: string;
  token: string;
  address: string;
}

/** The fields a contract-creation deploy needs (Hermes P3 / WP3.2). The bytecode is
 *  caller-supplied + compiled — the app never fabricates contract code (Rule 1). */
export interface ContractDeployInput {
  /** Compiled deploy bytecode (`0x`-hex or bare hex). Required. */
  bytecodeHex: string;
  /** ABI-encoded constructor args (`0x`-hex), or omit for a no-arg constructor. */
  constructorArgsHex?: string;
  /** Wei to send with the creation (decimal string), or omit for 0. */
  valueWei?: string;
  /** Gas limit (from a fork-sim estimate), or omit for the default. */
  gas?: number;
}

/** HUP-S6.4 — what `contract_deploy` returns: the pending ceremony view plus the READY D-4
 *  gate record core checked for exactly this init code. */
export type DeployProposalView = CeremonyView & { gate: DeployGateRecord };

export interface ContractsDomain {
  /** Hermes P3 / WP3.2 — propose deploying a compiled contract. Assembles the init code
   *  and submits a PENDING SignatureCeremony carrying the `to`-less creation tx (Rule 3 —
   *  the human approves + broadcasts via signing.broadcast(view.id); nothing signs here).
   *  Returns the decoded CeremonyView (a "contract creation") for the review modal. Empty
   *  bytecode rejects. HUP-S6.4: rejects with the honest refusal (naming the failing gate
   *  items) unless the D-4 deploy gate is READY for exactly this init code. */
  deploy(input: ContractDeployInput): Promise<DeployProposalView>;
  /** HUP-S6.4 — the init-code hash a deploy would carry and its gate record (null if none). */
  gateLookup(bytecodeHex: string, constructorArgsHex?: string): Promise<DeployGateLookup>;
  /** HUP-S6.4 — hand verifier outputs to core; core parses them and stores the verdict. */
  gateSubmit(inputs: DeployGateInputs): Promise<DeployGateRecord>;
  /** HUP-S4.3 — a contract's verified source/ABI/compiler from CitrateScan (read-only). Rejects
   *  only when the lookup itself fails; "not verified" is a normal answer. */
  verifiedSource(address: string): Promise<VerifiedSourceView>;
  /** HUP-S6.7 — CitrateScan's verified source and ABI for an address (`contract_source`). */
  source(address: string): Promise<ContractSourceView>;
  /** HUP-S6.7 — deployed code size at `address` on `target` ("citrate" or a loopback fork URL). */
  codeSize(target: string, address: string): Promise<number>;
  /** HUP-S6.7 — a read-only eth_call; resolves with the raw `0x` return data. */
  viewCall(target: string, address: string, calldata: string): Promise<string>;
  /** HUP-S6.7 — a write call on 40204 as a PENDING SignatureCeremony (nothing signs here). */
  proposeWrite(address: string, calldata: string, valueWei: string): Promise<CeremonyView>;
  /** HUP-S6.6 — where a hello-mint project stands after its deploy (local reads). */
  postdeployStatus(projectDir: string): Promise<PostDeployStatus>;
  /** HUP-S6.6 — the deploy tx's receipt on 40204; null while pending. */
  postdeployReceipt(txHash: string): Promise<DeployReceiptView | null>;
  /** HUP-S6.6 — submit the project contract's source to CitrateScan's verifier. */
  postdeployVerify(projectDir: string, address: string, constructorArgsHex?: string): Promise<VerifyOutcomeView>;
  /** HUP-S6.6 — point the page at chain 40204 and the deployed contract. */
  postdeploySwitchSite(projectDir: string, address: string): Promise<{ envPath: string; address: string }>;
  /** HUP-S6.6 — pin the built page to the app's IPFS daemon. */
  postdeployPinSite(projectDir: string): Promise<SitePinView>;
  /** HUP-S6.6 — write the Vercel-ready export folder (no account actions). */
  postdeployVercelExport(projectDir: string): Promise<VercelExportView>;
}

/** HUP-S6.7 — what CitrateScan says about an address. Mirrors Rust `contract_reader::VerifiedSource`. */
export interface ContractSourceView {
  status: "verified" | "partial" | "unverified" | "notContract";
  isContract: boolean;
  codeSize: number | null;
  contractName: string | null;
  compilerVersion: string | null;
  abi: unknown[] | null;
  source: string | null;
  note: string | null;
}

/** HUP-S6.6 — mirrors Rust `postdeploy::PostDeployStatus`. */
export interface PostDeployStatus {
  contractName: string;
  siteContract: string | null;
  built: boolean;
  exportDir: string | null;
}

/** HUP-S6.6 — mirrors Rust `postdeploy::DeployReceipt`. */
export interface DeployReceiptView {
  txHash: string;
  blockNumber: number;
  status: number | null;
  contractAddress: string | null;
}

/** HUP-S6.6 — mirrors Rust `postdeploy::VerifyOutcome`. */
export interface VerifyOutcomeView {
  status: "verified" | "partial" | "failed" | "unavailable";
  guid: string | null;
  matchType: string | null;
  contractName: string | null;
  message: string;
}

/** HUP-S6.6 — mirrors Rust `postdeploy::SitePin`. */
export interface SitePinView {
  cid: string;
  files: number;
  bytes: number;
  localGatewayUrl: string;
  publicGatewayUrl: string;
  note: string;
}

/** HUP-S6.6 — mirrors Rust `postdeploy::VercelExport`. */
export interface VercelExportView {
  dir: string;
  files: number;
  commands: string[];
}

/** A scrubbed diagnostic bundle (Telemetry WP-T.2/T.3). Mirrors the Rust DiagnosticBundle. */
export interface DiagnosticBundle {
  reportId: string;
  appVersion: string;
  os: string;
  crashTail: string;
  nodeLogTail: string;
  uiErrors: string[];
}

export interface TelemetryDomain {
  /** WP-T.2/T.3 — assemble the SCRUBBED bundle locally for review. No network. `uiErrors` is
   *  the frontend error ring. Returns already-scrubbed content the member can inspect. */
  bundle(uiErrors: string[]): Promise<DiagnosticBundle>;
  /** WP-T.4 — the ONE pinned HTTPS POST. Call ONLY after the member reviewed + consented
   *  (ConsentGate, WP-T.1). Sends exactly the reviewed bundle JSON. Honest error if the
   *  ingest service (WP-T.5) isn't live. */
  send(bundleJson: string): Promise<void>;
}

// ---- HUP-S1.6 — hardware tier (02_ARCHITECTURE §3, US-1.6). Mirrors Rust `tier.rs`. ----

export type TierId = "T0" | "T1" | "T2";

/** What this machine reported. `null` = could not be read — shown as unknown, never guessed. */
export interface HardwareFacts {
  os: string;
  arch: string;
  totalRamBytes: number | null;
  /** true on Apple Silicon (the GPU shares system memory); null = not known either way. */
  unifiedMemory: boolean | null;
  /** Largest dedicated GPU memory (NVIDIA only today); null = unknown / none. */
  gpuVramBytes: number | null;
  diskFreeBytes: number | null;
}

export interface TierProfile {
  tier: TierId;
  /** Display-only model family for the tier (S1.7 finalizes the picks). */
  modelHint: string;
  /** Normalized filename fragments identifying a matching model file. */
  modelMatch: string[];
  ctxTokens: number;
}

export interface TierRecommendation extends TierProfile {
  rationale: string[];
  /** T0: the guided / escalate tier. */
  guided: boolean;
  usableBytes: number | null;
}

export interface TierReport {
  facts: HardwareFacts;
  recommendation: TierRecommendation;
  /** The persisted user choice, or null when the recommendation applies. */
  overrideTier: TierId | null;
  /** The tier in effect (override ?? recommendation). */
  effective: TierId;
  profiles: TierProfile[];
}

export interface TierDomain {
  /** Probe this machine locally (no network) and recommend a tier. null = no hardware read is
   *  possible here (web preview) — never a fabricated machine. */
  recommend(): Promise<TierReport | null>;
  /** Persist the user's tier choice (null clears it). Stores only the tier id. */
  setOverride(tier: TierId | null): Promise<TierId | null>;
}

// ---- HUP-S1.5 — the escalation router (US-1.5). Mirrors Rust `escalation.rs`. ----
// Amounts are integer micro-USD. Prices are member-entered (per 1M tokens) and unverified.

export interface EscalationEndpointInput {
  label: string;
  baseUrl: string;
  model: string;
  inputMicrosPerMtok: number;
  outputMicrosPerMtok: number;
}

/** A member endpoint. Never carries the key (it is sealed in the OS keyring by core). */
export interface EscalationEndpoint extends EscalationEndpointInput {
  id: string;
  /** "label · host" — what every price card names. */
  destination: string;
}

export type EscalationMode = "budget" | "confirmed";

export interface EscalationSpendRecord {
  escalationId: string;
  endpointId: string;
  destination: string;
  quotedMicros: number;
  chargedMicros: number;
  mode: EscalationMode;
  outcome: "answered" | "not_sent" | "failed" | "unknown";
  usageReported: boolean;
  exceededQuote: boolean;
  atMs: number;
}

export interface EscalationBudget {
  capMicros: number;
  usedMicros: number;
  remainingMicros: number;
  /** Member-confirmed (HIC-1) spend today, outside the cap. */
  confirmedMicros: number;
  periodStartMs: number;
  periodEndMs: number;
  maxCapMicros: number;
  /** The ledger file could not be read: every escalation asks until the cap is set again. */
  unreadable: boolean;
  history: EscalationSpendRecord[];
}

/** The price card shown before an escalation runs. */
export interface EscalationQuote {
  quoteId: string;
  endpointId: string;
  destination: string;
  model: string;
  costMicros: number;
  costLabel: string;
  withinBudget: boolean;
  remainingMicros: number;
  capMicros: number;
  maxTokens: number;
  promptBytes: number;
  expiresMs: number;
}

export interface EscalationRun {
  escalationId: string;
  content: string;
  destination: string;
  mode: EscalationMode;
  chargedMicros: number;
  chargedLabel: string;
  usageReported: boolean;
  exceededQuote: boolean;
  remainingMicros: number;
}

export interface EscalationRegistryStatus {
  enabled: boolean;
  reason: string;
  missing: string[];
}

export interface EscalationDomain {
  endpoints(): Promise<EscalationEndpoint[]>;
  /** The key goes to core once and is sealed in the OS keyring; it is never returned. */
  addEndpoint(input: EscalationEndpointInput, apiKey: string): Promise<EscalationEndpoint>;
  removeEndpoint(id: string): Promise<void>;
  budget(): Promise<EscalationBudget>;
  setBudget(capMicros: number): Promise<EscalationBudget>;
  quote(endpointId: string, prompt: string, system?: string | null, maxTokens?: number | null): Promise<EscalationQuote>;
  /** Runs a quote the member was shown. `shownCostMicros` must equal the quote's price. */
  run(quoteId: string, shownCostMicros: number, confirmed: boolean, tainted: boolean): Promise<EscalationRun>;
  registryStatus(): Promise<EscalationRegistryStatus>;
}

// ---- HUP-S5.5 / S6.1 — signed first-run components. Mirrors Rust `components.rs`. ----

export interface ComponentsInstalled {
  name: string;
  version: string;
  previous: string | null;
  installedAt: number;
}

export interface ComponentsBundleTool {
  name: string;
  version: string;
  license: string;
  /** Platforms with a measured hash. */
  measuredPlatforms: string[];
  /** This machine's entry: measured | to_be_measured | to_be_built | upstream_unavailable | none. */
  thisPlatform: string;
}

export interface ComponentsStatus {
  /** false until the component signing key is set at the key ceremony; updates refuse until then. */
  keyConfigured: boolean;
  keyFingerprint: string | null;
  keyNote: string;
  platform: string | null;
  freshness: "never_checked" | "fresh" | "stale" | "expired";
  manifestAgeSecs: number | null;
  browserMayOpenWeb: boolean;
  installed: ComponentsInstalled[];
  bundle: ComponentsBundleTool[];
  libraries: { name: string; sha256: string }[];
  sla: {
    criticalHours: number;
    highDays: number;
    mediumDays: number;
    lowDays: number;
    staleAfterDays: number;
    pendingOwnerSignoff: boolean;
  };
  manifestUrl: string;
}

export interface ComponentsDomain {
  /** Read-only. null = no component store here (web preview), never an invented one. */
  status(): Promise<ComponentsStatus | null>;
  /** Verify-then-swap update of one component. Refuses while the key is not configured. */
  update(name: string): Promise<string>;
  rollback(name: string): Promise<string>;
}

export interface CxBridge {
  modelsCatalog: ModelsCatalogDomain;
  tier: TierDomain;
  escalation: EscalationDomain;
  telemetry: TelemetryDomain;
  storage: StorageDomain;
  groups: GroupsDomain;
  cluster: ClusterDomain;
  training: TrainingDomain;
  flRounds: FlRoundsDomain;
  agentHarness: AgentHarnessDomain;
  agentSkills: AgentSkillsDomain;
  contracts: ContractsDomain;
  social: SocialDomain;
  invites: InvitesDomain;
  components: ComponentsDomain;
}
