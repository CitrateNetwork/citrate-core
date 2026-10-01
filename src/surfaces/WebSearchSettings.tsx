// =====================================================================
// HUP-S5.2 / S5.3: Settings › "Web search & decisions".
//
// The member's opt-ins for Hermes's web tools: private search over a local SearXNG, page reading
// (local by default, the Jina Reader only by explicit choice), and the TypeSafe Jev backend for
// fast decisions on listed sites. Core stores them (hermes_web.rs) and passes them to the Hermes
// sidecar when it starts. Everything is off or local until the member changes it, and every choice
// that sends data to a third party says so on the card.
// =====================================================================
import { useEffect, useState } from "react";
import { BRIDGE_MODE } from "../bridge/mode";
import { invoke } from "../bridge/tauri/invoke";
import {
  DEFAULT_WEB_SETTINGS,
  MAX_JEV_ORIGINS,
  isAbsolutePath,
  loadWebSettings,
  parseOrigins,
  saveWebSettings,
  sendsToThirdParty,
  type HermesWebSettings,
  type HermesWebStatus,
  type WebSettingsIo,
} from "../agent/webSearch";

const defaultIo: WebSettingsIo = {
  mode: BRIDGE_MODE === "tauri" ? "tauri" : "sim",
  invoke: <T,>(cmd: string, args?: Record<string, unknown>) => invoke<T>(cmd, args),
};

const label = { fontSize: 12.5, color: "var(--tx-1)" } as const;
const hint = { fontSize: 11.5, color: "var(--tx-3)", lineHeight: 1.45 } as const;
const input = {
  fontFamily: "var(--font-mono)",
  fontSize: 12,
  padding: "6px 8px",
  border: "1px solid var(--line-1)",
  borderRadius: "var(--r-1)",
  background: "var(--srf-0)",
  color: "var(--tx-1)",
  width: "100%",
} as const;

function PathField({
  id,
  value,
  placeholder,
  disabled,
  onChange,
}: {
  id: string;
  value: string | null;
  placeholder: string;
  disabled: boolean;
  onChange: (v: string) => void;
}) {
  const v = value ?? "";
  const bad = v.trim() !== "" && !isAbsolutePath(v);
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 3 }}>
      <input data-testid={id} style={input} value={v} placeholder={placeholder} disabled={disabled} onChange={(e) => onChange(e.target.value)} />
      {bad && (
        <span data-testid={`${id}-bad`} style={{ ...hint, color: "var(--warn-text, var(--tx-2))" }}>
          Use the full path, starting with / (or a drive letter on Windows).
        </span>
      )}
    </div>
  );
}

export function WebSearchSettings({ io = defaultIo }: { io?: WebSettingsIo }) {
  const desktop = io.mode === "tauri";
  const [status, setStatus] = useState<HermesWebStatus | null>(null);
  const [draft, setDraft] = useState<HermesWebSettings>(DEFAULT_WEB_SETTINGS);
  const [originsText, setOriginsText] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    if (!desktop) return;
    let live = true;
    loadWebSettings(io)
      .then((st) => {
        if (!live) return;
        setStatus(st);
        setDraft(st.settings);
        setOriginsText(st.settings.jevOrigins.join("\n"));
      })
      .catch((e) => live && setError(String(e)));
    return () => {
      live = false;
    };
  }, [desktop, io]);

  const parsed = parseOrigins(originsText);
  const tooMany = parsed.origins.length > MAX_JEV_ORIGINS;
  const set = (patch: Partial<HermesWebSettings>) => setDraft((d) => ({ ...d, ...patch }));
  const disabled = !desktop || saving;

  const save = async () => {
    if (parsed.bad.length || tooMany) return;
    setSaving(true);
    setError(null);
    try {
      const st = await saveWebSettings(io, { ...draft, jevOrigins: parsed.origins });
      setStatus(st);
      setDraft(st.settings);
      setOriginsText(st.settings.jevOrigins.join("\n"));
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  };

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 12 }} data-testid="web-settings">
      {!desktop && (
        <div className="surface" style={{ padding: 14 }} data-testid="web-settings-preview">
          <span style={hint}>Web search and decision settings are desktop-only. This preview shows the defaults and cannot change them.</span>
        </div>
      )}
      {status?.loadError && (
        <div className="surface" style={{ padding: 14 }} data-testid="web-settings-load-error">
          <span style={hint}>{status.loadError}</span>
        </div>
      )}

      {/* ---------- search + reading ---------- */}
      <div className="surface" style={{ padding: 18, display: "flex", flexDirection: "column", gap: 10 }}>
        <span className="eyebrow">Web search & page reading</span>
        <label style={{ display: "flex", gap: 8, alignItems: "center", ...label }}>
          <input type="checkbox" data-testid="web-search-toggle" checked={draft.searchEnabled} disabled={disabled} onChange={(e) => set({ searchEnabled: e.target.checked })} />
          Let Hermes search the web and read pages
        </label>
        <span style={hint}>
          Off by default. Search runs on a SearXNG instance on this machine, which forwards each query to the search engines it is configured for. Whatever Hermes reads on the web is treated as untrusted: after it, every action needs your approval.
        </span>
        <span style={label}>SearXNG program</span>
        <PathField id="searxng-path" value={draft.searxngPath} placeholder="/path/to/searxng-run" disabled={disabled} onChange={(v) => set({ searxngPath: v })} />
        <span style={hint} data-testid="searxng-state">
          {status?.searxngFound
            ? "Found. Hermes starts it on loopback the first time it searches."
            : "Not found. Search is not bundled yet, so web search reports that it is not installed until you point to an installed searxng-run. Reading a known URL still works."}
        </span>
        <span style={label}>How pages are read</span>
        <label style={{ display: "flex", gap: 8, alignItems: "center", ...label }}>
          <input type="radio" name="reader" data-testid="reader-local" checked={draft.reader === "local"} disabled={disabled} onChange={() => set({ reader: "local" })} />
          On this machine (default)
        </label>
        <label style={{ display: "flex", gap: 8, alignItems: "center", ...label }}>
          <input type="radio" name="reader" data-testid="reader-jina" checked={draft.reader === "jina"} disabled={disabled} onChange={() => set({ reader: "jina" })} />
          Jina Reader (third-party service)
        </label>
        {draft.reader === "jina" && (
          <>
            <span style={hint} data-testid="jina-notice">
              Every URL Hermes reads is sent to r.jina.ai, which fetches the page for you. A key is optional.
            </span>
            <PathField id="jina-key-file" value={draft.jinaKeyFile} placeholder="/path/to/jina.key (optional)" disabled={disabled} onChange={(v) => set({ jinaKeyFile: v })} />
          </>
        )}
      </div>

      {/* ---------- Jev ---------- */}
      <div className="surface" style={{ padding: 18, display: "flex", flexDirection: "column", gap: 10 }}>
        <span className="eyebrow">Fast decisions</span>
        <span style={hint}>
          Hermes makes quick choices (which button to press, which result fits) with your local model. You can let TypeSafe Jev, a third-party service, make them instead for sites you list.
        </span>
        <label style={{ display: "flex", gap: 8, alignItems: "center", ...label }}>
          <input type="checkbox" data-testid="jev-toggle" checked={draft.jevEnabled} disabled={disabled} onChange={(e) => set({ jevEnabled: e.target.checked })} />
          Use TypeSafe Jev on the sites below
        </label>
        {draft.jevEnabled && (
          <>
            <span style={hint} data-testid="jev-notice">
              For these sites the page snapshot and the question are sent to TypeSafe. Never for a site where you are signed in, and never while Hermes is attached to your own browser.
            </span>
            <span style={label}>Sites (one per line, https only)</span>
            <textarea data-testid="jev-origins" style={{ ...input, minHeight: 70 }} value={originsText} disabled={disabled} onChange={(e) => setOriginsText(e.target.value)} placeholder="https://shop.example" />
            {parsed.bad.length > 0 && (
              <span data-testid="jev-origins-bad" style={hint}>
                Not a site address: {parsed.bad.join(", ")}. Use https://host or https://host:port.
              </span>
            )}
            {tooMany && <span style={hint}>At most {MAX_JEV_ORIGINS} sites.</span>}
            <label style={{ display: "flex", gap: 8, alignItems: "center", ...label }}>
              <input type="checkbox" data-testid="jev-non-web" checked={draft.jevNonWeb} disabled={disabled} onChange={(e) => set({ jevNonWeb: e.target.checked })} />
              Also for choices that are not about a website
            </label>
            <span style={label}>TypeSafe key file</span>
            <PathField id="jev-key-file" value={draft.jevKeyFile} placeholder="/path/to/typesafe.key" disabled={disabled} onChange={(v) => set({ jevKeyFile: v })} />
            <span style={hint} data-testid="jev-key-custody">
              Keeping this key in your custody vault is pending owner sign-off. For now, point to a key file only you can read. Without a key, Jev stays off.
            </span>
          </>
        )}
      </div>

      {/* ---------- what leaves the machine ---------- */}
      <div className="surface" style={{ padding: 18, display: "flex", flexDirection: "column", gap: 8 }}>
        <span className="eyebrow">What leaves this machine</span>
        {sendsToThirdParty(draft) ? null : (
          <span style={hint} data-testid="web-local-only">
            Nothing goes to a third party with these choices (search engines still see queries when search is on).
          </span>
        )}
        <ul data-testid="web-notices" style={{ margin: 0, paddingLeft: 18, display: "flex", flexDirection: "column", gap: 4 }}>
          {(status?.notices ?? []).map((n) => (
            <li key={n} style={hint}>
              {n}
            </li>
          ))}
        </ul>
        <span style={hint}>Changes apply the next time Hermes starts.</span>
        <div style={{ display: "flex", gap: 10, alignItems: "center" }}>
          <button className="btn btn-secondary btn-sm" data-testid="web-save" disabled={disabled || parsed.bad.length > 0 || tooMany} onClick={() => void save()}>
            {saving ? "Saving…" : "Save"}
          </button>
          {error && (
            <span data-testid="web-error" style={hint}>
              {error}
            </span>
          )}
        </div>
      </div>
    </div>
  );
}
