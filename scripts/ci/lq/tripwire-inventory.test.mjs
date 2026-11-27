// @vitest-environment node
//
// LQ-WP03: scripts/ci/lq/tripwire-inventory.mjs keeps every registered honesty and tripwire test,
// or lets it go only for a stricter replacement that is itself registered, exists in the runner's
// own test list and carries the weakened-copy evidence link. The inputs are the runners' own
// listings: `cargo test --workspace --locked -- --list` (stdout and stderr together) and
// `vitest list --json`. Each case drives the CLI the way CI does and checks both the exit code and
// the reason, so a missing script cannot pass a refusal case.
import { afterEach, describe, expect, it } from "vitest";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const SCRIPT = path.join(here, "tripwire-inventory.mjs");

const tmp = [];
afterEach(() => {
  for (const d of tmp.splice(0)) fs.rmSync(d, { recursive: true, force: true });
});

// A real-shaped `cargo test -- --list` log: cargo's "Running" lines go to stderr, each test
// binary's list goes to stdout, doc-tests are listed per crate and are not tripwires.
const CARGO_LIST = [
  "   Compiling citrate-core-kit v0.5.9 (/home/runner/work/citrate-core/citrate-core/kit)",
  "    Finished `test` profile [unoptimized + debuginfo] target(s) in 9m 01s",
  // cargo colours its status words on CI; the parser strips the escapes.
  "\x1b[1m\x1b[92m     Running\x1b[0m unittests src/lib.rs (target/debug/deps/citrate_core_kit-0a1b2c3d4e5f6a7b)",
  "ceremony_tests::rule3_tripwire_no_config_can_skip_the_ceremony: test",
  "ceremony_tests::join8_tripwire_catches_evasions: test",
  "ceremony_tests::a_plain_test: test",
  "custody_tests::adv2_wrong_passphrase_no_oracle: test",
  "",
  "4 tests, 0 benchmarks",
  "     Running unittests src/main.rs (target/debug/deps/citrate_components-1111111111111111)",
  "tests::never_extracts_outside_the_root: test",
  "",
  "1 test, 0 benchmarks",
  "     Running unittests src/lib.rs (target/debug/deps/citrate_components-2222222222222222)",
  "payouts::dark_refuses_join: test",
  "",
  "1 test, 0 benchmarks",
  "     Running tests/removed_commands_tests.rs (target/debug/deps/removed_commands_tests-3333333333333333)",
  "removed_commands_are_not_registered_or_allowed: test",
  "",
  "1 test, 0 benchmarks",
  "   Doc-tests citrate_core_kit",
  "src/lib.rs - tripwire_doc (line 12): test",
  "",
  "1 test, 0 benchmarks",
].join("\n");

const ROOT = "/home/runner/work/citrate-core/citrate-core";
const VITEST_LIST = [
  { name: "telemetry opt-out > tripwire: no send path skips the opt-out", file: `${ROOT}/src/privacy/telemetryOptOut.tripwire.test.ts` },
  { name: "every reader is wired", file: `${ROOT}/src/shell/syncProgress.tripwire.test.tsx` },
  { name: "harness > never a fabricated reply", file: `${ROOT}/src/agent/harness.test.ts` },
  { name: "renders the header", file: `${ROOT}/src/shell/Header.test.tsx` },
];

const R_CEREMONY = "citrate_core_kit::ceremony_tests::rule3_tripwire_no_config_can_skip_the_ceremony";
const R_EVASIONS = "citrate_core_kit::ceremony_tests::join8_tripwire_catches_evasions";
const R_ADV2 = "citrate_core_kit::custody_tests::adv2_wrong_passphrase_no_oracle";
const R_NEVER_BIN = "citrate_components@bin::tests::never_extracts_outside_the_root";
const R_DARK = "citrate_components::payouts::dark_refuses_join";
const R_REMOVED = "removed_commands_tests::removed_commands_are_not_registered_or_allowed";
const V_TELEMETRY = "vitest:src/privacy/telemetryOptOut.tripwire.test.ts::telemetry opt-out > tripwire: no send path skips the opt-out";
const V_SYNC = "vitest:src/shell/syncProgress.tripwire.test.tsx::every reader is wired";
const V_HARNESS = "vitest:src/agent/harness.test.ts::harness > never a fabricated reply";
const ALL_TRIPWIRES = [R_CEREMONY, R_EVASIONS, R_ADV2, R_NEVER_BIN, R_DARK, R_REMOVED, V_TELEMETRY, V_SYNC, V_HARNESS];

const EVIDENCE = "https://github.com/CitrateNetwork/citrate-core/actions/runs/45549119759";

function entry(id, extra = {}) {
  return { id, class: "honesty", since: "a826e441", replaced_by: null, ...extra };
}

function registry(entries) {
  return { version: 1, tripwires: entries };
}

function dir() {
  const d = fs.mkdtempSync(path.join(os.tmpdir(), "lq-tripwires-"));
  tmp.push(d);
  return d;
}

// Writes the inputs and runs the CLI. `cargo`/`vitest` override the listings; `reg`/`base` are
// registry objects (base omitted means "first introduction, no base registry").
function run(cmd, { reg, base, cargo = CARGO_LIST, vitest = VITEST_LIST, labels, extra = [] } = {}) {
  const d = dir();
  const files = {
    cargo: path.join(d, "cargo-list.txt"),
    vitest: path.join(d, "vitest-list.json"),
    reg: path.join(d, "tripwires.json"),
    base: path.join(d, "base-tripwires.json"),
  };
  fs.writeFileSync(files.cargo, cargo);
  fs.writeFileSync(files.vitest, JSON.stringify(vitest));
  const args = [SCRIPT, cmd, "--cargo-list", files.cargo, "--vitest-list", files.vitest, "--root", ROOT];
  if (reg !== undefined) {
    fs.writeFileSync(files.reg, typeof reg === "string" ? reg : JSON.stringify(reg));
    args.push("--registry", files.reg);
  }
  if (base !== undefined) {
    fs.writeFileSync(files.base, JSON.stringify(base));
    args.push("--base-registry", files.base);
  }
  if (labels !== undefined) args.push("--labels", labels);
  args.push(...extra);
  const r = spawnSync(process.execPath, args, { encoding: "utf8" });
  return { status: r.status, stdout: r.stdout, stderr: r.stderr, text: r.stdout + r.stderr };
}

const FULL = registry(ALL_TRIPWIRES.map((id) => entry(id)));

describe("parsing the runners' own listings", () => {
  it("parses cargo --list per test binary, keeps lib and bin units apart, and skips doc-tests", () => {
    const r = run("list");
    expect(r.status).toBe(0);
    const ids = JSON.parse(r.stdout);
    expect(ids.rust).toContain(R_CEREMONY);
    expect(ids.rust).toContain(R_NEVER_BIN);
    expect(ids.rust).toContain(R_DARK);
    expect(ids.rust).toContain(R_REMOVED);
    expect(ids.rust).toContain("citrate_core_kit::ceremony_tests::a_plain_test");
    expect(ids.rust.some((id) => id.includes("tripwire_doc"))).toBe(false);
    expect(ids.rust).toHaveLength(7);
    expect(ids.vitest).toContain(V_TELEMETRY);
    expect(ids.vitest).toContain("vitest:src/shell/Header.test.tsx::renders the header");
    expect(ids.vitest).toHaveLength(4);
  });

  it("tells apart two vitest tests with the same title in one file, so deleting one shows", () => {
    const twice = { name: "every reader is wired", file: `${ROOT}/src/shell/syncProgress.tripwire.test.tsx` };
    const r = run("list", { vitest: [...VITEST_LIST, twice] });
    expect(r.status).toBe(0);
    const ids = JSON.parse(r.stdout);
    expect(ids.vitest).toContain(V_SYNC);
    expect(ids.vitest).toContain(`${V_SYNC} #2`);
    const reg = registry([...FULL.tripwires, entry(`${V_SYNC} #2`)]);
    const gone = run("check", { reg, base: reg, labels: "" });
    expect(gone.status).toBe(1);
    expect(gone.text).toContain(`missing registered tripwire: ${V_SYNC} #2`);
  });

  it("refuses an empty cargo listing instead of reporting every tripwire missing", () => {
    const r = run("check", { reg: FULL, cargo: "    Finished `test` profile\n", labels: "" });
    expect(r.status).toBe(2);
    expect(r.text).toMatch(/no tests in the cargo listing/);
  });

  it("refuses an empty vitest listing", () => {
    const r = run("check", { reg: FULL, vitest: [], labels: "" });
    expect(r.status).toBe(2);
    expect(r.text).toMatch(/no tests in the vitest listing/);
  });
});

describe("generate", () => {
  it("registers every test that matches the tripwire patterns, class-tagged, and nothing else", () => {
    const r = run("generate", { extra: ["--since", "a826e441"] });
    expect(r.status).toBe(0);
    const doc = JSON.parse(r.stdout);
    expect(doc.version).toBe(1);
    expect(doc.tripwires.map((t) => t.id).sort()).toEqual([...ALL_TRIPWIRES].sort());
    const byId = Object.fromEntries(doc.tripwires.map((t) => [t.id, t]));
    expect(byId[R_CEREMONY].class).toBe("ceremony");
    expect(byId[R_ADV2].class).toBe("custody");
    expect(byId[R_DARK].class).toBe("money");
    expect(byId[R_REMOVED].class).toBe("ipc");
    expect(byId[V_TELEMETRY].class).toBe("privacy");
    expect(byId[V_SYNC].class).toBe("ui-truth");
    for (const t of doc.tripwires) {
      expect(t.since).toBe("a826e441");
      expect(t.replaced_by).toBeNull();
    }
  });

  it("keeps existing entries exactly as they are when refreshing", () => {
    const keep = entry(R_CEREMONY, { class: "custody", since: "0000aaaa" });
    const r = run("generate", { reg: registry([keep]), extra: ["--since", "a826e441"] });
    expect(r.status).toBe(0);
    const doc = JSON.parse(r.stdout);
    expect(doc.tripwires.find((t) => t.id === R_CEREMONY)).toEqual(keep);
    expect(doc.tripwires).toHaveLength(ALL_TRIPWIRES.length);
  });
});

describe("check: registered tripwires are kept", () => {
  it("passes when every registered tripwire is in the runners' lists", () => {
    const r = run("check", { reg: FULL, base: FULL, labels: "" });
    expect(r.status).toBe(0);
    expect(r.text).toMatch(/9 registered tripwires present/);
  });

  it("a missing registered id fails", () => {
    const cargo = CARGO_LIST.replace("ceremony_tests::rule3_tripwire_no_config_can_skip_the_ceremony: test\n", "");
    const r = run("check", { reg: FULL, base: FULL, cargo, labels: "" });
    expect(r.status).toBe(1);
    expect(r.text).toContain(`missing registered tripwire: ${R_CEREMONY}`);
  });

  it("a missing vitest tripwire fails", () => {
    const vitest = VITEST_LIST.filter((t) => !t.file.endsWith("syncProgress.tripwire.test.tsx"));
    const r = run("check", { reg: FULL, base: FULL, vitest, labels: "" });
    expect(r.status).toBe(1);
    expect(r.text).toContain(`missing registered tripwire: ${V_SYNC}`);
  });

  it("a rename without replaced_by fails", () => {
    const cargo = CARGO_LIST.replace(
      "ceremony_tests::join8_tripwire_catches_evasions: test",
      "ceremony_tests::join8_tripwire_catches_more_evasions: test",
    );
    const r = run("check", { reg: FULL, base: FULL, cargo, labels: "" });
    expect(r.status).toBe(1);
    expect(r.text).toContain(`missing registered tripwire: ${R_EVASIONS}`);
  });

  it("removing an entry from the registry fails even when the test is gone too", () => {
    const head = registry(FULL.tripwires.filter((t) => t.id !== R_ADV2));
    const cargo = CARGO_LIST.replace("custody_tests::adv2_wrong_passphrase_no_oracle: test\n", "");
    const r = run("check", { reg: head, base: FULL, cargo, labels: "tripwire-replace" });
    expect(r.status).toBe(1);
    expect(r.text).toContain(`registry entry removed: ${R_ADV2}`);
  });

  it("a malformed registry is refused", () => {
    const r = run("check", { reg: registry([{ id: R_CEREMONY, class: "whatever", since: "x", replaced_by: null }]), labels: "" });
    expect(r.status).toBe(1);
    expect(r.text).toMatch(/class "whatever" is not one of/);
  });

  it("a duplicate registry id is refused", () => {
    const r = run("check", { reg: registry([entry(R_CEREMONY), entry(R_CEREMONY)]), labels: "" });
    expect(r.status).toBe(1);
    expect(r.text).toContain(`duplicate registry id: ${R_CEREMONY}`);
  });
});

describe("check: stricter-only replacement", () => {
  const NEW = "citrate_core_kit::ceremony_tests::join8_tripwire_catches_more_evasions";
  const renamed = CARGO_LIST.replace(
    "ceremony_tests::join8_tripwire_catches_evasions: test",
    "ceremony_tests::join8_tripwire_catches_more_evasions: test",
  );
  const replaced = (extra = {}) =>
    registry([
      ...FULL.tripwires.map((t) =>
        t.id === R_EVASIONS ? { ...t, replaced_by: NEW, replacement_evidence: EVIDENCE, ...extra } : t,
      ),
      entry(NEW, { class: "ceremony" }),
    ]);

  it("a labeled replacement that exists, is registered and links its evidence passes", () => {
    const r = run("check", { reg: replaced(), base: FULL, cargo: renamed, labels: "chain-touch,tripwire-replace" });
    expect(r.status).toBe(0);
    expect(r.text).toContain(`replaced: ${R_EVASIONS} -> ${NEW}`);
  });

  it("replaced_by pointing to a non-existent test fails", () => {
    const r = run("check", { reg: replaced(), base: FULL, cargo: CARGO_LIST.replace("ceremony_tests::join8_tripwire_catches_evasions: test\n", ""), labels: "tripwire-replace" });
    expect(r.status).toBe(1);
    expect(r.text).toContain(`replaced_by points to a test that does not exist: ${R_EVASIONS} -> ${NEW}`);
  });

  it("a replacement without the tripwire-replace label fails", () => {
    const r = run("check", { reg: replaced(), base: FULL, cargo: renamed, labels: "chain-touch" });
    expect(r.status).toBe(1);
    expect(r.text).toContain(`needs the tripwire-replace label: ${R_EVASIONS}`);
  });

  it("a replacement without the weakened-copy evidence link fails", () => {
    const r = run("check", { reg: replaced({ replacement_evidence: "trust me" }), base: FULL, cargo: renamed, labels: "tripwire-replace" });
    expect(r.status).toBe(1);
    expect(r.text).toContain(`replacement_evidence must link the run or commit where ${NEW} failed against a weakened copy`);
  });

  it("a replacement that is not itself registered fails", () => {
    const reg = registry(FULL.tripwires.map((t) => (t.id === R_EVASIONS ? { ...t, replaced_by: NEW, replacement_evidence: EVIDENCE } : t)));
    const r = run("check", { reg, base: FULL, cargo: renamed, labels: "tripwire-replace" });
    expect(r.status).toBe(1);
    expect(r.text).toContain(`replacement is not registered: ${NEW}`);
  });

  it("a tripwire replaced by itself fails", () => {
    const reg = registry([entry(R_CEREMONY, { replaced_by: R_CEREMONY, replacement_evidence: EVIDENCE })]);
    const r = run("check", { reg, labels: "tripwire-replace" });
    expect(r.status).toBe(1);
    expect(r.text).toContain(`${R_CEREMONY} is replaced by itself`);
  });

  it("a replacement cycle fails", () => {
    const reg = registry([
      entry(R_CEREMONY, { replaced_by: R_EVASIONS, replacement_evidence: EVIDENCE }),
      entry(R_EVASIONS, { replaced_by: R_CEREMONY, replacement_evidence: EVIDENCE }),
    ]);
    const r = run("check", { reg, labels: "tripwire-replace" });
    expect(r.status).toBe(1);
    expect(r.text).toMatch(/replacement cycle/);
  });

  it("changing an earlier replacement fails", () => {
    const base = replaced();
    const other = "citrate_core_kit::ceremony_tests::a_plain_test";
    const head = registry([
      ...base.tripwires.map((t) => (t.id === R_EVASIONS ? { ...t, replaced_by: other } : t)),
      entry(other),
    ]);
    const r = run("check", { reg: head, base, cargo: renamed, labels: "tripwire-replace" });
    expect(r.status).toBe(1);
    expect(r.text).toContain(`replaced_by changed for ${R_EVASIONS}`);
  });

  it("in a merge group (no labels available) the label rule is left to the PR check, the rest still holds", () => {
    const ok = run("check", { reg: replaced(), base: FULL, cargo: renamed, extra: ["--no-labels"] });
    expect(ok.status).toBe(0);
    expect(ok.text).toMatch(/labels not available/);
    const bad = run("check", { reg: FULL, base: FULL, cargo: renamed, extra: ["--no-labels"] });
    expect(bad.status).toBe(1);
  });
});

describe("check: unregistered tripwire-named tests", () => {
  const SMALL = registry(ALL_TRIPWIRES.filter((id) => id !== R_DARK).map((id) => entry(id)));

  it("a new tripwire-named test that is not registered is reported (warn)", () => {
    const r = run("check", { reg: SMALL, base: SMALL, labels: "" });
    expect(r.status).toBe(0);
    expect(r.text).toContain(`warning: unregistered tripwire: ${R_DARK}`);
  });

  it("fails with --strict-unregistered (the setting after WP close)", () => {
    const r = run("check", { reg: SMALL, base: SMALL, labels: "", extra: ["--strict-unregistered"] });
    expect(r.status).toBe(1);
    expect(r.text).toContain(`unregistered tripwire: ${R_DARK}`);
  });
});

describe("count and usage", () => {
  it("counts registered, active and replaced entries and the ones present", () => {
    const r = run("count", { reg: FULL });
    expect(r.status).toBe(0);
    const c = JSON.parse(r.stdout);
    expect(c.registered).toBe(9);
    expect(c.active).toBe(9);
    expect(c.replaced).toBe(0);
    expect(c.present).toBe(9);
    expect(c.by_class).toEqual({ honesty: 9 });
  });

  it("an unknown command is a usage error", () => {
    const r = spawnSync(process.execPath, [SCRIPT, "frobnicate"], { encoding: "utf8" });
    expect(r.status).toBe(2);
    expect(r.stdout + r.stderr).toMatch(/usage: tripwire-inventory\.mjs/);
  });

  it("check without --labels or --no-labels is a usage error (CI must say which)", () => {
    const r = run("check", { reg: FULL });
    expect(r.status).toBe(2);
    expect(r.text).toMatch(/check needs --labels/);
  });

  it("check without a registry is a usage error", () => {
    const r = run("check", { labels: "" });
    expect(r.status).toBe(2);
    expect(r.text).toMatch(/--registry is required/);
  });
});
