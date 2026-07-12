// =====================================================================
// citrate-core — SIM ADAPTER (dev shim) · CORE-A1 · A1.2
//
// This is the DEV SHIM. It preserves the current prototype behavior 1:1 by
// DELEGATING to the existing imperative Store (the sim engine in
// src/shell/store.ts) rather than reimplementing it. Surfaces get their data
// through the bridge; the bridge (in sim mode) reads it straight off the live
// Store snapshot. The demo/persona panel walk therefore still works exactly.
//
// GUARD: every op asserts `assertSimAllowed()` so this file cannot execute in a
// packaged Tauri build — the sim path is unreachable there (Rule 1).
// =====================================================================
import type { AppState } from "../../shell/state";
import type { AppConfig, KeyringStatus, CustodyStatus, SlotInfo } from "../types";
import type { BridgeContract } from "../domains";
import { assertSimAllowed } from "../mode";

// The sim adapter is bound to the running Store via a state getter + a
// setState-style patcher, injected at bridge assembly. This keeps the Store
// free of any bridge import (no cycle) while letting the sim delegate to it.
export interface SimHost {
  getState(): AppState;
  patch(u: Partial<AppState>): void;
}

export function createSimBridge(host: SimHost): Omit<BridgeContract, "mode"> {
  const s = () => host.getState();

  // Sim custody: UI STATE ONLY. This holds NO real secret and stores nothing —
  // it exists so the prototype's Keys-&-security section still renders a
  // plausible lock/unlock in web-dev. The real vault (envelope + keyring +
  // crypto) lives entirely in the Tauri/Rust path (A2). Guarded out of packaged
  // builds by `assertSimAllowed`.
  let simUnlocked = false;
  let simInitialized = true; // the prototype presents an already-provisioned vault

  return {
    // ---- config: in sim, config lives in AppState (localStorage-backed) ----
    config: {
      async read(): Promise<AppConfig> {
        assertSimAllowed("config.read");
        const st = s();
        return {
          net: st.net,
          rpc: st.rpc,
          dataDir: st.dataDir,
          cpuCap: st.cpuCap,
          autolock: st.autolock,
          channel: st.channel,
          telemetry: st.telemetry,
          sigPolicy: st.sigPolicy,
        };
      },
      async write(patch: Partial<AppConfig>): Promise<AppConfig> {
        assertSimAllowed("config.write");
        host.patch(patch as Partial<AppState>);
        return this.read();
      },
      async keyringStatus(): Promise<KeyringStatus> {
        assertSimAllowed("config.keyringStatus");
        // The web/dev shim has no OS keyring; be honest about that.
        return "unavailable";
      },
    },

    // ---- custody: SIM UI STATE ONLY (no real secret, stores nothing) ----
    custody: {
      async status(): Promise<CustodyStatus> {
        assertSimAllowed("custody.status");
        return {
          initialized: simInitialized,
          unlocked: simUnlocked,
          autolockMins: s().autolock,
          // No OS keyring on the web shim — honest, matches config.keyringStatus.
          keyringStatus: "unavailable",
        };
      },
      async init(_passphrase: string): Promise<void> {
        assertSimAllowed("custody.init");
        // Simulate provisioning; NO passphrase is derived or stored.
        simInitialized = true;
        simUnlocked = true;
      },
      async unlock(_passphrase: string): Promise<void> {
        assertSimAllowed("custody.unlock");
        // UI state only — the web shim performs no crypto and holds no secret.
        simUnlocked = true;
      },
      async lock(): Promise<void> {
        assertSimAllowed("custody.lock");
        simUnlocked = false;
      },
      async listSlots(): Promise<SlotInfo[]> {
        assertSimAllowed("custody.listSlots");
        // The prototype vault exposes no real slots; metadata only, no bytes.
        return [];
      },
    },

    // ---- the eight seam domains: delegate to the live Store snapshot ----
    auth: {
      async userinfo() {
        assertSimAllowed("auth.userinfo");
        const st = s();
        return { sub: "usr_2af4c19e", email: "", tier: st.tier, role: "member", org: st.org };
      },
      async signOut() {
        assertSimAllowed("auth.signOut");
      },
    },

    wallet: {
      async balances() {
        assertSimAllowed("wallet.balances");
        const st = s();
        return {
          liquid: st.liquid,
          staked: (st.hasGrant ? 32000 : 0) + st.selfStake,
          claimable: st.claimable,
          address: st.walletAddr,
        };
      },
      async activity() {
        assertSimAllowed("wallet.activity");
        return s().activity;
      },
    },

    node: {
      async status() {
        assertSimAllowed("node.status");
        const st = s();
        return { state: st.node, peers: st.peers, height: st.height, syncPct: st.syncPct };
      },
      async start() {
        assertSimAllowed("node.start");
      },
      async stop() {
        assertSimAllowed("node.stop");
      },
    },

    memory: {
      async assert() {
        assertSimAllowed("memory.assert");
        return "approved";
      },
      async recall() {
        assertSimAllowed("memory.recall");
        return "ok";
      },
    },

    chat: {
      async backend() {
        assertSimAllowed("chat.backend");
        const st = s();
        return { kind: st.chatBackend, label: "infer.citrate.ai · local-proxy" };
      },
    },

    membership: {
      async entitlement() {
        assertSimAllowed("membership.entitlement");
        const st = s();
        return { status: st.entitlement, tier: st.tier, expiresAt: "2027-07-11" };
      },
    },

    commissary: {
      async catalog() {
        assertSimAllowed("commissary.catalog");
        return [];
      },
    },

    comms: {
      async connections() {
        assertSimAllowed("comms.connections");
        return s().connections;
      },
    },
  };
}
