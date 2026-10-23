// =====================================================================
// 0.5.2 Join — the one-time note for members who turned telemetry on before 0.5.2.
//
// Their consent was given under a disclosure that did not list `joinSteps`. Until they read this
// note and keep telemetry on, no join step is recorded for them (joinFunnel.funnelConsented).
// Members who opt in on 0.5.2 or later saw `joinSteps` on the normal consent screen and never
// see this. Data source (Rule 7): the telemetry toggle and its disclosure-version marker in app
// state; the field text and step list come from telemetry-fields.json.
// =====================================================================
import fields from "./telemetry-fields.json";
import { needsJoinStepsNote } from "./joinFunnel";

const joinStepsField = fields.fields.find((f) => f.key === "joinSteps");

export function JoinStepsNote({
  telemetry,
  consentVersion,
  onAcknowledge,
  onTurnOff,
}: {
  telemetry: boolean;
  consentVersion: number | null | undefined;
  onAcknowledge: () => void;
  onTurnOff: () => void;
}) {
  if (!needsJoinStepsNote(telemetry, consentVersion)) return null;
  return (
    <div
      role="dialog"
      aria-label="Crash reports now include join steps"
      data-testid="join-steps-note"
      style={{
        position: "fixed",
        right: 16,
        bottom: 16,
        maxWidth: 440,
        zIndex: 60,
        display: "flex",
        flexDirection: "column",
        gap: 8,
        background: "var(--srf-1)",
        border: "1px solid var(--line-2)",
        borderRadius: 10,
        padding: "14px 16px",
        boxShadow: "0 8px 24px rgba(0,0,0,.18)",
      }}
    >
      <span style={{ fontSize: 13, fontWeight: 600 }}>Crash reports now include one more field</span>
      <span style={{ fontSize: 12, lineHeight: 1.5, color: "var(--tx-2)" }}>
        You turned on crash reports before this version. Reports now also include{" "}
        <span className="mono">joinSteps</span>: {joinStepsField?.what}
      </span>
      <span style={{ fontSize: 12, lineHeight: 1.5 }}>
        Possible steps: <span className="mono">{fields.joinSteps.join(", ")}</span>
      </span>
      <span style={{ fontSize: 12, lineHeight: 1.5 }}>Never included:</span>
      <ul style={{ margin: 0, paddingLeft: 18, display: "flex", flexDirection: "column", gap: 2 }}>
        {fields.joinStepsNeverIncluded.map((n) => (
          <li key={n} style={{ fontSize: 12, lineHeight: 1.5 }}>
            {n}
          </li>
        ))}
      </ul>
      <span style={{ fontSize: 12, lineHeight: 1.5, color: "var(--tx-2)" }}>
        Nothing is recorded until you choose below. As before, nothing is sent until you review a report and press Send.
      </span>
      <span style={{ display: "flex", gap: 8 }}>
        <button className="btn btn-primary btn-sm" data-testid="join-steps-note-ok" onClick={onAcknowledge}>
          Keep crash reports on
        </button>
        <button className="btn btn-ghost btn-sm" data-testid="join-steps-note-off" onClick={onTurnOff}>
          Turn off crash reports
        </button>
      </span>
    </div>
  );
}
