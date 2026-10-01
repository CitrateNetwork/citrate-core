// HUP-S2.3 — Settings → Budgets. Create, view and revoke web-signing (Sign-In with Ethereum)
// budgets, with live countdowns and the "Signed for you" record history.
//
// Every change here is the member's own decision (HIC-1) and goes to core, which re-checks it.
// Hermes has no route to these commands. Auto sign-in additionally needs core to attest the page
// origin through the managed browser, which is not in this build: the panel says so plainly.
import { useCallback, useEffect, useState } from "react";
import {
  BUDGETS_UNAVAILABLE_OUTSIDE_DESKTOP,
  budgetsApi,
  formatCountdown,
  normalizeStatus,
  recordLabel,
  recordStatusLabel,
  statusLabel,
  validateGrantForm,
  type BudgetsApi,
  type WebBudgetStatus,
} from "./budgets";

const muted = { fontSize: 11.5, color: "var(--tx-3)" } as const;

function when(ms: number): string {
  try {
    return new Date(ms).toLocaleString();
  } catch {
    return String(ms);
  }
}

export function BudgetsPanel({
  initial,
  unavailable,
  api: apiProp,
}: {
  /** Pre-loaded status (tests / static render). `undefined` loads from core on mount. */
  initial?: WebBudgetStatus | null;
  /** Shown instead of the panel when budgets cannot be reached (outside the desktop app). */
  unavailable?: string;
  api?: BudgetsApi | null;
}) {
  const api = apiProp === undefined ? budgetsApi() : apiProp;
  const [st, setSt] = useState<WebBudgetStatus | null>(
    initial ? normalizeStatus(initial) : null,
  );
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [origin, setOrigin] = useState("");
  const [count, setCount] = useState<number>(initial?.defaults.maxCount ?? 10);
  const [days, setDays] = useState<number>(initial?.defaults.ttlDays ?? 7);
  // Count down from core's clock: offset = core now - local now at read time.
  const [offset, setOffset] = useState(
    initial ? initial.nowMs - Date.now() : 0,
  );
  const [tick, setTick] = useState(initial ? initial.nowMs : Date.now());

  const load = useCallback(async () => {
    if (!api) return;
    try {
      const s = normalizeStatus(await api.status());
      setSt(s);
      setOffset(s.nowMs - Date.now());
      setTick(s.nowMs);
      setErr(null);
    } catch (e) {
      setErr(String(e instanceof Error ? e.message : e));
    }
  }, [api]);

  useEffect(() => {
    if (initial === undefined) void load();
  }, [initial, load]);

  useEffect(() => {
    const t = setInterval(() => setTick(Date.now() + offset), 1000);
    return () => clearInterval(t);
  }, [offset]);

  useEffect(() => {
    if (st && !origin) {
      setCount(st.defaults.maxCount);
      setDays(st.defaults.ttlDays);
    }
    // Only when the defaults first arrive.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [st?.defaults.maxCount, st?.defaults.ttlDays]);

  const act = async (f: () => Promise<unknown>) => {
    setBusy(true);
    try {
      await f();
      setErr(null);
      await load();
    } catch (e) {
      setErr(String(e instanceof Error ? e.message : e));
    } finally {
      setBusy(false);
    }
  };

  if (unavailable || (!api && !st)) {
    return (
      <div className="surface" style={{ padding: 18 }}>
        <span className="eyebrow">Budgets</span>
        <p style={{ fontSize: 13, marginTop: 8 }}>
          {unavailable ?? BUDGETS_UNAVAILABLE_OUTSIDE_DESKTOP}
        </p>
      </div>
    );
  }
  if (!st) {
    return (
      <div className="surface" style={{ padding: 18 }}>
        <span className="eyebrow">Budgets</span>
        <p style={{ ...muted, marginTop: 8 }}>
          {err ? "Could not read budgets: " + err : "Loading budgets…"}
        </p>
      </div>
    );
  }

  const failed =
    st.snapshot.health.state === "failed"
      ? (st.snapshot.health as { reason: string }).reason
      : null;
  const formErr = origin
    ? validateGrantForm(origin, count, days, st.ceilings)
    : null;
  const live = st.snapshot.budgets.filter((b) => b.revokedAtMs == null);

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
      <div
        className="surface"
        style={{
          padding: 18,
          display: "flex",
          flexDirection: "column",
          gap: 8,
        }}
      >
        <span className="eyebrow">Sign-in budgets</span>
        <p style={{ fontSize: 13, margin: 0 }}>
          A budget lets Hermes sign in with your wallet (Sign-In with Ethereum
          only) on one https site you choose, up to a set number of times, until
          it expires or you revoke it. Transactions, permits and other
          signatures always ask you. With no budget, every sign-in asks you.
        </p>
        <p style={{ ...muted, margin: 0 }} data-testid="attestation">
          {st.attestation.available
            ? "Automatic sign-in is available for sites with an active budget."
            : st.attestation.reason}
        </p>
        <p style={{ ...muted, margin: 0 }}>
          Limits on top of every budget: one automatic sign-in per site every{" "}
          {st.rate.minGapSeconds} s, at most {st.rate.windowMax} per site in any{" "}
          {st.rate.windowHours} h, chain 40204 only, and never while the task
          has read content from another site.
        </p>
      </div>

      {failed && (
        <div
          className="surface"
          style={{
            padding: 18,
            display: "flex",
            flexDirection: "column",
            gap: 8,
            borderColor: "var(--danger)",
          }}
        >
          <span className="eyebrow" style={{ color: "var(--danger)" }}>
            Budgets are off
          </span>
          <p style={{ fontSize: 13, margin: 0 }}>
            {failed}. Every sign-in asks you until you reset budgets.
          </p>
          <div>
            <button
              className="btn btn-secondary btn-sm"
              disabled={busy || !api}
              onClick={() => api && void act(() => api.reset())}
            >
              Reset budgets
            </button>
          </div>
        </div>
      )}

      <div
        className="surface"
        style={{
          padding: 18,
          display: "flex",
          flexDirection: "column",
          gap: 10,
        }}
      >
        <span className="eyebrow">Grant a budget</span>
        <div
          style={{
            display: "grid",
            gridTemplateColumns: "minmax(0,1fr) 90px 90px auto",
            gap: 8,
            alignItems: "end",
          }}
        >
          <label
            style={{
              display: "flex",
              flexDirection: "column",
              gap: 4,
              fontSize: 11.5,
            }}
          >
            Site (https origin)
            <input
              className="input"
              value={origin}
              placeholder="https://app.example.org"
              onChange={(e) => setOrigin(e.target.value)}
            />
          </label>
          <label
            style={{
              display: "flex",
              flexDirection: "column",
              gap: 4,
              fontSize: 11.5,
            }}
          >
            Sign-ins
            <input
              className="input"
              type="number"
              min={1}
              max={st.ceilings.maxCount}
              value={count}
              onChange={(e) => setCount(Number(e.target.value))}
            />
          </label>
          <label
            style={{
              display: "flex",
              flexDirection: "column",
              gap: 4,
              fontSize: 11.5,
            }}
          >
            Days
            <input
              className="input"
              type="number"
              min={1}
              max={st.ceilings.ttlDays}
              value={days}
              onChange={(e) => setDays(Number(e.target.value))}
            />
          </label>
          <button
            className="btn btn-secondary btn-sm"
            disabled={
              busy ||
              !api ||
              !!failed ||
              !origin ||
              !!formErr ||
              !st.walletAddress
            }
            onClick={() =>
              api &&
              void act(() =>
                api.grant(origin.trim(), count, days).then(() => setOrigin("")),
              )
            }
          >
            Grant budget
          </button>
        </div>
        {formErr && (
          <span style={{ fontSize: 11.5, color: "var(--warn)" }}>
            {formErr}
          </span>
        )}
        {!st.walletAddress && (
          <span style={muted}>
            Unlock your wallet to grant a budget. A budget is tied to one
            wallet.
          </span>
        )}
        <span style={muted}>
          Suggested values ({st.defaults.maxCount} sign-ins,{" "}
          {st.defaults.ttlDays} days) are placeholders
          {st.defaults.pendingOwnerSignoff ? ", pending owner sign-off" : ""}.
          Ceiling: {st.ceilings.maxCount} sign-ins and {st.ceilings.ttlDays}{" "}
          days.
        </span>
      </div>

      <div
        className="surface"
        style={{
          padding: 18,
          display: "flex",
          flexDirection: "column",
          gap: 10,
        }}
      >
        <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
          <span className="eyebrow" style={{ flex: 1 }}>
            Budgets
          </span>
          <button
            className="btn btn-ghost btn-sm"
            disabled={busy || !api || live.length === 0}
            onClick={() => api && void act(() => api.revokeAll())}
          >
            Revoke all
          </button>
        </div>
        {st.snapshot.budgets.length === 0 ? (
          <span style={muted}>No budgets. Every sign-in asks you.</span>
        ) : (
          st.snapshot.budgets.map((b) => {
            const active = b.status === "active";
            return (
              <div
                key={b.id}
                style={{ display: "flex", alignItems: "center", gap: 12 }}
              >
                <span style={{ flex: 1, minWidth: 0 }}>
                  <span
                    className="mono"
                    style={{
                      display: "block",
                      fontSize: 12.5,
                      overflowWrap: "anywhere",
                    }}
                  >
                    {b.origin}
                  </span>
                  <span style={{ ...muted, display: "block" }}>
                    {b.usedCount} of {b.maxCount} used · {b.remaining} left ·{" "}
                    {b.revokedAtMs != null
                      ? "revoked " + when(b.revokedAtMs)
                      : "ends in " + formatCountdown(b.expiresAtMs - tick)}
                  </span>
                </span>
                <span
                  className="mono"
                  style={{
                    fontSize: 10,
                    color: active ? "var(--ok)" : "var(--tx-3)",
                  }}
                >
                  {statusLabel(b.status)}
                </span>
                {b.revokedAtMs == null && (
                  <button
                    className="btn btn-ghost btn-sm"
                    disabled={busy || !api}
                    onClick={() => api && void act(() => api.revoke(b.id))}
                  >
                    Revoke
                  </button>
                )}
              </div>
            );
          })
        )}
      </div>

      <div
        className="surface"
        style={{
          padding: 18,
          display: "flex",
          flexDirection: "column",
          gap: 8,
        }}
      >
        <span className="eyebrow">Decision records</span>
        <span style={muted}>
          Every grant, revoke and automatic signature is recorded and
          hash-chained ({st.snapshot.recordCount} so far). Head{" "}
          <span className="mono">{st.snapshot.headHash.slice(0, 18)}…</span>
        </span>
        {st.snapshot.records.length === 0 ? (
          <span style={muted}>No decisions yet.</span>
        ) : (
          st.snapshot.records.map((r) => (
            <div
              key={r.recordId}
              style={{
                fontSize: 12,
                borderTop: "1px solid var(--line)",
                paddingTop: 6,
              }}
            >
              <span style={{ fontWeight: 500 }}>{recordLabel(r.kind)}</span>
              {r.origin ? <span className="mono"> · {r.origin}</span> : null}
              {r.kind === "auto_sign" && (
                <span> · {recordStatusLabel(r.status)}</span>
              )}
              <span style={{ ...muted, display: "block" }}>
                {when(r.atMs)}
                {r.statement ? " · “" + r.statement + "”" : ""}
                {r.nonce ? " · nonce " + r.nonce : ""}
                {r.remainingAfter != null && r.kind === "auto_sign"
                  ? " · " + r.remainingAfter + " left"
                  : ""}
                {r.note ? " · " + r.note : ""}
              </span>
            </div>
          ))
        )}
      </div>

      {err && (
        <span style={{ fontSize: 12, color: "var(--danger)" }}>{err}</span>
      )}
    </div>
  );
}
