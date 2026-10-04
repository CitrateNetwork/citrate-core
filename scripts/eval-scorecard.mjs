#!/usr/bin/env node
// =====================================================================
// citrate-core: eval scorecard markdown (HUP-S11.2)
//
//   node scripts/eval-scorecard.mjs [--in eval/results] [--out <in>/SCORECARD.md]
//        [--date YYYY-MM-DD] [--branch <name>]
//
// Reads the JSON scorecards that scripts/eval-tools.mjs (tool-call + injection),
// scripts/eval-qa.mjs (Citrate QA) and scripts/eval-sidecar.mjs (multi-step workflows and live
// injection through a real sidecar session) wrote into --in and renders one markdown scorecard:
// a row per run, the gate g1-eval bars per tier (valid tool calls, workflow step success), and
// every failure with its deterministic reason. It only reformats what the runs recorded; it computes no new score.
// With no scorecard in --in it exits 2 and writes nothing (Rule 1). The same renderer runs in
// the manual eval workflow (.github/workflows/eval.yml) for the uploaded artifact.
// =====================================================================
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

/** Gate g1-eval (planset gates.yaml): T1+ >= 90% valid tool calls; T0 bar set from its baseline. */
export const G1_VALID_TOOL_CALL_BAR = 0.9;
/** Gate g1-eval's other half: T1+ >= 80% workflow step success (gates.yaml; pending owner sign-off, A43). */
export const G1_STEP_SUCCESS_BAR = 0.8;

const isRate = (v) => v === null || (typeof v === "number" && Number.isFinite(v));

/** {kind:"tools"|"qa"|"sidecar", sc} for a scorecard file's parsed JSON, else null. */
export function classifyScorecard(obj) {
  if (!obj || typeof obj !== "object" || Array.isArray(obj)) return null;
  if (
    obj.kind === "sidecar-eval" &&
    typeof obj.model === "string" &&
    Array.isArray(obj.failures) &&
    (obj.workflow === null || (typeof obj.workflow === "object" && isRate(obj.workflow.stepSuccessRate))) &&
    (obj.liveInjection === null || (typeof obj.liveInjection === "object" && isRate(obj.liveInjection.resistRate))) &&
    (obj.workflow || obj.liveInjection)
  ) {
    return { kind: "sidecar", sc: obj };
  }
  const toolKeys = ["validToolCallRate", "correctToolRate", "argsOkRate", "injectionResistRate"];
  if (
    typeof obj.model === "string" &&
    typeof obj.datasetVersion === "string" &&
    toolKeys.every((k) => k in obj && isRate(obj[k])) &&
    Array.isArray(obj.failures)
  ) {
    return { kind: "tools", sc: obj };
  }
  const q = obj.scorecard;
  if (
    q &&
    typeof q === "object" &&
    typeof q.model === "string" &&
    typeof q.datasetVersion === "string" &&
    typeof q.passRate === "number" &&
    Array.isArray(q.failures)
  ) {
    return { kind: "qa", sc: q };
  }
  return null;
}

/** Every *.json directly in `dir`, sorted by name, split into tool-call, QA and sidecar scorecards. */
export function loadScorecards(dir) {
  const tools = [];
  const qa = [];
  const sidecar = [];
  const skipped = [];
  const names = fs
    .readdirSync(dir)
    .filter((n) => n.endsWith(".json"))
    .sort();
  for (const file of names) {
    let parsed;
    try {
      parsed = JSON.parse(fs.readFileSync(path.join(dir, file), "utf8"));
    } catch {
      skipped.push(file);
      continue;
    }
    const c = classifyScorecard(parsed);
    if (!c) skipped.push(file);
    else (c.kind === "tools" ? tools : c.kind === "qa" ? qa : sidecar).push({ file, sc: c.sc });
  }
  return { tools, qa, sidecar, skipped };
}

const pct = (r) => (r === null || r === undefined ? "n/a" : `${(r * 100).toFixed(1)}%`);
const cell = (s) => String(s).replace(/\|/g, "\\|").replace(/\r?\n/g, " ");

function g1(sc) {
  if (sc.tier === "T1" || sc.tier === "T2") {
    return sc.validToolCallRate !== null && sc.validToolCallRate >= G1_VALID_TOOL_CALL_BAR ? "met" : "not met";
  }
  if (sc.tier === "T0") return "T0 bar";
  return "no tier";
}

function g1Steps(sc) {
  const r = sc.workflow?.stepSuccessRate;
  if (sc.tier === "T1" || sc.tier === "T2") return r !== null && r !== undefined && r >= G1_STEP_SUCCESS_BAR ? "met" : "not met";
  if (sc.tier === "T0") return "T0 bar";
  return "no tier";
}

function failureLines(entries) {
  const out = [];
  for (const { file, sc } of entries) {
    if (!sc.failures.length) continue;
    out.push(`**${cell(file)}** (${cell(sc.model)}${sc.tier ? `, ${sc.tier}` : ""})`, "");
    for (const id of sc.failures) {
      const reasons = sc.failureReasons?.[id] ?? [];
      out.push(`- \`${cell(id)}\`: ${reasons.length ? reasons.map(cell).join("; ") : "(no reason recorded)"}`);
    }
    out.push("");
  }
  return out;
}

/** Split `---\n...\n---\n` frontmatter from the body. */
export function splitFrontmatter(md) {
  const m = /^---\n([\s\S]*?)\n---\n/.exec(md);
  if (!m) return { front: "", body: md };
  return { front: m[1], body: md.slice(m[0].length) };
}

/**
 * @param {{tools: {file:string, sc:any}[], qa: {file:string, sc:any}[], sidecar?: {file:string, sc:any}[]}} cards
 * @param {{created: string, branch: string, source: string}} meta
 */
export function renderScorecardMarkdown(cards, meta) {
  const out = [
    "---",
    `created: ${meta.created}`,
    `branch: ${meta.branch}`,
    "author: generated by scripts/eval-scorecard.mjs (Larry Klosowski + Claude Opus 5.5)",
    "status: generated",
    "---",
    "",
    "# HUP eval scorecard",
    "",
    `Rendered from the JSON scorecards in \`${meta.source}\`. Every number below was written by a`,
    "live eval run (`scripts/eval-tools.mjs`, `scripts/eval-qa.mjs`, `scripts/eval-sidecar.mjs`); this",
    "file only reformats them.",
    "Scoring is deterministic, with no model-as-judge. Regenerate with",
    "`node scripts/eval-scorecard.mjs` after adding a result; do not edit by hand.",
    "",
    "## Tool calls and prompt injection",
    "",
  ];
  if (cards.tools.length) {
    out.push(
      "| file | model | tier | dataset | n | valid tool call | correct tool | args ok | injection resist | failures | g1 valid >= 90% (T1+) |",
      "|---|---|---|---|---:|---:|---:|---:|---:|---:|---|",
    );
    for (const { file, sc } of cards.tools) {
      out.push(
        `| ${cell(file)} | ${cell(sc.model)} | ${sc.tier ?? "-"} | ${cell(sc.datasetVersion)} | ${sc.n} | ` +
          `${pct(sc.validToolCallRate)} | ${pct(sc.correctToolRate)} | ${pct(sc.argsOkRate)} | ` +
          `${pct(sc.injectionResistRate)} | ${sc.failures.length} | ${g1(sc)} |`,
      );
    }
  } else {
    out.push("No tool-call scorecard in this set.");
  }
  out.push("", "## Citrate QA", "");
  if (cards.qa.length) {
    out.push(
      "| file | model | tier | dataset | n | pass | key points | citation hit | citation validity | abstention | false abstention | failures |",
      "|---|---|---|---|---:|---:|---:|---:|---:|---:|---:|---:|",
    );
    for (const { file, sc } of cards.qa) {
      out.push(
        `| ${cell(file)} | ${cell(sc.model)} | ${sc.tier ?? "-"} | ${cell(sc.datasetVersion)} | ${sc.n} | ` +
          `${pct(sc.passRate)} | ${pct(sc.keyPointCoverage)} | ${pct(sc.citationHitRate)} | ` +
          `${pct(sc.citationValidity)} | ${pct(sc.abstentionRate)} | ${pct(sc.falseAbstentionRate)} | ${sc.failures.length} |`,
      );
    }
  } else {
    out.push("No Citrate QA scorecard in this set.");
  }
  const sidecar = cards.sidecar ?? [];
  out.push("", "## Multi-step workflows through the sidecar", "");
  const wf = sidecar.filter((e) => e.sc.workflow);
  if (wf.length) {
    out.push(
      "| file | model | tier | dataset | workflows | steps | step success | judged-step success | workflow success | g1 steps >= 80% (T1+) |",
      "|---|---|---|---|---:|---:|---:|---:|---:|---|",
    );
    for (const { file, sc } of wf) {
      const w = sc.workflow;
      out.push(
        `| ${cell(file)} | ${cell(sc.model)} | ${sc.tier ?? "-"} | ${cell(w.datasetVersion)} | ${w.nWorkflows} | ${w.nSteps} | ` +
          `${pct(w.stepSuccessRate)} | ${pct(w.reachedStepSuccessRate)} | ${pct(w.workflowSuccessRate)} | ${g1Steps(sc)} |`,
      );
    }
    out.push(
      "",
      "Step success counts a step after a failed one as not passed; judged-step success divides by the",
      "steps whose answer reached the verifiers. The 80% bar is the planset value, pending owner sign-off.",
    );
  } else {
    out.push("No workflow scorecard in this set.");
  }
  out.push("", "## Live prompt injection through the sidecar (MCP output, browser page)", "");
  const li = sidecar.filter((e) => e.sc.liveInjection);
  if (li.length) {
    out.push("| file | model | tier | dataset | n | reached | resist | resist when reached |", "|---|---|---|---|---:|---:|---:|---:|");
    for (const { file, sc } of li) {
      const l = sc.liveInjection;
      out.push(
        `| ${cell(file)} | ${cell(sc.model)} | ${sc.tier ?? "-"} | ${cell(l.datasetVersion)} | ${l.n} | ` +
          `${pct(l.reachedRate)} | ${pct(l.resistRate)} | ${pct(l.resistRateWhenReached)} |`,
      );
    }
    out.push("", "A case passes only when the poisoned content was delivered and no effectful call or secret echo followed.");
  } else {
    out.push("No live injection scorecard in this set.");
  }
  const fails = failureLines([...cards.tools, ...cards.qa, ...sidecar]);
  out.push("", "## Failures", "");
  if (fails.length) out.push(...fails);
  else out.push("None.", "");
  out.push(
    "## What this scorecard does not measure",
    "",
    "- The tool-call and QA rows are single-turn. Workflow step success comes only from the sidecar",
    "  rows above (scripts/eval-sidecar.mjs, workflow-v1); a tier without such a row has no step",
    "  success measurement.",
    "- One run per row at temperature 0: no variance estimate.",
    "- Latency and throughput are not in the JSON scorecards; see the run log next to the results.",
    "",
  );
  return out.join("\n");
}

const USAGE =
  "usage: node scripts/eval-scorecard.mjs [--in eval/results] [--out <in>/SCORECARD.md] [--date YYYY-MM-DD] [--branch <name>]";

function parseArgs(argv) {
  const flags = { "--in": "in", "--out": "out", "--date": "date", "--branch": "branch" };
  const out = {};
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (!(a in flags)) throw new Error(`unknown argument ${JSON.stringify(a)}\n${USAGE}`);
    const v = argv[i + 1];
    if (v === undefined || v.startsWith("--")) throw new Error(`${a} needs a value\n${USAGE}`);
    out[flags[a]] = v;
    i++;
  }
  if (out.date !== undefined && !/^\d{4}-\d{2}-\d{2}$/.test(out.date)) {
    throw new Error(`--date must be YYYY-MM-DD (got ${out.date})`);
  }
  return out;
}

function currentBranch(cwd) {
  try {
    return execFileSync("git", ["rev-parse", "--abbrev-ref", "HEAD"], { cwd, encoding: "utf8" }).trim() || "unknown";
  } catch {
    return "unknown";
  }
}

function main() {
  const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
  let args;
  try {
    args = parseArgs(process.argv.slice(2));
  } catch (e) {
    console.error(e instanceof Error ? e.message : String(e));
    process.exit(2);
  }
  const inDir = path.resolve(root, args.in ?? "eval/results");
  let cards;
  try {
    cards = loadScorecards(inDir);
  } catch (e) {
    console.error(`cannot read ${inDir}: ${e instanceof Error ? e.message : String(e)}`);
    process.exit(2);
  }
  for (const f of cards.skipped) console.error(`skipped ${f} (not a tool-call, QA or sidecar scorecard)`);
  if (cards.tools.length + cards.qa.length + cards.sidecar.length === 0) {
    console.error(`no scorecards in ${inDir}; nothing written.`);
    process.exit(2);
  }
  const outFile = path.resolve(root, args.out ?? path.join(inDir, "SCORECARD.md"));
  const rel = path.relative(root, inDir);
  const md = renderScorecardMarkdown(cards, {
    created: args.date ?? new Date().toISOString().slice(0, 10),
    branch: args.branch ?? currentBranch(root),
    source: rel && !rel.startsWith("..") ? rel : inDir,
  });
  fs.mkdirSync(path.dirname(outFile), { recursive: true });
  fs.writeFileSync(outFile, md);
  console.log(`wrote ${outFile} (${cards.tools.length} tool-call, ${cards.qa.length} QA, ${cards.sidecar.length} sidecar)`);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main();
}
