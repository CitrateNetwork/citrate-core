// =====================================================================
// HUP-S7.5 — the Hermes daily report, as rows.
//
// Data source: the Hermes sidecar's `GET /metering/daily` (via the `hermes_metering_daily`
// command). Records are derived from the agent loop's event stream; tokens come from the model
// provider's own usage report. A value the sidecar does not have is shown as "unknown", never as
// zero, and every D-27 measure this build does not collect is listed as unknown.
//
// HUP-S7.5 (D-27, US-7.3 AC1): time to first token and tokens per second come from llama-server's
// own timings; CPU/GPU/RAM peaks from the sidecar sampling the machine during each turn; the
// energy figure is an estimate and says so; the self-review is the model's opinion, never a
// verdict; SALT spent and gas come from the receipts of Hermes's own transactions (the ceremony
// that sent them reports them). An older sidecar omits these fields: they show as unknown.
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
  /** D-27 (report schema 2). Absent from an older sidecar; null when no turn reported it. */
  ttft_ms?: { p50: number; p95: number; max: number } | null;
  speed?: { tokens: number; generation_ms: number; tokens_per_s_milli: number; turns_reporting: number } | null;
  resources?: { turns_sampled: number; cpu_peak_bps: number; ram_used_peak_bytes: number; ram_total_bytes: number; gpu_peak_bps: number | null } | null;
  energy_estimate?: { label: string; microwatt_hours: number; turns_estimated: number; turns_without_gpu: number } | null;
  self_review?: { label: string; pass: number; fail: number; unclear: number; agreed_with_verifiers: number; disagreed_with_verifiers: number };
}

/** D-27: the day's Hermes transactions on chain (wei amounts are decimal strings). */
export interface ChainSpendWire {
  day: string;
  transactions: number;
  reverted: number;
  gasUsed: number;
  feeWei: string;
  valueWei: string;
  saltSpentWei: string;
  byPurpose: Record<string, number>;
}

export interface DailyResponse {
  day: string;
  source: "log" | "memory";
  persisted: boolean;
  report: DailyReportWire;
  /** D-27: absent from an older sidecar. */
  chain?: ChainSpendWire;
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
  rows.push(...d27Rows(d));
  for (const m of d.notMeasured) rows.push(row(m, "unknown (not measured yet)", true));
  return rows;
}

const OLD_SIDECAR = "unknown (this Hermes build does not measure it)";

const pct = (bps: number) => `${Math.floor(bps / 100)}.${String(bps % 100).padStart(2, "0")}%`;
const mib = (bytes: number) => `${Math.floor(bytes / (1024 * 1024))} MiB`;

/** `wei` (a decimal string) as SALT, trailing zeros trimmed; null when it is not a whole number. */
export function formatSalt(wei: string): string | null {
  if (!/^\d{1,39}$/.test(wei)) return null;
  const n = BigInt(wei);
  const unit = 10n ** 18n;
  const whole = n / unit;
  const frac = n % unit;
  if (frac === 0n) return `${whole} SALT`;
  return `${whole}.${frac.toString().padStart(18, "0").replace(/0+$/, "")} SALT`;
}

/** HUP-S7.5 (D-27): the measures beyond counts, each labelled; unknowns say why. */
export function d27Rows(d: DailyResponse): MeteringRow[] {
  const r = d.report;
  const rows: MeteringRow[] = [];
  if (r.ttft_ms === undefined) rows.push(row("Time to first token p50 / p95", OLD_SIDECAR, true));
  else if (r.ttft_ms === null) rows.push(row("Time to first token p50 / p95", "unknown (the model server did not report it)", true));
  else rows.push(row("Time to first token p50 / p95", `${r.ttft_ms.p50} ms / ${r.ttft_ms.p95} ms (the model server's own timing)`));

  if (r.speed === undefined) rows.push(row("Tokens per second", OLD_SIDECAR, true));
  else if (r.speed === null) rows.push(row("Tokens per second", "unknown (no generation time reported)", true));
  else rows.push(row("Tokens per second", `${(r.speed.tokens_per_s_milli / 1000).toFixed(1)} (over ${r.speed.turns_reporting} turns)`));

  const peakLabel = "Peak CPU / GPU / RAM (whole machine)";
  if (r.resources === undefined) rows.push(row(peakLabel, OLD_SIDECAR, true));
  else if (r.resources === null) rows.push(row(peakLabel, "unknown (no turn was long enough to sample)", true));
  else {
    const x = r.resources;
    const gpu = x.gpu_peak_bps === null ? "GPU unknown" : pct(x.gpu_peak_bps);
    rows.push(row(peakLabel, `${pct(x.cpu_peak_bps)} / ${gpu} / ${mib(x.ram_used_peak_bytes)} of ${mib(x.ram_total_bytes)}`));
  }

  const energyLabel = "Energy (estimate, not measured)";
  if (r.energy_estimate === undefined) rows.push(row(energyLabel, OLD_SIDECAR, true));
  else if (r.energy_estimate === null) rows.push(row(energyLabel, "unknown (no turn was sampled)", true));
  else {
    const e = r.energy_estimate;
    const mwh = (e.microwatt_hours / 1000).toFixed(3);
    const note = e.turns_without_gpu > 0 ? `; GPU left out of ${e.turns_without_gpu}` : "";
    rows.push(row(energyLabel, `${mwh} mWh, ${e.label} over ${e.turns_estimated} turns${note}`));
  }

  const reviewLabel = "Self-review (opinion, not a verdict)";
  const sr = r.self_review;
  if (sr === undefined) rows.push(row(reviewLabel, OLD_SIDECAR, true));
  else if (sr.pass + sr.fail + sr.unclear === 0) rows.push(row(reviewLabel, "none recorded"));
  else rows.push(row(reviewLabel, `PASS ${sr.pass} / FAIL ${sr.fail} / unclear ${sr.unclear} (${sr.label}; agreed with verifiers ${sr.agreed_with_verifiers}, disagreed ${sr.disagreed_with_verifiers})`));

  const c = d.chain;
  if (c === undefined) {
    rows.push(row("SALT spent on chain", OLD_SIDECAR, true));
    rows.push(row("Gas used", OLD_SIDECAR, true));
  } else if (c.transactions === 0) {
    rows.push(row("SALT spent on chain", "none (no Hermes transaction this day)"));
    rows.push(row("Gas used", "0 (no Hermes transaction this day)"));
  } else {
    const spent = formatSalt(c.saltSpentWei);
    const fee = formatSalt(c.feeWei);
    const value = formatSalt(c.valueWei);
    rows.push(
      spent === null
        ? row("SALT spent on chain", "unknown (unreadable amount)", true)
        : row("SALT spent on chain", `${spent} (gas ${fee ?? "unknown"}, value sent ${value ?? "unknown"})`),
    );
    rows.push(row("Gas used", `${c.gasUsed} over ${c.transactions} transactions${c.reverted > 0 ? `, ${c.reverted} reverted` : ""}`));
  }
  return rows;
}

/** `YYYY-MM-DD` of the UTC day `daysBack` days before `now`. */
export function utcDay(now: Date, daysBack: number): string {
  const d = new Date(Date.UTC(now.getUTCFullYear(), now.getUTCMonth(), now.getUTCDate() - daysBack));
  return d.toISOString().slice(0, 10);
}
