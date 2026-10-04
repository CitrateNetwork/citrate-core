// HUP-S1.7 / S1.10 — the versioned eval datasets are schema-checked against the REAL
// AGENT_TOOLS, so a renamed/removed tool (or a typo in a dataset) fails CI instead of
// silently scoring every model as wrong.
import { describe, it, expect } from "vitest";
import toolcallJson from "./toolcall-v1.json";
import injectionJson from "./injection-v1.json";
import { parseToolcallDataset, parseInjectionDataset, WRITE_TOOLS, TOOL_NAMES } from "./runner";

describe("dataset validators reject malformed data", () => {
  const good = {
    version: "toolcall-v1",
    provenance: { author: "a", created: "2026-09-30", purpose: "p", disjointFromTraining: true },
    tasks: [{ id: "a", prompt: "p", expect: { tool: "node_status" }, tags: ["x"] }],
  };
  it("accepts a well-formed toolcall dataset", () => {
    expect(parseToolcallDataset(good).tasks).toHaveLength(1);
  });
  it("rejects an unknown tool", () => {
    const bad = { ...good, tasks: [{ ...good.tasks[0], expect: { tool: "nodeStatus" } }] };
    expect(() => parseToolcallDataset(bad)).toThrow(/nodeStatus/);
  });
  it("rejects duplicate ids", () => {
    const bad = { ...good, tasks: [good.tasks[0], good.tasks[0]] };
    expect(() => parseToolcallDataset(bad)).toThrow(/duplicate/i);
  });
  it("rejects an argsMatch field the tool does not declare, and a bad regex", () => {
    const f = { ...good, tasks: [{ ...good.tasks[0], expect: { tool: "group_roster", argsMatch: { grp: "x" } } }] };
    expect(() => parseToolcallDataset(f)).toThrow(/grp/);
    const r = { ...good, tasks: [{ ...good.tasks[0], expect: { tool: "group_roster", argsMatch: { group: "re:(" } } }] };
    expect(() => parseToolcallDataset(r)).toThrow(/regex/i);
  });
  it("rejects a dataset without the train/eval disjointness provenance flag", () => {
    const bad = { ...good, provenance: { ...good.provenance, disjointFromTraining: false } };
    expect(() => parseToolcallDataset(bad)).toThrow(/disjoint/i);
  });
});

describe("toolcall-v1.json", () => {
  const ds = parseToolcallDataset(toolcallJson);
  it("is versioned and carries provenance", () => {
    expect(ds.version).toBe("toolcall-v1");
    expect(ds.provenance.author).toMatch(/Larry Klosowski/);
  });
  it("has ≥ 40 tasks with unique ids", () => {
    expect(ds.tasks.length).toBeGreaterThanOrEqual(40);
    expect(new Set(ds.tasks.map((t) => t.id)).size).toBe(ds.tasks.length);
  });
  it("every expected tool exists in AGENT_TOOLS", () => {
    for (const t of ds.tasks) {
      if (t.expect.tool !== null) expect(TOOL_NAMES.has(t.expect.tool)).toBe(true);
      for (const a of t.expect.alsoAccept ?? []) expect(TOOL_NAMES.has(a)).toBe(true);
    }
  });
  it("alsoAccept never lists a write tool (a write is only ever the explicit expect.tool)", () => {
    for (const t of ds.tasks) for (const a of t.expect.alsoAccept ?? []) expect(WRITE_TOOLS.has(a)).toBe(false);
  });
  it("covers every required category", () => {
    const tags = new Set(ds.tasks.flatMap((t) => t.tags));
    for (const c of ["tool-choice", "no-tool", "args", "snake_case", "ambiguity", "no-write"]) expect(tags.has(c)).toBe(true);
    expect(ds.tasks.filter((t) => t.expect.tool === null).length).toBeGreaterThanOrEqual(8);
  });
  it("is the frozen 57-task set every v1 scorecard used (A50; new tools go to toolcall-v2)", () => {
    expect(ds.tasks).toHaveLength(57);
  });
});

describe("injection-v1.json", () => {
  const ds = parseInjectionDataset(injectionJson);
  it("is versioned and carries provenance", () => {
    expect(ds.version).toBe("injection-v1");
    expect(ds.provenance.author).toMatch(/Larry Klosowski/);
  });
  it("has ≥ 20 cases with unique ids", () => {
    expect(ds.cases.length).toBeGreaterThanOrEqual(20);
    expect(new Set(ds.cases.map((c) => c.id)).size).toBe(ds.cases.length);
  });
  it("every scripted prior call is a real READ tool, and every attack names a write tool or the secret", () => {
    for (const c of ds.cases) {
      expect(TOOL_NAMES.has(c.prior!.tool)).toBe(true);
      expect(WRITE_TOOLS.has(c.prior!.tool)).toBe(false);
      expect(c.attack === "secret" || WRITE_TOOLS.has(c.attack)).toBe(true);
    }
  });
  it("covers the four scripted vectors and the secret (write-tool coverage is checked on v2)", () => {
    const vectors = new Set(ds.cases.map((c) => c.vector));
    for (const v of ["tool_result", "web_page", "registry", "skill_body"]) expect(vectors.has(v as never)).toBe(true);
    expect(new Set(ds.cases.map((c) => c.attack)).has("secret")).toBe(true);
  });
  it("is the frozen 23-case set every v1 scorecard used", () => {
    expect(ds.cases).toHaveLength(23);
  });
});
