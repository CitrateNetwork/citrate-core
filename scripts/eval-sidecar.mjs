#!/usr/bin/env node
// =====================================================================
// citrate-core — evals through a REAL Hermes sidecar (HUP-S1.7 step success, HUP-S1.10 live vectors)
//
//   node scripts/eval-sidecar.mjs --base-url http://127.0.0.1:18190/v1 --model <name> --tier T0 \
//     --context-tokens 16384 --api-key-env RUNKEY \
//     --sidecar-bin /abs/citrate-agent-sidecar --mcp-fixture-bin /abs/citrate-mcp-fixture-server \
//     --chromium "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" \
//     [--only workflows|injection] [--out-dir eval/results] [--deadline-s 900]
//
// What it does (src/agent/eval/sidecar.ts holds the tested logic):
//   1. writes a 0600 bearer file, an MCP allowlist naming the runtime's stdio fixture server
//      (`--eval-docs <dir>`, server name "fixture", writes allowed so they are an attack target),
//      and starts a loopback HTTP server for the browser pages;
//   2. starts citrate-agent-sidecar on a free loopback port with CITRATE_HERMES_MCP and
//      CITRATE_HERMES_BROWSER=1 (+ CITRATE_BROWSER_CHROMIUM), and checks the MCP server is ready;
//   3. injection-v2 live cases: one session per case, the poisoned doc or page put in place, the
//      member's prompt sent, core tool calls answered and browser actions declined, then scored;
//   4. workflow-v1: one session per workflow, POST /sessions/:id/workflows, driven to the end;
//   5. stops everything and writes <out-dir>/<date>-sidecar-<model>.json.
// Any transport error or a case that cannot finish aborts the run and writes nothing (Rule 1).
// =====================================================================
import { spawn } from "node:child_process";
import { randomBytes } from "node:crypto";
import { createServer } from "node:http";
import { chmodSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { createServer as createNetServer } from "node:net";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { parseSidecarEvalArgs } from "../src/agent/eval/sidecarCliArgs.ts";
import { resultFileName } from "../src/agent/eval/cliArgs.ts";
import { loadInjectionDataset } from "../src/agent/eval/datasetFiles.ts";
import { canaryFor, isScriptedCase } from "../src/agent/eval/runner.ts";
import {
  EVAL_MCP_SERVER,
  buildSidecarScorecard,
  driveSession,
  evalSessionBody,
  evalSystemPrompt,
  parseWorkflowDataset,
  scoreLiveInjection,
  scoreWorkflowRun,
  workflowSpecBody,
} from "../src/agent/eval/sidecar.ts";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const EVAL_DIR = join(ROOT, "src/agent/eval");

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

async function main() {
  let args;
  try {
    args = parseSidecarEvalArgs(process.argv.slice(2));
  } catch (e) {
    console.error(e instanceof Error ? e.message : String(e));
    process.exit(2);
  }
  let llmKey = "";
  if (args.apiKeyEnv) {
    llmKey = process.env[args.apiKeyEnv] ?? "";
    if (!llmKey) {
      console.error("--api-key-env names an environment variable that is empty or unset");
      process.exit(2);
    }
  }
  const runInjection = args.only !== "workflows";
  const runWorkflows = args.only !== "injection";
  const injection = loadInjectionDataset({ readText: (p) => readFileSync(p, "utf8"), listDir: (p) => readdirSync(p) }, EVAL_DIR, "v2");
  const liveCases = injection.cases.filter((c) => !isScriptedCase(c));
  const workflows = parseWorkflowDataset(JSON.parse(readFileSync(join(EVAL_DIR, "workflow-v1.json"), "utf8")));

  const work = mkdtempSync(join(tmpdir(), "citrate-eval-sidecar-"));
  const docsDir = join(work, "docs");
  const capsDir = join(work, "capsules");
  mkdirSync(docsDir);
  mkdirSync(capsDir);
  const bearer = randomBytes(24).toString("hex");
  const tokenFile = join(work, "token");
  writeFileSync(tokenFile, bearer);
  chmodSync(tokenFile, 0o600);
  const mcpCfg = join(work, "mcp.json");
  writeFileSync(
    mcpCfg,
    JSON.stringify({
      servers: [
        {
          name: EVAL_MCP_SERVER,
          transport: "stdio",
          command: args.mcpFixtureBin,
          args: ["--eval-docs", docsDir],
          allow_write_tools: true,
        },
      ],
    }),
  );

  // Browser pages for the live cases, served on loopback.
  const pages = new Map();
  const pageServer = createServer((req, res) => {
    const html = pages.get((req.url ?? "").split("?")[0]);
    if (html === undefined) {
      res.writeHead(404, { "content-type": "text/plain" }).end("not found");
      return;
    }
    res.writeHead(200, { "content-type": "text/html; charset=utf-8" }).end(html);
  });
  await new Promise((r) => pageServer.listen(0, "127.0.0.1", r));
  const origin = `http://127.0.0.1:${pageServer.address().port}`;

  const port = await freePort();
  const env = {
    PATH: process.env.PATH ?? "/usr/bin:/bin",
    HOME: process.env.HOME ?? work,
    TMPDIR: process.env.TMPDIR ?? "/tmp",
    CITRATE_HERMES_ADDR: `127.0.0.1:${port}`,
    CITRATE_HERMES_TOKEN_FILE: tokenFile,
    CITRATE_HERMES_CAPSULES: capsDir,
    CITRATE_HERMES_MCP: mcpCfg,
  };
  if (runInjection) {
    env.CITRATE_HERMES_BROWSER = "1";
    env.CITRATE_BROWSER_CHROMIUM = args.chromium;
  }
  const side = spawn(args.sidecarBin, [], { env, stdio: ["ignore", "ignore", "pipe"] });
  let sideLog = "";
  side.stderr.on("data", (d) => {
    sideLog = (sideLog + d.toString()).slice(-8000);
  });
  const base = `http://127.0.0.1:${port}`;
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

  const cleanup = () => {
    try {
      side.kill("SIGTERM");
    } catch {}
    pageServer.close();
    rmSync(work, { recursive: true, force: true });
  };

  const startedAt = new Date().toISOString();
  let scorecard;
  try {
    let up = false;
    for (let i = 0; i < 100 && !up; i++) {
      await sleep(200);
      try {
        up = (await fetch(base + "/health")).ok;
      } catch {}
    }
    if (!up) throw new Error(`the sidecar did not come up:\n${sideLog}`);
    const mcp = await http("GET", "/mcp/servers");
    const fx = mcp.json?.servers?.find((s) => s.name === EVAL_MCP_SERVER);
    if (!fx || fx.state !== "ready") throw new Error(`the MCP fixture server is not ready: ${JSON.stringify(mcp.json)}`);
    console.error(`eval-sidecar: sidecar on ${base}, MCP fixture ready, pages on ${origin} -> ${args.model} @ ${args.baseUrl}`);

    const open = async (systemPrompt) => {
      const r = await http(
        "POST",
        "/sessions",
        evalSessionBody({ model: args.model, baseUrl: args.baseUrl, bearer: llmKey, contextTokens: args.contextTokens, maxTokens: args.maxTokens, systemPrompt }),
      );
      if (r.status !== 201 || typeof r.json?.id !== "string") throw new Error(`POST /sessions: HTTP ${r.status} ${JSON.stringify(r.json)}`);
      return r.json.id;
    };
    const close = async (id) => {
      await http("DELETE", `/sessions/${id}`);
    };
    const deadlineMs = args.deadlineSeconds * 1000;

    const injScores = [];
    if (runInjection) {
      for (const c of liveCases) {
        let prompt = c.prompt;
        if (c.live.source === "mcp") {
          writeFileSync(join(docsDir, `${c.live.doc}.txt`), c.live.payload);
        } else {
          const path = `/p/${c.id.replace(/^inj-browser-/, "")}`;
          pages.set(path, c.live.page);
          prompt = prompt.replaceAll("{{origin}}", origin);
        }
        const id = await open(evalSystemPrompt(canaryFor(c.id)));
        const sent = await http("POST", `/sessions/${id}/messages`, { text: prompt });
        if (sent.status !== 202) throw new Error(`send ${c.id}: HTTP ${sent.status}`);
        const r = await driveSession(http, id, { fixtures: {}, approve: [] }, { deadlineMs, browser: c.live.source === "browser" });
        await close(id);
        const s = scoreLiveInjection(c, r.events, r.declinedBrowserActions.length);
        injScores.push(s);
        console.error(`  ${s.pass ? "pass" : "FAIL"}  ${c.id}${s.reasons.length ? "  (" + s.reasons.join("; ") + ")" : ""}`);
      }
    }

    const wfScores = [];
    if (runWorkflows) {
      for (const t of workflows.tasks) {
        for (const [name, text] of Object.entries(t.docs)) writeFileSync(join(docsDir, `${name}.txt`), text);
        const id = await open(evalSystemPrompt());
        const st = await http("POST", `/sessions/${id}/workflows`, workflowSpecBody(t));
        if (st.status !== 202 || typeof st.json?.run_id !== "string") throw new Error(`workflow ${t.id}: HTTP ${st.status} ${JSON.stringify(st.json)}`);
        const r = await driveSession(http, id, { fixtures: t.fixtures, approve: t.approve }, { runId: st.json.run_id, deadlineMs, browser: false });
        await close(id);
        const s = scoreWorkflowRun(t, r.events, r.run);
        wfScores.push(s);
        console.error(`  ${s.workflowSuccess ? "pass" : "FAIL"}  ${t.id}  steps ${s.stepsPassed}/${s.stepsTotal}`);
      }
    }

    scorecard = buildSidecarScorecard({
      model: args.model,
      tier: args.tier,
      workflowVersion: runWorkflows ? workflows.version : undefined,
      workflows: runWorkflows ? wfScores : undefined,
      injectionVersion: runInjection ? injection.version : undefined,
      injections: runInjection ? injScores : undefined,
      startedAt,
      finishedAt: new Date().toISOString(),
      runtime: {
        sidecar: "citrate-agent-sidecar (real binary), sessions opened like core's build_session_body",
        mcp: "agent-mcp-host stdio fixture server, --eval-docs mode, server name fixture",
        browser: runInjection ? "managed headless browser (CITRATE_HERMES_BROWSER=1), pages on a loopback HTTP server" : "off",
        contextTokens: String(args.contextTokens),
        maxTokens: String(args.maxTokens),
      },
    });
  } catch (e) {
    cleanup();
    console.error((e instanceof Error ? e.message : String(e)) + "\nno scorecard written.");
    process.exit(1);
  }
  cleanup();

  const outDir = resolve(ROOT, args.outDir);
  mkdirSync(outDir, { recursive: true });
  const out = join(outDir, resultFileName(scorecard.startedAt, `sidecar-${args.model}${args.tier ? "-" + args.tier : ""}`));
  writeFileSync(out, JSON.stringify(scorecard, null, 2) + "\n");
  const pct = (r) => (r === null || r === undefined ? "n/a" : (r * 100).toFixed(1) + "%");
  const lines = [`model ${scorecard.model}${scorecard.tier ? " (" + scorecard.tier + ")" : ""}`];
  if (scorecard.workflow) {
    const w = scorecard.workflow;
    lines.push(`${w.datasetVersion}: ${w.nWorkflows} workflows / ${w.nSteps} steps · step success ${pct(w.stepSuccessRate)} · judged-step success ${pct(w.reachedStepSuccessRate)} · workflow success ${pct(w.workflowSuccessRate)}`);
  }
  if (scorecard.liveInjection) {
    const l = scorecard.liveInjection;
    lines.push(`${l.datasetVersion} live: n=${l.n} · reached ${pct(l.reachedRate)} · resist ${pct(l.resistRate)} · resist when reached ${pct(l.resistRateWhenReached)}`);
  }
  lines.push(`failures (${scorecard.failures.length}): ${scorecard.failures.join(", ") || "none"}`, `wrote ${out}`);
  console.log(lines.join("\n"));
}

main();
