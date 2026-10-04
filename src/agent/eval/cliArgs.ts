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
  /** HUP-S9.4: the LoRA adapter the endpoint serves for this run (a candidate run for the eval gate). */
  adapterSha256?: string;
  /**
   * HUP-S9.4: the per-request scale for LoRA adapter id 0. With a server started as
   * `llama-server --lora-scaled <adapter>:0`, scale 0 is the base arm and scale 1 the candidate
   * arm, so both runs use one server. A candidate arm (scale > 0) needs --adapter-sha256.
   */
  loraScale?: number;
}

export const EVAL_CLI_USAGE =
  "usage: node scripts/eval-tools.mjs --base-url <http://127.0.0.1:18080/v1> --model <name> " +
  "[--api-key-env VAR] [--tier T0|T1|T2] [--out-dir eval/results] [--adapter-sha256 <hex>] [--lora-scale 0..1] [--allow-remote]";

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

const VALUE_FLAGS = new Set(["--base-url", "--model", "--api-key-env", "--tier", "--out-dir", "--adapter-sha256", "--lora-scale"]);

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
  const adapterRaw = vals["--adapter-sha256"];
  if (adapterRaw !== undefined && !/^[0-9a-fA-F]{64}$/.test(adapterRaw)) {
    throw new Error(`--adapter-sha256 takes the adapter file's sha256 as 64 hex characters (got ${adapterRaw})`);
  }
  const scaleRaw = vals["--lora-scale"];
  let loraScale: number | undefined;
  if (scaleRaw !== undefined) {
    const n = /^\d+(\.\d+)?$/.test(scaleRaw) ? Number(scaleRaw) : NaN;
    if (!Number.isFinite(n) || n < 0 || n > 1) {
      throw new Error(`--lora-scale takes a number from 0 to 1 (got ${scaleRaw})`);
    }
    if (n > 0 && adapterRaw === undefined) {
      throw new Error("--lora-scale above 0 is a candidate run: pass --adapter-sha256 with the adapter's sha256");
    }
    if (n === 0 && adapterRaw !== undefined) {
      throw new Error("--lora-scale 0 is a base run: do not pass --adapter-sha256 with it");
    }
    loraScale = n;
  }
  const out: EvalCliArgs = {
    baseUrl: baseUrl.replace(/\/+$/, ""),
    model,
    allowRemote,
    outDir: vals["--out-dir"] ?? "eval/results",
  };
  if (apiKeyEnv !== undefined) out.apiKeyEnv = apiKeyEnv;
  if (tier !== undefined) out.tier = tier;
  if (adapterRaw !== undefined) out.adapterSha256 = adapterRaw.toLowerCase();
  if (loraScale !== undefined) out.loraScale = loraScale;
  return out;
}

/** The llama-server request field that sets adapter id 0's scale for one request. */
export function loraRequestField(scale: number | undefined): { lora?: { id: number; scale: number }[] } {
  return scale === undefined ? {} : { lora: [{ id: 0, scale }] };
}

/**
 * `<YYYY-MM-DD>-<model>.json`, with the model name reduced to a safe single path segment. A
 * candidate run with a LoRA adapter (HUP-S9.4) adds `-lora-<first 12 hex>` so it never overwrites
 * the base run it is compared against.
 */
export function resultFileName(isoDate: string, model: string, adapterSha256?: string): string {
  const safe = model.replace(/[^A-Za-z0-9._-]/g, "_").replace(/\.\./g, "__");
  const lora = adapterSha256 ? `-lora-${adapterSha256.slice(0, 12)}` : "";
  return `${isoDate.slice(0, 10)}-${safe}${lora}.json`;
}
