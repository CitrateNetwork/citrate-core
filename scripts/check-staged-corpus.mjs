#!/usr/bin/env node
// =====================================================================
// citrate-core: pre-bundle check that the Hermes knowledge corpus is staged (fail closed)
//
//   node scripts/check-staged-corpus.mjs [--dir src-tauri/knowledge-corpus]
//        [--pins src-tauri/runtime-deps.sha256] [--allow-dev]
//
// Every bundle overlay ships the resource glob `knowledge-corpus/**/*`, and the committed
// README.md always matches it, so a build whose corpus was never staged still bundles and
// ships README-only (first run reports `skipped: no-bundle`). Run this right before
// `tauri build` on every platform. It refuses unless:
//
//   - the directory holds a corpus (manifest.json), not just the README;
//   - scripts/stage-knowledge-corpus.mjs staged it: its record `<dir>.staged.json` exists, and
//     the corpus's bundle digest and node count equal what the stager recorded;
//   - the corpus re-verifies file by file (stage-knowledge-corpus.mjs verifyCorpus: digest,
//     tenant and vectors hashes, exact file set);
//   - it is a release corpus: every included source at a clean commit and every node carrying
//     precomputed vectors (a corpus staged with --allow-dirty / --allow-unembedded needs
//     --allow-dev, for dev builds only);
//   - with --pins, it was staged from the `knowledge-corpus.tar.gz` whose sha256 is pinned in
//     that manifest (src-tauri/runtime-deps.sha256).
//
// Exit 0 ok, 1 refused, 2 usage. Node standard library only.
// =====================================================================
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { RECORD_FORMAT, checkSourceCommits, recordPath, verifyCorpus } from "./stage-knowledge-corpus.mjs";

export { recordPath };

export const CORPUS_ASSET = "knowledge-corpus.tar.gz";
const USAGE = "usage: node scripts/check-staged-corpus.mjs [--dir <staged corpus dir>] [--pins <runtime-deps.sha256>] [--allow-dev]";

/** The sha256 pinned for `asset` in a `shasum -a 256` manifest, or null when it is not pinned exactly once. */
export function pinnedSha256(pinsText, asset = CORPUS_ASSET) {
  const hits = pinsText
    .split("\n")
    .map((l) => l.match(/^([0-9a-f]{64}) {2}(\S+)$/))
    .filter((m) => m && m[2] === asset);
  return hits.length === 1 ? hits[0][1] : null;
}

/** Check the staged corpus in `dir`. Returns { digest, nodes, bytes } or throws with the reason. */
export function checkStaged(dir, { pinsText, allowDev = false } = {}) {
  if (!fs.existsSync(path.join(dir, "manifest.json"))) {
    throw new Error(
      `${dir} holds no staged corpus (no manifest.json); the bundle would ship the README alone. ` +
        `Stage it with scripts/stage-knowledge-corpus.mjs (docs/RELEASE.md section 3)`,
    );
  }
  let rec;
  try {
    rec = JSON.parse(fs.readFileSync(recordPath(dir), "utf8"));
  } catch {
    rec = null;
  }
  if (!rec || rec.format !== RECORD_FORMAT) {
    throw new Error(`${dir} was not staged by stage-knowledge-corpus.mjs (no valid ${path.basename(recordPath(dir))} beside it)`);
  }
  const { manifest, bytes } = verifyCorpus(dir, { ignore: ["README.md"] });
  const nodes = manifest.tenants.reduce((n, t) => n + t.nodes, 0);
  if (manifest.bundle_digest !== rec.bundle_digest) {
    throw new Error(`staged corpus bundle digest ${manifest.bundle_digest} is not the one the stager recorded (${rec.bundle_digest})`);
  }
  if (nodes !== rec.nodes) throw new Error(`staged corpus node count ${nodes} is not the one the stager recorded (${rec.nodes})`);
  if (!allowDev) {
    checkSourceCommits(manifest);
    const unembedded = manifest.tenants.filter((t) => t.vectors == null);
    if (unembedded.length) {
      throw new Error(
        `${unembedded.map((t) => t.tenant).join(", ")}: no precomputed vectors; a corpus staged for a dev build ` +
          `(--allow-unembedded) does not ship. Pass --allow-dev for a dev build`,
      );
    }
  }
  if (pinsText !== undefined) {
    const pin = pinnedSha256(pinsText);
    if (!pin) throw new Error(`${CORPUS_ASSET} is not pinned exactly once in the pins manifest`);
    if (rec.input_sha256 !== pin) {
      throw new Error(
        `the staged corpus came from ${rec.input ?? "?"} (sha256 ${rec.input_sha256 ?? "none: staged from a directory"}), ` +
          `not the pinned ${CORPUS_ASSET} (${pin})`,
      );
    }
  }
  return { digest: manifest.bundle_digest, nodes, bytes };
}

function parseArgs(argv, root) {
  const out = { dir: path.join(root, "src-tauri", "knowledge-corpus"), pins: undefined, allowDev: false };
  const valueFlags = { "--dir": "dir", "--pins": "pins" };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a in valueFlags) {
      const v = argv[i + 1];
      if (v === undefined || v.startsWith("--")) throw new Error(`${a} needs a value\n${USAGE}`);
      out[valueFlags[a]] = v;
      i++;
    } else if (a === "--allow-dev") out.allowDev = true;
    else throw new Error(`unknown argument ${JSON.stringify(a)}\n${USAGE}`);
  }
  return out;
}

function main() {
  const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
  let args;
  try {
    args = parseArgs(process.argv.slice(2), root);
  } catch (e) {
    console.error(e instanceof Error ? e.message : String(e));
    process.exit(2);
  }
  try {
    const dir = path.resolve(args.dir);
    const pinsText = args.pins === undefined ? undefined : fs.readFileSync(args.pins, "utf8");
    const r = checkStaged(dir, { pinsText, allowDev: args.allowDev });
    console.log(`knowledge corpus staged: ${r.digest} (${r.nodes} nodes, ${r.bytes} bytes) in ${dir}`);
  } catch (e) {
    console.error(`knowledge corpus check failed: ${e instanceof Error ? e.message : String(e)}`);
    process.exit(1);
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main();
}
