// HUP-S7.6 follow-up (US-7.4 AC1) — a plain chat turn reports a plan to the Activity monitor.
//
// BDD:
//   Given a chat turn where the model asks for tools, when each model step asks for them, then a
//     plan event lists one row per step with the tools it asked for (in-app loop and sidecar).
//   Given the sidecar reuses call ids on a later step, then both steps are still in the plan;
//     a replayed event for the same step and call is not counted twice.
//   Given a workflow run, then the provider adds no chat plan (the workflow reports its own).
//   Given a chat plan, then the monitor shows each row as done, running or stopped, and a chat
//     plan never replaces a workflow's plan in the same turn.
//   Given a model that answers without tools, then no plan event is emitted.
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { ChatPlan, chatPlanLabel, MAX_CHAT_PLAN_ROWS, MAX_TOOLS_PER_ROW } from "./chatPlan";
import { createAgentProvider, type TurnActivityEvent } from "./harness";
import { createSidecarProvider, type SidecarSessionApi } from "./sidecarProvider";
import { IDLE_ACTIVITY, beginTurn, chatPlanStates, endTurn, planReported, turnActivity } from "../shell/slices/turnActivity";
import { buildMonitorSnapshot, isMonitorSnapshot, type MonitorInputs } from "../popout/monitorSnapshot";
import { ActivityMonitor } from "../popout/ActivityMonitor";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

beforeEach(() => turnActivity.set(IDLE_ACTIVITY));

describe("ChatPlan", () => {
  it("groups the model's tool calls by step, in step order, and counts repeats", () => {
    const p = new ChatPlan();
    expect(p.note(2, "c9", "file_write")).toBe(true);
    expect(p.note(1, "c1", "web_search")).toBe(true);
    expect(p.note(1, "c2", "fetch_url")).toBe(true);
    expect(p.note(2, "c10", "file_write")).toBe(true);
    expect(p.steps()).toEqual(["Step 1: web_search, fetch_url", "Step 2: file_write x2"]);
  });

  it("dedupes a replayed call within its step but keeps a reused id on a later step", () => {
    const p = new ChatPlan();
    expect(p.note(1, "call_0", "node_status")).toBe(true);
    expect(p.note(1, "call_0", "node_status")).toBe(false);
    expect(p.note(2, "call_0", "memory_search")).toBe(true);
    expect(p.steps()).toEqual(["Step 1: node_status", "Step 2: memory_search"]);
  });

  it("refuses a bad step, an empty name and rows past the cap", () => {
    const p = new ChatPlan();
    expect(p.note(0, "a", "x")).toBe(false);
    expect(p.note(1.5, "a", "x")).toBe(false);
    expect(p.note(1, "a", "")).toBe(false);
    expect(p.steps()).toEqual([]);
    for (let i = 1; i <= MAX_CHAT_PLAN_ROWS; i++) expect(p.note(i, "c", "t")).toBe(true);
    expect(p.note(MAX_CHAT_PLAN_ROWS + 1, "c", "t")).toBe(false);
    // An existing row still takes more calls.
    expect(p.note(1, "d", "t")).toBe(true);
    expect(p.steps()).toHaveLength(MAX_CHAT_PLAN_ROWS);
  });

  it("names at most a few tools per row and counts the rest", () => {
    const tools = Array.from({ length: MAX_TOOLS_PER_ROW + 2 }, (_, i) => `t${i}`);
    expect(chatPlanLabel(3, tools)).toBe(`Step 3: ${tools.slice(0, MAX_TOOLS_PER_ROW).join(", ")}, and 2 more`);
  });
});

describe("the in-app loop reports a chat turn's plan", () => {
  it("emits the plan once per tool step, before that step's tools run", async () => {
    const replies = [
      JSON.stringify({
        role: "assistant",
        content: null,
        tool_calls: [
          { id: "c1", function: { name: "node_status", arguments: "{}" } },
          { id: "c2", function: { name: "memory_search", arguments: "{}" } },
        ],
      }),
      JSON.stringify({ role: "assistant", content: null, tool_calls: [{ id: "c3", function: { name: "journal_read", arguments: "{}" } }] }),
      JSON.stringify({ role: "assistant", content: "Done." }),
    ];
    const order: string[] = [];
    const events: TurnActivityEvent[] = [];
    const p = createAgentProvider("local", () => ({}) as never, async () => replies.shift() ?? "");
    await p.send({
      messages: [{ role: "user", content: "status and notes?" }],
      callbacks: {
        onStatus: vi.fn(),
        onToken: vi.fn(),
        onToolCall: vi.fn(async (c) => {
          order.push("run " + c.name);
          return "{}";
        }),
        onActivity: (e) => {
          events.push(e);
          if (e.kind === "plan") order.push("plan " + e.steps.length);
        },
      },
    });
    expect(events.filter((e) => e.kind === "plan")).toEqual([
      { kind: "plan", source: "chat", steps: ["Step 1: node_status, memory_search"] },
      { kind: "plan", source: "chat", steps: ["Step 1: node_status, memory_search", "Step 2: journal_read"] },
    ]);
    expect(order).toEqual(["plan 1", "run node_status", "run memory_search", "plan 2", "run journal_read"]);
  });

  it("a direct answer reports no plan", async () => {
    const events: TurnActivityEvent[] = [];
    const p = createAgentProvider("local", () => ({}) as never, async () => JSON.stringify({ role: "assistant", content: "hi" }));
    await p.send({ messages: [{ role: "user", content: "hi" }], callbacks: { onStatus: vi.fn(), onToken: vi.fn(), onToolCall: vi.fn(), onActivity: (e) => events.push(e) } });
    expect(events.some((e) => e.kind === "plan")).toBe(false);
  });
});

type Ev = { seq: number; event: Record<string, unknown> };

/** A sidecar session that hands out one page of events per poll. */
function pagedApi(pages: Ev[][]): SidecarSessionApi {
  let page = 0;
  return {
    open: vi.fn(async () => "s1"),
    send: vi.fn(async () => {}),
    events: vi.fn(async (_id: string, after: number) => {
      const p = pages[page];
      if (!p) return { events: [], lastSeq: after, busy: false };
      page += 1;
      return { events: p, lastSeq: p[p.length - 1].seq, busy: true };
    }),
    toolResult: vi.fn(async () => {}),
    stop: vi.fn(async () => {}),
  };
}

describe("the sidecar loop reports a chat turn's plan", () => {
  it("builds rows from its tool_call events, including a reused id on a later step and a replay", async () => {
    const step1: Ev[] = [
      { seq: 1, event: { type: "step_start", step: 1 } },
      { seq: 2, event: { type: "tool_call", step: 1, host: "sidecar", call: { id: "call_0", name: "web_search", arguments: "{}" } } },
      { seq: 3, event: { type: "tool_result", step: 1, call_id: "call_0", status: "ok", content: "[]" } },
    ];
    const step2: Ev[] = [
      // A re-delivered event (seq at or below what was processed) is skipped.
      { seq: 2, event: { type: "tool_call", step: 1, host: "sidecar", call: { id: "call_0", name: "web_search", arguments: "{}" } } },
      { seq: 4, event: { type: "step_start", step: 2 } },
      // No step on the event: the latest step_start is used.
      { seq: 5, event: { type: "tool_call", host: "sidecar", call: { id: "call_0", name: "sheet_read", arguments: "{}" } } },
      { seq: 6, event: { type: "tool_result", step: 2, call_id: "call_0", status: "ok", content: "{}" } },
      { seq: 7, event: { type: "final", content: "ok" } },
      { seq: 8, event: { type: "done", outcome: "answered" } },
    ];
    const events: TurnActivityEvent[] = [];
    const p = createSidecarProvider(pagedApi([step1, step2]), () => "p", () => []);
    await p.send({ messages: [{ role: "user", content: "x" }], callbacks: { onStatus: vi.fn(), onToken: vi.fn(), onToolCall: vi.fn(), onActivity: (e) => events.push(e) } });
    expect(events.filter((e) => e.kind === "plan")).toEqual([
      { kind: "plan", source: "chat", steps: ["Step 1: web_search"] },
      { kind: "plan", source: "chat", steps: ["Step 1: web_search", "Step 2: sheet_read"] },
    ]);
  });

  it("a workflow's own plan is forwarded as is, with no chat rows", async () => {
    const pages: Ev[][] = [
      [
        { seq: 1, event: { type: "plan", steps: ["write-test", "implement"] } },
        { seq: 2, event: { type: "final", content: "ok" } },
        { seq: 3, event: { type: "done", outcome: "answered" } },
      ],
    ];
    const events: TurnActivityEvent[] = [];
    const p = createSidecarProvider(pagedApi(pages), () => "p", () => []);
    await p.send({ messages: [{ role: "user", content: "x" }], callbacks: { onStatus: vi.fn(), onToken: vi.fn(), onToolCall: vi.fn(), onActivity: (e) => events.push(e) } });
    expect(events.filter((e) => e.kind === "plan")).toEqual([{ kind: "plan", steps: ["write-test", "implement"] }]);
  });

  it("a workflow run's own tool calls add no chat rows to its plan", async () => {
    const pages: Ev[][] = [
      [
        { seq: 1, event: { type: "plan", steps: ["write-test", "implement"] } },
        { seq: 2, event: { type: "step_start", step: 1 } },
        { seq: 3, event: { type: "tool_call", step: 1, host: "sidecar", call: { id: "call_0", name: "fs_write", arguments: "{}" } } },
        { seq: 4, event: { type: "tool_result", step: 1, call_id: "call_0", status: "ok", content: "{}" } },
      ],
    ];
    const api = pagedApi(pages);
    api.trackWorkflowRun = vi.fn(async () => ({ run_id: "r1" }));
    api.workflowStatus = vi.fn(async () => ({ run_id: "r1", workflow_id: "w", state: "verified" }) as const);
    const events: TurnActivityEvent[] = [];
    const p = createSidecarProvider(api, () => "p", () => []);
    const view = await p.runWorkflow!("w", { callbacks: { onStatus: vi.fn(), onToken: vi.fn(), onToolCall: vi.fn(), onActivity: (e) => events.push(e) } });
    expect(view.state).toBe("verified");
    expect(events.filter((e) => e.kind === "plan")).toEqual([{ kind: "plan", steps: ["write-test", "implement"] }]);
  });
});

describe("a chat plan in the turn slice and the monitor", () => {
  const inputs = (over: Partial<MonitorInputs> = {}): MonitorInputs => ({
    activity: turnActivity.get(),
    providerKind: "local",
    providerLabel: "local model · llama-server · agentic",
    modelLabel: "Gemma 4 E4B",
    modelId: "local:gemma",
    tier: "T0",
    localCtxTokens: 8192,
    now: 10_000,
    ...over,
  });

  let root: Root | null = null;
  let host: HTMLDivElement | null = null;
  afterEach(() => {
    act(() => root?.unmount());
    host?.remove();
    root = null;
    host = null;
  });
  const rows = () => {
    host = document.createElement("div");
    document.body.appendChild(host);
    root = createRoot(host);
    const snap = buildMonitorSnapshot(inputs());
    expect(isMonitorSnapshot(snap)).toBe(true);
    act(() => root!.render(<ActivityMonitor snapshot={snap} now={12_000} onStop={() => {}} />));
    return Array.from(host.querySelectorAll('[data-testid="mon-plan-row"]')).map((r) => r.textContent);
  };

  it("shows earlier rows done and the latest running while the turn runs, then done once answered", () => {
    beginTurn("local", "local", 1000);
    planReported(["Step 1: node_status"], "chat");
    planReported(["Step 1: node_status", "Step 2: journal_read"], "chat");
    expect(turnActivity.get().planSource).toBe("chat");
    expect(rows()).toEqual(["Step 1: node_status done", "Step 2: journal_read running"]);
    act(() => root?.unmount());
    host?.remove();
    endTurn("answered", 2000);
    expect(rows()).toEqual(["Step 1: node_status done", "Step 2: journal_read done"]);
  });

  it("a stopped or failed turn shows its latest row stopped", () => {
    expect(chatPlanStates(["a", "b"], { state: "idle", outcome: "stopped" })).toEqual([
      { step: "a", state: "done" },
      { step: "b", state: "stopped" },
    ]);
    expect(chatPlanStates(["a"], { state: "idle", outcome: "failed" })).toEqual([{ step: "a", state: "stopped" }]);
    expect(chatPlanStates(["a"], { state: "stopping", outcome: null })).toEqual([{ step: "a", state: "running" }]);
  });

  it("a chat plan never replaces a workflow's plan in the same turn, and a new turn starts empty", () => {
    beginTurn("sidecar", "Hermes · workflow", 1000);
    planReported(["write-test", "implement"]);
    planReported(["Step 1: shell_run"], "chat");
    expect(turnActivity.get().plan).toEqual(["write-test", "implement"]);
    expect(turnActivity.get().planSource).toBe("workflow");
    beginTurn("local", "local", 3000);
    expect(turnActivity.get().plan).toBeNull();
    // Nothing is recorded outside a turn.
    endTurn("answered", 4000);
    planReported(["Step 1: x"], "chat");
    expect(turnActivity.get().plan).toBeNull();
  });

  it("a snapshot row with an unknown state is refused", () => {
    beginTurn("local", "local", 1000);
    planReported(["Step 1: node_status"], "chat");
    const snap = buildMonitorSnapshot(inputs());
    const bad = { ...snap, turn: { ...snap.turn, plan: [{ step: "Step 1: node_status", state: "finished" }] } };
    expect(isMonitorSnapshot(bad)).toBe(false);
  });
});
