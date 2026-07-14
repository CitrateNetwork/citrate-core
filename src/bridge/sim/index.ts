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
import { PERSONAS } from "../../shell/state";
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
  DecodedAction,
} from "../types";
import type { BridgeContract } from "../domains";
import { SIGNED_OUT_AUTH, UNRECOGNIZED_ACTION } from "../types";
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

  // Sim SignatureCeremony: STATE MACHINE ONLY. The web shim holds NO real key
  // and cannot sign — the real ceremony (vault-gated signer) lives entirely in
  // the Tauri/Rust path (B1.2). This models request→approve→reject (single-use
  // consumption, the raw-ack gate, explicit-id binding) so the dev UI behaves
  // identically, and returns a CLEARLY-FAKE, honestly-labeled sim signature
  // (never dressed as a real one). Guarded out of packaged builds.
  const simCeremonies = new Map<string, CeremonyView>();
  let simCeremonyId = 1;

  const simDecode = (intent: SignatureIntent): { decoded: DecodedAction; rawAck: boolean } => {
    // Mirror the Rust decode intent-by-intent so the dev UI shows the same shape.
    if (intent.kind === "personal_sign") {
      let text: string | null = null;
      try {
        const hex = intent.raw.startsWith("0x") ? intent.raw.slice(2) : intent.raw;
        const bytes = hex.match(/.{1,2}/g)?.map((h) => parseInt(h, 16)) ?? [];
        text = new TextDecoder("utf-8", { fatal: true }).decode(new Uint8Array(bytes));
      } catch {
        text = null;
      }
      if (text !== null) {
        return {
          decoded: { action: `Sign message: "${text.slice(0, 120)}"`, cost: "no funds moved", destination: intent.origin },
          rawAck: false,
        };
      }
    }
    // typed_data / transaction / undecodable → Unrecognized (raw-ack gated),
    // matching the Rust default for shapes the shim does not decode.
    return { decoded: { action: UNRECOGNIZED_ACTION, cost: "", destination: "" }, rawAck: true };
  };

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

    // ---- auth: SIM persona flow (dev shim). Derives the claim-derived
    // AuthStatus from the live prototype persona/AppState so web-dev renders the
    // Account/entitlement UI 1:1. Guarded out of packaged builds; the real OIDC
    // flow lives in the Tauri/Rust path (A3). No token exists here to leak.
    auth: {
      async status(): Promise<AuthStatus> {
        assertSimAllowed("auth.status");
        return simAuthStatus(s());
      },
      async login(): Promise<AuthStatus> {
        assertSimAllowed("auth.login");
        // The prototype "signs in" by advancing its own onboarding state; the
        // shim just reflects the current persona claim.
        return simAuthStatus(s());
      },
      async userinfo(): Promise<AuthStatus> {
        assertSimAllowed("auth.userinfo");
        return simAuthStatus(s());
      },
      async refresh(): Promise<AuthStatus> {
        assertSimAllowed("auth.refresh");
        return simAuthStatus(s());
      },
      async logout(): Promise<void> {
        assertSimAllowed("auth.logout");
        // No real token in the shim; the Store resets prototype state elsewhere.
      },
      async kycStart(): Promise<void> {
        assertSimAllowed("auth.kycStart");
        // The prototype drives KYC via its own onboarding timers.
      },
    },

    // ---- signing: SIM STATE MACHINE ONLY (no real key, no real signature) ----
    signing: {
      async request(intent: SignatureIntent): Promise<CeremonyView> {
        assertSimAllowed("signing.request");
        const { decoded, rawAck } = simDecode(intent);
        const id = String(simCeremonyId++);
        const view: CeremonyView = {
          id,
          origin: intent.origin, // TRUE origin, verbatim (anti-spoof), same as Rust
          kind: intent.kind,
          chainId: intent.chainId,
          decoded,
          requiresRawAck: rawAck,
        };
        simCeremonies.set(id, view);
        return view;
      },
      async approve(id: string, rawAck: boolean): Promise<Signature> {
        assertSimAllowed("signing.approve");
        const view = simCeremonies.get(id);
        if (!view) throw new Error("ceremony: unknown or already-consumed id");
        // Consume-first (single-use) + raw-ack gate, mirroring the Rust core.
        if (view.requiresRawAck && !rawAck) {
          throw new Error("ceremony: undecodable calldata requires an explicit raw-mode ack");
        }
        simCeremonies.delete(id);
        // HONEST: the web shim cannot produce a real signature (no key). Return a
        // clearly-labeled sim value; it is never presented as a live signature.
        return { sigHex: "sim-unsigned-no-real-key", kind: view.kind };
      },
      async broadcast(id: string, rawAck: boolean): Promise<BroadcastResult> {
        assertSimAllowed("signing.broadcast");
        // HONEST (Rule 1): the web shim holds NO key and reaches NO chain — it
        // cannot sign or broadcast a real 40204 tx. Consume the ceremony (mirror
        // the single-use/raw-ack state machine) then THROW rather than fabricate
        // a tx hash. The real signer+broadcast lives entirely in the Tauri/Rust
        // path (B1.4). Guarded out of packaged builds.
        const view = simCeremonies.get(id);
        if (!view) throw new Error("ceremony: unknown or already-consumed id");
        if (view.requiresRawAck && !rawAck) {
          throw new Error("ceremony: undecodable calldata requires an explicit raw-mode ack");
        }
        simCeremonies.delete(id);
        throw new Error(
          "signing.broadcast: the web shim cannot sign or broadcast a real 40204 transaction (no key, no chain). Use the packaged Tauri app.",
        );
      },
      async reject(id: string): Promise<void> {
        assertSimAllowed("signing.reject");
        if (!simCeremonies.delete(id)) throw new Error("ceremony: unknown or already-consumed id");
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

    agent: {
      async status() {
        assertSimAllowed("agent.status");
        // Sim mirrors the node's lifecycle; no real bearer session in sim.
        return { state: s().node, authed: false };
      },
      async start() {
        assertSimAllowed("agent.start");
      },
      async stop() {
        assertSimAllowed("agent.stop");
      },
      async earnings() {
        // Sim-only: the prototype claimable (SALT) rendered as a wei string in the
        // SAME shape the real eth_call returns. Guarded out of packaged builds by
        // assertSimAllowed — a real build reads ContributionAccounting.claimable
        // (Rule 1). No per-source breakdown here either (the contract has none).
        assertSimAllowed("agent.earnings");
        const st = s();
        const wei = BigInt(Math.round(st.claimable * 1e18)).toString();
        return {
          claimableWei: wei,
          walletAddress: st.walletAddr,
          contract: "0xcdd2477387279c7d44a1053f44db5dac0fd8faef",
        };
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

/**
 * Derive the claim-derived AuthStatus from the prototype persona + AppState.
 * This mirrors the shape the real Rust `auth_status` returns, so the entitlement
 * engine + Account UI read one contract in both sim and Tauri. `signedIn` is
 * false until the prototype has completed S1 sign-in (stage past s1), so the
 * onboarding S1 gate behaves the same way it does against real login events.
 */
function simAuthStatus(st: AppState): AuthStatus {
  const p = PERSONAS[st.persona] || PERSONAS.p1;
  const signedIn = st.stage === "done" || ["s2", "s3", "s4", "s5", "s6"].includes(st.stage) || st.s1 === "done";
  if (!signedIn) return SIGNED_OUT_AUTH;
  const effTier = st.entitlement === "lapsed" ? "free" : st.tier;
  const kyc = st.hasSbt || st.s2 === "verified" ? "verified" : st.s2 === "none" ? "none" : st.s2;
  return {
    signedIn: true,
    sub: "usr_2af4c19e" + p.initials.toLowerCase(),
    tier: effTier,
    org: st.org,
    role: p.role,
    kycStatus: kyc,
    walletAddr: st.walletAddr,
    expiresAt: st.entitlement === "lapsed" ? "2026-06-28" : "2027-07-11",
    email: p.email,
  };
}
