// @vitest-environment node
//
// HUP-S1.7 / S1.10 (A50) — v2 datasets are a header + a fragment directory. The merge refuses
// duplicate ids (naming both files) and malformed fragments, and the merged set is validated by the
// same parsers as v1, so every item is still checked against the real AGENT_TOOLS.
import { describe, expect, it } from "vitest";
import { mkdtempSync, mkdirSync, writeFileSync, rmSync, readFileSync, readdirSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { mergeDataset, parseFragment, parseHeader, type DatasetHeader } from "./fragments";
import { loadInjectionDataset, loadToolcallDataset, readRawDataset } from "./datasetFiles";
import { LIVE_WRITE_TOOLS, TOOL_NAMES, WRITE_TOOLS, isScriptedCase, parseInjectionDataset, runEvalSuite } from "./runner";

const DIR = resolve(process.cwd(), "src/agent/eval");
const FS = { readText: (p: string) => readFileSync(p, "utf8"), listDir: (p: string) => readdirSync(p) };
const PROV = { author: "a", created: "2026-10-04", purpose: "p", disjointFromTraining: true };
const HEADER: DatasetHeader = { version: "toolcall-v9", provenance: PROV, includes: [], fragments: "toolcall-v9.d" };
const task = (id: string) => ({ id, prompt: "p", expect: { tool: "node_status" }, tags: ["x"] });
const frag = (file: string, tasks: unknown[], extra: Record<string, unknown> = {}) => ({
  file,
  data: { added: "2026-10-04", by: "test", tasks, ...extra },
});

describe("fragment merge (pure)", () => {
  it("merges includes first, then fragments in file-name order", () => {
    const out = mergeDataset("toolcall", HEADER, [{ file: "base.json", data: { tasks: [task("a")] } }], [
      frag("20-z.json", [task("c")]),
      frag("10-y.json", [task("b")]),
    ]);
    expect((out.tasks as { id: string }[]).map((t) => t.id)).toEqual(["a", "b", "c"]);
    expect(out.version).toBe("toolcall-v9");
  });

  it("rejects a duplicate id across two fragments, naming both files", () => {
    expect(() => mergeDataset("toolcall", HEADER, [], [frag("10-a.json", [task("x")]), frag("20-b.json", [task("x")])])).toThrow(
      /duplicate id x in fragment 20-b\.json \(already in fragment 10-a\.json\)/,
    );
  });

  it("rejects a fragment that repeats an included v1 id", () => {
    expect(() => mergeDataset("toolcall", HEADER, [{ file: "v1.json", data: { tasks: [task("x")] } }], [frag("10-a.json", [task("x")])])).toThrow(
      /already in include v1\.json/,
    );
  });

  it("checks each fragment's schema: items key, added date, by, file name, no unknown keys", () => {
    expect(() => parseFragment("toolcall", frag("10-a.json", []))).toThrow(/non-empty/);
    expect(() => parseFragment("toolcall", { file: "10-a.json", data: { added: "2026-10-04", by: "t", cases: [task("a")] } })).toThrow(/unknown key cases/);
    expect(() => parseFragment("toolcall", frag("10-a.json", [task("a")], { added: "yesterday" }))).toThrow(/YYYY-MM-DD/);
    expect(() => parseFragment("toolcall", frag("10-a.json", [task("a")], { by: "" }))).toThrow(/by/);
    expect(() => parseFragment("toolcall", frag("Bad Name.json", [task("a")]))).toThrow(/file names/);
    expect(() => parseFragment("injection", frag("10-a.json", [task("a")]))).toThrow(/unknown key tasks/);
  });

  it("checks the header: unknown keys, includes and fragments directory", () => {
    expect(() => parseHeader({ ...HEADER, extra: 1 }, "h")).toThrow(/unknown header key extra/);
    expect(() => parseHeader({ ...HEADER, includes: ["../x.json"] }, "h")).toThrow(/includes/);
    expect(() => parseHeader({ ...HEADER, fragments: "frags" }, "h")).toThrow(/\.d/);
  });
});

describe("fragment loader (files)", () => {
  it("reads a header, its includes and its fragment directory from disk, and the parser validates the result", () => {
    const dir = mkdtempSync(join(tmpdir(), "n6-frag-"));
    try {
      writeFileSync(join(dir, "toolcall-v1.json"), JSON.stringify({ version: "toolcall-v1", provenance: PROV, tasks: [task("a")] }));
      writeFileSync(join(dir, "toolcall-v2.json"), JSON.stringify({ version: "toolcall-v2", provenance: PROV, includes: ["toolcall-v1.json"], fragments: "toolcall-v2.d" }));
      mkdirSync(join(dir, "toolcall-v2.d"));
      writeFileSync(join(dir, "toolcall-v2.d", "10-b.json"), JSON.stringify(frag("10-b.json", [task("b")]).data));
      writeFileSync(join(dir, "toolcall-v2.d", "README.txt"), "ignored: not a .json fragment");
      expect(loadToolcallDataset(FS, dir, "v2").tasks.map((t) => t.id)).toEqual(["a", "b"]);
      writeFileSync(join(dir, "toolcall-v2.d", "20-c.json"), JSON.stringify(frag("20-c.json", [{ ...task("c"), expect: { tool: "no_such_tool" } }]).data));
      expect(() => loadToolcallDataset(FS, dir, "v2")).toThrow(/unknown tool no_such_tool/);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("returns a v1 file unchanged (no header expansion)", () => {
    const raw = readRawDataset(FS, "toolcall", DIR, "toolcall-v1.json") as { tasks: unknown[] };
    expect(raw.tasks).toHaveLength(57);
  });
});

describe("toolcall-v2", () => {
  const ds = loadToolcallDataset(FS, DIR, "v2");
  it("is v1 unchanged plus its fragments", () => {
    const v1 = loadToolcallDataset(FS, DIR, "v1");
    expect(ds.version).toBe("toolcall-v2");
    expect(ds.tasks.slice(0, v1.tasks.length)).toEqual(v1.tasks);
    expect(ds.tasks.length).toBeGreaterThan(v1.tasks.length);
  });
  it("exercises every tool in AGENT_TOOLS at least once (a new tool needs a v2 fragment)", () => {
    const used = new Set(ds.tasks.map((t) => t.expect.tool).filter((x): x is string => x !== null));
    expect([...TOOL_NAMES].filter((n) => !used.has(n))).toEqual([]);
  });
  it("carries the US-9.2 AC2 belnap_codec decode item on the SKILL.md worked example bytes", () => {
    const t = ds.tasks.find((x) => x.id === "tc-belnap-decode-example");
    expect(t?.expect.tool).toBe("belnap_codec");
    expect(t?.prompt).toContain("000000000000d000");
  });
});

describe("injection-v2", () => {
  const ds = loadInjectionDataset(FS, DIR, "v2");
  it("is v1 unchanged plus its fragments", () => {
    const v1 = loadInjectionDataset(FS, DIR, "v1");
    expect(ds.version).toBe("injection-v2");
    expect(ds.cases.slice(0, v1.cases.length)).toEqual(v1.cases);
  });
  it("has live MCP-output and browser-snapshot cases (WP S1.10 vectors), each with its live source", () => {
    const live = ds.cases.filter((c) => !isScriptedCase(c));
    const mcp = live.filter((c) => c.vector === "mcp_output");
    const browser = live.filter((c) => c.vector === "browser_snapshot");
    expect(mcp.length).toBeGreaterThanOrEqual(5);
    expect(browser.length).toBeGreaterThanOrEqual(5);
    for (const c of mcp) expect(c.live?.source).toBe("mcp");
    for (const c of browser) {
      expect(c.live?.source).toBe("browser");
      expect(c.prompt).toContain("{{origin}}");
    }
  });
  it("targets every write tool, a live write tool of each kind, and the secret", () => {
    const targets = new Set(ds.cases.map((c) => c.attack));
    for (const w of WRITE_TOOLS) expect(targets.has(w), w).toBe(true);
    for (const w of LIVE_WRITE_TOOLS) expect(targets.has(w), w).toBe(true);
    expect(targets.has("secret")).toBe(true);
  });
  it("refuses a live vector with a scripted prior, or with the wrong live source", () => {
    const base = { version: "x", provenance: PROV };
    const live = { id: "l", vector: "mcp_output", prompt: "p", live: { source: "mcp", doc: "d", payload: "x" }, attack: "secret", tags: [] };
    expect(parseInjectionDataset({ ...base, cases: [live] }).cases).toHaveLength(1);
    expect(() => parseInjectionDataset({ ...base, cases: [{ ...live, prior: { tool: "node_status", args: {}, wrap: "raw", payload: "x" } }] })).toThrow(/no scripted prior/);
    expect(() => parseInjectionDataset({ ...base, cases: [{ ...live, live: { source: "browser", page: "<p>" } }] })).toThrow(/live.source "mcp"/);
    expect(() => parseInjectionDataset({ ...base, cases: [{ ...live, live: { source: "mcp", doc: "../x", payload: "x" } }] })).toThrow(/live.doc/);
    expect(() => parseInjectionDataset({ ...base, cases: [{ ...live, attack: "browser_back" }] })).toThrow(/attack/);
  });
  it("the single-turn runner scores only scripted cases and reports how many live cases it left out", async () => {
    const sc = await runEvalSuite({
      complete: async () => ({ content: "ok" }),
      model: "m",
      toolcall: { version: "t", tasks: [] },
      injection: { version: ds.version, cases: ds.cases },
      now: () => "2026-10-04T00:00:00.000Z",
    });
    const live = ds.cases.filter((c) => !isScriptedCase(c)).length;
    expect(sc.nInjection).toBe(ds.cases.length - live);
    expect(sc.injectionLiveNotScored).toBe(live);
  });
});
