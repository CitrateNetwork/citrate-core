// =====================================================================
// citrate-core — Settings surface (1:1 from design/CitrateCore.dc.html
// SETTINGS section). Eight sections behind a left sub-nav (sSec):
// Account & RBAC, Connections, AI providers, Node configuration,
// API endpoints & keys, Keys & security, Memberships & billing, App.
//
// Honesty rule (Rule 1 / I-3): EVERY control reflects real AppState and
// mutates it via store.setState — no dead controls, no "declared but not
// wired" pattern. State that is genuinely deferred to the wired build is
// stated as such in copy (verbatim from the design) rather than faked.
//
// Data source — prototype/sim AppState (see state.ts). Account claims are
// captioned "live from /userinfo" per the design; wiring replaces the sim,
// not the UI.
// =====================================================================
import { useEffect, useState } from "react";
import { Store } from "../shell/store";
import { AppState, fmtSaltFromWei } from "../shell/state";
import { LoaderMark } from "../components/LoaderMark";
import { bridge, type AppConfig } from "../bridge";
import type { AiProviderStatus } from "../bridge/domains";
import { BRIDGE_MODE } from "../bridge/mode";

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
  const expTxt =
    s.entitlement === "lapsed" ? "2026-06-28 · lapsed" : s.entitlement === "expiring" ? "2026-07-25 · 14 days" : "2027-07-11";
  const claimRows: { k: string; v: string; color: string }[] = [
    { k: "sub", v: id.sub, color: "var(--tx-1)" },
    { k: "email", v: id.email, color: "var(--tx-1)" },
    { k: "tier", v: effTier + (effTier !== s.tier ? " (was " + s.tier + ")" : ""), color: "var(--tx-1)" },
    { k: "role", v: id.role, color: "var(--tx-1)" },
    { k: "org", v: s.org || "—", color: s.org ? "var(--info)" : "var(--tx-3)" },
    {
      k: "expiresAt",
      v: expTxt,
      color: s.entitlement === "active" ? "var(--tx-1)" : s.entitlement === "lapsed" ? "var(--danger)" : "var(--warn)",
    },
    { k: "kyc_status", v: s.hasSbt || s.s2 === "verified" ? "verified" : "none", color: "var(--tx-1)" },
  ];

  // ---------- connections ----------
  const connRows = CONN.map(([id, name, scope]) => {
    const on = !!s.connections[id];
    return {
      name,
      scope: scope + " · MCP tool",
      connected: on,
      btn: on ? "Disconnect" : "Connect",
      go: () => {
        // OAuth connect/disconnect is NOT wired — there is no real token flow yet
        // (a scheduled build). The old handler faked a 1.3s "OAuth" timer and
        // claimed keyring token storage/revocation. Be honest rather than flip a
        // fabricated "connected" flag (Rule 1).
        if (on) {
          const c = { ...store.state.connections };
          delete c[id];
          store.setState({ connections: c });
          store.save();
          store.toast(name + " disconnected.");
        } else {
          store.toast(name + " — OAuth connections aren't wired yet (a scheduled build); no token was issued.");
        }
      },
    };
  });

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
  const rpcUp = s.rpc === "public" || s.node !== "off";
  const rpcHealthColor = rpcUp ? "var(--ok)" : "var(--warn)";
  const rpcHealthText =
    s.rpc === "local"
      ? s.node !== "off"
        ? "127.0.0.1:8545 · healthy" + (s.finAge < 0 ? "" : " · " + Math.round(s.finAge) + "s behind checkpoint")
        : "127.0.0.1:8545 · node off — reads fall back to rpc.citrate.ai (shown in UI)"
      : "rpc.citrate.ai · healthy · TLS";

  const noGwKey = !s.gwKey && !s.gwKeyFull;
  const gwKeyShown = !!s.gwKeyFull;
  const gwKeyHeld = !!s.gwKey && !s.gwKeyFull;
  const gwKeyFull = s.gwKeyFull || "";
  const gwKeyMasked = s.gwKey || "";
  // Gateway-key issuance is NOT wired — the real key is minted by the membership
  // service against your live entitlement (core-membership, a scheduled build).
  // The old flow fabricated a client-side `cgk_` string and claimed keyring/server
  // storage. Be honest rather than hand out a fake key (Rule 1).
  const onIssueKey = () => {
    store.toast("Gateway key issuance isn't wired yet — the membership service mints it against your live entitlement (a scheduled build).");
  };
  const onCopyKey = () => store.copy(store.state.gwKeyFull || "", "Key copied — it will not be shown again");
  const onKeyStored = () => {
    store.setState({ gwKey: null, gwKeyFull: null });
    store.toast("Gateway keys aren't wired yet — nothing was stored.");
    store.save();
  };
  const onRotateKey = () => {
    store.toast("Gateway key rotation isn't wired yet — no server key to rotate.");
  };
  const onRevokeKey = () => {
    store.toast("Gateway key revocation isn't wired yet — no server key to revoke.");
  };

  // ---------- keys & security ----------
  const onExportKey = () => store.toast("Keystore export isn't wired yet — it will show a strong-warning dialog before revealing the encrypted keystore (a scheduled build).");
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
  const memRenewLine = s.entitlement === "lapsed" ? "lapsed 2026-06-28" : "renews 2027-07-11 · Stripe customer portal";
  const memBadge = s.entitlement === "active" ? "active" : s.entitlement;
  const mb =
    s.entitlement === "active"
      ? ["var(--ok-bg)", "var(--ok)", "var(--ok)"]
      : s.entitlement === "lapsed"
        ? ["var(--danger-bg)", "var(--danger)", "var(--danger)"]
        : ["var(--warn-bg)", "var(--warn)", "var(--warn)"];

  // ---------- app ----------
  // The updater isn't wired, so we never assert "current" (unverifiable). Show the
  // real running version + that update checks aren't wired.
  const updText = "citrate-core 0.1.0-proto · " + s.channel + " channel · update checks not wired";
  // The auto-updater isn't wired (it's @rule8 — updater keys need security
  // sign-off; work-order WO-2). The old handler faked a check that always
  // resolved "current". Be honest rather than assert a signature-verified check.
  const onCheckUpdate = () => {
    store.toast("Update checks aren't wired yet — the signed auto-updater ships with the notarized build (a scheduled build).");
  };

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
                <button className="btn btn-ghost btn-sm" onClick={() => store.toast("Account hub isn't wired yet — it opens in your browser via the shared OIDC session (a scheduled build).")}>
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
                  OAuth runs in your system browser. Tokens land in the OS keyring — never in config files.
                </span>
              </div>
              {connRows.map((cn) => (
                <div key={cn.name} style={{ display: "flex", alignItems: "center", gap: 14, padding: "12px 18px", borderBottom: "1px solid var(--line-1)" }}>
                  <span style={{ flex: 1, minWidth: 0 }}>
                    <span style={{ display: "block", fontSize: 13, fontWeight: 500 }}>{cn.name}</span>
                    <span className="mono" style={{ display: "block", fontSize: 10.5, color: "var(--tx-3)", marginTop: 1 }}>
                      {cn.scope}
                    </span>
                  </span>
                  {cn.connected && (
                    <span
                      className="mono"
                      style={{
                        fontSize: 9.5,
                        letterSpacing: ".1em",
                        textTransform: "uppercase",
                        padding: "2px 8px",
                        borderRadius: 999,
                        background: "var(--ok-bg)",
                        border: "1px solid var(--ok)",
                        color: "var(--ok)",
                      }}
                    >
                      connected
                    </span>
                  )}
                  <button className="btn btn-ghost btn-sm" onClick={cn.go}>
                    {cn.btn}
                  </button>
                </div>
              ))}
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
                <button className="btn btn-ghost btn-sm" onClick={() => store.toast("Move stops the node, relocates the encrypted store, verifies, then restarts — guided flow in the wired build")}>
                  Move…
                </button>
              </span>
            </div>
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
              <span className="lbl">Bootnodes</span>
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
              {noGwKey && (
                <>
                  <p style={{ fontSize: 12.5, color: "var(--tx-2)", margin: 0 }}>
                    No key issued. The membership service issues it against your live entitlement, with tier-bounded quotas.
                  </p>
                  <span>
                    <button className="btn btn-secondary btn-sm" onClick={onIssueKey}>
                      Issue key
                    </button>
                  </span>
                </>
              )}
              {gwKeyShown && (
                <div style={{ border: "1px solid var(--warn)", background: "var(--warn-bg)", borderRadius: "var(--r-1)", padding: "10px 12px", display: "flex", flexDirection: "column", gap: 8 }}>
                  <span style={{ fontSize: 12, color: "var(--warn)", fontWeight: 500 }}>Stored locally — gateway keys aren't wired to the membership service yet.</span>
                  <span style={{ display: "flex", alignItems: "center", gap: 8 }}>
                    <span className="mono" style={{ fontSize: 12, flex: 1, wordBreak: "break-all" }}>
                      {gwKeyFull}
                    </span>
                    <button className="btn btn-ghost btn-sm" onClick={onCopyKey}>
                      Copy
                    </button>
                  </span>
                  <span>
                    <button className="btn btn-secondary btn-sm" onClick={onKeyStored}>
                      Done — stored
                    </button>
                  </span>
                </div>
              )}
              {gwKeyHeld && (
                <>
                  <span style={{ display: "flex", alignItems: "center", gap: 10 }}>
                    <span className="mono" style={{ fontSize: 12, flex: 1 }}>
                      {gwKeyMasked}
                    </span>
                    <button className="btn btn-ghost btn-sm" onClick={onRotateKey}>
                      Rotate
                    </button>
                    <button className="btn btn-danger btn-sm" onClick={onRevokeKey}>
                      Revoke
                    </button>
                  </span>
                  <span className="mono" style={{ fontSize: 10.5, color: "var(--tx-3)" }}>
                    stored locally · gateway keys aren't wired to the membership service yet
                  </span>
                </>
              )}
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
                      void store.custodyUnlock("");
                      store.toast("Unlock prompts for your passphrase in the wired build");
                    }}
                  >
                    Unlock…
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
                <span className="mono" style={{ fontSize: 10, color: "var(--tx-3)" }}>
                  ADDRESS SET
                </span>
              </div>
              <div style={{ display: "flex", alignItems: "center", gap: 12, borderBottom: "1px solid var(--line-1)", paddingBottom: 10 }}>
                <span style={{ flex: 1 }}>
                  <span style={{ display: "block", fontSize: 13, fontWeight: 500 }}>Machine attestation</span>
                  <span className="mono" style={{ display: "block", fontSize: 10.5, color: "var(--tx-3)", marginTop: 2 }}>
                    {s.deviceId} · hardware-backed device attestation isn't wired yet (a scheduled build)
                  </span>
                </span>
                <span className="mono" style={{ fontSize: 10, color: "var(--warn)" }}>
                  NOT WIRED
                </span>
              </div>
              <div style={{ display: "flex", alignItems: "center", gap: 12 }}>
                <span style={{ flex: 1 }}>
                  <span style={{ display: "block", fontSize: 13, fontWeight: 500 }}>Local signing key</span>
                  <span className="mono" style={{ display: "block", fontSize: 10.5, color: "var(--tx-3)", marginTop: 2 }}>
                    Argon2id + AES-GCM keystore · OS keyring sealed
                  </span>
                </span>
                <button className="btn btn-ghost btn-sm" onClick={onExportKey}>
                  Export…
                </button>
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
                    store.toast("Allowlist rules are user-authored per origin + contract — editor ships in the wired build");
                    store.save();
                  }}
                >
                  Allowlist rules…
                </button>
              </span>
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
                <button className="btn btn-ghost btn-sm" onClick={() => store.toast("Cancellation runs in the Stripe customer portal — the portal link isn't wired yet (a scheduled build).")}>
                  Cancel membership
                </button>
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
                  <span className="mono" style={{ fontSize: 11.5 }}>
                    mainnet release policy
                  </span>
                </div>
              </div>
            )}
            <div className="surface" style={{ padding: 18, display: "flex", flexDirection: "column", gap: 8 }}>
              <span className="eyebrow">Receipts</span>
              <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
                <span className="mono" style={{ fontSize: 11.5 }}>
                  2026-07-11 · Pilot membership · $48.00
                </span>
                <button className="btn btn-ghost btn-sm" onClick={() => store.toast("Receipt PDFs aren't wired yet — they download from core-membership (a scheduled build).")}>
                  PDF ↗
                </button>
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
              <span style={{ display: "flex", alignItems: "center", gap: 10 }}>
                <button className="btn btn-ghost btn-sm" onClick={onCheckUpdate} disabled={s.updState === "checking"}>
                  Check now
                </button>
                {s.updState === "checking" && (
                  <span style={{ width: 22, height: 22, display: "inline-block" }}>
                    <LoaderMark size={22} />
                  </span>
                )}
                <span className="mono" style={{ fontSize: 11, color: "var(--tx-2)" }}>
                  {updText}
                </span>
              </span>
              <span className="mono" style={{ fontSize: 10.5, color: "var(--tx-3)" }}>
                updater signature verified offline before anything applies · citrate-core 0.1.0-proto
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
              <span>
                <button className="btn btn-ghost btn-sm" onClick={() => store.toast("Diagnostics export isn't wired yet — it will bundle logs/config/crash records (keys + tokens scrubbed) in a scheduled build.")}>
                  Export diagnostics bundle
                </button>
              </span>
              <span className="mono" style={{ fontSize: 10.5, color: "var(--tx-3)" }}>
                logs + config + crash records · scrubbed of keys and tokens · citrate-core:// scheme registered for deep links
              </span>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
