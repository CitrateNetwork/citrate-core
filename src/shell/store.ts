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
  STORAGE_KEY,
  freshState,
  greeting,
  loadState,
  makeHash,
} from "./state";
import { NODE_LOG_TEMPLATES } from "../data/seed";
import { createDemoProvider, ChatProvider, ToolCall } from "../agent/harness";
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

export class Store {
  state: AppState;
  private subs = new Set<() => void>();
  private snap: AppState;
  private cid = 0;
  private mid = 0;
  private resolvers: Record<string, (v: string) => void> = {};
  private timer: ReturnType<typeof setInterval> | null = null;
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

  /** Fold a claim-derived AuthStatus into AppState (the entitlement engine). */
  private applyAuthStatus(st: {
    signedIn: boolean;
    tier: string | null;
    org: string | null;
    role: string | null;
    kycStatus: string | null;
    expiresAt?: string | null;
  }): void {
    if (!st.signedIn) return;
    const patch: Partial<AppState> = {};
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
    }
    patch.org = st.org;
    if (st.role) patch.citrateRole = st.role;
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
    this.setState({ tier: "free", citrateRole: "member", org: null });
    this.save();
  }

  /** Open KYC in the browser; S2 status then arrives via userinfo polling. */
  async kycStart(): Promise<void> {
    await bridge.auth.kycStart();
  }
  stop(): void {
    if (this.timer) clearInterval(this.timer);
    this.timer = null;
  }

  // ---------- helpers ----------
  persona(): Persona {
    return PERSONAS[this.state.persona] || PERSONAS.p1;
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
      u.height = s.height + (Math.random() < 0.85 ? 1 : 2);
      u.finAge = s.finAge + 0.6;
      if (u.height - s.lastCp >= 50) {
        u.lastCp = u.height;
        u.finAge = 0;
      }
      const running = s.node !== "off" && s.node !== "prov";
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
    setTimeout(() => {
      this.setState({ node: "syncing", syncPct: 2, peerRows: this.makePeers(8) });
      this.save();
    }, 1500);
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

  openMicroApp(a: { name?: string; capabilities?: string[] }): void {
    this.requestSig({
      origin: "micro-app",
      requester: (a.name || "micro-app") + " · catalog-declared capabilities",
      title: "Open " + a.name + " in an isolated window",
      rows: [{ k: "Surface", v: "isolated webview · capability-scoped bridge" }].concat(
        (a.capabilities || []).map((c, i) => ({ k: "Capability " + (i + 1), v: c })),
      ),
      cost: "none — capabilities only; any transaction it proposes returns to this ceremony",
      sponsor: "no chain transaction",
      sponsorColor: "var(--tx-3)",
      chainless: true,
      apply: () => this.toast(a.name + " opened — denied capabilities are inert, not errors"),
    });
  }

  // ---------- onboarding transitions ----------
  onJoin(): void {
    this.setState({ stage: "s1" });
    this.save();
  }
  onExplore(): void {
    this.setState({ stage: "done", tier: "free", coachDone: true, chatMsgs: [greeting(this.persona())] });
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
    setTimeout(() => {
      this.setState({ s3: "settled", tier: "pilot" });
      this.save();
    }, 2500);
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
      chatMsgs: s.chatMsgs.length ? s.chatMsgs : [greeting(this.persona())],
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
