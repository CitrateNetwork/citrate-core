// @vitest-environment node
//
// LQ-WP07: the r1007 block-hash agreement check (scripts/ci/lq/chain-agreement.mjs, check
// `lq / chain-agreement`). The verdicts here have the shape scripts/e2e/cold-sync.py writes in its
// bounded anchored-prefix mode (sync.by_number), taken from cold-sync.yml run 45471958218.
import { describe, expect, it } from "vitest";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";
import { R1007, compare, plan, summarize, wrongGenesis } from "./chain-agreement.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const repo = path.join(here, "..", "..", "..");
const SCRIPT = path.join(here, "chain-agreement.mjs");

const SHA = "809fa547ac9180805b2802cd69c1dde386949f97f783d3a390529bcdfbcdf03a";
const FP = "0xa2f16115438794f0410b187c99a14e34";
const REV = "68434069de707e8454a1313a1f2e20ba449300f5";
const h = (n) => "0x" + n.toString(16).padStart(64, "a");
const RULES = ["profile", "launch", "node_binary", "endpoint_identity", "local_genesis", "node_chain_id",
  "fingerprint", "reaches_target", "agrees", "not_wedged", "peers", "at_rest", "node_alive", "cleanup"];

function verdict({ heights = [0, 1000, 2000, 3000, 4000, 5000, 5342], hash = (x) => (x === 0 ? R1007.genesisHash : h(x)), sha = SHA, fp = FP, chainId = R1007.chainId, genesis = R1007.genesisHash, status = {} } = {}) {
  const by = heights.map((height) => ({ height, node: hash(height), network: hash(height) }));
  const rules = RULES.map((rule) => ({ rule, status: status[rule] ?? "ok", detail: "" }));
  const fails = rules.filter((r) => r.status === "FAIL");
  return {
    tool: "cold-sync", verdict: fails.length ? "FAIL" : "PASS", reasons: fails.map((r) => r.rule), rules,
    profile: { name: "r1007", chainId, genesisHash: genesis },
    node: { sha256: sha, local_genesis: genesis, consensus_fingerprint: fp },
    sync: { mode: "bounded", anchor: { height: heights.at(-1), hash: hash(heights.at(-1)) }, mismatches: [], by_number: by },
  };
}
function negative(over = {}) {
  const st = { endpoint_identity: "FAIL", local_genesis: "FAIL", reaches_target: "skip", ...over };
  const rules = RULES.map((rule) => ({ rule, status: st[rule] ?? "ok", detail: "" }));
  return { verdict: rules.some((r) => r.status === "FAIL") ? "FAIL" : "PASS", rules };
}
const opts = (o = {}) => ({ arch: "aarch64", rev: REV, expectSha256: SHA, expectFingerprint: FP, ...o });
const summary = (arch, v = verdict()) => summarize(v, negative(), opts({ arch }));

describe("summarize (agreement-<arch>.json)", () => {
  it("a passing bounded run is recorded as agreement to its anchor height, not as a complete sync", () => {
    const s = summary("x86_64");
    expect(s.height).toBe(5342);
    expect(s.label).toBe("agreement to height 5342");
    expect(s.sampled.map((x) => x.height)).toEqual([0, 1000, 2000, 3000, 4000, 5000, 5342]);
    expect(s.chainId).toBe(40204);
    expect(s.genesisHash).toBe(R1007.genesisHash);
  });

  it("agreement summary refuses a run with fewer than 3 sampled heights", () => {
    expect(() => summarize(verdict({ heights: [0, 5342] }), negative(), opts())).toThrow(/fewer than 3 sampled heights/);
  });

  it("the evidence writer refuses a dirty binary (bytes other than the pin)", () => {
    expect(() => summarize(verdict({ sha: "0".repeat(63) + "1" }), negative(), opts())).toThrow(/not the pinned/);
  });

  it("refuses a node that reports another consensus fingerprint", () => {
    expect(() => summarize(verdict({ fp: "0x2b0f96ee2bb3ff72d69370dc19a7ffd4" }), negative(), opts())).toThrow(/fingerprint/);
  });

  it("refuses a run whose block 0 is not the r1007 genesis, even when the profile agrees with it", () => {
    const g = "0x" + "1".repeat(64);
    expect(() => summarize(verdict({ genesis: g, hash: (x) => (x === 0 ? g : h(x)) }), negative(), opts())).toThrow(/genesis/);
  });

  it("refuses a run on another chain id", () => {
    expect(() => summarize(verdict({ chainId: 40205 }), negative(), opts())).toThrow(/chain id/);
  });

  it("refuses a verdict that is not PASS", () => {
    expect(() => summarize(verdict({ status: { not_wedged: "FAIL" } }), negative(), opts())).toThrow(/not PASS/);
  });

  it("refuses a sampled height where the node and the network differ", () => {
    const v = verdict();
    v.sync.by_number[3].network = h(999999);
    expect(() => summarize(v, negative(), opts())).toThrow(/height 3000/);
  });

  it("refuses when the wrong-genesis case was not refused by both the endpoint and the node", () => {
    expect(() => summarize(verdict(), negative({ local_genesis: "ok" }), opts())).toThrow(/wrong genesis/);
    expect(() => summarize(verdict(), null, opts())).toThrow(/wrong genesis/);
  });
});

describe("compare (cross-arch)", () => {
  it("two arches that agree at every common sampled height pass", () => {
    const r = compare([summary("x86_64"), summary("aarch64", verdict({ heights: [0, 1000, 2000, 3000, 4000, 5000, 5400] }))]);
    expect(r.common_heights).toEqual([0, 1000, 2000, 3000, 4000, 5000]);
    expect(r.label).toMatch(/x86_64 and aarch64 report the same block hash at 6 common sampled heights/);
  });

  it("cross-arch compare fails on one differing hash", () => {
    const b = summary("aarch64");
    b.sampled[2] = { height: 2000, hash: h(424242) };
    expect(() => compare([summary("x86_64"), b])).toThrow(/height 2000/);
  });

  it("refuses a single arch, a repeated arch, or a different rev", () => {
    expect(() => compare([summary("x86_64")])).toThrow(/two arches/);
    expect(() => compare([summary("x86_64"), summary("x86_64")])).toThrow(/two arches/);
    const b = summary("aarch64");
    b.rev = "72c8e029b958c0ffee00000000000000000000aa";
    expect(() => compare([summary("x86_64"), b])).toThrow(/rev/);
  });

  it("refuses fewer than 3 common heights", () => {
    const b = summary("aarch64", verdict({ heights: [0, 1500, 2500, 3500] }));
    expect(() => compare([summary("x86_64"), b])).toThrow(/fewer than 3 common/);
  });

  it("refuses a summary that was not written by summarize (schema)", () => {
    const b = summary("aarch64");
    delete b.sampled;
    expect(() => compare([summary("x86_64"), b])).toThrow(/sampled/);
  });
});

describe("plan and wrong-genesis on this tree", () => {
  it("plan names the pinned runtime-deps asset, its pin and the cut's fingerprint", () => {
    for (const arch of ["x86_64", "aarch64"]) {
      const p = plan(repo, arch);
      expect(p.asset).toMatch(new RegExp(`^citrate-${arch}-unknown-linux-gnu-[0-9a-f]{12}$`));
      expect(p.sha256).toMatch(/^[0-9a-f]{64}$/);
      expect(p.fingerprint).toMatch(/^0x[0-9a-f]{32}$/);
    }
  });

  it("plan refuses an arch it has no runner for", () => {
    expect(() => plan(repo, "riscv64")).toThrow(/arch/);
  });

  it("wrong-genesis flips the genesis of the profile and its address book", () => {
    const out = fs.mkdtempSync(path.join(os.tmpdir(), "lq-agree-"));
    try {
      wrongGenesis(repo, path.join(repo, "src-tauri/network/profiles.json"), null, out);
      const p = JSON.parse(fs.readFileSync(path.join(out, "wrong-profile.json"), "utf8"));
      const b = JSON.parse(fs.readFileSync(path.join(out, "wrong-book.json"), "utf8"));
      expect(p.genesisHash).not.toBe(R1007.genesisHash);
      expect(p.genesisHash).toHaveLength(66);
      expect(b.genesisHash).toBe(p.genesisHash);
    } finally {
      fs.rmSync(out, { recursive: true, force: true });
    }
  });

  it("the CLI exits 1 on a refusal and 2 on a usage error", () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), "lq-agree-"));
    try {
      fs.writeFileSync(path.join(dir, "v.json"), JSON.stringify(verdict({ heights: [0, 5342] })));
      fs.writeFileSync(path.join(dir, "n.json"), JSON.stringify(negative()));
      const r = spawnSync(process.execPath, [SCRIPT, "summarize", "--verdict", path.join(dir, "v.json"), "--negative", path.join(dir, "n.json"),
        "--arch", "aarch64", "--rev", REV, "--expect-sha256", SHA, "--expect-fingerprint", FP, "--out", path.join(dir, "s.json")], { encoding: "utf8" });
      expect(r.status).toBe(1);
      expect(fs.existsSync(path.join(dir, "s.json"))).toBe(false);
      expect(spawnSync(process.execPath, [SCRIPT, "nonsense"], { encoding: "utf8" }).status).toBe(2);
    } finally {
      fs.rmSync(dir, { recursive: true, force: true });
    }
  });
});
