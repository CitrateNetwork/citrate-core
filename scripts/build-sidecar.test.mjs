// @vitest-environment node
//
// scripts/build-sidecar.sh: a per-target node rev (release/components.json
// citrate-node source.target_revs, Windows 0.5.9) builds only when it is the base rev plus commits
// that touch no consensus path (component-pins.mjs NODE_CONSENSUS_PATHS). Every case refuses or
// accepts before any cargo build runs: a fake chain checkout, a scratch manifest, no compiler.
import { afterEach, describe, expect, it } from "vitest";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const REPO = path.join(here, "..");
const WIN = "x86_64-pc-windows-msvc";
const has = (cmd) => spawnSync("sh", ["-c", `command -v ${cmd}`]).status === 0;
const CAN_RUN = process.platform !== "win32" && has("git") && has("bash");

const tmpDirs = [];
afterEach(() => {
  for (const d of tmpDirs.splice(0)) fs.rmSync(d, { recursive: true, force: true });
});

const git = (cwd, ...args) => {
  const r = spawnSync("git", ["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false", ...args], { cwd, encoding: "utf8" });
  if (r.status !== 0) throw new Error(`git ${args.join(" ")}: ${r.stderr}`);
  return r.stdout.trim();
};
const put = (dir, rel, text) => {
  fs.mkdirSync(path.dirname(path.join(dir, rel)), { recursive: true });
  fs.writeFileSync(path.join(dir, rel), text);
};

/** A fake citrate-chain: a base commit, then one commit per `changes` entry ({ file: text }). */
function fixture(changes) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "build-sidecar-"));
  tmpDirs.push(root);
  const chain = path.join(root, "citrate-chain");
  fs.mkdirSync(chain);
  git(chain, "init", "-q");
  put(chain, "core/execution/src/executor.rs", "pub const VALUE_TRANSFER_ACTIVATION_HEIGHT: u64 = 0;\n");
  put(chain, "core/consensus/src/ghostdag.rs", "pub const MERGE_DEPTH_ACTIVATION_HEIGHT: u64 = 0;\n");
  put(chain, "core/consensus/src/crypto.rs", "pub fn sign_transaction_v2() {}\n");
  put(chain, "core/storage/src/fd_limit.rs", "// unix only\n");
  git(chain, "add", "-A");
  git(chain, "commit", "-q", "-m", "base");
  const base = git(chain, "rev-parse", "HEAD");
  for (const c of changes) {
    for (const [f, t] of Object.entries(c)) put(chain, f, t);
    git(chain, "add", "-A");
    git(chain, "commit", "-q", "-m", "change");
  }
  const head = git(chain, "rev-parse", "HEAD");
  const m = JSON.parse(fs.readFileSync(path.join(REPO, "release", "components.json"), "utf8"));
  const node = m.components.find((c) => c.id === "citrate-node");
  node.source.rev = base;
  node.source.target_revs = { [WIN]: { rev: head, why: "test", base } };
  fs.mkdirSync(path.join(root, "release"));
  const manifestPath = path.join(root, "release", "components.json");
  fs.writeFileSync(manifestPath, JSON.stringify(m, null, 2));
  return { root, chain, base, head, manifestPath, m, node };
}

const run = (f, target = WIN) =>
  spawnSync("bash", [path.join(here, "build-sidecar.sh"), "--chain", f.chain, "--target", target, "--print-digest"], {
    encoding: "utf8",
    env: { ...process.env, CITRATE_COMPONENTS_MANIFEST: f.manifestPath, CITRATE_COMPONENT_BUILD_ROOT: path.join(f.root, "cb") },
  });

describe.skipIf(!CAN_RUN)("build-sidecar.sh: a per-target node rev", { timeout: 60_000 }, () => {
  it("accepts the base plus a cfg-only storage change, and reports what it adds", () => {
    const f = fixture([{ "core/storage/src/fd_limit.rs": "// #[cfg(unix)] gated\n" }]);
    const r = run(f);
    expect(r.stdout).toContain(`builds the node at ${f.head}, the base ${f.base} plus:`);
    expect(r.stdout).toContain("touches core/storage/src/fd_limit.rs");
    expect(r.stdout).toContain("OK: no consensus path changed");
    // It then reaches the lineage gate (this fake chain has no MIN_CHAIN_REV, so it stops there).
    expect(r.stderr).toMatch(/does not contain [0-9a-f]{40} \(activation hardening\)/);
  });

  for (const file of ["core/consensus/src/ghostdag.rs", "core/execution/src/vm.rs", "core/storage/src/state/trie.rs", "Cargo.lock", "core/network/src/peer.rs"]) {
    it(`refuses a target rev that changes ${file}`, () => {
      const f = fixture([{ "core/storage/src/fd_limit.rs": "// gated\n" }, { [file]: "// changed\n" }]);
      const r = run(f);
      expect(r.status).not.toBe(0);
      expect(r.stderr).toContain(`changes consensus paths relative to ${f.base}: ${file}`);
    });
  }

  it("refuses a target rev that does not descend from the base", () => {
    const f = fixture([{ "core/storage/src/fd_limit.rs": "// gated\n" }]);
    // Point the base at the head and the target rev at the old base: not a descendant.
    f.node.source.rev = f.head;
    f.node.source.target_revs[WIN] = { rev: f.base, why: "test", base: f.head };
    fs.writeFileSync(f.manifestPath, JSON.stringify(f.m, null, 2));
    const r = run(f);
    expect(r.status).not.toBe(0);
    expect(r.stderr).toContain(`does not descend from the base ${f.head}`);
  });

  it("builds every other target from the base, with no target-rev step", () => {
    const f = fixture([{ "core/consensus/src/ghostdag.rs": "// would be refused for Windows\n" }]);
    const r = run(f, "x86_64-unknown-linux-gnu");
    expect(r.stdout).not.toContain("builds the node at");
    expect(r.stdout).toContain(`verifying the pinned citrate-chain source (${f.base})`);
  });
});
