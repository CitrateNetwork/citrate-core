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
import { describe, expect, it, beforeEach, afterEach, beforeAll, afterAll } from "vitest";
import { spawn, spawnSync } from "node:child_process";
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
// A cut that builds every node target (the shipped cut's `notBuilt` is dropped; the notBuilt tests
// at the end put it back).
const filledCut = ({ notBuilt, ...cut }) => ({
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

// core#611 item 2: `check --release` asks the cut's rpcUrl for eth_chainId and block 0. In the
// tests that request goes to a stand-in RPC on loopback (NETWORK_CUT_RPC_PROBE), a separate
// process because py() blocks this one. It answers from a JSON file each test can rewrite.
const RPC_STAND_IN = `
import json, sys
from http.server import BaseHTTPRequestHandler, HTTPServer
answers = sys.argv[1]
class H(BaseHTTPRequestHandler):
    def log_message(self, *a): pass
    def do_POST(self):
        req = json.loads(self.rfile.read(int(self.headers["content-length"])))
        a = json.load(open(answers))
        if a.get("status", 200) != 200:
            self.send_response(a["status"]); self.end_headers(); return
        body = a.get("raw") or json.dumps({"jsonrpc": "2.0", "id": req["id"], "result": a.get(req["method"])})
        self.send_response(200); self.send_header("content-type", "application/json"); self.end_headers()
        self.wfile.write(body.encode())
s = HTTPServer(("127.0.0.1", 0), H)
print(s.server_address[1], flush=True)
s.serve_forever()
`;
let rpcProc;
let rpcPort;
let answersPath;
const goodAnswers = () => ({ eth_chainId: "0x9d0c", eth_getBlockByNumber: { number: "0x0", hash: T_GENESIS } });
const setAnswers = (a) => fs.writeFileSync(answersPath, JSON.stringify(a));
beforeAll(async () => {
  answersPath = path.join(fs.mkdtempSync("/tmp/core053-rpc-"), "answers.json");
  setAnswers(goodAnswers());
  rpcProc = spawn("python3", ["-I", "-c", RPC_STAND_IN, answersPath], { stdio: ["ignore", "pipe", "inherit"] });
  rpcPort = await new Promise((res) => rpcProc.stdout.once("data", (d) => res(Number(String(d).trim()))));
});
afterAll(() => {
  rpcProc?.kill();
  if (answersPath) fs.rmSync(path.dirname(answersPath), { recursive: true, force: true });
});
beforeEach(() => answersPath && setAnswers(goodAnswers()));

let root;
let probe; // undefined: the stand-in; null: no override (the real rpcUrl); a string: that value
const env = () => {
  const e = { PATH: process.env.PATH, HOME: root ?? repo, LANG: "C.UTF-8" };
  const p = probe === undefined ? `http://127.0.0.1:${rpcPort}` : probe;
  if (p !== null) e.NETWORK_CUT_RPC_PROBE = p;
  return e;
};
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

// The repo now carries the APPLIED r1007 cut (2026-11-12). The apply/fill tests run on a copy of the
// tree as it was before the cut: the four files the cut rewrites come from fixtures/network-cut/pre-cut
// (their bytes at the commit before the fill), and the files where the cut only moved the chain rev
// get the pre-cut rev back.
const PRE_CUT_DIR = path.join(repo, "fixtures/network-cut/pre-cut");
const PRE_CUT_FILES = ["release/network-cut.json", "release/components.json", "src-tauri/network/profiles.json", "src-tauri/runtime-deps.sha256"];
const PRE_CUT_REV = JSON.parse(fs.readFileSync(path.join(PRE_CUT_DIR, "release/components.json"), "utf8")).components.find((c) => c.id === "citrate-node").source.rev;
const SHIPPED = JSON.parse(fs.readFileSync(path.join(repo, "release/network-cut.json"), "utf8"));
const preCut = (f) => {
  if (PRE_CUT_FILES.includes(f)) return fs.readFileSync(path.join(PRE_CUT_DIR, f), "utf8");
  const text = fs.readFileSync(path.join(repo, f), "utf8");
  return text.split(SHIPPED.chainRev).join(PRE_CUT_REV).split(`#${SHIPPED.chainRev.slice(0, 8)}`).join(`#${PRE_CUT_REV.slice(0, 8)}`);
};
beforeEach(() => {
  root = fs.mkdtempSync("/tmp/core053-cut-");
  for (const f of CUT_FILES) {
    fs.mkdirSync(path.dirname(path.join(root, f)), { recursive: true });
    fs.writeFileSync(path.join(root, f), preCut(f));
  }
});
afterEach(() => {
  if (root && root.startsWith("/tmp/core053-cut-")) fs.rmSync(root, { recursive: true, force: true });
  root = undefined;
  probe = undefined;
});

// The values the r1007 cut was filled with on 2026-11-12, each checked against its source: block 0,
// the bootnodes and the fingerprint against the live fleet (rpc.citrate.ai citrate_nodeIdentity and
// each host's own node), the chain rev against public citrate-chain (tag r1007-genesis), the book
// digest against the address lane's r1007 root book.
const R1007_CUT = {
  genesisHash: "0xcf68634502b4fca1e977ac8dd35aa006bb40c04208d5cd55f1c2b508bba96631",
  genesisProfile: "r1007",
  compiledActivations: true,
  rpcUrl: "https://rpc.citrate.ai",
  chainRev: "a53bac644665a992bf871831250af7354fd69512",
  consensusFingerprint: "0x5d26c4ab5b0be144d26916b9070a70a4",
  addressBookSha256: "1b908e56f99ef14e0fe260d27e0ce837200ee4bb0884771b5a007ec66a93ca96",
  bootnodes: [
    "noise_f356d3ebb07371eaad371b3960272f9d58fc457cde409c34549ef03776b78141@boot1.citrate.ai:30303",
    "noise_4ed281386422f6a65b92d8760d24baa82bb1b476e9dd3e21d5a070d026802c07@boot2.citrate.ai:30303",
    "noise_2b4924671e0babc9f52eb1695c72141a9c639e17ad95a2a2d2a715eae34a420e@boot3.citrate.ai:30303",
    "noise_6ee549718d522c9ccc122585dfedef72aea8df24f4a2bb0264e7658a91118b4a@rpc.citrate.ai:30303",
  ],
};

describe("the shipped cut (release/network-cut.json, applied 2026-11-12)", SLOW, () => {
  it("is the complete r1007 cut, applied: nothing pending, Linux aarch64 not built, r1007 active", () => {
    expect(SHIPPED.profile).toBe("r1007");
    expect(Object.keys(SHIPPED.retire)).toEqual(["r1006"]);
    expect(SHIPPED).toMatchObject(R1007_CUT);
    expect(SHIPPED.pending).toEqual({});
    expect(Object.keys(SHIPPED.notBuilt)).toEqual(["aarch64-unknown-linux-gnu"]);
    expect(SHIPPED.node["aarch64-apple-darwin"].runtime_deps_asset).toBe("citrate-aarch64-apple-darwin-a53bac644665");
    expect(SHIPPED.node["x86_64-unknown-linux-gnu"].sha256).toMatch(/^[0-9a-f]{64}$/);
    const prof = JSON.parse(fs.readFileSync(path.join(repo, "src-tauri/network/profiles.json"), "utf8"));
    expect(prof.active).toBe("r1007");
    expect(prof.profiles.r1007).toMatchObject({ genesisHash: R1007_CUT.genesisHash, minChainRev: R1007_CUT.chainRev, bootnodes: R1007_CUT.bootnodes, pending: {} });
    expect(prof.profiles.r1006.retired).toMatch(/r1007/);
    // The shipped tree passes the release check of the cut (the stand-in RPC answers as the fleet does).
    setAnswers({ eth_chainId: "0x9d0c", eth_getBlockByNumber: { number: "0x0", hash: R1007_CUT.genesisHash } });
    const r = py(["check", "--release", "--root", repo, "--cut-only"]);
    expect(r.code, r.err).toBe(0);
  });

  it("before the fill (fixtures/network-cut/pre-cut), every value was pending with a reason", () => {
    const cut = json("release/network-cut.json");
    expect(cut.profile).toBe("r1007");
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
        "node.x86_64-unknown-linux-gnu",
        "rpcUrl",
      ].sort(),
    );
    for (const [k, why] of Object.entries(cut.pending)) expect(why.trim().length, k).toBeGreaterThan(20);
    const r = py(["check", "--root", root, "--cut-only"]);
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

  it("check --release fails when the applied profile's rpcUrl is not the cut's (core#611)", () => {
    writeJson("release/network-cut.json", filledCut(json("release/network-cut.json")));
    expect(py(["apply", "--root", root]).code).toBe(0);
    const p = json("src-tauri/network/profiles.json");
    p.profiles.r1007.rpcUrl = "https://rpc2.citrate.ai";
    writeJson("src-tauri/network/profiles.json", p);
    const c = py(["check", "--release", "--root", root, "--cut-only"]);
    expect(c.code).not.toBe(0);
    expect(c.err).toMatch(/r1007\.rpcUrl/);
  });

  it("apply fills a profile whose rpcUrl is unset: the cut is its one source (core#611)", () => {
    const p = json("src-tauri/network/profiles.json");
    p.profiles.r1007.rpcUrl = null;
    p.profiles.r1007.pending.rpcUrl = "the public JSON-RPC of the r1007 fleet; set by the r1007 cut";
    writeJson("src-tauri/network/profiles.json", p);
    writeJson("release/network-cut.json", filledCut(json("release/network-cut.json")));
    const r = py(["apply", "--root", root]);
    expect(r.code, r.err).toBe(0);
    const after = json("src-tauri/network/profiles.json").profiles.r1007;
    expect(after.rpcUrl).toBe(CANDIDATE.rpcUrl);
    expect(Object.keys(after.pending ?? {})).not.toContain("rpcUrl");
  });

  it("check --release refuses the pre-cut file and names every pending value", () => {
    const r = py(["check", "--release", "--root", root, "--cut-only"]);
    expect(r.code).not.toBe(0);
    for (const k of ["genesisHash", "bootnodes", "chainRev", "chainParams", "rpcUrl", "consensusFingerprint", "node.aarch64-apple-darwin", "node.x86_64-unknown-linux-gnu"]) {
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
  it("--release --require-active r1007 passes on this build (the cut is applied)", () => {
    setAnswers({ eth_chainId: "0x9d0c", eth_getBlockByNumber: { number: "0x0", hash: R1007_CUT.genesisHash } });
    const r = sh(checkSh, ["--release", "--require-active", "r1007"]);
    expect(r.code, r.err).toBe(0);
    expect(r.out).toMatch(/active r1007, release/);
  });

  it("--release --require-active r1006 fails on this build: r1006 is retired", () => {
    const r = sh(checkSh, ["--release", "--require-active", "r1006"]);
    expect(r.code).not.toBe(0);
    expect(r.err).toMatch(/r1006/);
  });

  it("a pre-cut build with active flipped to r1007 by hand fails the release check, naming the unset values and the cut", () => {
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
    ["a pruning depth above the member retention (core#611)", (c) => (c.chainParams.pruning_depth = 20000), /pruning_depth.*memberPruneRetain/],
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

// core#611 item 2 (lead decision): the rpcUrl host is on the cut's allowlist, and `check --release`
// asks the RPC itself for chain 40204 and the cut's block 0, failing closed.
describe("rpcUrl: allowlisted host, verified live by check --release (core#611)", SLOW, () => {
  const applied = () => {
    writeJson("release/network-cut.json", filledCut(json("release/network-cut.json")));
    expect(py(["apply", "--root", root]).code).toBe(0);
  };
  const release = () => py(["check", "--release", "--root", root, "--cut-only"]);

  it("the shipped cut allowlists rpc.citrate.ai", () => {
    expect(json("release/network-cut.json").rpcHosts).toEqual(["rpc.citrate.ai"]);
  });

  const hostRefusals = [
    ["a host that only starts with an allowlisted one", "https://rpc.citrate.ai.attacker.io"],
    ["another host", "https://rpc.example.org"],
    ["user info in front of the host", "https://rpc.citrate.ai@attacker.io"],
  ];
  for (const [what, url] of hostRefusals) {
    it(`check refuses an rpcUrl on ${what}`, () => {
      const cut = filledCut(json("release/network-cut.json"));
      cut.rpcUrl = url;
      writeJson("release/network-cut.json", cut);
      const r = py(["check", "--root", root]);
      expect(r.code).not.toBe(0);
      expect(r.err).toMatch(/rpcUrl.*(rpcHosts|https)/);
    });
  }

  it("a second allowlisted host is accepted, and a malformed allowlist is refused", () => {
    const cut = filledCut(json("release/network-cut.json"));
    cut.rpcHosts = ["rpc.citrate.ai", "rpc-r1007.citrate.ai"];
    cut.rpcUrl = "https://rpc-r1007.citrate.ai";
    writeJson("release/network-cut.json", cut);
    expect(py(["check", "--root", root]).code).toBe(0);
    for (const bad of [[], ["RPC.citrate.ai"], ["rpc.citrate.ai", "rpc.citrate.ai"], ["https://rpc.citrate.ai"], "rpc.citrate.ai"]) {
      writeJson("release/network-cut.json", { ...cut, rpcHosts: bad });
      const r = py(["check", "--root", root]);
      expect(r.code, JSON.stringify(bad)).not.toBe(0);
      expect(r.err).toMatch(/rpcHosts/);
    }
  });

  it("check --release asks the RPC and passes that part when it serves chain 40204 and the cut's block 0", () => {
    applied();
    const r = release();
    expect(r.err).not.toMatch(/FAIL: rpcUrl/);
    expect(r.err).toMatch(/probe goes to the test endpoint http:\/\/127\.0\.0\.1:\d+, not https:\/\/rpc\.citrate\.ai/);
    // What is left is the Cargo.lock the cut does not write (as in the apply test above).
    expect(r.err.split("\n").filter((l) => l.includes("FAIL:"))).toEqual([expect.stringMatching(/FAIL: Cargo\.lock/)]);
  });

  const liveRefusals = [
    ["another chain", { ...goodAnswers(), eth_chainId: "0x1" }, /chain.*40204/],
    ["another block 0", { ...goodAnswers(), eth_getBlockByNumber: { number: "0x0", hash: "0x" + "ab".repeat(32) } }, /block 0.*genesisHash/],
    ["no block 0", { ...goodAnswers(), eth_getBlockByNumber: null }, /block 0/],
    ["an HTTP error", { status: 503 }, /could not be checked live/],
    ["an answer that is not JSON", { raw: "<html>" }, /could not be checked live/],
  ];
  for (const [what, answers, msg] of liveRefusals) {
    it(`check --release fails closed on ${what}`, () => {
      applied();
      setAnswers(answers);
      const r = release();
      expect(r.code).not.toBe(0);
      expect(r.err).toMatch(msg);
    });
  }

  it("check --release fails closed when the RPC cannot be reached", () => {
    applied();
    probe = "http://127.0.0.1:9";
    const r = release();
    expect(r.code).not.toBe(0);
    expect(r.err).toMatch(/rpcUrl.*could not be checked live/);
  });

  it("the probe override takes only a loopback http test endpoint", () => {
    applied();
    for (const p of ["https://rpc.example.org", "http://10.0.0.5:8545", "http://127.0.0.1:8545/x", ""]) {
      probe = p;
      const r = release();
      expect(r.code, p).not.toBe(0);
      expect(r.err).toMatch(/NETWORK_CUT_RPC_PROBE/);
    }
  });

  it("no workflow or script sets the probe override", () => {
    const hits = [];
    const walk = (d) => {
      for (const e of fs.readdirSync(path.join(repo, d), { withFileTypes: true })) {
        const rel = path.join(d, e.name);
        if (e.isDirectory()) walk(rel);
        else if (!rel.endsWith(".test.mjs") && fs.readFileSync(path.join(repo, rel), "utf8").includes("NETWORK_CUT_RPC_PROBE")) hits.push(rel);
      }
    };
    walk(".github");
    walk("scripts");
    expect(hits).toEqual(["scripts/network-cut.py"]);
  });
});

// 0.5.3 release readiness (owner decision 2026-11-11): one command fills the cut from the
// genesis outputs and applies it. `fill --values <file>` writes the values into the cut file and
// clears them from `pending`; with `--apply` it then applies, all or nothing.
describe("network-cut.py fill (the one-command cut)", SLOW, () => {
  // The values a real fill passes: no pin for a target the shipped cut does not build.
  const valuesOf = (cut) => {
    const { profile, retire, pending, _comment, rpcHosts, ...values } = filledCut(cut);
    for (const t of Object.keys(cut.notBuilt || {})) delete values.node[t];
    return values;
  };
  const writeValues = (v) => {
    const f = path.join(root, "cut-values.json");
    fs.writeFileSync(f, JSON.stringify(v, null, 2));
    return f;
  };

  it("fills every value, empties pending, and the cut checks", () => {
    const f = writeValues(valuesOf(json("release/network-cut.json")));
    const r = py(["fill", "--values", f, "--root", root]);
    expect(r.code, r.err).toBe(0);
    const cut = json("release/network-cut.json");
    expect(cut.pending).toEqual({});
    expect(cut.genesisHash).toBe(T_GENESIS);
    expect(cut.chainRev).toBe(T_REV);
    expect(cut.rpcHosts).toEqual(["rpc.citrate.ai"]);
    expect(py(["check", "--root", root, "--cut-only"]).code).toBe(0);
  });

  it("a partial fill sets only those values and keeps the rest pending", () => {
    const f = writeValues({ genesisHash: T_GENESIS, rpcUrl: CANDIDATE.rpcUrl });
    expect(py(["fill", "--values", f, "--root", root]).code).toBe(0);
    const cut = json("release/network-cut.json");
    expect(cut.genesisHash).toBe(T_GENESIS);
    expect(Object.keys(cut.pending)).not.toContain("genesisHash");
    expect(Object.keys(cut.pending)).toContain("chainRev");
  });

  const refusals = [
    ["an unknown key", { genesisHash: T_GENESIS, genesis_hash: T_GENESIS }, /unknown/],
    ["a null value", { genesisHash: null }, /null/],
    ["a malformed value", { genesisHash: "0x" + "00".repeat(32) }, /genesisHash/],
    ["the target profile", { profile: "r1006" }, /unknown|profile/],
    ["an unknown node target", { node: { "riscv64-unknown-linux-gnu": { sha256: "7d".repeat(32) } } }, /node/],
  ];
  for (const [what, v, msg] of refusals) {
    it(`refuses ${what} and writes nothing`, () => {
      const before = snapshot();
      const r = py(["fill", "--values", writeValues(v), "--root", root]);
      expect(r.code).not.toBe(0);
      expect(r.err).toMatch(msg);
      expect(snapshot()).toEqual(before);
    });
  }

  it("never replaces a value already in the cut", () => {
    expect(py(["fill", "--values", writeValues({ genesisHash: T_GENESIS }), "--root", root]).code).toBe(0);
    const before = snapshot();
    const r = py(["fill", "--values", writeValues({ genesisHash: "0x" + "7e".repeat(32) }), "--root", root]);
    expect(r.code).not.toBe(0);
    expect(r.err).toMatch(/already set/);
    expect(snapshot()).toEqual(before);
  });

  it("fill --apply writes the cut and every pin in one go", () => {
    const r = py(["fill", "--values", writeValues(valuesOf(json("release/network-cut.json"))), "--apply", "--root", root]);
    expect(r.code, r.err).toBe(0);
    expect(json("src-tauri/network/profiles.json").active).toBe("r1007");
    expect(rd("scripts/build-sidecar.sh")).toContain(`MIN_CHAIN_REV="${T_REV}"`);
    const c = py(["check", "--release", "--root", root, "--cut-only"]);
    expect(c.err.split("\n").filter((l) => l.includes("FAIL:"))).toEqual([expect.stringMatching(/FAIL: Cargo\.lock/)]);
  });

  it("fill --apply with values missing writes nothing at all", () => {
    const before = snapshot();
    const r = py(["fill", "--values", writeValues({ genesisHash: T_GENESIS }), "--apply", "--root", root]);
    expect(r.code).not.toBe(0);
    expect(r.err).toMatch(/not complete/);
    expect(snapshot()).toEqual(before);
  });
});

// Owner 2026-11-12: 0.5.3 ships macOS and Linux x86_64; Linux aarch64 is not built. The cut names a
// target it does not build in `notBuilt` (with the reason), so nothing pins, stages or waits for it.
describe("notBuilt: a node target this release does not build (owner 2026-11-12)", SLOW, () => {
  const A64 = "aarch64-unknown-linux-gnu";
  const REASON = "owner 2026-11-12: Linux aarch64 is not built for 0.5.3";
  const withoutA64 = (cut) => {
    const c = filledCut(cut);
    c.node = { ...c.node, [A64]: null };
    c.notBuilt = { [A64]: REASON };
    return c;
  };

  it("the shipped cut does not build Linux aarch64, and does not wait for it", () => {
    const cut = json("release/network-cut.json");
    expect(cut.notBuilt).toEqual({ [A64]: expect.stringMatching(/owner 2026-11-12/) });
    expect(cut.node[A64]).toBeNull();
    expect(Object.keys(cut.pending)).not.toContain(`node.${A64}`);
    const r = py(["check", "--root", root, "--cut-only"]);
    expect(r.code, r.err).toBe(0);
    expect(r.out).not.toContain(`node.${A64}`);
  });

  it("a complete cut without Linux aarch64 applies: no aarch64 pin, and citrate-node names it not built", () => {
    writeJson("release/network-cut.json", withoutA64(json("release/network-cut.json")));
    expect(py(["check", "--root", root, "--cut-only"]).code).toBe(0);
    const r = py(["apply", "--root", root]);
    expect(r.code, r.err).toBe(0);
    const node = json("release/components.json").components.find((c) => c.id === "citrate-node");
    expect(node.artifacts[A64]).toBeUndefined();
    expect(node.pending).toEqual({ [A64]: expect.stringMatching(/not built.*owner 2026-11-12/) });
    expect(rd("src-tauri/runtime-deps.sha256")).not.toContain(`citrate-${A64}-${T_REV.slice(0, 12)}`);
    expect(rd("src-tauri/runtime-deps.sha256")).toContain(`citrate-x86_64-unknown-linux-gnu-${T_REV.slice(0, 12)}.PROVENANCE.txt`);
    const c = py(["check", "--release", "--root", root, "--cut-only"]);
    expect(c.err.split("\n").filter((l) => l.includes("FAIL:"))).toEqual([expect.stringMatching(/FAIL: Cargo\.lock/)]);
  });

  const refusals = [
    ["the macOS node", (c) => ({ ...c, notBuilt: { "aarch64-apple-darwin": REASON } }), /notBuilt.*aarch64-apple-darwin/],
    ["a target that is also pinned", (c) => ({ ...c, node: filledCut(c).node }), /notBuilt.*pinned/],
    ["an empty reason", (c) => ({ ...c, notBuilt: { [A64]: " " } }), /notBuilt.*reason/],
    ["an unknown target", (c) => ({ ...c, notBuilt: { "riscv64-unknown-linux-gnu": REASON } }), /notBuilt.*riscv64/],
  ];
  for (const [what, mk, msg] of refusals) {
    it(`check refuses notBuilt for ${what}`, () => {
      writeJson("release/network-cut.json", mk(withoutA64(json("release/network-cut.json"))));
      const r = py(["check", "--root", root, "--cut-only"]);
      expect(r.code).not.toBe(0);
      expect(r.err).toMatch(msg);
    });
  }

  it("fill refuses a pin for a target the cut does not build, and writes nothing", () => {
    const before = snapshot();
    const v = { node: { [A64]: filledCut(json("release/network-cut.json")).node[A64] } };
    const f = path.join(root, "v.json");
    fs.writeFileSync(f, JSON.stringify(v));
    const r = py(["fill", "--values", f, "--root", root]);
    expect(r.code).not.toBe(0);
    expect(r.err).toMatch(/not built/);
    expect(snapshot()).toEqual(before);
  });
});
