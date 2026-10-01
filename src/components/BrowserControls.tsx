// =====================================================================
// citrate-core — the main window's controls for Hermes's browser (HUP-S5.1 + S5.6)
//
// The browser runs in the Hermes sidecar and is off by default; while it is off this renders
// nothing, so the Agent pane is unchanged for members. When it is on, this is where the member:
// - decides on the browser action that is waiting (after Hermes reads a page, every click, typed
//   entry or new address waits here; no decision means no action);
// - consents to an origin in attach mode (a banking, email or health site is excluded by default
//   and needs a separate "include" tick for that one site);
// - attaches Hermes to their own Chrome, only after ticking consent for this session, or detaches;
// - stops the browser (latched) or resumes it, and opens the Browser pop-out to watch.
// Every call goes through the main-window-only Rust commands; errors are shown, never swallowed.
// =====================================================================
import { useCallback, useEffect, useState, type CSSProperties } from "react";
import { BROWSER_OFF, parseBrowserStatus, type BrowserState } from "../popout/browserView";
import type { BrowserApi } from "../popout/browserApi";

/** How often the status is re-read while the browser is on. */
export const BROWSER_STATUS_POLL_MS = 2_000;
/** How often while it is off (the default): a cheap local check, kept infrequent. */
export const BROWSER_STATUS_IDLE_POLL_MS = 15_000;
/** The remote debugging port suggested for attach (Chrome's usual one). */
export const DEFAULT_ATTACH_PORT = 9222;

const box: CSSProperties = { display: "flex", flexDirection: "column", gap: 8, padding: "10px 16px", borderBottom: "1px solid var(--line-1)", background: "var(--srf-1)", fontSize: 12 };
const row: CSSProperties = { display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" };
const note: CSSProperties = { fontSize: 11.5, color: "var(--tx-2)", lineHeight: 1.45 };
const card: CSSProperties = { display: "flex", flexDirection: "column", gap: 6, padding: "8px 10px", border: "1px solid var(--line-2)", borderRadius: 6, background: "var(--srf-0)" };
const chip: CSSProperties = { fontSize: 10, letterSpacing: ".06em", textTransform: "uppercase", color: "var(--tx-2)", background: "transparent", border: "1px solid var(--line-1)", borderRadius: 999, padding: "3px 9px", cursor: "pointer" };

const errText = (e: unknown) => (e instanceof Error ? e.message : String(e));

export function BrowserControls({ api, onOpen }: { api: BrowserApi; onOpen: () => void }) {
  const [state, setState] = useState<BrowserState>(BROWSER_OFF);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [includeSensitive, setIncludeSensitive] = useState(false);
  const [attachConsent, setAttachConsent] = useState(false);
  const [port, setPort] = useState(String(DEFAULT_ATTACH_PORT));

  const refresh = useCallback(async (): Promise<BrowserState> => {
    let next: BrowserState;
    try {
      next = parseBrowserStatus(await api.status());
    } catch {
      next = BROWSER_OFF;
    }
    setState(next);
    return next;
  }, [api]);

  useEffect(() => {
    let alive = true;
    let timer: ReturnType<typeof setTimeout> | null = null;
    const run = async () => {
      if (!alive) return;
      const s = await refresh();
      if (alive) timer = setTimeout(() => void run(), s.enabled ? BROWSER_STATUS_POLL_MS : BROWSER_STATUS_IDLE_POLL_MS);
    };
    void run();
    return () => {
      alive = false;
      if (timer !== null) clearTimeout(timer);
    };
  }, [refresh]);

  // A new consent request starts with the include box cleared.
  const consentOrigin = state.consentNeeded?.origin ?? null;
  useEffect(() => setIncludeSensitive(false), [consentOrigin]);

  const run = async (f: () => Promise<unknown>) => {
    setBusy(true);
    setError(null);
    try {
      await f();
    } catch (e) {
      setError(errText(e));
    } finally {
      setBusy(false);
      await refresh();
    }
  };

  if (!state.enabled) return null;

  const pending = state.pendingAction;
  const need = state.consentNeeded;
  const needLabel = need?.category ? state.excludedCategories.find((c) => c.id === need.category)?.label ?? need.category : null;
  const portNum = Number(port);
  const portOk = Number.isInteger(portNum) && portNum >= 1024 && portNum <= 65535;
  const modeText = state.mode === "managed" ? "managed browser" : state.mode === "attached" ? `your Chrome (port ${state.attachPort ?? "?"})` : "idle";

  return (
    <div data-testid="browser-controls" style={box}>
      <div style={row}>
        <span style={{ fontWeight: 500 }}>Browser</span>
        <span className="mono" style={{ fontSize: 10, letterSpacing: ".08em", color: "var(--tx-3)", textTransform: "uppercase" }}>
          {state.stopped ? "stopped" : modeText}
        </span>
        <span style={{ flex: 1 }} />
        <button className="mono" data-testid="browser-open" style={chip} onClick={onOpen} title="Watch Hermes's browser in its own window">
          Watch
        </button>
        {state.stopped ? (
          <button className="btn btn-sm" data-testid="browser-resume" disabled={busy} onClick={() => void run(() => api.resume())}>
            Resume
          </button>
        ) : (
          <button className="btn btn-sm" data-testid="browser-stop-main" disabled={busy || state.mode === "off"} onClick={() => void run(() => api.stop())} style={{ color: "var(--danger)", borderColor: "var(--danger)", background: "transparent" }}>
            Stop
          </button>
        )}
      </div>

      {state.chromium?.state === "not_installed" && state.mode === "off" ? (
        <div style={note}>No Chromium is installed. The managed browser arrives with the component updater; until then Hermes uses Chrome, Chromium, Edge or Brave if one is installed.</div>
      ) : null}

      {pending ? (
        <div data-testid="browser-pending" style={{ ...card, borderColor: "var(--accent)" }}>
          <span>
            <strong>Hermes wants to:</strong> <span data-testid="browser-pending-summary">{pending.summary}</span>
          </span>
          {pending.reason ? <span style={note}>{pending.reason}.</span> : null}
          <div style={row}>
            <button className="btn btn-sm" data-testid="browser-allow" disabled={busy} onClick={() => void run(() => api.decide(pending.id, true))}>
              Allow once
            </button>
            <button className="btn btn-sm" data-testid="browser-deny" disabled={busy} onClick={() => void run(() => api.decide(pending.id, false))}>
              Deny
            </button>
          </div>
        </div>
      ) : null}

      {need ? (
        <div data-testid="browser-consent-needed" style={card}>
          {needLabel ? (
            <>
              <span>
                <strong>{need.origin}</strong> is a {needLabel} site. Sites like this are excluded by default because Hermes would act with your signed-in session there.
              </span>
              <label style={{ ...row, fontSize: 12 }}>
                <input type="checkbox" data-testid="browser-include-sensitive" checked={includeSensitive} onChange={(e) => setIncludeSensitive(e.target.checked)} />
                Include this one site for this session anyway
              </label>
            </>
          ) : (
            <span>
              <strong>{need.origin}</strong> needs your consent before Hermes can use it in your Chrome. Consent lasts until you detach.
            </span>
          )}
          <div style={row}>
            <button
              className="btn btn-sm"
              data-testid="browser-allow-origin"
              disabled={busy || (needLabel !== null && !includeSensitive)}
              onClick={() => void run(() => api.origin(need.origin, true, needLabel !== null && includeSensitive))}
            >
              Allow this site
            </button>
          </div>
        </div>
      ) : null}

      {state.mode === "attached" ? (
        <div style={card}>
          <span style={note}>Hermes works in its own tab in your Chrome and only on the sites you allowed:</span>
          {state.consentedOrigins.length === 0 ? <span style={note}>None yet.</span> : null}
          {state.consentedOrigins.map((o, i) => (
            <div key={o} style={row}>
              <span className="mono" style={{ fontSize: 11 }}>{o}</span>
              <button className="mono" style={chip} data-testid={`browser-revoke-${i}`} disabled={busy} onClick={() => void run(() => api.origin(o, false, false))}>
                Revoke
              </button>
            </div>
          ))}
          <div style={row}>
            <button className="btn btn-sm" data-testid="browser-detach" disabled={busy} onClick={() => void run(() => api.detach())}>
              Detach from my Chrome
            </button>
          </div>
        </div>
      ) : (
        <details>
          <summary style={{ cursor: "pointer", color: "var(--tx-2)" }}>Use my own Chrome instead</summary>
          <div style={{ ...card, marginTop: 6 }}>
            <span style={note}>
              Start Chrome with remote debugging on (for example <span className="mono">--remote-debugging-port={DEFAULT_ATTACH_PORT}</span>) and a separate profile (<span className="mono">--user-data-dir</span>). Citrate connects on this computer only. Hermes opens its own tab, every site needs your consent, and banking, email and health sites stay excluded unless you include one.
            </span>
            <label style={{ ...row, fontSize: 12 }}>
              Port
              <input aria-label="Remote debugging port" data-testid="browser-attach-port" value={port} inputMode="numeric" onChange={(e) => setPort(e.target.value.replace(/[^0-9]/g, "").slice(0, 5))} style={{ width: 70 }} />
            </label>
            <label style={{ ...row, fontSize: 12 }}>
              <input type="checkbox" data-testid="browser-attach-consent" checked={attachConsent} onChange={(e) => setAttachConsent(e.target.checked)} />
              I consent to Hermes opening a tab in my Chrome for this session
            </label>
            <div style={row}>
              <button
                className="btn btn-sm"
                data-testid="browser-attach"
                disabled={busy || !attachConsent || !portOk}
                onClick={() => void run(async () => {
                  await api.attach(portNum, true);
                  setAttachConsent(false);
                })}
              >
                Attach
              </button>
            </div>
          </div>
        </details>
      )}

      {error ? (
        <div data-testid="browser-error" role="alert" style={{ ...note, color: "var(--danger)" }}>
          {error}
        </div>
      ) : null}
    </div>
  );
}
