// CX bridge impl — telemetry (WP-T.2/T.3/T.4), TAURI.
//
// bundle() assembles a SCRUBBED diagnostic bundle locally (Rust diagnostics_bundle — no
// network); send() is the ONE pinned HTTPS POST (Rust telemetry_send). Consent is enforced by
// the caller (Settings/on-crash prompt, gated on the telemetry toggle) per the ConsentGate
// spec (WP-T.1); nothing egresses here except an explicit send.
import { invoke } from "./invoke";
import type { DiagnosticBundle, TelemetryDomain } from "../domains";

export const tauriTelemetry: TelemetryDomain = {
  bundle(uiErrors: string[]): Promise<DiagnosticBundle> {
    return invoke<DiagnosticBundle>("diagnostics_bundle", { uiErrors });
  },
  async send(bundleJson: string): Promise<void> {
    await invoke("telemetry_send", { bundleJson });
  },
};
