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
  PendingWithdrawal,
  AiProviderStatus,
  GrantStatus,
  ModelStatus,
  NodeLogLine,
} from "../domains";
import { Unavailable } from "../types";

function unavailable(domain: string, op: string): never {
  throw new Unavailable(domain, op);
}

export function createTauriBridge(): Omit<BridgeContract, "mode"> {
  return {
    // ---- shell: open an external federation link (https only) in the browser ----
    shell: {
      async openExternal(url: string): Promise<void> {
        await invoke("open_external", { url });
      },
    },
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
      // CORE — REAL balances: native liquid SALT via eth_getBalance + the real
      // claimable (ContributionAccounting) + the real self-stake
      // (LiquidStakingPool.balanceOf) on 40204. `staked` here is the user's OWN
      // pool position; the vaulted membership grant (32k) is tracked separately in
      // AppState (hasGrant), so the UI adds it on top. Every field is a live read
      // (Rule 1) — no `-1` sentinel now that the pool balance is grounded.
      async balances(): Promise<{ liquid: number; staked: number; claimable: number; address: string }> {
        const b = await invoke<{ liquidWei: string; claimableWei: string; stakedWei: string; address: string }>("wallet_balances");
        const toSalt = (wei: string) => Number(BigInt(wei)) / 1e18;
        return { liquid: toSalt(b.liquidWei), staked: toSalt(b.stakedWei), claimable: toSalt(b.claimableWei), address: b.address };
      },
      // CORE item 4 — REAL indexed 40204 tx history from the CitrateScan `txlist`
      // endpoint (public, no-auth read via the Rust `wallet_activity` command;
      // data source: citrate-explorer /api/v1?module=account&action=txlist). Each
      // row is a real indexed tx (hash/kind/amount/ts + status/direction); a fresh
      // address / not-provisioned index honestly returns [] (never fabricated,
      // Rule 1). We surface the Activity shape the state model renders.
      async activity(): Promise<{ id: string; kind: string; amount: string; hash: string; ts: number; status: number | null; direction: string }[]> {
        const rows = await invoke<{ id: string; kind: string; amount: string; hash: string; ts: number; status: number | null; direction: string }[]>(
          "wallet_activity",
        );
        // Q-E.2 (C-5) — pass the receipt `status` (1 ok / 0 reverted / null pending)
        // and classified `direction` THROUGH the boundary. They used to be stripped
        // here, so a reverted tx rendered identically to a success; the Activity
        // table now marks a failed/pending tx from these real fields (Rule 1 — no
        // fabricated status, absent → honest "pending").
        return rows.map((r) => ({ id: r.id, kind: r.kind, amount: r.amount, hash: r.hash, ts: r.ts, status: r.status, direction: r.direction }));
      },
      // CORE (@rule8) — build a native SALT transfer as a PENDING ceremony and
      // return its decoded view. Signs NOTHING; the human approves via
      // signing.broadcast (B1.4 → real 40204 tx). `amountWei` (camelCase) maps to
      // the Rust `amount_wei` arg. Mirrors agent.claim's request→broadcast split.
      async send(to: string, amountWei: string): Promise<CeremonyView> {
        return invoke<CeremonyView>("wallet_send", { to, amountWei });
      },
      // CORE (@rule8) — build a LiquidStakingPool deposit() stake as a PENDING
      // ceremony and return its decoded view. Signs NOTHING; the human approves
      // via signing.broadcast (B1.4 → real 40204 deposit tx). Mirrors send().
      async stake(amountWei: string): Promise<CeremonyView> {
        return invoke<CeremonyView>("wallet_stake", { amountWei });
      },
      // CORE WP2 (@rule8) — build a requestWithdrawal(shares) ceremony. The
      // SALT→shares conversion is done in Rust from live reads; `amountWei` is the
      // SALT to withdraw. Signs NOTHING; approved via signing.broadcast. Mirrors
      // stake().
      async requestWithdrawal(amountWei: string): Promise<CeremonyView> {
        return invoke<CeremonyView>("wallet_request_withdrawal", { amountWei });
      },
      // CORE WP2 (@rule8) — build a claimWithdrawal(id) ceremony for a matured
      // request. The 50,400-block delay is enforced on-chain. Signs NOTHING.
      async claimWithdrawal(id: string): Promise<CeremonyView> {
        return invoke<CeremonyView>("wallet_claim_withdrawal", { requestId: id });
      },
      // CORE WP2 — the wallet's real pending withdrawals from live chain state
      // (getLogs + withdrawals(id) + block_number). A fresh wallet returns [].
      async pendingWithdrawals(): Promise<PendingWithdrawal[]> {
        return invoke<PendingWithdrawal[]>("wallet_pending_withdrawals");
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
      // Q-A.2/Q-B.2 — the REAL recent node log lines, streamed from the
      // supervised node's stdout+stderr ring (Rust `node_logs`). Live node
      // output in the packaged app; a stopped node returns [] honestly. No
      // fabricated template ever crosses this boundary (Rule 1).
      async logs() {
        return invoke<NodeLogLine[]>("node_logs");
      },
    },
    // ---- model: the local Gemma download + verify + llama-server sidecar (BC-3) ----
    // status/download/verify drive the real BC-3.1 Rust commands (streamed +
    // resumable download, SHA-256 verify); serveStart spawns the BC-3.2
    // llama-server sidecar (fails closed unless the model is verified-Ready + the
    // binary is bundled). No secret ever crosses this boundary.
    model: {
      async status(): Promise<ModelStatus> {
        return invoke<ModelStatus>("model_status");
      },
      async download(): Promise<void> {
        await invoke("model_download");
      },
      async verify(): Promise<void> {
        await invoke("model_verify");
      },
      async serveStart(): Promise<void> {
        await invoke("model_serve_start");
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
    // ---- chat: REAL OpenAI-compatible inference, key sealed in Rust (AI1) ----
    // @rule8: setProvider seals {baseURL,model,apiKey} in the OS keyring;
    // providerStatus returns metadata ONLY (never the key); infer reads the STORED
    // baseURL for the given id and POSTs /v1/chat/completions from Rust — the
    // webview picks WHICH provider, never the URL (exfil-binding). backend stays
    // Unavailable (the demo/real selection is done in the store, not here).
    chat: {
      async backend() {
        return unavailable("chat", "backend");
      },
      async setProvider(providerId: string, baseURL: string, model: string, apiKey: string): Promise<void> {
        // camelCase JS keys map to the Rust snake_case args (base_url/api_key).
        await invoke("ai_set_provider", { providerId, baseUrl: baseURL, model, apiKey });
      },
      async providerStatus(): Promise<AiProviderStatus[]> {
        return invoke<AiProviderStatus[]>("ai_provider_status");
      },
      async clearProvider(providerId: string): Promise<void> {
        await invoke("ai_clear_provider", { providerId });
      },
      async infer(providerId: string, messagesJson: string, contextJson: string): Promise<string> {
        return invoke<string>("ai_chat", { providerId, messagesJson, contextJson });
      },
      // W3.3 — one agentic turn (tools attached; returns the assistant message
      // JSON with any tool_calls). The webview runs the loop; Rust holds the key.
      async inferTools(
        providerId: string,
        messagesJson: string,
        toolsJson: string,
        contextJson: string,
      ): Promise<string> {
        return invoke<string>("ai_chat_tools", { providerId, messagesJson, toolsJson, contextJson });
      },
      // BC-3.2 — REAL LOCAL inference. The webview supplies ONLY messages +
      // context; Rust derives the loopback endpoint from the serve manager's
      // port (the webview can never supply a URL/host — exfil-binding).
      async inferLocal(messagesJson: string, contextJson: string): Promise<string> {
        return invoke<string>("ai_chat_local", { messagesJson, contextJson });
      },
      // BC-3.2 — the honest inference-routing state, computed in Rust from the
      // real model status + serve health + the gateway-key presence.
      async inferenceState(gatewayConfigured: boolean): Promise<string> {
        return invoke<string>("model_inference_state", { gatewayConfigured });
      },
    },
    membership: {
      async entitlement() {
        return unavailable("membership", "entitlement");
      },
      // CORE-D3.C — open the REAL core-membership checkout popup. Resolves once
      // the popup is opened; the money + grant are server-side. The store then
      // polls auth.userinfo() until the entitlement lands (no fabricated settle).
      async checkout(): Promise<void> {
        await invoke("membership_checkout");
      },
      // BC-1.3 (@rule8) — read the member's REAL on-chain grant status from 40204
      // (MembershipStakeVault.attributedStake/attributedShares + CitrateMemberSBT
      // .balanceOf). A PURE READ; signs nothing. S5 settles the grant leg ONLY from
      // this real read (never a fabricated settlement — Rule 1).
      async grantStatus(memberAddress: string): Promise<GrantStatus> {
        return invoke<GrantStatus>("membership_grant_status", { memberAddress });
      },
      // BC-5.3 — read the AUTHORITATIVE wholly-on-chain SBT emblem (isSubBound ->
      // tokenIdForSub(keccak256(sub)) -> tokenURI on CitrateMemberSBT/40204),
      // returning the decoded `data:image/svg+xml;base64,...` image data-URI, or
      // null honestly when the member has no SBT (Rule 1 — no fabricated art). A
      // PURE READ; signs nothing.
      async sbtArt(sub: string): Promise<string | null> {
        return invoke<string | null>("sbt_token_uri", { sub });
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
