#!/usr/bin/env node
// =====================================================================
// citrate-core: stage the Hermes knowledge corpus into the app bundle (HUP-S3.1)
//
//   node scripts/stage-knowledge-corpus.mjs <corpus-dir | knowledge-corpus.tar.gz>
//        --bge-dir src-tauri/models/bge-base-en-v1.5 [--allow-unembedded] [--allow-dirty]
//        [--dest src-tauri/knowledge-corpus] [--mem-mcp <bundled mem-mcp binary>]
//
// The corpus is built in citrate-memories (`scripts/build-corpus.sh`, format citrate-corpus/2)
// and shipped as the app resource `knowledge-corpus/`, which the first-run import
// (src-tauri/src/knowledge_import.rs) hands to `mem-mcp import-corpus`. This script is the
// release-side gate before the corpus is bundled and signed:
//
//   - the corpus is exactly manifest.json, NOTICE.md, skills.lock (when the manifest pins one),
//     tenants/<tenant>.corpus.json for each manifest tenant and, when the release embedded the
//     corpus, tenants/<tenant>.vectors.f16 (sha256 and size checked): nothing else, no symlinks;
//   - manifest.format is citrate-corpus/2, its bundle_digest re-hashes (same canonical JSON as
//     mem_corpus::manifest::Manifest::compute_digest), every tenant file and the skills.lock
//     match their manifest sha256;
//   - with --mem-mcp, the bundled memory binary implements `import-corpus` (an older mem-mcp
//     would take the arguments as a store path and never answer the import);
//   - the corpus depends on the bundled BGE embedder (--bge-dir, required). Without
//     config.json, tokenizer.json and model.safetensors the first-run import is skipped as
//     `not-semantic`, so the corpus would ship as dead weight. Every tenant must carry vectors made
//     with exactly those weights (same sha256, model id and dimension): otherwise the importer
//     ignores them and each member embeds about 21k nodes on their own CPU (hours). A dev build may
//     stage an unembedded corpus with --allow-unembedded; it is warned, never silent.
//   - every included source records a clean commit. The builder suffixes a commit with `-dirty`
//     when the source's work tree had local changes, and records `unpinned` for a source outside
//     any git work tree (mem_corpus build.rs resolve_git); either way the
//     corpus text and its NOTICE.md cannot be reproduced from that commit (HUP g3-licence,
//     L-sizelicence). A release corpus is built from clean checkouts; a dev build may pass
//     --allow-dirty, which is warned, never silent.
//
// On success the destination keeps its README.md and receives the verified files; anything else
// in it is replaced. The stager then writes a record beside the destination (`<dest>.staged.json`:
// bundle digest, node count, the input asset's sha256, any dev-build allowances), which
// scripts/check-staged-corpus.mjs compares against the directory right before bundling, so a
// build whose corpus was never staged (README only) or was changed afterwards fails closed. The importer re-verifies everything on the member's machine; this gate
// stops a wrong corpus from being signed into a release. Exit 0 staged, 1 refused, 2 usage.
// No dependencies beyond Node's standard library (and `tar` for a .tar.gz input).
// =====================================================================
import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

export const CORPUS_FORMAT = "citrate-corpus/2";
export const KNOWLEDGE_TENANTS = ["citrate-docs", "skills", "refs", "methodology"];
const USAGE =
  "usage: node scripts/stage-knowledge-corpus.mjs <corpus-dir | corpus.tar.gz> --bge-dir <bundled bge model dir> " +
  "[--allow-unembedded] [--allow-dirty] [--dest <dir>] [--mem-mcp <binary>]";

/** `Embedder::model_id` of the app's BGE embedder (citrate-memories mem-index DEFAULT_MODEL_ID). */
export const APP_EMBED_MODEL = "bge-base-en-v1.5";
/** Files the memory daemon loads from CITRATE_BGE_MODEL_DIR (mem-index transformer.rs). */
export const BGE_FILES = ["config.json", "tokenizer.json", "model.safetensors"];
/** CPU embedding rate measured on an Apple M2 Max (2026-10-01 corpus builds: 2.7 to 3.7 nodes/s), rounded. */
const CPU_NODES_PER_SECOND = 3;

const sha256 = (buf) => createHash("sha256").update(buf).digest("hex");

/** Tenant file path inside a corpus directory (mem_corpus::tenant_file). */
export const tenantFile = (tenant) => `tenants/${tenant}.corpus.json`;

/**
 * Recompute a manifest's bundle_digest: sha256 over its compact JSON with bundle_digest "".
 * `manifestText` must be the file as written (its key order is the Rust struct's order).
 */
export function computeDigest(manifestText) {
  const m = JSON.parse(manifestText);
  m.bundle_digest = "";
  return sha256(Buffer.from(JSON.stringify(m), "utf8"));
}

/** Every regular file under `dir` as a sorted list of "/"-separated relative paths. Throws on a symlink. */
export function listFiles(dir) {
  const out = [];
  const walk = (rel) => {
    for (const e of fs.readdirSync(path.join(dir, rel), { withFileTypes: true })) {
      const r = rel ? `${rel}/${e.name}` : e.name;
      if (e.isSymbolicLink()) throw new Error(`${r} is a symlink`);
      if (e.isDirectory()) walk(r);
      else if (e.isFile()) out.push(r);
      else throw new Error(`${r} is not a regular file`);
    }
  };
  walk("");
  return out.sort();
}

/**
 * Verify a corpus directory. Returns { manifest, files, bytes } or throws with the reason.
 * `ignore` names top-level files that may sit beside the corpus (the committed README.md).
 */
export function verifyCorpus(dir, { ignore = [] } = {}) {
  const text = fs.readFileSync(path.join(dir, "manifest.json"), "utf8");
  let m;
  try {
    m = JSON.parse(text);
  } catch (e) {
    throw new Error(`manifest.json is not JSON: ${e instanceof Error ? e.message : String(e)}`);
  }
  if (m.format !== CORPUS_FORMAT) throw new Error(`unsupported corpus format ${JSON.stringify(m.format)} (expected ${CORPUS_FORMAT})`);
  if (!/^[0-9a-f]{64}$/.test(m.bundle_digest ?? "")) throw new Error("manifest has no valid bundle_digest");
  if (computeDigest(text) !== m.bundle_digest) throw new Error("manifest digest does not match its contents");
  if (!Array.isArray(m.tenants) || m.tenants.length === 0) throw new Error("manifest lists no tenants");

  const expected = new Set(["manifest.json", "NOTICE.md"]);
  if (m.skills_lock_sha256 != null) {
    expected.add("skills.lock");
    const lock = fs.readFileSync(path.join(dir, "skills.lock"));
    if (sha256(lock) !== m.skills_lock_sha256) throw new Error("skills.lock does not match the manifest");
  }
  const seen = new Set();
  for (const t of m.tenants) {
    if (!KNOWLEDGE_TENANTS.includes(t.tenant)) throw new Error(`tenant ${JSON.stringify(t.tenant)} is not a knowledge tenant`);
    if (seen.has(t.tenant)) throw new Error(`tenant ${t.tenant} listed twice`);
    seen.add(t.tenant);
    if (t.file !== tenantFile(t.tenant)) throw new Error(`tenant ${t.tenant} file must be ${tenantFile(t.tenant)}`);
    const bytes = fs.readFileSync(path.join(dir, t.file));
    if (sha256(bytes) !== t.sha256) throw new Error(`${t.file} does not match its manifest hash`);
    expected.add(t.file);
    if (t.vectors != null) expected.add(verifyVectors(dir, t));
  }
  const files = listFiles(dir).filter((f) => !ignore.includes(f));
  const extra = files.filter((f) => !expected.has(f));
  if (extra.length) throw new Error(`unexpected file(s) in the corpus: ${extra.join(", ")}`);
  const missing = [...expected].filter((f) => !files.includes(f));
  if (missing.length) throw new Error(`missing file(s): ${missing.join(", ")}`);
  const bytes = files.reduce((n, f) => n + fs.statSync(path.join(dir, f)).size, 0);
  return { manifest: m, files, bytes };
}

/** Vectors file path inside a corpus directory (mem_corpus::vectors::vectors_file). */
export const vectorsFile = (tenant) => `tenants/${tenant}.vectors.f16`;

/** Check a tenant's precomputed vectors (mem_corpus::vectors) against its manifest entry; returns the file path. */
function verifyVectors(dir, t) {
  const v = t.vectors;
  if (v.file !== vectorsFile(t.tenant)) throw new Error(`tenant ${t.tenant} vectors file must be ${vectorsFile(t.tenant)}`);
  if (v.encoding !== "f16le") throw new Error(`tenant ${t.tenant} vectors encoding ${JSON.stringify(v.encoding)} is not supported`);
  if (!Number.isInteger(v.dim) || v.dim < 1 || v.dim > 4096) throw new Error(`tenant ${t.tenant} vectors dimension ${v.dim} is not supported`);
  const bytes = fs.readFileSync(path.join(dir, v.file));
  if (sha256(bytes) !== v.sha256) throw new Error(`${v.file} does not match its manifest hash`);
  const want = t.nodes * v.dim * 2;
  if (bytes.length !== want) throw new Error(`${v.file} holds ${bytes.length} bytes, expected ${want}`);
  return v.file;
}

/**
 * Check the corpus against the BGE model the release bundles. Returns what the importer will reuse;
 * throws when the import would be skipped (no or partial model) or the vectors would be ignored.
 */
export function checkEmbedder(manifest, bgeDir, { allowUnembedded = false } = {}) {
  for (const f of BGE_FILES) {
    let st;
    try {
      st = fs.lstatSync(path.join(bgeDir, f));
    } catch {
      st = null;
    }
    if (!st || !st.isFile() || st.size === 0) {
      throw new Error(
        `the bundled BGE model at ${bgeDir} has no ${f}; without it the first-run import is skipped as not-semantic, so the corpus would ship unused`,
      );
    }
  }
  let dim;
  try {
    dim = JSON.parse(fs.readFileSync(path.join(bgeDir, "config.json"), "utf8")).hidden_size;
  } catch (e) {
    throw new Error(`the bundled BGE config.json is not JSON: ${e instanceof Error ? e.message : String(e)}`);
  }
  if (!Number.isInteger(dim) || dim < 1) throw new Error("the bundled BGE config.json has no hidden_size");
  const weightsSha256 = sha256(fs.readFileSync(path.join(bgeDir, "model.safetensors")));
  let embeddedNodes = 0;
  const unembedded = [];
  for (const t of manifest.tenants) {
    const v = t.vectors;
    if (v == null) {
      unembedded.push(t);
      continue;
    }
    if (v.model !== APP_EMBED_MODEL) throw new Error(`tenant ${t.tenant} vectors come from ${JSON.stringify(v.model)}, not the app's ${APP_EMBED_MODEL}`);
    if (v.dim !== dim) throw new Error(`tenant ${t.tenant} vectors have dimension ${v.dim}; the bundled model has ${dim}`);
    if (v.weights_sha256 !== weightsSha256) {
      throw new Error(
        `tenant ${t.tenant} vectors were made with other model weights (${v.weights_sha256.slice(0, 12)}, bundled ${weightsSha256.slice(0, 12)}); ` +
          `the importer would ignore them and embed ${t.nodes} nodes on each member's CPU`,
      );
    }
    embeddedNodes += t.nodes;
  }
  if (unembedded.length && !allowUnembedded) {
    const n = unembedded.reduce((a, t) => a + t.nodes, 0);
    throw new Error(
      `${unembedded.map((t) => t.tenant).join(", ")}: no precomputed vectors; each member's first launch would embed ${n} nodes on their own CPU ` +
        `(about ${Math.ceil(n / CPU_NODES_PER_SECOND / 60)} minutes on an Apple M2 Max). Build the corpus with EMBED_BGE_DIR, or pass --allow-unembedded for a dev build`,
    );
  }
  return { model: APP_EMBED_MODEL, dim, weightsSha256, embeddedNodes };
}

/** Suffix mem-corpus appends to a source commit built from a work tree with local changes. */
export const DIRTY_SUFFIX = "-dirty";

/** What mem-corpus records for a source that is not in a git work tree (resolve_git). */
export const UNPINNED = "unpinned";

/** A clean commit: a bare hex object id, nothing appended. */
const CLEAN_COMMIT = /^[0-9a-f]{7,64}$/;

/** Why a recorded commit is not clean, worded to follow "was" (the refusal and the warning). */
export function dirtyReason(commit) {
  if (typeof commit === "string" && commit.endsWith(DIRTY_SUFFIX)) return "built from a work tree with local changes";
  if (commit === UNPINNED) return "not built from a git checkout";
  return "recorded without a clean commit";
}

/**
 * Included sources whose recorded commit is not a clean commit (`<sha>-dirty`, `unpinned`, or
 * anything else that is not a bare sha), as [{ id, commit }]. Throws listing them unless
 * `allowDirty`; with `allowDirty` returns them so the caller can warn.
 */
export function checkSourceCommits(manifest, { allowDirty = false } = {}) {
  if (!Array.isArray(manifest.sources)) throw new Error("manifest lists no sources");
  const dirty = manifest.sources
    .filter((s) => s.included === true && !(typeof s.commit === "string" && CLEAN_COMMIT.test(s.commit)))
    .map((s) => ({ id: s.id, commit: s.commit }));
  if (dirty.length && !allowDirty) {
    const groups = new Map();
    for (const d of dirty) {
      const why = dirtyReason(d.commit);
      if (!groups.has(why)) groups.set(why, []);
      groups.get(why).push(`${d.id} @ ${d.commit}`);
    }
    throw new Error(
      `${[...groups].map(([why, list]) => `${list.join(", ")}: ${why}`).join("; ")}, so this corpus and its NOTICE ` +
        `cannot be reproduced from the recorded commit. Rebuild from clean checkouts, or pass --allow-dirty for a dev build`,
    );
  }
  return dirty;
}

/** True when a mem-mcp binary carries the `import-corpus` subcommand (its usage string). */
export function memMcpSupportsImport(binPath) {
  return fs.readFileSync(binPath).includes(Buffer.from("mem-mcp import-corpus <store-path> <corpus-dir>"));
}

/** Where the stager records what it staged into `dest`: beside it, never inside the bundled directory. */
export const recordPath = (dest) => `${path.resolve(dest).replace(/[\\/]+$/, "")}.staged.json`;

/** Format of the stager record read by scripts/check-staged-corpus.mjs. */
export const RECORD_FORMAT = "citrate-corpus-staged/1";

/** Replace everything in `dest` except README.md with the verified corpus files from `src`. */
export function stageInto(src, dest, files) {
  fs.mkdirSync(dest, { recursive: true });
  for (const e of fs.readdirSync(dest)) {
    if (e !== "README.md") fs.rmSync(path.join(dest, e), { recursive: true, force: true });
  }
  for (const f of files) {
    fs.mkdirSync(path.dirname(path.join(dest, f)), { recursive: true });
    fs.copyFileSync(path.join(src, f), path.join(dest, f));
  }
}

function parseArgs(argv, root) {
  const out = {
    dest: path.join(root, "src-tauri", "knowledge-corpus"),
    memMcp: undefined,
    bgeDir: undefined,
    allowUnembedded: false,
    allowDirty: false,
    input: undefined,
  };
  const valueFlags = { "--dest": "dest", "--mem-mcp": "memMcp", "--bge-dir": "bgeDir" };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a in valueFlags) {
      const v = argv[i + 1];
      if (v === undefined || v.startsWith("--")) throw new Error(`${a} needs a value\n${USAGE}`);
      out[valueFlags[a]] = v;
      i++;
    } else if (a === "--allow-unembedded") out.allowUnembedded = true;
    else if (a === "--allow-dirty") out.allowDirty = true;
    else if (a.startsWith("--")) throw new Error(`unknown argument ${JSON.stringify(a)}\n${USAGE}`);
    else if (out.input === undefined) out.input = a;
    else throw new Error(`one corpus input only\n${USAGE}`);
  }
  if (!out.input) throw new Error(USAGE);
  if (!out.bgeDir) throw new Error(`--bge-dir is required: the corpus imports only beside the bundled BGE model\n${USAGE}`);
  return out;
}

/** Locate the corpus root (the directory holding manifest.json) inside an extracted tarball. */
function findCorpusRoot(dir) {
  if (fs.existsSync(path.join(dir, "manifest.json"))) return dir;
  const subs = fs.readdirSync(dir, { withFileTypes: true }).filter((e) => e.isDirectory());
  if (subs.length === 1 && fs.existsSync(path.join(dir, subs[0].name, "manifest.json"))) return path.join(dir, subs[0].name);
  throw new Error("no manifest.json at the top of the corpus archive");
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
  let tmp;
  try {
    let src = path.resolve(args.input);
    let inputSha256 = null;
    if (src.endsWith(".tar.gz") || src.endsWith(".tgz")) {
      inputSha256 = sha256(fs.readFileSync(src));
      tmp = fs.mkdtempSync(path.join(os.tmpdir(), "knowledge-corpus-"));
      execFileSync("tar", ["-xzf", src, "-C", tmp]);
      src = findCorpusRoot(tmp);
    }
    const { manifest, files, bytes } = verifyCorpus(src);
    const dirty = checkSourceCommits(manifest, { allowDirty: args.allowDirty });
    const emb = checkEmbedder(manifest, path.resolve(args.bgeDir), { allowUnembedded: args.allowUnembedded });
    if (args.memMcp && !memMcpSupportsImport(args.memMcp)) {
      throw new Error(`${args.memMcp} predates \`mem-mcp import-corpus\`; stage a mem-mcp built from citrate-memories with mem-corpus`);
    }
    const record = recordPath(args.dest);
    fs.rmSync(record, { force: true });
    stageInto(src, path.resolve(args.dest), files);
    const nodes = manifest.tenants.reduce((n, t) => n + t.nodes, 0);
    fs.writeFileSync(
      record,
      JSON.stringify(
        {
          format: RECORD_FORMAT,
          bundle_digest: manifest.bundle_digest,
          nodes,
          tenants: Object.fromEntries(manifest.tenants.map((t) => [t.tenant, t.nodes])),
          files: files.length,
          bytes,
          input: path.basename(path.resolve(args.input)),
          input_sha256: inputSha256,
          dirty_sources: dirty,
          unembedded_nodes: nodes - emb.embeddedNodes,
        },
        null,
        2,
      ) + "\n",
    );
    for (const d of dirty) {
      console.error(`warning: source ${d.id} was ${dirtyReason(d.commit)} (${d.commit}); not reproducible, dev build only`);
    }
    const tenants = manifest.tenants.map((t) => `${t.tenant} ${t.nodes} nodes`).join(", ");
    console.log(`staged knowledge corpus ${manifest.bundle_digest} (${files.length} files, ${bytes} bytes; ${tenants}) -> ${args.dest}`);
    const total = manifest.tenants.reduce((n, t) => n + t.nodes, 0);
    if (emb.embeddedNodes === total) {
      console.log(`vectors for every node match the bundled ${emb.model} (dim ${emb.dim}, weights ${emb.weightsSha256.slice(0, 12)})`);
    } else {
      console.error(
        `warning: ${total - emb.embeddedNodes} of ${total} nodes have no precomputed vectors; the first-run import will embed them on the member's CPU`,
      );
    }
  } catch (e) {
    console.error(`knowledge corpus refused: ${e instanceof Error ? e.message : String(e)}`);
    process.exit(1);
  } finally {
    if (tmp) fs.rmSync(tmp, { recursive: true, force: true });
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main();
}
