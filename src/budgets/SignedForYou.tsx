// HUP-S2.3: the "Signed for you" notice (Rule-3 ADR D4, after-the-fact visibility #1).
//
// Right after core signs a sign-in inside a budget, it emits `web-budget://auto-signed` to the main
// window. This shows a non-modal notice for each one: where Hermes signed in, how many automatic
// sign-ins are left, and a one-click "Revoke this budget". The same record is in Settings, Budgets.
// Nothing here signs; revoking goes through core's `web_budget_revoke`.
import { useEffect, useState, type CSSProperties } from "react";
import { BRIDGE_MODE } from "../bridge/mode";
import { AUTO_SIGNED_EVENT, parseAutoSignedNotice, signedForYouText, type AutoSignedNotice } from "./signIn";
import { tauriBudgets } from "./budgets";

/** At most this many notices are shown at once (oldest drop first). */
export const MAX_NOTICES = 3;

export type Subscribe = (handler: (payload: unknown) => void) => Promise<() => void>;

async function tauriSubscribe(handler: (payload: unknown) => void): Promise<() => void> {
  const { listen } = await import("@tauri-apps/api/event");
  return listen<unknown>(AUTO_SIGNED_EVENT, (ev) => handler(ev.payload));
}

const wrap: CSSProperties = { position: "fixed", right: 20, bottom: 72, zIndex: 79, display: "flex", flexDirection: "column", gap: 8, maxWidth: 380 };
const card: CSSProperties = { display: "flex", flexDirection: "column", gap: 8, padding: "12px 14px", borderRadius: "var(--r-2)", border: "1px solid var(--line-2)", background: "var(--srf-0)", boxShadow: "var(--shadow-lift)", fontSize: 12.5, lineHeight: 1.45 };

type Item = AutoSignedNotice & { key: number; revoked: boolean; error: string | null };

export function SignedForYou({ subscribe, revoke }: { subscribe?: Subscribe; revoke?: (budgetId: number) => Promise<void> }) {
  const [items, setItems] = useState<Item[]>([]);
  const sub = subscribe ?? (BRIDGE_MODE === "tauri" ? tauriSubscribe : null);
  const doRevoke = revoke ?? ((id: number) => tauriBudgets.revoke(id));

  useEffect(() => {
    if (!sub) return;
    let alive = true;
    let stop: (() => void) | null = null;
    let n = 0;
    void sub((payload) => {
      const notice = parseAutoSignedNotice(payload);
      if (!notice || !alive) return;
      n += 1;
      const key = n;
      setItems((prev) => [...prev, { ...notice, key, revoked: false, error: null }].slice(-MAX_NOTICES));
    }).then((u) => {
      if (alive) stop = u;
      else u();
    });
    return () => {
      alive = false;
      if (stop) stop();
    };
  }, [sub]);

  if (items.length === 0) return null;
  const dismiss = (key: number) => setItems((prev) => prev.filter((i) => i.key !== key));
  const onRevoke = async (it: Item) => {
    try {
      await doRevoke(it.budgetId);
      setItems((prev) => prev.map((i) => (i.budgetId === it.budgetId ? { ...i, revoked: true, error: null } : i)));
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      setItems((prev) => prev.map((i) => (i.key === it.key ? { ...i, error: msg } : i)));
    }
  };

  return (
    <div style={wrap} role="status" aria-live="polite" data-testid="signed-for-you">
      {items.map((it) => (
        <div key={it.key} style={card} data-register="charter">
          <span className="mono" style={{ fontSize: 10, letterSpacing: ".12em", textTransform: "uppercase", color: "var(--tx-2)" }}>
            Signed for you
          </span>
          <span>{signedForYouText(it)}</span>
          {it.revoked ? <span style={{ color: "var(--tx-2)" }}>Budget revoked. The next sign-in on this site asks you.</span> : null}
          {it.error ? <span style={{ color: "var(--danger)" }}>{it.error}</span> : null}
          <div style={{ display: "flex", gap: 8, justifyContent: "flex-end" }}>
            {!it.revoked ? (
              <button className="btn btn-sm" onClick={() => void onRevoke(it)}>
                Revoke this budget
              </button>
            ) : null}
            <button className="btn btn-sm btn-ghost" onClick={() => dismiss(it.key)}>
              Dismiss
            </button>
          </div>
        </div>
      ))}
    </div>
  );
}
