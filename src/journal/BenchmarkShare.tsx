// =====================================================================
// HUP-S7.5 (US-7.3 AC3): share a closed day's Hermes aggregates with BenchmarkRegistry.
//
// Off unless the member turned sharing on. Core fetches the sidecar's calls, rebuilds and checks
// each one, and raises one wallet approval card per metric; nothing is signed or sent here, and
// nothing reaches the chain until the member approves each card.
// =====================================================================
import { useState } from "react";
import type { ShareView } from "./proofView";
import type { ReportInvoke } from "./HermesDailyReport";

function message(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

export function BenchmarkShare({ invoke, sharing, day }: { invoke: ReportInvoke; sharing: boolean; day: string }) {
  const [view, setView] = useState<ShareView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const share = async () => {
    if (busy || !sharing) return;
    setBusy(true);
    try {
      setView(await invoke<ShareView>("hermes_benchmark_share", { day }));
      setError(null);
    } catch (e) {
      setError(message(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
      <button data-testid="bs-share" className="btn btn-ghost btn-sm" style={{ alignSelf: "flex-start" }} onClick={() => void share()} disabled={!sharing || busy}>
        Share the numbers of {day} (UTC)
      </button>
      {error && (
        <span data-testid="bs-error" style={{ color: "var(--tx-2)" }}>
          {error}
        </span>
      )}
      {view && (
        <>
          <span data-testid="bs-result">
            {view.cards.length} approval cards are waiting in your wallet review, one per number. Nothing is sent until you approve each one.
          </span>
          <ul style={{ margin: 0, paddingLeft: 18, color: "var(--tx-3)" }}>
            {view.calls.map((c) => (
              <li key={c.metric} className="mono" style={{ fontSize: 11 }}>
                {c.metric}: {c.value}
              </li>
            ))}
          </ul>
          {view.pendingOwnerSignOff.length > 0 && (
            <details style={{ color: "var(--tx-3)" }}>
              <summary>Some sharing details are pending owner sign-off</summary>
              <ul style={{ margin: "6px 0 0", paddingLeft: 18 }}>
                {view.pendingOwnerSignOff.map((d) => (
                  <li key={d}>{d}</li>
                ))}
              </ul>
            </details>
          )}
        </>
      )}
    </div>
  );
}
