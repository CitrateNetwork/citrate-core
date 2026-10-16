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
//                                    sidecar (HUP-S7.7, US-9.2 AC1): each question runs in a REAL
//                                    citrate-agent-sidecar session that offers the bundled skills
//                                    (--skills, --skills-lock/--skills-third-party) and the
//                                    memory_search core tool, which this script answers from the
//                                    daemon (src/agent/eval/qaSidecar.ts). Needs --sidecar-bin and
//                                    --context-tokens. Result <date>-<set>-sidecar-<model>.json.
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
import { spawn } from "node:child_process";
import { randomBytes } from "node:crypto";
import { chmodSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { readFile, mkdir, writeFile } from "node:fs/promises";
import { createConnection, createServer as createNetServer } from "node:net";
import { tmpdir } from "node:os";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { parseQaCliArgs, qaDatasetFiles, qaResultFileName } from "../src/agent/eval/qaCliArgs.ts";
import { QA_SYSTEM_PROMPT, findMissingCitations, parseQaDataset, runQaEval } from "../src/agent/eval/qa.ts";
import { buildCorpusCitationIndex } from "../src/agent/eval/corpusCitations.ts";
import { buildRetrievalContext, parsePassages, parseSearchResponse, retrievalUserMessage, searchRequestLine, selectRetrievalPassages } from "../src/agent/eval/retrieval.ts";
import { QA_TOOL_MAX_TURNS, answerWithMemoryTool } from "../src/agent/eval/toolLoop.ts";
import { qaCoreAnswerer, qaOutcomeFromEvents, qaSessionBody } from "../src/agent/eval/qaSidecar.ts";
import { driveSession } from "../src/agent/eval/sidecar.ts";

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

function freePort() {
  return new Promise((res, rej) => {
    const s = createNetServer();
    s.once("error", rej);
    s.listen(0, "127.0.0.1", () => {
      const { port } = s.address();
      s.close(() => res(port));
    });
  });
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/**
 * HUP-S7.7: start the real sidecar with the skills sources (as core sets them for the child) and a
 * 0600 bearer file. Resolves to { http, stop, log }; rejects when it does not come up.
 */
async function startSidecar(sc) {
  const work = mkdtempSync(join(tmpdir(), "citrate-eval-qa-sidecar-"));
  const capsDir = join(work, "capsules");
  mkdirSync(capsDir);
  const bearer = randomBytes(24).toString("hex");
  const tokenFile = join(work, "token");
  writeFileSync(tokenFile, bearer);
  chmodSync(tokenFile, 0o600);
  const port = await freePort();
  const env = {
    PATH: process.env.PATH ?? "/usr/bin:/bin",
    HOME: process.env.HOME ?? work,
    TMPDIR: process.env.TMPDIR ?? "/tmp",
    CITRATE_HERMES_ADDR: `127.0.0.1:${port}`,
    CITRATE_HERMES_TOKEN_FILE: tokenFile,
    CITRATE_HERMES_CAPSULES: capsDir,
  };
  if (sc.skills.length) env.CITRATE_HERMES_SKILLS = sc.skills.join(":");
  if (sc.thirdParty) {
    env.CITRATE_HERMES_SKILLS_LOCK = sc.thirdParty.lock;
    env.CITRATE_HERMES_SKILLS_THIRD_PARTY = sc.thirdParty.root;
  }
  const side = spawn(sc.bin, [], { env, stdio: ["ignore", "ignore", "pipe"] });
  let log = "";
  side.stderr.on("data", (d) => {
    log = (log + d.toString()).slice(-8000);
  });
  const base = `http://127.0.0.1:${port}`;
  const stop = () => {
    try {
      side.kill("SIGTERM");
    } catch {}
    rmSync(work, { recursive: true, force: true });
  };
  const http = async (method, path, body) => {
    let res;
    try {
      res = await fetch(base + path, {
        method,
        headers: { authorization: `Bearer ${bearer}`, "content-type": "application/json" },
        body: body === undefined ? undefined : JSON.stringify(body),
        signal: AbortSignal.timeout(60_000),
      });
    } catch (e) {
      throw new Error(`sidecar ${method} ${path}: ${e?.cause?.code || e?.message || String(e)}`);
    }
    const text = await res.text();
    let json = null;
    try {
      json = text ? JSON.parse(text) : null;
    } catch {
      json = { raw: text.slice(0, 300) };
    }
    return { status: res.status, json };
  };
  let up = false;
  // Loading and hash-checking the reviewed skills can take minutes on a loaded machine.
  for (let i = 0; i < 1500 && !up; i++) {
    await sleep(200);
    try {
      up = (await fetch(base + "/health")).ok;
    } catch {}
  }
  if (!up) {
    stop();
    throw new Error(`the sidecar did not come up:\n${log}`);
  }
  return { http, stop, log: () => log, base };
}

function makeAsk(args, apiKey, sidecar) {
  let rpcId = 0;
  const nextId = () => ++rpcId;
  const r = args.retrieval;
  if (r?.mode === "sidecar") {
    // HUP-S7.7 / US-9.2 AC1: the app's path with the sidecar loop on. One session per question.
    const sc = r.sidecar;
    return async (question) => {
      const open = await sidecar.http(
        "POST",
        "/sessions",
        qaSessionBody({ model: args.model, baseUrl: args.baseUrl, bearer: apiKey ?? "", contextTokens: sc.contextTokens, maxTokens: sc.maxTokens, systemPrompt: QA_SYSTEM_PROMPT }),
      );
      if (open.status !== 201 || typeof open.json?.id !== "string") throw new Error(`POST /sessions: HTTP ${open.status} ${JSON.stringify(open.json)}`);
      const id = open.json.id;
      const sent = await sidecar.http("POST", `/sessions/${id}/messages`, { text: question });
      if (sent.status !== 202) throw new Error(`send: HTTP ${sent.status} ${JSON.stringify(sent.json)}`);
      const log = { calls: [], retrieved: [] };
      const run = await driveSession(sidecar.http, id, { fixtures: {}, approve: [] }, {
        deadlineMs: sc.deadlineSeconds * 1000,
        browser: false,
        answerCore: qaCoreAnswerer((tenant, query, k, passages) => searchOnce(r.socket, nextId(), tenant, query, k, passages), log),
      });
      await sidecar.http("DELETE", `/sessions/${id}`);
      const o = qaOutcomeFromEvents(run.events);
      if (!o.text) console.error(`  (no answer; session outcome ${o.outcome ?? "unknown"})`);
      return { text: o.text, retrieved: log.retrieved, toolCalls: log.calls, skillLoads: o.skillLoads };
    };
  }
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
        ? args.retrieval.mode === "sidecar"
          ? ` · sidecar ${args.retrieval.sidecar.bin} (skills: ${args.retrieval.sidecar.skills.length} dir(s)${args.retrieval.sidecar.thirdParty ? " + reviewed third-party" : ""}; ctx ${args.retrieval.sidecar.contextTokens}) · memory_search via ${args.retrieval.socket}`
          : args.retrieval.mode === "tool"
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

  let sidecar;
  if (args.retrieval?.mode === "sidecar") {
    try {
      sidecar = await startSidecar(args.retrieval.sidecar);
    } catch (e) {
      console.error((e instanceof Error ? e.message : String(e)) + "\nno scorecard written.");
      process.exit(1);
    }
    console.error(`eval-qa: sidecar up on ${sidecar.base}`);
  }

  let out;
  try {
    out = await runQaEval(
      ds,
      index,
      { ask: makeAsk(args, apiKey, sidecar), onProgress: (id, pass) => console.error(`  ${pass ? "pass" : "FAIL"}  ${id}`) },
      { model: args.model, tier: args.tier },
      {
        ...(args.coverageThreshold === undefined ? {} : { coverageThreshold: args.coverageThreshold }),
        ...(corpus ? { corpus } : {}),
      },
    );
  } catch (e) {
    sidecar?.stop();
    console.error((e instanceof Error ? e.message : String(e)) + "\nno scorecard written.");
    process.exit(1);
  }
  sidecar?.stop();

  const { scorecard, items } = out;
  if (args.adapterSha256) scorecard.adapterSha256 = args.adapterSha256;
  if (args.retrieval) {
    const sc = args.retrieval.sidecar;
    scorecard.retrieval =
      args.retrieval.mode === "sidecar"
        ? {
            mode: "memory_search tool via sidecar",
            tenants: args.retrieval.tenants,
            k: args.retrieval.k,
            sidecar: {
              skills: sc.skills.map((d) => relative(ROOT, d) || "."),
              reviewedThirdParty: Boolean(sc.thirdParty),
              contextTokens: sc.contextTokens,
              maxTokens: sc.maxTokens,
            },
          }
        : args.retrieval.mode === "tool"
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
