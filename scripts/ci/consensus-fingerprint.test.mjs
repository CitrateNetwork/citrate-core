// @vitest-environment node
//
// scripts/ci/consensus-fingerprint.py: a node built from the base rev plus cfg-only commits is the
// base rev's consensus when its fingerprint, recomputed with git_sha = the base, is the cut's.
// The fixture is the real Windows node's `consensus --json` (citrate-chain 98c513ec, release-windows
// run 45472309246). Changing any input other than git_sha must be refused.
import { afterEach, describe, expect, it } from "vitest";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const REPO = path.join(here, "..", "..");
const SCRIPT = path.join(here, "consensus-fingerprint.py");
const FIXTURE = path.join(REPO, "fixtures", "consensus-fingerprint", "windows-98c513ec.json");
const CUT = JSON.parse(fs.readFileSync(path.join(REPO, "release", "network-cut.json"), "utf8"));
const BASE = CUT.chainRev;
const WANT = CUT.consensusFingerprint;
const python = ["python3", "python"].find((p) => spawnSync(p, ["--version"]).status === 0);

const tmp = [];
afterEach(() => {
  for (const d of tmp.splice(0)) fs.rmSync(d, { recursive: true, force: true });
});

function run(doc, extra = []) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "consensus-fp-"));
  tmp.push(dir);
  const f = path.join(dir, "consensus.json");
  // As the binary prints it: a log line, then the JSON.
  fs.writeFileSync(f, `2026-11-24T19:40:36Z INFO citrate_storage::fd_limit: no RLIMIT_NOFILE\n${JSON.stringify(doc, null, 2)}\n`);
  const r = spawnSync(python, [SCRIPT, f, "--base-sha", BASE, "--want", WANT, ...extra], { encoding: "utf8" });
  return { status: r.status, out: r.status === 0 || r.stdout ? JSON.parse(r.stdout || "{}") : {}, err: r.stderr };
}
const fixture = () => JSON.parse(fs.readFileSync(FIXTURE, "utf8"));

describe.skipIf(!python)("consensus-fingerprint.py", () => {
  it("reproduces the Windows node's fingerprint, and gives the cut's with the base git_sha", () => {
    const r = run(fixture());
    expect(r.err).toBe("");
    expect(r.status).toBe(0);
    expect(r.out.reported).toBe("0x2b0f96ee2bb3ff72d69370dc19a7ffd4");
    expect(r.out.own).toBe(r.out.reported);
    expect(r.out.base).toBe(WANT);
    expect(r.out.base_sha).toBe(BASE.slice(0, 12));
  });

  // Every input of the pre-image but git_sha: a change to any of them is another consensus.
  const MUTATIONS = {
    feat_halo2_verifier: (d) => (d.feat_halo2_verifier = !d.feat_halo2_verifier),
    feat_commd_fold_verify: (d) => (d.feat_commd_fold_verify = !d.feat_commd_fold_verify),
    activation_gated: (d) => d.activation_gated.pop(),
    pba_hardening_height: (d) => (d.pba_hardening_height = 1),
    value_transfer_activation_height: (d) => (d.value_transfer_activation_height = 300000),
    create_nonce_fix_activation_height: (d) => (d.create_nonce_fix_activation_height = 7),
    merge_depth_activation_height: (d) => (d.merge_depth_activation_height = 100000),
    canonical_base_fee_per_gas: (d) => (d.canonical_base_fee_per_gas += 1),
    epoch: (d) => (d.epoch += 1),
    snapshot_lag: (d) => (d.snapshot_lag += 1),
    agent_precompiles_height: (d) => (d.agent_precompiles_height = 5),
    exact_create_code_height: (d) => (d.exact_create_code_height = null),
    vrf_canonical_encoding_height: (d) => (d.vrf_canonical_encoding_height = 9),
    supply_hardening_height: (d) => (d.supply_hardening_height = 9),
    storage_root_hardening_height: (d) => (d.storage_root_hardening_height = 9),
    mpt_state_root_height: (d) => (d.mpt_state_root_height = 9),
    genesis_hash: (d) => (d.genesis_hash = d.r1006_genesis),
    genesis_rules: (d) => (d.genesis_rules = d.genesis_rules.replace("ghostdag_k=18", "ghostdag_k=19")),
  };
  for (const [field, mutate] of Object.entries(MUTATIONS)) {
    it(`refuses a node whose ${field} differs from the base's`, () => {
      const d = fixture();
      mutate(d);
      // Keep the reconstruction exact (the binary would report the fingerprint of what it runs),
      // so only the base-equivalence decides.
      const own = run(d, []).out.own;
      d.fingerprint = own;
      const r = run(d);
      expect(r.status).toBe(1);
      expect(r.out.reconstruction_exact).toBe(true);
      expect(r.out.base_equivalent).toBe(false);
      expect(r.err).toMatch(/this node is NOT the base rev's consensus/);
    });
  }

  it("ignores what the pre-image excludes (build target)", () => {
    const d = fixture();
    d.build_target = "aarch64-apple-darwin";
    expect(run(d).status).toBe(0);
  });

  it("fails closed when its pre-image no longer reproduces the reported fingerprint", () => {
    const d = fixture();
    d.fingerprint = "0x" + "1".repeat(32);
    const r = run(d);
    expect(r.status).toBe(1);
    expect(r.err).toMatch(/the chain's fingerprint_preimage changed/);
  });

  it("refuses a binary built from a dirty tree", () => {
    const d = fixture();
    d.git_dirty = true;
    const r = run(d);
    expect(r.status).toBe(1);
    expect(r.err).toMatch(/dirty tree/);
  });
});
