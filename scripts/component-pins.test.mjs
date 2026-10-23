// @vitest-environment node
//
// release/components.json consistency check (scripts/component-pins.mjs check), the CI tripwire
// that keeps the component pins, the build scripts, the bundle overlays, the runtime-deps pins,
// release.yml and the app's model pin in step. Each case copies the files the check reads into a
// scratch tree, breaks one thing, and expects the check to name it.
import { afterEach, describe, expect, it } from "vitest";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { execFileSync } from "node:child_process";
import {
  check,
  requirementByteErrors,
  requirementRevErrors,
  revContains,
  stagedRequirementErrors,
} from "./component-pins.mjs";

const repo = path.join(path.dirname(fileURLToPath(import.meta.url)), "..");
const FILES = [
  "release/components.json",
  "release/licences.json",
  "src-tauri/runtime-deps.sha256",
  "src-tauri/Cargo.toml",
  "src-tauri/src/model.rs",
  ".github/workflows/release.yml",
  "docs/RELEASE.md",
];

const tmpDirs = [];
afterEach(() => {
  for (const d of tmpDirs.splice(0)) fs.rmSync(d, { recursive: true, force: true });
});

function fixture() {
  // <tmp>/core, so the check's sibling checkouts (<root>/../<checkout>) are only what a test puts there.
  const parent = fs.mkdtempSync(path.join(os.tmpdir(), "component-pins-"));
  tmpDirs.push(parent);
  const root = path.join(parent, "core");
  fs.mkdirSync(root);
  const copy = (rel) => {
    fs.mkdirSync(path.dirname(path.join(root, rel)), { recursive: true });
    fs.copyFileSync(path.join(repo, rel), path.join(root, rel));
  };
  FILES.forEach(copy);
  for (const n of fs.readdirSync(path.join(repo, "src-tauri")).filter((x) => /^tauri.*\.conf\.json$/.test(x))) copy(`src-tauri/${n}`);
  for (const n of fs.readdirSync(path.join(repo, "scripts")).filter((x) => /^build-.*\.sh$/.test(x))) copy(`scripts/${n}`);
  return root;
}
const read = (root, rel) => fs.readFileSync(path.join(root, rel), "utf8");
const write = (root, rel, text) => fs.writeFileSync(path.join(root, rel), text);
const edit = (root, rel, from, to) => {
  const t = read(root, rel);
  if (!t.includes(from)) throw new Error(`fixture: '${from}' not in ${rel}`);
  write(root, rel, t.split(from).join(to));
};
const manifest = (root) => JSON.parse(read(root, "release/components.json"));
const setManifest = (root, m) => write(root, "release/components.json", JSON.stringify(m, null, 2));
const comp = (m, id) => m.components.find((c) => c.id === id);
const errorsOf = (root) => check(root).join("\n");

describe("component-pins check", () => {
  it("passes on this repository", () => {
    expect(check(repo)).toEqual([]);
  });

  it("passes on an unmodified copy (the fixture is faithful)", () => {
    expect(check(fixture())).toEqual([]);
  });

  it("refuses a bundle externalBin with no component", () => {
    const r = fixture();
    const m = manifest(r);
    m.components = m.components.filter((c) => c.id !== "cluster-daemon");
    setManifest(r, m);
    expect(errorsOf(r)).toMatch(/externalBin binaries\/cluster-daemon .* has no component/);
  });

  it("refuses a source rev that is not a full commit", () => {
    const r = fixture();
    const m = manifest(r);
    comp(m, "comms-member-daemon").source.rev = "9cd12c4";
    setManifest(r, m);
    expect(errorsOf(r)).toMatch(/'comms-member-daemon': source.rev must be a full 40-hex commit/);
  });

  it("refuses federation drift that is not explained", () => {
    const r = fixture();
    const m = manifest(r);
    delete comp(m, "hermes-sidecar").federation.note;
    setManifest(r, m);
    expect(errorsOf(r)).toMatch(/'hermes-sidecar': federation drift needs a note/);
  });

  it("refuses a 'match' that does not match", () => {
    const r = fixture();
    const m = manifest(r);
    comp(m, "node-agent").federation.status = "match";
    setManifest(r, m);
    expect(errorsOf(r)).toMatch(/'node-agent': federation.status is match but/);
  });

  it("refuses an artifact whose runtime-deps asset has no pin", () => {
    const r = fixture();
    edit(r, "src-tauri/runtime-deps.sha256", "  hermes-aarch64-apple-darwin-626486b8a7db", "  hermes-aarch64-apple-darwin-renamed");
    expect(errorsOf(r)).toMatch(/runtime-deps asset hermes-aarch64-apple-darwin-626486b8a7db is pinned 0 times/);
  });

  it("refuses an asset pinned twice (ambiguous)", () => {
    const r = fixture();
    const line = read(r, "src-tauri/runtime-deps.sha256").split("\n").find((l) => l.endsWith("  mem-mcp-aarch64-apple-darwin-0e9d48822054"));
    write(r, "src-tauri/runtime-deps.sha256", read(r, "src-tauri/runtime-deps.sha256") + line + "\n");
    expect(errorsOf(r)).toMatch(/mem-mcp-aarch64-apple-darwin-0e9d48822054 is pinned 2 times/);
  });

  it("refuses an artifact with no evidence", () => {
    const r = fixture();
    const m = manifest(r);
    delete comp(m, "kubo").artifacts["aarch64-apple-darwin"].evidence;
    setManifest(r, m);
    expect(errorsOf(r)).toMatch(/'kubo' aarch64-apple-darwin: evidence missing/);
  });

  it("refuses a download whose archive is not pinned", () => {
    const r = fixture();
    const m = manifest(r);
    delete comp(m, "kubo").artifacts["x86_64-unknown-linux-gnu"].archive_sha256;
    setManifest(r, m);
    expect(errorsOf(r)).toMatch(/'kubo' x86_64-unknown-linux-gnu: archive_sha256 missing/);
  });

  it("refuses a component build script that runs cargo itself", () => {
    const r = fixture();
    write(r, "scripts/build-rogue.sh", "#!/usr/bin/env bash\ncd ../citrate-cluster && cargo build --release -p cluster-daemon\n");
    expect(errorsOf(r)).toMatch(/scripts\/build-rogue.sh does not delegate/);
    expect(errorsOf(r)).toMatch(/scripts\/build-rogue.sh runs cargo build itself/);
  });

  it("refuses a script that records the checkout HEAD as provenance", () => {
    const r = fixture();
    edit(r, "scripts/build-cluster-daemon.sh", "set -euo pipefail", 'set -euo pipefail\nREV="$(git -C x rev-parse --short HEAD)"');
    expect(errorsOf(r)).toMatch(/build-cluster-daemon.sh records a checkout HEAD/);
  });

  it("refuses a wrapper that builds a component the manifest does not give it", () => {
    const r = fixture();
    write(r, "scripts/build-mislabeled.sh", '#!/usr/bin/env bash\nexec "$(dirname "$0")/build-component.sh" cluster-daemon "$@"\n');
    expect(errorsOf(r)).toMatch(/build-mislabeled.sh builds 'cluster-daemon' but the manifest names scripts\/build-cluster-daemon.sh/);
  });

  it("refuses release.yml that does not stage a pinned sidecar", () => {
    const r = fixture();
    edit(r, ".github/workflows/release.yml", "              hermes-aarch64-apple-darwin-626486b8a7db\n", "");
    expect(errorsOf(r)).toMatch(/release.yml does not stage 'hermes-sidecar'/);
  });

  it("refuses release.yml that stages an asset the manifest does not name", () => {
    const r = fixture();
    edit(r, ".github/workflows/release.yml", "            knowledge-corpus.tar.gz\n", "            knowledge-corpus.tar.gz\n            gemma-4-E4B-it-Q4_0.gguf\n");
    expect(errorsOf(r)).toMatch(/release.yml stages runtime-deps asset gemma-4-E4B-it-Q4_0.gguf, which release\/components.json does not name/);
  });

  it("refuses release.yml that fetches Kubo without the pinned engine", () => {
    const r = fixture();
    edit(r, ".github/workflows/release.yml", "scripts/build-component.sh kubo --target aarch64-apple-darwin", "cp /tmp/ipfs src-tauri/binaries/ipfs-aarch64-apple-darwin");
    expect(errorsOf(r)).toMatch(/release.yml does not stage 'kubo' from a pinned source/);
  });

  it("refuses a signing or publishing step that runs in a dry run", () => {
    const r = fixture();
    edit(r, ".github/workflows/release.yml", "        if: ${{ env.DRY_RUN != 'true' }}\n        uses: tauri-apps/tauri-action", "        uses: tauri-apps/tauri-action");
    expect(errorsOf(r)).toMatch(/step 'Build · sign · notarize · publish updater artifacts' signs or publishes but is not guarded/);
  });

  it("refuses a dry-run step that reads a signing secret", () => {
    const r = fixture();
    edit(r, ".github/workflows/release.yml", "      - name: Build the bundle unsigned (dry run)\n        if: ${{ env.DRY_RUN == 'true' }}\n", "      - name: Build the bundle unsigned (dry run)\n        if: ${{ env.DRY_RUN == 'true' }}\n        env:\n          K: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}\n");
    expect(errorsOf(r)).toMatch(/dry-run step 'Build the bundle unsigned \(dry run\)' reads secrets.TAURI_SIGNING_PRIVATE_KEY/);
  });

  it("refuses a DRY_RUN that a tag push could set", () => {
    const r = fixture();
    edit(r, ".github/workflows/release.yml", "DRY_RUN: ${{ github.event_name == 'workflow_dispatch' && inputs.dry_run == true }}", "DRY_RUN: ${{ inputs.dry_run }}");
    expect(errorsOf(r)).toMatch(/release.yml must set job env DRY_RUN/);
  });

  it("refuses a node pin that differs from build-sidecar.sh's floor or the app's chain crates", () => {
    const r = fixture();
    const m = manifest(r);
    comp(m, "citrate-node").source.rev = "f9f6257b2dcde21149fe9c09236231ccfc32da68";
    setManifest(r, m);
    const e = errorsOf(r);
    expect(e).toMatch(/citrate-node rev f9f6257b\S+ != build-sidecar.sh MIN_CHAIN_REV c384fc98/);
    expect(e).toMatch(/src-tauri\/Cargo.toml pins citrate-chain c384fc98\S+, the bundled node is f9f6257b/);
  });

  it("refuses a Gemma pin that differs from the app's own model pin", () => {
    const r = fixture();
    edit(r, "src-tauri/src/model.rs", "a555b900214b477d8880e7832e0b8925e139b0159640036b09fe472b6f2097f2", "0".repeat(64));
    expect(errorsOf(r)).toMatch(/'gemma-4-e4b': sha256 a555b900\S+ != src-tauri\/src\/model.rs MODEL_SHA256 0{64}/);
  });

  it("refuses a Gemma size that differs from the app's", () => {
    const r = fixture();
    edit(r, "src-tauri/src/model.rs", "4_590_807_392", "4_590_807_391");
    expect(errorsOf(r)).toMatch(/'gemma-4-e4b': size 4590807392 != src-tauri\/src\/model.rs MODEL_SIZE_BYTES/);
  });

  it("refuses a Gemma URL on a branch instead of a revision", () => {
    const r = fixture();
    const m = manifest(r);
    m.models.find((x) => x.id === "gemma-4-e4b").url = "https://huggingface.co/ggml-org/gemma-4-E4B-it-GGUF/resolve/main/gemma-4-E4B-it-Q4_0.gguf";
    setManifest(r, m);
    expect(errorsOf(r)).toMatch(/'gemma-4-e4b': url must be a Hugging Face resolve URL at a 40-hex revision/);
  });

  it("refuses a component the licence inventory does not cover", () => {
    const r = fixture();
    const lic = JSON.parse(read(r, "release/licences.json"));
    lic.components = lic.components.filter((c) => c.id !== "kubo");
    write(r, "release/licences.json", JSON.stringify(lic));
    expect(errorsOf(r)).toMatch(/'kubo' has no entry in release\/licences.json/);
  });

  it("refuses a toolchain that is not an exact version", () => {
    const r = fixture();
    const m = manifest(r);
    m.toolchain.rust = "stable";
    setManifest(r, m);
    expect(errorsOf(r)).toMatch(/toolchain.rust must be an exact x.y.z version/);
  });
});

// citrate-core hands mem-mcp its store key in an owner-only file named by
// CITRATE_MEM_STORE_KEY_FILE; mem-mcp reads it from citrate-memories 5aeda317 (#25) on. An older
// mem-mcp ignores the file and runs under a different daemon identity.
const KEY_FILE_REV = "5aeda31723693edf577f0bad238f691586ccde39";
const KEY_FILE_MARKER = "CITRATE_MEM_STORE_KEY_FILE";
const PRE_KEY_FILE_REV = "0e9d48822054f6c9d644e691909569d0bc19be02";

/** A git repo with a linear history a <- b <- c, beside a fixture root; returns its shas. */
function sourceRepo(root, name) {
  const dir = path.join(path.dirname(root), name);
  fs.mkdirSync(dir);
  const git = (...a) => execFileSync("git", ["-C", dir, "-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false", ...a], { encoding: "utf8" }).trim();
  git("init", "-q");
  const shas = [];
  for (const n of ["a", "b", "c"]) {
    fs.writeFileSync(path.join(dir, n), n);
    git("add", n);
    git("commit", "-q", "-m", n);
    shas.push(git("rev-parse", "HEAD"));
  }
  return { dir, a: shas[0], b: shas[1], c: shas[2] };
}

describe("component requires (mem-mcp store key file)", () => {
  const memories = path.join(repo, "..", "citrate-memories");
  // The sibling citrate-memories checkout (the release Mac's layout), with both revs; CI has none.
  const hasCommit = (sha) => {
    try {
      execFileSync("git", ["-C", memories, "cat-file", "-e", `${sha}^{commit}`], { stdio: "ignore" });
      return true;
    } catch {
      return false;
    }
  };
  const haveMemories = fs.existsSync(memories) && hasCommit(KEY_FILE_REV) && hasCommit(PRE_KEY_FILE_REV);

  it("mem-mcp declares the store-key-file intake it must be built with", () => {
    const req = comp(manifest(repo), "mem-mcp").requires || [];
    expect(req).toEqual(expect.arrayContaining([expect.objectContaining({ rev: KEY_FILE_REV, marker: KEY_FILE_MARKER })]));
  });

  it.skipIf(!haveMemories)("the pinned mem-mcp contains citrate-memories#25 (key file)", () => {
    const rev = comp(manifest(repo), "mem-mcp").source.rev;
    expect(revContains(memories, KEY_FILE_REV, rev)).toBe(true);
  });

  it.skipIf(!haveMemories)("the 0.5.1 mem-mcp rev predates the key file (the pin this replaces)", () => {
    const c = { id: "mem-mcp", source: { rev: PRE_KEY_FILE_REV }, requires: [{ rev: KEY_FILE_REV, marker: KEY_FILE_MARKER, why: "key file" }] };
    expect(requirementRevErrors(c, memories).join("\n")).toMatch(/'mem-mcp': pinned rev 0e9d4882\S+ predates 5aeda317/);
  });

  it("refuses a pinned rev older than a required rev", () => {
    const r = fixture();
    const src = sourceRepo(r, "citrate-memories");
    const m = manifest(r);
    const c = comp(m, "mem-mcp");
    c.source.rev = src.a;
    c.requires = [{ rev: src.b, marker: KEY_FILE_MARKER, why: "store key file" }];
    setManifest(r, m);
    expect(errorsOf(r)).toMatch(new RegExp(`'mem-mcp': pinned rev ${src.a} predates ${src.b}`));
  });

  it("accepts a pinned rev at or after the required rev, and is strict only on request", () => {
    const r = fixture();
    const src = sourceRepo(r, "citrate-memories");
    const at = (rev, need) => ({ id: "x", source: { rev }, requires: [{ rev: need, marker: KEY_FILE_MARKER, why: "w" }] });
    expect(requirementRevErrors(at(src.b, src.b), src.dir)).toEqual([]);
    expect(requirementRevErrors(at(src.c, src.b), src.dir)).toEqual([]);
    expect(requirementRevErrors(at(src.b, src.c), src.dir).join()).toMatch(/predates/);
    // A commit the checkout does not have: unknown. Lenient in CI's check, an error for a build.
    const missing = "1".repeat(40);
    expect(requirementRevErrors(at(missing, src.b), src.dir)).toEqual([]);
    expect(requirementRevErrors(at(missing, src.b), src.dir, { strict: true }).join()).toMatch(/cannot tell whether 1{40} contains/);
    expect(requirementRevErrors(at(src.c, src.b), path.join(src.dir, "nope"), { strict: true }).join()).toMatch(/cannot tell/);
  });

  it("refuses bytes without the required marker", () => {
    const r = fixture();
    const c = comp(manifest(r), "mem-mcp");
    c.requires = [{ rev: KEY_FILE_REV, marker: KEY_FILE_MARKER, why: "store key file" }];
    const old = path.join(r, "mem-mcp-old");
    const now = path.join(r, "mem-mcp-new");
    // 0.5.1's mem-mcp names only the inline env var, which is a prefix of the file one.
    fs.writeFileSync(old, Buffer.concat([Buffer.alloc(32), Buffer.from("CITRATE_MEM_STORE_KEY unset"), Buffer.alloc(8)]));
    fs.writeFileSync(now, Buffer.concat([Buffer.alloc(32), Buffer.from("CITRATE_MEM_STORE_KEY_FILECITRATE_MEM_STORE_KEY"), Buffer.alloc(8)]));
    expect(requirementByteErrors(c, old).join()).toMatch(/does not carry CITRATE_MEM_STORE_KEY_FILE, so it predates 5aeda317/);
    expect(requirementByteErrors(c, now)).toEqual([]);
    expect(requirementByteErrors(c, path.join(r, "absent")).join()).toMatch(/does not exist/);
  });

  it("checks every staged sidecar that has requirements", () => {
    const r = fixture();
    const m = manifest(r);
    comp(m, "mem-mcp").requires = [{ rev: KEY_FILE_REV, marker: KEY_FILE_MARKER, why: "store key file" }];
    const dir = path.join(r, "bins");
    fs.mkdirSync(dir);
    expect(stagedRequirementErrors(m, dir, "aarch64-apple-darwin").join()).toMatch(/mem-mcp-aarch64-apple-darwin does not exist/);
    fs.writeFileSync(path.join(dir, "mem-mcp-aarch64-apple-darwin"), "usage: mem-mcp <store-path> <sock-path>");
    expect(stagedRequirementErrors(m, dir, "aarch64-apple-darwin").join()).toMatch(/does not carry CITRATE_MEM_STORE_KEY_FILE/);
    fs.writeFileSync(path.join(dir, "mem-mcp-aarch64-apple-darwin"), "CITRATE_MEM_STORE_KEY_FILE");
    expect(stagedRequirementErrors(m, dir, "aarch64-apple-darwin")).toEqual([]);
  });

  it("refuses a malformed requirement", () => {
    const r = fixture();
    const m = manifest(r);
    comp(m, "mem-mcp").requires = [{ rev: "5aeda31", marker: "KEY", why: "" }];
    setManifest(r, m);
    const e = errorsOf(r);
    expect(e).toMatch(/'mem-mcp' requires\[0\]: rev must be a full 40-hex commit/);
    expect(e).toMatch(/'mem-mcp' requires\[0\]: marker must be a string of at least 8 characters/);
    expect(e).toMatch(/'mem-mcp' requires\[0\]: why missing/);
  });

  it("refuses a rev bump that keeps the old build's runtime-deps asset", () => {
    const r = fixture();
    const m = manifest(r);
    comp(m, "node-agent").source.rev = "2".repeat(40);
    setManifest(r, m);
    expect(errorsOf(r)).toMatch(/'node-agent' aarch64-apple-darwin: runtime-deps asset node-agent-aarch64-apple-darwin-1d8cb8e4e24a is not the build of the pinned rev \(expected node-agent-aarch64-apple-darwin-222222222222\)/);
  });

  it("refuses release.yml that does not verify the staged sidecars' requirements", () => {
    const r = fixture();
    const m = manifest(r);
    comp(m, "mem-mcp").requires = [{ rev: KEY_FILE_REV, marker: KEY_FILE_MARKER, why: "store key file" }];
    setManifest(r, m);
    const wf = read(r, ".github/workflows/release.yml");
    write(r, ".github/workflows/release.yml", wf.split("\n").filter((l) => !l.includes("verify-staged")).join("\n"));
    expect(errorsOf(r)).toMatch(/release.yml does not run `component-pins.mjs verify-staged src-tauri\/binaries aarch64-apple-darwin`/);
  });
});
