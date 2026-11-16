// =====================================================================
// 0.5.4 — Node › Advanced "Service logs": the newest lines of any of the nine supervised
// services' log files.
//
// Data source (Rule 7): `sidecar_logs_tail` (Rust `sidecar_logs.rs`), which reads
// `<app log dir>/sidecars/<supervisor>.log` and its rotated files. Lines were filtered when
// written (secrets redacted; content-bearing sidecars keep lifecycle and error lines only) and
// are scrubbed again on read. A failed read shows the call and its error; a service with no log
// file yet shows that, never a made-up line.
// =====================================================================
import { useCallback, useEffect, useState } from "react";
import { invoke } from "../../bridge/tauri/invoke";
import type { ServiceId } from "../../bridge/domains";
import { createSlice } from "../../shell/slices/createSlice";
import { BoundedLog, Panel, errText } from "./parts";

export interface SidecarLogTail {
  service: ServiceId;
  file: string;
  lines: string[];
  lifecycleOnly: boolean;
}

export interface ServiceLogsIo {
  tail(service: ServiceId, lines: number): Promise<SidecarLogTail>;
}

export const LIVE_SERVICE_LOGS: ServiceLogsIo = {
  tail: (service, lines) => invoke<SidecarLogTail>("sidecar_logs_tail", { service, lines }),
};

/** The nine services, in the order Node › Services lists them. */
export const SERVICE_LOG_CHOICES: [ServiceId, string][] = [
  ["node", "Node"],
  ["nodeAgent", "Node agent"],
  ["memory", "Memory"],
  ["llamaChat", "Llama chat"],
  ["llamaEmbed", "Llama embed"],
  ["ipfs", "IPFS"],
  ["hermes", "Hermes"],
  ["comms", "Messaging"],
  ["cluster", "Cluster"],
];

/** Lines one read asks for. */
export const SERVICE_LOG_LINES = 300;

/** Which service's log Node › Advanced shows; Node › Services sets it before it navigates. */
export const serviceLogsSlice = createSlice<{ service: ServiceId }>({ service: "node" });

export function openServiceLogs(service: ServiceId): void {
  serviceLogsSlice.set({ service });
}

type Read = { state: "reading" } | { state: "ok"; value: SidecarLogTail } | { state: "error"; error: string };

export function ServiceLogs({ io }: { io: ServiceLogsIo }) {
  const { service } = serviceLogsSlice.use();
  const [read, setRead] = useState<Read>({ state: "reading" });
  const load = useCallback(() => {
    setRead({ state: "reading" });
    io.tail(service, SERVICE_LOG_LINES).then(
      (value) => setRead({ state: "ok", value }),
      (e) => setRead({ state: "error", error: errText(e) }),
    );
  }, [io, service]);
  useEffect(() => {
    load();
  }, [load]);
  const label = SERVICE_LOG_CHOICES.find(([id]) => id === service)?.[1] ?? service;
  const caption = read.state === "ok" ? `${read.value.lines.length} newest lines · ${read.value.file}` : "";
  return (
    <Panel title="Service logs" caption={caption}>
      <div data-testid="service-logs" data-source="sidecar_logs_tail" style={{ margin: "12px 18px 14px", display: "flex", flexDirection: "column", gap: 8 }}>
        <span style={{ display: "flex", gap: 8, alignItems: "center", flexWrap: "wrap" }}>
          <label style={{ fontSize: 12, color: "var(--tx-2)", display: "flex", gap: 6, alignItems: "center" }}>
            Service
            <select className="input mono" aria-label="service whose log to show" value={service} onChange={(e) => openServiceLogs(e.target.value as ServiceId)} style={{ height: 28, fontSize: 12 }}>
              {SERVICE_LOG_CHOICES.map(([id, name]) => (
                <option key={id} value={id}>
                  {name}
                </option>
              ))}
            </select>
          </label>
          <button className="btn btn-ghost btn-sm" onClick={load} disabled={read.state === "reading"}>
            {read.state === "reading" ? "Reading" : "Read again"}
          </button>
        </span>
        {read.state === "error" ? (
          <span role="alert" data-testid="service-logs-error" className="mono" style={{ fontSize: 11.5, color: "var(--danger)" }}>
            Couldn’t read the {label} log (sidecar_logs_tail): {read.error}
          </span>
        ) : read.state === "ok" ? (
          <>
            <BoundedLog
              testid="service-log"
              lines={read.value.lines.map((line, i) => ({ id: i, t: "", line }))}
              empty={`No log lines from ${label} yet: it has not written its log file in this install.`}
            />
            {read.value.lifecycleOnly && (
              <span className="mono" data-testid="service-logs-lifecycle" style={{ fontSize: 10, color: "var(--tx-3)" }}>
                {label} can handle your content, so its log keeps only start, stop and error lines, never what it processed.
              </span>
            )}
          </>
        ) : (
          <span className="mono" style={{ fontSize: 10.5, color: "var(--tx-3)" }}>
            Reading the {label} log…
          </span>
        )}
      </div>
    </Panel>
  );
}
