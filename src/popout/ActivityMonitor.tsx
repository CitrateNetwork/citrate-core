// =====================================================================
// citrate-core — Activity monitor pop-out view (HUP-S7.6, US-7.4)
//
// A pure view over one MonitorSnapshot ("why am I waiting"): model and tier, the context meter,
// the current step and tool calls, elapsed time, spend, the agent's worker processes (HUP-S1.9),
// and a Stop button that is always visible.
// Every unknown value is shown as "unknown" with its reason; nothing is invented (Rule 1).
// Styled only with the register tokens (--srf/--tx/--line), so it reads in the dark register.
// =====================================================================
import type { CSSProperties, ReactNode } from "react";
import { formatElapsed, workerLine, type MonitorSnapshot } from "./monitorSnapshot";

const fmt = (n: number) => n.toLocaleString("en-US");

const label: CSSProperties = { fontSize: 9.5, letterSpacing: ".13em", textTransform: "uppercase", color: "var(--tx-3)" };
const value: CSSProperties = { fontSize: 13, color: "var(--tx-1)" };
const note: CSSProperties = { fontSize: 11, color: "var(--tx-2)" };

function Row({ name, testId, children, hint }: { name: string; testId: string; children: ReactNode; hint?: string }) {
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 3, padding: "8px 0", borderBottom: "1px solid var(--line-1)" }}>
      <span className="mono" style={label}>{name}</span>
      <span data-testid={testId} style={value}>
        {children}
        {hint ? <span style={{ ...note, display: "block" }}>{hint}</span> : null}
      </span>
    </div>
  );
}

const TOOL_STATE_COLOR: Record<string, string> = {
  running: "var(--accent-text)",
  done: "var(--ok)",
  failed: "var(--danger)",
  abandoned: "var(--tx-3)",
};

export function ActivityMonitor({ snapshot, now, onStop }: { snapshot: MonitorSnapshot; now: number; onStop: () => void }) {
  const { model, provider, tier, context, turn, spend, workers } = snapshot;
  const running = turn.state === "running";
  const ctxWindow = context.windowTokens !== null ? `${fmt(context.windowTokens)} tokens` : "unknown";
  const ctxUsed = context.usedTokens !== null ? `${fmt(context.usedTokens)} used` : "used: unknown";
  const spendText = spend.amount !== null ? `${spend.amount} ${spend.unit}` : "unknown";
  const endAt = turn.state === "idle" ? turn.endedAt ?? now : now;

  return (
    <div
      data-testid="activity-monitor"
      data-register="instrument"
      style={{ minHeight: "100vh", boxSizing: "border-box", padding: "14px 16px", background: "var(--srf-0)", color: "var(--tx-1)", fontFamily: "var(--font-sans)", display: "flex", flexDirection: "column", gap: 4 }}
    >
      <div style={{ display: "flex", alignItems: "center", gap: 10, paddingBottom: 8 }}>
        <span style={{ fontSize: 14, fontWeight: 500, flex: 1 }}>Activity monitor</span>
        <button
          className="btn btn-sm"
          data-testid="mon-stop"
          onClick={onStop}
          disabled={!running}
          title={running ? "Stop the running turn" : turn.state === "stopping" ? "Stopping the turn" : "Nothing is running"}
          style={{ color: running ? "var(--danger)" : "var(--tx-3)", borderColor: running ? "var(--danger)" : "var(--line-2)", background: "transparent" }}
        >
          {turn.state === "stopping" ? "Stopping…" : "Stop"}
        </button>
      </div>

      <div role="status" aria-live="polite" data-testid="mon-why" style={{ fontSize: 13, color: "var(--tx-1)", padding: "8px 10px", border: "1px solid var(--line-2)", borderRadius: 8, background: "var(--srf-1)" }}>
        {turn.why}
      </div>

      <Row name="Model" testId="mon-model" hint={provider.label}>
        {model.label}
      </Row>
      <Row name="Tier" testId="mon-tier" hint={tier === null ? "the hardware check has not answered" : undefined}>
        {tier ?? "unknown"}
      </Row>
      <Row name="Context" testId="mon-ctx" hint={`${context.windowNote}; ${context.usedNote}`}>
        {ctxUsed} of {ctxWindow}
      </Row>
      <Row name="Elapsed" testId="mon-elapsed-row">
        <span data-testid="mon-elapsed">{formatElapsed(turn.startedAt, endAt)}</span>
      </Row>
      <Row name="Step" testId="mon-step" hint={turn.step === null && turn.state !== "idle" ? "this provider does not report steps" : undefined}>
        {turn.step === null ? (turn.state === "idle" ? "none" : "unknown") : String(turn.step)}
      </Row>
      <Row name="Spend this turn" testId="mon-spend" hint={spend.note}>
        {spendText}
      </Row>

      <div style={{ paddingTop: 8 }}>
        <span className="mono" style={label}>Tool calls</span>
        {turn.tools.length === 0 ? (
          <div style={{ ...note, paddingTop: 4 }}>None this turn.</div>
        ) : (
          <ul style={{ listStyle: "none", margin: 0, padding: "4px 0 0", display: "flex", flexDirection: "column", gap: 4 }}>
            {turn.tools.map((t) => (
              <li key={t.id + ":" + t.startedAt} data-testid="mon-tool-row" style={{ display: "flex", gap: 8, fontSize: 12, color: "var(--tx-1)" }}>
                <span className="mono" style={{ flex: 1, minWidth: 0, overflowWrap: "anywhere" }}>{t.name}</span>
                <span className="mono" style={{ color: TOOL_STATE_COLOR[t.state] ?? "var(--tx-2)" }}>{t.state}</span>
                <span className="mono" style={{ color: "var(--tx-3)" }}>{formatElapsed(t.startedAt, t.endedAt ?? now)}</span>
              </li>
            ))}
          </ul>
        )}
      </div>
      <div style={{ paddingTop: 8 }} data-testid="mon-workers">
        <span className="mono" style={label}>Worker processes</span>
        <div style={{ ...note, paddingTop: 4 }}>{workers.note}</div>
        {workers.rows && workers.rows.length > 0 ? (
          <ul style={{ listStyle: "none", margin: 0, padding: "4px 0 0", display: "flex", flexDirection: "column", gap: 4 }}>
            {workers.rows.map((w) => (
              <li key={w.kind} data-testid="mon-worker-row" style={{ display: "flex", gap: 8, fontSize: 12, color: "var(--tx-1)" }}>
                <span className="mono" style={{ minWidth: 72 }}>{w.kind}</span>
                <span
                  style={{
                    flex: 1,
                    minWidth: 0,
                    overflowWrap: "anywhere",
                    color: w.state === "failed" || w.healthy === false ? "var(--danger)" : w.state === "running" ? "var(--tx-1)" : "var(--tx-2)",
                  }}
                >
                  {workerLine(w)}
                </span>
              </li>
            ))}
          </ul>
        ) : null}
      </div>
      {turn.state === "idle" && turn.outcome ? (
        <div style={{ ...note, paddingTop: 8 }}>Last turn: {turn.outcome === "stopped" ? "stopped by you" : turn.outcome}.</div>
      ) : null}
    </div>
  );
}
