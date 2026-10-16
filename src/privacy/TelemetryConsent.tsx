// =====================================================================
// HUP-S10.5 — telemetry consent screen.
//
// Off by default. Turning crash reports on goes through this screen, which lists
// exactly the fields a report carries (from telemetry-fields.json, which a Rust
// test pins to the real bundle), what is never sent, and where it would go. Even
// when on, nothing is sent in the background: each report is prepared, reviewed
// and sent by hand (DiagnosticReport, the ConsentGate).
// =====================================================================
import { useState } from "react";
import fields from "./telemetry-fields.json";

export const TELEMETRY_FIELDS = fields;

export function TelemetryConsent({ enabled, onChange }: { enabled: boolean; onChange: (on: boolean) => void }) {
  const [open, setOpen] = useState(false);

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 10 }} data-testid="telemetry-consent">
      <span style={{ display: "flex", alignItems: "center", gap: 12, flexWrap: "wrap" }}>
        <span className="mono" style={{ fontSize: 12 }} data-testid="telemetry-state">
          {enabled ? "On: crash reports only, each one reviewed before it is sent" : "Off (the default)"}
        </span>
        {enabled ? (
          <button className="btn btn-ghost btn-sm" data-testid="telemetry-off" onClick={() => onChange(false)}>
            Turn off
          </button>
        ) : (
          !open && (
            <button className="btn btn-secondary btn-sm" data-testid="telemetry-review" onClick={() => setOpen(true)}>
              Review what crash reports contain
            </button>
          )
        )}
        {enabled && !open && (
          <button className="btn btn-ghost btn-sm" data-testid="telemetry-review" onClick={() => setOpen(true)}>
            What is sent
          </button>
        )}
      </span>

      {open && (
        <div
          role="region"
          aria-label="What crash reports contain"
          data-testid="telemetry-consent-screen"
          style={{ display: "flex", flexDirection: "column", gap: 8, background: "var(--srf-1)", border: "1px solid var(--line-1)", borderRadius: 8, padding: "12px 14px" }}
        >
          <span style={{ fontSize: 13, fontWeight: 600 }}>A crash report contains exactly these fields</span>
          <ul style={{ margin: 0, paddingLeft: 18, display: "flex", flexDirection: "column", gap: 4 }}>
            {TELEMETRY_FIELDS.fields.map((f) => (
              <li key={f.key} style={{ fontSize: 12, lineHeight: 1.5 }}>
                <span className="mono">{f.key}</span>: {f.what}
              </li>
            ))}
          </ul>
          <span style={{ fontSize: 13, fontWeight: 600 }}>Never sent</span>
          <ul style={{ margin: 0, paddingLeft: 18, display: "flex", flexDirection: "column", gap: 4 }}>
            {TELEMETRY_FIELDS.neverSent.map((n) => (
              <li key={n} style={{ fontSize: 12, lineHeight: 1.5 }}>
                {n}
              </li>
            ))}
          </ul>
          <span style={{ fontSize: 12, color: "var(--tx-2)", lineHeight: 1.5 }}>
            {TELEMETRY_FIELDS.sends} Reports go to <span className="mono">{TELEMETRY_FIELDS.endpoint}</span>. That service is not live yet, so a send fails with an honest error until it is.
          </span>
          <span style={{ display: "flex", gap: 8 }}>
            {!enabled && (
              <button
                className="btn btn-primary btn-sm"
                data-testid="telemetry-consent-on"
                onClick={() => {
                  onChange(true);
                  setOpen(false);
                }}
              >
                Turn on crash reports
              </button>
            )}
            <button className="btn btn-ghost btn-sm" data-testid="telemetry-consent-close" onClick={() => setOpen(false)}>
              {enabled ? "Close" : "Keep off"}
            </button>
          </span>
        </div>
      )}
    </div>
  );
}
