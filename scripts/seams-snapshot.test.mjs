// L0-WP02: the snapshot helper catches every kind of change a seam move could make (mutation
// cases), never writes outside the fixture directories, and ci/characterization.json (the manifest
// LQ runs on every merge group) names suites that exist.
import { describe, expect, it } from "vitest";
import { existsSync, mkdtempSync, readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { Buffer } from "node:buffer";
import { ROOT, MISSING_MARK, VOLATILE, canonical, checkSnapshot, decodeLog, diffFixture, diffSnapshot, inputFixture, maskVolatile } from "./seams-snapshot.mjs";

const methods = { methods: { go: "method/1", save: "method/0", sendChat: "method/2" } };

describe("seams snapshot helper", () => {
  it("names a dropped member, an added member and a changed arity", () => {
    const dropped = { methods: { go: "method/1", save: "method/0" } };
    const added = { methods: { ...methods.methods, extra: "method/0" } };
    const arity = { methods: { ...methods.methods, sendChat: "method/3" } };
    expect(diffSnapshot(methods, dropped)).toEqual(["- methods.sendChat"]);
    expect(diffSnapshot(methods, added)).toEqual(["+ methods.extra"]);
    expect(diffSnapshot(methods, arity)).toEqual(['~ methods.sendChat: "method/2" -> "method/3"']);
    expect(diffSnapshot(["a", "b"], ["a"])).toEqual(["- 1"]);
    expect(diffSnapshot(methods, methods)).toEqual([]);
  });

  it("keeps what JSON would drop, so a value turning undefined is a change", () => {
    const c = canonical({ b: undefined, a: new Set([1]), m: new Map([["k", 2n]]), f: (x, y) => x + y });
    expect(Object.keys(c)).toEqual(["a", "b", "f", "m"]);
    expect(c).toEqual({ a: { "<set>": [1] }, b: "<undefined>", f: "<function/2>", m: { "<map>": [["k", "<bigint 2>"]] } });
    expect(diffSnapshot(canonical({ x: null }), canonical({ x: undefined }))).toHaveLength(1);
  });

  it("masks only what differs between two runs, and a shape change as a whole", () => {
    const a = canonical({ id: "0xaa", n: 1, rows: [{ ts: 1 }], keys: { k1: 1 } });
    const b = canonical({ id: "0xbb", n: 1, rows: [{ ts: 2 }], keys: { k2: 1 } });
    expect(maskVolatile(a, b)).toEqual({ id: VOLATILE, keys: VOLATILE, n: 1, rows: [{ ts: VOLATILE }] });
  });

  it("fails on a missing fixture and prints the exact bytes it would record", () => {
    const root = mkdtempSync(join(tmpdir(), "seams-"));
    const lines = [];
    expect(() => checkSnapshot("fixtures/seams/x.json", methods, { root, env: {}, log: (l) => lines.push(l) })).toThrow(/is missing/);
    const [[rel, body]] = decodeLog(lines.join("\n"));
    expect(rel).toBe("fixtures/seams/x.json");
    mkdirSync(join(root, "fixtures/seams"), { recursive: true });
    writeFileSync(join(root, rel), body);
    expect(checkSnapshot(rel, methods, { root, env: {} })).toEqual([]);
    writeFileSync(join(root, rel), JSON.stringify(canonical(methods)) + "\n");
    expect(checkSnapshot(rel, methods, { root, env: {} })).toEqual(["~ (bytes): the same value, written differently"]);
  });

  it("records in write mode only, and an input fixture is read back as written", () => {
    const root = mkdtempSync(join(tmpdir(), "seams-"));
    const env = { SEAMS_SNAPSHOT_WRITE: "1" };
    expect(checkSnapshot("fixtures/seams/y.json", methods, { root, env })).toEqual([]);
    expect(existsSync(join(root, "fixtures/seams/y.json"))).toBe(true);
    // diffFixture never writes, even in write mode: a mutation case cannot overwrite a fixture.
    expect(diffFixture("fixtures/seams/y.json", { methods: {} }, { root })).toEqual(["- methods.go", "- methods.save", "- methods.sendChat"]);
    expect(checkSnapshot("fixtures/seams/y.json", methods, { root, env: {} })).toEqual([]);
    const save = { persona: "p1", stage: "done" };
    expect(inputFixture("fixtures/seams/save.json", () => save, { root, env })).toEqual({ value: save, recorded: true });
    expect(inputFixture("fixtures/seams/save.json", () => ({}), { root, env: {} })).toEqual({ value: save, recorded: true });
    const lines = [];
    const gone = inputFixture("fixtures/seams/none.json", () => save, { root, env: {}, log: (l) => lines.push(l) });
    expect(gone).toEqual({ value: save, recorded: false });
    expect(decodeLog(lines.join("\n"))).toEqual([["fixtures/seams/none.json", JSON.stringify(save, null, 2) + "\n"]]);
  });

  it("refuses write mode under CI, so a job can never pass by recording", () => {
    const root = mkdtempSync(join(tmpdir(), "seams-"));
    const env = { SEAMS_SNAPSHOT_WRITE: "1", CI: "true" };
    expect(() => checkSnapshot("fixtures/seams/z.json", methods, { root, env })).toThrow(/refused under CI/);
    expect(() => inputFixture("fixtures/seams/z.json", () => ({}), { root, env })).toThrow(/refused under CI/);
    expect(existsSync(join(root, "fixtures/seams/z.json"))).toBe(false);
  });

  it("decode refuses a path outside the fixture directories", () => {
    const hex = Buffer.from("x\n").toString("hex");
    expect(() => decodeLog(`${MISSING_MARK} ../../etc/x.json ${hex}`)).toThrow(/refusing/);
    expect(() => decodeLog(`${MISSING_MARK} src/shell/store.ts ${hex}`)).toThrow(/refusing/);
    expect(() => decodeLog(`${MISSING_MARK} fixtures/seams/../../src/a.ts ${hex}`)).toThrow(/refusing/);
    const other = Buffer.from("y\n").toString("hex");
    expect(() => decodeLog(`${MISSING_MARK} fixtures/seams/a.json ${hex}\n${MISSING_MARK} fixtures/seams/a.json ${other}`)).toThrow(/twice/);
  });
});

describe("ci/characterization.json (the LQ manifest)", () => {
  const manifest = JSON.parse(readFileSync(join(ROOT, "ci/characterization.json"), "utf8"));
  const lib = readFileSync(join(ROOT, "src-tauri/src/lib.rs"), "utf8");

  it("lists unique L0 suites whose files, fixtures and cargo modules exist", () => {
    expect(manifest.version).toBe(1);
    const ids = manifest.suites.map((s) => s.id);
    expect(new Set(ids).size).toBe(ids.length);
    expect(ids).toEqual(expect.arrayContaining(["SEAM-CMDS", "SEAM-STORE", "SEAM-STATE", "SEAM-BRIDGE", "SEAM-ROUTES"]));
    for (const s of manifest.suites) {
      expect(s.lane).toMatch(/^L[0-9]$/);
      for (const f of s.fixtures ?? []) expect(existsSync(join(ROOT, f)), f).toBe(true);
      if (s.runner === "vitest") {
        expect(s.files.length).toBeGreaterThan(0);
        for (const f of s.files) expect(existsSync(join(ROOT, f)), f).toBe(true);
      } else {
        expect(s.runner).toBe("cargo");
        const mod = s.filter.replace(/::$/, "");
        expect(lib, `lib.rs declares mod ${mod}`).toMatch(new RegExp(`^mod ${mod};$`, "m"));
      }
    }
  });
});
