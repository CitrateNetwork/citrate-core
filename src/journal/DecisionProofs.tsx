// =====================================================================
// HUP-S7.3 (US-7.2 AC3): "Prove a past decision" in the Journal.
//
// The list loads only when the member asks for it. "Check proof" asks core, which recomputes the
// record hash, the audit path and the day value itself and reads AnchorRegistry on 40204; the
// line shown is core's verdict, never the sidecar's claim. Read-only: nothing is signed or sent.
// =====================================================================
import { useState } from "react";
import { anchorState, describeRecord, type ProofVerdict, type RecordRow, type RecordsPage } from "./proofView";
import type { ReportInvoke } from "./HermesDailyReport";

const PAGE = 10;

function message(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

export function DecisionProofs({ invoke }: { invoke: ReportInvoke }) {
  const [rows, setRows] = useState<RecordRow[] | null>(null);
  const [next, setNext] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [verdicts, setVerdicts] = useState<Record<number, ProofVerdict | string>>({});

  const load = async (before: number | null) => {
    if (busy) return;
    setBusy(true);
    try {
      const p = await invoke<RecordsPage>("hermes_anchor_records", { before, limit: PAGE });
      setRows((cur) => (before === null ? p.records : [...(cur ?? []), ...p.records]));
      setNext(p.nextBefore);
      setError(null);
    } catch (e) {
      setError(message(e));
    } finally {
      setBusy(false);
    }
  };

  const prove = async (seq: number) => {
    if (busy) return;
    setBusy(true);
    try {
      const v = await invoke<ProofVerdict>("hermes_anchor_proof", { seq });
      setVerdicts((cur) => ({ ...cur, [seq]: v }));
    } catch (e) {
      setVerdicts((cur) => ({ ...cur, [seq]: message(e) }));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
      <span className="eyebrow">Prove a past decision</span>
      <span style={{ color: "var(--tx-3)" }}>
        Each approval or budgeted action is recorded on this device. Once its day is anchored, the app can prove the record was not changed since.
      </span>
      {rows === null && (
        <button data-testid="dp-load" className="btn btn-ghost btn-sm" style={{ alignSelf: "flex-start" }} onClick={() => void load(null)} disabled={busy}>
          Show my recorded decisions
        </button>
      )}
      {error && (
        <span data-testid="dp-error" style={{ color: "var(--tx-2)" }}>
          Unknown: {error}
        </span>
      )}
      {rows !== null && rows.length === 0 && !error && (
        <span data-testid="dp-empty" style={{ color: "var(--tx-3)" }}>
          No decisions are recorded yet.
        </span>
      )}
      {rows?.map((r) => {
        const v = verdicts[r.seq];
        return (
          <div key={r.seq} data-testid={`dp-row-${r.seq}`} style={{ borderBottom: "1px solid var(--line-1)", padding: "6px 0", display: "flex", flexDirection: "column", gap: 4 }}>
            <span style={{ display: "flex", gap: 8, alignItems: "baseline" }}>
              <span className="mono tabular" style={{ color: "var(--tx-3)" }}>
                #{r.seq}
              </span>
              <span style={{ flex: 1 }}>{describeRecord(r.record)}</span>
              <span style={{ color: "var(--tx-3)" }}>
                {r.date}, {anchorState(r)}
              </span>
              <button data-testid={`dp-prove-${r.seq}`} className="btn btn-ghost btn-sm" onClick={() => void prove(r.seq)} disabled={busy || !r.batched}>
                Check proof
              </button>
            </span>
            {v !== undefined && (
              <span
                data-testid={`dp-verdict-${r.seq}`}
                data-proven={typeof v === "string" ? "false" : String(v.proven)}
                style={{ color: typeof v !== "string" && v.proven ? "var(--tx-1)" : "var(--tx-2)" }}
              >
                {typeof v === "string" ? `Unknown: ${v}` : v.line}
              </span>
            )}
          </div>
        );
      })}
      {rows !== null && next !== null && (
        <button data-testid="dp-older" className="btn btn-ghost btn-sm" style={{ alignSelf: "flex-start" }} onClick={() => void load(next)} disabled={busy}>
          Older decisions
        </button>
      )}
    </div>
  );
}
