// =====================================================================
// RunTimeline: the steps Hermes took for one reply, and what the answer read (FL design board 01).
//
// Rows come only from the reply's RunRecord (src/agent/runTimeline.ts), which the send path writes
// from the turn's real events. The running row is the only one that may animate (an opacity pulse,
// removed under prefers-reduced-motion by the .run-pulse rule in tokens.css). State is never told
// by colour alone: each row has a glyph and a visible word.
// =====================================================================
import { useState } from "react";
import { Unknown } from "../fl/primitives";
import { formatDuration, receiptsOf, runSummary, stepLabel, STATE_WORD, type RunRecord, type RunStep } from "../agent/runTimeline";

const RING: Record<RunStep["state"], string> = {
  running: "var(--warn)",
  done: "var(--ok)",
  refused: "var(--danger)",
  failed: "var(--danger)",
  stopped: "var(--line-2)",
};
const GLYPH: Record<RunStep["state"], string> = { running: "", done: "✓", refused: "⊘", failed: "!", stopped: "–" };

function StepRow({ st }: { st: RunStep }) {
  const filled = st.state === "done" || st.state === "refused" || st.state === "failed";
  const dur = formatDuration(st.durationMs);
  const durTitle = st.timing === "model" ? "model time, reported by the model server" : st.timing === "core" ? "timed by Citrate Core" : "time between the events as this app received them";
  return (
    <li data-testid="run-step" data-state={st.state} data-kind={st.kind} style={{ display: "grid", gridTemplateColumns: "22px minmax(0,1fr) auto", gap: 8, padding: "5px 12px", alignItems: "start" }}>
      <span
        aria-hidden
        className={st.state === "running" ? "run-pulse" : undefined}
        style={{ width: 15, height: 15, marginTop: 2, borderRadius: 999, border: "1.6px solid " + RING[st.state], background: filled ? RING[st.state] : "transparent", display: "inline-flex", alignItems: "center", justifyContent: "center", color: "var(--srf-2)", fontSize: 9.5, lineHeight: 1 }}
      >
        {GLYPH[st.state]}
      </span>
      <span style={{ display: "flex", flexDirection: "column", gap: 2, minWidth: 0 }}>
        <span style={{ fontSize: 12.5, color: "var(--tx-1)" }}>
          {stepLabel(st)}
          {st.state !== "done" && <span style={{ color: "var(--tx-3)" }}> · {STATE_WORD[st.state]}</span>}
          {st.state === "done" && <span className="sr-only"> · done</span>}
        </span>
        {st.detail && <span style={{ fontSize: 11.5, color: "var(--tx-2)", overflowWrap: "anywhere" }}>{st.detail}</span>}
        {st.block !== undefined && (
          <span style={{ fontSize: 11.5, color: "var(--tx-3)" }}>{st.block === null ? (
              <>
                Block: <Unknown reason="the node height was not read yet" />
              </>
            ) : (
              `Read at block ${st.block.toLocaleString("en-US")}`
            )}</span>
        )}
      </span>
      <span className="mono" title={dur ? durTitle : "duration not known"} style={{ fontSize: 10.5, color: "var(--tx-3)", whiteSpace: "nowrap" }}>
        {dur}
      </span>
    </li>
  );
}

/** The run card above a reply: live steps while it runs, collapsible once it has ended. */
export function RunTimeline({ run, onStop }: { run: RunRecord; onStop?: () => void }) {
  const [open, setOpen] = useState(true);
  const live = run.outcome === null;
  const { title, meta } = runSummary(run);
  if (run.steps.length === 0 && !live) return null;
  const shown = live || open;
  return (
    <section data-testid="run-timeline" aria-label="Steps Hermes took" style={{ border: "1px solid var(--line-1)", borderRadius: 8, background: "var(--srf-1)" }}>
      <div style={{ display: "flex", alignItems: "center", gap: 10, padding: "6px 12px", borderBottom: shown && run.steps.length > 0 ? "1px solid var(--line-1)" : "none", flexWrap: "wrap" }}>
        <span className="mono" style={{ fontSize: 10, letterSpacing: ".1em", textTransform: "uppercase", color: "var(--tx-2)" }}>
          {title}
        </span>
        <span className="mono" style={{ fontSize: 10.5, color: "var(--tx-3)" }}>
          {meta}
        </span>
        <span style={{ flex: 1 }}></span>
        {live && onStop && (
          <button type="button" className="btn btn-sm btn-danger" data-testid="run-stop" onClick={onStop} aria-keyshortcuts="Escape">
            Stop · Esc
          </button>
        )}
        {!live && (
          <button type="button" className="btn btn-ghost btn-sm" data-testid="run-toggle" aria-expanded={open} onClick={() => setOpen((v) => !v)}>
            {open ? "Hide steps" : "Show steps"}
          </button>
        )}
      </div>
      {shown && run.steps.length > 0 && (
        <ol aria-label="Steps" style={{ listStyle: "none", margin: 0, padding: "4px 0" }}>
          {run.steps.map((st) => (
            <StepRow key={st.id} st={st} />
          ))}
        </ol>
      )}
    </section>
  );
}

/** What the answer read, under the reply. Chain reads name the block the node was at. */
export function RunReceipts({ run }: { run: RunRecord | undefined }) {
  const receipts = receiptsOf(run);
  if (receipts.length === 0) return null;
  return (
    <details data-testid="run-receipts" style={{ fontSize: 12 }}>
      <summary className="btn btn-ghost btn-sm" style={{ display: "inline-flex" }}>
        Sources ({receipts.length})
      </summary>
      <ul style={{ margin: "4px 0 0", paddingLeft: 18, color: "var(--tx-2)", lineHeight: 1.6 }}>
        {receipts.map((r, i) => (
          <li key={i} data-testid="run-receipt" data-status={r.status}>
            {r.label} <span className="mono" style={{ color: "var(--tx-3)", fontSize: 11 }}>{r.tool}</span>
            {r.status === "refused" && " · refused, nothing was run"}
            {r.status === "failed" && " · the read failed"}
            {r.chain && r.status === "ok" && (r.block ? (
              ` · at block ${r.block.toLocaleString("en-US")}`
            ) : (
              <>
                {" · block "}
                <Unknown reason="node height not read yet" />
              </>
            ))}
          </li>
        ))}
      </ul>
    </details>
  );
}
