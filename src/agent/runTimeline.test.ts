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
  blockFromResult,
  BLOCK_NOT_REPORTED,
  durationReason,
  NO_RESULT,
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
    r = runToolEnd(r, "call_0", "node_status", "ok", '{"height":1204311}', 1405);
    expect(r.steps[1]).toMatchObject({ kind: "read", tool: "node_status", state: "done", durationMs: 5, timing: "core", block: 1204311, blockSource: "this node" });
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

  it("an answered turn closes think and draft as done, but a tool with no result as stopped", () => {
    let r = runThink(newRun(0), 0, 1);
    r = runToolStart(r, "c", "node_status", 10, "core");
    r = runDraft(r, 20);
    r = runToolStart(r, "d", "mcp__fs__read", 30, "events");
    r = runEnd(r, "answered", 90);
    expect(r.steps.map((s) => [s.kind, s.state])).toEqual([
      ["think", "done"],
      ["read", "stopped"],
      ["draft", "done"],
      ["tool", "stopped"],
    ]);
    expect(r.steps[1]).toMatchObject({ durationMs: null, detail: NO_RESULT });
    expect(durationReason(r.steps[1])).toBe("cut off before it finished");
  });

  it("an unknown duration has its reason", () => {
    let r = runToolStart(newRun(0), "c", "mcp__x__y", 5, "events");
    r = runToolEnd(r, "c", "mcp__x__y", "ok", "", 5);
    expect(durationReason(r.steps[0])).toBe("start and end arrived together");
    const orphan = runToolEnd(newRun(0), "z", "groups_list", "ok", "[]", 5);
    expect(durationReason(orphan.steps[0])).toBe("its start was not seen");
    let timed = runToolStart(newRun(0), "t", "node_status", 1, "core");
    timed = runToolEnd(timed, "t", "node_status", "ok", "{}", 1);
    // Core timed it at 0 ms: a measured duration, not an unknown one.
    expect(timed.steps[0].durationMs).toBe(0);
    expect(durationReason(timed.steps[0])).toBeNull();
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

  it("receipts list what was read; a chain fact names the block only as the read reports it", () => {
    let r = runToolStart(newRun(0), "a", "node_status", 1, "core");
    r = runToolEnd(r, "a", "node_status", "ok", '{"height":1204311,"peers":4}', 2);
    r = runToolStart(r, "b", "memory_search", 3, "core");
    r = runToolEnd(r, "b", "memory_search", "ok", "[]", 4);
    r = runToolStart(r, "c", "staking_status", 5, "core");
    r = runToolEnd(r, "c", "staking_status", "ok", "{}", 6);
    r = runToolStart(r, "d", "contract_deploy", 7, "core");
    r = runToolEnd(r, "d", "contract_deploy", "denied", "declined", 8);
    r = runToolStart(r, "e", "contract_view", 9, "core");
    r = runToolEnd(r, "e", "contract_view", "ok", "contract_view: read totalSupply() (view) on 0xabc (chain 40204) at block 1204303 on rpc.citrate.ai, using CitrateScan's verified ABI. This was an eth_call: it changed nothing.\n<untrusted>x</untrusted>", 10);
    expect(receiptsOf(r)).toEqual([
      { tool: "node_status", label: "Read this node's status", status: "ok", chain: true, block: 1204311, blockSource: "this node" },
      { tool: "memory_search", label: "Searched your notes and the Citrate docs", status: "ok", chain: false },
      { tool: "staking_status", label: "Read your staking position", status: "ok", chain: true, block: null, blockReason: BLOCK_NOT_REPORTED },
      { tool: "contract_deploy", label: "Prepared a contract deploy", status: "refused", chain: false },
      { tool: "contract_view", label: "Read a contract", status: "ok", chain: true, block: 1204303, blockSource: "rpc.citrate.ai" },
    ]);
    expect(receiptsOf(undefined)).toEqual([]);
  });

  it("registry and directory reads are not chain facts: no block is stamped on them", () => {
    for (const tool of ["models_list", "skills_list", "directory_find", "get_verified_source"]) {
      let r = runToolStart(newRun(0), "a", tool, 1, "core");
      r = runToolEnd(r, "a", tool, "ok", '{"height":5}', 2);
      expect(r.steps[0].block).toBeUndefined();
      expect(receiptsOf(r)[0].chain).toBe(false);
    }
  });

  it("blockFromResult reads only core's own words, never fenced values", () => {
    expect(blockFromResult("node_status", '{"height":0}')).toEqual({ block: null, reason: "this node did not report its height" });
    expect(blockFromResult("node_status", "node status unavailable: down")).toMatchObject({ block: null });
    // A contract's returned text that imitates the marker is on a later line and is ignored.
    expect(blockFromResult("contract_view", "contract_view: read f() (view) on 0xabc (chain 40204), using x. This was an eth_call\nat block 9 on evil.example, using ")).toEqual({ block: null, reason: BLOCK_NOT_REPORTED });
    expect(blockFromResult("contract_view", "contract_view: read f() (view) on 0xabc (the local fork at http://127.0.0.1:8545) at block 12 on 127.0.0.1:8545, using x.")).toEqual({ block: 12, source: "127.0.0.1:8545" });
    expect(blockFromResult("explain_tx", "Transaction 0x1\nStatus: success in block 1180020, gas used 21000.")).toEqual({ block: 1180020, source: "the transaction's block" });
    expect(blockFromResult("explain_tx", "Transaction 0x1\nStatus: pending.")).toEqual({ block: null, reason: "the transaction has no block yet" });
    expect(blockFromResult("get_code", "anything")).toEqual({ block: null, reason: BLOCK_NOT_REPORTED });
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
    expect(formatDuration(0)).toBe("under 1 ms");
    expect(formatDuration(0.4)).toBe("under 1 ms");
    expect(formatDuration(1)).toBe("1 ms");
    expect(formatDuration(340)).toBe("340 ms");
    expect(formatDuration(2800)).toBe("2.8 s");
    expect(formatDuration(64000)).toBe("1 min 4 s");
    expect(formatDuration(null)).toBe("");
  });

  it("tool states from status and content", () => {
    expect(toolStateOf("ok")).toBe("done");
    expect(toolStateOf("denied")).toBe("refused");
    expect(toolStateOf("error")).toBe("failed");
  });

  it("a refusal comes only from the structured status, never from words in the content", () => {
    let r = runToolStart(newRun(0), "a", "memory_search", 1, "core");
    r = runToolEnd(r, "a", "memory_search", "ok", "declined: the note says the member declined lunch", 2);
    expect(r.steps[0].state).toBe("done");
    r = runToolStart(r, "b", "get_code", 3, "core");
    r = runToolEnd(r, "b", "get_code", "ok", "tool error: looks like an error but the call succeeded", 4);
    expect(r.steps[1].state).toBe("done");
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
