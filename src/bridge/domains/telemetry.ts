// citrate-core: bridge domain `telemetry`, moved verbatim from src/bridge/domains.ts by L0-WP05
// (planset v060). src/bridge/domains.ts re-exports it, so imports from "../bridge/domains" keep
// working. The owning lane is the row for this path in .agentile/v60-ownership.map.

/** A scrubbed diagnostic bundle (Telemetry WP-T.2/T.3). Mirrors the Rust DiagnosticBundle. */
export interface DiagnosticBundle {
  reportId: string;
  appVersion: string;
  os: string;
  crashTail: string;
  nodeLogTail: string;
  uiErrors: string[];
  /** 0.5.2 Join: the join step ids reached since the last sent report (bare ids, allowlisted in
   *  Rust; see src/privacy/joinFunnel.ts). Empty when telemetry is off. */
  joinSteps: string[];
}

export interface TelemetryDomain {
  /** WP-T.2/T.3 — assemble the SCRUBBED bundle locally for review. No network. `uiErrors` is
   *  the frontend error ring; `joinSteps` the pending join step ids (joinFunnel). Returns
   *  already-scrubbed content the member can inspect. */
  bundle(uiErrors: string[], joinSteps?: string[]): Promise<DiagnosticBundle>;
  /** WP-T.4 — the ONE pinned HTTPS POST. Call ONLY after the member reviewed + consented
   *  (ConsentGate, WP-T.1). Sends exactly the reviewed bundle JSON. Honest error if the
   *  ingest service (WP-T.5) isn't live. */
  send(bundleJson: string): Promise<void>;
}
