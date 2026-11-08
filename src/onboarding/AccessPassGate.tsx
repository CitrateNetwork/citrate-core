// Download gate: the first-run access check. Wraps onboarding. While the check is off (every build
// without CITRATE_ACCESS_GATE_URL), or this install is already recorded, it renders onboarding as is.
// When the build checks and this is a fresh install, it asks for the access code from the download
// email first. Rust decides everything (src-tauri/src/access_pass.rs); nothing here grants access.
import { useEffect, useRef, useState, type ReactNode } from "react";
import type { AppState } from "../shell/state";
import type { Store } from "../shell/store";
import { ACCESS_OFF, accessPassStatus, accessPassVerify, type AccessPassView } from "./accessPass";

type Phase = { kind: "loading" } | { kind: "ready"; view: AccessPassView } | { kind: "error"; message: string };

/** Whether onboarding may show (pure, for tests). */
export function accessAllowsSetup(phase: Phase): boolean {
  return phase.kind === "ready" && !phase.view.required;
}

export function AccessPassGate({ store, s, children }: { store: Store; s: AppState; children: ReactNode }) {
  const [phase, setPhase] = useState<Phase>({ kind: "loading" });
  useEffect(() => {
    let live = true;
    accessPassStatus()
      .then((view) => live && setPhase({ kind: "ready", view }))
      .catch((e) => live && setPhase({ kind: "error", message: String((e as Error)?.message ?? e) }));
    return () => {
      live = false;
    };
  }, []);
  if (phase.kind === "loading") return null;
  if (phase.kind === "error") {
    return <AccessPanel store={store} s={s} startError={phase.message} onDone={(view) => setPhase({ kind: "ready", view })} />;
  }
  if (accessAllowsSetup(phase)) return <>{children}</>;
  return <AccessPanel store={store} s={s} onDone={(view) => setPhase({ kind: "ready", view })} />;
}

function AccessPanel({ store, s, startError, onDone }: { store: Store; s: AppState; startError?: string; onDone: (v: AccessPassView) => void }) {
  const ref = useRef<HTMLInputElement>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(startError ?? null);
  useEffect(() => {
    if (s.pendingAccessLink && ref.current && !ref.current.value) ref.current.value = s.pendingAccessLink;
  }, [s.pendingAccessLink]);
  const submit = async () => {
    const v = ref.current?.value.trim() ?? "";
    if (!v) return;
    setBusy(true);
    setError(null);
    try {
      const view = await accessPassVerify(v);
      store.setState({ pendingAccessLink: null });
      onDone(view.gate === "off" ? ACCESS_OFF : view);
    } catch (e) {
      setError(String((e as Error)?.message ?? e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <div style={{ display: "flex", alignItems: "center", justifyContent: "center", height: "100%" }} data-testid="access-pass">
      <div style={{ display: "flex", flexDirection: "column", gap: 12, maxWidth: 480, padding: 24 }}>
        <h1 style={{ fontSize: 22, margin: 0 }}>Enter your access code</h1>
        <span style={{ fontSize: 13.5, lineHeight: 1.6, color: "var(--tx-2)" }}>
          Citrate Core is opening to people in small groups. Paste the access code from your download email, or open the
          citrate://access link in it on this computer.
        </span>
        <span style={{ display: "flex", gap: 8 }}>
          <input
            ref={ref}
            className="input mono"
            data-testid="access-pass-input"
            placeholder="Access code or citrate://access link"
            style={{ flex: 1, minWidth: 220 }}
            onKeyDown={(e) => e.key === "Enter" && void submit()}
          />
          <button className="btn btn-primary btn-sm" data-testid="access-pass-submit" disabled={busy} onClick={() => void submit()}>
            {busy ? "Checking…" : "Continue"}
          </button>
        </span>
        {error && (
          <span role="alert" style={{ fontSize: 12, color: "var(--danger)", lineHeight: 1.5 }}>
            {error}
          </span>
        )}
        <span className="mono" style={{ fontSize: 11, color: "var(--tx-3)" }}>
          The code is checked once and kept for this computer. It is not a group invite.
        </span>
      </div>
    </div>
  );
}
