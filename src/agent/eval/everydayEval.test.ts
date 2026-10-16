// HUP-S10.2 — the everyday eval fragment (toolcall-v2.d/everyday.json) is valid against the real
// AGENT_TOOLS. Uses the v1 task parser, so the fragment is checked on this branch before the v2
// fragment loader (A50) merges the directory.
import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { parseToolcallDataset } from "./runner";
import v1 from "./toolcall-v1.json";

const fragment = JSON.parse(readFileSync(join(__dirname, "toolcall-v2.d", "everyday.json"), "utf8")) as {
  added: string;
  by: string;
  note?: string;
  tasks: { id: string; expect: { tool: string | null } }[];
};

describe("toolcall-v2.d/everyday.json", () => {
  it("has only fragment keys, a date and an owner", () => {
    expect(Object.keys(fragment).sort()).toEqual(["added", "by", "note", "tasks"]);
    expect(fragment.added).toMatch(/^\d{4}-\d{2}-\d{2}$/);
    expect(fragment.by).toMatch(/HUP-S10\.2/);
  });

  it("parses against the real tools and adds no id that v1 already has", () => {
    const ds = parseToolcallDataset({
      version: "toolcall-v2-everyday-check",
      provenance: { author: "Larry Klosowski + Claude Opus 5.5", created: fragment.added, purpose: fragment.by, disjointFromTraining: true },
      tasks: fragment.tasks,
    });
    const v1Ids = new Set(v1.tasks.map((t) => t.id));
    for (const t of ds.tasks) expect(v1Ids.has(t.id), t.id).toBe(false);
  });

  it("covers every everyday tool", () => {
    const tools = new Set(fragment.tasks.map((t) => t.expect.tool));
    for (const n of ["gsheets_read", "gsheets_append", "schedule_list", "schedule_add", "calendar_list"]) expect(tools.has(n), n).toBe(true);
  });
});
