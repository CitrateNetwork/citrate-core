// =====================================================================
// K2 (0.5.2): Hermes › Controls › 3 · Records. What happened, from stores that already exist:
//
//   Approval records     Hermes approvals: written by hic_record_decision (hic_records.rs); NO
//                        command reads them back yet (HicLog::records has no command), so this
//                        says "not available yet" and names the gap. Web sign-in decisions:
//                        web_budget_status → snapshot.records (budgets.ts), hash-chained in core.
//   Spend ledger         escalation_budget → capMicros, usedMicros, confirmedMicros, history.
//   Daemon run history   daemon_runs_between (the run log in the metering folder).
//
// Rule 7: every view names its call. UI truth contract: a failed read shows the call and its
// error, never a zero. Money (§5, chain-team rulings 4 and 5): dollar amounts are the provider's
// list price for replies on the member's own keys; no SALT amount appears here, and daemons say
// "Daemons can't spend SALT".
// =====================================================================
import { useEffect, useState } from "react";
import { bridge } from "../../bridge";
import type { EscalationBudget, EscalationSpendRecord } from "../../bridge/domains";
import { formatMicros } from "../../agent/escalation";
import { budgetsApi, normalizeStatus, type DecisionRecord, type WebBudgetStatus } from "../../budgets/budgets";
import { createSlice } from "../../shell/slices/createSlice";
import { DAEMONS_NO_SALT } from "../../daemons/DaemonsPanel";
import { DaemonRunRow, RUN_WINDOW_DAYS, appRunLogIo, fmtWhen, useDaemonRuns, type RunLogIo } from "../../daemons/DaemonRunLog";

export { RUN_WINDOW_DAYS };

export type RecordsView = "approvals" | "spend" | "runs";

/** Which records view is open, and the daemon the run history is filtered to (null = all). App-wide,
 *  so Hermes › Work can open the run history (or the approval records) and land on it. */
export const recordsSlice = createSlice<{ view: RecordsView; daemonId: string | null }>({ view: "approvals", daemonId: null });

/** Choose the records view before navigating to hermes/controls/records. */
export function openRecords(view: RecordsView, daemonId: string | null = null): void {
  recordsSlice.set({ view, daemonId });
}

export interface RecordsIo extends RunLogIo {
  escalationBudget(): Promise<EscalationBudget>;
  /** null outside the desktop app (the budget store and its records live in core). */
  webSignIn: (() => Promise<WebBudgetStatus>) | null;
}

export function appRecordsIo(): RecordsIo {
  const budgets = budgetsApi();
  return {
    ...appRunLogIo(),
    escalationBudget: () => bridge.escalation.budget(),
    webSignIn: budgets ? () => budgets.status() : null,
  };
}

const NAV: [RecordsView, string][] = [
  ["approvals", "Approval records"],
  ["spend", "Spend ledger"],
  ["runs", "Daemon run history"],
];

const msg = (e: unknown) => (e instanceof Error ? e.message : String(e));
const head = { display: "flex", alignItems: "center", gap: 10, padding: "12px 16px", borderBottom: "1px solid var(--line-1)" } as const;
const para = { fontSize: 12.5, lineHeight: 1.6, color: "var(--tx-3)", margin: 0, padding: 16 } as const;
const foot = { fontSize: 11.5, lineHeight: 1.55, color: "var(--tx-3)", margin: 0 } as const;
const errStyle = { ...para, color: "var(--danger)" } as const;

// ---------------------------------------------------------------- approvals

function ApprovalRecords({ io }: { io: RecordsIo }) {
  const [web, setWeb] = useState<DecisionRecord[] | null>(null);
  const [webErr, setWebErr] = useState<string | null>(null);
  useEffect(() => {
    if (!io.webSignIn) return;
    let live = true;
    io.webSignIn().then(
      (st) => live && setWeb([...normalizeStatus(st).snapshot.records].sort((a, b) => b.recordId - a.recordId)),
      (e) => live && setWebErr(msg(e)),
    );
    return () => {
      live = false;
    };
  }, [io]);
  return (
    <>
      <div className="surface" data-testid="hic-records-gap" data-gap="hic-records-read" data-source="none yet (hic_record_decision writes; no read command)" style={{ display: "flex", flexDirection: "column" }}>
        <div style={head}>
          <span style={{ fontSize: 13.5, fontWeight: 500 }}>Hermes approvals</span>
          <span className="eyebrow" style={{ marginLeft: "auto" }}>
            Not available yet
          </span>
        </div>
        <p style={para}>
          Every approve or decline you make on a Hermes card is written on this machine (hic_record_decision), append-only. Reading those records back here is not available yet: no command serves them. What waits for you now is on Hermes › Work under Awaiting your review.
        </p>
      </div>

      <div className="surface" data-testid="web-signin-records" data-source="web_budget_status" style={{ display: "flex", flexDirection: "column" }}>
        <div style={head}>
          <span style={{ fontSize: 13.5, fontWeight: 500 }}>Web sign-in decisions</span>
          <span className="mono" style={{ marginLeft: "auto", fontSize: 10, color: "var(--tx-3)" }}>
            hash-chained in core · newest first
          </span>
        </div>
        {!io.webSignIn ? (
          <p data-testid="web-records-unavailable" style={para}>
            Web sign-in budgets and their records live in the desktop app.
          </p>
        ) : webErr ? (
          <p data-testid="web-records-error" role="alert" style={errStyle}>
            Couldn’t read the decision records (web_budget_status): {webErr}
          </p>
        ) : web === null ? (
          <p style={para}>Reading the decision records…</p>
        ) : web.length === 0 ? (
          <p style={para}>No web sign-in decisions yet. Granting, revoking or using a sign-in budget writes a record here.</p>
        ) : (
          web.map((r) => (
            <div key={r.recordId} data-testid="web-record" style={{ display: "grid", gridTemplateColumns: "50px 110px 150px 90px minmax(0,1fr)", gap: 10, padding: "9px 16px", borderBottom: "1px solid var(--line-1)", alignItems: "baseline" }}>
              <span className="mono tabular" style={{ fontSize: 10.5, color: "var(--tx-3)" }}>#{r.recordId}</span>
              <span className="mono tabular" style={{ fontSize: 10.5, color: "var(--tx-3)" }}>{fmtWhen(r.atMs)}</span>
              <span className="mono" style={{ fontSize: 10.5, color: "var(--tx-2)" }}>{r.kind.replace(/_/g, " ")}</span>
              <span className="mono" style={{ fontSize: 10.5, color: "var(--tx-2)" }}>{r.status.replace(/_/g, " ")}</span>
              <span style={{ fontSize: 12, minWidth: 0, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{r.origin}</span>
            </div>
          ))
        )}
      </div>
    </>
  );
}

// ---------------------------------------------------------------- spend

const modeLabel = (m: EscalationSpendRecord["mode"]) => (m === "budget" ? "within the cap" : "you approved");

function SpendLedger({ io }: { io: RecordsIo }) {
  const [b, setB] = useState<EscalationBudget | null>(null);
  const [err, setErr] = useState<string | null>(null);
  useEffect(() => {
    let live = true;
    io.escalationBudget().then(
      (v) => live && setB(v),
      (e) => live && setErr(msg(e)),
    );
    return () => {
      live = false;
    };
  }, [io]);

  const tile = (testid: string, k: string, v: string, sub: string) => (
    <div data-testid={testid} style={{ background: "var(--srf-1)", padding: "12px 14px", display: "flex", flexDirection: "column", gap: 4 }}>
      <span className="eyebrow">{k}</span>
      <span className="tabular" style={{ fontFamily: "var(--font-display)", fontSize: 20, fontWeight: 420 }}>
        {v}
      </span>
      <span className="mono" style={{ fontSize: 10, color: "var(--tx-3)" }}>{sub}</span>
    </div>
  );

  const history = b ? [...b.history].sort((x, y) => y.atMs - x.atMs) : [];
  return (
    <>
      {err ? (
        <div className="surface">
          <p data-testid="spend-error" role="alert" style={errStyle}>
            Couldn’t read the spend ledger (escalation_budget): {err}
          </p>
        </div>
      ) : !b ? (
        <div className="surface">
          <p style={para}>Reading the spend ledger…</p>
        </div>
      ) : b.unreadable ? (
        <div className="surface">
          <p data-testid="spend-unreadable" role="alert" style={{ ...para, color: "var(--warn)" }}>
            The spend ledger file could not be read, so today’s spend is unknown and every escalation asks you first until the cap is set again in Details › Escalation & spend caps.
          </p>
        </div>
      ) : (
        <>
          <div style={{ display: "grid", gridTemplateColumns: "repeat(3,minmax(0,1fr))", gap: 1, background: "var(--line-1)", border: "1px solid var(--line-1)", borderRadius: "var(--r-2)", overflow: "hidden" }}>
            {tile("spend-cap", "Daily cap", formatMicros(b.capMicros), b.capMicros === 0 ? "every escalation asks" : "a notice under the cap")}
            {tile("spend-used", "Used under the cap", formatMicros(b.usedMicros), "today · provider list price")}
            {tile("spend-confirmed", "You confirmed", formatMicros(b.confirmedMicros), "today · outside the cap")}
          </div>
          <div className="surface" data-source="escalation_budget" style={{ display: "flex", flexDirection: "column" }}>
            <div style={{ display: "grid", gridTemplateColumns: "110px minmax(0,1fr) 110px 80px 80px 90px", gap: 10, padding: "9px 16px", borderBottom: "1px solid var(--line-1)", background: "var(--srf-1)" }}>
              {["When", "Destination", "Mode", "Quoted", "Charged", "Outcome"].map((h) => (
                <span key={h} className="eyebrow">
                  {h}
                </span>
              ))}
            </div>
            {history.length === 0 ? (
              <p data-testid="spend-empty" style={para}>
                {b.capMicros === 0 ? "No spend. The daily cap is $0.00, so every escalation asks you first." : "No escalations today."}
              </p>
            ) : (
              history.map((h) => (
                <div key={h.escalationId} data-testid="spend-row" style={{ display: "grid", gridTemplateColumns: "110px minmax(0,1fr) 110px 80px 80px 90px", gap: 10, padding: "9px 16px", borderBottom: "1px solid var(--line-1)", alignItems: "baseline" }}>
                  <span className="mono tabular" style={{ fontSize: 10.5, color: "var(--tx-3)" }}>{fmtWhen(h.atMs)}</span>
                  <span style={{ fontSize: 12, minWidth: 0, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{h.destination}</span>
                  <span className="mono" style={{ fontSize: 10.5, color: "var(--tx-2)" }}>{modeLabel(h.mode)}</span>
                  <span className="mono tabular" style={{ fontSize: 10.5, color: "var(--tx-2)" }}>{formatMicros(h.quotedMicros)}</span>
                  <span className="mono tabular" style={{ fontSize: 10.5 }}>{formatMicros(h.chargedMicros)}</span>
                  <span className="mono" style={{ fontSize: 10.5, color: "var(--tx-2)" }}>{h.outcome.replace(/_/g, " ")}</span>
                </div>
              ))
            )}
          </div>
        </>
      )}
      <p style={foot}>Dollar amounts are your provider’s list price for each reply on your own key, as you entered it for the endpoint. {DAEMONS_NO_SALT}</p>
    </>
  );
}

// ---------------------------------------------------------------- runs

function RunHistory({ io, daemonId }: { io: RecordsIo; daemonId: string | null }) {
  const { log, error } = useDaemonRuns(io);
  if (!io.runsBetween) {
    return (
      <div className="surface">
        <p data-testid="runs-unavailable" style={para}>
          Daemons run in the desktop app, so this preview has no run history.
        </p>
      </div>
    );
  }
  const daemons = log ? [...new Map(log.runs.map((r) => [r.daemonId, r.name])).entries()] : [];
  const runs = log ? log.runs.filter((r) => daemonId === null || r.daemonId === daemonId) : [];
  const pick = (id: string | null, label: string) => (
    <button key={id ?? "all"} data-testid={`runs-pick-${id ?? "all"}`} className={"btn btn-sm " + (daemonId === id ? "btn-secondary" : "btn-ghost")} onClick={() => recordsSlice.set({ daemonId: id })}>
      {label}
    </button>
  );
  return (
    <>
      {daemons.length > 0 && <span style={{ display: "flex", gap: 6, flexWrap: "wrap" }}>{[pick(null, "All daemons"), ...daemons.map(([id, name]) => pick(id, name))]}</span>}
      <div className="surface" data-source="daemon_runs_between" style={{ display: "flex", flexDirection: "column" }}>
        <div style={{ display: "grid", gridTemplateColumns: "110px minmax(0,1fr) 100px 90px", gap: 10, padding: "9px 16px", borderBottom: "1px solid var(--line-1)", background: "var(--srf-1)" }}>
          {["Started", "Daemon", "Outcome", "Tokens"].map((h) => (
            <span key={h} className="eyebrow">
              {h}
            </span>
          ))}
        </div>
        {error ? (
          <p data-testid="runs-error" role="alert" style={errStyle}>
            Couldn’t read the run log (daemon_runs_between): {error}
          </p>
        ) : !log ? (
          <p style={para}>Reading the run log…</p>
        ) : runs.length === 0 ? (
          <p style={para}>No runs recorded in the last {RUN_WINDOW_DAYS} days. A daemon’s runs appear here after its first run.</p>
        ) : (
          runs.map((r) => <DaemonRunRow key={r.daemonId + r.runId} r={r} testid="run-row" />)
        )}
        {log && log.unreadable > 0 && (
          <p data-testid="runs-unreadable" style={{ ...para, color: "var(--warn)" }}>
            {log.unreadable} line{log.unreadable === 1 ? "" : "s"} of the log could not be read and {log.unreadable === 1 ? "was" : "were"} skipped, not guessed.
          </p>
        )}
      </div>
      <p style={foot}>From the run log in the metering folder, last {RUN_WINDOW_DAYS} days. Conversation content is never kept in this log. {DAEMONS_NO_SALT}</p>
    </>
  );
}

// ---------------------------------------------------------------- page

export function ControlsRecords({ io: ioProp }: { io?: RecordsIo }) {
  const [io] = useState<RecordsIo>(() => ioProp ?? appRecordsIo());
  const { view, daemonId } = recordsSlice.use();
  return (
    <div data-testid="controls-records" style={{ display: "flex", flexDirection: "column", gap: 12, maxWidth: 1040 }}>
      <span style={{ display: "flex", gap: 6 }}>
        {NAV.map(([id, label]) => (
          <button key={id} data-testid={`records-nav-${id}`} className={"btn btn-sm " + (view === id ? "btn-secondary" : "btn-ghost")} onClick={() => recordsSlice.set({ view: id })}>
            {label}
          </button>
        ))}
      </span>
      {view === "approvals" && <ApprovalRecords io={io} />}
      {view === "spend" && <SpendLedger io={io} />}
      {view === "runs" && <RunHistory io={io} daemonId={daemonId} />}
    </div>
  );
}
