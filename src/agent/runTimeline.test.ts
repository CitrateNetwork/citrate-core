// FL design board 01: the run timeline is written only from real turn events.
import { describe, it, expect } from "vitest";
import {
  formatDuration,
  kindForTool,
  MAX_RUN_STEPS,
  newRun,
  parseRun,
  receiptsOf,
  runDraft,
  runEnd,
  runModelTime,
  runSummary,
  runThink,
  runToolEnd,
  runToolStart,
  stepAnnouncement,
  stepLabel,
  toolLabel,
  toolStateOf,
} from "./runTimeline";

describe("run timeline reducers", () => {
  it("records think, a read, and the draft in order, each closed by the next", () => {
    let r = newRun(1000);
    r = runThink(r, 1000);
    r = runThink(r, 1000, 1); // the same step reported again names its number, adds no row
    expect(r.steps).toHaveLength(1);
    expect(r.steps[0].step).toBe(1);
    r = runToolStart(r, "call_0", "node_status", 1400, "core");
    expect(r.steps[0]).toMatchObject({ kind: "think", state: "done", durationMs: 400 });
    r = runToolEnd(r, "call_0", "node_status", "ok", '{"height":1204311}', 1405, { block: 1204311 });
    expect(r.steps[1]).toMatchObject({ kind: "read", tool: "node_status", state: "done", durationMs: 5, timing: "core", block: 1204311 });
    r = runDraft(r, 2000);
    r = runEnd(r, "answered", 2600);
    expect(r.steps.map((s) => [s.kind, s.state])).toEqual([
      ["think", "done"],
      ["read", "done"],
      ["draft", "done"],
    ]);
    expect(r.steps[2].durationMs).toBe(600);
    expect(r.outcome).toBe("answered");
  });

  it("never shows a duration for two events that arrived in the same page", () => {
    let r = runToolStart(newRun(0), "c1", "mcp__fs__read", 5000, "events");
    r = runToolEnd(r, "c1", "mcp__fs__read", "ok", "x", 5000);
    expect(r.steps[0].durationMs).toBeNull();
    expect(formatDuration(r.steps[0].durationMs)).toBe("");
  });

  it("the model server's own time replaces the measured think time", () => {
    let r = runThink(newRun(0), 0, 1);
    r = runModelTime(r, 2600);
    r = runToolStart(r, "c", "node_status", 9000, "core");
    expect(r.steps[0]).toMatchObject({ durationMs: 2600, timing: "model", state: "done" });
    expect(runModelTime(r, null)).toBe(r);
  });

  it("a refused call is refused with core's reason, a failed one failed", () => {
    let r = runToolStart(newRun(0), "c1", "contract_deploy", 10, "events");
    r = runToolEnd(r, "c1", "contract_deploy", "denied", "declined: the member said no. Nothing was done.", 20);
    expect(r.steps[0]).toMatchObject({ state: "refused", detail: "the member said no. Nothing was done." });
    r = runToolStart(r, "c2", "get_code", 30, "core");
    r = runToolEnd(r, "c2", "get_code", "error", "tool error: rpc unreachable", 40);
    expect(r.steps[1]).toMatchObject({ state: "failed", detail: "rpc unreachable" });
    expect(r.steps[1].block).toBeUndefined();
  });

  it("call ids repeat across model steps: each start gets its own row", () => {
    let r = runToolStart(newRun(0), "call_0", "node_status", 1, "core");
    r = runToolEnd(r, "call_0", "node_status", "ok", "a", 2);
    r = runToolStart(r, "call_0", "staking_status", 3, "core");
    r = runToolEnd(r, "call_0", "staking_status", "ok", "b", 4);
    expect(r.steps.map((s) => s.tool)).toEqual(["node_status", "staking_status"]);
  });

  it("a result whose start was not seen is added without a duration", () => {
    const r = runToolEnd(newRun(0), "x", "groups_list", "ok", "[]", 5);
    expect(r.steps[0]).toMatchObject({ state: "done", durationMs: null });
  });

  it("Stop closes running rows as stopped, with no duration", () => {
    let r = runThink(newRun(0), 0);
    r = runToolStart(r, "c", "node_status", 10, "core");
    r = runEnd(r, "stopped", 50);
    expect(r.steps[1]).toMatchObject({ state: "stopped", durationMs: null });
    // Nothing is added after the run ended.
    expect(runThink(r, 60)).toBe(r);
    expect(runToolStart(r, "d", "x", 60, "core")).toBe(r);
  });

  it("keeps at most MAX_RUN_STEPS rows", () => {
    let r = newRun(0);
    for (let i = 0; i < MAX_RUN_STEPS + 5; i++) r = runToolStart(r, "c" + i, "node_status", i, "core");
    expect(r.steps).toHaveLength(MAX_RUN_STEPS);
  });
});

describe("labels, receipts and summaries", () => {
  it("reads are the tools whose annotation changes nothing", () => {
    expect(kindForTool("node_status")).toBe("read");
    expect(kindForTool("contract_deploy")).toBe("tool");
    expect(kindForTool("mcp__x__y")).toBe("tool");
    expect(toolLabel("mcp__citrate-node__deploy_propose")).toBe("Used deploy_propose on MCP server citrate-node");
    expect(toolLabel("made_up")).toBe("Used made_up");
  });

  it("receipts list what was read, chain reads naming the block, unknown block as null", () => {
    let r = runToolStart(newRun(0), "a", "node_status", 1, "core");
    r = runToolEnd(r, "a", "node_status", "ok", "{}", 2, { block: 1204311 });
    r = runToolStart(r, "b", "memory_search", 3, "core");
    r = runToolEnd(r, "b", "memory_search", "ok", "[]", 4);
    r = runToolStart(r, "c", "staking_status", 5, "core");
    r = runToolEnd(r, "c", "staking_status", "ok", "{}", 6, { block: null });
    r = runToolStart(r, "d", "contract_deploy", 7, "core");
    r = runToolEnd(r, "d", "contract_deploy", "denied", "declined", 8);
    expect(receiptsOf(r)).toEqual([
      { tool: "node_status", label: "Read this node's status", status: "ok", chain: true, block: 1204311 },
      { tool: "memory_search", label: "Searched your notes and the Citrate docs", status: "ok", chain: false },
      { tool: "staking_status", label: "Read your staking position", status: "ok", chain: true, block: null },
      { tool: "contract_deploy", label: "Prepared a contract deploy", status: "refused", chain: false },
    ]);
    expect(receiptsOf(undefined)).toEqual([]);
  });

  it("summaries and the one-time step announcement", () => {
    let r = runThink(newRun(0), 0);
    expect(runSummary(r).title).toBe("Working");
    expect(stepAnnouncement(r)).toBe("Thinking.");
    r = runToolStart(r, "a", "node_status", 100, "core");
    expect(stepAnnouncement(r)).toBe("Read this node's status.");
    r = runEnd(runToolEnd(r, "a", "node_status", "ok", "", 200), "answered", 6100);
    expect(runSummary(r)).toEqual({ title: "Worked for 6.1 s", meta: "2 steps · 1 read · 0 actions" });
    expect(stepAnnouncement(r)).toBe("Hermes worked for 6.1 s.");
    expect(runSummary(runEnd(newRun(0), "stopped", 900)).title).toBe("Stopped after 900 ms");
    expect(stepLabel({ id: "x", kind: "draft", state: "running", startedAt: 0, endedAt: null, durationMs: null })).toBe("Writing the answer");
  });

  it("durations read plainly", () => {
    expect(formatDuration(0.4)).toBe("1 ms");
    expect(formatDuration(340)).toBe("340 ms");
    expect(formatDuration(2800)).toBe("2.8 s");
    expect(formatDuration(64000)).toBe("1 min 4 s");
    expect(formatDuration(null)).toBe("");
  });

  it("tool states from status and content", () => {
    expect(toolStateOf("ok", "fine")).toBe("done");
    expect(toolStateOf("ok", "declined: no")).toBe("refused");
    expect(toolStateOf("denied", "x")).toBe("refused");
    expect(toolStateOf("error", "x")).toBe("failed");
  });
});

describe("parseRun (sealed history read back)", () => {
  it("drops malformed steps and closes a step saved mid-run as stopped", () => {
    const r = parseRun({
      startedAt: 1,
      endedAt: null,
      outcome: null,
      steps: [{ id: "a", kind: "read", tool: "node_status", state: "running", startedAt: 1, endedAt: null, durationMs: null }, { kind: "bogus" }, "x"],
    });
    expect(r?.steps).toHaveLength(1);
    expect(r?.steps[0].state).toBe("stopped");
    expect(r?.outcome).toBe("stopped");
    expect(parseRun(null)).toBeUndefined();
    expect(parseRun({ steps: [] })).toBeUndefined();
  });
});
