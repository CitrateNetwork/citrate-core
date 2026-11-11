// Bridge impl — the services registry snapshot (K1a-2) and sidecar health (CORE-D4), TAURI. Rust
// `services.rs`: `services_snapshot` and `sidecars_health`, each row read from that service's kit
// supervisor; read-only. Alerts (CORE-D3) arrive on the `sidecars://alert` event.
import { listen } from "@tauri-apps/api/event";
import { invoke } from "./invoke";
import type { DiagnosticsExport, ServicesDomain, ServicesSnapshot, SidecarAlert, SidecarsHealth } from "../domains";

/** The event `services.rs` emits when a sidecar becomes Failed or recovers from it. */
export const SIDECAR_ALERT_EVENT = "sidecars://alert";

export const tauriServices: ServicesDomain = {
  snapshot() {
    return invoke<ServicesSnapshot>("services_snapshot");
  },
  health(lines?: number) {
    return invoke<SidecarsHealth>("sidecars_health", { lines: lines ?? null });
  },
  async onAlert(cb: (alert: SidecarAlert) => void) {
    return listen<SidecarAlert>(SIDECAR_ALERT_EVENT, (ev) => cb(ev.payload));
  },
  // CORE-D5: `sidecar_logs.rs` `diagnostics_logs_export` (local file; nothing is sent).
  exportDiagnostics(path: string) {
    return invoke<DiagnosticsExport>("diagnostics_logs_export", { path });
  },
};
