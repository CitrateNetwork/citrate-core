// =====================================================================
// HUP-S7.3 (US-7.2 AC3): past decisions and their proofs, as the Journal shows them.
//
// Data sources: the Hermes sidecar's `/anchor/records` (via `hermes_anchor_records`) for the list,
// and core's own proof check (`hermes_anchor_proof`), which recomputes the proof and reads
// AnchorRegistry on 40204 itself. The verdict line comes from core; this file only formats.
// =====================================================================

export interface RecordRow {
  seq: number;
  tsMs: number;
  day: number;
  date: string;
  hash: string;
  batched: boolean;
  anchored: boolean;
  anchorTx: string | null;
  anchorBlock: number | null;
  record: unknown;
}

export interface RecordsPage {
  configured: boolean;
  recordsPresent: boolean;
  limit: number;
  records: RecordRow[];
  nextBefore: number | null;
}

export interface ProofVerdict {
  seq: number;
  day: number;
  date: string | null;
  commitment: string | null;
  inclusionOk: boolean;
  inclusionError: string | null;
  recordBound: boolean | null;
  chain: { state: string; [k: string]: unknown };
  proven: boolean;
  line: string;
  record: unknown;
}

export interface ShareCall {
  metric: string;
  value: string;
}

export interface ShareView {
  day: string;
  registry: string;
  agentId: string;
  calls: ShareCall[];
  cards: { id: string }[];
  pendingOwnerSignOff: string[];
}

function obj(v: unknown): Record<string, unknown> | null {
  return v !== null && typeof v === "object" && !Array.isArray(v) ? (v as Record<string, unknown>) : null;
}

function str(v: unknown): string {
  return typeof v === "string" ? v : "";
}

const DECISION_WORD: Record<string, string> = {
  approved: "Approved",
  denied: "Denied",
  auto_within_budget: "Allowed inside a budget",
};

/** One line for a decision record (decision or outcome); anything else is "Unrecognized record". */
export function describeRecord(record: unknown): string {
  const entry = obj(obj(record)?.entry);
  const d = obj(entry?.decision);
  if (d) {
    const word = DECISION_WORD[str(d.decision)] ?? str(d.decision);
    const tier = str(d.tier).toUpperCase();
    return `${word}: ${str(d.subject)} (${tier}, ${str(d.kind)})`;
  }
  const o = obj(entry?.outcome);
  if (o) {
    return `Outcome of #${String(o.decision_seq)}: ${str(o.outcome).replace(/_/g, " ")}`;
  }
  return "Unrecognized record";
}

/** The anchor state of one row, in words. */
export function anchorState(r: RecordRow): string {
  if (r.anchored) return r.anchorBlock !== null ? `anchored in block ${r.anchorBlock}` : "anchored";
  if (r.batched) return "batched, waiting for the anchor";
  return "not anchored yet";
}
