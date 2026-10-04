// HUP-S6.7 / US-6.3 AC2 — the toolcall-v2 fragment for contract_view: well formed, unique ids,
// and every expected tool is one Hermes really has (the v2 loader merges fragments by file).
import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { AGENT_TOOLS } from "../harness";
import { parseToolcallDataset } from "./runner";

const RAW = readFileSync("src/agent/eval/toolcall-v2.d/contract.json", "utf8");
const FRAG = JSON.parse(RAW) as {
  version: string;
  tasks: { id: string; prompt: string; expect: { tool: string | null; argsMatch?: Record<string, string>; alsoAccept?: string[] }; tags: string[] }[];
};
const v1 = JSON.parse(readFileSync("src/agent/eval/toolcall-v1.json", "utf8")) as { tasks: { id: string }[] };

describe("toolcall-v2 fragment: contract", () => {
  it("is a toolcall-v2 fragment the dataset validator accepts", () => {
    expect(FRAG.version).toBe("toolcall-v2");
    // The same checks as every dataset: real tools, declared argument names, valid regexes,
    // no write tool in alsoAccept, the train/eval disjointness flag.
    expect(parseToolcallDataset(FRAG).tasks).toHaveLength(FRAG.tasks.length);
    const ids = FRAG.tasks.map((t) => t.id);
    expect(new Set(ids).size).toBe(ids.length);
    for (const id of ids) expect(v1.tasks.map((t) => t.id)).not.toContain(id);
    for (const t of FRAG.tasks) {
      expect(t.prompt.length).toBeGreaterThan(10);
      expect(Array.isArray(t.tags) && t.tags.length > 0).toBe(true);
      for (const re of Object.values(t.expect.argsMatch ?? {})) expect(() => new RegExp(re.replace(/^re:/, ""))).not.toThrow();
    }
  });

  it("names only real tools, and covers contract_view", () => {
    const names = new Set(AGENT_TOOLS.map((t) => t.function.name));
    for (const t of FRAG.tasks) {
      if (t.expect.tool) expect(names.has(t.expect.tool), t.expect.tool).toBe(true);
      for (const a of t.expect.alsoAccept ?? []) expect(names.has(a), a).toBe(true);
    }
    expect(FRAG.tasks.filter((t) => t.expect.tool === "contract_view").length).toBeGreaterThanOrEqual(3);
  });
});
