// =====================================================================
// citrate-core — imperative store
// Ports the DCLogic class methods from design/CitrateCore.dc.html:
// the 600ms sim tick, ceremony queue + resolvers, chat send loop, journal
// operations, node lifecycle. React binds via useStore() (see below).
//
// Kept imperative (one mutable state object + subscriber notify) to match
// the prototype's setState-with-updater semantics exactly. A React reducer
// would fight the timer/resolver flow; this is the honest 1:1.
// =====================================================================
import { useSyncExternalStore } from "react";
import {
  AppState,
  CerSpec,
  ChatMsg,
  Persona,
  PERSONAS,
  PERSIST_KEYS,
  RANK,
  STORAGE_KEY,
  freshState,
  greeting,
  loadState,
  makeHash,
} from "./state";
import { NODE_LOG_TEMPLATES } from "../data/seed";
import { createDemoProvider, createRealProvider, ChatProvider, ToolCall } from "../agent/harness";
import { bindSimHost, bridge } from "../bridge";
import { BRIDGE_MODE } from "../bridge/mode";

type Updater = Partial<AppState> | ((s: AppState) => Partial<AppState>);

/**
 * A3-03 — is an entitlement `expiresAt` claim in the past (or anomalous)?
 * - ABSENT (null/undefined/empty) → NOT expired: the authority legitimately may
 *   not send an expiry; the hard id_token `exp` gate in Rust still bounds the
 *   session.
 * - PRESENT but UNPARSEABLE → treated as EXPIRED (fail-closed): a malformed
 *   expiry on a T1 gating surface is an anomaly, not a licence to keep the tier.
 * - PRESENT + in the past → expired.
 * Accepts an ISO date/datetime or a unix-seconds string.
 */
export function isExpiredClaim(expiresAt: string | null | undefined): boolean {
  if (!expiresAt) return false;
  const ms = /^\d+$/.test(expiresAt) ? Number(expiresAt) * 1000 : Date.parse(expiresAt);
  if (!Number.isFinite(ms)) return true; // fail-closed on an unparseable value
  return ms < Date.now();
}

/**
 * CORE-D3.C — has the S3 checkout entitlement actually landed?
 *
 * The onboarding "Pay" step advances ONLY when `/userinfo` shows the REAL grant
 * (Rule 1 — the tauri path never fakes a settled membership). "Landed" means the
 * folded claim shows a PAID tier (rank past `free`/public — the commercial.kyc
 * grant) AND the entitlement reads `active`. A still-`free`/`public` tier, or a
 * lapsed/grace entitlement, is NOT settled — the poll keeps waiting.
 *
 * This is the single decision point the poll uses, so it is unit-tested against
 * the exact folded AppState the entitlement engine produces.
 */
export function isPaidEntitlementActive(st: { tier: string; entitlement: string }): boolean {
  const rank = RANK[st.tier];
  // Unknown tiers are treated as unpaid (fail-closed): only a KNOWN paid tier
  // (rank > free) with an ACTIVE entitlement counts as the grant having landed.
  if (rank === undefined || rank <= RANK.free) return false;
  return st.entitlement === "active";
}

/**
 * Derive a display name + initials from an email local-part. The authority
 * issues NO display-name claim, so `larry@citrate.ai` → { name: "Larry",
 * initials: "LA" } and `ada.lovelace@x.io` → { name: "Ada Lovelace", initials:
 * "AL" }. Used by applyAuthStatus to render a real signed-in user's identity;
 * exported so the derivation is unit-tested independently of the auth plumbing.
 */
export function deriveIdentityFromEmail(email: string): { name: string; initials: string } {
  const local = email.split("@")[0];
  const words = local.split(/[._+-]+/).filter(Boolean);
  const name = words.map((w) => w.charAt(0).toUpperCase() + w.slice(1)).join(" ") || email;
  const fromWords = words.length > 1 ? words.map((w) => w.charAt(0)).join("") : local.slice(0, 2);
  const initials = (fromWords || local.slice(0, 2)).slice(0, 2).toUpperCase();
  return { name, initials };
}

/**
 * Map the REAL node supervisor state (bridge.node.status → node.rs `map_state`:
 * stopped/starting/running/restarting/failed) onto the app's node lifecycle
 * enum. A `running` node is `syncing` until fully synced, then `validating` when
 * its stake meets the threshold, else `synced`. This is the seam that replaces
 * the sim `tick()` node state in a Tauri build.
 */
export function mapNodeState(state: string, syncPct: number, staked: number): AppState["node"] {
  switch (state) {
    case "starting":
    case "restarting":
      return "prov";
    case "failed":
      return "error";
    case "running":
      if (syncPct < 100) return "syncing";
      return staked >= 32000 ? "validating" : "synced";
    case "stopped":
    default:
      return "off";
  }
}

/**
 * CORE-AI1 — the pure real-vs-demo provider SELECTION rule, extracted so it is
 * unit-tested independently of the store singleton + bridge. A REAL provider is
 * chosen ONLY when (a) we are in the Tauri build (an OS keyring exists) AND (b) the
 * current default id is actually CONFIGURED (its key is sealed). Otherwise chat
 * falls back to the honest built-in demo agent (Rule 1 — never a fabricated
 * provider, never a real provider in the keyring-less web preview).
 */
export function pickChatProviderKind(
  statuses: { id: string; configured: boolean }[],
  aiDefault: string,
  mode: "sim" | "tauri",
): "real" | "demo" {
  if (mode !== "tauri") return "demo";
  return statuses.some((p) => p.id === aiDefault && p.configured) ? "real" : "demo";
}

export class Store {
  state: AppState;
  private subs = new Set<() => void>();
  private snap: AppState;
  private cid = 0;
  private mid = 0;
  private resolvers: Record<string, (v: string) => void> = {};
  private timer: ReturnType<typeof setInterval> | null = null;
  private nodeTimer: ReturnType<typeof setInterval> | null = null;
  private nodeStarting = false;
  private _saveT: ReturnType<typeof setTimeout> | null = null;
  private _toastT: ReturnType<typeof setTimeout> | null = null;
  private _s1t: ReturnType<typeof setTimeout> | null = null;
  provider: ChatProvider | null = null;
  // element refs (imperative, like the design)
  chatScrollEl: HTMLElement | null = null;
  chatInputEl: HTMLInputElement | null = null;
  jChatScrollEl: HTMLElement | null = null;
  jChatInputEl: HTMLInputElement | null = null;

  constructor() {
    this.state = loadState();
    this.snap = this.state;
    this.provider = createDemoProvider(() => this.snapshot());
    // Bind the sim adapter to this Store so the bridge (in sim mode) reads and
    // writes the live prototype state — the 1:1 UI is preserved (CORE-A1 A1.2).
    bindSimHost({
      getState: () => this.state,
      patch: (u) => this.setState(u),
    });
  }

  // ---- React binding ----
  subscribe = (cb: () => void): (() => void) => {
    this.subs.add(cb);
    return () => this.subs.delete(cb);
  };
  getSnapshot = (): AppState => this.snap;

  setState(u: Updater): void {
    const patch = typeof u === "function" ? u(this.state) : u;
    this.state = { ...this.state, ...patch };
    this.snap = this.state;
    this.subs.forEach((cb) => cb());
  }

  start(): void {
    if (this.timer) return;
    this.timer = setInterval(() => this.tick(), 600);
    // CORE-A2 — pull the real custody lock state into the Keys-&-security
    // section. In a Tauri build this reads the live vault (custody_status); in
    // web-dev it is the sim shim. Failures leave the field "unknown" (honest).
    void this.refreshCustody();
    // CORE-A3 — fold the live entitlement claim into the engine on launch. In a
    // Tauri build this reads the real /userinfo-derived status (silent if signed
    // out); in web-dev it reads the sim persona. Honest no-op on failure.
    void this.refreshAuth();
    // CORE-AI1 — select the chat provider: a REAL OpenAI-compatible provider if
    // the default id is configured (key sealed in the OS keyring), else the honest
    // built-in demo agent. Web-dev has no keyring, so this always resolves to demo.
    void this.rebuildProvider();
    // CORE (Phase 1) — in a Tauri build, poll the REAL node vitals (height/peers/
    // syncPct/state) from the supervisor + local RPC every 2s and fold them into
    // AppState, so the Node surface + Sidebar render live numbers instead of the
    // sim tick(). No-op in web-dev (the sim tick drives those there).
    if (BRIDGE_MODE === "tauri") {
      void this.refreshNode();
      this.nodeTimer = setInterval(() => void this.refreshNode(), 2000);
      // Real wallet balances (native liquid + claimable) — folded once on launch;
      // the Wallet surface also refreshes on mount + after a settled ceremony.
      void this.refreshWallet();
      // WP2 — the real pending-withdrawal queue (chain-sourced), folded on launch;
      // the Wallet surface also refreshes on mount + after a settle.
      void this.refreshPendingWithdrawals();
    }
  }

  /**
   * Refresh the runtime custody lock state from the bridge. Reads real vault
   * status in a Tauri build; the sim shim in web-dev. Never throws — an
   * unavailable/failed read is reported honestly as `"unknown"`.
   */
  async refreshCustody(): Promise<void> {
    try {
      const st = await bridge.custody.status();
      this.setState({
        custodyLock: st.unlocked ? "unlocked" : "locked",
        // config.autolock is the single source of truth; keep UI in sync.
        autolock: st.autolockMins,
      });
    } catch {
      this.setState({ custodyLock: "unknown" });
    }
  }

  /**
   * CORE-AI1 (@rule8) — (re)select the chat provider from the live AI config.
   *
   * Reads `bridge.chat.providerStatus()` (Tauri: the OS-keyring-sealed provider
   * metadata, NEVER the key). If the current `aiDefault` id is CONFIGURED, chat
   * runs through the REAL provider — `createRealProvider` calls the Rust `ai_chat`
   * command, which reads the STORED baseURL for that id and POSTs from Rust (the
   * key never touches the webview; the webview never supplies the URL). Otherwise
   * chat falls back to the honest built-in demo agent (Rule 1 — never a fabricated
   * "gateway" reply). Web-dev has no keyring, so this always resolves to the demo.
   * Called on launch and after a provider config change (Settings), so the active
   * provider tracks the config. Never throws — a failed status read keeps the demo.
   */
  async rebuildProvider(): Promise<void> {
    try {
      const statuses = await bridge.chat.providerStatus();
      const def = this.state.aiDefault;
      if (pickChatProviderKind(statuses, def, BRIDGE_MODE) === "real") {
        this.provider = createRealProvider(def, () => this.snapshot(), (pid, msgs, ctx) =>
          bridge.chat.infer(pid, msgs, ctx),
        );
        return;
      }
      // No configured default (or web-dev): the honest built-in demo agent.
      this.provider = createDemoProvider(() => this.snapshot());
    } catch {
      // Honest no-op: providerStatus unavailable (web shim / failed read) — keep
      // the built-in demo agent rather than a fabricated provider.
      this.provider = createDemoProvider(() => this.snapshot());
    }
  }

  /**
   * CORE-AI1 (@rule8) — seal an AI provider's {baseURL, model, apiKey} in the OS
   * keyring via the bridge (Rust binds the key to its https baseURL). The key is
   * handed to Rust ONCE and never stored in AppState/localStorage (aiKeys is gone).
   * On success the id becomes the default route and the provider is rebuilt so chat
   * uses it immediately. Web-dev has no keyring — honest toast, no fake seal.
   */
  async aiSetProvider(providerId: string, baseURL: string, model: string, apiKey: string): Promise<void> {
    if (BRIDGE_MODE !== "tauri") {
      this.toast("Provider keys seal in the OS keyring — desktop app only (no keyring in web preview).");
      return;
    }
    try {
      await bridge.chat.setProvider(providerId, baseURL, model, apiKey);
      this.setState({ aiDefault: providerId, aiEdit: null });
      await this.rebuildProvider();
      this.toast("Provider key sealed in the OS keyring — chat now uses it.");
      this.save();
    } catch (err) {
      this.toast("Could not save the provider — " + String((err as Error).message ?? err));
    }
  }

  /**
   * CORE-AI1 (@rule8) — delete a provider's sealed config via the bridge, then
   * rebuild the provider (falls back to the demo agent if the default was cleared).
   */
  async aiClearProvider(providerId: string): Promise<void> {
    if (BRIDGE_MODE !== "tauri") {
      this.setState({ aiEdit: null });
      return;
    }
    try {
      await bridge.chat.clearProvider(providerId);
      await this.rebuildProvider();
      this.toast("Provider key removed from the keyring.");
      this.save();
    } catch (err) {
      this.toast("Could not remove the provider — " + String((err as Error).message ?? err));
    }
  }

  /** CORE-AI1 — set the default provider route + rebuild (no key involved). */
  async aiSetDefault(providerId: string): Promise<void> {
    this.setState({ aiDefault: providerId });
    await this.rebuildProvider();
    this.save();
  }

  /** Unlock the custody vault, then refresh lock state. */
  async custodyUnlock(passphrase: string): Promise<void> {
    await bridge.custody.unlock(passphrase);
    await this.refreshCustody();
  }

  /** Lock the custody vault, then refresh lock state. */
  async custodyLock(): Promise<void> {
    await bridge.custody.lock();
    await this.refreshCustody();
  }

  // ---------- CORE-A3 auth (real OIDC in Tauri; sim persona in web-dev) ----
  /**
   * Pull the live claim-derived auth status from the bridge and fold it into the
   * entitlement engine (tier/org/role/entitlement) + the KYC S2 state. In a
   * Tauri build this reads the real /userinfo entitlement claim; in web-dev it
   * reads the sim persona. Never throws — an unavailable/failed read leaves the
   * prior state untouched (honest).
   */
  async refreshAuth(): Promise<void> {
    try {
      const st = await bridge.auth.status();
      this.applyAuthStatus(st);
    } catch {
      /* honest no-op: the auth domain reported unavailable / not signed in */
    }
  }

  /**
   * CORE (Phase 1) — pull the REAL node vitals from the bridge and fold them into
   * AppState. `bridge.node.status()` (Tauri) hits the supervisor + local node RPC
   * (eth_blockNumber / net_peerCount) — height/peers are live 40204 truth, never
   * fabricated. A user-initiated `paused` is respected (not overwritten). Failure
   * leaves the last honest values untouched — no sim fallback (Rule 1).
   */
  async refreshNode(): Promise<void> {
    if (this.state.node === "paused") return; // respect an explicit pause
    try {
      const st = await bridge.node.status();
      const staked = (this.state.hasGrant ? 32000 : 0) + this.state.selfStake;
      const patch: Partial<AppState> = {
        height: st.height,
        peers: st.peers,
        syncPct: st.syncPct,
        // There is NO real finality/checkpoint-age source from the node yet
        // (WO-1 adds citrate_getDagStats). Mark it unavailable (< 0) rather than
        // leaving the fabricated seed value — displays render "—" (Rule 1).
        finAge: -1,
      };
      // While a start is in flight, a transient "stopped" poll (supervisor not
      // yet registered during spawn) must NOT demote the optimistic "prov" back
      // to "off" — that caused a prov→off→prov flicker. Still fold height/peers.
      if (!this.nodeStarting) patch.node = mapNodeState(st.state, st.syncPct, staked);
      this.setState(patch);
    } catch {
      /* honest no-op: a failed poll keeps the last real values, never a sim number */
    }
  }

  /**
   * CORE — pull REAL wallet balances (native `liquid` SALT via eth_getBalance +
   * the real `claimable` via ContributionAccounting + the real SELF-stake via
   * LiquidStakingPool.balanceOf) and fold them into AppState. `staked` from the
   * bridge is the self-stake only; the displayed staked = (grant ? 32000 : 0) +
   * selfStake, so we fold it into `selfStake`. Failure leaves the last honest
   * values untouched — no fabricated number (Rule 1).
   */
  async refreshWallet(): Promise<void> {
    try {
      const b = await bridge.wallet.balances();
      const patch: Partial<AppState> = { liquid: b.liquid };
      if (b.claimable >= 0) {
        patch.claimable = b.claimable;
        // Only claim a "chain" data source for a REAL read (Tauri). In web-dev the
        // sim bridge echoes AppState, so labeling it chain-sourced would be a
        // fabricated caption (Rule 1).
        if (BRIDGE_MODE === "tauri") patch.earnSource = "chain";
      }
      // b.staked >= 0 ⇒ a grounded self-stake read (balanceOf) — fold it. The UI
      // adds the vaulted grant on top (hasGrant ? 32000 : 0). A negative value
      // would mean "not grounded" — keep the local value (defensive; the real and
      // sim bridges both return >= 0 today).
      if (b.staked >= 0) patch.selfStake = b.staked;
      this.setState(patch);
    } catch {
      /* honest no-op */
    }
  }

  /** Fold a claim-derived AuthStatus into AppState (the entitlement engine). */
  private applyAuthStatus(st: {
    signedIn: boolean;
    sub?: string | null;
    email?: string | null;
    walletAddr?: string | null;
    tier: string | null;
    org: string | null;
    role: string | null;
    kycStatus: string | null;
    expiresAt?: string | null;
  }): void {
    if (!st.signedIn) return;
    const patch: Partial<AppState> = {};
    // Fold the REAL identity so no prototype persona ever surfaces to a signed-in
    // user (Rule 1). The authority issues no display-name claim, so derive a
    // display name + initials from the email local part.
    patch.signedIn = true;
    patch.authSub = st.sub ?? null;
    patch.authEmail = st.email ?? null;
    if (st.email) {
      const d = deriveIdentityFromEmail(st.email);
      patch.authName = d.name;
      patch.authInitials = d.initials;
    }
    if (st.walletAddr) patch.walletAddr = st.walletAddr;
    // A3-03: enforce the entitlement expiry at the DECISION point — a claim whose
    // `expiresAt` is in the past is downgraded to the free tier + lapsed, so no
    // gated surface stays unlocked on a stale claim. A valid future expiry keeps
    // the claimed tier and marks the entitlement active.
    if (isExpiredClaim(st.expiresAt)) {
      patch.tier = "free";
      patch.entitlement = "lapsed";
    } else if (st.tier) {
      patch.tier = st.tier;
      patch.entitlement = "active";
    } else {
      // Signed in but the authority granted NO tier — fail closed to free so an
      // ungranted session can never inherit a prior session's persisted paid
      // tier (tier/entitlement are persisted keys).
      patch.tier = "free";
      patch.entitlement = "lapsed";
    }
    patch.org = st.org;
    // The authority may omit the citrate_role claim for a plain member; a
    // signed-in paid account is a "member" by default (never the persona's role).
    patch.citrateRole = st.role || "member";
    // KYC claim → the S2 seam's five states (none/pending/verified/failed/review).
    if (st.kycStatus === "verified") patch.s2 = "verified";
    else if (st.kycStatus === "pending") patch.s2 = "pending";
    else if (st.kycStatus === "failed") patch.s2 = "failed";
    else if (st.kycStatus === "review") patch.s2 = "review";
    this.setState(patch);
  }

  /** Run the real loopback-PKCE sign-in (Tauri), then fold in the claim. */
  async authLogin(): Promise<void> {
    const st = await bridge.auth.login();
    this.applyAuthStatus(st);
    // The login result may carry only id_token claims (no email/wallet). Fold
    // the live /userinfo immediately so the real identity (email/wallet/tier)
    // is complete on the first render — never a persona fallback for a signed-in
    // user. Best-effort: keep the login-folded state if /userinfo is unavailable.
    try {
      await this.authUserinfo();
    } catch {
      /* honest no-op — the login-folded claim stands */
    }
    this.save();
  }

  /** Live /userinfo entitlement re-check, folded into the entitlement engine. */
  async authUserinfo(): Promise<void> {
    const st = await bridge.auth.userinfo();
    this.applyAuthStatus(st);
  }

  /** Sign out: revoke + clear the vault slot + wipe the session, then reset. */
  async authLogout(): Promise<void> {
    await bridge.auth.logout();
    this.setState({
      tier: "free",
      // Coherent free/no-membership state — matches the expired-claim path
      // (tier:free + entitlement:lapsed); leaving a prior "active" would render a
      // false membership badge on a signed-out/free account (Rule 1).
      entitlement: "lapsed",
      citrateRole: "member",
      org: null,
      signedIn: false,
      authSub: null,
      authEmail: null,
      authName: null,
      authInitials: null,
    });
    this.save();
  }

  /** Open KYC in the browser; S2 status then arrives via userinfo polling. */
  async kycStart(): Promise<void> {
    await bridge.auth.kycStart();
  }

  /**
   * Open an external federation link (Atlas docs/tutorials, CitrateScan, a service
   * webapp) in the system browser. `url` must be https. The destination RP runs
   * its own OIDC login today; a real signed-in handoff (shared-authority SSO) is
   * work-order WO-6. Honest toast on failure — never a silent dead click.
   */
  async openExternal(url: string): Promise<void> {
    try {
      await bridge.shell.openExternal(url);
    } catch (err) {
      this.toast("Could not open the link — " + String((err as Error).message ?? err));
    }
  }

  /**
   * Open the REAL core-membership checkout (Settings → Billing "Renew"). Uses the
   * same wired path as onboarding S3 (bridge.membership.checkout → the checkout
   * popup). Web-dev has no popup — honest message. Entitlement refresh then
   * arrives via the normal /userinfo poll after settlement.
   */
  async renewMembership(): Promise<void> {
    if (BRIDGE_MODE !== "tauri") {
      this.toast("Renewal opens the checkout in the desktop app.");
      return;
    }
    try {
      await bridge.membership.checkout();
    } catch (err) {
      this.toast("Could not open checkout — " + String((err as Error).message ?? err));
    }
  }
  stop(): void {
    if (this.timer) clearInterval(this.timer);
    this.timer = null;
    if (this.nodeTimer) clearInterval(this.nodeTimer);
    this.nodeTimer = null;
  }

  // ---------- helpers ----------
  persona(): Persona {
    return PERSONAS[this.state.persona] || PERSONAS.p1;
  }

  /**
   * The identity every surface renders — the REAL signed-in user when a live
   * session exists, else the sim persona (web-dev only). This is the single
   * seam that stops prototype identity (Dana Okafor et al.) from ever showing
   * to a real, signed-in account. Name/initials derive from the email local
   * part because the authority issues no display-name claim; the wallet, sub,
   * email, tier, role and org come straight from the folded /userinfo claims.
   */
  identity(): {
    name: string;
    initials: string;
    email: string;
    sub: string;
    wallet: string;
    tier: string;
    role: string;
    org: string | null;
    real: boolean;
  } {
    const s = this.state;
    if (s.signedIn) {
      // A signed-in user NEVER renders the sim persona (Rule 1) — even if
      // /userinfo has not folded the email yet or failed. Fall back to a neutral
      // real placeholder derived from the claims we do have, never Dana et al.
      const email = s.authEmail || "";
      return {
        name: s.authName || email || "Member",
        initials: s.authInitials || (email ? email.slice(0, 2).toUpperCase() : "M"),
        email: email || "—",
        sub: s.authSub || "—",
        wallet: s.walletAddr,
        tier: s.tier,
        role: s.citrateRole || "member",
        org: s.org,
        real: true,
      };
    }
    const P = this.persona();
    return {
      name: P.name,
      initials: P.initials,
      email: P.email,
      sub: "usr_2af4c19e…" + P.initials.toLowerCase(),
      wallet: s.walletAddr,
      tier: s.tier,
      role: s.citrateRole || P.role,
      org: s.org,
      real: false,
    };
  }

  save(): void {
    if (this._saveT) clearTimeout(this._saveT);
    this._saveT = setTimeout(() => {
      const s = this.state;
      const keep: Record<string, unknown> = {};
      for (const k of PERSIST_KEYS) keep[k] = s[k];
      try {
        localStorage.setItem(STORAGE_KEY, JSON.stringify(keep));
      } catch {
        /* ignore */
      }
    }, 400);
  }

  snapshot() {
    const s = this.state;
    return {
      height: s.height,
      peers: s.peers,
      finalityAge: Math.round(s.finAge),
      nodeState: nodeLabelLocal(s.node),
      staked: (s.hasGrant ? 32000 : 0) + s.selfStake,
      liquid: s.liquid,
      claimable: s.claimable,
      earningsToday: s.earnToday,
      walletAddr: s.walletAddr,
      tier: s.tier,
    };
  }

  toast(text: string): void {
    if (this._toastT) clearTimeout(this._toastT);
    this.setState({ toast: text });
    this._toastT = setTimeout(() => this.setState({ toast: null }), 3000);
  }
  copy(text: string, note?: string): void {
    try {
      if (navigator.clipboard) navigator.clipboard.writeText(text);
    } catch {
      /* ignore */
    }
    this.toast(note || "Copied");
  }
  go(route: string): void {
    this.setState({ route });
    try {
      if ((location.hash || "").replace(/^#\//, "") !== route) location.hash = "#/" + route;
    } catch {
      /* ignore */
    }
    this.save();
  }

  // ---------- sim tick (verbatim logic) ----------
  tick(): void {
    this.setState((s) => {
      const u: Partial<AppState> = {};
      const running = s.node !== "off" && s.node !== "prov";
      // NODE VITALS SIMULATION — web-dev ONLY. In a Tauri build height/peers/
      // syncPct/node-state come from the REAL node via `refreshNode`
      // (bridge.node.status → node.rs → local RPC eth_blockNumber/net_peerCount),
      // and cpu/ram/logs/peer-rows are honestly blank rather than fabricated
      // (Rule 1). This whole block is the source of the implausible always-
      // climbing height + steady ~24 peers a packaged build must never show.
      if (BRIDGE_MODE === "sim") {
        u.height = s.height + (Math.random() < 0.85 ? 1 : 2);
        u.finAge = s.finAge + 0.6;
        if (u.height - s.lastCp >= 50) {
          u.lastCp = u.height;
          u.finAge = 0;
        }
        const target = running ? 24 : 0;
        let peers = s.peers;
        if (peers < target) peers += Math.ceil(Math.random() * 3);
        else if (peers > target) peers -= Math.ceil(Math.random() * 4);
        else if (running && Math.random() < 0.15) peers += Math.random() < 0.5 ? 1 : -1;
        u.peers = Math.max(0, Math.min(32, peers));
        if (s.node === "syncing") {
          const np = Math.min(100, s.syncPct + 5 + Math.random() * 9);
          u.syncPct = np;
          if (np >= 100) {
            u.node = ((s.hasGrant ? 32000 : 0) + s.selfStake) >= 32000 ? "validating" : "synced";
            if (s.stage === "s6") u.s6ready = true;
          }
        }
        if (s.node === "validating" && Math.random() < 0.018) u.blocksProposed = s.blocksProposed + 1;
        if (s.node === "validating") {
          const dv = 0.004 + Math.random() * 0.005,
            dp = 0.0009 + Math.random() * 0.0006,
            dc = 0.0004 + Math.random() * 0.0004;
          u.earnVal = s.earnVal + dv;
          u.earnPin = s.earnPin + dp;
          u.earnComp = s.earnComp + dc;
          u.earnToday = s.earnToday + dv + dp + dc;
          u.claimable = s.claimable + (dv + dp + dc) * 0.85;
        }
        if (running) {
          u.hb = (s.hb + 0.6) % 30;
          u.cpu = s.node === "paused" ? 2 + Math.random() * 2 : (s.node === "validating" ? 18 : 11) + Math.random() * 12;
          u.ram = (s.node === "validating" ? 780 : 540) + Math.random() * 140;
          if (Math.random() < 0.55) {
            const T = NODE_LOG_TEMPLATES;
            const line = T[(Math.random() * T.length) | 0]
              .replace(/\{h\}/g, String(u.height))
              .replace(/\{peers\}/g, String(u.peers))
              .replace(/\{r\}/g, String((u.height! / 50) | 0));
            const d = new Date();
            const t =
              String(d.getHours()).padStart(2, "0") + ":" + String(d.getMinutes()).padStart(2, "0") + ":" + String(d.getSeconds()).padStart(2, "0");
            u.logs = s.logs.concat([{ t, line, id: this.mid++ }]).slice(-14);
          }
          if (!s.peerRows.length || (u.height! % 60 === 0)) u.peerRows = this.makePeers(u.peers!);
        }
      }
      if (s.s5 === "settling") {
        u.s5n = Math.min(32000, s.s5n + 6800);
        if (u.s5n >= 32000 && s.s5n < 32000) {
          u.s5 = "settled";
          u.hasGrant = true;
          u.hasSbt = true;
        }
      }
      if (s.storageMode === "dl") {
        u.modelPct = Math.min(100, s.modelPct + 3 + Math.random() * 5);
        if (u.modelPct >= 100) u.storageMode = "semantic";
      }
      u.pollIn = s.pollIn <= 0.6 ? 20 : s.pollIn - 0.6;
      if (running && s.pins.length) {
        u.pins = s.pins.map((p) => {
          const nx = p.nextIn - 0.6;
          if (nx <= 0) return { ...p, nextIn: p.cadH * 3600, last: p.last === "pending" ? "pending" : "attested" };
          return { ...p, nextIn: nx };
        });
      }
      return u;
    });
  }

  makePeers(n: number) {
    const rows = [];
    const cities = ["fra1", "sgp1", "nyc3", "ams2", "tok1", "sfo2"];
    for (let i = 0; i < Math.min(5, Math.max(2, (n / 5) | 0)); i++) {
      rows.push({
        id: "16Uiu2HA" + Math.random().toString(36).slice(2, 8) + "…" + cities[i % cities.length],
        dir: Math.random() < 0.5 ? "in" : "out",
        lat: ((18 + Math.random() * 120) | 0) + " ms",
      });
    }
    return rows;
  }

  // ---------- ceremony ----------
  requestSig(spec: CerSpec): Promise<string> {
    return new Promise((resolve) => {
      const id = "cer" + ++this.cid;
      this.resolvers[id] = resolve;
      this.setState((s) => ({
        queue: s.queue.concat([{ id, ...spec }]),
        cerPhase: s.queue.length ? s.cerPhase : "review",
        cerStep: 0,
      }));
    });
  }
  finishCer(result: string): void {
    const s = this.state;
    const head = s.queue[0];
    if (!head) return;
    const res = this.resolvers[head.id!];
    delete this.resolvers[head.id!];
    this.setState({ queue: s.queue.slice(1), cerPhase: "review", cerStep: 0 });
    if (res) res(result);
  }
  approveCer(): void {
    const head = this.state.queue[0];
    if (!head) return;
    const chainless = head.chainless;
    this.setState({ cerPhase: "busy", cerStep: 0, cerHash: makeHash() });
    const step = (n: number, ms: number) => setTimeout(() => this.setState({ cerStep: n }), ms);
    step(1, 650);
    step(2, 1350);
    setTimeout(() => this.setState({ cerPhase: "done" }), chainless ? 1200 : 2050);
    setTimeout(
      () => {
        if (head.apply) head.apply(this.state.cerHash);
        this.finishCer("approved");
        this.save();
      },
      chainless ? 2300 : 3300,
    );
  }
  addActivity(kind: string, amount: string, hash?: string): void {
    this.setState((s) => ({
      activity: [{ id: "a" + Date.now(), kind, amount, hash: hash || makeHash(), ts: Date.now() }].concat(s.activity).slice(0, 24),
      justSigned: kind,
    }));
    setTimeout(() => this.setState({ justSigned: null }), 2200);
  }

  // ---------- chat ----------
  async sendChat(text: string): Promise<void> {
    text = (text || "").trim();
    if (!text || this.state.chatStatus !== "ready" || !this.provider) return;
    const userMsg: ChatMsg = { id: "m" + ++this.mid, who: "You", text, chips: [], streaming: false };
    this.setState((s) => ({ chatMsgs: s.chatMsgs.concat([userMsg]), chatStatus: "thinking" }));
    if (this.chatInputEl) this.chatInputEl.value = "";
    const asstId = "m" + ++this.mid;
    let started = false;
    const ensure = () => {
      if (started) return;
      started = true;
      this.setState((s) => ({ chatMsgs: s.chatMsgs.concat([{ id: asstId, who: "Agent", text: "", chips: [], streaming: true }]) }));
    };
    const patch = (fn: (m: ChatMsg) => ChatMsg) =>
      this.setState((s) => ({ chatMsgs: s.chatMsgs.map((m) => (m.id === asstId ? fn(m) : m)) }));
    try {
      await this.provider.send({
        messages: this.state.chatMsgs
          .filter((m) => !m.streaming)
          .map((m) => ({ role: m.who === "You" ? "user" : "assistant", content: m.text }))
          .concat([{ role: "user", content: text }]),
        callbacks: {
          onStatus: (st) => {
            if (st === "streaming") ensure();
            this.setState({ chatStatus: st === "done" || st === "error" ? "ready" : (st as AppState["chatStatus"]) });
          },
          onToken: (tk) => {
            ensure();
            patch((m) => ({ ...m, text: m.text + tk.replace(/\*\*/g, "") }));
            this.scrollChat();
          },
          onToolCall: (call) => this.handleTool(call, asstId, ensure),
        },
      });
    } catch (e) {
      console.error(e);
    }
    patch((m) => ({ ...m, streaming: false }));
    this.setState({ chatStatus: "ready" });
    this.scrollChat();
    this.save();
  }
  scrollChat(): void {
    requestAnimationFrame(() => {
      if (this.chatScrollEl) this.chatScrollEl.scrollTop = this.chatScrollEl.scrollHeight;
      if (this.jChatScrollEl) this.jChatScrollEl.scrollTop = this.jChatScrollEl.scrollHeight;
    });
  }

  async handleTool(call: ToolCall, asstId: string, ensure: () => void): Promise<string> {
    let args: Record<string, string> = {};
    try {
      args = JSON.parse(call.arguments || "{}");
    } catch {
      /* ignore */
    }
    let status = "ok";
    let result = "ok";
    if (call.name === "memory_assert") {
      const r = await this.requestSig({
        origin: "chat agent",
        requester: "dashboard agent · tool memory_assert",
        title: "Write to your memory graph",
        chainless: true,
        rows: [
          { k: "Assertion", v: "“" + (args.fact || "") + "”" },
          { k: "Tenant", v: "personal · your capability grant" },
          { k: "Store", v: this.state.socketPath },
        ],
        cost: "none — local memory write",
        sponsor: "no chain transaction",
        sponsorColor: "var(--tx-3)",
      });
      status = r;
      result = r;
    } else if (call.name === "journal_append") {
      const entry = args.entry || "work note";
      const today = new Date().toISOString().slice(0, 10);
      const r = await this.requestSig({
        origin: "chat agent",
        requester: "dashboard agent · tool journal_append",
        title: "Write to your journal",
        chainless: true,
        rows: [
          { k: "Entry", v: "“" + entry + "”" },
          { k: "Page", v: today + " · daily note" },
          { k: "Store", v: "local journal — off-chain, encrypted at rest" },
        ],
        cost: "none — local journal write",
        sponsor: "no chain transaction",
        sponsorColor: "var(--tx-3)",
      });
      if (r === "approved") {
        this.setState((st) => {
          const pages = st.jPages.slice();
          let pg = pages.find((p) => p.id === "d-" + today);
          if (!pg) {
            pg = { id: "d-" + today, title: today, kind: "daily", pinned: false, blocks: [] };
            pages.unshift(pg);
          }
          const upd = { ...pg, blocks: pg.blocks.concat(["@agent " + entry]) };
          return { jPages: pages.map((p) => (p.id === upd.id ? upd : p)) };
        });
        this.save();
      }
      status = r;
      result = r;
    } else if (call.name === "app_navigate") {
      const route = ["wallet", "node", "storage", "comms", "commissary", "settings", "dashboard"].indexOf(args.route) >= 0 ? args.route : "dashboard";
      this.go(route);
      result = "ok";
    } else if (call.name === "chain_read") {
      result = JSON.stringify(this.snapshot());
    } else if (call.name === "memory_recall" || call.name === "docs_link") {
      result = "ok";
    }
    const label =
      call.name.replace("_", ".") +
      (args.query ? " · " + args.query : "") +
      (status === "approved" ? " · approved" : status === "declined" ? " · declined" : " ✓");
    ensure();
    this.setState((s) => ({
      chatMsgs: s.chatMsgs.map((m) => (m.id === asstId ? { ...m, chips: m.chips.concat([{ label, status }]) } : m)),
    }));
    return result;
  }

  // ---------- journal capture ----------
  appendCapture(prefix: string): void {
    const txt = (this.jChatInputEl ? this.jChatInputEl.value : "").trim();
    if (!txt) return this.toast("Nothing to save — speak or type first");
    const st0 = this.state;
    const selId = st0.jSel || (st0.jPages[0] && st0.jPages[0].id);
    if (!selId) return;
    this.setState((st) => ({
      jInterim: "",
      jPages: st.jPages.map((p) => (p.id === selId ? { ...p, blocks: p.blocks.concat([prefix + txt]) } : p)),
    }));
    if (this.jChatInputEl) this.jChatInputEl.value = "";
    this.toast(prefix ? "Saved as a runnable prompt on this page" : "Saved to the page");
    this.save();
  }

  // ---------- journal export ----------
  private esc(t: string): string {
    return String(t).replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
  }
  private download(name: string, content: string, mime: string): void {
    const blob = new Blob([content], { type: mime });
    const a = document.createElement("a");
    a.href = URL.createObjectURL(blob);
    a.download = name;
    document.body.appendChild(a);
    a.click();
    a.remove();
    setTimeout(() => URL.revokeObjectURL(a.href), 3000);
  }
  private journalHtml(page: { title: string; blocks: string[] }): string {
    const items = page.blocks
      .map((b) => {
        const lead = (b.match(/^\s*/) || [""])[0].length;
        let t = b.trim();
        let tag = "";
        if (t.indexOf("@agent ") === 0) {
          t = t.slice(7);
          tag = '<span style="font-size:10px;color:#5d60c9;border:1px solid #5d60c9;border-radius:99px;padding:0 6px;margin-right:6px;">agent</span>';
        }
        if (t.indexOf("@prompt ") === 0) {
          t = t.slice(8);
          tag = '<span style="font-size:10px;color:#5a8205;border:1px solid #5a8205;border-radius:99px;padding:0 6px;margin-right:6px;">prompt</span>';
        }
        return '<li style="margin-left:' + Math.min(3, Math.floor(lead / 2)) * 22 + 'px;margin-bottom:6px;">' + tag + this.esc(t) + "</li>";
      })
      .join("");
    return (
      '<!DOCTYPE html><html><head><meta charset="utf-8"><title>' +
      this.esc(page.title) +
      "</title></head>" +
      '<body style="font-family:Georgia,serif;color:#0e0f0c;max-width:640px;margin:48px auto;line-height:1.6;">' +
      '<div style="font-size:11px;letter-spacing:.14em;text-transform:uppercase;color:#84867f;font-family:monospace;">Citrate journal · local · off-chain</div>' +
      '<h1 style="font-family:Helvetica,Arial,sans-serif;font-weight:500;">' +
      this.esc(page.title) +
      "</h1>" +
      '<ul style="list-style:disc;padding-left:20px;">' +
      items +
      "</ul></body></html>"
    );
  }
  exportJournal(fmt: string): void {
    const s = this.state;
    const page = s.jPages.find((p) => p.id === s.jSel) || s.jPages[0];
    if (!page) return;
    this.setState({ jExportOpen: false });
    const fname = page.title.replace(/[^\w-]+/g, "_");
    const mdLines = page.blocks.map((b) => {
      const lead = (b.match(/^\s*/) || [""])[0].length;
      return "  ".repeat(Math.min(3, Math.floor(lead / 2))) + "- " + b.trim();
    });
    if (fmt === "md") {
      this.download(fname + ".md", "# " + page.title + "\n\n" + mdLines.join("\n") + "\n", "text/markdown");
      this.toast("Markdown exported");
    } else if (fmt === "txt") {
      this.download(fname + ".txt", page.title + "\n\n" + page.blocks.map((b) => b.trim()).join("\n") + "\n", "text/plain");
      this.toast("Plain text exported");
    } else if (fmt === "doc") {
      this.download(fname + ".doc", this.journalHtml(page), "application/msword");
      this.toast("Word document exported");
    } else if (fmt === "pdf") {
      const f = document.createElement("iframe");
      f.style.cssText = "position:fixed;right:100%;width:800px;height:1000px;";
      document.body.appendChild(f);
      f.srcdoc = this.journalHtml(page);
      f.onload = () => {
        try {
          f.contentWindow!.focus();
          f.contentWindow!.print();
          this.toast("Print dialog opened — save as PDF");
        } catch {
          this.toast("Print blocked here — exported Markdown instead");
          this.exportJournal("md");
        }
        setTimeout(() => f.remove(), 60000);
      };
    }
  }

  // ---------- node lifecycle ----------
  startNode(): void {
    this.setState({ node: "prov", syncPct: 0, logs: [], peerRows: [] });
    if (BRIDGE_MODE === "tauri") {
      // Spawn the REAL supervised node (bridge.node.start → node.rs). The 2s
      // poller (refreshNode) then folds live state/height/peers as it boots +
      // syncs. `nodeStarting` guards the poller from demoting the optimistic
      // "prov" to "off" during the spawn window. On failure (e.g. the node
      // binary is not bundled) show an HONEST error, never a faked sync (Rule 1).
      this.nodeStarting = true;
      void bridge.node
        .start()
        .then(() => {
          this.nodeStarting = false;
          return this.refreshNode();
        })
        .catch((e) => {
          this.nodeStarting = false;
          this.setState({ node: "error" });
          this.toast("Could not start the node: " + (e instanceof Error ? e.message : "supervisor unavailable"));
        });
      return;
    }
    setTimeout(() => {
      this.setState({ node: "syncing", syncPct: 2, peerRows: this.makePeers(8) });
      this.save();
    }, 1500);
  }

  /** Stop the node. Tauri: release the real supervisor (bridge.node.stop). */
  stopNode(): void {
    this.setState({ node: "off", peers: 0, logs: [], syncPct: 0, peerRows: [] });
    if (BRIDGE_MODE === "tauri") {
      // Only claim the supervisor was released once the real stop RESOLVES — the
      // optimistic "off" reflects intent, but the toast must not overstate.
      void bridge.node
        .stop()
        .then(() => {
          this.toast("Node stopped — supervisor released");
          return this.refreshNode();
        })
        .catch(() => {
          this.toast("Stop requested — the supervisor releases on the next poll");
        });
      this.save();
      return;
    }
    this.toast("Node stopped — supervisor released");
    this.save();
  }

  /**
   * CORE-C2 — pull the REAL claimable from the chain and fold it into state.
   * Reads `ContributionAccounting.claimable(vaultAddress)` via `eth_call` on
   * 40204 through the bridge (data source: eth_call, Rule 11). On success the
   * claimable becomes a chain value (`earnSource: "chain"`); a fresh node
   * honestly reads 0. There is NO per-source breakdown from chain — the sim
   * `earnVal`/`earnPin`/`earnComp` are left as-is and the Earning tab labels them
   * as off-chain estimates (Rule 1 / I-3: no fabricated on-chain split).
   *
   * Failures (locked vault, RPC blip) are swallowed to a toast — the tab keeps
   * showing the last honest value rather than a fabricated one.
   */
  async refreshEarnings(): Promise<void> {
    try {
      const e = await bridge.agent.earnings();
      // Wei string → SALT number for the prototype's numeric display.
      const salt = Number(BigInt(e.claimableWei)) / 1e18;
      this.setState({ claimable: salt, earnSource: "chain" });
      this.save();
    } catch (err) {
      // Honest: no fabricated fallback. Keep the prior value, note the source.
      this.toast("Claimable unavailable — " + String((err as Error).message ?? err));
    }
  }

  /**
   * CORE-C2-F-1 (@rule8) — the REAL Claim. This REPLACES the old sim-ceremony
   * `apply` that toasted "Claimed — balance updated from chain" and locally did
   * `setState({liquid:+amt, claimable:0})` (a fabricated settlement — Rule 1 / I-3).
   *
   * It drives `bridge.agent.claim()`, which reads the REAL on-chain claimable and
   * either reports an honest "nothing to claim" (0) or mints a REAL pending
   * ceremony (the `claimRewards()` intent). In the Tauri build that ceremony is then
   * approved through the ONE human-in-the-loop signer via `signing.broadcast`
   * (B1.4 → a real 40204 tx); the balance changes ONLY when the real tx settles and
   * we re-read `claimable` from chain. In web-dev (sim) there is no key and no chain,
   * so a claim CANNOT settle — we say so honestly and never fake a balance change.
   */
  async claimRewards(): Promise<void> {
    let res: Awaited<ReturnType<typeof bridge.agent.claim>>;
    try {
      res = await bridge.agent.claim();
    } catch (err) {
      this.toast("Claim unavailable — " + String((err as Error).message ?? err));
      return;
    }
    if (res.kind === "nothing") {
      this.toast("No claimable earnings — nothing to claim.");
      return;
    }
    // A REAL pending ceremony exists (id: res.view.id). In web-dev the sim signer
    // cannot settle it — be honest rather than fabricate a claim.
    if (BRIDGE_MODE !== "tauri") {
      this.toast("Claim prepared — claims settle only in the desktop app (no key/chain in web preview).");
      return;
    }
    // Tauri: approve the real ceremony → sign + broadcast the real claimRewards()
    // tx (B1.4). No local balance mutation; the tab re-reads claimable from chain.
    try {
      const result = await bridge.signing.broadcast(res.view.id, false);
      this.addActivity("Claim rewards", "claimRewards()", result.txHash);
      this.toast("Claim broadcast — tx " + result.txHash.slice(0, 10) + "…; balance updates when it settles.");
      await this.refreshEarnings();
      this.save();
    } catch (err) {
      this.toast("Claim not settled — " + String((err as Error).message ?? err));
    }
  }

  /**
   * CORE (@rule8) — a REAL native SALT transfer. Builds the pending ceremony
   * (bridge.wallet.send → wallet_send), then approves + broadcasts the real
   * EIP-155 tx on 40204 (B1.4). NEVER mutates the local balance and NEVER
   * fabricates a hash (unlike the old sim ceremony) — the balance re-reads from
   * chain after settle (refreshWallet). Mirrors claimRewards(). Web-dev cannot
   * settle — honest message, no fake transfer.
   */
  async walletSend(to: string, amountWei: string): Promise<void> {
    if (BRIDGE_MODE !== "tauri") {
      this.toast("Send settles only in the desktop app — no key or chain in web preview.");
      return;
    }
    let view: Awaited<ReturnType<typeof bridge.wallet.send>>;
    try {
      view = await bridge.wallet.send(to, amountWei);
    } catch (err) {
      this.toast("Send unavailable — " + String((err as Error).message ?? err));
      return;
    }
    try {
      // Approve the real pending ceremony → sign + broadcast the real transfer.
      const result = await bridge.signing.broadcast(view.id, false);
      this.addActivity("Send", view.decoded.cost || "transfer", result.txHash);
      this.toast("Send broadcast — tx " + result.txHash.slice(0, 10) + "…; balance updates when it settles.");
      await this.refreshWallet();
      this.save();
    } catch (err) {
      // Honest failure — reject the pending ceremony so it isn't left dangling.
      try {
        await bridge.signing.reject(view.id);
      } catch {
        /* best-effort cleanup */
      }
      this.toast("Send not settled — " + String((err as Error).message ?? err));
    }
  }

  /**
   * CORE (@rule8) — a REAL LiquidStakingPool stake (`deposit()`). Builds the
   * pending ceremony (bridge.wallet.stake → wallet_stake), then approves +
   * broadcasts the real 40204 deposit tx (B1.4). NEVER mutates the local balance
   * and NEVER fabricates a hash — the staked figure re-reads from chain after
   * settle (refreshWallet → balanceOf). Mirrors walletSend. Web-dev cannot settle
   * — honest message, no fake stake. `amountWei` is a decimal wei string.
   */
  async walletStake(amountWei: string): Promise<void> {
    if (BRIDGE_MODE !== "tauri") {
      this.toast("Staking settles only in the desktop app — no key or chain in web preview.");
      return;
    }
    let view: Awaited<ReturnType<typeof bridge.wallet.stake>>;
    try {
      view = await bridge.wallet.stake(amountWei);
    } catch (err) {
      this.toast("Stake unavailable — " + String((err as Error).message ?? err));
      return;
    }
    try {
      // Approve the real pending ceremony → sign + broadcast the real deposit().
      const result = await bridge.signing.broadcast(view.id, false);
      this.addActivity("Stake", view.decoded.cost || "deposit", result.txHash);
      this.toast("Stake broadcast — tx " + result.txHash.slice(0, 10) + "…; staked balance updates when it settles.");
      await this.refreshWallet();
      this.save();
    } catch (err) {
      // Honest failure — reject the pending ceremony so it isn't left dangling.
      try {
        await bridge.signing.reject(view.id);
      } catch {
        /* best-effort cleanup */
      }
      this.toast("Stake not settled — " + String((err as Error).message ?? err));
    }
  }

  /**
   * CORE WP2 — pull the wallet's REAL pending withdrawals from chain and fold
   * them into AppState. `bridge.wallet.pendingWithdrawals()` (Tauri) reads the
   * live on-chain queue (getLogs WithdrawalRequested + withdrawals(id) +
   * block_number); the sim shim returns [] (no chain). A fresh wallet honestly
   * reads []. Failure leaves the last honest list untouched — no fabricated rows.
   */
  async refreshPendingWithdrawals(): Promise<void> {
    try {
      const list = await bridge.wallet.pendingWithdrawals();
      this.setState({ pendingWithdrawals: list });
    } catch {
      /* honest no-op: keep the last real list, never a fabricated one */
    }
  }

  /**
   * CORE WP2 (@rule8) — a REAL withdraw of self-added stake. Builds the pending
   * `requestWithdrawal(shares)` ceremony (bridge.wallet.requestWithdrawal →
   * wallet_request_withdrawal; the SALT→shares conversion is done in Rust from
   * live reads), then approves + broadcasts the real 40204 tx (B1.4). It burns
   * shares into the ~7-day queue; the SALT is claimable later via
   * walletClaimWithdrawal. NEVER mutates a local balance or fabricates a hash —
   * balances + the pending list re-read from chain after settle. Mirrors
   * walletStake. Web-dev cannot settle — honest message. `amountWei` = SALT (wei).
   */
  async walletRequestWithdrawal(amountWei: string): Promise<void> {
    if (BRIDGE_MODE !== "tauri") {
      this.toast("Withdraw settles only in the desktop app — no key or chain in web preview.");
      return;
    }
    let view: Awaited<ReturnType<typeof bridge.wallet.requestWithdrawal>>;
    try {
      view = await bridge.wallet.requestWithdrawal(amountWei);
    } catch (err) {
      this.toast("Withdraw unavailable — " + String((err as Error).message ?? err));
      return;
    }
    try {
      const result = await bridge.signing.broadcast(view.id, false);
      this.addActivity("Withdraw request", view.decoded.cost || "requestWithdrawal", result.txHash);
      this.toast("Withdrawal requested — SALT unlocks after ~7 days (50,400 blocks), then Claim. tx " + result.txHash.slice(0, 10) + "…");
      await this.refreshWallet();
      await this.refreshPendingWithdrawals();
      this.save();
    } catch (err) {
      try {
        await bridge.signing.reject(view.id);
      } catch {
        /* best-effort cleanup */
      }
      this.toast("Withdrawal not requested — " + String((err as Error).message ?? err));
    }
  }

  /**
   * CORE WP2 (@rule8) — claim a MATURED withdrawal. Builds the pending
   * `claimWithdrawal(id)` ceremony (bridge.wallet.claimWithdrawal →
   * wallet_claim_withdrawal), then approves + broadcasts the real 40204 tx (B1.4).
   * The ~7-day (50,400-block) delay is enforced ON-CHAIN — a too-early claim
   * reverts (we never fabricate an early settlement). NEVER mutates a local
   * balance or fabricates a hash — balances + pending list re-read from chain.
   * Web-dev cannot settle — honest message. `id` = the decimal request id.
   */
  async walletClaimWithdrawal(id: string): Promise<void> {
    if (BRIDGE_MODE !== "tauri") {
      this.toast("Claim settles only in the desktop app — no key or chain in web preview.");
      return;
    }
    let view: Awaited<ReturnType<typeof bridge.wallet.claimWithdrawal>>;
    try {
      view = await bridge.wallet.claimWithdrawal(id);
    } catch (err) {
      this.toast("Claim unavailable — " + String((err as Error).message ?? err));
      return;
    }
    try {
      const result = await bridge.signing.broadcast(view.id, false);
      this.addActivity("Withdraw claim", view.decoded.cost || "claimWithdrawal", result.txHash);
      this.toast("Claim broadcast — tx " + result.txHash.slice(0, 10) + "…; SALT lands when it settles.");
      await this.refreshWallet();
      await this.refreshPendingWithdrawals();
      this.save();
    } catch (err) {
      try {
        await bridge.signing.reject(view.id);
      } catch {
        /* best-effort cleanup */
      }
      this.toast("Claim not settled — " + String((err as Error).message ?? err));
    }
  }

  /**
   * Honest outcome for a value action whose REAL on-chain path is NOT wired yet.
   * Today this is bonded pinning. (The WITHDRAW leg — LiquidStakingPool
   * `requestWithdrawal` → `claimWithdrawal` — is now wired via
   * walletRequestWithdrawal / walletClaimWithdrawal, WP2.) NEVER fabricates a hash
   * or mutates a balance (Rule 1/3). Web-dev: settles only in the desktop app.
   */
  settleUnwired(action: string): void {
    if (BRIDGE_MODE === "tauri") {
      this.toast(action + " isn't wired to a real 40204 transaction yet — no funds moved. (Grounded contract pending.)");
    } else {
      this.toast(action + " settles only in the desktop app — no key or chain in web preview.");
    }
  }

  openMicroApp(a: { name?: string; capabilities?: string[] }): void {
    // No isolated capability-scoped webview is spawned yet — the old path toasted
    // "opened" but nothing happened. Honest until the micro-app runtime lands.
    this.toast((a.name || "This micro-app") + " isn't wired to an isolated window yet — capability-scoped micro-apps land in a later build.");
  }

  // ---------- onboarding transitions ----------
  onJoin(): void {
    this.setState({ stage: "s1" });
    this.save();
  }
  onExplore(): void {
    this.setState({ stage: "done", tier: "free", coachDone: true, chatMsgs: [greeting({ ...this.persona(), name: this.identity().name })] });
    this.save();
  }
  onS1Start(): void {
    this.setState({ s1: "waiting" });
    // Tauri: drive the REAL loopback-PKCE sign-in. The system browser opens; on
    // success the entitlement claim folds in and S1 completes. A failure/cancel
    // returns to idle (honest — no faked success).
    if (BRIDGE_MODE === "tauri") {
      void this.authLogin()
        .then(() => {
          this.setState({ s1: "done" });
          this.save();
        })
        .catch(() => {
          this.setState({ s1: "idle" });
        });
      return;
    }
    // Web-dev sim: the prototype attestation animation (no real auth).
    this._s1t = setTimeout(() => {
      this.setState({ s1: "attest", s1c: 0 });
      setTimeout(() => this.setState({ s1c: 1 }), 700);
      setTimeout(() => this.setState({ s1c: 2 }), 1500);
      setTimeout(() => this.setState({ s1c: 3 }), 2300);
      setTimeout(() => {
        this.setState({ s1: "done" });
        this.save();
      }, 2800);
    }, 2300);
  }
  onS1Cancel(): void {
    if (this._s1t) clearTimeout(this._s1t);
    this.setState({ s1: "idle" });
  }
  onS2Start(): void {
    this.setState({ s2: "pending" });
    // Tauri: open the REAL KYC flow in the browser, then poll /userinfo for the
    // kyc_status claim change (none→pending→verified/failed/review). The S2 UI
    // drives entirely off the real claim.
    if (BRIDGE_MODE === "tauri") {
      void this.kycStart().catch(() => {
        /* browser open failed — stay pending; the user can retry */
      });
      this.pollKyc();
      return;
    }
    // Web-dev sim: resolve to the persona's scripted outcome after a beat.
    setTimeout(() => {
      this.setState({ s2: this.state.kycOutcome });
      this.save();
    }, 2600);
  }

  /**
   * Poll the live /userinfo entitlement for a KYC status change while S2 is
   * pending (Tauri only). Stops once the claim resolves to a terminal state.
   */
  private pollKyc(): void {
    if (BRIDGE_MODE !== "tauri") return;
    const tick = () => {
      if (this.state.s2 !== "pending") return; // resolved or navigated away
      void this.authUserinfo().finally(() => {
        if (this.state.s2 === "pending") setTimeout(tick, 5000);
        else this.save();
      });
    };
    setTimeout(tick, 5000);
  }
  onS3Pay(): void {
    this.setState({ s3: "paying" });
    // Tauri: open the REAL core-membership checkout popup, then POLL /userinfo
    // for the entitlement grant. The money + grant happen ENTIRELY server-side
    // (core-membership → droplet); this app only opens the URL and watches its
    // OWN entitlement. S3 advances ONLY when /userinfo shows the REAL grant
    // (a PAID tier + active entitlement) — NEVER a faked settle (Rule 1). This
    // is the same "open a flow then poll userinfo" shape as onS2Start/pollKyc.
    if (BRIDGE_MODE === "tauri") {
      void bridge.membership.checkout().catch(() => {
        // Popup open failed (headless / user cancel at OS level) — stay "paying";
        // the poll below simply never settles. The user can retry (S3 re-enterable).
      });
      this.pollMembership();
      return;
    }
    // Web-dev sim: keep the prototype's fake settle so onboarding still walks.
    // (Guarded: the sim membership.checkout is a no-op; the tier flip is sim-only.)
    setTimeout(() => {
      this.setState({ s3: "settled", tier: "pilot" });
      this.save();
    }, 2500);
  }

  /**
   * CORE-D3.C — poll the live /userinfo entitlement while S3 is "paying" (Tauri
   * only), advancing to "settled" ONLY once the REAL grant lands (a paid tier +
   * active entitlement — `isPaidEntitlementActive`). The money + grant are
   * server-side; a user who closes the checkout popup without paying simply
   * never flips the entitlement, so this never settles (no fabrication — Rule 1).
   *
   * Bounded (mirrors pollKyc's cadence) so a never-completing checkout does not
   * poll forever: after the cap it leaves S3 re-enterable (still "paying" state
   * the UI can offer a retry from) rather than fabricating a settlement.
   */
  private pollMembership(attempt = 0): void {
    if (BRIDGE_MODE !== "tauri") return;
    const MAX_ATTEMPTS = 60; // ~5 min at 5s cadence — matches the checkout window
    const tick = () => {
      if (this.state.s3 !== "paying") return; // resolved or navigated away
      void this.authUserinfo().finally(() => {
        // Re-read the folded entitlement engine state at the DECISION point. Only
        // a REAL paid+active grant settles S3 (Rule 1 — never before /userinfo).
        if (this.state.s3 !== "paying") return;
        if (isPaidEntitlementActive(this.state)) {
          this.setState({ s3: "settled" });
          this.save();
          return;
        }
        // Not yet granted — keep polling until the bounded cap.
        if (attempt + 1 < MAX_ATTEMPTS) this.pollMembership(attempt + 1);
        // At the cap we stop polling; S3 stays "paying" (re-enterable), never faked.
      });
    };
    setTimeout(tick, 5000);
  }
  onS5Begin(): void {
    this.setState({ s5: "verifying", s5c: 0 });
    setTimeout(() => this.setState({ s5c: 1 }), 800);
    setTimeout(() => this.setState({ s5c: 2 }), 1600);
    setTimeout(() => this.setState({ s5c: 3 }), 2400);
    setTimeout(() => {
      this.setState({ s5: "settling", s5n: 0 });
    }, 3300);
  }
  onEnter(): void {
    const s = this.state;
    this.setState({
      stage: "done",
      coach: s.coachDone ? -1 : 0,
      chatMsgs: s.chatMsgs.length ? s.chatMsgs : [greeting({ ...this.persona(), name: this.identity().name })],
    });
    this.save();
  }

  // ---------- demo panel ----------
  selectPersona(pid: string): void {
    const ns = freshState(pid);
    ns.demoOpen = true;
    this.setState(ns);
    this.save();
  }
  resetProto(): void {
    try {
      localStorage.removeItem(STORAGE_KEY);
    } catch {
      /* ignore */
    }
    const ns = freshState("p1");
    this.setState(ns);
    this.toast("Prototype reset — fresh install");
  }
}

function nodeLabelLocal(n: string): string {
  return (
    { off: "off", prov: "provisioning", syncing: "syncing", synced: "synced", paused: "paused", validating: "validating", error: "error" } as Record<string, string>
  )[n] || n;
}

// One shared store instance for the app.
export const store = new Store();

export function useStore(): AppState {
  return useSyncExternalStore(store.subscribe, store.getSnapshot, store.getSnapshot);
}
