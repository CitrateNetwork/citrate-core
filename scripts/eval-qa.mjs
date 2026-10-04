#!/usr/bin/env node
// =====================================================================
// citrate-core — Citrate QA eval CLI (HUP-S3.5, US-3.1)
//
//   node scripts/eval-qa.mjs --base-url http://127.0.0.1:18080/v1 --model <name> \
//        [--api-key-env VAR] [--tier T0|T1|T2] [--out-dir eval/results] \
//        [--coverage-threshold 0..1] [--dataset qa-v1] [--allow-remote]
//        [--adapter-sha256 <hex>]   (HUP-S9.4: the endpoint serves this LoRA; stamped into the
//                                    scorecard so the app's eval gate can bind it to the file)
//        [--memory-socket <path> [--retrieval-mode tool|passages]
//         [--retrieve-tenants citrate-docs,methodology] [--retrieve-k 5]   (passages mode only)
//         [--corpus-dir <dir>] [--corpus-digest <hex>]]  (HUP-S3.1, g2-knowledge: answer from the
//                                    bundled knowledge corpus held by a mem-mcp daemon whose store
//                                    imported it.
//                                    tool (default, what the app does): the model is offered the
//                                    app's memory_search tool, picks query and tenant, and the
//                                    result is rendered with the app's formatter
//                                    (src/agent/eval/toolLoop.ts). Result <date>-qa-tool-<model>.json.
//                                    passages: each question first runs `memory.search {passages:
//                                    true}` per tenant and the passages go before the question.
//                                    Result <date>-qa-rag-<model>.json.
//                                    Both record the node ids each search returned and resolve every
//                                    answer citation to them (scorecard citationNodeRate). With
//                                    --corpus-dir (the imported corpus), a citation also counts as
//                                    valid when it resolves to a bundled node; the digest is read
//                                    from its manifest.)
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
import { buildCorpusCitationIndex } from "../src/agent/eval/corpusCitations.ts";
import { buildRetrievalContext, parsePassages, parseSearchResponse, retrievalUserMessage, searchRequestLine, selectRetrievalPassages } from "../src/agent/eval/retrieval.ts";
import { QA_TOOL_MAX_TURNS, answerWithMemoryTool } from "../src/agent/eval/toolLoop.ts";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const REQUEST_TIMEOUT_MS = 180_000;
const SEARCH_TIMEOUT_MS = 60_000;
/** Most characters of retrieved passage text put before a question (about 1,500 tokens). */
const RETRIEVAL_MAX_CHARS = 6000;

/** One `memory.search` over the daemon's local socket; resolves to the tool text. */
function searchOnce(socket, id, tenant, query, k, passages = true) {
  return new Promise((resolvePromise, reject) => {
    const conn = createConnection(socket);
    let buf = "";
    const timer = setTimeout(() => {
      conn.destroy();
      reject(new Error(`memory daemon at ${socket} did not answer within ${SEARCH_TIMEOUT_MS / 1000} s`));
    }, SEARCH_TIMEOUT_MS);
    conn.setEncoding("utf8");
    conn.on("connect", () => conn.write(searchRequestLine(id, tenant, query, k, passages)));
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

/** Retrieve passages for `question` from every configured tenant: the user turn and what it showed. */
async function retrievedUserTurn(retrieval, question, nextId) {
  const perTenant = [];
  for (const tenant of retrieval.tenants) {
    perTenant.push(parsePassages(await searchOnce(retrieval.socket, nextId(), tenant, question, retrieval.k)));
  }
  const shown = selectRetrievalPassages(perTenant, RETRIEVAL_MAX_CHARS);
  return { user: retrievalUserMessage(question, buildRetrievalContext(perTenant, RETRIEVAL_MAX_CHARS)), retrieved: shown.map((p) => (p.cite ? { id: p.id, cite: p.cite } : { id: p.id })) };
}

async function loadJson(rel) {
  return JSON.parse(await readFile(join(ROOT, rel), "utf8"));
}

/** One /chat/completions request; resolves to choices[0].message. `tools` turns on tool calling. */
async function chat(args, apiKey, messages, tools) {
  const headers = { "content-type": "application/json" };
  if (apiKey) headers.authorization = `Bearer ${apiKey}`;
  const body = { model: args.model, messages, temperature: 0 };
  if (tools) body.tools = tools;
  let res;
  try {
    res = await fetch(`${args.baseUrl}/chat/completions`, {
      method: "POST",
      headers,
      body: JSON.stringify(body),
      signal: AbortSignal.timeout(REQUEST_TIMEOUT_MS),
    });
  } catch (e) {
    const cause = e?.cause?.code || e?.cause?.message || e?.message || String(e);
    throw new Error(`cannot reach ${args.baseUrl}/chat/completions (${cause})`);
  }
  if (!res.ok) {
    const text = (await res.text()).slice(0, 300);
    throw new Error(`HTTP ${res.status} from ${args.baseUrl}/chat/completions: ${text}`);
  }
  const json = await res.json();
  const message = json?.choices?.[0]?.message;
  if (!message || typeof message !== "object") throw new Error("response has no choices[0].message");
  return message;
}

function makeAsk(args, apiKey) {
  let rpcId = 0;
  const nextId = () => ++rpcId;
  const r = args.retrieval;
  if (r?.mode === "tool") {
    // g2-knowledge (b): the app's own path. The model calls memory_search; the daemon answers.
    return async (question) => {
      const out = await answerWithMemoryTool(QA_SYSTEM_PROMPT, question, {
        complete: (messages, tools) => chat(args, apiKey, messages, tools),
        search: (tenant, query, k, passages) => searchOnce(r.socket, nextId(), tenant, query, k, passages),
      });
      if (out.turnLimit) console.error(`  (no answer within ${QA_TOOL_MAX_TURNS} model requests)`);
      return { text: out.text, retrieved: out.retrieved, toolCalls: out.calls };
    };
  }
  return async (question) => {
    // HUP-S3.1: in passages mode the model answers from passages retrieved before the question.
    const turn = r ? await retrievedUserTurn(r, question, nextId) : { user: question };
    const message = await chat(args, apiKey, [
      { role: "system", content: QA_SYSTEM_PROMPT },
      { role: "user", content: turn.user },
    ]);
    if (typeof message.content !== "string") throw new Error("response has no choices[0].message.content string");
    return turn.retrieved ? { text: message.content, retrieved: turn.retrieved } : { text: message.content };
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
      (args.retrieval
        ? args.retrieval.mode === "tool"
          ? ` · memory_search tool (k=${args.retrieval.k}, ${QA_TOOL_MAX_TURNS} turns) via ${args.retrieval.socket}`
          : ` · retrieval ${args.retrieval.tenants.join("+")} k=${args.retrieval.k} via ${args.retrieval.socket}`
        : " · closed-book"),
  );

  // HUP-S3.1: the bundled corpus the answers may cite (retrieval runs with --corpus-dir).
  let corpus;
  if (args.retrieval?.corpusDir) {
    const dir = resolve(args.retrieval.corpusDir);
    const manifest = JSON.parse(await readFile(join(dir, "manifest.json"), "utf8"));
    if (args.retrieval.corpusDigest && args.retrieval.corpusDigest !== manifest.bundle_digest) {
      console.error(`--corpus-digest ${args.retrieval.corpusDigest} is not the corpus in ${dir} (${manifest.bundle_digest})`);
      process.exit(2);
    }
    args.retrieval.corpusDigest = manifest.bundle_digest;
    corpus = buildCorpusCitationIndex(await Promise.all(manifest.tenants.map((t) => readFile(join(dir, t.file), "utf8"))));
    console.error(`eval-qa: citations may resolve to ${corpus.files.size} bundled files (corpus ${manifest.bundle_digest.slice(0, 12)})`);
  }

  let out;
  try {
    out = await runQaEval(
      ds,
      index,
      { ask: makeAsk(args, apiKey), onProgress: (id, pass) => console.error(`  ${pass ? "pass" : "FAIL"}  ${id}`) },
      { model: args.model, tier: args.tier },
      {
        ...(args.coverageThreshold === undefined ? {} : { coverageThreshold: args.coverageThreshold }),
        ...(corpus ? { corpus } : {}),
      },
    );
  } catch (e) {
    console.error((e instanceof Error ? e.message : String(e)) + "\nno scorecard written.");
    process.exit(1);
  }

  const { scorecard, items } = out;
  if (args.adapterSha256) scorecard.adapterSha256 = args.adapterSha256;
  if (args.retrieval) {
    scorecard.retrieval =
      args.retrieval.mode === "tool"
        ? { mode: "memory_search tool", tenants: args.retrieval.tenants, k: args.retrieval.k, maxTurns: QA_TOOL_MAX_TURNS }
        : { mode: "memory.search passages", tenants: args.retrieval.tenants, k: args.retrieval.k };
    if (args.retrieval.corpusDigest) scorecard.retrieval.corpusDigest = args.retrieval.corpusDigest;
    if (corpus) scorecard.retrieval.citationsResolveToCorpus = true;
  }
  const outDir = resolve(ROOT, args.outDir);
  await mkdir(outDir, { recursive: true });
  const file = join(outDir, qaResultFileName(scorecard.startedAt, args.model, ds.version, args.adapterSha256, args.retrieval?.mode));
  await writeFile(file, JSON.stringify({ scorecard, items }, null, 2) + "\n");
  const pct = (r) => (r === null ? "n/a" : (r * 100).toFixed(1) + "%");
  console.log(
    [
      `model ${scorecard.model}${scorecard.tier ? " (" + scorecard.tier + ")" : ""} · ${scorecard.datasetVersion} · n=${scorecard.n}`,
      `pass ${pct(scorecard.passRate)} · key points ${pct(scorecard.keyPointCoverage)} · citation hit ${pct(scorecard.citationHitRate)} · ` +
        `citation validity ${pct(scorecard.citationValidity)} · abstention ${pct(scorecard.abstentionRate)} · ` +
        `false abstention ${pct(scorecard.falseAbstentionRate)}` +
        (scorecard.citationNodeRate === undefined ? "" : ` · citations to nodes ${pct(scorecard.citationNodeRate)}`),
      `failures (${scorecard.failures.length}): ${scorecard.failures.join(", ") || "none"}`,
      `wrote ${file}`,
    ].join("\n"),
  );
}

main();
