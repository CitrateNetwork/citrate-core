// @vitest-environment node
//
// CORE-N1/N2 (0.5.3 = the network release for r1007): one cut file, release/network-cut.json,
// holds every value the r1007 genesis cut produces (genesis, bootnode keys, the chain rev the node
// and the app's chain crates are built from, the node bytes for macOS arm64 and Linux x86_64 and
// aarch64, the consensus fingerprint). scripts/network-cut.py applies it to every file that pins
// one of them, and refuses to do anything while a value is pending. Until the real values exist:
//
//   - the shipped cut file is well formed and every unset value names what sets it;
//   - `check --release` refuses it, naming each pending value;
//   - `scripts/check-network-profile.sh --release --require-active r1007` fails;
//   - a build with `active` flipped to r1007 by hand fails the release check too.
//
// The apply cases run on a copy of the files the cut writes, in a fresh temp dir, with test-only
// values defined below (never real genesis values, Rule 1). Nothing touches the repo.
import { describe, expect, it, beforeEach, afterEach } from "vitest";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const repo = path.resolve(here, "..");
const cutPy = path.join(here, "network-cut.py");
const checkSh = path.join(here, "check-network-profile.sh");
const SLOW = { timeout: 60_000 };

// The files the cut reads or writes, relative to the repo root.
const CUT_FILES = [
  "release/network-cut.json",
  "release/components.json",
  "src-tauri/network/profiles.json",
  "src-tauri/runtime-deps.sha256",
  "src-tauri/Cargo.toml",
  "kit/Cargo.toml",
  "Cargo.toml",
  "Cargo.lock",
  "scripts/build-sidecar.sh",
];

// The r1007 genesis values as the chain's genesis notes record them today: CANDIDATE fixtures
// (fixtures/network-cut/r1007-candidate.json), never release values. The block-0 hash changes at the
// chain's integration step; release values are written only into release/network-cut.json.
const CANDIDATE = JSON.parse(fs.readFileSync(path.join(repo, "fixtures/network-cut/r1007-candidate.json"), "utf8"));
// What `apply` writes into profiles.json from the candidate cut below (the r1007 profile, its
// nodeGenesisProfiles entry, `active`). src-tauri/src/network_profile_tests.rs renders the member
// node config from this same file, so the two tests together cover cut -> profile -> node.toml.
const APPLIED = "fixtures/network-cut/r1007-candidate.applied.json";
// Test-only values. Not a real genesis, key, commit or digest.
const T_GENESIS = CANDIDATE.genesisHash;
const T_REV = "7a".repeat(20);
const T_FP = "0x" + "7f".repeat(16);
const key = (b) => b.repeat(32);
const T_BOOTNODES = [
  `noise_${key("b1")}@boot1.citrate.ai:30303`,
  `noise_${key("b2")}@boot2.citrate.ai:30303`,
  `noise_${key("b3")}@boot3.citrate.ai:30303`,
  `noise_${key("c4")}@rpc.citrate.ai:30303`,
];
// The provenance record of each node build: a runtime-deps release asset next to the node bytes,
// named after them (lead decision on #605), with its sha256 pinned in runtime-deps.sha256.
const prov = (target, b) => ({ asset: `citrate-${target}-${T_REV.slice(0, 12)}.PROVENANCE.txt`, sha256: b.repeat(32) });
const filledCut = (cut) => ({
  ...cut,
  genesisHash: T_GENESIS,
  genesisProfile: CANDIDATE.genesisProfile,
  compiledActivations: CANDIDATE.compiledActivations,
  rpcUrl: CANDIDATE.rpcUrl,
  chainParams: { ...CANDIDATE.chainParams },
  bootnodes: T_BOOTNODES,
  addressBookSha256: "7b".repeat(32),
  chainRev: T_REV,
  consensusFingerprint: T_FP,
  node: {
    "aarch64-apple-darwin": {
      runtime_deps_asset: `citrate-aarch64-apple-darwin-${T_REV.slice(0, 12)}`,
      sha256: "7c".repeat(32),
      evidence: "test-only bytes (network-cut.test.mjs)",
      provenance: prov("aarch64-apple-darwin", "8c"),
    },
    "x86_64-unknown-linux-gnu": { sha256: "7d".repeat(32), evidence: "test-only bytes (network-cut.test.mjs)", provenance: prov("x86_64-unknown-linux-gnu", "8d") },
    // release.yml's Linux aarch64 job stages the node from runtime-deps, as the macOS job does.
    "aarch64-unknown-linux-gnu": {
      runtime_deps_asset: `citrate-aarch64-unknown-linux-gnu-${T_REV.slice(0, 12)}`,
      sha256: "7e".repeat(32),
      evidence: "test-only bytes (network-cut.test.mjs)",
      provenance: prov("aarch64-unknown-linux-gnu", "8e"),
    },
  },
  pending: {},
});

let root;
const env = () => ({ PATH: process.env.PATH, HOME: root ?? repo, LANG: "C.UTF-8" });
function py(args) {
  const r = spawnSync("python3", ["-I", cutPy, ...args], { env: env(), encoding: "utf8" });
  return { code: r.status, out: r.stdout, err: r.stderr };
}
function sh(script, args) {
  const r = spawnSync("bash", [script, ...args], { env: env(), encoding: "utf8" });
  return { code: r.status, out: r.stdout, err: r.stderr };
}
const rd = (rel) => fs.readFileSync(path.join(root, rel), "utf8");
const json = (rel) => JSON.parse(rd(rel));
const writeJson = (rel, v) => fs.writeFileSync(path.join(root, rel), JSON.stringify(v, null, 2) + "\n");
const snapshot = () => Object.fromEntries(CUT_FILES.map((f) => [f, rd(f)]));

beforeEach(() => {
  root = fs.mkdtempSync("/tmp/core053-cut-");
  for (const f of CUT_FILES) {
    fs.mkdirSync(path.dirname(path.join(root, f)), { recursive: true });
    fs.copyFileSync(path.join(repo, f), path.join(root, f));
  }
});
afterEach(() => {
  if (root && root.startsWith("/tmp/core053-cut-")) fs.rmSync(root, { recursive: true, force: true });
  root = undefined;
});

describe("the shipped cut file (release/network-cut.json)", SLOW, () => {
  it("targets r1007, retires r1006, and is well formed with a reason for every pending value", () => {
    const cut = JSON.parse(fs.readFileSync(path.join(repo, "release/network-cut.json"), "utf8"));
    expect(cut.profile).toBe("r1007");
    expect(Object.keys(cut.retire)).toEqual(["r1006"]);
    const pending = Object.keys(cut.pending).sort();
    expect(pending).toEqual(
      [
        "addressBookSha256",
        "bootnodes",
        "chainParams",
        "chainRev",
        "compiledActivations",
        "consensusFingerprint",
        "genesisHash",
        "genesisProfile",
        "node.aarch64-apple-darwin",
        "node.aarch64-unknown-linux-gnu",
        "node.x86_64-unknown-linux-gnu",
        "rpcUrl",
      ].sort(),
    );
    for (const [k, why] of Object.entries(cut.pending)) expect(why.trim().length, k).toBeGreaterThan(20);
    const r = py(["check"]);
    expect(r.code, r.err).toBe(0);
    expect(r.out).toMatch(/pending/);
  });

  it("check --release also fails while the applied pins lack a node build's provenance pin", () => {
    writeJson("release/network-cut.json", filledCut(json("release/network-cut.json")));
    expect(py(["apply", "--root", root]).code).toBe(0);
    const deps = rd("src-tauri/runtime-deps.sha256").split("\n").filter((l) => !l.endsWith("x86_64-unknown-linux-gnu-" + T_REV.slice(0, 12) + ".PROVENANCE.txt"));
    fs.writeFileSync(path.join(root, "src-tauri/runtime-deps.sha256"), deps.join("\n"));
    const c = py(["check", "--release", "--root", root, "--cut-only"]);
    expect(c.code).not.toBe(0);
    expect(c.err).toMatch(/runtime-deps\.sha256.*x86_64-unknown-linux-gnu.*PROVENANCE/);
  });

  it("check --release fails when the applied profile's [checkpoint] values are not the cut's", () => {
    writeJson("release/network-cut.json", filledCut(json("release/network-cut.json")));
    expect(py(["apply", "--root", root]).code).toBe(0);
    const p = json("src-tauri/network/profiles.json");
    p.profiles.r1007.nodeCheckpoint.interval = 2;
    writeJson("src-tauri/network/profiles.json", p);
    const c = py(["check", "--release", "--root", root, "--cut-only"]);
    expect(c.code).not.toBe(0);
    expect(c.err).toMatch(/r1007\.nodeCheckpoint/);
  });

  it("check --release refuses it and names every pending value", () => {
    const r = py(["check", "--release"]);
    expect(r.code).not.toBe(0);
    for (const k of ["genesisHash", "bootnodes", "chainRev", "chainParams", "rpcUrl", "consensusFingerprint", "node.aarch64-apple-darwin", "node.x86_64-unknown-linux-gnu", "node.aarch64-unknown-linux-gnu"]) {
      expect(r.err).toContain(k);
    }
  });

  it("apply refuses while anything is pending and writes nothing", () => {
    const before = snapshot();
    const r = py(["apply", "--root", root]);
    expect(r.code).not.toBe(0);
    expect(r.err).toMatch(/pending/);
    expect(snapshot()).toEqual(before);
  });
});

describe("check-network-profile.sh against r1007", SLOW, () => {
  it("--release --require-active r1007 fails on this build", () => {
    const r = sh(checkSh, ["--release", "--require-active", "r1007"]);
    expect(r.code).not.toBe(0);
    expect(r.err).toMatch(/r1007/);
  });

  it("a build with active flipped to r1007 by hand fails the release check, naming the unset values and the cut", () => {
    const p = json("src-tauri/network/profiles.json");
    p.active = "r1007";
    writeJson("src-tauri/network/profiles.json", p);
    const r = sh(checkSh, [
      "--release",
      "--require-active",
      "r1007",
      "--profiles",
      path.join(root, "src-tauri/network/profiles.json"),
      "--sidecar",
      path.join(root, "scripts/build-sidecar.sh"),
    ]);
    expect(r.code).not.toBe(0);
    expect(r.err).toMatch(/genesisHash/);
    expect(r.err).toMatch(/network-cut/);
  });

  it("a retired profile can never be active, release or not", () => {
    const p = json("src-tauri/network/profiles.json");
    p.profiles.r1006.retired = "replaced by r1007";
    writeJson("src-tauri/network/profiles.json", p);
    const r = sh(checkSh, ["--profiles", path.join(root, "src-tauri/network/profiles.json")]);
    expect(r.code).not.toBe(0);
    expect(r.err).toMatch(/r1006.*retired/);
  });
});

describe("network-cut.py apply (test-only values on a copy)", SLOW, () => {
  it("writes every pin from the one file, and nothing else", () => {
    writeJson("release/network-cut.json", filledCut(json("release/network-cut.json")));
    const before = snapshot();
    const r = py(["apply", "--root", root]);
    expect(r.code, r.err).toBe(0);

    const prof = json("src-tauri/network/profiles.json");
    expect(prof.active).toBe("r1007");
    expect(prof.profiles.r1007).toMatchObject({
      genesisHash: T_GENESIS,
      genesisProfile: "r1007",
      bootnodes: T_BOOTNODES,
      addressBookSha256: "7b".repeat(32),
      minChainRev: T_REV,
      rpcUrl: CANDIDATE.rpcUrl,
      // The [checkpoint] values the r1007 node requires: it refuses to start when its node config
      // names another interval, committee size or vote depth than the genesis compiles.
      nodeCheckpoint: {
        interval: CANDIDATE.chainParams.checkpoint_interval,
        committeeSize: CANDIDATE.chainParams.committee_size,
        voteDepth: CANDIDATE.chainParams.checkpoint_vote_depth,
      },
      pending: {},
    });
    expect(prof.nodeGenesisProfiles.r1007).toEqual({ genesisHash: T_GENESIS, nodeDefault: false, compiledActivations: true });
    // The profile the Rust render test reads is exactly what apply wrote from the candidate cut.
    const { _comment, ...golden } = JSON.parse(fs.readFileSync(path.join(repo, APPLIED), "utf8"));
    expect(
      { active: prof.active, nodeGenesisProfiles: { r1007: prof.nodeGenesisProfiles.r1007 }, profiles: { r1007: prof.profiles.r1007 } },
      `${APPLIED} is stale: apply the candidate cut (this test's filledCut) to a copy and copy the r1007 entries`,
    ).toEqual(golden);
    expect(prof.profiles.r1006.retired).toMatch(/r1007/);
    // r1006's values are kept: a data dir from it is still recognised and named.
    const was = JSON.parse(before["src-tauri/network/profiles.json"]);
    expect({ ...prof.profiles.r1006, retired: undefined }).toEqual({ ...was.profiles.r1006, retired: undefined });

    expect(rd("scripts/build-sidecar.sh")).toMatch(new RegExp(`^MIN_CHAIN_REV="${T_REV}"$`, "m"));
    const node = json("release/components.json").components.find((c) => c.id === "citrate-node");
    expect(node.source.rev).toBe(T_REV);
    expect(node.federation.rev).toBe(T_REV);
    expect(node.artifacts["aarch64-apple-darwin"]).toEqual({
      runtime_deps_asset: `citrate-aarch64-apple-darwin-${T_REV.slice(0, 12)}`,
      evidence: "test-only bytes (network-cut.test.mjs)",
    });
    expect(node.artifacts["x86_64-unknown-linux-gnu"].sha256).toBe("7d".repeat(32));
    expect(node.artifacts["aarch64-unknown-linux-gnu"]).toEqual({
      runtime_deps_asset: `citrate-aarch64-unknown-linux-gnu-${T_REV.slice(0, 12)}`,
      evidence: "test-only bytes (network-cut.test.mjs)",
    });
    expect(node.pending).toBeUndefined();
    expect(rd("src-tauri/runtime-deps.sha256")).toContain(`${"7c".repeat(32)}  citrate-aarch64-apple-darwin-${T_REV.slice(0, 12)}`);
    expect(rd("src-tauri/runtime-deps.sha256")).toContain(`${"7e".repeat(32)}  citrate-aarch64-unknown-linux-gnu-${T_REV.slice(0, 12)}`);
    // Each node build's PROVENANCE record is pinned next to its bytes.
    for (const [t, b] of [["aarch64-apple-darwin", "8c"], ["x86_64-unknown-linux-gnu", "8d"], ["aarch64-unknown-linux-gnu", "8e"]]) {
      expect(rd("src-tauri/runtime-deps.sha256")).toContain(`${b.repeat(32)}  citrate-${t}-${T_REV.slice(0, 12)}.PROVENANCE.txt`);
    }
    for (const f of ["src-tauri/Cargo.toml", "kit/Cargo.toml"]) {
      const t = rd(f);
      expect(t, f).not.toMatch(/citrate-chain\.git", rev = "(?!(?:7a){20})/);
      expect(t, f).toContain(`rev = "${T_REV}"`);
    }
    // Every other component and model is byte-identical in meaning.
    const strip = (m) => ({ ...m, components: m.components.filter((c) => c.id !== "citrate-node") });
    expect(strip(json("release/components.json"))).toEqual(strip(JSON.parse(before["release/components.json"])));
    // Cargo.lock is regenerated by cargo, not by the cut; the cut says so.
    expect(rd("Cargo.lock")).toBe(before["Cargo.lock"]);
    expect(r.out).toMatch(/cargo update/);

    // The cut's own release checks pass; what is left is named (the lock file and the embedded
    // book, which are regenerated by their own tools).
    const c = py(["check", "--release", "--root", root, "--cut-only"]);
    expect(c.err).not.toMatch(/pending|MIN_CHAIN_REV|components\.json|profiles\.json|runtime-deps/);
    expect(c.err).toMatch(/Cargo\.lock/);
    expect(c.code).not.toBe(0);
  });

  const refusals = [
    ["a zero genesis", (c) => (c.genesisHash = "0x" + "00".repeat(32)), /genesisHash/],
    ["r1006's genesis", (c) => (c.genesisHash = "0xa3651a272fe411948cbb4cf7cca4cee44be4248d32ab4376cea7636821c6cdd4"), /genesisHash.*r1006/],
    ["a short chain rev", (c) => (c.chainRev = "7a7a7a7a"), /chainRev/],
    ["a bootnode on another host", (c) => (c.bootnodes = [...T_BOOTNODES.slice(0, 3), `noise_${key("c4")}@evil.example:30303`]), /bootnodes/],
    ["a missing bootnode host", (c) => (c.bootnodes = T_BOOTNODES.slice(0, 3)), /bootnodes/],
    ["a malformed fingerprint", (c) => (c.consensusFingerprint = "0x1234"), /consensusFingerprint/],
    ["a macOS asset of another rev", (c) => (c.node["aarch64-apple-darwin"].runtime_deps_asset = "citrate-aarch64-apple-darwin-000000000000"), /aarch64-apple-darwin/],
    ["a Linux aarch64 asset of another rev", (c) => (c.node["aarch64-unknown-linux-gnu"].runtime_deps_asset = "citrate-aarch64-unknown-linux-gnu-000000000000"), /aarch64-unknown-linux-gnu.*runtime_deps_asset/],
    ["a Linux aarch64 node with no runtime-deps asset", (c) => delete c.node["aarch64-unknown-linux-gnu"].runtime_deps_asset, /node.aarch64-unknown-linux-gnu must have exactly/],
    ["a Linux pin with no evidence", (c) => (c.node["x86_64-unknown-linux-gnu"].evidence = ""), /x86_64-unknown-linux-gnu/],
    ["a value left in pending", (c) => (c.pending = { genesisHash: "still owed" }), /pending/],
    ["a node build with no provenance record", (c) => delete c.node["x86_64-unknown-linux-gnu"].provenance, /x86_64-unknown-linux-gnu.*provenance/],
    ["a provenance record not named after its node bytes", (c) => (c.node["aarch64-unknown-linux-gnu"].provenance.asset = "PROVENANCE-linux.txt"), /aarch64-unknown-linux-gnu.*provenance/],
    ["chain parameters missing one", (c) => delete c.chainParams.checkpoint_vote_depth, /chainParams.*checkpoint_vote_depth/],
    ["chain parameters with an unknown one", (c) => (c.chainParams.block_time = 2), /chainParams.*block_time/],
    ["a zero checkpoint interval", (c) => (c.chainParams.checkpoint_interval = 0), /chainParams.*checkpoint_interval/],
    ["a chain parameter that is not an integer", (c) => (c.chainParams.committee_size = "100"), /chainParams.*committee_size/],
    ["an undo window above the member retention", (c) => (c.chainParams.undo_window = 20000), /undo_window.*memberPruneRetain/],
    ["an RPC that is not https", (c) => (c.rpcUrl = "http://rpc.citrate.ai"), /rpcUrl.*https/],
    ["a provenance record with a malformed digest", (c) => (c.node["aarch64-apple-darwin"].provenance.sha256 = "8c"), /aarch64-apple-darwin.*provenance/],
  ];
  for (const [what, edit, msg] of refusals) {
    it(`refuses ${what} and writes nothing`, () => {
      const cut = filledCut(json("release/network-cut.json"));
      edit(cut);
      writeJson("release/network-cut.json", cut);
      const before = snapshot();
      const r = py(["apply", "--root", root]);
      expect(r.code).not.toBe(0);
      expect(r.err).toMatch(msg);
      expect(snapshot()).toEqual(before);
    });
  }
});
