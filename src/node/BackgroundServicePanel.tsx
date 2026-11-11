// =====================================================================
// V60 SVC-2 — "Keep your node running": the onboarding step (03 §3.5) and Settings › Node.
//
// On by default (owner, 2026-11-07): during setup, a member who has not chosen yet gets the
// service turned on when the step first shows. Turning it off shows the exact opt-out sentence
// and the app runs the node in-process again. Settings › Node adds the power rules.
// Data sources: `svc_status` / `svc_set_enabled` / `svc_power_status` / `svc_power_set` (Rust).
// =====================================================================
import { useEffect, useRef, useState } from "react";
import {
  HOURS,
  SVC_DESKTOP_ONLY,
  desktopSvcIo,
  hh,
  svcLine,
  type PowerPolicy,
  type PowerView,
  type SvcIo,
  type SvcView,
} from "./svc";
import { SVC_ON_COPY, SVC_OPT_OUT_COPY, SVC_POWER_SUMMARY, SVC_STEP_TITLE, SVC_WHAT_RUNS } from "../shell/svcCopy";

export interface BackgroundServicePanelProps {
  io: SvcIo;
  /** "setup" = the onboarding card (applies the default); "settings" = Settings › Node (+ power). */
  variant: "setup" | "settings";
}

/** The panel over the app's IO (loaded once); renders nothing until the IO is ready. */
export function BackgroundServiceStep({ variant }: { variant: "setup" | "settings" }) {
  const [io, setIo] = useState<SvcIo | null>(null);
  useEffect(() => {
    let live = true;
    void desktopSvcIo().then((x) => live && setIo(x));
    return () => {
      live = false;
    };
  }, []);
  return io ? <BackgroundServicePanel io={io} variant={variant} /> : null;
}

export function BackgroundServicePanel({ io, variant }: BackgroundServicePanelProps) {
  const [view, setView] = useState<SvcView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [open, setOpen] = useState(false);
  const [power, setPower] = useState<PowerView | null>(null);
  const applied = useRef(false);

  const apply = async (enabled: boolean) => {
    setBusy(true);
    setError(null);
    try {
      const v = await io.invoke<SvcView>("svc_set_enabled", { enabled });
      setView(v);
      // Off: the node runs in the app again.
      if (!enabled) await io.invoke<void>("node_start", {}).catch(() => undefined);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  useEffect(() => {
    if (io.mode !== "tauri") return;
    let live = true;
    io.invoke<SvcView>("svc_status", {})
      .then((v) => {
        if (!live) return;
        setView(v);
        // The default is on: applied once, at setup, for a member who has not chosen.
        if (variant === "setup" && v.supported && v.enabled === null && !applied.current) {
          applied.current = true;
          void apply(true);
        }
      })
      .catch((e) => live && setError(String(e)));
    if (variant === "settings") {
      io.invoke<PowerView>("svc_power_status", {})
        .then((p) => live && setPower(p))
        .catch(() => undefined);
    }
    return () => {
      live = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [io, variant]);

  const savePower = async (policy: PowerPolicy) => {
    setError(null);
    try {
      setView(await io.invoke<SvcView>("svc_power_set", { policy }));
      setPower(await io.invoke<PowerView>("svc_power_status", {}));
    } catch (e) {
      setError(String(e));
    }
  };

  // The toggle shows the member's choice; before the first answer, the default (on).
  const on = view ? view.enabled !== false : true;

  return (
    <div className="surface" data-testid={`svc-panel-${variant}`} style={{ padding: 18, display: "flex", flexDirection: "column", gap: 10, marginTop: variant === "setup" ? 14 : 0 }}>
      <span className="eyebrow">{variant === "setup" ? SVC_STEP_TITLE : "Background service"}</span>
      {io.mode !== "tauri" ? (
        <span style={{ fontSize: 13, color: "var(--tx-2)" }}>{SVC_DESKTOP_ONLY}</span>
      ) : (
        <>
          <label style={{ display: "flex", alignItems: "center", gap: 10, fontSize: 14 }}>
            <input
              type="checkbox"
              role="switch"
              data-testid="svc-toggle"
              aria-checked={on}
              checked={on}
              disabled={busy || (view !== null && !view.supported)}
              onChange={(e) => void apply(e.target.checked)}
            />
            Keep my node running when the app is closed
          </label>
          {on ? (
            <span style={{ fontSize: 13, color: "var(--tx-2)" }}>{SVC_ON_COPY}</span>
          ) : (
            <span data-testid="svc-opt-out" style={{ fontSize: 13, color: "var(--warn, var(--tx-1))" }}>
              {SVC_OPT_OUT_COPY}
            </span>
          )}
          {view && (
            <span className="mono" data-testid="svc-line" style={{ fontSize: 11, color: "var(--tx-3)" }}>
              {svcLine(view)}
              {view.runsLoggedOut === true ? " · also while you are logged out" : ""}
            </span>
          )}
          {view?.notes.map((n) => (
            <span key={n} style={{ fontSize: 12, color: "var(--tx-2)" }}>
              {n}
            </span>
          ))}
          <button className="btn btn-sm btn-ghost" style={{ alignSelf: "flex-start" }} onClick={() => setOpen(!open)} aria-expanded={open}>
            What runs in the background
          </button>
          {open && (
            <ul data-testid="svc-what-runs" style={{ margin: 0, paddingLeft: 18, fontSize: 12, color: "var(--tx-2)", display: "flex", flexDirection: "column", gap: 4 }}>
              {SVC_WHAT_RUNS.map((w) => (
                <li key={w}>{w}</li>
              ))}
              <li>{SVC_POWER_SUMMARY}</li>
            </ul>
          )}
          {variant === "settings" && view && <PowerRules policy={view.power} power={power} onSave={savePower} />}
          {error && <span style={{ fontSize: 12, color: "var(--err, var(--tx-1))" }}>{error}</span>}
          <span className="mono" style={{ fontSize: 10.5, color: "var(--tx-3)" }}>
            Data sources: service settings file · unit file · the service&apos;s status over its local socket
            {variant === "settings" ? " · " + (power?.source ?? "power state") : ""}
          </span>
        </>
      )}
    </div>
  );
}

function PowerRules({ policy, power, onSave }: { policy: PowerPolicy; power: PowerView | null; onSave: (p: PowerPolicy) => void }) {
  return (
    <div data-testid="svc-power" style={{ display: "flex", flexDirection: "column", gap: 8, borderTop: "1px solid var(--line-1)", paddingTop: 10 }}>
      <span className="lbl">Power</span>
      <label style={{ display: "flex", gap: 8, alignItems: "center", fontSize: 13 }}>
        <input
          type="checkbox"
          data-testid="svc-train-battery"
          checked={policy.trainOnBattery}
          onChange={(e) => onSave({ ...policy, trainOnBattery: e.target.checked })}
        />
        Allow training on battery
      </label>
      {policy.trainOnBattery && (
        <label style={{ display: "flex", gap: 8, alignItems: "center", fontSize: 13 }}>
          Pause below
          <select value={policy.pauseBelowPct} onChange={(e) => onSave({ ...policy, pauseBelowPct: Number(e.target.value) })}>
            {[10, 20, 30, 40, 50, 60, 70, 80].map((p) => (
              <option key={p} value={p}>
                {p} %
              </option>
            ))}
          </select>
        </label>
      )}
      <label style={{ display: "flex", gap: 8, alignItems: "center", fontSize: 13 }}>
        <input
          type="checkbox"
          data-testid="svc-nightly"
          checked={policy.nightly !== null}
          onChange={(e) => onSave({ ...policy, nightly: e.target.checked ? { from: 23, to: 7 } : null })}
        />
        Train only at night
        {policy.nightly && (
          <>
            <select value={policy.nightly.from} onChange={(e) => onSave({ ...policy, nightly: { from: Number(e.target.value), to: policy.nightly?.to ?? 7 } })}>
              {HOURS.map((h) => (
                <option key={h} value={h}>
                  {hh(h)}
                </option>
              ))}
            </select>
            to
            <select value={policy.nightly.to} onChange={(e) => onSave({ ...policy, nightly: { from: policy.nightly?.from ?? 23, to: Number(e.target.value) } })}>
              {HOURS.map((h) => (
                <option key={h} value={h}>
                  {hh(h)}
                </option>
              ))}
            </select>
          </>
        )}
      </label>
      {power && (
        <span className="mono" data-testid="svc-power-now" style={{ fontSize: 11, color: "var(--tx-3)" }}>
          {power.state.onAc === null ? "power state unknown" : power.state.onAc ? "on AC power" : "on battery"}
          {power.state.batteryPct !== null ? ` · ${power.state.batteryPct} %` : ""} · training now: {power.training.allowed ? "allowed" : "paused"} ({power.training.reason})
        </span>
      )}
      <span style={{ fontSize: 11.5, color: "var(--tx-3)" }}>
        These rules are saved for the training worker. No training runs from the app in this release.
      </span>
    </div>
  );
}
