// =====================================================================
// citrate-core — the daemon run log (K2, 0.5.2)
//
// Finished daemon runs from the run log in the metering folder (`daemon_runs_between`, HUP-S10.3):
// when it started, which daemon, the outcome, and the token count with where it came from. The log
// keeps no conversation content. Hermes › Work shows the newest few; Hermes › Controls › 3 · Records
// shows the full history for the window. Desktop app only: the web preview has no run log.
// A failed read names the call and shows no rows; it is never drawn as "no runs".
// =====================================================================
import { useEffect, useState } from "react";
import { daemonsApi, type DaemonRunEntry, type DaemonRunLog as RunLogData, type RunOutcome } from "./api";

/** How far back the run history reads. */
export const RUN_WINDOW_DAYS = 7;
/** How many runs Hermes › Work shows before "Full history". */
export const WORK_RUN_LOG_LIMIT = 5;

export interface RunLogIo {
  /** null outside the desktop app. */
  runsBetween: ((fromMs: number, toMs: number) => Promise<RunLogData>) | null;
  now(): number;
}

export const appRunLogIo = (): RunLogIo => {
  const api = daemonsApi();
  return { runsBetween: api ? (from, to) => api.runsBetween(from, to) : null, now: () => Date.now() };
};

export const OUTCOME_LABEL: Record<RunOutcome, { text: string; color: string }> = {
  answered: { text: "answered", color: "var(--ok)" },
  failed: { text: "failed", color: "var(--danger)" },
  stopped: { text: "stopped", color: "var(--tx-2)" },
  timed_out: { text: "timed out", color: "var(--warn)" },
  over_budget: { text: "over budget", color: "var(--warn)" },
  // The session never opened (refused before any turn ran): not a failure of the run, and 0 tokens.
  not_started: { text: "not started", color: "var(--tx-3)" },
};

export const outcomeLabel = (o: string) => OUTCOME_LABEL[o as RunOutcome] ?? { text: o.replace(/_/g, " "), color: "var(--tx-2)" };

export const fmtWhen = (ms: number) => new Date(ms).toLocaleString(undefined, { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" });

export interface RunsState {
  log: RunLogData | null;
  error: string | null;
}

/** Read the run log for the last RUN_WINDOW_DAYS once per io. Newest first. */
export function useDaemonRuns(io: RunLogIo, enabled = true): RunsState {
  const [st, setSt] = useState<RunsState>({ log: null, error: null });
  useEffect(() => {
    if (!enabled || !io.runsBetween) return;
    let live = true;
    const now = io.now();
    io.runsBetween(now - RUN_WINDOW_DAYS * 86_400_000, now + 1).then(
      (log) => live && setSt({ log: { ...log, runs: [...log.runs].sort((a, b) => b.startedMs - a.startedMs) }, error: null }),
      (e: unknown) => live && setSt({ log: null, error: e instanceof Error ? e.message : String(e) }),
    );
    return () => {
      live = false;
    };
  }, [io, enabled]);
  return st;
}

const small = { fontSize: 12, lineHeight: 1.6, color: "var(--tx-3)", margin: 0, padding: 16 } as const;

function RunRow({ r, testid }: { r: DaemonRunEntry; testid: string }) {
  const o = outcomeLabel(r.outcome);
  return (
    <div data-testid={testid} style={{ display: "grid", gridTemplateColumns: "110px minmax(0,1fr) 100px 90px", gap: 10, padding: "9px 16px", borderBottom: "1px solid var(--line-1)", alignItems: "baseline" }}>
      <span className="mono tabular" style={{ fontSize: 10.5, color: "var(--tx-3)" }}>{fmtWhen(r.startedMs)}</span>
      <span style={{ fontSize: 12.5, minWidth: 0, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{r.name}</span>
      <span className="mono" style={{ fontSize: 10.5, color: o.color }}>{o.text}</span>
      <span className="mono tabular" style={{ fontSize: 10.5, color: "var(--tx-2)" }}>
        {r.tokens.toLocaleString("en-US")} {r.tokenSource}
      </span>
    </div>
  );
}

export { RunRow as DaemonRunRow };

/** daemon_runs_between skips the log lines it cannot read and counts them. Every view of the run
 *  log shows that count (a tripwire in hermesWork.test.tsx checks it), so a partly or wholly
 *  unreadable log is never drawn as complete or empty. Renders nothing when every line was read. */
export function RunLogUnreadable({ count, testid, style }: { count: number; testid: string; style?: React.CSSProperties }) {
  if (count <= 0) return null;
  return (
    <p data-testid={testid} role="status" style={{ ...small, color: "var(--warn)", ...style }}>
      {count} line{count === 1 ? "" : "s"} of the log could not be read and {count === 1 ? "was" : "were"} skipped, not guessed.
    </p>
  );
}

/** The empty state of a run log view: "no runs" only when every line was read. */
export function runLogEmptyText(unreadable: number, readAll: string): string {
  return unreadable > 0 ? `No runs could be read in the last ${RUN_WINDOW_DAYS} days.` : readAll;
}

/** Hermes › Work: the newest daemon runs, with a link to the full history. */
export function DaemonRunLog({ io: ioProp, onFullHistory }: { io?: RunLogIo; onFullHistory: () => void }) {
  const [io] = useState<RunLogIo>(() => ioProp ?? appRunLogIo());
  const { log, error } = useDaemonRuns(io);
  const runs = log?.runs.slice(0, WORK_RUN_LOG_LIMIT) ?? [];
  return (
    <div className="surface" data-testid="daemon-run-log" data-source="daemon_runs_between" style={{ display: "flex", flexDirection: "column" }}>
      <div style={{ display: "flex", alignItems: "center", gap: 10, padding: "12px 16px", borderBottom: "1px solid var(--line-1)" }}>
        <span style={{ fontSize: 13.5, fontWeight: 500 }}>Run log</span>
        <span className="mono" style={{ fontSize: 10, color: "var(--tx-3)" }}>daemons · no conversation content · tokens measured or estimated</span>
        <button className="btn btn-ghost btn-sm" data-testid="run-log-full-history" onClick={onFullHistory} style={{ marginLeft: "auto" }}>
          Full history ›
        </button>
      </div>
      {!io.runsBetween ? (
        <p style={small}>Daemons run in the desktop app, so this preview has no run log.</p>
      ) : error ? (
        <p role="alert" style={{ ...small, color: "var(--danger)" }}>
          Couldn’t read the run log (daemon_runs_between): {error}
        </p>
      ) : !log ? (
        <p style={small}>Reading the run log…</p>
      ) : runs.length === 0 ? (
        <p style={small}>{runLogEmptyText(log.unreadable, `No runs yet in the last ${RUN_WINDOW_DAYS} days. Each finished daemon run is written here with its outcome and token count.`)}</p>
      ) : (
        runs.map((r) => <RunRow key={r.daemonId + r.runId} r={r} testid="work-run-row" />)
      )}
      {log && !error && <RunLogUnreadable count={log.unreadable} testid="work-runs-unreadable" />}
    </div>
  );
}
