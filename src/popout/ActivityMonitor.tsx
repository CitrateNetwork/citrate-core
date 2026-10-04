// =====================================================================
// citrate-core — Activity monitor pop-out view (HUP-S7.6, US-7.4)
//
// A pure view over one MonitorSnapshot ("why am I waiting"): model and tier, the context meter,
// the current step and tool calls, elapsed time, spend, the agent's worker processes (HUP-S1.9),
// and a Stop button that is always visible.
// Every unknown value is shown as "unknown" with its reason; nothing is invented (Rule 1).
// Styled only with the register tokens (--srf/--tx/--line), so it reads in the dark register.
// HUP-S10.6 (a11y): a <main> landmark named by its <h1>; each fact is a group named by its label;
// the tool-call list is named; Stop is first in the tab order and its accessible name says what it
// does (or why it cannot be pressed). The "why" line is the only live region: the elapsed clock
// ticks every second and must not be announced each time.
// HUP-S2.9: with an undo panel it also lists the agent session's recent file changes, with Undo for
// each and Undo all. The pop-out only asks; the main window runs the undo and sends the result.
// HUP-S10.3: the scheduled daemons, with Pause / Resume and Stop for the run in flight.
// =====================================================================
import { useId, type CSSProperties, type ReactNode } from "react";
import { formatElapsed, workerLine, type DaemonsSection, type MonitorSnapshot } from "./monitorSnapshot";
import type { UndoPanel } from "./undoPanel";
import type { RunRow } from "../shell/slices/turnActivity";

const fmt = (n: number) => n.toLocaleString("en-US");

const label: CSSProperties = { fontSize: 9.5, letterSpacing: ".13em", textTransform: "uppercase", color: "var(--tx-3)" };
const value: CSSProperties = { fontSize: 13, color: "var(--tx-1)" };
const note: CSSProperties = { fontSize: 11, color: "var(--tx-2)" };

function Row({ name, testId, children, hint }: { name: string; testId: string; children: ReactNode; hint?: string }) {
  const id = useId();
  return (
    <div role="group" aria-labelledby={id} style={{ display: "flex", flexDirection: "column", gap: 3, padding: "8px 0", borderBottom: "1px solid var(--line-1)" }}>
      <span id={id} className="mono" style={label}>{name}</span>
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
  const headingId = useId();
  return (
    <div data-testid="mon-undo" style={{ paddingTop: 10 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
        <span id={headingId} className="mono" style={{ ...label, flex: 1 }}>
          File changes
        </span>
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
        <ul aria-labelledby={headingId} style={{ listStyle: "none", margin: 0, padding: "4px 0 0", display: "flex", flexDirection: "column", gap: 4 }}>
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

/** HUP-S2.2 — a command run's outcome in words, with its exit code when it has one. */
function runStatusText(r: RunRow): string {
  if (r.timedOut) return "timed out";
  const words: Record<string, string> = { completed: "done", declined: "declined by you", refused: "refused", failed: "failed", not_installed: "not installed" };
  const base = words[r.status] ?? r.status;
  return r.exitCode === null ? base : `${base}, exit ${r.exitCode}`;
}

const runDuration = (ms: number) => (ms < 1000 ? `${ms} ms` : formatElapsed(0, ms));

const clock = (ms: number) => new Date(ms).toLocaleString(undefined, { weekday: "short", hour: "2-digit", minute: "2-digit" });

/** HUP-S10.3 — scheduled daemons: status, today's budget use, next run, and Pause / Resume / Stop. */
function Daemons({ d, onPause, onStopRun }: { d: DaemonsSection; onPause?: (id: string, paused: boolean) => void; onStopRun?: () => void }) {
  const running = d.rows.some((r) => r.running);
  const headingId = useId();
  return (
    <div data-testid="mon-daemons" style={{ paddingTop: 10 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
        <span id={headingId} className="mono" style={{ ...label, flex: 1 }}>
          Daemons{d.allPaused ? " · all paused" : ""}
        </span>
        {running && onStopRun ? (
          <button className="btn btn-sm" data-testid="mon-daemon-stop" onClick={onStopRun} style={{ color: "var(--danger)", borderColor: "var(--danger)", background: "transparent" }}>
            Stop the daemon run
          </button>
        ) : null}
      </div>
      {d.blocked ? <div style={{ ...note, paddingTop: 4 }}>Runs are held: {d.blocked}.</div> : null}
      {d.error ? <div role="alert" style={{ ...note, color: "var(--danger)", paddingTop: 4 }}>{d.error}</div> : null}
      {d.rows.length === 0 ? (
        <div style={{ ...note, paddingTop: 4 }}>No daemons. Create one on the Hermes home.</div>
      ) : (
        <ul aria-labelledby={headingId} style={{ listStyle: "none", margin: 0, padding: "4px 0 0", display: "flex", flexDirection: "column", gap: 6 }}>
          {d.rows.map((r) => (
            <li key={r.id} data-testid="mon-daemon-row" style={{ display: "flex", flexDirection: "column", gap: 2, fontSize: 12, borderBottom: "1px solid var(--line-1)", paddingBottom: 6 }}>
              <span style={{ display: "flex", gap: 8, alignItems: "center" }}>
                <span style={{ flex: 1, minWidth: 0, overflowWrap: "anywhere" }}>{r.name}</span>
                <span className="mono" style={{ color: r.running ? "var(--accent-text)" : "var(--tx-2)" }}>{r.status}</span>
                {onPause ? (
                  <button
                    className="btn btn-sm"
                    data-testid="mon-daemon-pause"
                    aria-label={(r.paused ? "Resume " : "Pause ") + r.name}
                    onClick={() => onPause(r.id, !r.paused)}
                    disabled={d.allPaused}
                  >
                    {r.paused ? "Resume" : "Pause"}
                  </button>
                ) : null}
              </span>
              <span className="mono" style={{ color: "var(--tx-3)" }}>
                {r.runsToday} of {r.maxRuns} runs · {fmt(r.tokensToday)} of {fmt(r.maxTokens)} tokens (estimated) · spend 0 SALT
              </span>
              <span style={note}>
                {r.nextRunAt !== null ? "Next: " + clock(r.nextRunAt) : "No run scheduled"}
                {r.lastOutcome ? " · last: " + r.lastOutcome.replace("_", " ") : ""}
                {r.lastNote ? " · " + r.lastNote : ""}
              </span>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

export function ActivityMonitor({
  snapshot,
  now,
  onStop,
  undo,
  onUndo,
  onPauseDaemon,
  onStopDaemon,
}: {
  snapshot: MonitorSnapshot;
  now: number;
  onStop: () => void;
  undo?: UndoPanel | null;
  onUndo?: (session: string, seq: number | null) => void;
  onPauseDaemon?: (id: string, paused: boolean) => void;
  onStopDaemon?: () => void;
}) {
  const { model, provider, tier, context, turn, spend, workers } = snapshot;
  const running = turn.state === "running";
  const ctxWindow = context.windowTokens !== null ? `${fmt(context.windowTokens)} tokens` : "unknown";
  const ctxUsed = context.usedTokens !== null ? `${fmt(context.usedTokens)} used` : "used: unknown";
  const spendText = spend.amount !== null ? `${spend.amount} ${spend.unit}` : "unknown";
  const endAt = turn.state === "idle" ? turn.endedAt ?? now : now;
  const headingId = useId();
  const toolsId = useId();
  const workersId = useId();
  const runsId = useId();
  const runs = turn.runs ?? [];
  const stopName = running ? "Stop the running turn" : turn.state === "stopping" ? "Stopping the turn" : "Stop (nothing is running)";

  return (
    <main
      aria-labelledby={headingId}
      data-testid="activity-monitor"
      data-register="instrument"
      style={{ minHeight: "100vh", boxSizing: "border-box", padding: "14px 16px", background: "var(--srf-0)", color: "var(--tx-1)", fontFamily: "var(--font-sans)", display: "flex", flexDirection: "column", gap: 4 }}
    >
      <div style={{ display: "flex", alignItems: "center", gap: 10, paddingBottom: 8 }}>
        <h1 id={headingId} style={{ fontSize: 14, fontWeight: 500, flex: 1, margin: 0 }}>
          Activity monitor
        </h1>
        <button
          className="btn btn-sm"
          data-testid="mon-stop"
          onClick={onStop}
          disabled={!running}
          aria-label={stopName}
          title={stopName}
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
        <span id={toolsId} className="mono" style={label}>
          Tool calls
        </span>
        {turn.tools.length === 0 ? (
          <div style={{ ...note, paddingTop: 4 }}>None this turn.</div>
        ) : (
          <ul aria-labelledby={toolsId} style={{ listStyle: "none", margin: 0, padding: "4px 0 0", display: "flex", flexDirection: "column", gap: 4 }}>
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
      <div style={{ paddingTop: 8 }} data-testid="mon-runs">
        <span id={runsId} className="mono" style={label}>
          Command runs
        </span>
        {runs.length === 0 ? (
          <div style={{ ...note, paddingTop: 4 }}>No commands ran this turn.</div>
        ) : (
          <ul aria-labelledby={runsId} style={{ listStyle: "none", margin: 0, padding: "4px 0 0", display: "flex", flexDirection: "column", gap: 6 }}>
            {runs.map((r) => (
              <li key={r.callId + ":" + r.at} data-testid="mon-run-row" style={{ display: "flex", flexDirection: "column", gap: 2, fontSize: 12, color: "var(--tx-1)" }}>
                <span style={{ display: "flex", gap: 8 }}>
                  <span className="mono" style={{ flex: 1, minWidth: 0, overflowWrap: "anywhere" }}>{r.tool}</span>
                  <span className="mono" style={{ color: r.status === "completed" && !r.timedOut ? "var(--tx-2)" : "var(--warn)" }}>{runStatusText(r)}</span>
                  <span className="mono" style={{ color: "var(--tx-3)" }}>{r.durationMs === null ? "" : runDuration(r.durationMs)}</span>
                </span>
                {r.summary ? <span style={{ ...note, overflowWrap: "anywhere" }}>{r.summary}</span> : null}
                <span style={{ ...note, overflowWrap: "anywhere" }}>{r.sandbox ?? "no OS sandbox reported"}</span>
              </li>
            ))}
          </ul>
        )}
      </div>
      <div style={{ paddingTop: 8 }} data-testid="mon-workers">
        <span id={workersId} className="mono" style={label}>
          Worker processes
        </span>
        <div style={{ ...note, paddingTop: 4 }}>{workers.note}</div>
        {workers.rows && workers.rows.length > 0 ? (
          <ul aria-labelledby={workersId} style={{ listStyle: "none", margin: 0, padding: "4px 0 0", display: "flex", flexDirection: "column", gap: 4 }}>
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
      <Daemons d={snapshot.daemons} onPause={onPauseDaemon} onStopRun={onStopDaemon} />
      {undo && onUndo ? <UndoSection panel={undo} onUndo={onUndo} /> : null}
    </main>
  );
}
