// =====================================================================
// citrate-core: Hermes's adapter tools (0.5.4, Hermes x FL WP-H9, gap analysis §5.5)
//
//   fl_adapter_gate_read     the eval gate's record for an adapter or a round   (read, no approval)
//   fl_adapter_load_propose  load one ACCEPTED adapter into the local model     (write, HIC-1 card)
//   fl_adapter_unload        serve the base model alone again                   (lowers authority, no card)
//
// Data sources (Rule 7): core's src-tauri/src/fl_rounds.rs through bridge.flRounds:
//  - `fl_overview`: the gate records (`fl_rounds.json`), the loaded adapter, the adapter put back
//    after a restart and why a put-back failed;
//  - `fl_adapter_load`: core's authorize_load refuses unless the latest gate record for that hash
//    is ACCEPT and was measured on the base model being served, re-hashes the copy, then restarts
//    llama-server with `--lora`;
//  - `fl_adapter_unload`: restarts llama-server without `--lora` and forgets the remembered
//    adapter, so it is not put back after a restart either.
//
// Policy is core's, not the prompt's: this module never decides that an adapter may load. It shows
// the member the record core holds, asks on a card, and passes the hash to core, which re-checks.
// Running the gate itself needs the member to pick the adapter and scorecard files on
// Rounds › Adapters; Hermes cannot pick files and does not run the gate.
//
// Re-apply after a restart is on whenever an adapter is loaded, and core has no separate off
// switch: unload is the only way to stop it. There is no fl_adapter_reapply_set tool until core
// has that command.
// =====================================================================
import type { ApprovalCard } from "./approvalCards";
import { fieldsCard } from "./approvalCards";
import type { ToolAnnotation } from "./toolAnnotations";
import { fenceUntrusted } from "./untrusted";
import type { CerSpec } from "../shell/state";
import type { FlAdapterGateRecord, FlOverview, FlRoundsDomain } from "../bridge/domains";

export const FL_ADAPTER_TOOL_NAMES = ["fl_adapter_gate_read", "fl_adapter_load_propose", "fl_adapter_unload"] as const;
export type FlAdapterToolName = (typeof FL_ADAPTER_TOOL_NAMES)[number];

export function isFlAdapterTool(name: string): name is FlAdapterToolName {
  return (FL_ADAPTER_TOOL_NAMES as readonly string[]).includes(name);
}

export interface FlAdapterDeps {
  fl: Pick<FlRoundsDomain, "overview" | "loadAdapter" | "unloadAdapter">;
  /** Ask the member on the SignatureCeremony queue. Resolves "approved" only on an explicit Approve. */
  ask: (spec: CerSpec, card: ApprovalCard) => Promise<string>;
}

export interface FlAdapterOutcome {
  status: string;
  result: string;
}

/** The six checks core runs before the gate (fl_rounds.rs, board 09). The first is always run;
 *  the other five only when the member gave the round result file. */
export const GATE_CHECKS: readonly { t: string; round: boolean }[] = [
  { t: "File SHA-256 matches", round: false },
  { t: "The round result says Accepted", round: true },
  { t: "At least three devices took part", round: true },
  { t: "Chain, bundle and replay digests agree", round: true },
  { t: "Replay found no mismatch and checked the merged adapter", round: true },
  { t: "The round result names one round, with well-formed ids", round: true },
];

const SHA = /^[0-9a-f]{64}$/;
const ROUND = /^0x[0-9a-f]{64}$/;

function str(v: unknown): string {
  return typeof v === "string" ? v.trim().toLowerCase() : "";
}

const msg = (e: unknown) => (e instanceof Error ? e.message : String(e));

/** The newest gate record for this adapter hash (core keeps every decision; the newest binds). */
export function latestGateFor(gates: readonly FlAdapterGateRecord[], sha: string): FlAdapterGateRecord | null {
  let best: FlAdapterGateRecord | null = null;
  for (const g of gates) {
    if (g.adapterSha256.toLowerCase() !== sha) continue;
    if (!best || g.decidedAtMs > best.decidedAtMs) best = g;
  }
  return best;
}

/** The checks a recorded decision passed. A record exists only when every check core ran passed. */
export function checkLines(rec: FlAdapterGateRecord): string[] {
  return GATE_CHECKS.map((c) => `- ${c.t}: ${c.round && !rec.round ? "not applicable, no round result was given" : "passed"}`);
}

const f3 = (v: number | null) => (v === null ? "n/a" : v.toFixed(3));

/** One record, as Hermes reads it. Core's verdict, numbers and checked ids are outside the fence.
 *  The base model name is the scorecard file's own `model` field, and the reasons can quote the
 *  scorecards' model and dataset names, so both are fenced: a scorecard may come from someone else. */
export function formatGateRecord(rec: FlAdapterGateRecord): string {
  const d = rec.decision;
  const lines = [
    `Adapter ${rec.adapterSha256}`,
    `Verdict: ${d.verdict} (mean of shared metrics ${f3(d.compositeBase)} to ${f3(d.compositeCandidate)}), decided ${new Date(rec.decidedAtMs).toISOString()}`,
    rec.round
      ? `Round ${rec.round.roundId}, record ${rec.round.recordDigest}, ${rec.round.participants} devices, chain ${rec.round.chainId}. Core checked the round result file the member gave; it did not read the round ledger.`
      : "No round result was given, so this adapter is not tied to a round.",
    "Checks before the gate:",
    ...checkLines(rec),
    "Metrics (base to candidate):",
    ...d.metrics.map((m) => `- ${m.metric}: ${f3(m.base)} to ${f3(m.candidate)}${m.improvement === null ? "" : ` (${m.improvement >= 0 ? "+" : ""}${m.improvement.toFixed(3)})`}`),
    fenceUntrusted("from the scorecard files: the base model they name, and the gate's reasons", { measuredOn: rec.baseModel, reasons: d.reasons }),
  ];
  return lines.join("\n");
}

/** What is loaded now and whether it is put back after a restart. */
export function formatServing(ov: FlOverview): string {
  const lines: string[] = [];
  lines.push(ov.activeAdapter ? `The local model is set to serve the adapter at ${ov.activeAdapter}.` : "The local model is set to serve no adapter; the base model serves alone.");
  if (ov.rememberedAdapter) {
    lines.push(`Remembered adapter: ${ov.rememberedAdapter.sha256} on base model ${ov.rememberedAdapter.baseModel}. Core puts it back after a restart while the gate still accepts it on that base; unloading it stops that.`);
  }
  if (ov.restoreError) lines.push(`The last put-back after a restart did not happen: ${ov.restoreError}`);
  if (ov.storeError) lines.push(`Core could not read its rounds file, so this may be incomplete: ${ov.storeError}`);
  return lines.join("\n");
}

const NO_RECORD =
  "has no eval gate record on this device. The gate needs the adapter file and the base and candidate scorecards, which the member picks on Rounds › Adapters; Hermes cannot pick files or run the gate.";

async function gateRead(args: Record<string, unknown>, deps: FlAdapterDeps): Promise<FlAdapterOutcome> {
  const sha = str(args.sha256);
  const round = str(args.roundId);
  if (sha && !SHA.test(sha)) return { status: "ok", result: "fl_adapter_gate_read needs the adapter's SHA-256 as 64 hex characters. Nothing was read." };
  if (round && !ROUND.test(round)) return { status: "ok", result: "fl_adapter_gate_read needs the round id as 0x followed by 64 hex characters. Nothing was read." };
  let ov: FlOverview;
  try {
    ov = await deps.fl.overview();
  } catch (e) {
    return { status: "ok", result: "couldn't read the eval gate records: " + msg(e) };
  }
  const serving = formatServing(ov);
  if (sha) {
    const rec = latestGateFor(ov.gates, sha);
    return { status: "ok", result: (rec ? formatGateRecord(rec) : `Adapter ${sha} ${NO_RECORD}`) + "\n\n" + serving };
  }
  if (round) {
    const recs = ov.gates.filter((g) => g.round?.roundId.toLowerCase() === round);
    const shas = [...new Set(recs.map((g) => g.adapterSha256.toLowerCase()))];
    const body = shas.length
      ? shas.map((s) => formatGateRecord(latestGateFor(recs, s)!)).join("\n\n")
      : `No adapter from round ${round} ${NO_RECORD}`;
    return { status: "ok", result: body + "\n\n" + serving };
  }
  if (ov.gates.length === 0) return { status: "ok", result: `No adapter ${NO_RECORD}\n\n${serving}` };
  const shas = [...new Set(ov.gates.map((g) => g.adapterSha256.toLowerCase()))];
  const rows = shas.map((s) => {
    const r = latestGateFor(ov.gates, s)!;
    return `- ${s}: ${r.decision.verdict}${r.round ? `, round ${r.round.roundId}` : ", no round result"}`;
  });
  return {
    status: "ok",
    result: [`Eval gate records on this device (newest decision per adapter):`, ...rows, "", "Ask with sha256 for one adapter's checks and metrics.", "", serving].join("\n"),
  };
}

async function loadPropose(args: Record<string, unknown>, ann: ToolAnnotation | null, deps: FlAdapterDeps): Promise<FlAdapterOutcome> {
  const sha = str(args.sha256);
  if (!SHA.test(sha)) return { status: "ok", result: "fl_adapter_load_propose needs the adapter's SHA-256 as 64 hex characters. Nothing was proposed." };
  let ov: FlOverview;
  try {
    ov = await deps.fl.overview();
  } catch (e) {
    return { status: "ok", result: "couldn't read the eval gate records, so nothing was proposed: " + msg(e) };
  }
  const rec = latestGateFor(ov.gates, sha);
  if (!rec) return { status: "ok", result: `Adapter ${sha} ${NO_RECORD} Nothing was proposed.` };
  if (rec.decision.verdict !== "ACCEPT") {
    return { status: "ok", result: `The eval gate's latest decision for adapter ${sha} is REJECT, and core loads only an accepted adapter. Nothing was proposed.\n${formatGateRecord(rec)}` };
  }
  // The base model name comes from the scorecard file, not from core: the card labels it so and
  // clips it. Core's authorize_load refuses unless it matches the model actually served.
  const fields: Record<string, unknown> = {
    adapter: sha,
    "measured on (as the scorecard file names it)": rec.baseModel.slice(0, 120),
    "eval gate": `ACCEPT, mean of shared metrics ${f3(rec.decision.compositeBase)} to ${f3(rec.decision.compositeCandidate)}`,
    round: rec.round ? `${rec.round.roundId} (${rec.round.participants} devices, from the round result file)` : "none given",
    effect: "restarts the local model with this adapter; it is put back after a restart until you unload it",
  };
  const spec: CerSpec = {
    origin: "chat agent",
    requester: "chat agent · LoRA adapter",
    title: "Load an adapter into the local model",
    chainless: true,
    rows: Object.entries(fields).map(([k, v]) => ({ k, v: String(v) })),
    cost: "none, no chain transaction",
    sponsor: "your explicit decision (HIC-1)",
    sponsorColor: "var(--tx-3)",
  };
  const r = await deps.ask(spec, fieldsCard("fl_adapter_load_propose", ann, fields, "load adapter " + sha.slice(0, 12) + " into the local model"));
  if (r !== "approved") {
    return { status: r, result: r === "expired" ? "The approval expired because the run ended; no adapter was loaded." : "The member declined loading the adapter; nothing changed." };
  }
  try {
    await deps.fl.loadAdapter(sha);
    return { status: "approved", result: `Loaded adapter ${sha} into the local model, after core checked its gate record against the base model being served and re-hashed the file.` };
  } catch (e) {
    return { status: "error", result: "Core refused loading the adapter: " + msg(e) };
  }
}

async function unload(deps: FlAdapterDeps): Promise<FlAdapterOutcome> {
  try {
    const unloaded = await deps.fl.unloadAdapter();
    if (!unloaded) return { status: "ok", result: "No adapter was loaded or remembered, so nothing changed; the base model serves alone." };
    return { status: "ok", result: "Unloaded the adapter. The base model serves alone, and no adapter is put back after a restart." };
  } catch (e) {
    return { status: "error", result: "couldn't unload the adapter: " + msg(e) };
  }
}

export async function runFlAdapterTool(name: FlAdapterToolName, args: Record<string, unknown>, ann: ToolAnnotation | null, deps: FlAdapterDeps): Promise<FlAdapterOutcome> {
  switch (name) {
    case "fl_adapter_gate_read":
      return gateRead(args, deps);
    case "fl_adapter_load_propose":
      return loadPropose(args, ann, deps);
    case "fl_adapter_unload":
      return unload(deps);
  }
}
