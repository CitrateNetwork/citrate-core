// =====================================================================
// HUP-S7.5 — the Hermes daily report, as rows.
//
// Data source: the Hermes sidecar's `GET /metering/daily` (via the `hermes_metering_daily`
// command). Records are derived from the agent loop's event stream; tokens come from the model
// provider's own usage report. A value the sidecar does not have is shown as "unknown", never as
// zero, and every D-27 measure this build does not collect is listed as unknown.
// =====================================================================

export interface DailyReportWire {
  schema: number;
  day: string;
  turns: number;
  sessions: number;
  outcomes: { answered: number; stopped: number; step_limit: number; failed: number; unknown: number };
  verification: { passed: number; failed: number; unverified: number };
  verified_success_bps: number | null;
  latency_ms: { p50: number; p95: number; max: number } | null;
  tokens: { tokens_in: number; tokens_out: number; turns_reporting: number };
  steps_total: number;
  tool_calls: Record<string, { calls: number; ok: number; denied: number; error: number; hic_required: number }>;
  verifiers: Record<string, { passed: number; failed: number }>;
  models: Record<string, number>;
  tainted_turns: number;
}

export interface DailyResponse {
  day: string;
  source: "log" | "memory";
  persisted: boolean;
  report: DailyReportWire;
  markdown: string;
  notMeasured: string[];
}

export interface AnchorCard {
  id: string;
  origin: string;
  day: number;
  date: string;
  commitment: string;
  registry: string;
  chainId: number;
  decoded: { action: string; cost: string; destination: string };
}

export interface ChainStatus {
  anchor: {
    gate: "not_deployed" | "off" | "ready";
    statusLine: string;
    registry: string | null;
    enabled: boolean;
    anchorKey: string | null;
    anchorKeyError: string | null;
    sidecar: { configured?: boolean; recordsPresent?: boolean; pendingDays?: { day: number; date: string }[]; awaitingConfirmation?: unknown[]; anchored?: unknown[]; incomplete?: unknown[] } | null;
    sidecarError: string | null;
    pending: AnchorCard[];
    submitted: unknown[];
  };
  benchmark: { registry: string | null; deployed: boolean; sharing: boolean; statusLine: string };
  pendingOwnerSignOff: string[];
}

export interface AnchorApprove {
  receipt: { day: number; commitment: string; txHash: string; blockNumber: number | null; status: number | null };
  anchored: boolean;
  statusLine: string;
}

export interface MeteringRow {
  label: string;
  value: string;
  unknown: boolean;
}

function row(label: string, value: string, unknown = false): MeteringRow {
  return { label, value, unknown };
}

/** The report as labelled rows; unknown values are marked and never shown as zero. */
export function meteringRows(d: DailyResponse): MeteringRow[] {
  const r = d.report;
  const rows: MeteringRow[] = [
    row("Turns", String(r.turns)),
    row("Sessions", String(r.sessions)),
    row("Verified passed / failed / unverified", `${r.verification.passed} / ${r.verification.failed} / ${r.verification.unverified}`),
  ];
  rows.push(
    r.verified_success_bps === null
      ? row("Verified success rate", "unknown (no turn was judged by a verifier)", true)
      : row("Verified success rate", `${Math.floor(r.verified_success_bps / 100)}.${String(r.verified_success_bps % 100).padStart(2, "0")}%`),
  );
  rows.push(row("Answered / stopped / step limit / failed", `${r.outcomes.answered} / ${r.outcomes.stopped} / ${r.outcomes.step_limit} / ${r.outcomes.failed}`));
  rows.push(r.latency_ms === null ? row("Latency p50 / p95", "unknown (no turns)", true) : row("Latency p50 / p95", `${r.latency_ms.p50} ms / ${r.latency_ms.p95} ms`));
  rows.push(
    r.tokens.turns_reporting === 0
      ? row("Tokens in / out", "unknown (the model did not report usage)", true)
      : row("Tokens in / out", `${r.tokens.tokens_in} / ${r.tokens.tokens_out} (reported for ${r.tokens.turns_reporting} of ${r.turns} turns)`),
  );
  rows.push(row("Model steps", String(r.steps_total)));
  rows.push(row("Turns that read untrusted content", String(r.tainted_turns)));
  for (const m of d.notMeasured) rows.push(row(m, "unknown (not measured yet)", true));
  return rows;
}

/** `YYYY-MM-DD` of the UTC day `daysBack` days before `now`. */
export function utcDay(now: Date, daysBack: number): string {
  const d = new Date(Date.UTC(now.getUTCFullYear(), now.getUTCMonth(), now.getUTCDate() - daysBack));
  return d.toISOString().slice(0, 10);
}
