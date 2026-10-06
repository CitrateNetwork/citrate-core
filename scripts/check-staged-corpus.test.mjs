// @vitest-environment node
//
// Pre-bundle gate for the knowledge corpus (scripts/check-staged-corpus.mjs).
//
// Every bundle overlay ships `knowledge-corpus/**/*`, and the committed README.md always
// matches that glob, so a build whose corpus was never staged still bundles and ships
// README-only. The check runs right before `tauri build` and refuses unless the directory
// holds the corpus that stage-knowledge-corpus.mjs verified and recorded (same bundle digest,
// same node count), re-verified file by file. The fixture corpus is staged through the real
// stager CLI, then broken one way at a time. Exit contract: 0 ok, 1 refused, 2 usage.
import { describe, expect, it, beforeEach, afterEach } from "vitest";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { computeDigest, listFiles, tenantFile } from "./stage-knowledge-corpus.mjs";
import { recordPath } from "./check-staged-corpus.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const STAGER = path.join(here, "stage-knowledge-corpus.mjs");
const CHECK = path.join(here, "check-staged-corpus.mjs");
const FIXTURE = path.resolve(here, "../src-tauri/tests/fixtures/knowledge-corpus");

let tmp;
let corpus;
let bge;
let dest;

function freshCorpus() {
  const dir = path.join(tmp, `corpus-${Math.random().toString(16).slice(2)}`);
  for (const f of listFiles(FIXTURE).filter((f) => f !== "README.md" && f !== ".gitattributes")) {
    fs.mkdirSync(path.dirname(path.join(dir, f)), { recursive: true });
    fs.copyFileSync(path.join(FIXTURE, f), path.join(dir, f));
  }
  return dir;
}

function reseal(dir, edit) {
  const p = path.join(dir, "manifest.json");
  const m = JSON.parse(fs.readFileSync(p, "utf8"));
  edit(m);
  m.bundle_digest = "";
  m.bundle_digest = computeDigest(JSON.stringify(m, null, 2) + "\n");
  fs.writeFileSync(p, JSON.stringify(m, null, 2) + "\n");
}

function fakeBge(dim = 4) {
  const dir = path.join(tmp, "bge");
  fs.mkdirSync(dir, { recursive: true });
  fs.writeFileSync(path.join(dir, "config.json"), JSON.stringify({ hidden_size: dim }));
  fs.writeFileSync(path.join(dir, "tokenizer.json"), "{}");
  fs.writeFileSync(path.join(dir, "model.safetensors"), "weights-v1");
  return { dir, sha: createHash("sha256").update("weights-v1").digest("hex") };
}

function embedAll(dir, b, dim = 4) {
  const m = JSON.parse(fs.readFileSync(path.join(dir, "manifest.json"), "utf8"));
  const entries = {};
  for (const t of m.tenants) {
    const buf = Buffer.alloc(t.nodes * dim * 2, 0x3c);
    const file = `tenants/${t.tenant}.vectors.f16`;
    fs.writeFileSync(path.join(dir, file), buf);
    entries[t.tenant] = { file, sha256: createHash("sha256").update(buf).digest("hex"), encoding: "f16le", model: "bge-base-en-v1.5", dim, weights_sha256: b.sha };
  }
  reseal(dir, (mm) => {
    for (const t of mm.tenants) t.vectors = entries[t.tenant];
  });
}

/** A destination that looks like the repo's: only the committed README. */
function readmeOnlyDest() {
  const d = path.join(tmp, "knowledge-corpus");
  fs.mkdirSync(d, { recursive: true });
  fs.writeFileSync(path.join(d, "README.md"), "committed README");
  return d;
}

const stage = (...args) => spawnSync(process.execPath, [STAGER, ...args], { encoding: "utf8" });
const check = (...args) => spawnSync(process.execPath, [CHECK, ...args], { encoding: "utf8" });

function tarball(dir) {
  const parent = path.join(tmp, `pack-${Math.random().toString(16).slice(2)}`);
  fs.mkdirSync(parent);
  fs.cpSync(dir, path.join(parent, "knowledge-corpus"), { recursive: true });
  const tgz = path.join(parent, "knowledge-corpus.tar.gz");
  expect(spawnSync("tar", ["-czf", tgz, "-C", parent, "knowledge-corpus"]).status).toBe(0);
  return { tgz, sha: createHash("sha256").update(fs.readFileSync(tgz)).digest("hex") };
}

beforeEach(() => {
  tmp = fs.mkdtempSync(path.join(os.tmpdir(), "check-corpus-"));
  corpus = freshCorpus();
  bge = fakeBge();
  embedAll(corpus, bge);
  dest = readmeOnlyDest();
});
afterEach(() => fs.rmSync(tmp, { recursive: true, force: true }));

describe("check-staged-corpus: a README-only corpus directory never reaches the bundle", () => {
  it("refuses the committed README alone (the fail-open case: the glob still matches)", () => {
    const r = check("--dir", dest);
    expect(r.status).toBe(1);
    expect(r.stderr).toMatch(/no staged corpus/);
  });

  it("refuses a missing directory", () => {
    const r = check("--dir", path.join(tmp, "nope"));
    expect(r.status).toBe(1);
  });

  it("accepts the corpus the stager staged and recorded, and reports its digest and node count", () => {
    const s = stage(corpus, "--dest", dest, "--bge-dir", bge.dir);
    expect(s.status, s.stderr).toBe(0);
    const rec = JSON.parse(fs.readFileSync(recordPath(dest), "utf8"));
    const m = JSON.parse(fs.readFileSync(path.join(corpus, "manifest.json"), "utf8"));
    expect(rec.bundle_digest).toBe(m.bundle_digest);
    expect(rec.nodes).toBe(m.tenants.reduce((n, t) => n + t.nodes, 0));
    const r = check("--dir", dest);
    expect(r.status, r.stderr).toBe(0);
    expect(r.stdout).toContain(m.bundle_digest);
    expect(r.stdout).toContain(`${rec.nodes} nodes`);
  });

  it("does not leave the record inside the bundled directory (the importer accepts only corpus files)", () => {
    expect(stage(corpus, "--dest", dest, "--bge-dir", bge.dir).status).toBe(0);
    expect(fs.readdirSync(dest).sort()).toEqual(["NOTICE.md", "README.md", "manifest.json", "skills.lock", "tenants"].filter((f) => fs.existsSync(path.join(dest, f))));
    expect(path.dirname(recordPath(dest))).not.toBe(path.resolve(dest));
  });

  it("refuses a corpus copied in by hand (no stager record)", () => {
    for (const f of listFiles(corpus)) {
      fs.mkdirSync(path.dirname(path.join(dest, f)), { recursive: true });
      fs.copyFileSync(path.join(corpus, f), path.join(dest, f));
    }
    const r = check("--dir", dest);
    expect(r.status).toBe(1);
    expect(r.stderr).toMatch(/not staged by stage-knowledge-corpus/);
  });

  it("refuses when the directory holds a different corpus than the one recorded", () => {
    expect(stage(corpus, "--dest", dest, "--bge-dir", bge.dir).status).toBe(0);
    const other = freshCorpus();
    embedAll(other, bge);
    reseal(other, (m) => {
      m.name = "another-corpus";
    });
    for (const f of listFiles(other)) fs.copyFileSync(path.join(other, f), path.join(dest, f));
    const r = check("--dir", dest);
    expect(r.status).toBe(1);
    expect(r.stderr).toMatch(/bundle digest/);
  });

  it("refuses when the recorded node count differs", () => {
    expect(stage(corpus, "--dest", dest, "--bge-dir", bge.dir).status).toBe(0);
    const p = recordPath(dest);
    const rec = JSON.parse(fs.readFileSync(p, "utf8"));
    rec.nodes += 1;
    fs.writeFileSync(p, JSON.stringify(rec));
    const r = check("--dir", dest);
    expect(r.status).toBe(1);
    expect(r.stderr).toMatch(/node count/);
  });

  it("refuses a staged tenant file changed after staging", () => {
    expect(stage(corpus, "--dest", dest, "--bge-dir", bge.dir).status).toBe(0);
    fs.appendFileSync(path.join(dest, tenantFile("skills")), " ");
    const r = check("--dir", dest);
    expect(r.status).toBe(1);
    expect(r.stderr).toMatch(/does not match its manifest hash/);
  });

  it("refuses a dev-staged corpus (dirty sources or no vectors) unless --allow-dev", () => {
    const dev = freshCorpus();
    expect(stage(dev, "--dest", dest, "--bge-dir", bge.dir, "--allow-unembedded").status).toBe(0);
    const r = check("--dir", dest);
    expect(r.status).toBe(1);
    expect(r.stderr).toMatch(/dev build/);
    expect(check("--dir", dest, "--allow-dev").status).toBe(0);
  });

  it("with --pins, accepts only a corpus staged from the pinned knowledge-corpus.tar.gz", () => {
    const { tgz, sha } = tarball(corpus);
    expect(stage(tgz, "--dest", dest, "--bge-dir", bge.dir).status).toBe(0);
    const pins = path.join(tmp, "runtime-deps.sha256");

    fs.writeFileSync(pins, `# pins\n${sha}  knowledge-corpus.tar.gz\n`);
    const ok = check("--dir", dest, "--pins", pins);
    expect(ok.status, ok.stderr).toBe(0);

    fs.writeFileSync(pins, `${"0".repeat(64)}  knowledge-corpus.tar.gz\n`);
    const wrong = check("--dir", dest, "--pins", pins);
    expect(wrong.status).toBe(1);
    expect(wrong.stderr).toMatch(/pinned/);

    fs.writeFileSync(pins, `# no corpus pin\n`);
    const none = check("--dir", dest, "--pins", pins);
    expect(none.status).toBe(1);
    expect(none.stderr).toMatch(/pinned/);
  });

  it("with --pins, refuses a corpus staged from a directory (no asset digest to compare)", () => {
    expect(stage(corpus, "--dest", dest, "--bge-dir", bge.dir).status).toBe(0);
    const pins = path.join(tmp, "runtime-deps.sha256");
    fs.writeFileSync(pins, `${"a".repeat(64)}  knowledge-corpus.tar.gz\n`);
    const r = check("--dir", dest, "--pins", pins);
    expect(r.status).toBe(1);
  });

  it("exits 2 on usage errors", () => {
    expect(check("--dir").status).toBe(2);
    expect(check("--surprise").status).toBe(2);
  });
});
