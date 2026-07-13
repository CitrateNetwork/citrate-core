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
import type { AppConfig, KeyringStatus, CustodyStatus, SlotInfo, AuthStatus } from "../types";
import type { BridgeContract } from "../domains";
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
    wallet: {
      async balances() {
        return unavailable("wallet", "balances");
      },
      async activity() {
        return unavailable("wallet", "activity");
      },
    },
    node: {
      async status() {
        return unavailable("node", "status");
      },
      async start() {
        return unavailable("node", "start");
      },
      async stop() {
        return unavailable("node", "stop");
      },
    },
    memory: {
      async assert() {
        return unavailable("memory", "assert");
      },
      async recall() {
        return unavailable("memory", "recall");
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
