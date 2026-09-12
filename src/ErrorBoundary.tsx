import React from "react";
import { recordUiError } from "./shell/errorRing";
import { STORAGE_KEY } from "./shell/state";
import { DiagnosticReport } from "./components/DiagnosticReport";

/** Read the persisted telemetry consent without the store (we're in a crashed subtree). */
function telemetryConsented(): boolean {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    return raw ? Boolean(JSON.parse(raw).telemetry) : false;
  } catch {
    return false;
  }
}

/**
 * Top-level error boundary. Before this, ANY uncaught render/lifecycle error unmounted the
 * whole React tree to a blank white screen with no way back (the app had no boundary). Now a
 * throw is contained: we show the real error message + a Reload button, so a single bad
 * surface degrades gracefully AND the failing error is visible (not a silent white screen).
 *
 * NOTE: this catches JavaScript exceptions only. A WKWebView content-process crash (e.g.
 * memory pressure) is below React and cannot be caught here — if the screen still goes fully
 * blank with nothing rendered, that's a process crash, not a JS error.
 */
interface State {
  error: Error | null;
}

export class ErrorBoundary extends React.Component<{ children: React.ReactNode }, State> {
  state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  componentDidCatch(error: Error, info: React.ErrorInfo) {
    // Surface to the console (visible in the webview inspector) so the exact throw + the
    // component stack can be read off a packaged build during triage.
    console.error("[citrate-core] uncaught UI error:", error, info.componentStack);
    // WP-T.2 — record into the local error ring so a diagnostic report (only ever sent on
    // explicit consent, WP-T.4) can include it. Local only; nothing egresses here.
    recordUiError(`${error.message || String(error)}\n${info.componentStack ?? ""}`);
  }

  render() {
    const { error } = this.state;
    if (!error) return this.props.children;
    return (
      <div
        data-register="charter"
        style={{
          height: "100vh",
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          background: "var(--srf-0, #0b0f0d)",
          color: "var(--tx-1, #e8efe9)",
          fontFamily: "var(--font-sans, -apple-system, Segoe UI, Roboto, sans-serif)",
          padding: 24,
        }}
      >
        <div style={{ maxWidth: 520, display: "flex", flexDirection: "column", gap: 12 }}>
          <div style={{ fontFamily: "var(--font-display, inherit)", fontSize: 20 }}>Something went wrong on this screen.</div>
          <p style={{ fontSize: 13.5, lineHeight: 1.55, color: "var(--tx-2, #9db0a3)", margin: 0 }}>
            The rest of the app is fine — this view hit an error and stopped instead of blanking the
            whole window. Reloading returns you to where you were.
          </p>
          <pre
            className="mono"
            style={{
              fontSize: 11.5,
              whiteSpace: "pre-wrap",
              wordBreak: "break-word",
              background: "var(--srf-1, #121815)",
              border: "1px solid var(--line-1, #22302a)",
              borderRadius: 8,
              padding: "10px 12px",
              margin: 0,
              maxHeight: 180,
              overflow: "auto",
            }}
          >
            {error.message || String(error)}
          </pre>
          <div>
            <button className="btn btn-primary btn-sm" onClick={() => window.location.reload()}>
              Reload
            </button>
          </div>
          {/* WP-T.4 — on-crash prompt, gated on the telemetry toggle (default off). The crash is
              already shown above; this lets the member review + send the full scrubbed bundle. */}
          {telemetryConsented() && (
            <div style={{ borderTop: "1px solid var(--line-1, #22302a)", paddingTop: 12 }}>
              <DiagnosticReport enabled={true} />
            </div>
          )}
        </div>
      </div>
    );
  }
}
