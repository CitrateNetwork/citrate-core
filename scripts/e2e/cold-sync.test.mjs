// @vitest-environment node
//
// RC52 gate G4 (app side): scripts/e2e/cold-sync.py, the cold-sync harness for the node sidecar on
// the K6 network profile.
//
// The node is a stand-in (fixtures/stand-in-node.py) that takes the real command line and serves a
// JSON-RPC; the network RPC is a local stand-in. The live run against a real node and the public
// RPC is the workflow `.github/workflows/cold-sync.yml` and the PR evidence.
//
// 1. Launch spec: the node starts the way NodeManager::build_spec starts it (the profile's
//    --network, --data-dir, --config with the profile's bootnodes and chain id, the storage key as
//    a file path, the consensus env read from node.rs and the address book), in a temp dir that is
//    deleted afterwards, on ports that are not the app's.
// 2. The genesis gate: a profile whose genesis is not the network's, or a node whose block 0 is not
//    the profile's, is refused. So is a profile that is not ready (K6: null fields) or malformed.
// 3. Sync: bounded and to-tip targets pass; a stalled head, a fork, a dead node, no peers, a wrong
//    chain id and an unencrypted data dir fail. Every failure still deletes the temp dir.
// 4. Tripwire: no script outside the K6 profile and the member config carries a bootnode key or a
//    literal `--network testnet`, so a stale copy of the network cannot come back.
import { describe, expect, it, beforeAll, afterAll, beforeEach, afterEach, vi } from "vitest";
import { execFile, spawn } from "node:child_process";
import crypto from "node:crypto";
import fs from "node:fs";
import http from "node:http";
import net from "node:net";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const repo = path.resolve(here, "..", "..");
const script = path.join(here, "cold-sync.py");
const standIn = path.join(here, "fixtures", "stand-in-node.py");
const PY = process.env.COLD_SYNC_PYTHON || "python3";
// Each case starts a node process and polls it; the slowest (timeouts) take a few seconds.
vi.setConfig({ testTimeout: 60_000 });

const GENESIS = "0x" + "ab".repeat(32);
const OTHER = "0x" + "cd".repeat(32);
const REGISTRY = "0x1111111111111111111111111111111111111111";
const sha = (s) => "0x" + crypto.createHash("sha256").update(s).digest("hex");
const hashAt = (seed, h) => (h === 0 ? remote.genesis : sha(`${seed}:${h}`));
// The network's selected-parent chain: block h's parent is the chain block at h - 1. With
// `decoyEvery`, eth_getBlockByNumber answers a sibling at every multiple (not canonical, as on r1006).
const isDecoy = (h) => remote.decoyEvery && h > 0 && h % remote.decoyEvery === 0;
const remoteBlock = (h, decoy) => ({
  number: "0x" + h.toString(16),
  hash: decoy ? sha(`decoy:${h}`) : hashAt(remote.seed, h),
  parentHash: h > 0 ? hashAt(remote.seed, h - 1) : "0x" + "00".repeat(32),
  selectedParentHash: h > 0 ? hashAt(remote.seed, h - 1) : "0x" + "00".repeat(32),
});
const remoteMissing = (h) => remote.missing && remote.missing[0] <= h && h <= remote.missing[1];
let remoteIndex = null;
function remoteByHash(hash) {
  if (!remoteIndex || remoteIndex.head !== remote.head || remoteIndex.seed !== remote.seed || remoteIndex.decoyEvery !== remote.decoyEvery) {
    const map = new Map();
    for (let h = 0; h <= remote.head; h++) {
      map.set(hashAt(remote.seed, h), [h, false]);
      if (isDecoy(h)) map.set(sha(`decoy:${h}`), [h, true]);
    }
    remoteIndex = { head: remote.head, seed: remote.seed, decoyEvery: remote.decoyEvery, map };
  }
  const hit = remoteIndex.map.get(String(hash).toLowerCase());
  return !hit || remoteMissing(hit[0]) ? null : remoteBlock(hit[0], hit[1]);
}

function py(args, opts = {}) {
  return new Promise((resolve) => {
    execFile(PY, args, { timeout: 60_000, ...opts }, (err, stdout, stderr) => {
      resolve({ code: err ? (typeof err.code === "number" ? err.code : 1) : 0, stdout, stderr });
    });
  });
}

// ── the network RPC stand-in ─────────────────────────────────────────────────────────────────────
const remote = { chainId: 40204, genesis: GENESIS, head: 1200, seed: "main", missing: null, decoyEvery: null };
const server = http.createServer((req, res) => {
  let body = "";
  req.on("data", (c) => (body += c));
  req.on("end", () => {
    const { id, method, params = [] } = JSON.parse(body);
    let result = null;
    if (method === "eth_chainId") result = "0x" + remote.chainId.toString(16);
    else if (method === "eth_blockNumber") result = "0x" + remote.head.toString(16);
    else if (method === "eth_getBlockByNumber") {
      const h = params[0] === "latest" ? remote.head : parseInt(params[0], 16);
      result = h > remote.head || remoteMissing(h) ? null : remoteBlock(h, isDecoy(h));
    } else if (method === "eth_getBlockByHash") result = remoteByHash(params[0]);
    res.setHeader("content-type", "application/json");
    res.end(JSON.stringify({ jsonrpc: "2.0", id, result }));
  });
});
let rpcUrl;
beforeAll(() => new Promise((r) => server.listen(0, "127.0.0.1", () => r((rpcUrl = `http://127.0.0.1:${server.address().port}`)))));
afterAll(() => new Promise((r) => server.close(r)));

let dir;
beforeEach(() => {
  Object.assign(remote, { chainId: 40204, genesis: GENESIS, head: 1200, seed: "main", missing: null, decoyEvery: null });
  dir = fs.mkdtempSync(path.join(os.tmpdir(), "cold-sync-test-"));
});
afterEach(() => fs.rmSync(dir, { recursive: true, force: true }));

const BOOTNODES = [
  "noise_" + "1".repeat(64) + "@boot-a.example:30303",
  "noise_" + "2".repeat(64) + "@boot-b.example:30303",
];
function profile(over = {}) {
  return {
    label: "test network",
    chainId: 40204,
    genesisHash: GENESIS,
    nodeNetwork: "testnet",
    rpcUrl,
    explorerUrl: "https://explorer.example",
    faucetUrl: "https://faucet.example",
    bootnodes: BOOTNODES,
    addressBook: "addresses/40204.json",
    genesisProfile: "testnet_beta",
    pending: {},
    ...over,
  };
}
function writeJson(name, obj) {
  const p = path.join(dir, name);
  fs.writeFileSync(p, JSON.stringify(obj, null, 2));
  return p;
}
function book(genesis = GENESIS) {
  return writeJson("book.json", { chainId: 40204, genesisHash: genesis, addresses: { ValidatorRegistry: REGISTRY } });
}
// The node: a wrapper that points the stand-in at its behaviour file (cold-sync.py gives the node a
// minimal env, so the wrapper is the only way in).
function node(behaviour = {}) {
  const record = path.join(dir, "record.json");
  const cfg = writeJson("stand-in.json", { record, chainId: 40204, genesis: GENESIS, rate: 2000, peers: 3, ...behaviour });
  const bin = path.join(dir, "citrate");
  fs.writeFileSync(bin, `#!/bin/sh\nSTAND_IN_CFG='${cfg}' exec ${PY} '${standIn}' "$@"\n`, { mode: 0o755 });
  return { bin, record };
}
async function run({ prof = profile(), behaviour, extra = [], blocks = "1000", every = "100", timeout = "20" } = {}) {
  const { bin, record } = node(behaviour);
  const out = path.join(dir, "out");
  const args = [script, "--node-bin", bin, "--profile", writeJson("profile.json", prof), "--address-book", book(), "--out", out];
  args.push("--every", every, "--timeout", timeout, "--poll", "0.2", "--genesis-wait", "10");
  if (blocks !== null) args.push("--blocks", blocks);
  const r = await py([...args, ...extra]);
  const vp = path.join(out, "verdict.json");
  const v = fs.existsSync(vp) ? JSON.parse(fs.readFileSync(vp, "utf8")) : null;
  const rec = fs.existsSync(record) ? JSON.parse(fs.readFileSync(record, "utf8")) : null;
  return { ...r, v, rec };
}
const failed = (v) => v.rules.filter((r) => r.status === "FAIL").map((r) => r.rule).sort();
const status = (v, rule) => v.rules.find((r) => r.rule === rule)?.status;
const nodeRs = fs.readFileSync(path.join(repo, "src-tauri", "src", "node.rs"), "utf8");
const rsConst = (name) => nodeRs.match(new RegExp(`const ${name}: &str = "([^"]*)";`))[1];

describe("launch spec (NodeManager::build_spec)", () => {
  it("starts the node like the app: profile network and config, key file, consensus env, temp dir removed", async () => {
    const r = await run();
    expect(r.v.verdict, r.stderr).toBe("PASS");
    expect(r.code).toBe(0);
    const { argv, env, config } = r.rec;
    // --network from the profile, --data-dir, --config inside the data dir; no mining flags.
    expect(argv[argv.indexOf("--network") + 1]).toBe("testnet");
    const dataDir = argv[argv.indexOf("--data-dir") + 1];
    expect(argv[argv.indexOf("--config") + 1]).toBe(path.join(dataDir, "node.toml"));
    expect(argv).not.toContain("--mine");
    expect(argv).not.toContain("--coinbase");
    // The config: the profile's bootnodes and chain id, the data dir, RPC on loopback and not the
    // app's ports (the member's own node may be running), mining off.
    const nodes = [...config.match(/bootstrap_nodes\s*=\s*\[([^\]]*)\]/s)[1].matchAll(/"([^"]+)"/g)].map((m) => m[1]);
    expect(nodes).toEqual(BOOTNODES);
    expect(config).toMatch(/^chain_id = 40204$/m);
    // RC52: the genesis profile the node must build its block 0 from.
    expect(config.match(/\[chain\][^[]*?^genesis_profile\s*=\s*"([^"]*)"/ms)?.[1]).toBe("testnet_beta");
    expect(config).toContain(`data_dir = "${dataDir}"`);
    const rpcAddr = config.match(/\[rpc\][^[]*?listen_addr\s*=\s*"([^"]+)"/s)[1];
    expect(rpcAddr).toMatch(/^127\.0\.0\.1:\d+$/);
    expect(rpcAddr).not.toBe("127.0.0.1:8545");
    expect(config.match(/\[network\][^[]*?listen_addr\s*=\s*"([^"]+)"/s)[1]).not.toMatch(/:30303$/);
    expect(config.match(/\[mining\][^[]*?^enabled\s*=\s*(\w+)/ms)[1]).toBe("false");
    // The storage key is a file path (owner-only, 64 hex digits), never the key itself.
    expect(env.CITRATE_STORAGE_KEY).toBeUndefined();
    expect(env.CITRATE_STORAGE_KEY_FILE.startsWith(dataDir)).toBe(true);
    expect(r.rec.keyFileMode).toBe("0o600");
    expect(r.rec.keyLen).toBe(64);
    // Consensus env: the values node.rs gives the sidecar, the registry from the address book.
    expect(env.CITRATE_BLOCK_V2).toBe(rsConst("NODE_BLOCK_V2_VALUE"));
    expect(env.CITRATE_VALIDATOR_ACTIVATION_HEIGHT).toBe(rsConst("NODE_VALIDATOR_ACTIVATION_HEIGHT_VALUE"));
    expect(env.CITRATE_VALIDATOR_REGISTRY).toBe(REGISTRY);
    expect(env.CITRATE_DAG_PRUNE_RETAIN).toBeUndefined();
    expect(env.NO_COLOR).toBe("1");
    // Nothing reaches the member's home or the app's metrics port.
    expect(env.HOME.startsWith(dataDir) || dataDir.startsWith(path.dirname(env.HOME))).toBe(true);
    expect(env.HOME).not.toBe(os.homedir());
    expect(env.CITRATE_METRICS_ADDR).toMatch(/^127\.0\.0\.1:\d+$/);
    expect(env.CITRATE_METRICS_ADDR).not.toBe("127.0.0.1:9090");
    // The temp dir is gone and the verdict says so.
    expect(fs.existsSync(dataDir)).toBe(false);
    expect(r.v.data_dir_removed).toBe(true);
    expect(status(r.v, "cleanup")).toBe("ok");
  });

  it("RC52: on a genesis with compiled activations (r1006) it names that genesis profile and passes no validator override", async () => {
    const r = await run({ prof: profile({ genesisProfile: "r1006" }) });
    expect(r.v.verdict, r.stderr).toBe("PASS");
    const { env, config } = r.rec;
    expect(config.match(/\[chain\][^[]*?^genesis_profile\s*=\s*"([^"]*)"/ms)?.[1]).toBe("r1006");
    // The node refuses an env activation height or registry that differs from the compiled ones.
    expect(env.CITRATE_VALIDATOR_ACTIVATION_HEIGHT).toBeUndefined();
    expect(env.CITRATE_VALIDATOR_REGISTRY).toBeUndefined();
    expect(env.CITRATE_BLOCK_V2).toBe(rsConst("NODE_BLOCK_V2_VALUE"));
    expect(r.v.node.env).not.toContain("CITRATE_VALIDATOR_ACTIVATION_HEIGHT");
  });

  for (const [what, gp] of [["a genesis profile the bundled node does not build", "r9999"], ["no genesis profile", null],
    ["a malformed genesis profile", "r1006\"\nx = 1"]]) {
    it(`RC52: refuses a profile with ${what}, and starts no node`, async () => {
      const r = await run({ prof: profile({ genesisProfile: gp }) });
      expect(r.code).toBe(2);
      expect(r.rec).toBeNull();
      expect(r.v.rules[0]).toMatchObject({ rule: "profile", status: "FAIL" });
      expect(r.v.rules[0].detail).toMatch(/genesisProfile/);
    });
  }

  it("has no option to point the node at an existing data dir", async () => {
    const r = await py([script, "--help"]);
    expect(r.code).toBe(0);
    expect(r.stdout).not.toMatch(/--data-dir/);
    expect(r.stdout).toMatch(/--profile/);
  });

  it("refuses an RPC port that something already answers on, and starts no node", async () => {
    const busy = net.createServer();
    await new Promise((res) => busy.listen(0, "127.0.0.1", res));
    const r = await run({ extra: ["--rpc-port", String(busy.address().port)] });
    await new Promise((res) => busy.close(res));
    expect(r.code).toBe(2);
    expect(r.rec).toBeNull();
  });
});

describe("the genesis gate", () => {
  it("refuses a profile whose genesis is not the network's block 0, and does not sync", async () => {
    remote.genesis = OTHER;
    const r = await run();
    expect(r.code).toBe(1);
    expect(r.v.verdict).toBe("FAIL");
    expect(failed(r.v)).toContain("endpoint_identity");
    expect(status(r.v, "reaches_target")).toBe("skip");
    expect(r.v.reasons.join(" ")).toMatch(/block 0/);
  });

  it("refuses a node whose block 0 is not the profile's genesis (wrong chain), and stops it", async () => {
    const r = await run({ behaviour: { genesis: OTHER } });
    expect(r.code).toBe(1);
    expect(failed(r.v)).toEqual(["local_genesis"]);
    expect(status(r.v, "reaches_target")).toBe("skip");
    expect(r.v.node.local_genesis).toBe(OTHER);
    expect(r.v.data_dir_removed).toBe(true);
  });

  it("refuses a node on another chain id", async () => {
    const r = await run({ behaviour: { chainId: 1 } });
    expect(r.code).toBe(1);
    expect(failed(r.v)).toContain("node_chain_id");
  });

  it("refuses a network RPC on another chain id", async () => {
    remote.chainId = 1;
    const r = await run();
    expect(failed(r.v)).toContain("endpoint_identity");
  });

  it("refuses an address book for another genesis", async () => {
    const { bin, record } = node();
    const out = path.join(dir, "out");
    const r = await py([script, "--node-bin", bin, "--profile", writeJson("profile.json", profile()), "--address-book", book(OTHER), "--out", out, "--blocks", "10"]);
    expect(r.code).toBe(2);
    expect(fs.existsSync(record)).toBe(false);
    expect(JSON.parse(fs.readFileSync(path.join(out, "verdict.json"), "utf8")).rules[0]).toMatchObject({ rule: "profile", status: "FAIL" });
  });

  const notReady = {
    "a null genesis (a K6 profile that is not set yet)": { genesisHash: null },
    "a malformed genesis": { genesisHash: "0x1234" },
    "an all-zero genesis": { genesisHash: "0x" + "0".repeat(64) },
    "no bootnodes": { bootnodes: [] },
    "null bootnodes": { bootnodes: null },
    "a null node network": { nodeNetwork: null },
    "a chain id that is not a number": { chainId: "40204" },
    "a pending entry": { pending: { genesisHash: "recorded at the genesis cut" } },
  };
  for (const [name, over] of Object.entries(notReady)) {
    it(`refuses a profile with ${name}, and starts no node`, async () => {
      const r = await run({ prof: profile(over) });
      expect(r.code).toBe(2);
      expect(r.rec).toBeNull();
      expect(r.v.verdict).toBe("FAIL");
      expect(r.v.rules[0]).toMatchObject({ rule: "profile", status: "FAIL" });
    });
  }

  it("reads a named profile out of a profiles.json, and refuses an unknown name", async () => {
    const all = { active: "a", profiles: { a: profile(), b: profile({ genesisHash: null }) } };
    const p = writeJson("profiles.json", all);
    const { bin } = node();
    const out = path.join(dir, "out");
    const base = [script, "--node-bin", bin, "--profiles", p, "--address-book", book(), "--out", out, "--blocks", "300", "--every", "100", "--poll", "0.2"];
    expect((await py(base)).code).toBe(0); // the active one
    expect(JSON.parse(fs.readFileSync(path.join(out, "verdict.json"), "utf8")).profile.name).toBe("a");
    expect((await py([...base, "--name", "b"])).code).toBe(2);
    expect((await py([...base, "--name", "nope"])).code).toBe(2);
  });
});

describe("sync", () => {
  it("passes a bounded run and checks every K blocks against the network", async () => {
    const r = await run({ blocks: "1000", every: "100" });
    expect(r.code).toBe(0);
    expect(r.v.mode).toBe("bounded");
    expect(r.v.sync.final_head).toBeGreaterThanOrEqual(1000);
    expect(r.v.sync.checked_heights.slice(0, 3)).toEqual([0, 100, 200]);
    expect(r.v.sync.checked_heights.length).toBeGreaterThanOrEqual(11);
    expect(r.v.sync.mismatches).toEqual([]);
    expect(failed(r.v)).toEqual([]);
  });

  it("passes a to-tip run once the head reaches the network's", async () => {
    remote.head = 1500;
    const r = await run({ blocks: null, extra: ["--tip-slack", "5"] });
    expect(r.code, r.stderr).toBe(0);
    expect(r.v.mode).toBe("tip");
    expect(r.v.sync.final_head).toBeGreaterThanOrEqual(1495);
  });

  it("fails a fork and names the first height that disagrees (the walks' common ancestor + 1)", async () => {
    const r = await run({ behaviour: { forkAt: 450 } });
    expect(r.code).toBe(1);
    expect(failed(r.v)).toEqual(["agrees"]);
    expect(r.v.sync.mismatches[0].height).toBe(450);
    expect(r.v.sync.common_ancestor).toMatchObject({ height: 449 });
  });

  // RC52 / chain#421 class: eth_getBlockByNumber is not canonical on r1006, even deep below the tip
  // (DGX G4, fed#331): by number two sides can name different blocks at a height on the same chain.
  it("passes when by-number answers differ but the selected-parent chains are the same", async () => {
    remote.decoyEvery = 100;
    const r = await run({ blocks: "1000", every: "100", behaviour: { decoyEvery: 300 } });
    expect(r.code, r.stderr).toBe(0);
    expect(failed(r.v)).toEqual([]);
    expect(r.v.sync.mismatches).toEqual([]);
    expect(r.v.sync.anchor?.height).toBe(Math.min(r.v.sync.final_head, r.v.sync.remote_head) - r.v.sync.lag);
    expect(r.v.sync.walk?.reaches_genesis).toBe(true);
  });

  it("fails when the node's chain is another chain, even if by-number answers agree", async () => {
    const r = await run({ behaviour: { forkAt: 1, forkSeed: "other" } });
    expect(r.code).toBe(1);
    expect(failed(r.v)).toEqual(["agrees"]);
    expect(r.v.sync.mismatches[0].height).toBe(1);
  });

  it("fails a head that stalls before the target", async () => {
    const r = await run({ behaviour: { headMax: 50 }, timeout: "3" });
    expect(r.code).toBe(1);
    expect(failed(r.v)).toContain("reaches_target");
    expect(r.v.data_dir_removed).toBe(true);
  });

  it("fails a node that exits during the run", async () => {
    const r = await run({ behaviour: { rate: 50, exitAfter: 1.5 }, timeout: "10" });
    expect(r.code).toBe(1);
    expect(failed(r.v)).toContain("node_alive");
  });

  it("fails a node that never answers RPC", async () => {
    const r = await run({ behaviour: { noRpc: true }, timeout: "3", extra: ["--genesis-wait", "2"] });
    expect(r.code).toBe(1);
    expect(failed(r.v)).toContain("local_genesis");
    expect(r.v.data_dir_removed).toBe(true);
  });

  it("fails a node with no peers", async () => {
    const r = await run({ behaviour: { peers: 0 } });
    expect(failed(r.v)).toContain("peers");
  });

  it("fails a data dir with no encryption at rest", async () => {
    const r = await run({ behaviour: { encMeta: false } });
    expect(failed(r.v)).toEqual(["at_rest"]);
  });

  it("fails a node binary whose sha256 is not the expected one, and starts no node", async () => {
    const r = await run({ extra: ["--expect-sha256", "0".repeat(64)] });
    expect(r.code).toBe(2);
    expect(r.rec).toBeNull();
  });
});

describe("every height is covered by the selected-parent walk (review round 1, item 1; RC52)", () => {
  for (const side of ["network", "node"]) {
    it(`fails when the ${side} cannot serve a block on its selected-parent walk`, async () => {
      remote.head = 2000;
      if (side === "network") remote.missing = [1300, 1999];
      const r = await run({ blocks: "1500", every: "100", timeout: "4", behaviour: side === "node" ? { missing: [1, 1000] } : {} });
      expect(r.code).toBe(1);
      expect(r.v.verdict).toBe("FAIL");
      expect(failed(r.v)).toEqual(["agrees"]);
      expect(r.v.sync.walk?.error).toMatch(new RegExp(`the ${side}'s selected-parent walk stopped at height \\d+`));
    });
  }

  it("covers every height up to the last safe height (the anchor), listing every K-th and the anchor", async () => {
    const r = await run({ blocks: "1000", every: "100" });
    expect(r.code).toBe(0);
    const safe = Math.min(r.v.sync.final_head, r.v.sync.remote_head) - r.v.sync.lag;
    const want = [];
    for (let h = 0; h <= safe; h += 100) want.push(h);
    if (want[want.length - 1] !== safe) want.push(safe);
    expect(r.v.sync.checked_heights).toEqual(want);
    // Every height 0..anchor is covered: both walks met at the anchor and the node's reached block 0.
    expect(r.v.sync.anchor.height).toBe(safe);
    expect(r.v.sync.walk).toMatchObject({ reaches_genesis: true, error: null });
    expect(r.v.sync.walk.local_blocks).toBeGreaterThanOrEqual(safe);
  });
});

describe("every error path writes a verdict (review round 1, item 2)", () => {
  it("accepts a relative --node-bin", async () => {
    const { bin } = node();
    const out = path.join(dir, "out");
    const args = [script, "--node-bin", "./" + path.basename(bin), "--profile", writeJson("profile.json", profile()),
      "--address-book", book(), "--out", out, "--blocks", "300", "--every", "100", "--poll", "0.2"];
    const r = await py(args, { cwd: dir });
    expect(r.code, r.stderr).toBe(0);
    const v = JSON.parse(fs.readFileSync(path.join(out, "verdict.json"), "utf8"));
    expect(v.verdict).toBe("PASS");
    expect(path.isAbsolute(v.node.bin)).toBe(true);
  });

  it("writes a FAIL verdict when the node cannot be started, and removes the temp dir", async () => {
    const { bin } = node();
    fs.writeFileSync(bin, "#!/nonexistent/interpreter\n", { mode: 0o755 });
    const out = path.join(dir, "out");
    const work = fs.mkdtempSync(path.join(dir, "work-"));
    const r = await py([script, "--node-bin", bin, "--profile", writeJson("profile.json", profile()),
      "--address-book", book(), "--out", out, "--blocks", "300", "--work-root", work]);
    expect(r.code).toBe(1);
    expect(r.stderr).not.toMatch(/Traceback/);
    const v = JSON.parse(fs.readFileSync(path.join(out, "verdict.json"), "utf8"));
    expect(v.verdict).toBe("FAIL");
    expect(failed(v)).toContain("node_alive");
    expect(fs.readdirSync(work)).toEqual([]);
  });

  it("writes a FAIL verdict when the member config cannot be read", async () => {
    const { bin, record } = node();
    const out = path.join(dir, "out");
    const r = await py([script, "--node-bin", bin, "--profile", writeJson("profile.json", profile()),
      "--address-book", book(), "--out", out, "--member-config", path.join(dir, "missing.toml")]);
    expect(r.code).toBe(2);
    expect(r.stderr).not.toMatch(/Traceback/);
    expect(fs.existsSync(record)).toBe(false);
    const v = JSON.parse(fs.readFileSync(path.join(out, "verdict.json"), "utf8"));
    expect(v.rules.find((x) => x.rule === "launch").status).toBe("FAIL");
  });
});

describe("a signal stops the node and removes the temp dir (review round 1, item 3)", () => {
  for (const sig of ["SIGHUP", "SIGINT", "SIGTERM"]) {
    it(`${sig}: the node is stopped, the temp dir (with the key file) is gone, a verdict is written`, async () => {
      const { bin, record } = node({ rate: 5 });
      const out = path.join(dir, "out");
      const work = fs.mkdtempSync(path.join(dir, "work-"));
      const child = spawn(PY, [script, "--node-bin", bin, "--profile", writeJson("profile.json", profile()),
        "--address-book", book(), "--out", out, "--blocks", "100000", "--timeout", "60", "--poll", "0.2",
        "--work-root", work], { stdio: "ignore" });
      const exited = new Promise((res) => child.on("exit", (code, signal) => res({ code, signal })));
      let rec = null;
      for (let i = 0; i < 100 && !(rec && rec.argv.includes("--network")); i++) {
        await new Promise((res) => setTimeout(res, 100));
        try { rec = JSON.parse(fs.readFileSync(record, "utf8")); } catch { rec = null; }
      }
      expect(rec?.argv).toContain("--network");
      await new Promise((res) => setTimeout(res, 500));
      child.kill(sig);
      const { code, signal } = await exited;
      expect(signal).toBeNull();
      expect(code).toBe(1);
      let alive = true;
      try { process.kill(rec.pid, 0); } catch { alive = false; }
      if (alive) process.kill(rec.pid, "SIGKILL"); // never leave a stand-in running
      expect(alive).toBe(false);
      expect(fs.readdirSync(work)).toEqual([]);
      const v = JSON.parse(fs.readFileSync(path.join(out, "verdict.json"), "utf8"));
      expect(v.verdict).toBe("FAIL");
      expect(v.rules.find((x) => x.rule === "node_alive").detail).toMatch(/interrupted/);
      expect(v.data_dir_removed).toBe(true);
    });
  }
});

describe("a node that dies late is not read as alive (#524)", () => {
  // Both heads are capped at 350 and the target is 344 + lag 6 = 350, so the run ends with both
  // heads at 350 and the anchor is exactly 344. The node reads blocks by hash only in the final
  // selected-parent walk, which ends at block 0: it exits right after serving that one (RC52).
  it("fails node_alive when the node exits during the final agreement checks", async () => {
    remote.head = 350;
    const r = await run({ blocks: "344", every: "100", behaviour: { exitAfterHashOf: 0, headMax: 350 } });
    expect(r.code).toBe(1);
    expect(r.v.verdict).toBe("FAIL");
    expect(r.v.sync.checked_heights).toEqual([0, 100, 200, 300, 344]);
    expect(failed(r.v)).toEqual(["node_alive"]);
    expect(r.v.rules.find((x) => x.rule === "node_alive").detail).toMatch(/exited with code 4 during the final checks/);
    expect(r.v.data_dir_removed).toBe(true);
  });

  it("fails node_alive when the node exits right after the liveness probe (caught at cleanup)", async () => {
    remote.head = 350;
    const r = await run({ blocks: "344", every: "100", behaviour: { exitAfterProbeAfterHashOf: 0, headMax: 350 } });
    expect(r.code).toBe(1);
    expect(failed(r.v)).toEqual(["node_alive"]);
    expect(r.v.rules.find((x) => x.rule === "node_alive").detail).toMatch(/exited on its own with code 5/);
    expect(r.v.data_dir_removed).toBe(true);
  });
});

describe("tripwire: the network lives in the K6 profile and the member config only", () => {
  const allowed = new Set(["src-tauri/network/profiles.json", "src-tauri/config/member-node.toml"]);
  const roots = ["scripts", "src-tauri/scripts", ".github"];
  const files = [];
  const walk = (d) => {
    for (const e of fs.readdirSync(path.join(repo, d), { withFileTypes: true })) {
      const rel = path.join(d, e.name);
      if (e.isDirectory()) walk(rel);
      else if (/\.(sh|py|mjs|js|ts|ya?ml|toml|json)$/.test(e.name) && !/\.test\.mjs$/.test(e.name)) files.push(rel);
    }
  };
  for (const r of roots) if (fs.existsSync(path.join(repo, r))) walk(r);

  it("no script carries a bootnode key or launches a node with a literal --network testnet", () => {
    const hits = [];
    for (const f of files) {
      if (allowed.has(f)) continue;
      const t = fs.readFileSync(path.join(repo, f), "utf8");
      if (/noise_[0-9a-f]{64}@/.test(t)) hits.push(`${f}: bootnode key`);
      if (/--network[ =]testnet/.test(t)) hits.push(`${f}: --network testnet`);
    }
    expect(files.length).toBeGreaterThan(20);
    expect(hits).toEqual([]);
  });
});
