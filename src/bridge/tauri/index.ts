// =====================================================================
// citrate-core — TAURI ADAPTER · CORE-A1 · A1.3 + A1.4
//
// Invokes the Rust commands registered in src-tauri. The `config` domain is
// wired end-to-end for real (A1.4): it round-trips through a genuine on-disk
// Tauri store plus an OS-keyring status read. Every OTHER domain throws
// `Unavailable` (Rule 1) — in the real desktop app an unwired domain says so,
// it never shows sim data dressed as live. Each later phase replaces one of
// these throws with a real invoke, copying the config shape exactly.
// =====================================================================
import { invoke } from "@tauri-apps/api/core";
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
} from "../types";
import type {
  BridgeContract,
  ClaimResult,
  MemoryStatus,
  MemoryResult,
  MemoryNeighbor,
} from "../domains";
import { Unavailable } from "../types";

function unavailable(domain: string, op: string): never {
  throw new Unavailable(domain, op);
}

export function createTauriBridge(): Omit<BridgeContract, "mode"> {
  return {
    // ---- config: REAL round-trip through Rust (A1.4) ----
    config: {
      async read(): Promise<AppConfig> {
        return invoke<AppConfig>("config_read");
      },
      async write(patch: Partial<AppConfig>): Promise<AppConfig> {
        return invoke<AppConfig>("config_write", { patch });
      },
      async keyringStatus(): Promise<KeyringStatus> {
        return invoke<KeyringStatus>("config_keyring_status");
      },
    },

    // ---- custody: REAL invoke of the A2 vault commands ----
    // NOTE: there is deliberately no `get` here — no invoke command returns
    // secret bytes (ADV-8). Reading a secret is an in-process Rust API.
    custody: {
      async status(): Promise<CustodyStatus> {
        return invoke<CustodyStatus>("custody_status");
      },
      async init(passphrase: string): Promise<void> {
        await invoke("custody_init", { passphrase });
      },
      async unlock(passphrase: string): Promise<void> {
        await invoke("custody_unlock", { passphrase });
      },
      async lock(): Promise<void> {
        await invoke("custody_lock");
      },
      async listSlots(): Promise<SlotInfo[]> {
        return invoke<SlotInfo[]>("custody_list");
      },
    },

    // ---- auth: REAL OIDC loopback-PKCE (A3) ----
    // Every command returns claim-derived AuthStatus (or void) — no token ever
    // crosses this boundary (ADV-8). The refresh token lives in the A2 vault;
    // the access token stays in Rust memory.
    auth: {
      async status(): Promise<AuthStatus> {
        return invoke<AuthStatus>("auth_status");
      },
      async login(): Promise<AuthStatus> {
        return invoke<AuthStatus>("auth_login");
      },
      async userinfo(): Promise<AuthStatus> {
        return invoke<AuthStatus>("auth_userinfo");
      },
      async refresh(): Promise<AuthStatus> {
        return invoke<AuthStatus>("auth_refresh");
      },
      async logout(): Promise<void> {
        await invoke("auth_logout");
      },
      async kycStart(): Promise<void> {
        await invoke("kyc_start");
      },
    },
    // ---- signing: REAL invoke of the B1.2 SignatureCeremony ----
    // request → a decoded PENDING view (NO signature); approve → the signature
    // hex ONLY (never key/seed/entropy — I-2); reject → consumed, no signature.
    // The gated Rust signer is reachable only via sign_approve.
    signing: {
      async request(intent: SignatureIntent): Promise<CeremonyView> {
        return invoke<CeremonyView>("sign_request", { intent });
      },
      async approve(id: string, rawAck: boolean): Promise<Signature> {
        // Tauri maps snake_case Rust args from camelCase JS keys; `raw_ack`
        // arrives as `rawAck`.
        return invoke<Signature>("sign_approve", { id, rawAck });
      },
      async broadcast(id: string, rawAck: boolean): Promise<BroadcastResult> {
        // CORE-B1.4 — sign the real EIP-155 tx with the vault key + broadcast to
        // 40204, returning the real tx hash + block. Never returns key material.
        return invoke<BroadcastResult>("sign_and_broadcast", { id, rawAck });
      },
      async reject(id: string): Promise<void> {
        await invoke("sign_reject", { id });
      },
    },
    wallet: {
      async balances() {
        return unavailable("wallet", "balances");
      },
      async activity() {
        return unavailable("wallet", "activity");
      },
    },
    // ---- node: REAL citrate-node under the SidecarSupervisor (C1.1) ----
    // status returns the node's REAL sync state (height/peers from its local
    // JSON-RPC, state from the supervisor); start spawns the node with an
    // encrypted data dir (@rule8 keyring storage key); stop releases the
    // supervisor (no orphan). No secret ever crosses this boundary.
    node: {
      async status(): Promise<{ state: string; peers: number; height: number; syncPct: number }> {
        return invoke("node_status");
      },
      async start() {
        await invoke("node_start");
      },
      async stop() {
        await invoke("node_stop");
      },
    },
    // ---- agent: the node-agent under the SidecarSupervisor (C1.2) ----
    // status returns the supervisor state + whether a bearer session exists
    // (never the token); start spawns the daemon with a per-session OsRng bearer
    // handed via a 0600 file; stop releases the supervisor + wipes the bearer.
    // Signature requests route through the ceremony (origin "agent:node-agent");
    // the node-agent holds no keys. No secret ever crosses this boundary.
    agent: {
      async status(): Promise<{ state: string; authed: boolean }> {
        return invoke("agent_status");
      },
      async start() {
        await invoke("agent_start");
      },
      async stop() {
        await invoke("agent_stop");
      },
      // CORE-C2 — the REAL claimable via ContributionAccounting.claimable(addr)
      // eth_call on 40204. Returns the single real claimable (wei) + its data
      // source; NO fabricated per-source breakdown (Rule 1). Requires the vault
      // unlocked (to read the wallet's public address; the key is never touched).
      async earnings(): Promise<{ claimableWei: string; walletAddress: string; contract: string }> {
        return invoke("agent_earnings");
      },
      // CORE-C2-F-1 (@rule8) — the REAL claim. Read the on-chain claimable; if 0,
      // return an honest "nothing to claim" (no ceremony, no tx). Otherwise invoke
      // `user_claim`, which bridges the real claimRewards() intent into a PENDING
      // ceremony (returned here). The human then approves it via signing.broadcast
      // (B1.4 → a real 40204 tx). No local balance mutation, no faked hash (Rule 1).
      async claim(): Promise<ClaimResult> {
        const snap = await invoke<{ claimableWei: string; walletAddress: string; contract: string }>(
          "agent_earnings",
        );
        if (BigInt(snap.claimableWei) === 0n) {
          return { kind: "nothing", claimableWei: snap.claimableWei };
        }
        const view = await invoke<CeremonyView>("user_claim");
        return { kind: "ceremony", view };
      },
    },
    // ---- memory: REAL mcp_serve daemon over its Unix socket (C3) ----
    memory: {
      async status() {
        return invoke<MemoryStatus>("memory_status");
      },
      async start() {
        await invoke("memory_start");
      },
      async stop() {
        await invoke("memory_stop");
      },
      // `assert` (a signed WRITE) still routes through the SignatureCeremony in a
      // later WP — honest Unavailable until then, never a fabricated "approved".
      async assert() {
        return unavailable("memory", "assert");
      },
      async recall(tenant: string, budget?: number) {
        return invoke<MemoryResult>("memory_recall", { tenant, budget });
      },
      async search(tenant: string, query: string, budget?: number) {
        return invoke<MemoryResult>("memory_search", { tenant, query, budget });
      },
      async neighbors(tenant: string, idPrefix: string, budget?: number) {
        return invoke<MemoryNeighbor[]>("memory_neighbors", { tenant, idPrefix, budget });
      },
      async constellation(budget?: number) {
        return invoke<MemoryResult[]>("memory_constellation", { budget });
      },
    },
    chat: {
      async backend() {
        return unavailable("chat", "backend");
      },
    },
    membership: {
      async entitlement() {
        return unavailable("membership", "entitlement");
      },
    },
    commissary: {
      async catalog() {
        return unavailable("commissary", "catalog");
      },
    },
    comms: {
      async connections() {
        return unavailable("comms", "connections");
      },
    },
  };
}
