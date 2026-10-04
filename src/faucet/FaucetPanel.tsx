// HUP-S6.5: Settings → Budgets: the in-app faucet for deploy gas.
//
// Off by default. Turning it on is the member's own decision (HIC-1), confirmed here, and it
// grants a small HIC-2 budget: core may ask the Citrate faucet for SALT for the member's own
// wallet, once per 24 hours, only when a deploy the member started is short of gas. The member
// can turn it off at any time. Hermes has no route to the switch. The values are placeholders
// pending owner sign-off, listed at the bottom.
import { useCallback, useEffect, useState } from "react";
import {
  FAUCET_UNAVAILABLE_OUTSIDE_DESKTOP,
  faucetApi,
  formatSalt,
  formatWhen,
  healthLine,
  nextEligible,
  outcomeLabel,
  shortAddress,
  type FaucetApi,
  type FaucetResult,
  type FaucetStatus,
} from "./faucet";

const muted = { fontSize: 11.5, color: "var(--tx-3)" } as const;

export function FaucetPanel({
  initial,
  api: apiProp,
}: {
  /** Pre-loaded status (tests / static render). `undefined` loads from core on mount. */
  initial?: FaucetStatus | null;
  api?: FaucetApi | null;
}) {
  const api = apiProp === undefined ? faucetApi() : apiProp;
  const [st, setSt] = useState<FaucetStatus | null>(initial ?? null);
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [confirming, setConfirming] = useState(false);

  const load = useCallback(async () => {
    if (!api) return;
    try {
      setSt(await api.status());
      setErr(null);
    } catch (e) {
      setErr(String(e instanceof Error ? e.message : e));
    }
  }, [api]);

  useEffect(() => {
    if (initial === undefined) void load();
  }, [initial, load]);

  const act = async (f: () => Promise<unknown>) => {
    setBusy(true);
    try {
      await f();
      setErr(null);
      setConfirming(false);
      await load();
    } catch (e) {
      setErr(String(e instanceof Error ? e.message : e));
    } finally {
      setBusy(false);
    }
  };

  if (!api && !st) {
    return (
      <div className="surface" style={{ padding: 18 }} data-testid="faucet-panel">
        <span className="eyebrow">Faucet for deploy gas</span>
        <p style={{ fontSize: 13, marginTop: 8 }}>{FAUCET_UNAVAILABLE_OUTSIDE_DESKTOP}</p>
      </div>
    );
  }
  if (!st) {
    return (
      <div className="surface" style={{ padding: 18 }} data-testid="faucet-panel">
        <span className="eyebrow">Faucet for deploy gas</span>
        <p style={{ ...muted, marginTop: 8 }}>
          {err ? "Could not read the faucet settings: " + err : "Loading the faucet settings…"}
        </p>
      </div>
    );
  }

  const next = nextEligible(st);
  const stateLine = !st.enabled
    ? "Off. Core never asks the faucet for you."
    : st.walletMatches
      ? `On for ${shortAddress(st.wallet)}: at most ${st.maxPerWindow} top-up every ${st.windowHours} hours, only when a deploy you started needs gas.`
      : `On for ${shortAddress(st.budget?.wallet ?? null)}, which is not your current wallet. It does nothing until you turn it on again for this wallet.`;

  return (
    <div
      className="surface"
      data-testid="faucet-panel"
      style={{ padding: 18, display: "flex", flexDirection: "column", gap: 8 }}
    >
      <span className="eyebrow">Faucet for deploy gas</span>
      <p style={{ fontSize: 13, margin: 0 }} data-testid="faucet-state">
        {stateLine}
      </p>
      <p style={{ ...muted, margin: 0 }}>
        The Citrate faucet sends {formatSalt(st.dripWei, 2)} from its own account. The request
        carries no signature and your keys never leave the app. Deploys still need your approval
        every time.
      </p>
      <p
        style={{ fontSize: 12, margin: 0, color: st.health.reachable ? "var(--tx-2)" : "var(--danger)" }}
        data-testid="faucet-health"
      >
        {healthLine(st.health)}
      </p>
      {next !== null && (
        <p style={{ fontSize: 12, margin: 0 }} data-testid="faucet-next">
          Next top-up possible: {formatWhen(next)}
        </p>
      )}
      {st.walletError && (
        <p style={{ ...muted, margin: 0 }}>Wallet: {st.walletError}</p>
      )}
      {st.storeError && (
        <p style={{ fontSize: 12, margin: 0, color: "var(--danger)" }}>{st.storeError}</p>
      )}

      {confirming ? (
        <div
          role="group"
          aria-label="Confirm the faucet budget"
          style={{ border: "1px solid var(--line-2)", borderRadius: "var(--r-1)", padding: 12, display: "flex", flexDirection: "column", gap: 6 }}
        >
          <strong style={{ fontSize: 13 }}>Allow faucet top-ups for deploy gas?</strong>
          <ul style={{ margin: 0, paddingLeft: 18, fontSize: 12, lineHeight: 1.5 }}>
            <li>Only for your wallet {shortAddress(st.wallet)}; nobody can choose another address.</li>
            <li>Only when a deploy you started (and the deploy gate marked READY) is short of gas.</li>
            <li>At most {st.maxPerWindow} request every {st.windowHours} hours. Refusals are shown, never retried.</li>
            <li>You can turn it off here at any time.</li>
          </ul>
          <div style={{ display: "flex", gap: 8 }}>
            <button
              className="btn btn-primary btn-sm"
              disabled={busy || !st.wallet}
              onClick={() => api && void act(() => api.grant())}
            >
              Allow
            </button>
            <button className="btn btn-secondary btn-sm" disabled={busy} onClick={() => setConfirming(false)}>
              Cancel
            </button>
          </div>
        </div>
      ) : (
        <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
          {st.enabled && st.walletMatches ? (
            <button
              className="btn btn-secondary btn-sm"
              disabled={busy || !api}
              onClick={() => api && void act(() => api.revoke())}
            >
              Turn off
            </button>
          ) : (
            <button
              className="btn btn-primary btn-sm"
              disabled={busy || !api || !st.wallet}
              onClick={() => setConfirming(true)}
            >
              {st.enabled ? "Turn on for this wallet" : "Turn on"}
            </button>
          )}
          <button
            className="btn btn-secondary btn-sm"
            disabled={busy || !api || !st.wallet}
            onClick={() => api && void act(() => api.openChallenge())}
            title={st.faucetPage}
          >
            Open the faucet page
          </button>
        </div>
      )}
      {err && <p style={{ fontSize: 12, margin: 0, color: "var(--danger)" }}>{err}</p>}

      {st.ledger.length > 0 && (
        <div style={{ borderTop: "1px solid var(--line-1)", paddingTop: 8, display: "flex", flexDirection: "column", gap: 4 }}>
          <span className="mono" style={muted}>
            Requests
          </span>
          {st.ledger.map((e) => (
            <div key={e.atMs + e.origin} data-testid="faucet-ledger-row" style={{ fontSize: 12 }}>
              <span className="mono" style={{ marginRight: 6 }}>
                {formatWhen(e.atMs)}
              </span>
              {outcomeLabel(e.outcome)} · asked by {e.origin}
              {e.txHash ? ` · tx ${e.txHash.slice(0, 10)}…` : ""}
              {e.outcome !== "sent" && e.message ? ` · ${e.message}` : ""}
            </div>
          ))}
        </div>
      )}

      <details>
        <summary style={muted}>Pending owner sign-off</summary>
        <ul style={{ ...muted, margin: "6px 0 0", paddingLeft: 18 }}>
          {st.pendingOwnerSignOff.map((l) => (
            <li key={l}>{l}</li>
          ))}
        </ul>
      </details>
    </div>
  );
}

/**
 * HUP-S6.5: shown under a READY deploy in the signing review: ask core for a deploy-gas top-up
 * for exactly this deploy. Renders nothing outside the desktop app. Core decides whether a
 * request is sent (switch on, balance short, budget not used); the answer is shown as it is.
 */
export function FaucetTopUp({
  initcodeHash,
  api: apiProp,
}: {
  initcodeHash: string;
  api?: FaucetApi | null;
}) {
  const api = apiProp === undefined ? faucetApi() : apiProp;
  const [busy, setBusy] = useState(false);
  const [res, setRes] = useState<FaucetResult | null>(null);
  const [err, setErr] = useState<string | null>(null);
  if (!api) return null;
  const ask = async () => {
    setBusy(true);
    try {
      setRes(await api.request(initcodeHash));
      setErr(null);
    } catch (e) {
      setErr(String(e instanceof Error ? e.message : e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <div data-testid="faucet-topup" style={{ display: "flex", flexDirection: "column", gap: 4, fontSize: 12 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
        <button className="btn btn-secondary btn-sm" disabled={busy} onClick={() => void ask()}>
          {busy ? "Asking the faucet…" : "Short of gas? Ask the faucet"}
        </button>
        {res?.outcome === "challenge_required" && (
          <button className="btn btn-secondary btn-sm" onClick={() => void api.openChallenge()}>
            Open the faucet page
          </button>
        )}
      </div>
      {res && (
        <span data-testid="faucet-topup-result" role="status">
          {res.message}
        </span>
      )}
      {err && <span style={{ color: "var(--danger)" }}>{err}</span>}
    </div>
  );
}
