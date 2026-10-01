// =====================================================================
// HUP-S6.4 — the D-4 deploy gate, TypeScript side.
//
// Core (src-tauri/src/deploy_gate.rs) owns the verdict: it parses the raw verifier outputs and
// refuses `contract_deploy` unless a READY record exists for exactly the init code being
// deployed. These are the wire types (camelCase, mirroring the Rust serde shapes) and the pure
// model the verdict card renders. The card model fails closed: it shows READY only when core
// said READY, all five items are present and passing, and the record is for the hash shown.
// =====================================================================

export const GATE_ITEM_IDS = ["forge_tests", "slither", "aderyn", "medusa", "fork_dry_run"] as const;
export type GateItemId = (typeof GATE_ITEM_IDS)[number];

const LABELS: Record<GateItemId, string> = {
  forge_tests: "Forge tests",
  slither: "Slither",
  aderyn: "Aderyn",
  medusa: "Medusa campaign",
  fork_dry_run: "Fork dry run",
};

export interface CompilerSettings {
  solcVersion: string;
  optimizer: boolean;
  optimizerRuns: number;
  evmVersion: string;
  viaIr?: boolean;
}

export interface GateEvidence {
  counts: Record<string, number>;
  /** SHA-256 (hex) of the raw tool output; null when the tool did not run. */
  outputSha256: string | null;
  durationMs: number | null;
  toolVersion: string | null;
}

export interface DeployGateItem {
  id: GateItemId;
  label: string;
  pass: boolean;
  reason: string;
  evidence: GateEvidence;
}

export interface DeployGateRecord {
  /** `0x` + keccak256(creation bytecode ‖ constructor args). */
  initcodeHash: string;
  /** `0x` + keccak256(domain ‖ initcode hash ‖ keccak256(compiler settings)). */
  bindingHash: string;
  compiler: CompilerSettings;
  verdict: "READY" | "NOT_READY";
  items: DeployGateItem[];
  evaluatedAtMs: number;
}

/** What `deploy_gate_lookup` returns. */
export interface DeployGateLookup {
  initcodeHash: string;
  record: DeployGateRecord | null;
}

/** One verifier's report as the runtime hands it over. "notInstalled" is always a FAIL. */
export type ToolRun =
  | { state: "ran"; output: string; durationMs: number; toolVersion?: string }
  | { state: "notInstalled" }
  | { state: "error"; message: string };

/** The typed hand-over from the runtime verifiers (HUP-S6.3) to `deploy_gate_submit`. */
export interface DeployGateInputs {
  bytecodeHex: string;
  constructorArgsHex?: string;
  compiler: CompilerSettings;
  forgeTests: ToolRun;
  slither: ToolRun;
  aderyn: ToolRun;
  medusa: { run: ToolRun; callBudget: number };
  forkDryRun: { run: ToolRun; txInputHex: string; citratePrecompiles: "none" | "used" | "unknown" };
}

export interface GateCardItem {
  id: GateItemId;
  label: string;
  pass: boolean;
  reason: string;
  /** One line of evidence: counts, run time, short output digest. */
  evidence: string;
}

export interface GateCardModel {
  verdict: "READY" | "NOT READY";
  ready: boolean;
  initcodeHash: string;
  bindingHash: string | null;
  compiler: string | null;
  /** Why it is NOT READY, one line per failing item (empty when READY). */
  reasons: string[];
  items: GateCardItem[];
}

function seconds(ms: number): string {
  const s = ms / 1000;
  return (Number.isInteger(s) ? String(s) : s.toFixed(1)) + " s";
}

function evidenceLine(e: GateEvidence | undefined): string {
  if (!e) return "no output";
  const parts = Object.entries(e.counts ?? {}).map(([k, v]) => `${k.replace(/_/g, " ")} ${v}`);
  if (e.durationMs != null) parts.push(seconds(e.durationMs));
  if (e.outputSha256) parts.push("sha256 " + e.outputSha256.slice(0, 12) + "…");
  return parts.length ? parts.join(" · ") : "no output";
}

function compilerLine(c: CompilerSettings | undefined): string | null {
  if (!c) return null;
  const opt = c.optimizer ? `optimizer on, ${c.optimizerRuns} runs` : "optimizer off";
  return `solc ${c.solcVersion} · ${opt} · evm ${c.evmVersion}${c.viaIr ? " · via-IR" : ""}`;
}

/**
 * The verdict card model for `initcodeHash`. `record` is core's record for that hash, or null
 * when no gate has run. READY only when every check below holds; otherwise NOT READY with
 * one reason per problem.
 */
export function gateCardModel(record: DeployGateRecord | null | undefined, initcodeHash: string): GateCardModel {
  if (!record) {
    return {
      verdict: "NOT READY",
      ready: false,
      initcodeHash,
      bindingHash: null,
      compiler: null,
      reasons: ["No deploy gate has run for this exact bytecode. Any change to the bytecode or constructor arguments needs a new gate run."],
      items: [],
    };
  }
  const reasons: string[] = [];
  if (record.initcodeHash.toLowerCase() !== initcodeHash.toLowerCase()) {
    reasons.push(`The gate record is for different bytecode (${record.initcodeHash}).`);
  }
  const items: GateCardItem[] = GATE_ITEM_IDS.map((id) => {
    const it = record.items.find((x) => x.id === id);
    if (!it) return { id, label: LABELS[id], pass: false, reason: "no result", evidence: "no output" };
    return { id, label: it.label || LABELS[id], pass: it.pass, reason: it.reason, evidence: evidenceLine(it.evidence) };
  });
  for (const it of items) if (!it.pass) reasons.push(`${it.label}: ${it.reason}`);
  if (record.verdict !== "READY" && reasons.length === 0) reasons.push("Core reported NOT READY.");
  const ready = record.verdict === "READY" && reasons.length === 0;
  return {
    verdict: ready ? "READY" : "NOT READY",
    ready,
    initcodeHash,
    bindingHash: record.bindingHash ?? null,
    compiler: compilerLine(record.compiler),
    reasons,
    items,
  };
}
