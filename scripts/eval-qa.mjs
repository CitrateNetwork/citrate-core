#!/usr/bin/env node
// =====================================================================
// citrate-core — Citrate QA eval CLI (HUP-S3.5, US-3.1)
//
//   node scripts/eval-qa.mjs --base-url http://127.0.0.1:18080/v1 --model <name> \
//        [--api-key-env VAR] [--tier T0|T1|T2] [--out-dir eval/results] \
//        [--coverage-threshold 0..1] [--dataset qa-v1] [--allow-remote]
//        [--adapter-sha256 <hex>]   (HUP-S9.4: the endpoint serves this LoRA; stamped into the
//                                    scorecard so the app's eval gate can bind it to the file)
//        [--memory-socket <path> [--retrieve-tenants citrate-docs,methodology] [--retrieve-k 5]
//         [--corpus-digest <hex>]]  (HUP-S3.1, g2-knowledge: answer from the bundled knowledge corpus.
//                                    Each question first runs `memory.search {passages: true}` per
//                                    tenant on a mem-mcp daemon whose store holds the imported
//                                    corpus; the passages and their citations go before the
//                                    question. Result file <date>-qa-rag-<model>.json.)
//
// Asks every question in src/agent/eval/qa-v1.json (or, HUP-S7.7, the set named by --dataset, e.g.
// qa-literacy-v1) of a LIVE OpenAI-compatible /chat/completions endpoint (llama-server, or a user
// endpoint) and writes the scorecard plus per-item answers to <out-dir>/<date>-qa-<model>.json
// (<date>-<set>-<model>.json for a set other than qa-v1). Scoring is deterministic (src/agent/eval/qa.ts): key-point
// coverage, citation validity against qa-v1.anchors.json, abstention on unanswerable items. No
// model-as-judge. A transport error aborts the run and writes nothing (Rule 1: no partial or
// invented scorecard). Non-loopback URLs are refused unless --allow-remote.
//
// Sibling of scripts/eval-tools.mjs (tool-call + injection eval, HUP-S1.7/S1.10). Requires
// Node >= 22.18 / 23.6 (built-in TypeScript type stripping, no new dependency).
// =====================================================================
import { readFile, mkdir, writeFile } from "node:fs/promises";
import { createConnection } from "node:net";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { parseQaCliArgs, qaDatasetFiles, qaResultFileName } from "../src/agent/eval/qaCliArgs.ts";
import { QA_SYSTEM_PROMPT, findMissingCitations, parseQaDataset, runQaEval } from "../src/agent/eval/qa.ts";
import { buildRetrievalContext, parsePassages, parseSearchResponse, retrievalUserMessage, searchRequestLine } from "../src/agent/eval/retrieval.ts";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const REQUEST_TIMEOUT_MS = 180_000;
const SEARCH_TIMEOUT_MS = 60_000;
/** Most characters of retrieved passage text put before a question (about 1,500 tokens). */
const RETRIEVAL_MAX_CHARS = 6000;

/** One `memory.search {passages: true}` over the daemon's local socket; resolves to the tool text. */
function searchOnce(socket, id, tenant, query, k) {
  return new Promise((resolvePromise, reject) => {
    const conn = createConnection(socket);
    let buf = "";
    const timer = setTimeout(() => {
      conn.destroy();
      reject(new Error(`memory daemon at ${socket} did not answer within ${SEARCH_TIMEOUT_MS / 1000} s`));
    }, SEARCH_TIMEOUT_MS);
    conn.setEncoding("utf8");
    conn.on("connect", () => conn.write(searchRequestLine(id, tenant, query, k)));
    conn.on("data", (chunk) => {
      buf += chunk;
      const nl = buf.indexOf("\n");
      if (nl < 0) return;
      clearTimeout(timer);
      conn.end();
      try {
        resolvePromise(parseSearchResponse(buf.slice(0, nl)));
      } catch (e) {
        reject(e);
      }
    });
    conn.on("error", (e) => {
      clearTimeout(timer);
      reject(new Error(`cannot reach the memory daemon at ${socket} (${e.code || e.message})`));
    });
  });
}

/** Retrieve passages for `question` from every configured tenant and build the user turn. */
async function retrievedUserTurn(retrieval, question, nextId) {
  const perTenant = [];
  for (const tenant of retrieval.tenants) {
    perTenant.push(parsePassages(await searchOnce(retrieval.socket, nextId(), tenant, question, retrieval.k)));
  }
  return retrievalUserMessage(question, buildRetrievalContext(perTenant, RETRIEVAL_MAX_CHARS));
}

async function loadJson(rel) {
  return JSON.parse(await readFile(join(ROOT, rel), "utf8"));
}

function makeAsk(args, apiKey) {
  let rpcId = 0;
  const nextId = () => ++rpcId;
  return async (question) => {
    const headers = { "content-type": "application/json" };
    if (apiKey) headers.authorization = `Bearer ${apiKey}`;
    // HUP-S3.1: with --memory-socket the model answers from retrieved corpus passages.
    const user = args.retrieval ? await retrievedUserTurn(args.retrieval, question, nextId) : question;
    const messages = [
      { role: "system", content: QA_SYSTEM_PROMPT },
      { role: "user", content: user },
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
      console.error("--api-key-env names an environment variable that is empty or unset");
      process.exit(2);
    }
  }

  const files = qaDatasetFiles(args.dataset);
  const ds = parseQaDataset(await loadJson(files.dataset));
  const index = await loadJson(files.index);
  const missing = findMissingCitations(ds, index);
  if (missing.length) {
    console.error(`anchor index does not cover the dataset:\n${missing.join("\n")}\nrun scripts/qa-anchors.mjs --write`);
    process.exit(2);
  }
  console.error(
    `eval-qa: ${ds.version} (${ds.items.length}) → ${args.model} @ ${args.baseUrl}${args.allowRemote ? " (remote allowed)" : ""}` +
      (args.retrieval ? ` · retrieval ${args.retrieval.tenants.join("+")} k=${args.retrieval.k} via ${args.retrieval.socket}` : " · closed-book"),
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
  if (args.adapterSha256) scorecard.adapterSha256 = args.adapterSha256;
  if (args.retrieval) {
    scorecard.retrieval = { mode: "memory.search passages", tenants: args.retrieval.tenants, k: args.retrieval.k };
    if (args.retrieval.corpusDigest) scorecard.retrieval.corpusDigest = args.retrieval.corpusDigest;
  }
  const outDir = resolve(ROOT, args.outDir);
  await mkdir(outDir, { recursive: true });
  const file = join(outDir, qaResultFileName(scorecard.startedAt, args.model, ds.version, args.adapterSha256, Boolean(args.retrieval)));
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
