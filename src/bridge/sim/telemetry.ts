// CX bridge impl — telemetry (WP-T.2/T.3/T.4), SIM (web/dev).
//
// Honest: a browser preview has no local crash file / node logs and no pinned egress. bundle()
// returns a minimal, truthful shell (version/os come from the Rust side in a real build; here
// they're marked as the web preview) so the review UI renders; send() throws honestly — the
// web preview never egresses (Rule 1 / WP-T.1).
import type { DiagnosticBundle, TelemetryDomain } from "../domains";
import type { SimHost } from "./index";

export function simTelemetry(_host: SimHost): TelemetryDomain {
  return {
    async bundle(uiErrors: string[]): Promise<DiagnosticBundle> {
      return {
        reportId: "rpt_preview",
        appVersion: typeof __APP_VERSION__ === "string" ? __APP_VERSION__ : "0.0.0",
        os: "web-preview",
        crashTail: "",
        nodeLogTail: "",
        uiErrors,
      };
    },
    async send(): Promise<void> {
      throw new Error("sending a diagnostic report needs the desktop app");
    },
  };
}
