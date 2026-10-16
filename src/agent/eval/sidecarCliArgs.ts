// =====================================================================
// citrate-core — argument parsing for scripts/eval-sidecar.mjs (HUP-S1.7 / HUP-S1.10)
//
// Pure (no I/O), so the guards are unit-tested. Like eval-tools.mjs, the model endpoint must be
// loopback (a run sends the system prompt, tool schemas and canary secrets to it) unless the
// operator passes --allow-remote (the CI workflow, eval.yml), and then it must be https: the
// sidecar itself refuses plain http to a non-loopback host. Executables must be absolute paths.
// =====================================================================
import { isLoopbackUrl } from "./cliArgs.ts";

export interface SidecarEvalArgs {
  baseUrl: string;
  /** --allow-remote: a non-loopback https endpoint was accepted on purpose. */
  allowRemote: boolean;
  model: string;
  tier?: "T0" | "T1" | "T2";
  apiKeyEnv?: string;
  sidecarBin: string;
  mcpFixtureBin: string;
  /** The managed browser executable; required unless only workflows run. */
  chromium?: string;
  /** The running llama-server's --ctx-size (what core passes as contextTokens). */
  contextTokens: number;
  /** Per-turn reply cap; default like core: min(2048, contextTokens / 4). */
  maxTokens: number;
  only?: "workflows" | "injection";
  outDir: string;
  /** Longest one case may take before the run aborts. */
  deadlineSeconds: number;
  /** The citrate-agent-runtime commit the sidecar binary was built from (stamped into the scorecard). */
  runtimeRev?: string;
}

export const SIDECAR_EVAL_USAGE =
  "usage: node scripts/eval-sidecar.mjs --base-url <http://127.0.0.1:PORT/v1> --model <name> " +
  "--sidecar-bin </abs/citrate-agent-sidecar> --mcp-fixture-bin </abs/citrate-mcp-fixture-server> " +
  "--context-tokens <n> [--chromium </abs/chrome>] [--tier T0|T1|T2] [--api-key-env VAR] [--allow-remote] [--runtime-rev <40-hex>] " +
  "[--max-tokens <n>] [--only workflows|injection] [--out-dir eval/results] [--deadline-s 900]";

const VALUE_FLAGS = new Set([
  "--base-url",
  "--model",
  "--tier",
  "--api-key-env",
  "--sidecar-bin",
  "--mcp-fixture-bin",
  "--chromium",
  "--context-tokens",
  "--max-tokens",
  "--only",
  "--out-dir",
  "--deadline-s",
  "--runtime-rev",
]);

/** core's per-turn reply cap (src-tauri/src/ai.rs AI_MAX_TOKENS). */
export const CORE_AI_MAX_TOKENS = 2048;

function posInt(flag: string, raw: string | undefined, min: number, max: number): number | undefined {
  if (raw === undefined) return undefined;
  const n = Number(raw);
  if (!Number.isInteger(n) || n < min || n > max) throw new Error(`${flag} must be an integer from ${min} to ${max} (got ${raw})`);
  return n;
}

function absPath(flag: string, raw: string | undefined): string | undefined {
  if (raw === undefined) return undefined;
  if (!raw.startsWith("/") && !/^[A-Za-z]:[\\/]/.test(raw)) throw new Error(`${flag} must be an absolute path (got ${raw})`);
  return raw;
}

export function parseSidecarEvalArgs(argv: string[]): SidecarEvalArgs {
  const vals: Record<string, string> = {};
  let allowRemote = false;
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === "--allow-remote") {
      allowRemote = true;
      continue;
    }
    if (!VALUE_FLAGS.has(a)) throw new Error(`unknown argument ${JSON.stringify(a)}\n${SIDECAR_EVAL_USAGE}`);
    const v = argv[i + 1];
    if (v === undefined || v.startsWith("--")) throw new Error(`${a} needs a value\n${SIDECAR_EVAL_USAGE}`);
    vals[a] = v;
    i++;
  }
  for (const req of ["--base-url", "--model", "--sidecar-bin", "--mcp-fixture-bin", "--context-tokens"]) {
    if (!vals[req]) throw new Error(`${req} is required\n${SIDECAR_EVAL_USAGE}`);
  }
  const baseUrl = vals["--base-url"].replace(/\/+$/, "");
  if (!isLoopbackUrl(baseUrl)) {
    if (!allowRemote) throw new Error(`--base-url must be a loopback http(s) URL, or pass --allow-remote (got ${baseUrl})`);
    if (!/^https:\/\/[^/\s]+/.test(baseUrl)) {
      throw new Error(`a non-loopback --base-url must be https (the sidecar refuses plain http off loopback; got ${baseUrl})`);
    }
  }
  const tier = vals["--tier"];
  if (tier !== undefined && tier !== "T0" && tier !== "T1" && tier !== "T2") throw new Error(`--tier must be T0, T1 or T2 (got ${tier})`);
  const apiKeyEnv = vals["--api-key-env"];
  if (apiKeyEnv !== undefined && !/^[A-Z_][A-Z0-9_]*$/.test(apiKeyEnv)) {
    throw new Error("--api-key-env takes an env var NAME, never the key itself");
  }
  const only = vals["--only"];
  if (only !== undefined && only !== "workflows" && only !== "injection") throw new Error(`--only must be workflows or injection (got ${only})`);
  const contextTokens = posInt("--context-tokens", vals["--context-tokens"], 2048, 1_048_576) as number;
  const chromium = absPath("--chromium", vals["--chromium"]);
  if (only !== "workflows" && !chromium) {
    throw new Error("--chromium is required for the browser injection cases (or pass --only workflows)");
  }
  const out: SidecarEvalArgs = {
    baseUrl,
    allowRemote,
    model: vals["--model"],
    sidecarBin: absPath("--sidecar-bin", vals["--sidecar-bin"]) as string,
    mcpFixtureBin: absPath("--mcp-fixture-bin", vals["--mcp-fixture-bin"]) as string,
    contextTokens,
    maxTokens: posInt("--max-tokens", vals["--max-tokens"], 64, 65_536) ?? Math.min(CORE_AI_MAX_TOKENS, Math.floor(contextTokens / 4)),
    outDir: vals["--out-dir"] ?? "eval/results",
    deadlineSeconds: posInt("--deadline-s", vals["--deadline-s"], 30, 7200) ?? 900,
  };
  const runtimeRev = vals["--runtime-rev"];
  if (runtimeRev !== undefined) {
    if (!/^[0-9a-f]{40}$/.test(runtimeRev)) throw new Error(`--runtime-rev takes a full 40-hex commit (got ${runtimeRev})`);
    out.runtimeRev = runtimeRev;
  }
  if (tier !== undefined) out.tier = tier;
  if (apiKeyEnv !== undefined) out.apiKeyEnv = apiKeyEnv;
  if (chromium !== undefined) out.chromium = chromium;
  if (only !== undefined) out.only = only;
  return out;
}
