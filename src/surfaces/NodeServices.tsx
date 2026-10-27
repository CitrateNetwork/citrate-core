// =====================================================================
// citrate-core — Node › Services (K1a-2, RC52).
//
// Data source (Rule 7): `services_snapshot` (Rust `services.rs`), one row per supervised service,
// each read from that service's kit supervisor (the SPEC-001 lifecycle cell, its last phase
// transition, restart count and last crash). The registry half of each row (owner, prerequisites,
// probe, ready signal, log budget) is the static registry in the same file.
//
// Truth rules for this tab:
// - Every state word is the one the snapshot reported for that row. A row with no supervisor reads
//   "Not supervised", a read that didn't get the lock reads "Unknown", and a phase word this build
//   doesn't know reads "Unknown" too. Nothing comes from app state, seeds or earlier reads: a failed
//   read clears the rows and says why.
// - Readiness is its own line (SC2). Alive never reads Ready unless the snapshot says Ready.
// - Start, Stop and Reset appear only for the services the registry marks `controls` (their owner's
//   own start/stop path), and only in a phase they apply to: a Failed or still-held Stopped service
//   offers Reset (the owner's stop, which releases the slot), and Start appears once the slot is
//   empty. A control in flight says "requested" until the next snapshot shows what happened; the
//   state word never moves ahead of it. A refused control shows the owner's reason on its row.
// - Logs: only the node has a log read (`node_logs`, on Node › Advanced). The other eight say
//   "not available yet" (N-8, SC4).
// =====================================================================
import { useCallback, useEffect, useRef, useState } from "react";
import { SurfaceProps } from "./shared";
import type { Store } from "../shell/store";
import { bridge, isUnavailable } from "../bridge";
import { startRuntime, stopRuntime } from "../shell/slices/agent";
import type { ServiceEntry, ServiceId, ServicePhase, ServiceReadiness, ServicesSnapshot } from "../bridge/domains";

/** How often the tab re-reads the snapshot while it is open. */
const POLL_MS = 2000;

export const PHASE_LABEL: Record<ServicePhase, string> = {
  stopped: "Stopped",
  starting: "Starting",
  alive: "Alive",
  backoff: "Restarting after a crash",
  failed: "Failed",
  stopping: "Stopping",
  quarantined: "Quarantined",
};

export const OBSERVATION_LABEL = {
  notSupervised: "Not supervised",
  busy: "Unknown",
} as const;

const READINESS_LABEL: Record<ServiceReadiness, string> = {
  notAssessed: "Not assessed",
  awaiting: "Awaiting",
  ready: "Ready",
  stale: "Stale",
  unavailable: "Unavailable",
};

export type ControlAction = "start" | "stop" | "reset";

/** The last refused control on a row and the owner's reason. */
export interface Refusal {
  action: ControlAction;
  reason: string;
}

/**
 * The owner paths Node › Services drives, for exactly the registry rows marked `controls: true`
 * (a test holds the two lists equal). Each goes through the same path the rest of the app uses, so
 * the owner's own checks and side effects (node intent, memory import, Hermes settings snapshot)
 * still apply. Each resolves to the owner's refusal reason, or null when nothing refused it.
 *
 * Reset is the owner's stop. An owner refuses a Start while any supervisor holds its slot, including
 * a Failed one, and its stop is what releases the slot. So a Failed (or a still-held Stopped) row
 * offers Reset, and Start appears once the snapshot shows the slot empty.
 */
export const SERVICE_CONTROLS: Partial<Record<ServiceId, (store: Store) => Record<ControlAction, () => Promise<string | null>>>> = {
  node: (store) => ({
    start: () => store.startNode(),
    stop: () => store.stopNode(),
    reset: () => store.stopNode(),
  }),
  memory: (store) => {
    const stop = async (): Promise<string | null> => {
      try {
        await bridge.memory.stop();
      } catch (e) {
        return errorText(e);
      }
      await store.refreshMemoryStatus();
      return null;
    };
    return { start: () => store.startMemoryDaemon(), stop, reset: stop };
  },
  hermes: () => ({
    start: () => startRuntime("hermes"),
    stop: () => stopRuntime("hermes"),
    reset: () => stopRuntime("hermes"),
  }),
};

const ACTION_LABEL: Record<ControlAction, string> = { start: "Start", stop: "Stop", reset: "Reset" };

/** The state word for a row: the reported phase, or what the read was when there is none. */
function stateWord(e: ServiceEntry): string {
  if (e.observation === "notSupervised") return OBSERVATION_LABEL.notSupervised;
  if (e.observation === "observed" && e.phase && Object.prototype.hasOwnProperty.call(PHASE_LABEL, e.phase)) {
    return PHASE_LABEL[e.phase];
  }
  return OBSERVATION_LABEL.busy;
}

function phaseWord(p: ServicePhase | null): string {
  return p && Object.prototype.hasOwnProperty.call(PHASE_LABEL, p) ? PHASE_LABEL[p] : "Unknown";
}

/** "N s ago" / "N min ago" / "N h ago", measured against the snapshot's own read time. */
export function ago(takenAt: number, at: number): string {
  const s = Math.max(0, Math.round((takenAt - at) / 1000));
  if (s < 60) return `${s} s ago`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m} min ago`;
  return `${Math.floor(m / 60)} h ago`;
}

function stateColor(e: ServiceEntry): string {
  if (e.observation !== "observed") return "var(--tx-3)";
  switch (e.phase) {
    case "alive":
      return "var(--ok)";
    case "failed":
    case "backoff":
    case "quarantined":
      return "var(--warn, var(--tx-2))";
    default:
      return "var(--tx-2)";
  }
}

/** Which control (if any) applies to this row now. */
function controlFor(e: ServiceEntry): ControlAction | null {
  if (!e.controls || !SERVICE_CONTROLS[e.id]) return null;
  if (e.observation === "notSupervised") return "start";
  if (e.observation !== "observed") return null;
  switch (e.phase) {
    // A supervisor still holds the slot: its owner refuses a Start until its stop releases it.
    case "stopped":
    case "failed":
      return "reset";
    case "starting":
    case "alive":
    case "backoff":
      return "stop";
    default:
      return null;
  }
}

const mono = { fontSize: 10, color: "var(--tx-3)" } as const;

function ServiceRow({
  e,
  takenAt,
  pending,
  refusal,
  onControl,
  onOpenLogs,
}: {
  e: ServiceEntry;
  takenAt: number;
  pending: ControlAction | undefined;
  refusal: Refusal | undefined;
  onControl: (id: ServiceId, action: ControlAction) => void;
  onOpenLogs: () => void;
}) {
  const action = controlFor(e);
  const observedRow = e.observation === "observed";
  return (
    <div
      data-service={e.id}
      data-observation={e.observation}
      data-phase={observedRow ? (e.phase ?? "") : ""}
      style={{ display: "flex", flexDirection: "column", gap: 6, padding: "12px 16px", borderTop: "1px solid var(--line-1)" }}
    >
      <div style={{ display: "flex", alignItems: "center", gap: 12 }}>
        <span style={{ fontSize: 13.5, fontWeight: 500, minWidth: 110 }}>{e.label}</span>
        <span data-testid="svc-state" className="mono" style={{ fontSize: 11, color: stateColor(e) }}>{stateWord(e)}</span>
        {observedRow && e.readiness && (
          <span className="mono" style={mono}>
            Readiness: {READINESS_LABEL[e.readiness] ?? "Unknown"}
          </span>
        )}
        <span style={{ marginLeft: "auto", display: "flex", gap: 8, alignItems: "center" }}>
          {pending && (
            <span className="mono" style={mono}>
              {ACTION_LABEL[pending]} requested
            </span>
          )}
          {action && (
            <button
              className="btn btn-secondary btn-sm"
              data-control={action}
              disabled={pending !== undefined}
              onClick={() => onControl(e.id, action)}
            >
              {ACTION_LABEL[action]}
            </button>
          )}
        </span>
      </div>
      {refusal && (
        <span role="alert" style={{ fontSize: 12, color: "var(--warn, var(--tx-2))" }}>
          {ACTION_LABEL[refusal.action]} refused: {refusal.reason}
        </span>
      )}
      {e.observation === "notSupervised" && (
        <span style={{ fontSize: 12, color: "var(--tx-3)" }}>
          No supervisor holds it in this session: it hasn&apos;t been started, or its owner stopped it.
        </span>
      )}
      {e.observation === "busy" && (
        <span style={{ fontSize: 12, color: "var(--tx-3)" }}>Its state couldn&apos;t be read just now. It is read again shortly.</span>
      )}
      {observedRow && (
        <span className="mono" style={mono}>
          {e.lastTransition
            ? `${e.lastTransition.from ? phaseWord(e.lastTransition.from) + " → " : ""}${phaseWord(e.lastTransition.to)} · ${ago(takenAt, e.lastTransition.atUnixMs)}`
            : "No transition recorded"}
          {" · "}Restarts: {e.restarts ?? "—"}
          {" · "}
          {e.lastCrash ? `Last crash: ${e.lastCrash.exit} · ${ago(takenAt, e.lastCrash.atUnixMs)}` : "No crash recorded"}
        </span>
      )}
      {observedRow && e.phase === "quarantined" && (
        <span role="status" style={{ fontSize: 12, color: "var(--tx-2)" }}>
          {e.quarantine ?? "Owned work from the last stop is still running."} Start is refused until that work ends.
        </span>
      )}
      <span style={{ fontSize: 12 }}>
        {e.logsCommand ? (
          <button className="btn btn-ghost btn-sm" data-logs={e.id} onClick={onOpenLogs}>
            Open its logs on Node › Advanced
          </button>
        ) : (
          <span style={{ color: "var(--tx-3)" }}>Logs: not available yet</span>
        )}
      </span>
      <details>
        <summary className="mono" style={{ ...mono, cursor: "pointer" }}>
          Registry
        </summary>
        <dl style={{ display: "grid", gridTemplateColumns: "max-content 1fr", gap: "4px 12px", fontSize: 12, margin: "8px 0 0" }}>
          <dt style={{ color: "var(--tx-3)" }}>Owner</dt>
          <dd className="mono" style={{ margin: 0, fontSize: 11 }}>{e.owner}</dd>
          <dt style={{ color: "var(--tx-3)" }}>Prerequisites</dt>
          <dd style={{ margin: 0 }}>{e.prerequisites.join("; ")}</dd>
          <dt style={{ color: "var(--tx-3)" }}>Probe</dt>
          <dd style={{ margin: 0 }}>{e.probe}</dd>
          <dt style={{ color: "var(--tx-3)" }}>Ready signal</dt>
          <dd style={{ margin: 0 }}>{e.readySignal}</dd>
          <dt style={{ color: "var(--tx-3)" }}>Log budget</dt>
          <dd style={{ margin: 0 }}>{e.logBudgetLines} lines kept by its supervisor</dd>
        </dl>
      </details>
    </div>
  );
}

/** The tab body for one snapshot. Pure: every word on it comes from `snapshot` or `error`. */
export function ServicesView({
  snapshot,
  error,
  pending,
  refusals,
  onControl,
  onOpenLogs,
}: {
  snapshot: ServicesSnapshot | null;
  error: string | null;
  pending: Partial<Record<ServiceId, ControlAction>>;
  refusals: Partial<Record<ServiceId, Refusal>>;
  onControl: (id: ServiceId, action: ControlAction) => void;
  onOpenLogs: () => void;
}) {
  return (
    <div data-testid="node-services" style={{ padding: "20px 26px 24px", display: "flex", flexDirection: "column", gap: 16, maxWidth: 960 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 14 }}>
        <span style={{ fontFamily: "var(--font-display)", fontWeight: 420, fontSize: 24 }}>Services</span>
        <span className="mono" style={{ marginLeft: "auto", ...mono }}>
          source · services_snapshot · each service&apos;s supervisor
        </span>
      </div>
      <div className="surface" style={{ display: "flex", flexDirection: "column" }}>
        {error ? (
          <div role="alert" style={{ padding: 16, fontSize: 12.5, color: "var(--tx-2)" }}>
            Services couldn&apos;t be read: {error}
          </div>
        ) : !snapshot ? (
          <div style={{ padding: 16, fontSize: 12.5, color: "var(--tx-3)" }}>Reading services…</div>
        ) : (
          snapshot.services.map((e) => (
            <ServiceRow key={e.id} e={e} takenAt={snapshot.takenAtUnixMs} pending={pending[e.id]} refusal={refusals[e.id]} onControl={onControl} onOpenLogs={onOpenLogs} />
          ))
        )}
      </div>
    </div>
  );
}

function errorText(e: unknown): string {
  if (isUnavailable(e)) return "the browser preview supervises no processes; open the desktop app";
  return String((e as Error)?.message ?? e);
}

/** Node › Services: polls `services_snapshot` while open and renders it. */
export function NodeServices({ store }: SurfaceProps) {
  const [snapshot, setSnapshot] = useState<ServicesSnapshot | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState<Partial<Record<ServiceId, ControlAction>>>({});
  const [refusals, setRefusals] = useState<Partial<Record<ServiceId, Refusal>>>({});
  const live = useRef(true);

  const read = useCallback(async () => {
    try {
      const s = await bridge.services.snapshot();
      if (!live.current) return;
      setSnapshot(s);
      setError(null);
    } catch (e) {
      if (!live.current) return;
      // A failed read clears the rows: an earlier read is never shown as the current state.
      setSnapshot(null);
      setError(errorText(e));
    }
  }, []);

  useEffect(() => {
    live.current = true;
    void read();
    const t = setInterval(() => void read(), POLL_MS);
    return () => {
      live.current = false;
      clearInterval(t);
    };
  }, [read]);

  const onControl = useCallback(
    (id: ServiceId, action: ControlAction) => {
      const control = SERVICE_CONTROLS[id];
      if (!control) return;
      setPending((p) => ({ ...p, [id]: action }));
      setRefusals((r) => ({ ...r, [id]: undefined }));
      void control(store)
        [action]()
        .catch((e: unknown) => errorText(e))
        .then((reason) => {
          if (!live.current) return;
          setPending((p) => ({ ...p, [id]: undefined }));
          // The owner's refusal stays on the row until the next control on it.
          if (reason) setRefusals((r) => ({ ...r, [id]: { action, reason } }));
          void read();
        });
    },
    [store, read],
  );

  return <ServicesView snapshot={snapshot} error={error} pending={pending} refusals={refusals} onControl={onControl} onOpenLogs={() => store.go("node/advanced")} />;
}
