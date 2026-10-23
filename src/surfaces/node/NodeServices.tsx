// =====================================================================
// citrate-core — Node › Services (core plan K3 over K1; design slice 2 "Node services")
//
// A table of the nine supervised processes with their process phase and readiness (D-12), a
// bounded log and Restart, using only the commands that exist today (services.ts names each one).
// Readiness shows "—" until a status DTO carries it (SC2). Only the node has a log read (node_logs);
// the others say so. Restart runs the service's own stop and start, waits for the old process to be
// reported stopped, and is refused while a process is Starting, Stopping or Quarantined.
// =====================================================================
import { useCallback, useEffect, useState } from "react";
import { bridge } from "../../bridge";
import { BRIDGE_MODE } from "../../bridge/mode";
import { tauriServiceCommands } from "../../bridge/tauri/services";
import type { Store } from "../../shell/store";
import type { AppState } from "../../shell/state";
import {
  PHASE_LABEL,
  READINESS_LABEL,
  RESTART_DEFAULTS,
  SERVICES,
  canRestart,
  phaseReader,
  readServices,
  restartService,
  type Phase,
  type RestartOptions,
  type RestartableId,
  type ServiceControls,
  type ServiceId,
  type ServiceReads,
  type ServiceRow,
  type StatusDto,
} from "./services";
import { BoundedLog, PAGE, Pill, StateCard, logCaption, type PillTone } from "./parts";

/** How often the table re-reads the services while it is open. */
const POLL_MS = 3_000;

const PHASE_TONE: Record<Phase, PillTone> = {
  stopped: "muted",
  starting: "info",
  alive: "ok",
  backoff: "warn",
  failed: "danger",
  stopping: "warn",
  quarantined: "danger",
};

/** The live reads (Rule 7: each names its command in services.ts). */
export function liveServiceReads(): ServiceReads {
  return {
    node: () => bridge.node.status(),
    agent: () => bridge.agent.status(),
    memory: () => bridge.memory.status(),
    llama: () => tauriServiceCommands.serveStatus(),
    ipfs: () => tauriServiceCommands.ipfsStatus(),
    // hermes_status returns the sidecar's lifecycle HermesStatus { state, healthy } (hermes.rs).
    hermes: () => bridge.agentHarness.status() as unknown as Promise<StatusDto>,
    commsRelay: () => bridge.groups.relayStatus(),
  };
}

/**
 * The live stop/start pairs. The node and memory go through the store so its state stays right.
 * Each start resolves only once its command answered and rejects on failure, so Restart never
 * reports a failed start as "Started." (Codex on #362). The node's start runs after Restart's own
 * status read reported it stopped, so it doesn't consult the store's last-polled state.
 */
export function liveServiceControls(store: Store): ServiceControls {
  return {
    node: { stop: async () => store.stopNode(), start: () => store.startNodeAfterStop() },
    agent: { stop: () => bridge.agent.stop(), start: () => bridge.agent.start() },
    memory: {
      stop: () => bridge.memory.stop(),
      start: async () => {
        const failure = await store.startMemoryDaemon();
        if (failure != null) throw new Error(failure);
      },
    },
    llama: { stop: () => bridge.model.serveStop(), start: () => bridge.model.serveStart() },
    ipfs: { stop: () => tauriServiceCommands.ipfsStop(), start: () => bridge.node.startIpfs() },
    hermes: { stop: () => bridge.agentHarness.stop(), start: () => bridge.agentHarness.start() },
  };
}

export interface NodeServicesProps {
  store: Store;
  s: AppState;
  mode?: "tauri" | "sim";
  reads?: ServiceReads;
  controls?: ServiceControls;
  restartOptions?: RestartOptions;
}

export function NodeServices({ store, s, mode = BRIDGE_MODE, reads, controls, restartOptions = RESTART_DEFAULTS }: NodeServicesProps) {
  const [r] = useState<ServiceReads>(() => reads ?? liveServiceReads());
  const [c] = useState<ServiceControls>(() => controls ?? liveServiceControls(store));
  const [rows, setRows] = useState<ServiceRow[] | null>(null);
  const [sel, setSel] = useState<ServiceId>("node");
  const [busy, setBusy] = useState<ServiceId | null>(null);
  const [lastResult, setLastResult] = useState<string | null>(null);

  const read = useCallback(async () => {
    setRows(await readServices(r));
  }, [r]);

  useEffect(() => {
    if (mode !== "tauri") return;
    let live = true;
    const tick = () => {
      void readServices(r).then((x) => live && setRows(x));
    };
    tick();
    const t = setInterval(tick, POLL_MS);
    return () => {
      live = false;
      clearInterval(t);
    };
  }, [mode, r]);

  if (mode !== "tauri") {
    return (
      <div data-node-tab="services" style={PAGE}>
        <StateCard
          kind="unavailable"
          title="Services need the desktop app"
          body="The supervisor lives in the app's Rust side. The web preview has no supervised processes, so none are listed."
          why="BRIDGE_MODE = sim"
        />
      </div>
    );
  }

  const readable = rows?.filter((x) => SERVICES.find((d) => d.id === x.id)?.statusSource.startsWith("none yet") === false) ?? [];
  const allFailed = rows != null && readable.length > 0 && readable.every((x) => x.error);
  if (allFailed) {
    const first = readable.find((x) => x.error)?.error ?? "no answer";
    return (
      <div data-node-tab="services" style={PAGE}>
        <StateCard
          kind="error"
          title="Couldn't read the services"
          body="None of the status reads answered. The services keep running; their states are unknown here until a read answers."
          why={"node_status, agent_status, memory_status, model_serve_status, ipfs_status, hermes_status → error: " + first}
          cta={{ label: "Try again", go: () => void read() }}
        />
      </div>
    );
  }

  const alive = rows?.filter((x) => x.phase === "alive").length ?? 0;
  const anyRunning = rows?.some((x) => x.phase && x.phase !== "stopped" && x.phase !== "failed") ?? false;
  const countLine =
    rows == null ? "reading…" : SERVICES.length + " supervised · " + (anyRunning ? alive + " alive" : "none started yet");

  const onRestart = async (row: ServiceRow) => {
    if (!canRestart(row.id, row.phase).ok) return;
    setBusy(row.id);
    const id = row.id as RestartableId;
    const out = await restartService(id, c, phaseReader(id, r), restartOptions, row.phase);
    setBusy(null);
    const name = SERVICES.find((d) => d.id === id)?.name ?? id;
    setLastResult(name + ": " + out.message);
    store.toast(name + ": " + out.message);
    void read();
  };

  const selDef = SERVICES.find((d) => d.id === sel) ?? SERVICES[0];
  const selRow = rows?.find((x) => x.id === sel);
  const nodeLog = sel === "node";

  return (
    <div data-node-tab="services" style={{ ...PAGE, display: "grid", gridTemplateColumns: "minmax(0,1fr) 400px", alignItems: "start" }}>
      <div className="surface" style={{ display: "flex", flexDirection: "column", minWidth: 0 }}>
        <div style={{ display: "flex", alignItems: "center", gap: 10, padding: "13px 18px", borderBottom: "1px solid var(--line-1)" }}>
          <span style={{ fontSize: 14, fontWeight: 500 }}>Services</span>
          <span className="mono" style={{ fontSize: 10.5, color: "var(--tx-3)" }}>
            {countLine}
          </span>
        </div>
        <div style={{ display: "grid", gridTemplateColumns: "minmax(0,1fr) 110px 110px 150px", gap: 10, padding: "8px 18px", borderBottom: "1px solid var(--line-1)" }}>
          {["service", "process", "readiness", ""].map((h) => (
            <span key={h} className="mono" style={{ fontSize: 9.5, letterSpacing: ".1em", textTransform: "uppercase", color: "var(--tx-3)" }}>
              {h}
            </span>
          ))}
        </div>
        {SERVICES.map((d) => {
          const row = rows?.find((x) => x.id === d.id);
          const phase = row?.phase ?? null;
          const gate = canRestart(d.id, phase);
          const label = phase === "stopped" || phase === "failed" ? "Start" : "Restart";
          return (
            <div
              key={d.id}
              data-service={d.id}
              style={{ display: "grid", gridTemplateColumns: "minmax(0,1fr) 110px 110px 150px", gap: 10, alignItems: "center", padding: "10px 18px", borderBottom: "1px solid var(--line-1)", background: sel === d.id ? "var(--srf-1)" : "transparent" }}
            >
              <span style={{ minWidth: 0, display: "flex", flexDirection: "column", gap: 2 }}>
                <span style={{ fontSize: 13, fontWeight: 500 }}>{d.name}</span>
                <span style={{ fontSize: 11.5, color: row?.error ? "var(--danger)" : "var(--tx-3)", overflowWrap: "anywhere" }}>
                  {row?.error ? "read failed: " + row.error : row?.note || d.desc}
                </span>
              </span>
              <span>
                {phase ? (
                  <Pill tone={PHASE_TONE[phase]} data-phase="">
                    {PHASE_LABEL[phase]}
                  </Pill>
                ) : (
                  <span data-phase="" className="mono" style={{ fontSize: 11, color: "var(--tx-3)" }}>
                    —
                  </span>
                )}
              </span>
              <span>
                {row?.readiness ? (
                  <Pill tone={row.readiness === "ready" ? "ok" : row.readiness === "unavailable" ? "danger" : row.readiness === "stale" ? "warn" : "muted"} data-readiness="">
                    {READINESS_LABEL[row.readiness]}
                  </Pill>
                ) : (
                  <span data-readiness="" className="mono" style={{ fontSize: 11, color: "var(--tx-3)" }}>
                    —
                  </span>
                )}
              </span>
              <span style={{ display: "flex", gap: 6, justifyContent: "flex-end" }}>
                <button className="btn btn-ghost btn-sm" data-action="log" onClick={() => setSel(d.id)}>
                  Log
                </button>
                <button
                  className="btn btn-ghost btn-sm"
                  data-action="restart"
                  disabled={!gate.ok || busy != null}
                  title={gate.ok ? (d.restartSource ?? "") : gate.why}
                  onClick={() => void onRestart(row ?? { id: d.id, phase: null, readiness: null, note: "" })}
                >
                  {busy === d.id ? "…" : label}
                </button>
              </span>
            </div>
          );
        })}
        <p style={{ fontSize: 11.5, lineHeight: 1.55, color: "var(--tx-3)", margin: 0, padding: "12px 18px" }}>
          Alive only proves the process exists. Readiness is a separate probe result; this build's status reads don't carry it yet, so it shows “—”. A service with no status command shows “—”, never Stopped.
        </p>
      </div>

      <div className="surface" style={{ display: "flex", flexDirection: "column", minWidth: 0 }}>
        <div style={{ padding: "14px 18px", borderBottom: "1px solid var(--line-1)", display: "flex", flexDirection: "column", gap: 6 }}>
          <span style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
            <span style={{ fontSize: 15, fontWeight: 500, flex: 1 }}>{selDef.name}</span>
            {selRow?.phase && <Pill tone={PHASE_TONE[selRow.phase]}>{PHASE_LABEL[selRow.phase]}</Pill>}
          </span>
          <span style={{ fontSize: 12, lineHeight: 1.5, color: "var(--tx-2)" }}>{selDef.desc}</span>
          {lastResult && (
            <span className="mono" style={{ fontSize: 10.5, color: "var(--tx-3)" }}>
              {lastResult}
            </span>
          )}
        </div>
        {selRow?.phase === "quarantined" && (
          <div style={{ margin: "12px 18px 0", padding: "12px 14px", borderRadius: "var(--r-2)", border: "1px solid var(--danger)", background: "var(--danger-bg)", fontSize: 12.5, lineHeight: 1.55 }}>
            Quarantined. Citrate waits for the old process to end. It never frees the slot on a timer and never restarts on its own; Restart opens once cleanup finishes.
          </div>
        )}
        <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "10px 18px 6px" }}>
          <span className="eyebrow">Log</span>
          <span className="mono" style={{ fontSize: 10, color: "var(--tx-3)", marginLeft: "auto" }}>
            {nodeLog ? logCaption(s.logs.length) : "no log read"}
          </span>
        </div>
        <div style={{ margin: "0 18px 14px" }}>
          {nodeLog ? (
            <BoundedLog testid="service-log" lines={s.logs} maxHeight={260} empty="No output: the node isn't running. Nothing is shown from a template." />
          ) : (
            <div data-testid="service-log" className="mono" style={{ fontSize: 10.5, color: "var(--tx-3)", background: "var(--srf-inset)", border: "1px solid var(--line-1)", borderRadius: "var(--r-1)", padding: "10px 12px", lineHeight: 1.5 }}>
              No log read for this service yet. Only the node's log is exposed (node_logs); bounded capture for every service is planned (SC4).
            </div>
          )}
        </div>
        <div style={{ padding: "12px 18px", display: "flex", flexDirection: "column", gap: 7, borderTop: "1px solid var(--line-1)" }}>
          {[
            ["status", selDef.statusSource],
            ["restart", selDef.restartSource ?? "none yet"],
            ["log", selDef.logSource ?? "none yet (SC4)"],
            ["module", "src-tauri/src/" + selDef.module],
          ].map(([k, v]) => (
            <div key={k} style={{ display: "flex", gap: 10, alignItems: "baseline" }}>
              <span className="mono" style={{ fontSize: 9.5, letterSpacing: ".1em", textTransform: "uppercase", color: "var(--tx-3)", width: 70, flexShrink: 0 }}>
                {k}
              </span>
              <span style={{ fontSize: 12, lineHeight: 1.5, overflowWrap: "anywhere" }}>{v}</span>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
