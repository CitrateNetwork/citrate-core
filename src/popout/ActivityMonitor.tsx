// =====================================================================
// citrate-core — Activity monitor pop-out view (HUP-S7.6, US-7.4)
//
// A pure view over one MonitorSnapshot ("why am I waiting"): model and tier, the context meter,
// the current step and tool calls, elapsed time, spend, and a Stop button that is always visible.
// Every unknown value is shown as "unknown" with its reason; nothing is invented (Rule 1).
// Styled only with the register tokens (--srf/--tx/--line), so it reads in the dark register.
// HUP-S2.9: with an undo panel it also lists the agent session's recent file changes, with Undo for
// each and Undo all. The pop-out only asks; the main window runs the undo and sends the result.
// =====================================================================
import type { CSSProperties, ReactNode } from "react";
import { formatElapsed, type MonitorSnapshot } from "./monitorSnapshot";
import type { UndoPanel } from "./undoPanel";

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

const STEP_STATUS_TEXT: Record<string, string> = {
  committed: "changed",
  interrupted: "may have changed",
  prepared: "in progress",
  undone: "undone",
};

function UndoSection({ panel, onUndo }: { panel: UndoPanel; onUndo: (session: string, seq: number | null) => void }) {
  const session = panel.session;
  const canUndo = panel.enabled && session !== null && !panel.busy;
  const open = panel.steps.filter((st) => st.status !== "undone" && st.status !== "prepared").length;
  return (
    <div data-testid="mon-undo" style={{ paddingTop: 10 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
        <span className="mono" style={{ ...label, flex: 1 }}>File changes</span>
        {panel.enabled && session !== null && panel.steps.length > 0 ? (
          <button
            className="btn btn-sm"
            data-testid="mon-undo-all"
            disabled={!canUndo || open === 0}
            onClick={() => onUndo(session, null)}
            title="Undo every change the agent made in this session"
          >
            Undo all
          </button>
        ) : null}
      </div>
      {!panel.enabled || session === null ? (
        <div data-testid="mon-undo-note" style={{ ...note, paddingTop: 4 }}>{panel.note ?? "Undo is not available."}</div>
      ) : panel.steps.length === 0 ? (
        <div data-testid="mon-undo-note" style={{ ...note, paddingTop: 4 }}>No file changes in this session.</div>
      ) : (
        <ul style={{ listStyle: "none", margin: 0, padding: "4px 0 0", display: "flex", flexDirection: "column", gap: 4 }}>
          {panel.steps.map((st) => (
            <li key={st.seq} data-testid="mon-undo-row" style={{ display: "flex", alignItems: "center", gap: 8, fontSize: 12, color: "var(--tx-1)" }}>
              <span className="mono" style={{ color: "var(--tx-3)" }}>#{st.seq}</span>
              <span className="mono" style={{ flex: 1, minWidth: 0, overflowWrap: "anywhere" }}>{st.paths.join(", ")}</span>
              <span className="mono" style={{ color: st.status === "undone" ? "var(--tx-3)" : "var(--tx-2)" }}>{STEP_STATUS_TEXT[st.status] ?? st.status}</span>
              <button
                className="btn btn-sm"
                data-testid="mon-undo-step"
                disabled={!canUndo || st.status === "undone" || st.status === "prepared"}
                onClick={() => onUndo(session, st.seq)}
              >
                Undo
              </button>
            </li>
          ))}
        </ul>
      )}
      {panel.busy ? <div style={{ ...note, paddingTop: 4 }}>Undoing…</div> : null}
      {panel.last ? (
        <div data-testid="mon-undo-last" role={panel.last.ok ? "status" : "alert"} style={{ ...note, paddingTop: 4, color: panel.last.ok ? "var(--tx-2)" : "var(--danger)" }}>
          {panel.last.text}
        </div>
      ) : null}
    </div>
  );
}

export function ActivityMonitor({
  snapshot,
  now,
  onStop,
  undo,
  onUndo,
}: {
  snapshot: MonitorSnapshot;
  now: number;
  onStop: () => void;
  undo?: UndoPanel | null;
  onUndo?: (session: string, seq: number | null) => void;
}) {
  const { model, provider, tier, context, turn, spend } = snapshot;
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
      {turn.state === "idle" && turn.outcome ? (
        <div style={{ ...note, paddingTop: 8 }}>Last turn: {turn.outcome === "stopped" ? "stopped by you" : turn.outcome}.</div>
      ) : null}
      {undo && onUndo ? <UndoSection panel={undo} onUndo={onUndo} /> : null}
    </div>
  );
}
