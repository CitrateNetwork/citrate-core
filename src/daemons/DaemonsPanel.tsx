// =====================================================================
// citrate-core — Daemons card on the Hermes home (HUP-S10.3, US-10.3 AC2)
//
// Create a recurring Hermes task: a name, the task, a schedule (local time) and a budget. Each row
// shows its status, today's runs and tokens (measured or estimated) against the budget, spend (always 0), the
// next run and the last outcome, with Pause / Resume and Remove. "Pause all" stops every daemon.
// The budget defaults are conservative placeholders, pending owner sign-off.
// =====================================================================
import { useEffect, useState } from "react";
import { daemonsSlice } from "./slice";
import { DEFAULT_BUDGET, daemonsApi, type Budget, type DaemonInput } from "./api";
import { deleteDaemon, refreshDaemons, saveDaemon, setAllDaemonsPaused, setDaemonPaused } from "./appRunner";

export interface DaemonActions {
  available: boolean;
  refresh(): Promise<void>;
  save(input: DaemonInput): Promise<string | null>;
  setPaused(id: string, paused: boolean): Promise<string | null>;
  setAllPaused(paused: boolean): Promise<string | null>;
  remove(id: string): Promise<string | null>;
}

export const appDaemonActions = (): DaemonActions => ({
  available: daemonsApi() !== null,
  refresh: refreshDaemons,
  save: saveDaemon,
  setPaused: setDaemonPaused,
  setAllPaused: setAllDaemonsPaused,
  remove: deleteDaemon,
});

/** Schedule presets (5-field cron, local time). */
export const SCHEDULE_PRESETS: readonly { label: string; cron: string }[] = [
  { label: "Every morning at 9:00", cron: "0 9 * * *" },
  { label: "Weekdays at 9:00", cron: "0 9 * * 1-5" },
  { label: "Every evening at 18:00", cron: "0 18 * * *" },
  { label: "Every 6 hours", cron: "0 */6 * * *" },
  { label: "Every Monday at 8:00", cron: "0 8 * * 1" },
];

const head = { display: "flex", alignItems: "center", padding: "12px 16px", borderBottom: "1px solid var(--line-1)" } as const;
const small = { fontSize: 11, color: "var(--tx-3)", lineHeight: 1.45 } as const;
const input = { width: "100%", boxSizing: "border-box" as const, fontSize: 12, padding: "6px 8px", background: "var(--srf-1)", color: "var(--tx-1)", border: "1px solid var(--line-2)", borderRadius: 6 };
const fmt = (n: number) => n.toLocaleString("en-US");
const when = (ms: number | null) => (ms === null ? "not scheduled" : new Date(ms).toLocaleString(undefined, { weekday: "short", hour: "2-digit", minute: "2-digit" }));

export function DaemonsPanel({ actions = appDaemonActions() }: { actions?: DaemonActions }) {
  const st = daemonsSlice.use();
  const [creating, setCreating] = useState(false);
  const [name, setName] = useState("");
  const [prompt, setPrompt] = useState("");
  const [schedule, setSchedule] = useState(SCHEDULE_PRESETS[0].cron);
  const [budget, setBudget] = useState<Budget>(DEFAULT_BUDGET);
  const [err, setErr] = useState<string | null>(null);
  const [open, setOpen] = useState<string | null>(null);

  useEffect(() => {
    if (actions.available && !st.loaded) void actions.refresh();
  }, [actions, st.loaded]);

  if (!actions.available) {
    return (
      <div className="surface" data-testid="daemons-panel">
        <div style={head}>
          <span style={{ fontSize: 13.5, fontWeight: 500 }}>Daemons</span>
        </div>
        <p style={{ ...small, margin: 0, padding: 16 }}>Daemons run in the desktop app.</p>
      </div>
    );
  }

  const done = (e: string | null) => {
    setErr(e);
    return e === null;
  };
  const view = st.view;
  const allPaused = view?.allPaused ?? false;
  const blocked = st.runner.blockedReason;

  return (
    <div className="surface" data-testid="daemons-panel" style={{ display: "flex", flexDirection: "column" }}>
      <div style={head}>
        <span style={{ fontSize: 13.5, fontWeight: 500 }}>Daemons</span>
        <span style={{ marginLeft: "auto", display: "flex", gap: 6 }}>
          {view && view.daemons.length > 0 ? (
            <button className="btn btn-sm" data-testid="daemons-pause-all" onClick={() => void actions.setAllPaused(!allPaused).then(done)}>
              {allPaused ? "Resume all" : "Pause all"}
            </button>
          ) : null}
          <button className="btn btn-sm" data-testid="daemons-new" onClick={() => setCreating((c) => !c)}>
            {creating ? "Cancel" : "New"}
          </button>
        </span>
      </div>
      <p style={{ ...small, margin: 0, padding: "8px 16px 0" }}>
        Recurring Hermes tasks on this machine, on the local model. Anything a daemon wants to change waits for your approval. Spend is 0.
      </p>
      {blocked && view && view.daemons.length > 0 ? (
        <p data-testid="daemons-blocked" style={{ ...small, margin: 0, padding: "4px 16px 0" }}>
          Runs are held: {blocked}.
        </p>
      ) : null}
      {err || st.error || st.runner.error ? (
        <p role="alert" style={{ ...small, color: "var(--danger)", margin: 0, padding: "4px 16px 0" }}>
          {err ?? st.error ?? st.runner.error}
        </p>
      ) : null}
      {creating ? (
        <form
          data-testid="daemons-form"
          style={{ display: "flex", flexDirection: "column", gap: 6, padding: "8px 16px", borderBottom: "1px solid var(--line-1)" }}
          onSubmit={(e) => {
            e.preventDefault();
            void actions.save({ name, prompt, schedule, budget }).then((e2) => {
              if (done(e2)) {
                setCreating(false);
                setName("");
                setPrompt("");
              }
            });
          }}
        >
          <input aria-label="Daemon name" placeholder="Name, e.g. Morning node digest" value={name} onChange={(e) => setName(e.target.value)} style={input} />
          <textarea aria-label="Task" placeholder="What should Hermes do each time?" value={prompt} onChange={(e) => setPrompt(e.target.value)} rows={3} style={input} />
          <select aria-label="Schedule preset" value={SCHEDULE_PRESETS.some((p) => p.cron === schedule) ? schedule : ""} onChange={(e) => e.target.value && setSchedule(e.target.value)} style={input}>
            {SCHEDULE_PRESETS.map((p) => (
              <option key={p.cron} value={p.cron}>
                {p.label}
              </option>
            ))}
            <option value="">Custom (cron)</option>
          </select>
          <input aria-label="Schedule (cron: minute hour day month weekday)" value={schedule} onChange={(e) => setSchedule(e.target.value)} style={{ ...input, fontFamily: "var(--font-mono)" }} />
          <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr 1fr", gap: 6 }}>
            <label style={small}>
              Runs a day
              <input aria-label="Runs a day" type="number" min={1} max={48} value={budget.maxRunsPerDay} onChange={(e) => setBudget({ ...budget, maxRunsPerDay: Number(e.target.value) })} style={input} />
            </label>
            <label style={small}>
              Tokens a day
              <input aria-label="Tokens a day" type="number" min={1} value={budget.maxTokensPerDay} onChange={(e) => setBudget({ ...budget, maxTokensPerDay: Number(e.target.value) })} style={input} />
            </label>
            <label style={small}>
              Tokens a run
              <input aria-label="Tokens a run" type="number" min={1} value={budget.maxTokensPerRun} onChange={(e) => setBudget({ ...budget, maxTokensPerRun: Number(e.target.value) })} style={input} />
            </label>
          </div>
          <span style={small}>Spend: 0 SALT (daemons cannot spend). Default limits are placeholders, pending owner sign-off. Tokens are measured from the model server when it reports usage, otherwise estimated.</span>
          <button className="btn btn-sm" type="submit" data-testid="daemons-save">
            Save daemon
          </button>
        </form>
      ) : null}
      {!view || view.daemons.length === 0 ? (
        <p style={{ ...small, margin: 0, padding: 16 }}>No daemons. Nothing runs on a schedule until you create one.</p>
      ) : (
        view.daemons.map((d) => (
          <div key={d.id} data-testid="daemon-row" style={{ padding: "10px 16px", borderBottom: "1px solid var(--line-1)", display: "flex", flexDirection: "column", gap: 3 }}>
            <span style={{ display: "flex", gap: 8, alignItems: "center" }}>
              <button onClick={() => setOpen(open === d.id ? null : d.id)} style={{ flex: 1, minWidth: 0, textAlign: "left", background: "none", border: "none", padding: 0, color: "var(--tx-1)", fontSize: 12.5, fontWeight: 500, cursor: "pointer" }}>
                {d.name}
              </button>
              <span className="mono" data-testid="daemon-status" style={{ fontSize: 10, color: d.running ? "var(--accent-text)" : "var(--tx-3)" }}>
                {d.status}
              </span>
            </span>
            <span className="mono" style={{ fontSize: 10, color: "var(--tx-3)" }}>
              {d.schedule} · next {when(d.nextRunMs)}
            </span>
            <span className="mono" data-testid="daemon-budget" style={{ fontSize: 10, color: "var(--tx-3)" }}>
              {d.runsToday} of {d.budget.maxRunsPerDay} runs · {fmt(d.tokensToday)} of {fmt(d.budget.maxTokensPerDay)} tokens ({d.lastTokenSource === "measured" ? "last run measured" : "estimated"}) · spend {d.spendTodaySalt} SALT
              {d.skippedToday > 0 ? ` · ${d.skippedToday} skipped (budget)` : ""}
            </span>
            {d.lastOutcome ? (
              <span style={small}>
                Last: {d.lastOutcome.replace("_", " ")}
                {d.lastNote ? ` · ${d.lastNote}` : ""}
              </span>
            ) : null}
            {open === d.id ? (
              <div style={{ ...small, whiteSpace: "pre-wrap" }}>
                <div>Task: {d.prompt}</div>
                {st.replies[d.id] ? <div data-testid="daemon-reply">Last answer: {st.replies[d.id]}</div> : null}
              </div>
            ) : null}
            <span style={{ display: "flex", gap: 6 }}>
              <button className="btn btn-sm" data-testid="daemon-pause" disabled={allPaused} onClick={() => void actions.setPaused(d.id, !d.paused).then(done)}>
                {d.paused ? "Resume" : "Pause"}
              </button>
              <button className="btn btn-sm" data-testid="daemon-remove" onClick={() => void actions.remove(d.id).then(done)}>
                Remove
              </button>
            </span>
          </div>
        ))
      )}
    </div>
  );
}
