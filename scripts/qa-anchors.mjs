#!/usr/bin/env node
// =====================================================================
// citrate-core — Citrate QA eval set: anchor index build + provenance check (HUP-S3.5)
//
//   node scripts/qa-anchors.mjs [--check | --write] [--sources-root <dir>]
//
// For every file src/agent/eval/qa-v1.json cites, reads it from the LOCAL clone of its public
// source repo at the pinned commit (`git show <commit>:<path>`; nothing is fetched), records the
// git blob id and every heading anchor, and checks that each answer key point appears in the text
// of the section it cites. --write regenerates src/agent/eval/qa-v1.anchors.json; --check (the
// default) fails when the committed index differs or a key point is not found in its source.
//
// Source repos are looked up as <sources-root>/<source name> (default: the parent directory of
// this repo, i.e. the citrate-labs workspace; or $QA_SOURCES_ROOT). Requires Node >= 22.18 / 23.6
// (built-in TypeScript type stripping, same as scripts/eval-qa.mjs).
// =====================================================================
import { execFileSync } from "node:child_process";
import { existsSync } from "node:fs";
import { readFile, writeFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { extractAnchors, extractSection, parseQaDataset, ungroundedKeyPoints } from "../src/agent/eval/qa.ts";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const DATASET = "src/agent/eval/qa-v1.json";
const INDEX = "src/agent/eval/qa-v1.anchors.json";
const USAGE = "usage: node scripts/qa-anchors.mjs [--check | --write] [--sources-root <dir>]";

function parseArgs(argv) {
  let mode = "check";
  let sourcesRoot = process.env.QA_SOURCES_ROOT ?? resolve(ROOT, "..");
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === "--check" || a === "--write") mode = a.slice(2);
    else if (a === "--sources-root" && argv[i + 1]) sourcesRoot = resolve(argv[++i]);
    else throw new Error(`unknown argument ${JSON.stringify(a)}\n${USAGE}`);
  }
  return { mode, sourcesRoot };
}

function git(dir, args) {
  return execFileSync("git", ["-C", dir, ...args], { encoding: "utf8", maxBuffer: 1 << 26 });
}

async function main() {
  let args;
  try {
    args = parseArgs(process.argv.slice(2));
  } catch (e) {
    console.error(e.message);
    process.exit(2);
  }
  const ds = parseQaDataset(JSON.parse(await readFile(join(ROOT, DATASET), "utf8")));

  const index = { version: ds.version, sources: {} };
  const texts = new Map();
  for (const [name, src] of Object.entries(ds.sources)) {
    const dir = join(args.sourcesRoot, name);
    if (!existsSync(dir)) {
      console.error(`source repo ${name} not found at ${dir} (pass --sources-root or set QA_SOURCES_ROOT)`);
      process.exit(2);
    }
    try {
      git(dir, ["cat-file", "-e", `${src.commit}^{commit}`]);
    } catch {
      console.error(`${name}: pinned commit ${src.commit} is not in ${dir}; fetch it first`);
      process.exit(2);
    }
    index.sources[name] = { repo: src.repo, commit: src.commit, files: {} };
  }
  const paths = [...new Set(ds.items.flatMap((i) => i.citations.map((c) => `${c.source}\u0000${c.path}`)))].sort();
  for (const key of paths) {
    const [source, path] = key.split("\u0000");
    const dir = join(args.sourcesRoot, source);
    const commit = ds.sources[source].commit;
    let text;
    try {
      text = git(dir, ["show", `${commit}:${path}`]);
    } catch {
      console.error(`${source}:${path} does not exist at ${commit}`);
      process.exit(1);
    }
    texts.set(key, text);
    index.sources[source].files[path] = { blob: git(dir, ["rev-parse", `${commit}:${path}`]).trim(), anchors: extractAnchors(text) };
  }

  const problems = [];
  for (const it of ds.items) {
    const parts = [];
    for (const c of it.citations) {
      const sec = extractSection(texts.get(`${c.source}\u0000${c.path}`) ?? "", c.anchor);
      if (sec === null) problems.push(`${it.id}: ${c.source}:${c.path}#${c.anchor} has no such heading`);
      else parts.push(sec);
    }
    if (it.answerable) {
      for (const kp of ungroundedKeyPoints(it, parts.join("\n"))) {
        problems.push(`${it.id}: key point [${kp.join(" | ")}] not found in its cited section(s)`);
      }
    }
  }

  const rendered = JSON.stringify(index, null, 2) + "\n";
  if (args.mode === "write") {
    await writeFile(join(ROOT, INDEX), rendered);
    console.log(`wrote ${INDEX}: ${paths.length} files across ${Object.keys(index.sources).length} sources`);
  } else {
    let committed = "";
    try {
      committed = await readFile(join(ROOT, INDEX), "utf8");
    } catch {
      problems.push(`${INDEX} is missing; run with --write`);
    }
    if (committed && committed !== rendered) problems.push(`${INDEX} is stale; run with --write and review the diff`);
  }
  if (problems.length) {
    console.error(problems.join("\n"));
    console.error(`${problems.length} problem(s)`);
    process.exit(1);
  }
  console.log(`qa-v1 provenance OK: ${ds.items.length} items, ${paths.length} cited files, every key point grounded`);
}

main();
