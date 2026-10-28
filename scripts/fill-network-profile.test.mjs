// @vitest-environment node
//
// K6 (RC52 WP-S1-K6, D-2): the r1006 network profile, the one-command post-reroll fill
// (scripts/fill-network-profile.sh) and the release tripwire (scripts/check-network-profile.sh
// --release, used by the RC build gate G0).
//
// 1. The fill writes only the reroll values (genesisHash, addressBookSha256, minChainRev) and
//    their `pending` reasons; every other byte of profiles.json is kept, and the result is ready
//    by the app's own rule (src/network/profile.ts).
// 2. Every bad input is refused and the file is left byte-identical.
// 3. The release check refuses a build whose active profile has an unset value (negative
//    control: the shipped file with `active` flipped to the unfilled r1006), and passes the
//    shipped file (positive control).
// 4. The Python rule and the TypeScript rule agree on malformed values, including a value with a
//    trailing newline (Python's `$` matches before a final "\n", so every pattern is matched with
//    re.fullmatch; a tripwire below keeps it that way).
//
// Everything runs on copies in a fresh temp dir; nothing touches the repo or the network.
import { describe, expect, it, beforeEach, afterEach } from "vitest";
import { execFileSync, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { profileProblems } from "../src/network/profile";

const here = path.dirname(fileURLToPath(import.meta.url));
const repo = path.resolve(here, "..");
const fill = path.join(here, "fill-network-profile.sh");
const check = path.join(here, "check-network-profile.sh");
const realProfiles = path.join(repo, "src-tauri", "network", "profiles.json");
// The values the RC52 fill wrote into the shipped r1006 profile, and their reasons before it.
const R1006_FILLED = {
  genesisHash: "0xa3651a272fe411948cbb4cf7cca4cee44be4248d32ab4376cea7636821c6cdd4",
  addressBookSha256: "724e3843f677588a6aac680554dd1f2db768d5c3ca2bea18bdc0c173c100b90b",
  // The node rev is read from build-sidecar.sh (the pin the profile must equal), not repeated here,
  // so a node rev change is the two pin lines only.
  minChainRev: /^MIN_CHAIN_REV="([0-9a-f]{40})"/m.exec(fs.readFileSync(path.join(here, "build-sidecar.sh"), "utf8"))[1],
};
const PREFILL_REASONS = {
  genesisHash: "recorded at the r1006 reroll (block 0 of the fleet after fleet_agree.sh); fill with scripts/fill-network-profile.sh",
  addressBookSha256: "the canonical address book exported at the r1006 reroll; fill with scripts/fill-network-profile.sh",
  minChainRev: "the citrate-chain rev the r1006 fleet runs (WP-S1-REPIN); fill with scripts/fill-network-profile.sh",
};
/** The shipped profiles as they were before the RC52 fill (r1006 unfilled, alpha-r1005 active):
 * the fill and check cases below run on this, so they keep exercising an unfilled profile. */
function prefillProfiles() {
  const d = JSON.parse(fs.readFileSync(realProfiles, "utf8"));
  d.active = "alpha-r1005";
  Object.assign(d.profiles.r1006, { genesisHash: null, addressBookSha256: null, minChainRev: null, pending: { ...PREFILL_REASONS } });
  return d;
}
const realSidecar = path.join(here, "build-sidecar.sh");

const GENESIS = "0x" + "6a".repeat(32);
// Block 0 of r1006: what the bundled node builds for genesis profile r1006 (citrate-chain 9a32147e).
const R1006_GENESIS = "0xa3651a272fe411948cbb4cf7cca4cee44be4248d32ab4376cea7636821c6cdd4";
const REV = "1f".repeat(20);
const addr = (b) => "0x" + b.repeat(20);
const canonicalBook = (over = {}) => ({
  $schema: "./schema.json",
  chainId: 40204,
  chainName: "Citrate Network",
  deployer: addr("dd"),
  contracts: {
    CitrateMemberSBT: addr("01"),
    MembershipStakeVault: addr("02"),
    ValidatorRegistry: addr("03"),
    CitrateWalletFactory: addr("04"),
    LiquidStakingPool: addr("05"),
    IPFSIncentivesV3: addr("07"),
    ModelRegistry: addr("08"),
    SkillRegistry: addr("09"),
    ContributionAccounting: addr("0a"),
  },
  aaStack: { CitratePaymaster: addr("06") },
  genesis: { ArachnidDeterministicDeployer: addr("4e") },
  ...over,
});

let dir;
let profiles;
let bookPath;
const env = () => ({ PATH: process.env.PATH, HOME: dir, LANG: "C.UTF-8" });

function run(script, args) {
  const r = spawnSync("bash", [script, ...args], { env: env(), encoding: "utf8" });
  return { code: r.status, out: r.stdout, err: r.stderr };
}
const fillArgs = (over = {}) => {
  const o = { profile: "r1006", genesis: GENESIS, book: bookPath, rev: REV, extra: [], ...over };
  return [o.profile, "--genesis-hash", o.genesis, "--address-book", o.book, "--min-chain-rev", o.rev, ...o.extra,
    "--profiles", profiles];
};
const writeBook = (b, name = "book.json") => {
  const p = path.join(dir, name);
  fs.writeFileSync(p, typeof b === "string" ? b : JSON.stringify(b, null, 2) + "\n");
  return p;
};
const read = () => JSON.parse(fs.readFileSync(profiles, "utf8"));
// Script runs take ~0.5-3 s each; under parallel cargo load the 5 s default is too tight.
const SLOW = { timeout: 30_000 };

beforeEach(() => {
  dir = fs.mkdtempSync("/tmp/dgx-fillprof-");
  profiles = path.join(dir, "profiles.json");
  fs.writeFileSync(profiles, JSON.stringify(prefillProfiles(), null, 2) + "\n");
  bookPath = writeBook(canonicalBook());
});
afterEach(() => {
  if (dir && dir.startsWith("/tmp/dgx-fillprof-")) fs.rmSync(dir, { recursive: true, force: true });
});

describe("the shipped r1006 profile", () => {
  it("is filled with the reroll values (RC52 fill) and is the active profile, ready by the app's rule", () => {
    const d = JSON.parse(fs.readFileSync(realProfiles, "utf8"));
    const p = d.profiles.r1006;
    expect(d.active).toBe("r1006");
    expect(p).toMatchObject({ ...R1006_FILLED, genesisProfile: "r1006", pending: {} });
    expect(p.genesisHash).toBe(d.nodeGenesisProfiles.r1006.genesisHash);
    // Owner decision: the bootnodes stay boot1-3 + rpc.citrate.ai.
    expect(p.bootnodes.map((b) => b.split("@")[1])).toEqual(
      ["boot1.citrate.ai:30303", "boot2.citrate.ai:30303", "boot3.citrate.ai:30303", "rpc.citrate.ai:30303"]);
    expect(profileProblems({ id: "r1006", ...p })).toEqual([]);
    // Restoring the three reroll values to null gives back exactly the pre-fill profile below.
    expect(prefillProfiles().profiles.r1006).toMatchObject({ ...p, genesisHash: null, addressBookSha256: null, minChainRev: null });
  });

  it("before the fill: every value known then and only the reroll values unset, each with a reason", () => {
    const p = prefillProfiles().profiles.r1006;
    expect(p).toBeDefined();
    expect(p.chainId).toBe(40204);
    expect(p.nodeNetwork).toBe("testnet");
    expect(p.rpcUrl).toBe("https://rpc.citrate.ai");
    expect(p.explorerUrl).toBe("https://explorer.citrate.ai");
    expect(p.bootnodes.length).toBeGreaterThanOrEqual(4);
    expect(p.addressBook).toBe("addresses/40204.json");
    // RC52: the node builds the r1006 block 0 only when told to (`[chain] genesis_profile`).
    expect(p.genesisProfile).toBe("r1006");
    const unset = Object.keys(p).filter((k) => p[k] === null).sort();
    expect(unset).toEqual(["addressBookSha256", "genesisHash", "minChainRev"]);
    expect(Object.keys(p.pending).sort()).toEqual(unset);
    expect(profileProblems({ id: "r1006", ...p })).toEqual([
      expect.stringMatching(/^genesisHash not set/),
      expect.stringMatching(/^addressBookSha256 not set/),
      expect.stringMatching(/^minChainRev not set/),
    ]);
  });
});

describe("fill-network-profile.sh", SLOW, () => {
  it("writes only the reroll values; the filled profile is ready by the app's rule", () => {
    const before = fs.readFileSync(profiles, "utf8");
    const r = run(fill, fillArgs());
    expect(r.code, r.err).toBe(0);
    const after = read();
    const was = JSON.parse(before);
    const digest = createHash("sha256").update(fs.readFileSync(bookPath)).digest("hex");
    expect(after.profiles.r1006.genesisHash).toBe(GENESIS);
    expect(after.profiles.r1006.addressBookSha256).toBe(digest);
    expect(after.profiles.r1006.minChainRev).toBe(REV);
    expect(after.profiles.r1006.pending).toEqual({});
    // Everything else is unchanged: the other profiles, `active`, and the rest of r1006.
    const expected = structuredClone(was);
    Object.assign(expected.profiles.r1006, {
      genesisHash: GENESIS,
      addressBookSha256: digest,
      minChainRev: REV,
      pending: {},
    });
    expect(after).toEqual(expected);
    expect(after.active).toBe(was.active);
    // Byte level: the only new lines are the three values; the only lines gone are their nulls,
    // their reasons and the opening of the non-empty `pending`.
    const a = before.split("\n");
    const b = fs.readFileSync(profiles, "utf8").split("\n");
    expect(b.filter((l) => !a.includes(l)).map((l) => l.trim())).toEqual([
      `"genesisHash": "${GENESIS}",`,
      `"addressBookSha256": "${digest}",`,
      `"minChainRev": "${REV}",`,
    ]);
    const gone = a.filter((l) => !b.includes(l));
    expect(gone.length).toBeGreaterThan(0);
    expect(gone.every((l) => /^\s*"(genesisHash|addressBookSha256|minChainRev)": /.test(l) || /^\s*"pending": \{$/.test(l))).toBe(true);
    expect(profileProblems({ id: "r1006", ...after.profiles.r1006 })).toEqual([]);
  });

  it("--activate makes it the active profile; nothing else moves", () => {
    const r = run(fill, fillArgs({ extra: ["--activate"] }));
    expect(r.code, r.err).toBe(0);
    expect(read().active).toBe("r1006");
    expect(r.out).toMatch(/sync-addresses\.py/);
  });

  const refusals = [
    ["a short genesis hash", { genesis: "0x" + "6a".repeat(31) }, /--genesis-hash must be 0x \+ 64 hex/],
    ["a genesis hash without 0x", { genesis: "6a".repeat(32) }, /--genesis-hash must be 0x \+ 64 hex/],
    ["a non-hex genesis hash", { genesis: "0x" + "zz".repeat(32) }, /--genesis-hash must be 0x \+ 64 hex/],
    ["a zero genesis hash", { genesis: "0x" + "00".repeat(32) }, /--genesis-hash is zero/],
    ["an abbreviated chain rev", { rev: "1f1f1f1f" }, /--min-chain-rev must be a full 40-hex commit/],
    ["a non-hex chain rev", { rev: "g".repeat(40) }, /--min-chain-rev must be a full 40-hex commit/],
    ["a zero chain rev", { rev: "0".repeat(40) }, /--min-chain-rev is zero/],
    // Full-string matching: nothing before or after the value.
    ["a genesis hash with a trailing newline", { genesis: GENESIS + "\n" }, /--genesis-hash must be 0x \+ 64 hex/],
    ["a genesis hash with trailing junk", { genesis: GENESIS + "00" }, /--genesis-hash must be 0x \+ 64 hex/],
    ["a genesis hash with leading whitespace", { genesis: " " + GENESIS }, /--genesis-hash must be 0x \+ 64 hex/],
    ["a chain rev with a trailing newline", { rev: REV + "\n" }, /--min-chain-rev must be a full 40-hex commit/],
    ["a chain rev with a trailing CRLF", { rev: REV + "\r\n" }, /--min-chain-rev must be a full 40-hex commit/],
    ["a chain rev with leading whitespace", { rev: "\t" + REV }, /--min-chain-rev must be a full 40-hex commit/],
    ["a 41-hex chain rev", { rev: REV + "a" }, /--min-chain-rev must be a full 40-hex commit/],
    ["a 39-hex chain rev", { rev: REV.slice(1) }, /--min-chain-rev must be a full 40-hex commit/],
    ["r1005's genesis hash for r1006", { genesis: "SHIPPED_R1005" }, /is already the genesis of profile alpha-r1005/],
    ["an unknown profile", { profile: "r1007" }, /no profile 'r1007'/],
    ["a profile with other values unset (genesis-2026-12)", { profile: "genesis-2026-12" }, /also has .* unset/],
  ];
  for (const [what, over, msg] of refusals) {
    it(`refuses ${what} and writes nothing`, () => {
      const before = fs.readFileSync(profiles);
      // Another profile's genesis (in another case) is another network, not this one's.
      const args = over.genesis === "SHIPPED_R1005"
        ? { genesis: read().profiles["alpha-r1005"].genesisHash.toUpperCase().replace(/^0X/, "0x") }
        : over;
      const r = run(fill, fillArgs(args));
      expect(r.code).toBe(1);
      expect(r.err).toMatch(msg);
      expect(fs.readFileSync(profiles).equals(before)).toBe(true);
    });
  }

  const bookRefusals = [
    ["a book for another chain", canonicalBook({ chainId: 1 }), /address book is for chain 1, the profile is chain 40204/],
    ["a book whose chainId is a string", canonicalBook({ chainId: "40204" }), /chainId '40204' is not an integer/],
    ["a book that is not the canonical book (no deployer)", (() => { const b = canonicalBook(); delete b.deployer; return b; })(), /lacks \['deployer'\]/],
    ["the app's generated book instead of the canonical one", { chainId: 40204, genesisHash: GENESIS, addresses: {} }, /is not a canonical book/],
    ["a book with an entry that is not an address", canonicalBook({ aaStack: { CitratePaymaster: "0x1234" } }), /not an address in the book: aaStack.CitratePaymaster/],
    ["a book with an address with a trailing newline", canonicalBook({ aaStack: { CitratePaymaster: addr("06") + "\n" } }), /not an address in the book: aaStack.CitratePaymaster/],
    ["a book whose deployer has a trailing newline", canonicalBook({ deployer: addr("dd") + "\n" }), /deployer .* is not an address/],
    ["a book missing a pin the app needs", (() => { const b = canonicalBook(); delete b.contracts.ValidatorRegistry; return b; })(), /sync-addresses\.py --check: .*ValidatorRegistry is missing/],
    ["a book with two pins at one address", canonicalBook({ contracts: { ...canonicalBook().contracts, SkillRegistry: addr("08") } }), /share address/],
    ["a book with a different genesis", canonicalBook({ genesisHash: "0x" + "7b".repeat(32) }), /genesisHash .* != --genesis-hash/],
    ["a book that is not JSON", "{ not json", /cannot be read/],
  ];
  for (const [what, book, msg] of bookRefusals) {
    it(`refuses ${what} and writes nothing`, () => {
      const before = fs.readFileSync(profiles);
      const r = run(fill, fillArgs({ book: writeBook(book, "bad.json") }));
      expect(r.code, r.out).toBe(1);
      expect(r.err).toMatch(msg);
      expect(fs.readFileSync(profiles).equals(before)).toBe(true);
    });
  }

  it("refuses a missing book file", () => {
    const r = run(fill, fillArgs({ book: path.join(dir, "absent.json") }));
    expect(r.code).toBe(1);
    expect(r.err).toMatch(/cannot be read/);
  });

  it("refuses to overwrite filled values without --replace, and replaces with it", () => {
    expect(run(fill, fillArgs()).code).toBe(0);
    const filled = fs.readFileSync(profiles);
    const again = run(fill, fillArgs({ genesis: "0x" + "7c".repeat(32) }));
    expect(again.code).toBe(1);
    expect(again.err).toMatch(/already has \['genesisHash'\] set; pass --replace/);
    expect(fs.readFileSync(profiles).equals(filled)).toBe(true);
    // The same values again are a no-op, not an overwrite.
    expect(run(fill, fillArgs()).code).toBe(0);
    const replaced = run(fill, fillArgs({ genesis: "0x" + "7c".repeat(32), extra: ["--replace"] }));
    expect(replaced.code, replaced.err).toBe(0);
    expect(read().profiles.r1006.genesisHash).toBe("0x" + "7c".repeat(32));
  });

  it("stores the genesis hash and rev lowercase", () => {
    const r = run(fill, fillArgs({ genesis: "0x" + "6A".repeat(32), rev: "1F".repeat(20) }));
    expect(r.code, r.err).toBe(0);
    expect(read().profiles.r1006.genesisHash).toBe(GENESIS);
    expect(read().profiles.r1006.minChainRev).toBe(REV);
  });

  it("keeps the file's mode (a 0644 file stays 0644, not mkstemp's 0600)", () => {
    fs.chmodSync(profiles, 0o644);
    expect(run(fill, fillArgs()).code).toBe(0);
    expect(fs.statSync(profiles).mode & 0o777).toBe(0o644);
  });

  it("usage error without a profile", () => {
    expect(run(fill, ["--genesis-hash", GENESIS]).code).toBe(2);
  });
});

describe("check-network-profile.sh (release tripwire, G0)", SLOW, () => {
  it("positive control: the shipped profiles pass the release check, r1006 required (G0)", () => {
    const r = run(check, ["--release", "--require-active", "r1006"]);
    expect(r.code, r.err).toBe(0);
    expect(r.out).toMatch(/OK \(active r1006, release; not set \(inactive\): genesis-2026-12\)/);
  });

  it("negative control: the shipped file with the unfilled r1006 active fails the release check", () => {
    const data = read();
    data.active = "r1006";
    fs.writeFileSync(profiles, JSON.stringify(data, null, 2) + "\n");
    const r = run(check, ["--release", "--profiles", profiles]);
    expect(r.code).toBe(1);
    expect(r.err).toMatch(/active profile r1006 has unset values \['genesisHash', 'addressBookSha256', 'minChainRev'\]/);
    // Without --release the same file is only a well-formed, not-yet-set profile.
    expect(run(check, ["--profiles", profiles]).code).toBe(0);
  });

  it("negative control: the not-set genesis-2026-12 active fails the release check", () => {
    const data = read();
    data.active = "genesis-2026-12";
    fs.writeFileSync(profiles, JSON.stringify(data, null, 2) + "\n");
    expect(run(check, ["--release", "--profiles", profiles]).code).toBe(1);
  });

  it("a filled and activated r1006 passes once the embedded book and MIN_CHAIN_REV follow it", () => {
    expect(run(fill, fillArgs({ genesis: R1006_GENESIS, extra: ["--activate"] })).code).toBe(0);
    const appDir = path.join(dir, "app");
    fs.mkdirSync(path.join(appDir, "addresses"), { recursive: true });
    const appBook = path.join(appDir, "addresses", "40204.json");
    const sidecar = path.join(dir, "build-sidecar.sh");
    const opts = ["--release", "--profiles", profiles, "--app-book-dir", appDir, "--sidecar", sidecar];
    // Activated but not followed through: the old network's book and the old MIN_CHAIN_REV both fail.
    const oldBook = JSON.parse(fs.readFileSync(path.join(repo, "src-tauri", "addresses", "40204.json"), "utf8"));
    oldBook.genesisHash = prefillProfiles().profiles["alpha-r1005"].genesisHash;
    fs.writeFileSync(appBook, JSON.stringify(oldBook, null, 2) + "\n");
    fs.copyFileSync(realSidecar, sidecar);
    const stale = run(check, opts);
    expect(stale.code).toBe(1);
    expect(stale.err).toMatch(/embedded book .* genesis .* != profile/);
    expect(stale.err).toMatch(/MIN_CHAIN_REV .* != profile minChainRev/);
    // Followed through: the generated book for the new genesis, the re-pinned rev.
    execFileSync("python3", ["-I", path.join(here, "sync-addresses.py"), "--book", bookPath, "--genesis", R1006_GENESIS,
      "--out", appBook], { env: env() });
    fs.writeFileSync(sidecar, fs.readFileSync(realSidecar, "utf8").replace(/^MIN_CHAIN_REV="[0-9a-f]{40}"/m,
      `MIN_CHAIN_REV="${REV}"`));
    const ok = run(check, [...opts, "--require-active", "r1006", "--canonical-book", bookPath]);
    expect(ok.code, ok.err).toBe(0);
    // A different canonical book than the one the profile was filled from fails.
    const other = writeBook(canonicalBook({ chainName: "x" }), "other.json");
    expect(run(check, [...opts, "--canonical-book", other]).err).toMatch(/sha256 .* != profile addressBookSha256/);
    expect(run(check, [...opts, "--require-active", "alpha-r1005"]).err).toMatch(/required 'alpha-r1005'/);
  });

  it("the embedded book must be the one sync-addresses.py generates from the canonical book the profile pins", () => {
    expect(run(fill, fillArgs({ genesis: R1006_GENESIS, extra: ["--activate"] })).code).toBe(0);
    const appDir = path.join(dir, "app");
    fs.mkdirSync(path.join(appDir, "addresses"), { recursive: true });
    const appBook = path.join(appDir, "addresses", "40204.json");
    const sidecar = path.join(dir, "build-sidecar.sh");
    fs.writeFileSync(sidecar, fs.readFileSync(realSidecar, "utf8").replace(/^MIN_CHAIN_REV="[0-9a-f]{40}"/m,
      `MIN_CHAIN_REV="${REV}"`));
    const opts = ["--release", "--profiles", profiles, "--app-book-dir", appDir, "--sidecar", sidecar];
    const sync = (book) => execFileSync("python3", ["-I", path.join(here, "sync-addresses.py"), "--book", book,
      "--genesis", R1006_GENESIS, "--out", appBook], { env: env() });
    sync(bookPath);
    const ok = run(check, opts);
    expect(ok.code, ok.err).toBe(0);
    const good = fs.readFileSync(appBook, "utf8");

    // An edited embedded book (one pin moved) is refused, though its chain id and genesis are right.
    const edited = JSON.parse(good);
    edited.addresses.ValidatorRegistry = addr("ee");
    fs.writeFileSync(appBook, JSON.stringify(edited, null, 2) + "\n");
    const e1 = run(check, opts);
    expect(e1.code).toBe(1);
    expect(e1.err).toMatch(/embedded book .* is not the book scripts\/sync-addresses\.py generates from the canonical book/);

    // A stale embedded book, generated from another canonical book of the same genesis, is refused.
    const other = writeBook(canonicalBook({ contracts: { ...canonicalBook().contracts, SkillRegistry: addr("19") } }), "other.json");
    sync(other);
    const e2 = run(check, opts);
    expect(e2.code).toBe(1);
    expect(e2.err).toMatch(/sha256 [0-9a-f]{64} != profile addressBookSha256/);

    // The pinned canonical copy itself edited: its digest no longer matches the profile.
    sync(bookPath);
    expect(run(check, opts).code).toBe(0);
    const canon = fs.readdirSync(path.join(appDir, "addresses")).filter((f) => f !== "40204.json");
    expect(canon).toEqual(["40204.canonical.json"]);
    const canonPath = path.join(appDir, "addresses", canon[0]);
    fs.writeFileSync(canonPath, fs.readFileSync(canonPath, "utf8").replace(addr("03"), addr("ee")));
    const e3 = run(check, opts);
    expect(e3.code).toBe(1);
    expect(e3.err).toMatch(/sha256 [0-9a-f]{64} != profile addressBookSha256/);

    // No canonical copy at all: refused, never skipped.
    fs.rmSync(canonPath);
    const e4 = run(check, opts);
    expect(e4.code).toBe(1);
    expect(e4.err).toMatch(/canonical book .* cannot be read/);
  });

  it("a genesis that is not the block 0 the bundled node builds for the profile's genesis profile fails", () => {
    // RC52: genesisProfile chooses the block 0 the node builds; the profile's genesisHash must be it.
    expect(run(fill, fillArgs()).code).toBe(0);
    const bad = run(check, ["--profiles", profiles]);
    expect(bad.code).toBe(1);
    expect(bad.err).toMatch(new RegExp(`profile r1006: genesisHash ${GENESIS} is not the block 0 the bundled node builds for genesis profile r1006 \\(${R1006_GENESIS}\\)`));
    expect(run(fill, fillArgs({ genesis: R1006_GENESIS, extra: ["--replace"] })).code).toBe(0);
    const ok = run(check, ["--profiles", profiles]);
    expect(ok.code, ok.err).toBe(0);
  });

  it("a stale pending reason next to a set value fails, and so does an unset value with no reason", () => {
    const data = read();
    data.profiles["alpha-r1005"].pending = { genesisHash: "stale" };
    fs.writeFileSync(profiles, JSON.stringify(data, null, 2) + "\n");
    expect(run(check, ["--profiles", profiles]).err).toMatch(/pending has a reason for genesisHash, which is set/);
    const d2 = prefillProfiles();
    delete d2.profiles.r1006.pending.minChainRev;
    fs.writeFileSync(profiles, JSON.stringify(d2, null, 2) + "\n");
    expect(run(check, ["--profiles", profiles]).err).toMatch(/minChainRev is unset with no reason/);
  });

  it("an absent field (instead of null) and an unknown key are refused", () => {
    const d = read();
    delete d.profiles.r1006.minChainRev;
    fs.writeFileSync(profiles, JSON.stringify(d, null, 2) + "\n");
    expect(run(check, ["--profiles", profiles]).err).toMatch(/lacks \['minChainRev'\]/);
    const d2 = prefillProfiles();
    d2.profiles.r1006.genesisHas = null;
    fs.writeFileSync(profiles, JSON.stringify(d2, null, 2) + "\n");
    expect(run(check, ["--profiles", profiles]).err).toMatch(/unknown keys \['genesisHas'\]/);
  });

  // The Python rule (release gate) and the TypeScript rule (the webview's gate) agree.
  const malformed = [
    [{ chainId: 0 }, "chainId is not a chain id"],
    [{ genesisHash: "0x" + "0".repeat(64) }, "genesisHash is not a block hash"],
    [{ genesisHash: "0x1234" }, "genesisHash is not a block hash"],
    [{ nodeNetwork: "a b" }, "nodeNetwork is malformed"],
    [{ rpcUrl: "http://rpc.example.org" }, "rpcUrl is not an https URL"],
    [{ explorerUrl: "https://" }, "explorerUrl is not an https URL"],
    [{ faucetUrl: "https://a b" }, "faucetUrl is not an https URL"],
    [{ bootnodes: [] }, "bootnodes is empty"],
    [{ bootnodes: ["noise_" + "ab".repeat(32) + "@host:0"] }, "bootnodes has a malformed entry"],
    [{ bootnodes: ["noise_" + "ab".repeat(32) + "@host:70000"] }, "bootnodes has a malformed entry"],
    [{ bootnodes: ["noise_" + "ab".repeat(32) + "@ho_st:30303"] }, "bootnodes has a malformed entry"],
    [{ addressBook: "" }, "addressBook is empty"],
    [{ addressBookSha256: "ab".repeat(31) }, "addressBookSha256 is not a sha256 digest"],
    [{ addressBookSha256: "AB".repeat(32) }, "addressBookSha256 is not a sha256 digest"],
    [{ addressBookSha256: "0".repeat(64) }, "addressBookSha256 is not a sha256 digest"],
    [{ minChainRev: "c384fc98" }, "minChainRev is not a commit"],
    [{ minChainRev: "0".repeat(40) }, "minChainRev is not a commit"],
    // Full-string matching (a trailing newline, a CR, a leading space, the wrong case).
    [{ genesisHash: GENESIS + "\n" }, "genesisHash is not a block hash"],
    [{ genesisHash: " " + GENESIS }, "genesisHash is not a block hash"],
    [{ nodeNetwork: "testnet\n" }, "nodeNetwork is malformed"],
    [{ rpcUrl: "https://rpc.citrate.ai\n" }, "rpcUrl is not an https URL"],
    [{ bootnodes: ["noise_" + "ab".repeat(32) + "@host:30303\n"] }, "bootnodes has a malformed entry"],
    [{ addressBookSha256: "ab".repeat(32) + "\n" }, "addressBookSha256 is not a sha256 digest"],
    [{ addressBookSha256: "ab".repeat(32) + "\r" }, "addressBookSha256 is not a sha256 digest"],
    [{ minChainRev: REV + "\n" }, "minChainRev is not a commit"],
    [{ minChainRev: " " + REV }, "minChainRev is not a commit"],
    [{ minChainRev: "1F".repeat(20) }, "minChainRev is not a commit"],
    // RC52: a genesis profile the bundled node does not build (or a malformed name) is refused.
    [{ genesisProfile: "r9999" }, "genesisProfile is not a genesis profile the bundled node builds"],
    [{ genesisProfile: "" }, "genesisProfile is not a genesis profile the bundled node builds"],
    [{ genesisProfile: "testnet_beta\n" }, "genesisProfile is not a genesis profile the bundled node builds"],
    [{ genesisProfile: "Testnet_Beta" }, "genesisProfile is not a genesis profile the bundled node builds"],
  ];
  for (const [patch, why] of malformed) {
    it(`both rules refuse ${JSON.stringify(patch)}`, () => {
      const d = read();
      const p = { ...d.profiles["alpha-r1005"], ...patch };
      expect(profileProblems({ id: "alpha-r1005", ...p }).join("; ")).toContain(why);
      d.profiles["alpha-r1005"] = p;
      fs.writeFileSync(profiles, JSON.stringify(d, null, 2) + "\n");
      const r = run(check, ["--profiles", profiles]);
      expect(r.code).toBe(1);
      expect(r.err).toContain(why);
    });
  }
});

// Tripwire: a validating regex in a scripts/*.py helper must match the whole string. Python's `$`
// also matches before a final "\n" and `re.match` only anchors the start, so `^…$` with
// `.match`/`.search` lets "value\n" through. Use `re.fullmatch` (or `\A…\Z`).
const partialMatch = (src) =>
  src.split("\n").flatMap((line, i) => {
    const hits = [];
    if (/\b(re|[A-Z][A-Z0-9_]*)\.match\(/.test(line)) hits.push(`${i + 1}: .match( : ${line.trim()}`);
    if (/re\.compile\(r?["'][^"']*\$["']/.test(line)) hits.push(`${i + 1}: pattern ends in $ : ${line.trim()}`);
    return hits;
  });

describe("tripwire: full-string regex matching in the script helpers", () => {
  it("negative control: the scan flags `^…$` with .match", () => {
    expect(partialMatch('X = re.compile(r"^0x[0-9a-f]{40}$")\nif X.match(v):\n    pass')).toHaveLength(2);
    expect(partialMatch('if re.match(r"^a", v):')).toHaveLength(1);
    expect(partialMatch('X = re.compile(r"0x[0-9a-f]{40}")\nif X.fullmatch(v):')).toEqual([]);
  });

  for (const name of ["network-profile.py", "sync-addresses.py"]) {
    it(`${name} uses fullmatch`, () => {
      expect(partialMatch(fs.readFileSync(path.join(here, name), "utf8"))).toEqual([]);
    });
  }
});
