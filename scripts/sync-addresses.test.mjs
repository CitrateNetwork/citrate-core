// @vitest-environment node
//
// HUP-S7.1 (federation F-4): scripts/sync-addresses.py, the generator of the app's embedded
// 40204 address book.
//
// 1. CapsuleRegistry and InferenceRouter are optional pins: emitted when the canonical book has
//    them, left out (not guessed) when it does not.
// 2. Fail closed: a missing required name, an entry that is not an address, a pin with no code on
//    the chain (with --rpc), a wrong chain id or genesis each stop the sync, and nothing is written.
// 3. The chain is a local JSON-RPC stand-in here; scripts/anvil-sync-addresses.sh runs the same
//    checks against an anvil fork after a real DeployHupRegistries deploy.
import { describe, expect, it, beforeAll, afterAll, beforeEach, afterEach } from "vitest";
import { execFile } from "node:child_process";
import fs from "node:fs";
import http from "node:http";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const script = path.join(here, "sync-addresses.py");
const GENESIS = "0x" + "9e".repeat(32);

const addr = (b) => "0x" + b.repeat(20);
const REQUIRED = {
  CitrateMemberSBT: addr("01"),
  MembershipStakeVault: addr("02"),
  ValidatorRegistry: addr("03"),
  CitrateWalletFactory: addr("04"),
  LiquidStakingPool: addr("05"),
  CitratePaymaster: addr("06"),
  IPFSIncentivesV3: addr("07"),
  ModelRegistry: addr("08"),
  SkillRegistry: addr("09"),
};
const HUP = {
  AgentSBT: addr("a1"),
  OrganizationSBT: addr("a2"),
  AnchorRegistry: addr("a3"),
  BenchmarkRegistry: addr("a4"),
  CapsuleRegistry: addr("a5"),
  InferenceRouter: addr("a6"),
};

function book(contracts = { ...REQUIRED, ...HUP }) {
  return { chainId: 40204, contracts, aaStack: {} };
}

function run(args) {
  return new Promise((resolve) => {
    execFile("python3", [script, ...args], (err, stdout, stderr) => {
      resolve({ code: err ? (typeof err.code === "number" ? err.code : 1) : 0, stdout, stderr });
    });
  });
}

// A JSON-RPC stand-in for the chain: chain id, block 0 and code per address.
const chain = { id: 40204, genesis: GENESIS, noCode: new Set() };
let server;
let rpcUrl;

beforeAll(async () => {
  server = http.createServer((req, res) => {
    let body = "";
    req.on("data", (c) => (body += c));
    req.on("end", () => {
      const { id, method, params } = JSON.parse(body);
      let result = null;
      if (method === "eth_chainId") result = "0x" + chain.id.toString(16);
      else if (method === "eth_getBlockByNumber") result = { hash: chain.genesis };
      else if (method === "eth_getCode") result = chain.noCode.has(params[0].toLowerCase()) ? "0x" : "0x6080";
      res.setHeader("content-type", "application/json");
      res.end(JSON.stringify({ jsonrpc: "2.0", id, result }));
    });
  });
  await new Promise((r) => server.listen(0, "127.0.0.1", r));
  rpcUrl = `http://127.0.0.1:${server.address().port}`;
});

afterAll(() => new Promise((r) => server.close(r)));

let dir;
let bookPath;
let outPath;
beforeEach(() => {
  dir = fs.mkdtempSync(path.join(os.tmpdir(), "sync-addresses-"));
  bookPath = path.join(dir, "40204.json");
  outPath = path.join(dir, "app-40204.json");
  chain.id = 40204;
  chain.genesis = GENESIS;
  chain.noCode = new Set();
});
afterEach(() => fs.rmSync(dir, { recursive: true, force: true }));

const writeBook = (b) => fs.writeFileSync(bookPath, JSON.stringify(b, null, 2));
const sync = (...extra) => run(["--book", bookPath, "--genesis", GENESIS, "--out", outPath, ...extra]);

describe("optional HUP pins", () => {
  it("emits CapsuleRegistry and InferenceRouter when the book has them", async () => {
    writeBook(book());
    const r = await sync("--rpc", rpcUrl, "--with-inference-router");
    expect(r.code).toBe(0);
    const out = JSON.parse(fs.readFileSync(outPath, "utf8"));
    expect(out.chainId).toBe(40204);
    expect(out.genesisHash).toBe(GENESIS);
    for (const [name, a] of Object.entries({ ...REQUIRED, ...HUP })) expect(out.addresses[name]).toBe(a);
    expect(r.stdout).toContain("CapsuleRegistry, InferenceRouter");
  });

  it("withholds InferenceRouter unless --with-inference-router (the route stays off)", async () => {
    writeBook(book());
    const r = await sync("--rpc", rpcUrl);
    expect(r.code).toBe(0);
    const out = JSON.parse(fs.readFileSync(outPath, "utf8"));
    expect(out.addresses).not.toHaveProperty("InferenceRouter");
    expect(out.addresses.CapsuleRegistry).toBe(HUP.CapsuleRegistry);
    expect(r.stdout).toContain("withheld InferenceRouter");
  });

  it("leaves an absent optional pin out instead of guessing", async () => {
    const { CapsuleRegistry: _c, InferenceRouter: _i, ...rest } = { ...REQUIRED, ...HUP };
    writeBook(book(rest));
    const r = await sync("--rpc", rpcUrl);
    expect(r.code).toBe(0);
    const out = JSON.parse(fs.readFileSync(outPath, "utf8"));
    expect(out.addresses).not.toHaveProperty("CapsuleRegistry");
    expect(out.addresses).not.toHaveProperty("InferenceRouter");
  });

  it("--check verifies without writing", async () => {
    writeBook(book());
    const r = await sync("--check", "--rpc", rpcUrl);
    expect(r.code).toBe(0);
    expect(fs.existsSync(outPath)).toBe(false);
  });
});

describe("fails closed", () => {
  const refused = async (args, pattern) => {
    const r = await sync(...args);
    expect(r.code).toBe(1);
    expect(r.stderr).toMatch(pattern);
    expect(fs.existsSync(outPath)).toBe(false);
  };

  it("on a pin with no code on the chain (--check too)", async () => {
    writeBook(book());
    chain.noCode = new Set([HUP.InferenceRouter]);
    await refused(["--rpc", rpcUrl], /no code on the live chain at: InferenceRouter/);
    await refused(["--check", "--rpc", rpcUrl], /InferenceRouter/);
    chain.noCode = new Set([HUP.CapsuleRegistry]);
    await refused(["--check", "--rpc", rpcUrl], /CapsuleRegistry/);
  });

  it("on an optional entry that is not an address", async () => {
    writeBook(book({ ...REQUIRED, ...HUP, InferenceRouter: "0x1234" }));
    await refused(["--check"], /not an address .*contracts\.InferenceRouter/);
    writeBook(book({ ...REQUIRED, ...HUP, CapsuleRegistry: null }));
    await refused([], /contracts\.CapsuleRegistry/);
  });

  it("on a missing required name", async () => {
    const { SkillRegistry: _s, ...rest } = { ...REQUIRED, ...HUP };
    writeBook(book(rest));
    await refused(["--check"], /SkillRegistry is missing/);
  });

  it("on two names sharing an address", async () => {
    writeBook(book({ ...REQUIRED, ...HUP, InferenceRouter: HUP.CapsuleRegistry }));
    await refused(["--check"], /share address/);
  });

  it("on the wrong chain or genesis", async () => {
    writeBook(book());
    chain.id = 1;
    await refused(["--rpc", rpcUrl], /not 40204/);
    chain.id = 40204;
    chain.genesis = "0x" + "01".repeat(32);
    await refused(["--rpc", rpcUrl], /genesis/);
  });
});
