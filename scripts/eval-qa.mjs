#!/usr/bin/env node
// =====================================================================
// citrate-core — Citrate QA eval CLI (HUP-S3.5, US-3.1)
//
//   node scripts/eval-qa.mjs --base-url http://127.0.0.1:18080/v1 --model <name> \
//        [--api-key-env VAR] [--tier T0|T1|T2] [--out-dir eval/results] \
//        [--coverage-threshold 0..1] [--allow-remote]
//
// Asks every question in src/agent/eval/qa-v1.json of a LIVE OpenAI-compatible /chat/completions
// endpoint (llama-server, or a user endpoint) and writes the scorecard plus per-item answers to
// <out-dir>/<date>-qa-<model>.json. Scoring is deterministic (src/agent/eval/qa.ts): key-point
// coverage, citation validity against qa-v1.anchors.json, abstention on unanswerable items. No
// model-as-judge. A transport error aborts the run and writes nothing (Rule 1: no partial or
// invented scorecard). Non-loopback URLs are refused unless --allow-remote.
//
// Sibling of scripts/eval-tools.mjs (tool-call + injection eval, HUP-S1.7/S1.10). Requires
// Node >= 22.18 / 23.6 (built-in TypeScript type stripping, no new dependency).
// =====================================================================
import { readFile, mkdir, writeFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { parseQaCliArgs, qaResultFileName } from "../src/agent/eval/qaCliArgs.ts";
import { QA_SYSTEM_PROMPT, findMissingCitations, parseQaDataset, runQaEval } from "../src/agent/eval/qa.ts";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const REQUEST_TIMEOUT_MS = 180_000;

async function loadJson(rel) {
  return JSON.parse(await readFile(join(ROOT, rel), "utf8"));
}

function makeAsk(args, apiKey) {
  return async (question) => {
    const headers = { "content-type": "application/json" };
    if (apiKey) headers.authorization = `Bearer ${apiKey}`;
    const messages = [
      { role: "system", content: QA_SYSTEM_PROMPT },
      { role: "user", content: question },
    ];
    let res;
    try {
      res = await fetch(`${args.baseUrl}/chat/completions`, {
        method: "POST",
        headers,
        body: JSON.stringify({ model: args.model, messages, temperature: 0 }),
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
    const content = json?.choices?.[0]?.message?.content;
    if (typeof content !== "string") throw new Error("response has no choices[0].message.content string");
    return { text: content };
  };
}

async function main() {
  let args;
  try {
    args = parseQaCliArgs(process.argv.slice(2));
  } catch (e) {
    console.error(e instanceof Error ? e.message : String(e));
    process.exit(2);
  }
  let apiKey;
  if (args.apiKeyEnv) {
    apiKey = process.env[args.apiKeyEnv];
    if (!apiKey) {
      console.error(`--api-key-env ${args.apiKeyEnv} is set but that env var is empty`);
      process.exit(2);
    }
  }

  const ds = parseQaDataset(await loadJson("src/agent/eval/qa-v1.json"));
  const index = await loadJson("src/agent/eval/qa-v1.anchors.json");
  const missing = findMissingCitations(ds, index);
  if (missing.length) {
    console.error(`anchor index does not cover the dataset:\n${missing.join("\n")}\nrun scripts/qa-anchors.mjs --write`);
    process.exit(2);
  }
  console.error(
    `eval-qa: ${ds.version} (${ds.items.length}) → ${args.model} @ ${args.baseUrl}${args.allowRemote ? " (remote allowed)" : ""}`,
  );

  let out;
  try {
    out = await runQaEval(
      ds,
      index,
      { ask: makeAsk(args, apiKey), onProgress: (id, pass) => console.error(`  ${pass ? "pass" : "FAIL"}  ${id}`) },
      { model: args.model, tier: args.tier },
      args.coverageThreshold === undefined ? {} : { coverageThreshold: args.coverageThreshold },
    );
  } catch (e) {
    console.error((e instanceof Error ? e.message : String(e)) + "\nno scorecard written.");
    process.exit(1);
  }

  const { scorecard, items } = out;
  const outDir = resolve(ROOT, args.outDir);
  await mkdir(outDir, { recursive: true });
  const file = join(outDir, qaResultFileName(scorecard.startedAt, args.model));
  await writeFile(file, JSON.stringify({ scorecard, items }, null, 2) + "\n");
  const pct = (r) => (r === null ? "n/a" : (r * 100).toFixed(1) + "%");
  console.log(
    [
      `model ${scorecard.model}${scorecard.tier ? " (" + scorecard.tier + ")" : ""} · ${scorecard.datasetVersion} · n=${scorecard.n}`,
      `pass ${pct(scorecard.passRate)} · key points ${pct(scorecard.keyPointCoverage)} · citation hit ${pct(scorecard.citationHitRate)} · ` +
        `citation validity ${pct(scorecard.citationValidity)} · abstention ${pct(scorecard.abstentionRate)} · ` +
        `false abstention ${pct(scorecard.falseAbstentionRate)}`,
      `failures (${scorecard.failures.length}): ${scorecard.failures.join(", ") || "none"}`,
      `wrote ${file}`,
    ].join("\n"),
  );
}

main();
