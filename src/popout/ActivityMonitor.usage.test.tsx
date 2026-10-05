// HUP-S7.6 (US-7.4 AC1) — the Activity monitor shows what the turn is doing from the model
// server's own reports: context used and tokens per second (no longer always unknown), a workflow
// run's plan with each step's verifier state, the approvals asked this turn (pending, then
// decided), and the verifier verdicts. Nothing is estimated.
//
// BDD:
//   Given the in-app loop and a llama-server reply carrying usage and timings, then the turn's
//     usage is reported (createAgentProvider) and context used = prompt + written tokens.
//   Given the sidecar loop, then its usage and plan events and a held command's approval are
//     forwarded as activity (sidecarProvider).
//   Given that activity, then the snapshot and the pop-out show tokens/s, ctx used, the plan,
//     the approvals and the checks; Stop stays first in the tab order.
import { describe, it, expect, vi, afterEach, beforeEach } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { ActivityMonitor } from "./ActivityMonitor";
import { buildMonitorSnapshot, daemonsSection, isMonitorSnapshot, speedFor, usedFrom, type MonitorInputs } from "./monitorSnapshot";
import {
  IDLE_ACTIVITY,
  approvalNoted,
  beginTurn,
  endTurn,
  planReported,
  planStepStates,
  tokensPerSecond,
  turnActivity,
  usageReported,
  verifierReported,
} from "../shell/slices/turnActivity";
import { createAgentProvider, usageEventOf, type TurnActivityEvent } from "../agent/harness";
import { createSidecarProvider, type SidecarSessionApi } from "../agent/sidecarProvider";
import type { ShellPendingView } from "../bridge/domains";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

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

beforeEach(() => turnActivity.set(IDLE_ACTIVITY));

describe("usage from the model server", () => {
  it("parses core's citrate_usage and the sidecar's usage event alike, and refuses anything else", () => {
    expect(usageEventOf({ prompt_tokens: 812, completion_tokens: 40, generation_ms: 1333 })).toEqual({ kind: "usage", promptTokens: 812, completionTokens: 40, generationMs: 1333 });
    expect(usageEventOf({ type: "usage", step: 1, prompt_tokens: 5, completion_tokens: 2 })).toEqual({ kind: "usage", promptTokens: 5, completionTokens: 2, generationMs: null });
    for (const bad of [null, "x", {}, { prompt_tokens: 5 }, { prompt_tokens: -1, completion_tokens: 2 }, { prompt_tokens: 1.5, completion_tokens: 2 }, { prompt_tokens: "5", completion_tokens: 2 }]) {
      expect(usageEventOf(bad), JSON.stringify(bad)).toBeNull();
    }
  });

  it("the in-app loop reports each call's usage as activity", async () => {
    const events: TurnActivityEvent[] = [];
    const replies = [
      JSON.stringify({ role: "assistant", content: null, tool_calls: [{ id: "c1", function: { name: "node_status", arguments: "{}" } }], citrate_usage: { prompt_tokens: 700, completion_tokens: 12, generation_ms: 400 } }),
      JSON.stringify({ role: "assistant", content: "Height 7.", citrate_usage: { prompt_tokens: 760, completion_tokens: 30, generation_ms: 1000 } }),
    ];
    const p = createAgentProvider("local", () => ({}) as never, async () => replies.shift() ?? "");
    await p.send({
      messages: [{ role: "user", content: "height?" }],
      callbacks: { onStatus: vi.fn(), onToken: vi.fn(), onToolCall: vi.fn(async () => "{}"), onActivity: (e) => events.push(e) },
    });
    expect(events.filter((e) => e.kind === "usage")).toEqual([
      { kind: "usage", promptTokens: 700, completionTokens: 12, generationMs: 400 },
      { kind: "usage", promptTokens: 760, completionTokens: 30, generationMs: 1000 },
    ]);
  });

  it("a reply without usage reports none (unknown, never zero)", async () => {
    const events: TurnActivityEvent[] = [];
    const p = createAgentProvider("local", () => ({}) as never, async () => JSON.stringify({ role: "assistant", content: "hi" }));
    await p.send({ messages: [{ role: "user", content: "hi" }], callbacks: { onStatus: vi.fn(), onToken: vi.fn(), onToolCall: vi.fn(), onActivity: (e) => events.push(e) } });
    expect(events.some((e) => e.kind === "usage")).toBe(false);
  });
});

type Ev = { seq: number; event: Record<string, unknown> };
const PENDING: ShellPendingView = {
  id: "sh-1",
  callId: "c7",
  tool: "shell_run",
  hic: "required",
  argv: ["forge", "test"],
  resolvedProgram: "/opt/homebrew/bin/forge",
  cwd: "/Users/m/proj",
  timeoutSecs: 120,
  expiresInSecs: 300,
  sandbox: { backend: "seatbelt", enforced: true, network: "denied", writable: ["/Users/m/proj"], readable_extra: [], summary: "macOS Seatbelt" },
};

function sidecarApi(pages: Ev[][]): SidecarSessionApi & { decided: number } {
  let page = 0;
  const a: SidecarSessionApi & { decided: number } = {
    decided: 0,
    open: vi.fn(async () => "s1"),
    send: vi.fn(async () => {}),
    events: vi.fn(async (_id: string, after: number) => {
      if (page > 0 && a.decided === 0 && pages[page - 1].some((e) => e.event.type === "tool_call")) return { events: [], lastSeq: after, busy: true };
      const p = pages[page];
      if (!p) return { events: [], lastSeq: after, busy: false };
      const fresh = p.filter((e) => e.seq > after);
      if (fresh.length === 0) return { events: [], lastSeq: after, busy: true };
      page += 1;
      return { events: p, lastSeq: p[p.length - 1].seq, busy: true };
    }),
    toolResult: vi.fn(async () => {}),
    stop: vi.fn(async () => {}),
    shellPending: vi.fn(async () => [PENDING]),
    shellDecide: vi.fn(async () => {
      a.decided += 1;
    }),
  };
  return a;
}

describe("the sidecar's usage, plan and approvals reach the monitor", () => {
  it("forwards usage and a held command's approval, pending then decided", async () => {
    const a = sidecarApi([
      [
        { seq: 1, event: { type: "step_start", step: 1 } },
        { seq: 2, event: { type: "usage", step: 1, prompt_tokens: 900, completion_tokens: 20, generation_ms: 500 } },
        { seq: 3, event: { type: "tool_call", step: 1, host: "sidecar", call: { id: "c7", name: "shell_run", arguments: "{}" } } },
      ],
      [
        { seq: 4, event: { type: "tool_result", step: 1, call_id: "c7", status: "denied", content: "declined" } },
        { seq: 5, event: { type: "final", content: "ok" } },
        { seq: 6, event: { type: "done", outcome: "answered" } },
      ],
    ]);
    const events: TurnActivityEvent[] = [];
    const p = createSidecarProvider(a, () => "p", () => [], () => null, { shellPollMs: 1, shellWaitMs: 200 });
    await p.send({
      messages: [{ role: "user", content: "test it" }],
      callbacks: { onStatus: vi.fn(), onToken: vi.fn(), onToolCall: vi.fn(async () => "ok"), onActivity: (e) => events.push(e), onCommandApproval: async () => false },
    });
    expect(events.find((e) => e.kind === "usage")).toEqual({ kind: "usage", promptTokens: 900, completionTokens: 20, generationMs: 500 });
    expect(events.filter((e) => e.kind === "approval")).toEqual([
      { kind: "approval", callId: "c7", tool: "shell_run", state: "pending" },
      { kind: "approval", callId: "c7", tool: "shell_run", state: "declined" },
    ]);
  });

  it("forwards a workflow's plan", async () => {
    const a = sidecarApi([
      [
        { seq: 1, event: { type: "plan", steps: ["write-test", "implement", 7] } },
        { seq: 2, event: { type: "final", content: "ok" } },
        { seq: 3, event: { type: "done", outcome: "answered" } },
      ],
    ]);
    const events: TurnActivityEvent[] = [];
    const p = createSidecarProvider(a, () => "p", () => []);
    await p.send({ messages: [{ role: "user", content: "x" }], callbacks: { onStatus: vi.fn(), onToken: vi.fn(), onToolCall: vi.fn(), onActivity: (e) => events.push(e) } });
    expect(events.find((e) => e.kind === "plan")).toEqual({ kind: "plan", steps: ["write-test", "implement"] });
  });
});

describe("the turn activity slice", () => {
  it("records usage, the plan, approvals and verdicts only while a turn runs", () => {
    usageReported({ kind: "usage", promptTokens: 1, completionTokens: 1, generationMs: null });
    expect(turnActivity.get().usage).toBeNull();
    beginTurn("sidecar", "Hermes", 1000);
    usageReported({ kind: "usage", promptTokens: 800, completionTokens: 40, generationMs: 2000 });
    usageReported({ kind: "usage", promptTokens: 900, completionTokens: 60, generationMs: 2000 });
    planReported(["a", "b", "c"]);
    approvalNoted("c1", "gsheets_append", "pending", 1100);
    approvalNoted("c1", "gsheets_append", "approved", 1200);
    approvalNoted("c2", "shell_run", "pending", 1300);
    verifierReported({ kind: "verifier", step: "a", name: "forge", passed: false, detail: "1 failing" }, 1400);
    verifierReported({ kind: "verifier", step: "a", name: "forge", passed: true, detail: "" }, 1500);
    verifierReported({ kind: "verifier", step: "b", name: "slither", passed: false, detail: "high finding" }, 1600);
    const s = turnActivity.get();
    expect(s.usage).toEqual({ promptTokens: 900, completionTokens: 60, generationMs: 2000, calls: 2 });
    expect(s.approvals).toEqual([
      { callId: "c1", tool: "gsheets_append", state: "approved", at: 1200 },
      { callId: "c2", tool: "shell_run", state: "pending", at: 1300 },
    ]);
    expect(planStepStates(s.plan ?? [], s.verifiers ?? [])).toEqual([
      { step: "a", state: "passed" },
      { step: "b", state: "failed" },
      { step: "c", state: "not checked yet" },
    ]);
    endTurn("answered", 2000);
    expect(turnActivity.get().usage?.calls).toBe(2);
    beginTurn("local", "local", 3000);
    expect(turnActivity.get().usage).toBeNull();
    expect(turnActivity.get().approvals).toEqual([]);
  });

  it("tokens per second needs the server's generation time", () => {
    expect(tokensPerSecond({ promptTokens: 1, completionTokens: 60, generationMs: 2000, calls: 1 })).toBe(30);
    expect(tokensPerSecond({ promptTokens: 1, completionTokens: 60, generationMs: null, calls: 1 })).toBeNull();
    expect(tokensPerSecond({ promptTokens: 1, completionTokens: 60, generationMs: 0, calls: 1 })).toBeNull();
    expect(tokensPerSecond(null)).toBeNull();
  });
});

describe("the snapshot and the pop-out", () => {
  let root: Root | null = null;
  let host: HTMLDivElement | null = null;
  afterEach(() => {
    act(() => root?.unmount());
    host?.remove();
    root = null;
    host = null;
  });

  function render(snapshot = buildMonitorSnapshot(inputs())) {
    host = document.createElement("div");
    document.body.appendChild(host);
    root = createRoot(host);
    act(() => root!.render(<ActivityMonitor snapshot={snapshot} now={12_000} onStop={() => {}} />));
    return host;
  }
  const text = (h: HTMLElement, id: string) => h.querySelector(`[data-testid="${id}"]`)?.textContent ?? "";

  it("context used and tokens/s come from the server's report", () => {
    beginTurn("local", "local model", 1000);
    usageReported({ kind: "usage", promptTokens: 3000, completionTokens: 96, generationMs: 3200 });
    const s = buildMonitorSnapshot(inputs());
    expect(s.context.usedTokens).toBe(3096);
    expect(s.context.windowTokens).toBe(8192);
    expect(s.context.usedNote).toMatch(/^measured/);
    expect(s.speed).toEqual({ tokensPerSecond: 30, note: "measured: written tokens over the model server's generation time" });
    expect(isMonitorSnapshot(s)).toBe(true);
    const h = render(s);
    expect(text(h, "mon-ctx")).toMatch(/^3,096 used of 8,192 tokens/);
    expect(text(h, "mon-speed")).toMatch(/^30 tokens\/s/);
  });

  it("without a report both stay unknown and say why", () => {
    expect(usedFrom(null)).toEqual({ usedTokens: null, usedNote: "the model server has not reported token usage for this turn" });
    expect(speedFor({ promptTokens: 1, completionTokens: 1, generationMs: null, calls: 1 }).tokensPerSecond).toBeNull();
    const h = render();
    expect(text(h, "mon-speed")).toMatch(/^unknown/);
    expect(text(h, "mon-ctx")).toMatch(/^used: unknown/);
  });

  it("shows the plan with step states, the approvals and the checks; Stop stays first", () => {
    beginTurn("sidecar", "Hermes · workflow", 1000);
    planReported(["write-test", "implement"]);
    verifierReported({ kind: "verifier", step: "write-test", name: "forge_tests_pass", passed: true, detail: "" });
    verifierReported({ kind: "verifier", step: "implement", name: "slither", passed: false, detail: "1 high finding" });
    approvalNoted("c1", "gsheets_append", "pending");
    approvalNoted("c2", "schedule_add", "pending");
    approvalNoted("c2", "schedule_add", "declined");
    const h = render(buildMonitorSnapshot(inputs({ providerKind: "sidecar" })));
    const plan = Array.from(h.querySelectorAll('[data-testid="mon-plan-row"]')).map((r) => r.textContent);
    expect(plan).toEqual(["write-test passed", "implement failed"]);
    const appr = Array.from(h.querySelectorAll('[data-testid="mon-approval-row"]')).map((r) => r.textContent);
    expect(appr).toEqual(["gsheets_appendwaiting for you in the main window", "schedule_adddeclined by you"]);
    const checks = Array.from(h.querySelectorAll('[data-testid="mon-verifier-row"]')).map((r) => r.textContent);
    expect(checks[1]).toContain("1 high finding");
    const focusable = h.querySelectorAll("button");
    expect(focusable[0].getAttribute("data-testid")).toBe("mon-stop");
  });

  it("a chat turn with no tool step yet says it has no plan yet", () => {
    beginTurn("local", "local", 1000);
    const h = render(buildMonitorSnapshot(inputs()));
    expect(text(h, "mon-plan")).toMatch(/No plan yet: a plan shows when the model asks for tools/);
  });

  it("a snapshot from an older sender (no speed, plan or approvals) still validates and renders", () => {
    const s = buildMonitorSnapshot(inputs());
    const old = JSON.parse(JSON.stringify(s));
    delete old.speed;
    delete old.turn.plan;
    delete old.turn.approvals;
    delete old.turn.verifiers;
    expect(isMonitorSnapshot(old)).toBe(true);
    const h = render(old);
    expect(text(h, "mon-speed")).toMatch(/^unknown/);
    expect(text(h, "mon-approvals")).toMatch(/None asked/);
  });

  it("a malformed approval or speed is refused", () => {
    const s = JSON.parse(JSON.stringify(buildMonitorSnapshot(inputs())));
    expect(isMonitorSnapshot({ ...s, speed: { tokensPerSecond: "fast", note: "x" } })).toBe(false);
    expect(isMonitorSnapshot({ ...s, turn: { ...s.turn, approvals: [{ callId: "c", tool: "t", state: "maybe", at: 1 }] } })).toBe(false);
    expect(isMonitorSnapshot({ ...s, turn: { ...s.turn, plan: [{ step: "a", state: "finished" }] } })).toBe(false);
  });

  it("a daemon whose last run was measured says so; otherwise its tokens read as estimated", () => {
    const d = (id: string, lastTokenSource: string | null) => ({
      id,
      name: "Daemon " + id,
      prompt: "x",
      schedule: "0 9 * * *",
      budget: { maxRunsPerDay: 4, maxTokensPerDay: 20_000, maxTokensPerRun: 6_000, maxSpendSalt: "0" },
      paused: false,
      status: "scheduled" as const,
      running: false,
      runsToday: 1,
      tokensToday: 720,
      skippedToday: 0,
      spendTodaySalt: "0",
      nextRunMs: null,
      lastRunMs: null,
      lastOutcome: "answered",
      lastNote: null,
      lastTokenSource,
    });
    const daemons = daemonsSection({ allPaused: false, daemons: [d("a", "measured"), d("b", "estimated"), d("c", null)] }, { blocked: null, error: null });
    const snap = buildMonitorSnapshot(inputs({ daemons }));
    expect(isMonitorSnapshot(snap)).toBe(true);
    const h = render(snap);
    const rows = Array.from(h.querySelectorAll('[data-testid="mon-daemon-row"]')).map((r) => r.textContent ?? "");
    expect(rows[0]).toContain("720 of 20,000 tokens (last run measured)");
    expect(rows[1]).toContain("720 of 20,000 tokens (estimated)");
    expect(rows[2]).toContain("(estimated)");
  });
});

// HUP-S7.5 (D-27, US-7.3 AC1): the first-token time comes from the model server's own timing for
// the turn's first call; without it the row says unknown and why.
describe("first token (D-27)", () => {
  it("reads llama-server's prompt time from both loops' usage", () => {
    expect(usageEventOf({ prompt_tokens: 12, completion_tokens: 4, generation_ms: 140, prompt_ms: 96 })).toEqual({ kind: "usage", promptTokens: 12, completionTokens: 4, generationMs: 140, promptMs: 96 });
    expect(usageEventOf({ prompt_tokens: 12, completion_tokens: 4, prompt_ms: "soon" })).toEqual({ kind: "usage", promptTokens: 12, completionTokens: 4, generationMs: null });
  });

  it("the turn's first call decides it; a later call's time is not substituted", () => {
    beginTurn("local", "local model", 1000);
    usageReported({ kind: "usage", promptTokens: 300, completionTokens: 12, generationMs: 400, promptMs: 180 });
    usageReported({ kind: "usage", promptTokens: 340, completionTokens: 20, generationMs: 600, promptMs: 35 });
    expect(turnActivity.get().usage?.firstTokenMs).toBe(180);
    beginTurn("local", "local model", 2000);
    usageReported({ kind: "usage", promptTokens: 300, completionTokens: 12, generationMs: 400 });
    usageReported({ kind: "usage", promptTokens: 340, completionTokens: 20, generationMs: 600, promptMs: 35 });
    expect(turnActivity.get().usage?.firstTokenMs).toBeUndefined();
  });

  it("the snapshot and the pop-out show the measured value, or unknown with the reason", () => {
    beginTurn("local", "local model", 1000);
    usageReported({ kind: "usage", promptTokens: 3000, completionTokens: 96, generationMs: 3200, promptMs: 1240 });
    const s = buildMonitorSnapshot(inputs());
    expect(s.firstToken).toEqual({ ms: 1240, note: "measured: the model server's own time reading the prompt before its first token" });
    expect(isMonitorSnapshot(s)).toBe(true);
    expect(isMonitorSnapshot({ ...s, firstToken: { ms: "fast", note: "x" } })).toBe(false);
    const host = document.createElement("div");
    document.body.appendChild(host);
    const root = createRoot(host);
    act(() => root.render(<ActivityMonitor snapshot={s} now={12_000} onStop={() => {}} />));
    expect(host.querySelector('[data-testid="mon-ttft"]')?.textContent).toMatch(/^1,240 ms/);
    const unknown = buildMonitorSnapshot(inputs({ activity: IDLE_ACTIVITY }));
    expect(unknown.firstToken?.ms).toBeNull();
    act(() => root.render(<ActivityMonitor snapshot={unknown} now={12_000} onStop={() => {}} />));
    expect(host.querySelector('[data-testid="mon-ttft"]')?.textContent).toMatch(/^unknown/);
    act(() => root.unmount());
    host.remove();
  });
});
