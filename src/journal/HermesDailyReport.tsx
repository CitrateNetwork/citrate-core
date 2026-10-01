// =====================================================================
// HUP-S7.3 + S7.5 — the Hermes daily report panel in the Journal.
//
// Real numbers only (the sidecar's metering via `hermes_metering_daily`); unknowns are shown as
// unknown. Below the numbers: the nightly anchor state and the BenchmarkRegistry sharing toggle,
// both off and disabled until their registries are deployed on 40204. A pending anchor card is
// approved or rejected by its own id; the anchor key signs only inside the anchor ceremony.
// =====================================================================
import { useCallback, useEffect, useState } from "react";
import { meteringRows, utcDay, type AnchorApprove, type ChainStatus, type DailyResponse } from "./meteringView";

export type ReportInvoke = <T>(cmd: string, args?: Record<string, unknown>) => Promise<T>;

export interface HermesDailyReportProps {
  mode: "sim" | "tauri";
  invoke: ReportInvoke;
  now: () => Date;
}

function message(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

export function HermesDailyReport({ mode, invoke, now }: HermesDailyReportProps) {
  const [daysBack, setDaysBack] = useState(0);
  const [report, setReport] = useState<DailyResponse | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [chain, setChain] = useState<ChainStatus | null>(null);
  const [chainError, setChainError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [anchorResult, setAnchorResult] = useState<string | null>(null);
  const day = utcDay(now(), daysBack);

  const load = useCallback(async () => {
    if (mode !== "tauri") return;
    try {
      setReport(await invoke<DailyResponse>("hermes_metering_daily", { day }));
      setError(null);
    } catch (e) {
      setReport(null);
      setError(message(e));
    }
    try {
      setChain(await invoke<ChainStatus>("hermes_chain_status", {}));
      setChainError(null);
    } catch (e) {
      setChainError(message(e));
    }
  }, [mode, invoke, day]);

  useEffect(() => {
    void load();
  }, [load]);

  if (mode !== "tauri") {
    return (
      <div data-testid="hm-desktop-only" style={{ fontSize: 12.5, color: "var(--tx-3)" }}>
        The Hermes daily report is available in the desktop app.
      </div>
    );
  }

  const setSettings = async (anchorNightly: boolean, shareBenchmarks: boolean) => {
    if (busy) return;
    setBusy(true);
    try {
      setChain(await invoke<ChainStatus>("hermes_chain_settings_set", { anchorNightly, shareBenchmarks }));
      setChainError(null);
    } catch (e) {
      setChainError(message(e));
    } finally {
      setBusy(false);
    }
  };

  const decide = async (id: string, approve: boolean) => {
    if (busy) return;
    setBusy(true);
    try {
      if (approve) {
        const r = await invoke<AnchorApprove>("hermes_anchor_approve", { id });
        setAnchorResult(r.statusLine);
      } else {
        await invoke<null>("hermes_anchor_reject", { id });
        setAnchorResult("Rejected. Nothing was signed.");
      }
    } catch (e) {
      setAnchorResult(message(e));
    } finally {
      setBusy(false);
    }
    await load();
  };

  const rows = report ? meteringRows(report) : [];
  const anchor = chain?.anchor;
  const bench = chain?.benchmark;
  const waiting = anchor?.sidecar?.pendingDays?.length ?? 0;

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 12, fontSize: 12.5 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
        <span style={{ fontWeight: 520, flex: 1 }}>Hermes daily report ({day}, UTC)</span>
        <button data-testid="hm-today" className="btn btn-ghost btn-sm" onClick={() => setDaysBack(0)} disabled={daysBack === 0}>
          Today
        </button>
        <button data-testid="hm-yesterday" className="btn btn-ghost btn-sm" onClick={() => setDaysBack(1)} disabled={daysBack === 1}>
          Yesterday
        </button>
      </div>
      <span style={{ color: "var(--tx-3)" }}>Only verifier verdicts count as success. A turn no verifier judged is listed as unverified.</span>

      {error && (
        <div data-testid="hm-error" style={{ color: "var(--tx-2)" }}>
          Unknown: {error}
        </div>
      )}
      {report && (
        <>
          <table data-testid="hm-rows" style={{ borderCollapse: "collapse", width: "100%" }}>
            <tbody>
              {rows.map((r, i) => (
                <tr key={r.label} data-testid={`hm-row-${i}`} style={{ borderBottom: "1px solid var(--line-1)" }}>
                  <td style={{ padding: "5px 8px 5px 0", color: "var(--tx-2)" }}>{r.label}</td>
                  <td className="mono tabular" style={{ padding: "5px 0", textAlign: "right", color: r.unknown ? "var(--tx-3)" : "var(--tx-1)" }}>
                    {r.value}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          <span data-testid="hm-source" style={{ color: "var(--tx-3)" }}>
            {report.persisted ? "Records are stored on this device, with no conversation content." : "Records are kept in memory until Hermes restarts, with no conversation content."}
          </span>
        </>
      )}

      <div style={{ borderTop: "1px solid var(--line-1)", paddingTop: 10, display: "flex", flexDirection: "column", gap: 8 }}>
        <span className="eyebrow">On chain</span>
        {chainError && <span style={{ color: "var(--tx-2)" }}>Unknown: {chainError}</span>}
        {anchor && (
          <>
            <span data-testid="hm-anchor-line">{anchor.statusLine}</span>
            {anchor.sidecar && anchor.gate !== "not_deployed" && <span style={{ color: "var(--tx-3)" }}>Closed days waiting to be batched: {waiting}</span>}
            <label style={{ display: "flex", gap: 8, alignItems: "center" }}>
              <input
                data-testid="hm-anchor-toggle"
                type="checkbox"
                checked={anchor.enabled}
                disabled={anchor.gate === "not_deployed" || busy}
                onChange={(e) => void setSettings(e.currentTarget.checked, bench?.sharing ?? false)}
              />
              Anchor each day's decision records on 40204 (each day waits for your approval)
            </label>
            {anchor.pending.map((c) => (
              <div key={c.id} style={{ border: "1px solid var(--line-2)", borderRadius: "var(--r-2)", padding: "8px 10px", display: "flex", flexDirection: "column", gap: 4 }}>
                <span>{c.decoded.action}</span>
                <span className="mono" style={{ fontSize: 10.5, color: "var(--tx-3)" }}>
                  {c.origin} · {c.decoded.destination} · {c.decoded.cost}
                </span>
                <span style={{ display: "flex", gap: 8 }}>
                  <button data-testid={`hm-anchor-approve-${c.id}`} className="btn btn-secondary btn-sm" onClick={() => void decide(c.id, true)} disabled={busy}>
                    Approve and send
                  </button>
                  <button data-testid={`hm-anchor-reject-${c.id}`} className="btn btn-ghost btn-sm" onClick={() => void decide(c.id, false)} disabled={busy}>
                    Reject
                  </button>
                </span>
              </div>
            ))}
            {anchorResult && <span data-testid="hm-anchor-result">{anchorResult}</span>}
          </>
        )}
        {bench && (
          <>
            <label style={{ display: "flex", gap: 8, alignItems: "center" }}>
              <input
                data-testid="hm-bench-toggle"
                type="checkbox"
                checked={bench.sharing}
                disabled={!bench.deployed || busy}
                onChange={(e) => void setSettings(anchor?.enabled ?? false, e.currentTarget.checked)}
              />
              Share daily aggregates with BenchmarkRegistry (counts only, no content)
            </label>
            <span data-testid="hm-bench-line" style={{ color: "var(--tx-3)" }}>
              {bench.statusLine}
            </span>
          </>
        )}
        {chain && chain.pendingOwnerSignOff.length > 0 && (
          <details style={{ color: "var(--tx-3)" }}>
            <summary>Some anchoring details are pending owner sign-off</summary>
            <ul style={{ margin: "6px 0 0", paddingLeft: 18 }}>
              {chain.pendingOwnerSignOff.map((d) => (
                <li key={d}>{d}</li>
              ))}
            </ul>
          </details>
        )}
      </div>
    </div>
  );
}
