// =====================================================================
// citrate-core — argument parsing for scripts/eval-tools.mjs (HUP-S1.7)
//
// Pure (no I/O) so the loopback guard is unit-tested. An eval run sends the full Hermes system
// prompt, all tool schemas and — for injection cases — a canary secret to the endpoint, so a
// non-loopback base URL is refused unless the operator passes --allow-remote explicitly.
// =====================================================================

export interface EvalCliArgs {
  baseUrl: string;
  model: string;
  /** Name of the env var holding the API key (the key itself never appears on argv). */
  apiKeyEnv?: string;
  allowRemote: boolean;
  tier?: "T0" | "T1" | "T2";
  outDir: string;
}

export const EVAL_CLI_USAGE =
  "usage: node scripts/eval-tools.mjs --base-url <http://127.0.0.1:18080/v1> --model <name> " +
  "[--api-key-env VAR] [--tier T0|T1|T2] [--out-dir eval/results] [--allow-remote]";

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

const VALUE_FLAGS = new Set(["--base-url", "--model", "--api-key-env", "--tier", "--out-dir"]);

/** Parse argv (without the node + script entries). Throws with a readable message on error. */
export function parseEvalCliArgs(argv: string[]): EvalCliArgs {
  const vals: Record<string, string> = {};
  let allowRemote = false;
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === "--allow-remote") {
      allowRemote = true;
    } else if (VALUE_FLAGS.has(a)) {
      const v = argv[i + 1];
      if (v === undefined || v.startsWith("--")) throw new Error(`${a} needs a value\n${EVAL_CLI_USAGE}`);
      vals[a] = v;
      i++;
    } else {
      throw new Error(`unknown argument ${JSON.stringify(a)}\n${EVAL_CLI_USAGE}`);
    }
  }
  const baseUrl = vals["--base-url"];
  const model = vals["--model"];
  if (!baseUrl) throw new Error(`--base-url is required\n${EVAL_CLI_USAGE}`);
  if (!model) throw new Error(`--model is required\n${EVAL_CLI_USAGE}`);
  if (!parseHttpUrl(baseUrl)) throw new Error(`--base-url must be an http(s) URL: ${baseUrl}`);
  if (!allowRemote && !isLoopbackUrl(baseUrl)) {
    throw new Error(
      `refusing non-loopback --base-url ${baseUrl}: an eval run sends the system prompt, tool schemas and a canary ` +
        "secret to the endpoint. Pass --allow-remote to confirm.",
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
  const out: EvalCliArgs = {
    baseUrl: baseUrl.replace(/\/+$/, ""),
    model,
    allowRemote,
    outDir: vals["--out-dir"] ?? "eval/results",
  };
  if (apiKeyEnv !== undefined) out.apiKeyEnv = apiKeyEnv;
  if (tier !== undefined) out.tier = tier;
  return out;
}

/** `<YYYY-MM-DD>-<model>.json`, with the model name reduced to a safe single path segment. */
export function resultFileName(isoDate: string, model: string): string {
  const safe = model.replace(/[^A-Za-z0-9._-]/g, "_").replace(/\.\./g, "__");
  return `${isoDate.slice(0, 10)}-${safe}.json`;
}
