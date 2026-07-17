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
  activity(): Promise<{ id: string; kind: string; amount: string; hash: string; ts: number }[]>;
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

export interface NodeDomain {
  status(): Promise<{ state: string; peers: number; height: number; syncPct: number }>;
  start(): Promise<void>;
  stop(): Promise<void>;
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
}

export interface MembershipDomain {
  entitlement(): Promise<{ status: "active" | "expiring" | "grace" | "lapsed"; tier: string; expiresAt: string }>;
  /**
   * CORE-D3.C — open the REAL core-membership checkout in an in-app popup
   * (`{coreMembershipUrl}/checkout`). Resolves once the popup is OPENED — it does
   * NOT report payment success. The money + entitlement grant happen server-side
   * (core-membership → droplet); the caller then polls `auth.userinfo()` until the
   * entitlement goes active (the same "open a flow then poll userinfo" pattern as
   * `kycStart`/`pollKyc`). The Tauri impl invokes `membership_checkout`; the sim
   * impl is guarded out of packaged builds (web-dev keeps its fake settle).
   */
  checkout(): Promise<void>;
}

export interface CommissaryDomain {
  catalog(): Promise<{ name: string; capabilities: string[] }[]>;
}

export interface CommsDomain {
  connections(): Promise<Record<string, boolean>>;
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
  agent: AgentDomain;
  memory: MemoryDomain;
  chat: ChatDomain;
  membership: MembershipDomain;
  commissary: CommissaryDomain;
  comms: CommsDomain;
}
