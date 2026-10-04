// @vitest-environment node
// HUP-S7.7 / US-9.2 AC1 — Citrate QA through a real sidecar: the session body core would send for a
// QA turn, core's memory_search answer from the daemon, and the answer and skill reads read back
// from the session's own events. Red-first.
import { describe, it, expect } from "vitest";
import { MEMORY_SEARCH_TOOL } from "../knowledgeSearch";
import { AGENT_TOOL_ANNOTATIONS } from "../toolAnnotations";
import { QA_SYSTEM_PROMPT } from "./qa";
import { SKILL_LOAD_TOOL, qaCoreAnswerer, qaOutcomeFromEvents, qaSessionBody, type QaCoreLog } from "./qaSidecar";
import type { SidecarEvent } from "./sidecar";

const DOCS = [
  "tenant 'citrate-docs' — 1688 nodes, showing 1:",
  "  0a1b2c3d4e 0.812 [doc] Paraconsistent aggregation › The four values",
  "    cite: citrate-docs:content/research/paraconsistent.md#the-four-values",
  "    > Both: sources of comparable trust genuinely disagree.",
  "",
].join("\n");

const ev = (seq: number, event: Record<string, unknown>): SidecarEvent => ({ seq, event: event as SidecarEvent["event"] });

describe("qaSessionBody", () => {
  it("opens a session like core: the QA instruction and exactly memory_search as a core read tool", () => {
    const b = qaSessionBody({ model: "m", baseUrl: "http://127.0.0.1:1/v1", bearer: "k", contextTokens: 8192, maxTokens: 2048, systemPrompt: QA_SYSTEM_PROMPT });
    expect(b.systemPrompt).toBe(QA_SYSTEM_PROMPT);
    expect(b.llm).toEqual({ baseUrl: "http://127.0.0.1:1/v1", bearer: "k" });
    expect(b.hicAware).toBe(true);
    expect(b.maxToolsPerRequest).toBe(8);
    const tools = b.tools as Record<string, unknown>[];
    expect(tools).toHaveLength(1);
    expect(tools[0]).toMatchObject({
      name: "memory_search",
      description: MEMORY_SEARCH_TOOL.function.description,
      parameters: MEMORY_SEARCH_TOOL.function.parameters,
      host: "core",
      annotations: { effect: AGENT_TOOL_ANNOTATIONS.memory_search.effect, trust: AGENT_TOOL_ANNOTATIONS.memory_search.trust, read_only: true },
    });
    expect(qaSessionBody({ model: "m", baseUrl: "u", contextTokens: 4096, maxTokens: 1024, systemPrompt: "s" }).llm).toEqual({ baseUrl: "u", bearer: "" });
  });
});

describe("qaCoreAnswerer", () => {
  it("runs memory_search on the daemon with the app's tenant rule and budget, and records the nodes", async () => {
    const searches: [string, string, number, boolean][] = [];
    const log: QaCoreLog = { calls: [], retrieved: [] };
    const answer = qaCoreAnswerer(async (t, q, k, p) => {
      searches.push([t, q, k, p]);
      return DOCS;
    }, log);
    const a = await answer("memory_search", JSON.stringify({ query: "Belnap Both", tenant: "nonsense" }));
    expect(a.status).toBe("ok");
    expect(a.content).toContain("cite as citrate-docs:content/research/paraconsistent.md#the-four-values");
    expect(a.content).toContain("genuinely disagree");
    expect(searches).toEqual([["citrate-docs", "Belnap Both", 5, true]]);
    expect(log.calls).toEqual([{ tenant: "citrate-docs", query: "Belnap Both" }]);
    expect(log.retrieved).toEqual([{ id: "0a1b2c3d4e", cite: "citrate-docs:content/research/paraconsistent.md#the-four-values" }]);
  });
  it("answers personal searches with titles only, and refuses other tools and bad arguments without searching", async () => {
    const searches: [string, string, number, boolean][] = [];
    const log: QaCoreLog = { calls: [], retrieved: [] };
    const answer = qaCoreAnswerer(async (t, q, k, p) => {
      searches.push([t, q, k, p]);
      return "tenant 'personal' — 0 nodes, showing 0:\n";
    }, log);
    expect((await answer("memory_search", '{"query":"x","tenant":"personal"}')).content).toMatch(/No results in the personal memory/);
    expect(searches).toEqual([["personal", "x", 6, false]]);
    expect(await answer("node_status", "{}")).toEqual({ status: "error", content: expect.stringMatching(/only memory_search is offered/) });
    expect((await answer("memory_search", "{not json")).status).toBe("error");
    expect(searches).toHaveLength(1);
  });
  it("lets a daemon failure abort the run", async () => {
    const answer = qaCoreAnswerer(async () => {
      throw new Error("cannot reach the memory daemon");
    }, { calls: [], retrieved: [] });
    await expect(answer("memory_search", '{"query":"x"}')).rejects.toThrow(/memory daemon/);
  });
});

describe("qaOutcomeFromEvents", () => {
  it("returns the last final reply, the skills read with skill_load, and the done outcome", () => {
    const o = qaOutcomeFromEvents([
      ev(1, { type: "tool_call", host: "sidecar", call: { id: "s1", name: SKILL_LOAD_TOOL, arguments: '{"name":"citrate-paraconsensus"}' } }),
      ev(2, { type: "tool_call", host: "core", call: { id: "c1", name: "memory_search", arguments: '{"query":"q"}' } }),
      ev(3, { type: "final", content: "draft" }),
      ev(4, { type: "tool_call", host: "sidecar", call: { id: "s2", name: SKILL_LOAD_TOOL, arguments: "{}" } }),
      ev(5, { type: "final", content: "Both means contested (citrate-docs:content/research/paraconsistent.md#the-four-values)" }),
      ev(6, { type: "done", outcome: "answered" }),
    ]);
    expect(o).toEqual({
      text: "Both means contested (citrate-docs:content/research/paraconsistent.md#the-four-values)",
      skillLoads: ["citrate-paraconsensus", "(unnamed)"],
      outcome: "answered",
    });
  });
  it("gives an empty answer when the session ended without a final reply", () => {
    expect(qaOutcomeFromEvents([ev(1, { type: "done", outcome: "turn_limit" })])).toEqual({ text: "", skillLoads: [], outcome: "turn_limit" });
    expect(qaOutcomeFromEvents([])).toEqual({ text: "", skillLoads: [] });
  });
});
