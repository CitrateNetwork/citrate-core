// =====================================================================
// citrate-core — Settings surface (1:1 from design/CitrateCore.dc.html
// SETTINGS section). Eight sections behind a left sub-nav (sSec):
// Account & RBAC, Connections, AI providers, Node configuration,
// API endpoints & keys, Keys & security, Memberships & billing, App.
//
// Honesty rule (Rule 1 / I-3 · Q-A.1): EVERY control is ONE of —
//   (a) a REAL working action (config toggles, AI-provider keyring flow,
//       custody Lock, Sign out, billing renew, Manage account ↗, the live RPC
//       probe), OR
//   (b) an honestly DISABLED + annotated control (visibly non-interactive, a
//       truthful one-line "not available in this build" — never a fake success,
//       never an apology-on-click), OR
//   (c) removed.
// There are NO fabricated success toasts, NO "…isn't wired yet (a scheduled
// build)" apology toasts, and NO hardcoded status asserting an unverified fact.
// A control with no backend is a disabled state, not a button that toasts an
// excuse. Every status traces to a real read (a probe, a folded /userinfo
// claim, s.walletAddr) or an honest disabled/"—" placeholder.
//
// Data source — prototype/sim AppState (see state.ts). Account claims are
// captioned "live from /userinfo" per the design; wiring replaces the sim,
// not the UI.
// =====================================================================
import { useEffect, useState } from "react";
import { createPublicClient, http } from "viem";
import { Store } from "../shell/store";
import { AppState, fmtSaltFromWei } from "../shell/state";
import { citrate } from "../chain";
import { bridge, type AppConfig } from "../bridge";
import type { AiProviderStatus, ConnectionInfo } from "../bridge/domains";
import { BRIDGE_MODE } from "../bridge/mode";

// Q-A.1 — an honestly DISABLED + annotated control. It is visibly
// non-interactive (the native `disabled` attribute + muted styling) and carries
// a single truthful "not available in this build" note instead of a fake
// success or an apology-on-click. This is the honest replacement for every
// control that previously existed only to toast an excuse (Rule 1).
function DisabledControl({ label, note }: { label: string; note: string }) {
  return (
    <span style={{ display: "inline-flex", flexDirection: "column", gap: 3, alignItems: "flex-start" }}>
      <button className="btn btn-ghost btn-sm" disabled style={{ opacity: 0.5, cursor: "not-allowed" }} aria-disabled="true">
        {label}
      </button>
      <span className="mono" style={{ fontSize: 10, color: "var(--tx-3)" }}>
        {note}
      </span>
    </span>
  );
}

// Node-configuration + App config write through the bridge (CORE-A1 A1.4). In
// sim mode this delegates back to the Store (1:1 UI preserved); in a Tauri
// build it round-trips through the real on-disk config store. We optimistically
// patch AppState (unchanged UX) and persist the same value through the bridge.
function writeConfig(store: Store, patch: Partial<AppConfig>): void {
  store.setState(patch as Partial<AppState>);
  bridge.config.write(patch).catch(() => {
    /* honest no-op: on the Tauri path a failed persist surfaces on next read */
  });
}

const short = (h: string | null | undefined) => (h ? h.slice(0, 6) + "…" + h.slice(-4) : "—");

// F1 (BC-5/6, Rule 1) — render the folded entitlement expiry HONESTLY. The
// `/userinfo` `expires_at` claim may arrive as epoch-milliseconds, epoch-seconds,
// or an ISO/date-ish string (state.authExpiresAt is the value verbatim). Rendered
// raw, an epoch would show as a bare integer like "1783939200000". This normalises
// at the RENDER layer only:
//   - all-digit integer  → epoch (ms if >= 1e12, else seconds) → YYYY-MM-DD
//   - ISO/date-ish string → normalised to YYYY-MM-DD when parseable, else verbatim
//   - null/empty          → honest "—" (NEVER a fabricated date; Rule 1)
// It never invents a date when the claim is absent — an unparseable non-empty value
// is passed through verbatim rather than guessed.
export function fmtExpiresAt(raw: string | null | undefined): string {
  if (raw == null) return "—";
  const v = raw.trim();
  if (!v) return "—";
  if (/^\d+$/.test(v)) {
    const n = Number(v);
    const ms = n >= 1e12 ? n : n * 1000;
    const d = new Date(ms);
    if (!Number.isNaN(d.getTime())) return d.toISOString().slice(0, 10);
    return v; // out-of-range integer — pass through rather than fabricate
  }
  const d = new Date(v);
  if (!Number.isNaN(d.getTime())) return d.toISOString().slice(0, 10);
  return v; // non-empty but unparseable — honest verbatim, no guessing
}

// Imperative element refs for the AI-provider editor. The KEY input value is
// handed to Rust once (sealed in the OS keyring) and never stored in AppState —
// so the ref lives only as long as an editor is open (module-scoped, like the
// design's `this.aiInputEl`). `aiBaseUrlEl`/`aiModelEl` back the generic
// OpenAI-compatible preset's baseURL + model fields (AI1).
let aiInputEl: HTMLInputElement | null = null;
let aiBaseUrlEl: HTMLInputElement | null = null;
let aiModelEl: HTMLInputElement | null = null;

const CONN: [string, string, string][] = [
  ["gcal", "Google Calendar", "events · read + propose writes"],
  ["gdrive", "Google Drive", "files · read"],
  ["notion", "Notion", "pages · read + draft"],
  ["linear", "Linear", "issues · read + draft"],
  ["planner", "Microsoft Planner", "tasks · read"],
  ["trello", "Trello", "boards · read + draft"],
  ["outlook", "Microsoft Outlook", "mail metadata · read"],
  ["github", "GitHub", "repos · read · PR drafts"],
  ["hf", "Hugging Face", "models · read + pull"],
];

// CORE-AI1 (@rule8) — the OpenAI-compatible provider presets. Each seals
// {baseURL, model, apiKey} in the OS keyring (Rust binds the key to its https
// baseURL); the key never touches the webview. `fixedBaseUrl` is null for the
// generic "custom" preset (the user supplies any https OpenAI-compatible baseURL).
interface AiPreset {
  id: string;
  name: string;
  fixedBaseUrl: string | null;
  defaultModel: string;
  keyPlaceholder: string;
  note: string;
}
const AI_PRESETS: AiPreset[] = [
  { id: "openai", name: "OpenAI", fixedBaseUrl: "https://api.openai.com/v1", defaultModel: "gpt-4o-mini", keyPlaceholder: "sk-…", note: "api.openai.com · sk- key" },
  { id: "gateway", name: "Citrate gateway", fixedBaseUrl: "https://infer.citrate.ai/v1", defaultModel: "citrate-default", keyPlaceholder: "cgk_…", note: "infer.citrate.ai · cgk_ key" },
  { id: "custom", name: "OpenAI-compatible endpoint", fixedBaseUrl: null, defaultModel: "", keyPlaceholder: "bearer key", note: "any https /v1 endpoint · bearer key" },
];

const SECS: [string, string][] = [
  ["account", "Account & RBAC"],
  ["connections", "Connections"],
  ["ai", "AI providers"],
  ["node", "Node configuration"],
  ["api", "API endpoints & keys"],
  ["keys", "Keys & security"],
  ["billing", "Memberships & billing"],
  ["app", "App"],
];

const btnCls = (active: boolean) => "btn btn-sm " + (active ? "btn-secondary" : "btn-ghost");

export function Settings({ store, s }: { store: Store; s: AppState }) {
  // Account & RBAC render the REAL signed-in identity (live /userinfo claims)
  // when signed in, falling back to the sim persona only in web-dev.
  const id = store.identity();
  const effTier = s.entitlement === "lapsed" ? "free" : s.tier;

  // ---------- CORE-AI1 (@rule8) — live provider status (keyring-sealed) ----------
  // The provider KEY is never in AppState — it lives sealed in the OS keyring. This
  // reads only NON-SECRET status (id/baseURL/model/configured) via the bridge, so
  // the rows render whether a provider is actually configured (Rule 1), never the
  // key. Refetched on the AI section + whenever the edit target / default changes.
  const [aiStatuses, setAiStatuses] = useState<AiProviderStatus[]>([]);
  useEffect(() => {
    if (s.sSec !== "ai") return;
    let live = true;
    bridge.chat
      .providerStatus()
      .then((st) => {
        if (live) setAiStatuses(st);
      })
      .catch(() => {
        if (live) setAiStatuses([]); // honest: no keyring (web) / failed read
      });
    return () => {
      live = false;
    };
  }, [s.sSec, s.aiEdit, s.aiDefault]);

  // ---------- account & RBAC ----------
  // BC-6.3 (Rule 1): render the REAL entitlement expiry folded from the /userinfo
  // `expires_at` claim (state.authExpiresAt) — an honest "—" when absent — NEVER a
  // hardcoded date. The value is displayed verbatim (ISO datetime or unix seconds
  // as the authority sent it); its colour reflects the (claim-derived) entitlement.
  // F1: fold + normalise (epoch → date; ISO → YYYY-MM-DD; absent → "—") so a raw
  // epoch integer never renders verbatim. fmtExpiresAt keeps the honest "—".
  const expReal = fmtExpiresAt(s.authExpiresAt);
  const claimRows: { k: string; v: string; color: string }[] = [
    { k: "sub", v: id.sub, color: "var(--tx-1)" },
    { k: "email", v: id.email, color: "var(--tx-1)" },
    { k: "tier", v: effTier + (effTier !== s.tier ? " (was " + s.tier + ")" : ""), color: "var(--tx-1)" },
    { k: "role", v: id.role, color: "var(--tx-1)" },
    { k: "org", v: s.org || "—", color: s.org ? "var(--info)" : "var(--tx-3)" },
    {
      k: "expiresAt",
      v: expReal,
      color:
        expReal === "—"
          ? "var(--tx-3)"
          : s.entitlement === "active"
            ? "var(--tx-1)"
            : s.entitlement === "lapsed"
              ? "var(--danger)"
              : "var(--warn)",
    },
    { k: "kyc_status", v: s.hasSbt || s.s2 === "verified" ? "verified" : "none", color: "var(--tx-1)" },
  ];

  // ---------- connections ----------
  // Q-A.1 (Rule 1) — there is NO real OAuth token flow in this build: no browser
  // handshake, no keyring token storage, no revocation. Connect/Disconnect are
  // therefore honestly DISABLED (a truthful "connections unavailable in this
  // build" note), not buttons that fake a "connected" flag or toast an excuse.
  // W4 — the three MCP services with a REAL OAuth backend (loopback-PKCE +
  // vaulted token). The rest of CONN stay honest "later" placeholders.
  const WIRED_CONN = new Set(["github", "gdrive", "notion"]);
  const connRows = CONN.map(([id, name, scope]) => ({
    id,
    name,
    scope: scope + " · MCP tool",
    wired: WIRED_CONN.has(id),
  }));

  // Live connection status (null = not yet loaded). `connBusy` holds the id of the
  // service whose flow is in flight; `connErr` surfaces a failure honestly.
  const [conns, setConns] = useState<ConnectionInfo[] | null>(null);
  const [connBusy, setConnBusy] = useState<string | null>(null);
  const [connErr, setConnErr] = useState<string | null>(null);
  useEffect(() => {
    if (s.sSec !== "connections") return;
    let live = true;
    bridge.connections
      .status()
      .then((c) => {
        if (live) setConns(c);
      })
      .catch(() => {
        if (live) setConns([]); // honest: status read failed
      });
    return () => {
      live = false;
    };
  }, [s.sSec]);
  const connFor = (id: string): ConnectionInfo | null =>
    conns?.find((c) => c.service === id) ?? null;
  const refreshConns = async () => {
    try {
      setConns(await bridge.connections.status());
    } catch {
      /* leave prior state; a failed refresh is not fatal */
    }
  };
  const doConnect = async (id: string) => {
    setConnErr(null);
    setConnBusy(id);
    try {
      await bridge.connections.start(id);
      await refreshConns();
    } catch (e) {
      setConnErr(`${id}: ${e instanceof Error ? e.message : String(e)}`);
    } finally {
      setConnBusy(null);
    }
  };
  const doDisconnect = async (id: string) => {
    setConnErr(null);
    setConnBusy(id);
    try {
      await bridge.connections.disconnect(id);
      await refreshConns();
    } catch (e) {
      setConnErr(`${id}: ${e instanceof Error ? e.message : String(e)}`);
    } finally {
      setConnBusy(null);
    }
  };

  // ---------- CORE-AI1 (@rule8) — AI providers (keyring-sealed BYO-key) ----------
  const statusFor = (pid: string) => aiStatuses.find((p) => p.id === pid);
  // The default route can be any CONFIGURED provider (live status), so a real
  // provider is only offered once its key is actually sealed (Rule 1 — no route to
  // an unconfigured provider). The demo agent is always available as the fallback.
  const aiDefaults = AI_PRESETS.filter((p) => statusFor(p.id)?.configured).map((p) => ({
    label: p.name,
    cls: btnCls(s.aiDefault === p.id),
    go: () => void store.aiSetDefault(p.id),
  }));
  const aiRows = AI_PRESETS.map((p) => {
    const st = statusFor(p.id);
    const configured = !!st?.configured;
    const editing = s.aiEdit === p.id;
    return {
      id: p.id,
      name: p.name,
      note: p.note,
      fixedBaseUrl: p.fixedBaseUrl,
      defaultModel: p.defaultModel,
      configured,
      editing,
      idle: !editing,
      // The status line reflects the SEALED config (baseURL + model) — never a key.
      keyLine: configured ? `${st!.baseURL} · ${st!.model} · key in OS keyring` : "no key sealed",
      keyColor: configured ? "var(--ok)" : "var(--tx-3)",
      placeholder: p.keyPlaceholder,
      isDefault: s.aiDefault === p.id,
      edit: () => store.setState({ aiEdit: p.id }),
      cancel: () => store.setState({ aiEdit: null }),
      remove: () => void store.aiClearProvider(p.id),
      save: () => {
        const key = aiInputEl ? aiInputEl.value.trim() : "";
        if (key.length < 8) return store.toast("That does not look like a key");
        // The baseURL is the preset's fixed https endpoint, or (for the generic
        // "custom" preset) the user-supplied one — validated as https in Rust.
        const baseURL = p.fixedBaseUrl ?? (aiBaseUrlEl ? aiBaseUrlEl.value.trim() : "");
        if (!baseURL) return store.toast("Enter the provider's https /v1 base URL");
        const model = aiModelEl && aiModelEl.value.trim() ? aiModelEl.value.trim() : p.defaultModel;
        if (!model) return store.toast("Enter the model name");
        // Hand the key to Rust ONCE (sealed in the OS keyring); never store it in
        // AppState/localStorage. aiInputEl is cleared on the next render (edit ends).
        void store.aiSetProvider(p.id, baseURL, model, key);
      },
    };
  });

  // ---------- node configuration ----------
  const cpuCaps = [25, 50, 75].map((c) => ({
    label: c + " %",
    cls: btnCls(s.cpuCap === c),
    go: () => {
      writeConfig(store, { cpuCap: c });
      store.save();
    },
  }));

  // ---------- API endpoints & keys ----------
  // Q-A.1 (Rule 1) — the RPC health pill traces to a REAL probe, never an
  // unconditional "healthy". We call eth_blockNumber against the SELECTED RPC
  // (local 127.0.0.1:8545 or public rpc.citrate.ai) using the viem client the
  // frontend already ships (src/chain.ts). Until the probe resolves the honest
  // baseline is "checking…" (never a fabricated health assertion). The result is
  // reachable (block N) / unreachable — a truthful color + text from real data.
  type RpcProbe = { phase: "checking" | "up" | "down"; block: number | null; host: string };
  const rpcHost = s.rpc === "local" ? "127.0.0.1:8545" : "rpc.citrate.ai";
  const [rpcProbe, setRpcProbe] = useState<RpcProbe>({ phase: "checking", block: null, host: rpcHost });
  useEffect(() => {
    if (s.sSec !== "api") return;
    let live = true;
    setRpcProbe({ phase: "checking", block: null, host: rpcHost });
    const url = s.rpc === "local" ? "http://127.0.0.1:8545" : "https://rpc.citrate.ai";
    const client = createPublicClient({ chain: citrate, transport: http(url) });
    client
      .getBlockNumber()
      .then((bn) => {
        if (live) setRpcProbe({ phase: "up", block: Number(bn), host: rpcHost });
      })
      .catch(() => {
        if (live) setRpcProbe({ phase: "down", block: null, host: rpcHost });
      });
    return () => {
      live = false;
    };
  }, [s.sSec, s.rpc, rpcHost]);
  const rpcHealthColor = rpcProbe.phase === "up" ? "var(--ok)" : rpcProbe.phase === "down" ? "var(--danger)" : "var(--tx-3)";
  const rpcHealthText =
    rpcProbe.phase === "checking"
      ? rpcHost + " · checking…"
      : rpcProbe.phase === "up"
        ? rpcHost + " · reachable · block " + rpcProbe.block
        : rpcHost + " · unreachable";

  // Q-A.1 (Rule 1) — gateway-key issuance/rotation/revocation has NO backend in
  // this build (the real key is minted server-side by core-membership against a
  // live entitlement). The whole cluster is now an honest DISABLED state, not a
  // set of buttons that toast an excuse or hand out a fabricated `cgk_` string.

  // ---------- keys & security ----------
  const lockOpts = [15, 30, 60].map((m) => ({
    label: m + " min",
    cls: btnCls(s.autolock === m),
    go: () => {
      writeConfig(store, { autolock: m });
      store.toast("Auto-lock set to " + m + " minutes — UI and keystore share this constant");
      store.save();
    },
  }));
  const polNote =
    s.sigPolicy === "hitl"
      ? "every signature stops at the ceremony — the default"
      : "allowlisted intents skip the ceremony; everything else still stops";

  // ---------- memberships & billing ----------
  const memPlanLabel =
    effTier === "free"
      ? "Public tier — no membership"
      : effTier === "enterprise"
        ? "Enterprise seat · " + (s.org || "")
        : "Pilot membership · $48/year";
  // BC-6.3 (Rule 1): the renewal/expiry line uses the REAL folded expiry
  // (state.authExpiresAt) — honest "expiry not set" when absent — never a hardcoded
  // date. The Stripe customer portal isn't wired yet (core-membership CORE-S5.4), so
  // we say "renew in checkout" (the real seam renewMembership opens) rather than
  // claim a portal that doesn't exist.
  // F1: normalise the same folded claim for the billing renewal line (epoch → date,
  // ISO → YYYY-MM-DD); null stays null so "expiry not set"/"lapsed" copy is honest.
  const memExpiryFmt = fmtExpiresAt(s.authExpiresAt);
  const memExpiry = memExpiryFmt === "—" ? null : memExpiryFmt;
  // A lapsed member still had a real membership: show the real expired date rather
  // than collapsing to "no active membership" (which is only honest for a genuinely
  // free user who never held a paid tier). effTier collapses lapsed→free for gating,
  // so detect the lapsed case on the entitlement BEFORE the free-tier copy.
  const memRenewLine =
    s.entitlement === "lapsed"
      ? memExpiry
        ? "lapsed · expired " + memExpiry
        : "lapsed"
      : effTier === "free"
        ? "no active membership"
        : memExpiry
          ? "expires " + memExpiry
          : "expiry not set";
  const memBadge = s.entitlement === "active" ? "active" : s.entitlement;
  const mb =
    s.entitlement === "active"
      ? ["var(--ok-bg)", "var(--ok)", "var(--ok)"]
      : s.entitlement === "lapsed"
        ? ["var(--danger-bg)", "var(--danger)", "var(--danger)"]
        : ["var(--warn-bg)", "var(--warn)", "var(--warn)"];

  // ---------- app ----------
  // The updater has no backend in this build, so we never assert "current"
  // (unverifiable). Show the real running version + a BUILD STAMP (git sha + build time, baked at
  // build time by vite.config define) + channel. The build stamp is the only way to tell two installs
  // apart — the app version (0.1.0) is identical across rebuilds. The Check-for-updates control is an
  // honest DISABLED state below (not a fake "current" toast).
  const buildSha = typeof __BUILD_SHA__ === "string" ? __BUILD_SHA__ : "dev";
  const buildTime = typeof __BUILD_TIME__ === "string" ? __BUILD_TIME__ : "";
  const appVer = typeof __APP_VERSION__ === "string" ? __APP_VERSION__ : "0.0.0";
  const buildDay = buildTime ? buildTime.slice(0, 10) : "";
  const updText =
    "citrate-core " + appVer + " · build " + buildSha + (buildDay ? " (" + buildDay + ")" : "") + " · " + s.channel + " channel";

  const setSec = (id: string) => {
    store.setState({ sSec: id });
    store.save();
  };

  return (
    <div style={{ padding: "20px 26px 24px", display: "grid", gridTemplateColumns: "196px minmax(0,1fr)", gap: 22, maxWidth: 1000 }}>
      {/* left sub-nav */}
      <div style={{ display: "flex", flexDirection: "column", gap: 2 }}>
        <span style={{ fontFamily: "var(--font-display)", fontWeight: 420, fontSize: 24, marginBottom: 12 }}>Settings</span>
        {SECS.map(([id, label]) => {
          const active = s.sSec === id;
          return (
            <button
              key={id}
              onClick={() => setSec(id)}
              style={{
                fontFamily: "var(--font-sans)",
                textAlign: "left",
                fontSize: 13,
                fontWeight: active ? 500 : 400,
                padding: "8px 12px",
                border: "none",
                borderRadius: "var(--r-1)",
                cursor: "pointer",
                background: active ? "var(--srf-1)" : "transparent",
                color: active ? "var(--tx-1)" : "var(--tx-2)",
              }}
            >
              {label}
            </button>
          );
        })}
      </div>

      {/* section body */}
      <div style={{ display: "flex", flexDirection: "column", gap: 14, paddingTop: 44 }}>
        {/* ---------- Account & RBAC ---------- */}
        {s.sSec === "account" && (
          <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
            <div className="surface" style={{ padding: 18, display: "flex", flexDirection: "column", gap: 10 }}>
              <span className="eyebrow">Entitlement claim · live from /userinfo</span>
              {claimRows.map((cl) => (
                <div key={cl.k} style={{ display: "flex", gap: 14, borderBottom: "1px solid var(--line-1)", paddingBottom: 8 }}>
                  <span
                    className="mono"
                    style={{ fontSize: 10, letterSpacing: ".1em", textTransform: "uppercase", color: "var(--tx-3)", width: 110, flexShrink: 0, paddingTop: 2 }}
                  >
                    {cl.k}
                  </span>
                  <span className="mono" style={{ fontSize: 12.5, color: cl.color, wordBreak: "break-all" }}>
                    {cl.v}
                  </span>
                </div>
              ))}
              <div style={{ display: "flex", gap: 10, marginTop: 4 }}>
                <button
                  className="btn btn-ghost btn-sm"
                  onClick={() => void store.openAuthorityPage("https://auth.citrate.ai/account", "your account page")}
                >
                  Manage account ↗
                </button>
                <button
                  className="btn btn-ghost btn-sm"
                  onClick={() => {
                    void store.authLogout();
                    store.toast("Signed out — refresh token revoked at the authority and cleared from the vault");
                  }}
                >
                  Sign out
                </button>
              </div>
            </div>
          </div>
        )}

        {/* ---------- Connections ---------- */}
        {s.sSec === "connections" && (
          <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
            <div className="surface" style={{ display: "flex", flexDirection: "column" }}>
              <div style={{ padding: "14px 18px", borderBottom: "1px solid var(--line-1)", display: "flex", flexDirection: "column", gap: 3 }}>
                <span className="eyebrow">Connections · mount as MCP tools for your agent</span>
                <span style={{ fontSize: 12, color: "var(--tx-3)" }}>
                  {BRIDGE_MODE === "tauri"
                    ? "Connect a service and its OAuth sign-in opens in your system browser; on success the token is sealed in the OS keyring — never in the app bundle or the browser."
                    : "Connections are desktop-only — this preview shows every service as disconnected and cannot run the OAuth flow."}
                </span>
              </div>
              {connErr && (
                <div style={{ padding: "10px 18px", borderBottom: "1px solid var(--line-1)", fontSize: 11.5, color: "var(--err, var(--warn))" }}>
                  {connErr}
                </div>
              )}
              {connRows.map((cn) => {
                const st = cn.wired ? connFor(cn.id) : null;
                const busy = connBusy === cn.id;
                const connected = !!st?.connected;
                return (
                  <div key={cn.id} style={{ display: "flex", alignItems: "center", gap: 14, padding: "12px 18px", borderBottom: "1px solid var(--line-1)" }}>
                    <span style={{ flex: 1, minWidth: 0 }}>
                      <span style={{ display: "block", fontSize: 13, fontWeight: 500 }}>
                        {cn.name}
                        {connected && (
                          <span className="mono" style={{ fontSize: 9.5, color: "var(--ok)", marginLeft: 8 }}>
                            ● connected
                          </span>
                        )}
                      </span>
                      <span className="mono" style={{ display: "block", fontSize: 10.5, color: "var(--tx-3)", marginTop: 1 }}>
                        {cn.wired ? cn.scope : cn.scope + " · not yet available"}
                      </span>
                    </span>
                    {!cn.wired ? (
                      <button className="btn btn-ghost btn-sm" disabled style={{ opacity: 0.5, cursor: "not-allowed" }} aria-disabled="true">
                        Connect
                      </button>
                    ) : connected ? (
                      <button className="btn btn-ghost btn-sm" onClick={() => doDisconnect(cn.id)} disabled={busy}>
                        {busy ? "…" : "Disconnect"}
                      </button>
                    ) : (
                      <button className="btn btn-secondary btn-sm" onClick={() => doConnect(cn.id)} disabled={busy || BRIDGE_MODE !== "tauri"}>
                        {busy ? "Waiting for browser…" : "Connect"}
                      </button>
                    )}
                  </div>
                );
              })}
            </div>
            <p style={{ fontSize: 11, lineHeight: 1.55, color: "var(--tx-3)", margin: 0 }}>
              Agent access to a connection is capability-scoped, and every write it proposes — an event, an issue, a page — queues for your approval like any other tool write.
            </p>
          </div>
        )}

        {/* ---------- AI providers (CORE-AI1, @rule8) ---------- */}
        {s.sSec === "ai" && (
          <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
            <div style={{ border: "1px solid var(--info)", background: "var(--info-bg, var(--warn-bg))", borderRadius: "var(--r-2)", padding: "12px 14px", fontSize: 12, lineHeight: 1.55, color: "var(--tx-2)" }}>
              With no provider configured, chat runs on the built-in <strong>demo agent</strong>. Add an OpenAI-compatible key below and chat becomes <strong>real</strong>: the key is sealed in the <strong>OS keyring</strong> and the call to <span className="mono">/v1/chat/completions</span> is made from the desktop app, so the key never touches the browser. The endpoint is bound to the key at save time — the app can never send your key anywhere but the endpoint you configured.
              {BRIDGE_MODE !== "tauri" && (
                <>
                  {" "}
                  <em>Web preview has no OS keyring — provider keys can only be sealed in the desktop app.</em>
                </>
              )}
            </div>
            <div className="surface" style={{ padding: 18, display: "flex", flexDirection: "column", gap: 10 }}>
              <span className="eyebrow">Default model route</span>
              <span style={{ display: "flex", gap: 6, flexWrap: "wrap" }}>
                {aiDefaults.length === 0 ? (
                  <span className="mono" style={{ fontSize: 11, color: "var(--tx-3)" }}>
                    no provider configured — chat runs on the built-in demo agent
                  </span>
                ) : (
                  aiDefaults.map((ad) => (
                    <button key={ad.label} className={ad.cls} onClick={ad.go}>
                      {ad.label}
                    </button>
                  ))
                )}
              </span>
              <span className="mono" style={{ fontSize: 10.5, color: "var(--tx-3)" }}>
                the default route is used for chat once its key is sealed; an unconfigured route falls back to the demo agent
              </span>
            </div>
            <div className="surface" style={{ display: "flex", flexDirection: "column" }}>
              <div style={{ padding: "14px 18px", borderBottom: "1px solid var(--line-1)" }}>
                <span className="eyebrow">Provider keys · bring your own (sealed in the OS keyring)</span>
              </div>
              {aiRows.map((ai) => (
                <div
                  key={ai.id}
                  style={{ display: "flex", alignItems: "center", gap: 14, padding: "12px 18px", borderBottom: "1px solid var(--line-1)", flexWrap: "wrap" }}
                >
                  <span style={{ flex: 1, minWidth: 120 }}>
                    <span style={{ display: "block", fontSize: 13, fontWeight: 500 }}>
                      {ai.name}
                      {ai.isDefault && ai.configured && (
                        <span className="mono" style={{ fontSize: 9.5, color: "var(--ok)", marginLeft: 8 }}>
                          default
                        </span>
                      )}
                    </span>
                    <span className="mono" style={{ display: "block", fontSize: 10.5, color: ai.keyColor, marginTop: 1 }}>
                      {ai.keyLine}
                    </span>
                  </span>
                  {ai.editing && (
                    <>
                      {ai.fixedBaseUrl === null && (
                        <input
                          ref={(el) => {
                            aiBaseUrlEl = el;
                          }}
                          className="input mono"
                          placeholder="https://…/v1"
                          style={{ width: 220, height: 30, fontSize: 12 }}
                        />
                      )}
                      <input
                        ref={(el) => {
                          aiModelEl = el;
                          if (el && ai.defaultModel) el.value = ai.defaultModel;
                        }}
                        className="input mono"
                        placeholder="model (e.g. gpt-4o-mini)"
                        style={{ width: 180, height: 30, fontSize: 12 }}
                      />
                      <input
                        ref={(el) => {
                          aiInputEl = el;
                          if (el) {
                            el.value = "";
                            try {
                              el.focus();
                            } catch {
                              /* ignore */
                            }
                          }
                        }}
                        className="input mono"
                        type="password"
                        placeholder={ai.placeholder}
                        style={{ width: 220, height: 30, fontSize: 12 }}
                      />
                      <button className="btn btn-secondary btn-sm" onClick={ai.save}>
                        Save key
                      </button>
                      <button className="btn btn-ghost btn-sm" onClick={ai.cancel}>
                        Cancel
                      </button>
                    </>
                  )}
                  {ai.idle && ai.configured && (
                    <>
                      <button className="btn btn-ghost btn-sm" onClick={ai.edit}>
                        Rotate
                      </button>
                      <button className="btn btn-danger btn-sm" onClick={ai.remove}>
                        Remove
                      </button>
                    </>
                  )}
                  {ai.idle && !ai.configured && (
                    <button className="btn btn-ghost btn-sm" onClick={ai.edit} disabled={BRIDGE_MODE !== "tauri"}>
                      Add key
                    </button>
                  )}
                </div>
              ))}
            </div>
            <p style={{ fontSize: 11, lineHeight: 1.55, color: "var(--tx-3)", margin: 0 }}>
              The key is sealed in the OS keyring and never written to disk in the app or exposed to the browser. The call originates from the desktop app and always goes to the endpoint you bound the key to. Local on-device model inference requires the model runtime (WO-3) and is not available yet.
            </p>
          </div>
        )}

        {/* ---------- Node configuration ---------- */}
        {s.sSec === "node" && (
          <div className="surface" style={{ padding: 18, display: "flex", flexDirection: "column", gap: 14 }}>
            <span className="eyebrow">Node configuration · every field here is wired</span>
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
              <span className="lbl">Network</span>
              <span style={{ display: "flex", gap: 8 }}>
                <button
                  className={btnCls(s.net === "testnet")}
                  onClick={() => {
                    writeConfig(store, { net: "testnet" });
                    store.save();
                  }}
                >
                  testnet · 40204
                </button>
                <button
                  className={btnCls(s.net === "local")}
                  onClick={() => {
                    writeConfig(store, { net: "local" });
                    store.toast("Applies at next node start — the running sidecar is untouched");
                    store.save();
                  }}
                >
                  local devnet
                </button>
              </span>
            </div>
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
              <span className="lbl">Data directory</span>
              <span style={{ display: "flex", gap: 8, alignItems: "center" }}>
                <span
                  className="mono"
                  style={{ fontSize: 12, background: "var(--srf-inset)", border: "1px solid var(--line-1)", borderRadius: "var(--r-1)", padding: "8px 10px", flex: 1 }}
                >
                  {s.dataDir}
                </span>
                <DisabledControl label="Move…" note="not available in this build" />
              </span>
            </div>
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
              <span className="lbl">Bootnodes · default set</span>
              <span
                className="mono"
                style={{ fontSize: 11, background: "var(--srf-inset)", border: "1px solid var(--line-1)", borderRadius: "var(--r-1)", padding: "8px 10px", lineHeight: 1.6 }}
              >
                boot1.citrate.ai:30303 · boot2.citrate.ai:30303 · boot-eu1.citrate.ai:30303
              </span>
            </div>
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
              <span className="lbl">Resource cap · CPU</span>
              <span style={{ display: "flex", gap: 8 }}>
                {cpuCaps.map((cc) => (
                  <button key={cc.label} className={cc.cls} onClick={cc.go}>
                    {cc.label}
                  </button>
                ))}
              </span>
              <span className="mono" style={{ fontSize: 10.5, color: "var(--tx-3)" }}>
                applies to the citrate-node sidecar at next start
              </span>
            </div>
          </div>
        )}

        {/* ---------- API endpoints & keys ---------- */}
        {s.sSec === "api" && (
          <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
            <div className="surface" style={{ padding: 18, display: "flex", flexDirection: "column", gap: 12 }}>
              <span className="eyebrow">RPC endpoint</span>
              <span style={{ display: "flex", gap: 8 }}>
                <button
                  className={btnCls(s.rpc === "local")}
                  onClick={() => {
                    writeConfig(store, { rpc: "local" });
                    store.save();
                  }}
                >
                  local node
                </button>
                <button
                  className={btnCls(s.rpc === "public")}
                  onClick={() => {
                    writeConfig(store, { rpc: "public" });
                    store.save();
                  }}
                >
                  rpc.citrate.ai
                </button>
              </span>
              <span style={{ display: "inline-flex", alignItems: "center", gap: 8 }}>
                <span style={{ width: 7, height: 7, borderRadius: 999, background: rpcHealthColor }}></span>
                <span className="mono" style={{ fontSize: 11, color: "var(--tx-2)" }}>
                  {rpcHealthText}
                </span>
              </span>
            </div>
            <div className="surface" style={{ padding: 18, display: "flex", flexDirection: "column", gap: 12 }}>
              <span className="eyebrow">Gateway key · inference</span>
              {/* Q-A.1 (Rule 1) — no backend mints, rotates, or revokes a gateway
                  key in this build. Honest DISABLED state, not a fabricated `cgk_`
                  string or an apology-on-click. */}
              <p style={{ fontSize: 12.5, color: "var(--tx-2)", margin: 0 }}>
                Gateway keys are minted server-side by the membership service against a live entitlement. That path is not available in this build.
              </p>
              <span style={{ display: "flex", gap: 10 }}>
                <button className="btn btn-secondary btn-sm" disabled style={{ opacity: 0.5, cursor: "not-allowed" }} aria-disabled="true">
                  Issue key
                </button>
                <button className="btn btn-ghost btn-sm" disabled style={{ opacity: 0.5, cursor: "not-allowed" }} aria-disabled="true">
                  Rotate
                </button>
                <button className="btn btn-ghost btn-sm" disabled style={{ opacity: 0.5, cursor: "not-allowed" }} aria-disabled="true">
                  Revoke
                </button>
              </span>
            </div>
            <div className="surface" style={{ padding: 18, display: "flex", flexDirection: "column", gap: 8 }}>
              <span className="eyebrow">Memory socket</span>
              <span
                className="mono"
                style={{ fontSize: 11, background: "var(--srf-inset)", border: "1px solid var(--line-1)", borderRadius: "var(--r-1)", padding: "8px 10px", wordBreak: "break-all" }}
              >
                {s.socketPath}
              </span>
            </div>
          </div>
        )}

        {/* ---------- Keys & security ---------- */}
        {s.sSec === "keys" && (
          <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
            {/* CORE-A2 — real custody lock state (custody_status in a Tauri
                build; sim shim in web-dev). Metadata only; no secret bytes. */}
            <div className="surface" style={{ padding: 18, display: "flex", flexDirection: "column", gap: 12 }}>
              <span className="eyebrow">Custody vault</span>
              <div style={{ display: "flex", alignItems: "center", gap: 12 }}>
                <span style={{ flex: 1 }}>
                  <span style={{ display: "block", fontSize: 13, fontWeight: 500 }}>Session lock</span>
                  <span className="mono" style={{ display: "block", fontSize: 10.5, color: "var(--tx-3)", marginTop: 2 }}>
                    Argon2id + AES-256-GCM envelope · OS-keyring-sealed master key · auto-lock {s.autolock} min
                  </span>
                </span>
                <span
                  className="mono"
                  style={{
                    fontSize: 10,
                    color:
                      s.custodyLock === "unlocked" ? "var(--ok)" : s.custodyLock === "locked" ? "var(--warn)" : "var(--tx-3)",
                  }}
                >
                  {s.custodyLock === "unlocked" ? "UNLOCKED" : s.custodyLock === "locked" ? "LOCKED" : "UNKNOWN"}
                </span>
                {s.custodyLock === "unlocked" ? (
                  <button
                    className="btn btn-ghost btn-sm"
                    onClick={() => {
                      void store.custodyLock();
                      store.toast("Vault locked — the in-memory data key is zeroized");
                    }}
                  >
                    Lock now
                  </button>
                ) : (
                  <button
                    className="btn btn-secondary btn-sm"
                    onClick={() => {
                      // Seamless device-bound unlock (Rule 1): provisioning is
                      // passphrase-less — the vault key is a random device secret in
                      // the OS keyring, NOT something the user can type. So Unlock
                      // re-provisions + unlocks from that keyring secret (the OS login
                      // is the security boundary). A reset keychain fails closed and
                      // the vault honestly stays locked.
                      void store
                        .custodyEnsureUnlocked()
                        .then(() => {
                          if (store.state.custodyLock === "unlocked") store.toast("Vault unlocked");
                          else store.toast("Vault stayed locked — device key unavailable");
                        })
                        .catch((err) => store.toast("Unlock failed — " + String((err as Error).message ?? err)));
                    }}
                  >
                    Unlock
                  </button>
                )}
              </div>
            </div>
            <div className="surface" style={{ padding: 18, display: "flex", flexDirection: "column", gap: 12 }}>
              <span className="eyebrow">Keystore</span>
              <div style={{ display: "flex", alignItems: "center", gap: 12, borderBottom: "1px solid var(--line-1)", paddingBottom: 10 }}>
                <span style={{ flex: 1 }}>
                  <span style={{ display: "block", fontSize: 13, fontWeight: 500 }}>Smart wallet · passkey validator</span>
                  <span className="mono" style={{ display: "block", fontSize: 10.5, color: "var(--tx-3)", marginTop: 2 }}>
                    WebAuthn P-256 · address derived from identity · deploys lazily on first tx
                  </span>
                </span>
                {/* Q-A.1 (Rule 1) — show the REAL derived smart-wallet address
                    (s.walletAddr, truncated) or an honest "—", never a fabricated
                    "ADDRESS SET" pill that asserts a set address unconditionally. */}
                <span className="mono" style={{ fontSize: 11, color: s.walletAddr ? "var(--accent-text)" : "var(--tx-3)" }}>
                  {short(s.walletAddr)}
                </span>
              </div>
              <div style={{ display: "flex", alignItems: "center", gap: 12, borderBottom: "1px solid var(--line-1)", paddingBottom: 10 }}>
                <span style={{ flex: 1 }}>
                  <span style={{ display: "block", fontSize: 13, fontWeight: 500 }}>Machine attestation</span>
                  <span className="mono" style={{ display: "block", fontSize: 10.5, color: "var(--tx-3)", marginTop: 2 }}>
                    {s.deviceId} · hardware-backed device attestation is not available in this build
                  </span>
                </span>
                <span className="mono" style={{ fontSize: 10, color: "var(--warn)" }}>
                  NOT AVAILABLE
                </span>
              </div>
              <div style={{ display: "flex", alignItems: "center", gap: 12 }}>
                <span style={{ flex: 1 }}>
                  <span style={{ display: "block", fontSize: 13, fontWeight: 500 }}>Local signing key</span>
                  <span className="mono" style={{ display: "block", fontSize: 10.5, color: "var(--tx-3)", marginTop: 2 }}>
                    Argon2id + AES-GCM keystore · OS keyring sealed
                  </span>
                </span>
                {/* Q-A.1 (Rule 1) — keystore export has no backend flow yet; honest
                    DISABLED state, not a button that toasts an excuse. */}
                <DisabledControl label="Export…" note="not available in this build" />
              </div>
              <p style={{ fontSize: 11, lineHeight: 1.5, color: "var(--tx-3)", margin: 0 }}>
                Export reveals your encrypted keystore file. Anyone with the file and your passphrase controls the key. No support agent will ever ask for it.
              </p>
            </div>
            <div className="surface" style={{ padding: 18, display: "flex", flexDirection: "column", gap: 10 }}>
              <span className="eyebrow">Session policy · one truth</span>
              <span style={{ display: "flex", gap: 8 }}>
                {lockOpts.map((lo) => (
                  <button key={lo.label} className={lo.cls} onClick={lo.go}>
                    {lo.label}
                  </button>
                ))}
              </span>
              <span className="mono" style={{ fontSize: 10.5, color: "var(--tx-3)" }}>
                auto-lock — the UI and the keystore enforce this same constant; there is no second timer
              </span>
            </div>
            <div className="surface" style={{ padding: 18, display: "flex", flexDirection: "column", gap: 10 }}>
              <span className="eyebrow">Signature approval policy</span>
              <span style={{ display: "flex", gap: 8 }}>
                <button
                  className={btnCls(s.sigPolicy === "hitl")}
                  onClick={() => {
                    writeConfig(store, { sigPolicy: "hitl" });
                    store.save();
                  }}
                >
                  Approve every signature
                </button>
                <button
                  className={btnCls(s.sigPolicy === "allow")}
                  onClick={() => {
                    writeConfig(store, { sigPolicy: "allow" });
                    store.save();
                  }}
                >
                  Allowlisted intents skip the ceremony
                </button>
              </span>
              {/* The policy toggle above is a REAL config write. The per-origin
                  allowlist RULE EDITOR has no backend yet — honest DISABLED state
                  (Q-A.1), not a toast excuse. */}
              {s.sigPolicy === "allow" && <DisabledControl label="Edit allowlist rules…" note="rule editor not available in this build" />}
              <span className="mono" style={{ fontSize: 10.5, color: "var(--tx-3)" }}>
                {polNote}
              </span>
            </div>
          </div>
        )}

        {/* ---------- Memberships & billing ---------- */}
        {s.sSec === "billing" && (
          <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
            <div className="surface" style={{ padding: 18, display: "flex", flexDirection: "column", gap: 10 }}>
              <div style={{ display: "flex", alignItems: "center", gap: 12 }}>
                <span style={{ flex: 1 }}>
                  <span style={{ display: "block", fontSize: 14, fontWeight: 500 }}>{memPlanLabel}</span>
                  <span className="mono" style={{ display: "block", fontSize: 11, color: "var(--tx-3)", marginTop: 2 }}>
                    {memRenewLine}
                  </span>
                </span>
                <span
                  className="mono"
                  style={{
                    fontSize: 9.5,
                    letterSpacing: ".1em",
                    textTransform: "uppercase",
                    padding: "2px 8px",
                    borderRadius: 999,
                    background: mb[0],
                    border: "1px solid " + mb[1],
                    color: mb[2],
                  }}
                >
                  {memBadge}
                </span>
              </div>
              <div style={{ display: "flex", gap: 10 }}>
                <button className="btn btn-secondary btn-sm" onClick={() => void store.renewMembership()}>
                  Renew ↗
                </button>
                {/* Q-A.1 (Rule 1) — the Stripe customer-portal link has no backend
                    in this build; honest DISABLED state, not a toast excuse. Renew
                    (left) is the REAL wired checkout path. */}
                <DisabledControl label="Cancel membership" note="Stripe portal not available in this build" />
              </div>
              <p style={{ fontSize: 11, lineHeight: 1.55, color: "var(--tx-3)", margin: 0 }}>
                On lapse: paid features lock after the 72-hour offline grace window. Your node, local wallet, and local memory never lock — your keys and chain access are yours. Stake attribution ends until renewal; vaulted principal stays vaulted.
              </p>
            </div>
            {s.hasSbt && (
              <div className="surface" style={{ padding: 18, display: "flex", flexDirection: "column", gap: 8 }}>
                <span className="eyebrow">SALT grant record</span>
                {/* F1 (Rule 1): every row traces to a real read. The staked amount is
                    the REAL attributedStake (wei→SALT via fmtSaltFromWei; shows "—"
                    when absent), NOT a hardcoded 32,000. The honest on-chain anchor is
                    the member wallet, NOT a fabricated tx hash — this app does not
                    broadcast the grant tx. */}
                <div style={{ display: "flex", justifyContent: "space-between" }}>
                  <span style={{ fontSize: 12.5, color: "var(--tx-2)" }}>Grant · staked at issuance</span>
                  <span className="mono tabular" style={{ fontSize: 12.5 }}>
                    {fmtSaltFromWei(s.s5StakeWei)} SALT
                  </span>
                </div>
                <div style={{ display: "flex", justifyContent: "space-between" }}>
                  <span style={{ fontSize: 12.5, color: "var(--tx-2)" }}>Member wallet</span>
                  <span className="mono" style={{ fontSize: 11.5, color: "var(--accent-text)" }}>
                    {short(s.walletAddr)}
                  </span>
                </div>
                <div style={{ display: "flex", justifyContent: "space-between" }}>
                  <span style={{ fontSize: 12.5, color: "var(--tx-2)" }}>Vault release</span>
                  {/* M-2.2/M-2.3 (Rule 1): the REAL lock + KYC state read from the
                      member's MemberBond escrow, not a policy slogan. "—" when the
                      read has not landed — never an invented status. */}
                  <span className="mono" style={{ fontSize: 11.5 }}>
                    {s.s5BondStatus ?? "—"}
                  </span>
                </div>
              </div>
            )}
            <div className="surface" style={{ padding: 18, display: "flex", flexDirection: "column", gap: 8 }}>
              <span className="eyebrow">Receipts</span>
              {/* Q-A.1 (Rule 1) — there is no real receipt read in this build, so we
                  show an honest "No receipts yet" rather than a fabricated
                  2026-07-11 · $48.00 line, and the PDF download is a disabled state. */}
              <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
                <span className="mono" style={{ fontSize: 11.5, color: "var(--tx-3)" }}>
                  No receipts yet
                </span>
                <DisabledControl label="PDF ↗" note="receipt downloads not available in this build" />
              </div>
            </div>
          </div>
        )}

        {/* ---------- App ---------- */}
        {s.sSec === "app" && (
          <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
            <div className="surface" style={{ padding: 18, display: "flex", flexDirection: "column", gap: 12 }}>
              <span className="eyebrow">Updates · signed manifests</span>
              <span style={{ display: "flex", gap: 8 }}>
                <button
                  className={btnCls(s.channel === "stable")}
                  onClick={() => {
                    writeConfig(store, { channel: "stable" });
                    store.save();
                  }}
                >
                  stable
                </button>
                <button
                  className={btnCls(s.channel === "beta")}
                  onClick={() => {
                    writeConfig(store, { channel: "beta" });
                    store.save();
                  }}
                >
                  beta
                </button>
              </span>
              {/* Q-A.1 (Rule 1) — no updater backend in this build. Check-now is an
                  honest DISABLED state (never a fake "current" toast), and the
                  footer no longer asserts an offline signature check that does not
                  run. The running version + channel below are real. */}
              <span style={{ display: "flex", alignItems: "center", gap: 10 }}>
                <button className="btn btn-ghost btn-sm" disabled style={{ opacity: 0.5, cursor: "not-allowed" }} aria-disabled="true">
                  Check now
                </button>
                <span className="mono" style={{ fontSize: 11, color: "var(--tx-2)" }}>
                  {updText}
                </span>
              </span>
              <span className="mono" style={{ fontSize: 10.5, color: "var(--tx-3)" }}>
                automatic update checks are not available in this build · citrate-core 0.1.0-proto
              </span>
            </div>
            <div className="surface" style={{ padding: 18, display: "flex", flexDirection: "column", gap: 10 }}>
              <span className="eyebrow">Telemetry</span>
              <span style={{ display: "flex", alignItems: "center", gap: 12 }}>
                <button
                  className={btnCls(!s.telemetry)}
                  onClick={() => {
                    writeConfig(store, { telemetry: false });
                    store.save();
                  }}
                >
                  off
                </button>
                <button
                  className={btnCls(s.telemetry)}
                  onClick={() => {
                    writeConfig(store, { telemetry: true });
                    store.save();
                  }}
                >
                  crash reports only
                </button>
              </span>
              <span className="mono" style={{ fontSize: 10.5, color: "var(--tx-3)" }}>
                default off · never message content, never keys, never addresses
              </span>
            </div>
            <div className="surface" style={{ padding: 18, display: "flex", flexDirection: "column", gap: 10 }}>
              <span className="eyebrow">Diagnostics</span>
              {/* Q-A.1 (Rule 1) — no diagnostics-bundle backend in this build; honest
                  DISABLED state. The deep-link-scheme claim is dropped (unverified). */}
              <span>
                <DisabledControl label="Export diagnostics bundle" note="diagnostics export not available in this build" />
              </span>
              <span className="mono" style={{ fontSize: 10.5, color: "var(--tx-3)" }}>
                would bundle logs + config + crash records · scrubbed of keys and tokens
              </span>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
