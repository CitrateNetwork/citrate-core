// HUP-S3.4 — "Teach Hermes": the member launches a verified workflow from the app. They write the
// task and the phrases the answer must contain; the sidecar's verifiers judge the answer. Only a
// run whose checks all passed lets Hermes propose what to keep, and every proposal still waits for
// the member's Accept on its card. The view is pure; `TeachHermesCard` wires it to the bridge.
import { useState } from "react";
import { bridge } from "../bridge";
import { buildTeachWorkflow, runTeach, teachSummary, TEACH_LIMITS, type TeachProgress } from "../agent/learnLauncher";

const label = { fontSize: 9.5, letterSpacing: ".1em", textTransform: "uppercase" as const, color: "var(--tx-3)" };

export interface TeachHermesViewProps {
  /** Null when teaching is available; otherwise why not (shown instead of the form). */
  unavailable: string | null;
  task: string;
  checks: string;
  running: boolean;
  progress: TeachProgress | null;
  /** Why the current inputs cannot run (null when they can). */
  invalid: string | null;
  onTask(v: string): void;
  onChecks(v: string): void;
  onRun(): void;
}

export function TeachHermesView({ unavailable, task, checks, running, progress, invalid, onTask, onChecks, onRun }: TeachHermesViewProps) {
  const tone = progress?.phase === "failed" ? "var(--danger)" : progress?.phase === "unverified" ? "var(--warn)" : "var(--tx-2)";
  return (
    <div data-testid="teach-hermes" style={{ border: "1px solid var(--line-2)", borderRadius: "var(--r-1)", padding: "10px 12px", display: "flex", flexDirection: "column", gap: 8 }}>
      <div style={{ display: "flex", alignItems: "baseline", gap: 8 }}>
        <span className="mono" style={label}>
          Teach Hermes
        </span>
        <span style={{ fontSize: 11.5, color: "var(--tx-3)" }}>Give it a task and the checks its answer must pass. It can only learn from a run where every check passed.</span>
      </div>
      {unavailable ? (
        <span data-testid="teach-unavailable" style={{ fontSize: 12, color: "var(--tx-3)" }}>
          {unavailable}
        </span>
      ) : (
        <>
          <label style={{ display: "flex", flexDirection: "column", gap: 3 }}>
            <span style={{ fontSize: 11.5, color: "var(--tx-2)" }}>Task</span>
            <textarea
              data-testid="teach-task"
              value={task}
              maxLength={TEACH_LIMITS.maxTask}
              rows={2}
              disabled={running}
              onChange={(e) => onTask(e.target.value)}
              placeholder="What is the chain id of the Citrate network?"
              style={{ fontSize: 12.5, resize: "vertical" }}
            />
          </label>
          <label style={{ display: "flex", flexDirection: "column", gap: 3 }}>
            <span style={{ fontSize: 11.5, color: "var(--tx-2)" }}>Checks (one per line: a phrase the answer must contain)</span>
            <textarea data-testid="teach-checks" value={checks} rows={2} disabled={running} onChange={(e) => onChecks(e.target.value)} placeholder="40204" style={{ fontSize: 12.5, resize: "vertical" }} />
          </label>
          <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
            <button data-testid="teach-run" className="btn btn-sm" disabled={running || invalid !== null} onClick={onRun}>
              {running ? "Running..." : "Run and check"}
            </button>
            {invalid && !running && (task.trim() || checks.trim()) && (
              <span data-testid="teach-invalid" style={{ fontSize: 11.5, color: "var(--tx-3)" }}>
                {invalid}
              </span>
            )}
          </div>
        </>
      )}
      {progress && (
        <div data-testid="teach-progress" data-phase={progress.phase} style={{ display: "flex", flexDirection: "column", gap: 4 }}>
          <span role="status" style={{ fontSize: 12, color: tone }}>
            {teachSummary(progress)}
          </span>
          {(progress.verdicts ?? []).map((v) => (
            <span key={v.label} data-testid="teach-verdict" data-pass={v.passed ? "true" : "false"} className="mono" style={{ fontSize: 10.5, color: v.passed ? "var(--ok)" : "var(--danger)" }}>
              {v.passed ? "passed" : "failed"} · {v.label}
            </span>
          ))}
        </div>
      )}
    </div>
  );
}

/** The live card: runs a teach session through the bridge, then asks the panel to reload. */
export function TeachHermesCard({ enabled, running: hermesRunning, onFinished }: { enabled: boolean; running: boolean; onFinished(): void }) {
  const [task, setTask] = useState("");
  const [checks, setChecks] = useState("");
  const [busy, setBusy] = useState(false);
  const [progress, setProgress] = useState<TeachProgress | null>(null);
  const built = buildTeachWorkflow({ task, checks: checks.split("\n") });
  const unavailable = !hermesRunning
    ? "Teaching needs Hermes running on your local model."
    : !enabled
      ? "Learning is off in Hermes, so there is nothing to teach yet."
      : null;
  const run = async () => {
    if (!built.ok || busy) return;
    setBusy(true);
    setProgress(null);
    try {
      await runTeach(bridge.agentHarness, built.spec, setProgress);
    } finally {
      setBusy(false);
      onFinished();
    }
  };
  return (
    <TeachHermesView
      unavailable={unavailable}
      task={task}
      checks={checks}
      running={busy}
      progress={progress}
      invalid={built.ok ? null : built.reason}
      onTask={setTask}
      onChecks={setChecks}
      onRun={() => void run()}
    />
  );
}
