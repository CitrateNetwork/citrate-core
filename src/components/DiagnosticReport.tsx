// Telemetry WP-T.4 — the review + send control. Reusable in Settings and the on-crash prompt.
//
// The ConsentGate (WP-T.1) in one component: gated on `enabled` (the telemetry toggle); the
// member PREPARES a report (assembled + scrubbed locally, no network), REVIEWS the exact JSON,
// and only an explicit "Send this report" click egresses it (one pinned POST). Discarding sends
// nothing. What is sent is exactly what was reviewed (INV-Consent-3).
import { useState } from "react";
import { bridge } from "../bridge";
import type { DiagnosticBundle } from "../bridge/domains";
import { getUiErrors, clearUiErrors } from "../shell/errorRing";

export function DiagnosticReport({ enabled, toast }: { enabled: boolean; toast?: (m: string) => void }) {
  const [bundle, setBundle] = useState<DiagnosticBundle | null>(null);
  const [busy, setBusy] = useState(false);
  const [sent, setSent] = useState(false);
  const say = toast ?? ((m: string) => console.log("[diagnostics]", m));

  if (!enabled) {
    return (
      <span className="mono" style={{ fontSize: 10.5, color: "var(--tx-3)", lineHeight: 1.6 }}>
        Turn on “crash reports only” above to prepare and send a diagnostic report. It’s never sent
        without your explicit review + send.
      </span>
    );
  }

  if (sent) {
    return (
      <span className="mono" style={{ fontSize: 11, color: "var(--ok)", lineHeight: 1.6 }}>
        Report sent. Thank you — it’s anonymous and helps us fix crashes.
      </span>
    );
  }

  const prepare = async () => {
    setBusy(true);
    try {
      setBundle(await bridge.telemetry.bundle(getUiErrors()));
    } catch (e) {
      say("Couldn’t prepare a report — " + (e instanceof Error ? e.message : String(e)));
    } finally {
      setBusy(false);
    }
  };

  const send = async () => {
    if (!bundle) return;
    setBusy(true);
    try {
      await bridge.telemetry.send(JSON.stringify(bundle)); // sends exactly what was reviewed
      clearUiErrors();
      setBundle(null);
      setSent(true);
    } catch (e) {
      say("Couldn’t send the report — " + (e instanceof Error ? e.message : String(e)));
    } finally {
      setBusy(false);
    }
  };

  if (!bundle) {
    return (
      <div style={{ display: "flex", flexDirection: "column", gap: 6, alignItems: "flex-start" }}>
        <button className="btn btn-secondary btn-sm" onClick={prepare} disabled={busy}>
          {busy ? "Preparing…" : "Prepare a diagnostic report"}
        </button>
        <span className="mono" style={{ fontSize: 10, color: "var(--tx-3)", lineHeight: 1.5 }}>
          assembled + scrubbed on your device · you review it before anything is sent
        </span>
      </div>
    );
  }

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
      <span className="mono" style={{ fontSize: 10.5, color: "var(--tx-2)" }}>
        Review — this is exactly what would be sent (scrubbed of paths, addresses, emails, tokens):
      </span>
      <pre
        className="mono"
        style={{ fontSize: 10.5, whiteSpace: "pre-wrap", wordBreak: "break-word", background: "var(--srf-1)", border: "1px solid var(--line-1)", borderRadius: 8, padding: "10px 12px", margin: 0, maxHeight: 260, overflow: "auto" }}
      >
        {JSON.stringify(bundle, null, 2)}
      </pre>
      <div style={{ display: "flex", gap: 8 }}>
        <button className="btn btn-primary btn-sm" onClick={send} disabled={busy}>
          {busy ? "Sending…" : "Send this report"}
        </button>
        <button className="btn btn-ghost btn-sm" onClick={() => setBundle(null)} disabled={busy}>
          Discard
        </button>
      </div>
    </div>
  );
}
