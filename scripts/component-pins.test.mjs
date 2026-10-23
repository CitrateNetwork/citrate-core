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
import { check } from "./component-pins.mjs";

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
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "component-pins-"));
  tmpDirs.push(root);
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
