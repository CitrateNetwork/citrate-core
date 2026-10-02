// =====================================================================
// citrate-core — argument parsing for scripts/eval-qa.mjs (HUP-S3.5)
//
// Pure (no I/O) so the loopback guard is unit-tested. A QA run sends 150 questions and the
// Hermes QA instruction to the endpoint, so a non-loopback base URL is refused unless the
// operator passes --allow-remote. Same flag shape and guard as the tool-call eval CLI
// (scripts/eval-tools.mjs, HUP-S1.7 on its own branch); when both land the two parsers should
// fold into one module.
// =====================================================================

export interface QaCliArgs {
  baseUrl: string;
  model: string;
  /** Name of the env var holding the API key (the key itself never appears on argv). */
  apiKeyEnv?: string;
  allowRemote: boolean;
  tier?: "T0" | "T1" | "T2";
  outDir: string;
  /** HUP-S9.4: the LoRA adapter the endpoint serves for this run (a candidate run for the eval gate). */
  adapterSha256?: string;
  /** Minimum key-point coverage for an answerable item to pass (scorer default 0.6). */
  coverageThreshold?: number;
  /** HUP-S7.7: which QA set to run, by version name (default qa-v1). */
  dataset?: string;
  /**
   * HUP-S3.1 / g2-knowledge: answer from the bundled knowledge corpus, retrieved per question from a
   * memory daemon socket (src/agent/eval/retrieval.ts). Absent = closed-book.
   */
  retrieval?: { socket: string; tenants: string[]; k: number; corpusDigest?: string; corpusDir?: string };
}

/** Knowledge tenants a retrieval run may search (mem_corpus::KNOWLEDGE_TENANTS). */
export const QA_RETRIEVAL_TENANTS = ["citrate-docs", "methodology", "refs", "skills"];

/** A QA set version name: `qa-vN` or a named pack `qa-<pack>-vN` (same rule as the dataset validator). */
export const QA_SET_NAME_RE = /^qa(-[a-z0-9]+)*-v\d+$/;

/** HUP-S7.7: the dataset and anchor-index paths (repo-relative) of a QA set. Default qa-v1. */
export function qaDatasetFiles(name = "qa-v1"): { dataset: string; index: string } {
  if (!QA_SET_NAME_RE.test(name)) throw new Error(`unknown dataset name ${JSON.stringify(name)} (expected qa-vN or qa-<pack>-vN)`);
  return { dataset: `src/agent/eval/${name}.json`, index: `src/agent/eval/${name}.anchors.json` };
}

export const QA_CLI_USAGE =
  "usage: node scripts/eval-qa.mjs --base-url <http://127.0.0.1:18080/v1> --model <name> " +
  "[--api-key-env VAR] [--tier T0|T1|T2] [--out-dir eval/results] [--coverage-threshold 0..1] [--dataset qa-v1] [--adapter-sha256 <hex>] [--allow-remote] " +
  "[--memory-socket <path> [--retrieve-tenants citrate-docs,methodology] [--retrieve-k 5] [--corpus-dir <dir>] [--corpus-digest <hex>]]";

function parseHttpUrl(raw: string): URL | null {
  let u: URL;
  try {
    u = new URL(raw);
  } catch {
    return null;
  }
  return u.protocol === "http:" || u.protocol === "https:" ? u : null;
}

/** True iff `raw` is an http(s) URL whose host is loopback: 127.0.0.0/8, `localhost`, or ::1. */
export function isLoopbackUrl(raw: string): boolean {
  const u = parseHttpUrl(raw);
  if (!u) return false;
  const host = u.hostname.toLowerCase();
  if (host === "localhost" || host === "[::1]" || host === "::1") return true;
  const m = /^(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})$/.exec(host);
  if (!m) return false;
  const octets = m.slice(1).map(Number);
  return octets.every((o) => o <= 255) && octets[0] === 127;
}

const VALUE_FLAGS = new Set([
  "--base-url",
  "--model",
  "--api-key-env",
  "--tier",
  "--out-dir",
  "--adapter-sha256",
  "--coverage-threshold",
  "--dataset",
  "--memory-socket",
  "--retrieve-tenants",
  "--retrieve-k",
  "--corpus-digest",
  "--corpus-dir",
]);

/** Parse argv (without the node + script entries). Throws with a readable message on error. */
export function parseQaCliArgs(argv: string[]): QaCliArgs {
  const vals: Record<string, string> = {};
  let allowRemote = false;
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === "--allow-remote") {
      allowRemote = true;
    } else if (VALUE_FLAGS.has(a)) {
      const v = argv[i + 1];
      if (v === undefined || v.startsWith("--")) throw new Error(`${a} needs a value\n${QA_CLI_USAGE}`);
      vals[a] = v;
      i++;
    } else {
      throw new Error(`unknown argument ${JSON.stringify(a)}\n${QA_CLI_USAGE}`);
    }
  }
  const baseUrl = vals["--base-url"];
  const model = vals["--model"];
  if (!baseUrl) throw new Error(`--base-url is required\n${QA_CLI_USAGE}`);
  if (!model) throw new Error(`--model is required\n${QA_CLI_USAGE}`);
  if (!parseHttpUrl(baseUrl)) throw new Error(`--base-url must be an http(s) URL: ${baseUrl}`);
  if (!allowRemote && !isLoopbackUrl(baseUrl)) {
    throw new Error(
      `refusing non-loopback --base-url ${baseUrl}: a QA run sends every question and the system prompt to the ` +
        "endpoint. Pass --allow-remote to confirm.",
    );
  }
  const apiKeyEnv = vals["--api-key-env"];
  if (apiKeyEnv !== undefined && !/^[A-Z_][A-Z0-9_]*$/.test(apiKeyEnv)) {
    throw new Error("--api-key-env takes an env var NAME (e.g. EVAL_API_KEY), never the key itself");
  }
  const tier = vals["--tier"];
  if (tier !== undefined && tier !== "T0" && tier !== "T1" && tier !== "T2") {
    throw new Error(`--tier must be T0, T1 or T2 (got ${tier})`);
  }
  const thrRaw = vals["--coverage-threshold"];
  let coverageThreshold: number | undefined;
  if (thrRaw !== undefined) {
    coverageThreshold = Number(thrRaw);
    if (!Number.isFinite(coverageThreshold) || coverageThreshold < 0 || coverageThreshold > 1) {
      throw new Error(`--coverage-threshold must be a number in [0, 1] (got ${thrRaw})`);
    }
  }
  const dataset = vals["--dataset"];
  if (dataset !== undefined && !QA_SET_NAME_RE.test(dataset)) {
    throw new Error(`--dataset takes a QA set name such as qa-v1 or qa-literacy-v1 (got ${dataset})`);
  }
  const adapterRaw = vals["--adapter-sha256"];
  if (adapterRaw !== undefined && !/^[0-9a-fA-F]{64}$/.test(adapterRaw)) {
    throw new Error(`--adapter-sha256 takes the adapter file's sha256 as 64 hex characters (got ${adapterRaw})`);
  }
  const out: QaCliArgs = {
    baseUrl: baseUrl.replace(/\/+$/, ""),
    model,
    allowRemote,
    outDir: vals["--out-dir"] ?? "eval/results",
  };
  if (apiKeyEnv !== undefined) out.apiKeyEnv = apiKeyEnv;
  if (tier !== undefined) out.tier = tier;
  if (adapterRaw !== undefined) out.adapterSha256 = adapterRaw.toLowerCase();
  if (coverageThreshold !== undefined) out.coverageThreshold = coverageThreshold;
  if (dataset !== undefined) out.dataset = dataset;
  const socket = vals["--memory-socket"];
  const tenantsRaw = vals["--retrieve-tenants"];
  const kRaw = vals["--retrieve-k"];
  const digest = vals["--corpus-digest"];
  const corpusDir = vals["--corpus-dir"];
  if (socket === undefined) {
    if (tenantsRaw !== undefined || kRaw !== undefined || digest !== undefined || corpusDir !== undefined) {
      throw new Error("--retrieve-tenants, --retrieve-k, --corpus-digest and --corpus-dir need --memory-socket");
    }
    return out;
  }
  const tenants = (tenantsRaw ?? "citrate-docs,methodology").split(",").map((t) => t.trim()).filter(Boolean);
  const bad = tenants.filter((t) => !QA_RETRIEVAL_TENANTS.includes(t));
  if (!tenants.length || bad.length) {
    throw new Error(`--retrieve-tenants takes knowledge tenants (${QA_RETRIEVAL_TENANTS.join(", ")}); got ${bad.join(", ") || "none"}`);
  }
  const k = kRaw === undefined ? 5 : Number(kRaw);
  if (!Number.isInteger(k) || k < 1 || k > 20) throw new Error(`--retrieve-k must be an integer from 1 to 20 (got ${kRaw})`);
  if (digest !== undefined && !/^[0-9a-f]{64}$/.test(digest)) {
    throw new Error(`--corpus-digest takes the corpus manifest bundle_digest (64 lowercase hex; got ${digest})`);
  }
  out.retrieval = { socket, tenants, k };
  if (digest !== undefined) out.retrieval.corpusDigest = digest;
  if (corpusDir !== undefined) out.retrieval.corpusDir = corpusDir;
  return out;
}

/**
 * `<YYYY-MM-DD>-qa-<model>.json` for qa-v1, `<YYYY-MM-DD>-<set>-<model>.json` for any other set,
 * with the model name reduced to a safe single path segment.
 */
export function qaResultFileName(isoDate: string, model: string, dataset = "qa-v1", adapterSha256?: string, retrieval = false): string {
  const safe = model.replace(/[^A-Za-z0-9._-]/g, "_").replace(/\.\./g, "__");
  const set = dataset === "qa-v1" || !QA_SET_NAME_RE.test(dataset) ? "qa" : dataset;
  // HUP-S9.4: a LoRA candidate run never overwrites the base run it is compared against.
  const lora = adapterSha256 ? `-lora-${adapterSha256.slice(0, 12)}` : "";
  // HUP-S3.1: a run that answers from the retrieved corpus never overwrites the closed-book run.
  const rag = retrieval ? "-rag" : "";
  return `${isoDate.slice(0, 10)}-${set}${rag}-${safe}${lora}.json`;
}
