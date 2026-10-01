#!/usr/bin/env node
// =====================================================================
// citrate-core — HUP tool-call + injection eval CLI (HUP-S1.7 / HUP-S1.10)
//
//   node scripts/eval-tools.mjs --base-url http://127.0.0.1:18080/v1 --model <name> \
//        [--api-key-env VAR] [--tier T0|T1|T2] [--out-dir eval/results] [--allow-remote]
//
// Runs src/agent/eval/{toolcall-v1,injection-v1}.json against a LIVE OpenAI-compatible
// /chat/completions endpoint (llama-server --jinja, or a user endpoint) and writes the
// scorecard to <out-dir>/<date>-<model>.json. Scoring is deterministic (runner.ts); there is
// no model-as-judge. A transport error aborts the run and writes nothing (Rule 1: no
// partial or invented scorecard). Non-loopback URLs are refused unless --allow-remote.
//
// Loads the TypeScript runner with Node's built-in type stripping (Node >= 22.18 / 23.6),
// so no new dependency (tsx etc.) is needed. See eval/README.md.
// =====================================================================
import { readFile, mkdir, writeFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { parseEvalCliArgs, resultFileName } from "../src/agent/eval/cliArgs.ts";
import { parseToolcallDataset, parseInjectionDataset, runEvalSuite } from "../src/agent/eval/runner.ts";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const REQUEST_TIMEOUT_MS = 180_000;

async function loadJson(rel) {
  return JSON.parse(await readFile(join(ROOT, rel), "utf8"));
}

function makeComplete(args, apiKey) {
  return async (messages, tools) => {
    const headers = { "content-type": "application/json" };
    if (apiKey) headers.authorization = `Bearer ${apiKey}`;
    let res;
    try {
      res = await fetch(`${args.baseUrl}/chat/completions`, {
        method: "POST",
        headers,
        body: JSON.stringify({ model: args.model, messages, tools, tool_choice: "auto", temperature: 0 }),
        signal: AbortSignal.timeout(REQUEST_TIMEOUT_MS),
      });
    } catch (e) {
      const cause = e?.cause?.code || e?.cause?.message || e?.message || String(e);
      throw new Error(`cannot reach ${args.baseUrl}/chat/completions (${cause})`);
    }
    if (!res.ok) {
      const body = (await res.text()).slice(0, 300);
      throw new Error(`HTTP ${res.status} from ${args.baseUrl}/chat/completions: ${body}`);
    }
    const json = await res.json();
    const msg = json?.choices?.[0]?.message;
    if (!msg || typeof msg !== "object") throw new Error("response has no choices[0].message");
    return msg;
  };
}

async function main() {
  let args;
  try {
    args = parseEvalCliArgs(process.argv.slice(2));
  } catch (e) {
    console.error(e instanceof Error ? e.message : String(e));
    process.exit(2);
  }
  let apiKey;
  if (args.apiKeyEnv) {
    apiKey = process.env[args.apiKeyEnv];
    if (!apiKey) {
      console.error("--api-key-env names an environment variable that is empty or unset");
      process.exit(2);
    }
  }

  const toolcall = parseToolcallDataset(await loadJson("src/agent/eval/toolcall-v1.json"));
  const injection = parseInjectionDataset(await loadJson("src/agent/eval/injection-v1.json"));
  console.error(
    `eval: ${toolcall.version} (${toolcall.tasks.length}) + ${injection.version} (${injection.cases.length}) ` +
      `→ ${args.model} @ ${args.baseUrl}${args.allowRemote ? " (remote allowed)" : ""}`,
  );

  let scorecard;
  try {
    scorecard = await runEvalSuite({
      complete: makeComplete(args, apiKey),
      model: args.model,
      tier: args.tier,
      toolcall: { version: toolcall.version, tasks: toolcall.tasks },
      injection: { version: injection.version, cases: injection.cases },
      onProgress: (id, pass) => console.error(`  ${pass ? "pass" : "FAIL"}  ${id}`),
    });
  } catch (e) {
    console.error((e instanceof Error ? e.message : String(e)) + "\nno scorecard written.");
    process.exit(1);
  }

  const outDir = resolve(ROOT, args.outDir);
  await mkdir(outDir, { recursive: true });
  const out = join(outDir, resultFileName(scorecard.startedAt, args.model));
  await writeFile(out, JSON.stringify(scorecard, null, 2) + "\n");
  const pct = (r) => (r === null ? "n/a" : (r * 100).toFixed(1) + "%");
  console.log(
    [
      `model ${scorecard.model}${scorecard.tier ? " (" + scorecard.tier + ")" : ""} · ${scorecard.datasetVersion} · n=${scorecard.n}`,
      `valid tool call ${pct(scorecard.validToolCallRate)} · correct tool ${pct(scorecard.correctToolRate)} · ` +
        `args ok ${pct(scorecard.argsOkRate)} · injection resist ${pct(scorecard.injectionResistRate)}`,
      `failures (${scorecard.failures.length}): ${scorecard.failures.join(", ") || "none"}`,
      `wrote ${out}`,
    ].join("\n"),
  );
}

main();
