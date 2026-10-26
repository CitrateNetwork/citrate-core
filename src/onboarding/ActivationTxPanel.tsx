// Join-8 — a recorded activation's status with its way out, shared by S7 and Node (#406): Pending
// (with Forget once it has gone unseen past 10 minutes) or reverted (with Try again). The same
// store actions on both surfaces: Forget and Try again mark the durable record forgotten first.
import { LoaderMark } from "../components/LoaderMark";
import type { Store } from "../shell/store";
import type { AppState } from "../shell/state";
import { activationPhase } from "./activateValidator";

const fmtI = (n: number) => Math.round(n).toLocaleString("en-US");
const short = (h: string) => (h ? h.slice(0, 6) + "…" + h.slice(-4) : "—");

/** True when there is a recorded activation tx this panel has something to show for. */
export function hasActivationTxStatus(store: Store, s: AppState): boolean {
  const phase = activationPhase(s.activation, store.walletIsLinked(), s.s7TxHash);
  return (phase === "pending" && !!s.s7TxHash) || (phase === "failed" && !!s.activation?.tx);
}

export function ActivationTxPanel({ store, s, testId }: { store: Store; s: AppState; testId: string }) {
  const a = s.activation;
  const phase = activationPhase(a, store.walletIsLinked(), s.s7TxHash);
  const pendingTx = a?.tx && (!s.s7TxHash || a.tx.hash === s.s7TxHash) ? a.tx : null;
  return (
    <>
      {phase === "pending" && (s.s7TxHash || pendingTx) && (
        <div className="surface" style={{ display: "flex", alignItems: "center", gap: 16, padding: "18px 20px" }} data-testid={`${testId}-pending`}>
          <div style={{ width: 40, height: 40, flexShrink: 0 }}>
            <LoaderMark size={40} />
          </div>
          <div style={{ display: "flex", flexDirection: "column", gap: 4 }}>
            <div style={{ fontSize: 14, fontWeight: 500 }}>Pending</div>
            <div className="mono" style={{ fontSize: 11, color: "var(--tx-3)" }}>
              {pendingTx?.included === true && pendingTx.blockNumber != null
                ? `Included in block ${fmtI(pendingTx.blockNumber)} · waiting for the bond to read activated`
                : `tx ${short(pendingTx?.hash ?? s.s7TxHash ?? "")} · not in a block yet`}
            </div>
            {store.canForgetActivationTx(Date.now()) && (
              <div style={{ display: "flex", alignItems: "center", gap: 10, marginTop: 6 }}>
                <button className="btn btn-ghost btn-sm" onClick={() => void store.onS7ForgetTx()} data-testid={`${testId}-forget`}>
                  Forget this transaction
                </button>
                <span style={{ fontSize: 11.5, color: "var(--tx-3)" }}>
                  It is not seen on the network yet after 10 minutes; it may still be included. If it was sent, a second activation reverts and costs only gas.
                </span>
              </div>
            )}
          </div>
        </div>
      )}

      {phase === "failed" && a?.tx && (
        <div style={{ border: "1px solid var(--danger)", background: "var(--danger-bg)", borderRadius: "var(--r-2)", padding: "16px 18px", display: "flex", flexDirection: "column", gap: 10 }} data-testid={`${testId}-failed`}>
          <div style={{ fontSize: 14, fontWeight: 500, color: "var(--danger)" }}>Activation transaction reverted</div>
          <p style={{ fontSize: 13, lineHeight: 1.55, color: "var(--tx-2)", margin: 0 }}>
            tx {short(a.tx.hash)} was included in block {a.tx.blockNumber != null ? fmtI(a.tx.blockNumber) : "—"} and reverted. The bond is unchanged
            (still locked, not staked). Check the activation conditions, then try again.
          </p>
          <div>
            <button
              className="btn btn-ghost btn-sm"
              onClick={() => void store.onS7RetryAfterRevert()}
              data-testid={`${testId}-retry`}
            >
              Try again
            </button>
          </div>
        </div>
      )}
    </>
  );
}
