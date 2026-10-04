// =====================================================================
// citrate-core — the dApp forge's member path (HUP-S6.2 / S6.3 / S6.9, US-6.4)
//
// Types mirroring the Rust commands (template_forge.rs, forge_toolchain.rs,
// deploy_gate_toolchain.rs) and the pure helpers the forge panel uses. Core decides everything:
// the renderer validates every parameter, the deploy gate parses every tool report itself, and
// the tier budget comes from core. Nothing here signs, deploys or judges.
// =====================================================================
import type { DeployGateInputs, DeployGateRecord } from "./deployGate";

/** One field of a template's parameter form (OpenZeppelin Wizard style). */
export interface TemplateField {
  key: "name" | "symbol" | "supply" | "price" | "owner" | string;
  label: string;
  help: string;
  default: string | null;
  min: number | null;
  max: number | null;
  required: boolean;
}

export interface TemplateEntry {
  id: string;
  kind: "contract" | "dapp";
  title: string;
  description: string;
  fields: TemplateField[];
  medusa: boolean;
}

/** Mirrors Rust `citrate_templates::MedusaBudget` (snake_case on the wire). */
export interface MedusaBudget {
  test_limit: number;
  workers: number;
  call_sequence_length: number;
  timeout_secs: number;
  coverage_plateau_calls: number;
  min_coverage_pct: number;
}

/** `template_list`. */
export interface TemplateCatalog {
  templates: TemplateEntry[];
  tier: "T0" | "T1" | "T2";
  medusaBudget: MedusaBudget;
}

export interface TemplateRenderInput {
  template: string;
  params: Record<string, string>;
  outDir: string;
}

export interface DepStatus {
  name: string;
  commit: string;
  dir: string;
  installed: boolean;
  note: string | null;
}

/** `template_render`. */
export interface TemplateRenderView {
  template: string;
  tier: string;
  outDir: string;
  digest: string;
  files: string[];
  params: Record<string, string>;
  medusaBudget: MedusaBudget | null;
  contractDir: string;
  deps: DepStatus[];
}

/** `toolchain_settings_get` / `toolchain_settings_set`. */
export interface ToolchainStatus {
  settings: { enabled: boolean };
  programs: { program: string; tool: string; path: string | null }[];
  solc: string | null;
  searchPath: string[];
  notices: string[];
  appliesOnRestart: boolean;
  loadError: string | null;
}

/** What `deploy_gate_submit_toolchain` takes. */
export interface ToolchainGateRequest {
  sessionId: string;
  project: string;
  /** `File.sol/Contract.json` under the project's `out/`. */
  artifact: string;
  constructorArgsHex?: string;
  forkDryRun?: DeployGateInputs["forkDryRun"];
}

export interface MedusaBudgetCheck {
  tier: string;
  requiredCalls: number;
  minCoveragePct: number;
  runTestLimit: number | null;
  linesHit: number | null;
  linesTotal: number | null;
  coveragePct: number | null;
  problems: string[];
}

/** `deploy_gate_submit_toolchain`. */
export interface ToolchainGateResult {
  record: DeployGateRecord;
  artifact: string;
  sourcesSha256: string | null;
  medusa: MedusaBudgetCheck;
}

/** The starting values of a template's form: each field's default, or empty. */
export function initialValues(t: TemplateEntry): Record<string, string> {
  return Object.fromEntries(t.fields.map((f) => [f.key, f.default ?? ""]));
}

/** The parameters to send: every field with a value (trimmed); empty optional fields are left
 *  out so the renderer applies its default. Required fields that are empty are named. */
export function renderParams(t: TemplateEntry, values: Record<string, string>): { params: Record<string, string>; missing: string[] } {
  const params: Record<string, string> = {};
  const missing: string[] = [];
  for (const f of t.fields) {
    const v = (values[f.key] ?? "").trim();
    if (v === "") {
      if (f.required) missing.push(f.label);
      continue;
    }
    params[f.key] = v;
  }
  return { params, missing };
}

/** One line on the Medusa tier check, in plain words. */
export function medusaLine(m: MedusaBudgetCheck): string {
  const cov =
    m.coveragePct == null
      ? "no coverage measured"
      : `${m.coveragePct}% of src/ lines covered (${m.linesHit} of ${m.linesTotal})`;
  const head = `${m.tier}: at least ${m.requiredCalls.toLocaleString("en-US")} calls and ${m.minCoveragePct}% line coverage; ${cov}`;
  return m.problems.length ? `${head}. Not accepted: ${m.problems.join("; ")}` : `${head}.`;
}

/** The out/ artifact forge writes for a contract in `src/<file>`: `<file>/<Contract>.json`. */
export function artifactFor(sourceFile: string, contract: string): string {
  const base = sourceFile.split("/").pop() ?? sourceFile;
  return `${base}/${contract}.json`;
}
