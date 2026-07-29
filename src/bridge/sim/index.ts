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
import { PERSONAS, RANK } from "../../shell/state";
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
import type { BridgeContract, ClaimResult, MemoryResult, MemoryNeighbor, PendingWithdrawal, AiProviderStatus, GrantStatus, ModelStatus, NodeLogLine, ConnectionInfo } from "../domains";
import { SIGNED_OUT_AUTH, UNRECOGNIZED_ACTION, Unavailable } from "../types";
import { assertSimAllowed } from "../mode";
import { GRAPH, NODE_LOG_TEMPLATES } from "../../data/seed";

// The sim MemoryDomain maps the prototype seed GRAPH into the SAME contract the
// real C3 daemon returns. The seed uses tenant "chain-facts"; the REAL ingest
// tenant is "chain-state" (sprint Concern 2), so we alias here for parity.
const SIM_TENANT_ALIAS: Record<string, string> = { "chain-state": "chain-facts", "chain-facts": "chain-facts", personal: "personal" };

function simTenantResult(tenant: string): MemoryResult {
  const seedTenant = SIM_TENANT_ALIAS[tenant] ?? tenant;
  const nodes = GRAPH.nodes.filter((n) => n.tenant === seedTenant);
  return {
    tenant,
    totalInTenant: nodes.length,
    hits: nodes.map((n) => ({ id: n.id, kind: n.kind, title: n.label })),
  };
}

function simNeighbors(idPrefix: string): MemoryNeighbor[] {
  // Edges in the seed touching the node whose id starts with idPrefix.
  const byId: Record<string, (typeof GRAPH.nodes)[number]> = {};
  GRAPH.nodes.forEach((n) => (byId[n.id] = n));
  const out: MemoryNeighbor[] = [];
  for (const [a, b] of GRAPH.links) {
    if (a.startsWith(idPrefix) && byId[b]) out.push({ direction: "out", kind: "References", title: byId[b].label, proposed: false });
    else if (b.startsWith(idPrefix) && byId[a]) out.push({ direction: "in", kind: "References", title: byId[a].label, proposed: false });
  }
  return out;
}

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

  // Mint a sim ceremony from an intent (the ONE place ids + decode happen), reused
  // by `signing.request` AND the C2-F-1 `agent.claim` prototype path so both share
  // the single sim ceremony store. Guarded by the callers' assertSimAllowed.
  const simRequest = (intent: SignatureIntent): CeremonyView => {
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
  };

  // Q-E.1 — mint a DECODED wallet-action ceremony so the human-in-the-loop review
  // modal renders truthfully in web-dev (the same request→approve split the tauri
  // path uses). The view carries a legible action / destination / cost; approving
  // it routes through `signing.broadcast`, which HONESTLY throws "desktop only"
  // (the web shim holds no key + reaches no chain — no fabricated settlement,
  // Rule 1). We surface a REAL pending ceremony, not a fake settled tx.
  const simWalletCeremony = (decoded: DecodedAction): CeremonyView => {
    const id = String(simCeremonyId++);
    const view: CeremonyView = {
      id,
      origin: "local-user",
      kind: "transaction",
      chainId: 40204,
      decoded,
      requiresRawAck: false,
    };
    simCeremonies.set(id, view);
    return view;
  };
  const weiToSalt = (wei: string): string => {
    try {
      const w = BigInt(wei);
      const whole = w / 10n ** 18n;
      const frac = (w % 10n ** 18n).toString().padStart(18, "0").slice(0, 4).replace(/0+$/, "");
      return frac ? `${whole}.${frac} SALT` : `${whole} SALT`;
    } catch {
      return `${wei} wei`;
    }
  };

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

  // BC-3 sim model animation state (web-dev PREVIEW ONLY — never real bytes/hash).
  // A packaged Tauri build runs the real BC-3.1/3.2 commands instead; this path is
  // unreachable there (assertSimAllowed guards every op).
  let simModelPct = 0;
  let simModelDownloaded = false;
  let simModelVerified = false;

  return {
    // ---- shell: in web-dev, open the link in a new browser tab ----
    shell: {
      async openExternal(url: string): Promise<void> {
        assertSimAllowed("shell.openExternal");
        if (typeof window !== "undefined" && /^https:\/\//.test(url)) {
          window.open(url, "_blank", "noopener,noreferrer");
        }
      },
    },
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
          coreMembershipUrl: st.coreMembershipUrl,
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
        return simRequest(intent);
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
        // `staked` = the SELF-stake only (the atomic chain fact the tauri bridge
        // reads via LiquidStakingPool.balanceOf). The vaulted membership grant is
        // an entitlement the store/UI layers on top (hasGrant ? 32000 : 0), so it
        // is NOT included here — keeping the field's meaning identical in sim and
        // tauri (self-stake), which the store folds into AppState.selfStake.
        return {
          liquid: st.liquid,
          staked: st.selfStake,
          claimable: st.claimable,
          address: st.walletAddr,
        };
      },
      async activity() {
        assertSimAllowed("wallet.activity");
        return s().activity;
      },
      // Q-E.1 — build a DECODED pending review view so the human-in-the-loop modal
      // renders truthfully in web-dev (mirroring the tauri request→approve split).
      // Signs NOTHING; approving routes through signing.broadcast, which HONESTLY
      // throws "desktop only" (no key, no chain — no fabricated transfer, Rule 1).
      async send(to: string, amountWei: string): Promise<CeremonyView> {
        assertSimAllowed("wallet.send");
        return simWalletCeremony({ action: `Send ${weiToSalt(amountWei)}`, cost: "gas settles in the desktop app", destination: to });
      },
      async stake(amountWei: string): Promise<CeremonyView> {
        assertSimAllowed("wallet.stake");
        return simWalletCeremony({ action: `Stake ${weiToSalt(amountWei)} (LiquidStakingPool deposit)`, cost: "gas settles in the desktop app", destination: "LiquidStakingPool" });
      },
      async requestWithdrawal(amountWei: string): Promise<CeremonyView> {
        assertSimAllowed("wallet.requestWithdrawal");
        return simWalletCeremony({ action: `Request withdrawal of ${weiToSalt(amountWei)}`, cost: "gas settles in the desktop app", destination: "LiquidStakingPool" });
      },
      async claimWithdrawal(id: string): Promise<CeremonyView> {
        assertSimAllowed("wallet.claimWithdrawal");
        return simWalletCeremony({ action: `Claim matured withdrawal #${id}`, cost: "gas settles in the desktop app", destination: "LiquidStakingPool" });
      },
      async ensureReady(): Promise<{ address: string; created: boolean }> {
        // No custody vault or key exists in web-dev, so nothing is provisioned or
        // minted. Return the deterministic persona address — a NON-CHAIN preview
        // identity, not a real EOA — and `created:false` (nothing was minted). The
        // real init+unlock+mint happens only in the desktop app (Rule 1).
        assertSimAllowed("wallet.ensureReady");
        return { address: s().walletAddr, created: false };
      },
      async linkRequest(): Promise<CeremonyView> {
        // There is no custody key and no authority session in web-dev, so there
        // is nothing to prove control of. Refuse honestly rather than mint a
        // ceremony that could never produce a real proof (Rule 1).
        assertSimAllowed("wallet.linkRequest");
        throw new Error("Wallet linking runs in the desktop app (it signs with the custody key).");
      },
      async linkApprove(): Promise<{ address: string; linked: boolean }> {
        assertSimAllowed("wallet.linkApprove");
        throw new Error("Wallet linking runs in the desktop app (it signs with the custody key).");
      },
      async linkReject(): Promise<void> {
        assertSimAllowed("wallet.linkReject");
      },
      async pendingWithdrawals(): Promise<PendingWithdrawal[]> {
        // The web shim reaches no chain — there is no real pending queue to read.
        // Return an honest empty list (Rule 1) rather than fabricate rows; the
        // tauri path reads the real on-chain queue.
        assertSimAllowed("wallet.pendingWithdrawals");
        return [];
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
      // W1.3 — no real node/proposer key in the web preview → honest Unavailable
      // (never a fabricated registration ceremony).
      async registerValidator() {
        assertSimAllowed("node.registerValidator");
        throw new Unavailable("node", "registerValidator");
      },
      // No kubo daemon in the web preview — honest no-op (nothing spawned).
      async startIpfs() {
        assertSimAllowed("node.startIpfs");
      },
      // Q-A.2/Q-B.2 — SIM/web PREVIEW ONLY. There is NO real node process in the
      // web preview, so this cannot stream real stdout/stderr. It returns a small
      // window of the design-preview template lines (clearly a preview, never
      // dressed as live output) filled with the current sim height/peers, so the
      // Node LOG panel still renders in web-dev. Guarded by assertSimAllowed — it
      // can NEVER run in a packaged Tauri build, where the real node_logs runs
      // (Rule 1). An off/prov node has no stream → honest empty.
      async logs(): Promise<NodeLogLine[]> {
        assertSimAllowed("node.logs");
        const st = s();
        const running = st.node !== "off" && st.node !== "prov";
        if (!running) return [];
        const now = Date.now();
        return NODE_LOG_TEMPLATES.slice(0, 8).map((tpl, i) => ({
          ts: now - (8 - i) * 600,
          stream: "out" as const,
          line:
            "[sim preview] " +
            tpl
              .replace(/\{h\}/g, String(st.height))
              .replace(/\{peers\}/g, String(st.peers))
              .replace(/\{r\}/g, String((st.height / 50) | 0)),
        }));
      },
    },

    // ---- model: the local Gemma download + verify (BC-3), SIM PREVIEW ONLY ----
    // HONEST (Rule 1): the web preview cannot download or verify a real 5 GB GGUF
    // and reaches no llama-server. So this animates a CLEARLY-preview progress in
    // web-dev only (guarded by assertSimAllowed — it can NEVER run in a packaged
    // Tauri build, where the real BC-3.1/3.2 commands run). It never claims a real
    // "verified" model: `ready` is reached only AFTER the sim's own verify() step,
    // mirroring the real no-Ready-without-verify guard, and the whole path is
    // unreachable in the packaged app.
    model: {
      async status(): Promise<ModelStatus> {
        assertSimAllowed("model.status");
        // The sim total mirrors the real pinned size (5,335,289,824 bytes) so the
        // preview shows the honest number; simModelPct/simModelVerified are local
        // web-dev animation state (never real bytes/hash).
        const total = 5_335_289_824;
        if (simModelVerified) return { state: "ready" };
        if (simModelDownloaded) return { state: "verifying" };
        if (simModelPct <= 0) return { state: "notPresent" };
        return { state: "downloading", downloadedBytes: Math.round((simModelPct / 100) * total), totalBytes: total, pct: simModelPct };
      },
      async download(): Promise<void> {
        assertSimAllowed("model.download");
        // Web-dev animation ONLY: advance a fake progress toward complete. This is
        // a design preview, never a real 5 GB pull (which only the Tauri app does).
        simModelPct = Math.min(100, simModelPct + 25 + Math.random() * 20);
        if (simModelPct >= 100) simModelDownloaded = true;
      },
      async verify(): Promise<void> {
        assertSimAllowed("model.verify");
        // Only a completed (sim) download can be verified; ready is reached ONLY
        // after this verify step — the no-Ready-without-verify shape, in preview.
        if (!simModelDownloaded) throw new Error("model: nothing downloaded to verify");
        simModelVerified = true;
      },
      async serveStart(): Promise<void> {
        assertSimAllowed("model.serveStart");
        // No llama-server in the web preview — the real spawn is Tauri-only. This
        // resolves so the sim flow proceeds; it never claims a running local model.
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
      // CORE-C2-F-1 — the Claim button in web-dev. HONEST (Rule 1): the sim holds
      // NO key and reaches NO chain, so it can NEVER settle a real claim. If the
      // (prototype) claimable is 0 it returns "nothing to claim"; otherwise it mints
      // a SIM ceremony view so the prototype ceremony UI can render — but approving
      // it routes through sim `signing.broadcast`, which THROWS rather than fabricate
      // a settlement. There is no local balance mutation and no faked "claimed" hash.
      // Guarded out of packaged builds by assertSimAllowed.
      async claim(): Promise<ClaimResult> {
        assertSimAllowed("agent.claim");
        const st = s();
        const wei = BigInt(Math.round(st.claimable * 1e18));
        if (wei === 0n) {
          return { kind: "nothing", claimableWei: wei.toString() };
        }
        // Mint a SIM ceremony (via the shared simRequest) so the prototype ceremony
        // UI can render. Approving it in sim routes through signing.broadcast, which
        // THROWS honestly (no key, no chain) — never a fabricated settlement.
        const view = simRequest({
          origin: "agent:node-agent",
          kind: "transaction",
          chainId: 40204,
          // A legible claimRewards() selector-only calldata tx object, so the sim
          // ceremony decodes to a Call (not raw-gated) exactly like the real path.
          raw: JSON.stringify({
            to: "0xcdd2477387279c7d44a1053f44db5dac0fd8faef",
            value: "0x0",
            data: "0x372500ab",
            chainId: "0x9d0c",
          }),
        });
        return { kind: "ceremony", view };
      },
    },

    // The sim MemoryDomain drives the prototype seed GRAPH (design preview only;
    // guarded out of packaged builds by assertSimAllowed). It shapes the seed
    // into the SAME MemoryResult/MemoryNeighbor contract the real C3 daemon
    // returns, so the Storage surface renders identically in sim and tauri.
    memory: {
      async status() {
        assertSimAllowed("memory.status");
        return { state: "running", socketPath: s().socketPath, semantic: false };
      },
      async start() {
        assertSimAllowed("memory.start");
      },
      async stop() {
        assertSimAllowed("memory.stop");
      },
      async assert() {
        assertSimAllowed("memory.assert");
        return "approved";
      },
      async recall(tenant: string) {
        assertSimAllowed("memory.recall");
        return simTenantResult(tenant);
      },
      async search(tenant: string, query: string) {
        assertSimAllowed("memory.search");
        const r = simTenantResult(tenant);
        const q = query.toLowerCase();
        return { ...r, hits: r.hits.filter((h) => (h.title + " " + h.kind).toLowerCase().includes(q)) };
      },
      async neighbors(_tenant: string, idPrefix: string) {
        assertSimAllowed("memory.neighbors");
        return simNeighbors(idPrefix);
      },
      async constellation() {
        assertSimAllowed("memory.constellation");
        return [simTenantResult("personal"), simTenantResult("chain-state")];
      },
      // W3.2 — no mem daemon in the web preview: honest no-op (nothing ingested),
      // never a fabricated count.
      async ingestDocs() {
        assertSimAllowed("memory.ingestDocs");
        return { docs: 0, chunks: 0, skipped: "not-running" };
      },
    },

    // CORE-AI1 — the web preview has NO OS keyring and reaches no provider, so
    // real inference cannot happen here (Rule 1). providerStatus honestly reports
    // nothing configured; setProvider/clearProvider/infer throw Unavailable; the
    // store then falls back to the built-in demo agent. Guarded out of packaged
    // builds by assertSimAllowed.
    chat: {
      async backend() {
        assertSimAllowed("chat.backend");
        // Honest (Rule 1): web-dev chat runs on the built-in demo agent, not a
        // gateway — the label must not claim a remote proxy it does not call.
        return { kind: "demo", label: "built-in demo agent" };
      },
      async setProvider() {
        assertSimAllowed("chat.setProvider");
        // No OS keyring on the web shim — a provider key cannot be sealed here.
        throw new Unavailable("chat", "setProvider");
      },
      async providerStatus(): Promise<AiProviderStatus[]> {
        assertSimAllowed("chat.providerStatus");
        // The web shim has no keyring — nothing is configured (never fabricated).
        return [];
      },
      async clearProvider() {
        assertSimAllowed("chat.clearProvider");
        throw new Unavailable("chat", "clearProvider");
      },
      async infer() {
        assertSimAllowed("chat.infer");
        // No key, no provider reachable from the web preview — honest Unavailable
        // (the store uses the built-in demo agent instead of a fabricated reply).
        throw new Unavailable("chat", "infer");
      },
      async inferTools() {
        assertSimAllowed("chat.inferTools");
        // No gateway in the web preview — honest Unavailable (the store uses the
        // built-in demo agent, never a fabricated agentic reply).
        throw new Unavailable("chat", "inferTools");
      },
      async inferLocal() {
        assertSimAllowed("chat.inferLocal");
        // No llama-server in the web preview — honest Unavailable (the store falls
        // through to the built-in demo agent, never a fabricated local reply).
        throw new Unavailable("chat", "inferLocal");
      },
      async inferenceState(): Promise<string> {
        assertSimAllowed("chat.inferenceState");
        // The web preview has no local model + no keyring → the honest demo route.
        return "demo";
      },
    },

    membership: {
      async entitlement() {
        assertSimAllowed("membership.entitlement");
        const st = s();
        return { status: st.entitlement, tier: st.tier, expiresAt: "2027-07-11" };
      },
      // CORE-D3.C — SIM: no real popup + no real checkout. The web-dev onboarding
      // keeps its OWN fake settle in the Store (onS3Pay's sim branch). This just
      // resolves so the sim S3 flow proceeds; guarded out of packaged builds.
      async checkout(): Promise<void> {
        assertSimAllowed("membership.checkout");
      },
      // BC-1.3 — SIM: there is NO real chain in the web preview, so this NEVER
      // fabricates a real 40204 read. It derives an HONEST stand-in from the
      // persona/AppState: a granted status (32,000-SALT attributedStake + SBT) ONLY
      // when the sim persona is a PAID member (a paid tier with an active
      // entitlement); an unpaid/lapsed persona reads 0 stake / no SBT (so the sim S5
      // still settles from grantStatus, not a blind timer — the Rule-1 shape).
      async grantStatus(): Promise<GrantStatus> {
        assertSimAllowed("membership.grantStatus");
        const st = s();
        const rank = RANK[st.tier] ?? 0;
        const paid = rank > RANK.free && st.entitlement === "active";
        // 32,000 SALT in wei — the validator stake requirement a granted member meets.
        const REQUIREMENT_WEI = (32000n * 10n ** 18n).toString();
        return paid
          ? { attributedStakeWei: REQUIREMENT_WEI, attributedSharesWei: REQUIREMENT_WEI, hasSbt: true, bondedStakeWei: "0", hasValidator: false, nativeBalanceWei: REQUIREMENT_WEI }
          : { attributedStakeWei: "0", attributedSharesWei: "0", hasSbt: false, bondedStakeWei: "0", hasValidator: false, nativeBalanceWei: "0" };
      },
      // BC-5.3 — SIM: there is NO real chain in the web preview, so this NEVER
      // fabricates an on-chain emblem. It returns null so the caller renders the
      // LOCAL deterministic emblem (sbtArt.ts) as a labelled offline fallback — the
      // honest "local preview, not the on-chain art" state (Rule 1).
      async sbtArt(): Promise<string | null> {
        assertSimAllowed("membership.sbtArt");
        return null;
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

    // ---- connections: SIM is honestly disconnected (no backend, no token flow).
    // status returns all-disconnected (true, not fabricated); start is desktop-only.
    connections: {
      async status(): Promise<ConnectionInfo[]> {
        return ["github", "gdrive", "notion"].map((service) => ({
          service,
          connected: false,
          scope: null,
          connectedAt: null,
        }));
      },
      async start(): Promise<ConnectionInfo> {
        throw new Unavailable("connections", "start");
      },
      async disconnect(): Promise<void> {
        // Nothing is stored in the preview — a no-op is the honest result.
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
