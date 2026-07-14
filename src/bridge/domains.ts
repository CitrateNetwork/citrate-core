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

export interface MemoryDomain {
  assert(fact: string): Promise<"approved" | "declined">;
  recall(query: string): Promise<string>;
}

export interface ChatDomain {
  /** Which provider transport the harness routes to (gateway/local/demo). */
  backend(): Promise<{ kind: string; label: string }>;
}

export interface MembershipDomain {
  entitlement(): Promise<{ status: "active" | "expiring" | "grace" | "lapsed"; tier: string; expiresAt: string }>;
}

export interface CommissaryDomain {
  catalog(): Promise<{ name: string; capabilities: string[] }[]>;
}

export interface CommsDomain {
  connections(): Promise<Record<string, boolean>>;
}

// ---- the full bridge surface ----------------------------------------
export interface BridgeContract {
  readonly mode: "sim" | "tauri";
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
