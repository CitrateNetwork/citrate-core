// HUP-S1.9 — loop parity suite (TS side).
//
// Runs the CURRENT TypeScript agent loop (harness.ts createAgentProvider) against every scenario
// in parity-v1.json with a scripted model (inferTools) and a scripted tool host (onToolCall). The
// SAME bytes (pinned by sha256 below) live in citrate-agent-runtime at
// agent-loop/tests/fixtures/parity-v1.json and drive the Rust loop (agent-loop run_turn) and the
// sidecar (parse_turn + run_turn). A scenario marked known_divergence carries a per-implementation
// override; this file applies only the `ts` override. harness.ts is NOT retired by this suite —
// retirement is an owner call after a live run.
import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { createHash } from "node:crypto";
import { resolve } from "node:path";
import { createAgentProvider, AGENT_MAX_TURNS, AGENT_TOOLS, READ_ONLY_AGENT_TOOLS, type AgentContext, type ChatStatus, type ToolCall } from "../harness";

/** sha256 of parity-v1.json. Changing the fixture means bumping this here AND in the runtime. */
export const PARITY_V1_SHA256 = "d034be21e517e1f52c446394ca6bc439280c50aee0bf85448f19a710c8600392";

// vitest runs from the package root (jsdom gives import.meta.url a non-file scheme).
const FIXTURE_PATH = resolve(process.cwd(), "src/agent/parity/parity-v1.json");
const RAW = readFileSync(FIXTURE_PATH);

type ToolResult = { ok?: string; denied?: string; error?: string };
type ModelEntry = { message?: Record<string, unknown>; raw?: string; error?: string };
type ToolMsgExpect = { tool_call_id?: string; content?: string; content_contains?: string; id_synthesized?: boolean };
type Expect = {
  outcome?: "answered" | "stopped" | "step_limit" | "failed";
  final?: string | null;
  terminal?: "done" | "error";
  model_calls?: number;
  host_calls?: string[];
  host_arguments?: unknown[];
  tool_events?: number;
  last_request_roles?: string[];
  last_request_assistant_content?: string;
  tool_messages?: ToolMsgExpect[];
  first_request_messages?: { role: string; content: string }[];
  error_contains?: Record<string, string> | string;
};
type Scenario = {
  id: string;
  layer: "loop" | "wire";
  behavior: string;
  history?: { role: string; content: string }[];
  user: string;
  model: ModelEntry[];
  model_repeat_last?: boolean;
  tool_results: ToolResult[];
  tool_result_default?: ToolResult;
  stop_after_host_calls?: number;
  expect: Expect;
  known_divergence?: { reason: string; verdict: string; ts?: Expect; loop?: Expect; sidecar?: Expect };
};
type Fixture = {
  version: number;
  tools: { name: string; host: string; read_only: boolean }[];
  limits: { ts_max_turns: number; rust_max_steps: number; rust_max_tool_calls_per_step: number };
  scenarios: Scenario[];
};

const FIXTURE = JSON.parse(RAW.toString("utf8")) as Fixture;

const ctx: AgentContext = {
  height: 100,
  peers: 3,
  finalityAge: 4,
  nodeState: "synced",
  staked: 32000,
  liquid: 1,
  claimable: 0,
  earningsToday: 0,
  walletAddr: "0x0000000000000000000000000000000000000000",
  tier: "member",
};

type ConvoMsg = { role: string; content?: string | null; tool_call_id?: string; tool_calls?: unknown[] };

interface Run {
  outcome: "answered" | "step_limit" | "failed";
  final: string | null;
  streamed: string;
  error: string;
  statuses: ChatStatus[];
  requests: ConvoMsg[][];
  hostCalls: ToolCall[];
}

async function runTs(s: Scenario): Promise<Run> {
  const requests: ConvoMsg[][] = [];
  const hostCalls: ToolCall[] = [];
  const statuses: ChatStatus[] = [];
  let streamed = "";
  let modelIdx = 0;
  const inferTools = async (_pid: string, messagesJson: string): Promise<string> => {
    requests.push(JSON.parse(messagesJson) as ConvoMsg[]);
    const entry = s.model[Math.min(modelIdx, s.model.length - 1)];
    if (modelIdx >= s.model.length && !s.model_repeat_last) throw new Error("parity script exhausted");
    modelIdx++;
    if (entry.error !== undefined) throw new Error(entry.error);
    if (entry.raw !== undefined) return entry.raw;
    return JSON.stringify(entry.message);
  };
  const onToolCall = async (call: ToolCall): Promise<string> => {
    const r = s.tool_results[hostCalls.length] ?? s.tool_result_default;
    hostCalls.push({ ...call });
    if (!r) throw new Error("parity script: no tool result scripted");
    if (r.error !== undefined) throw new Error(r.error);
    // A refusal: core's gated handlers resolve with a sentence saying the member declined.
    if (r.denied !== undefined) return r.denied;
    return r.ok ?? "";
  };
  const provider = createAgentProvider("parity", () => ctx, inferTools);
  const messages = [...(s.history ?? []), { role: "user", content: s.user }];
  try {
    const res = await provider.send({
      messages,
      callbacks: {
        onStatus: (st) => statuses.push(st),
        onToken: (t) => {
          streamed += t;
        },
        onToolCall,
      },
    });
    return { outcome: "answered", final: res.content, streamed, error: "", statuses, requests, hostCalls };
  } catch (e) {
    const error = e instanceof Error ? e.message : String(e);
    // The TS loop reports the turn cap only through its error text.
    const outcome = /tool-turn limit/.test(error) ? "step_limit" : "failed";
    return { outcome, final: null, streamed, error, statuses, requests, hostCalls };
  }
}

function effective(s: Scenario): Expect {
  return { ...s.expect, ...(s.known_divergence?.ts ?? {}) };
}

describe("parity-v1 fixture integrity (HUP-S1.9)", () => {
  it("is pinned by sha256 (identical bytes in citrate-agent-runtime)", () => {
    expect(createHash("sha256").update(RAW).digest("hex")).toBe(PARITY_V1_SHA256);
  });

  it("matches the TS loop's limits and is well-formed", () => {
    expect(FIXTURE.version).toBe(1);
    expect(FIXTURE.limits.ts_max_turns).toBe(AGENT_MAX_TURNS);
    const ids = FIXTURE.scenarios.map((s) => s.id);
    expect(new Set(ids).size).toBe(ids.length);
    for (const s of FIXTURE.scenarios) {
      expect(["loop", "wire"]).toContain(s.layer);
      expect(s.behavior.length).toBeGreaterThan(0);
      if (s.known_divergence) {
        expect(s.known_divergence.reason.length).toBeGreaterThan(0);
        expect(["rust_correct", "owner_decision"]).toContain(s.known_divergence.verdict);
      }
    }
  });

  it("offers exactly harness.ts's tools, with its reviewed read-only set", () => {
    expect(FIXTURE.tools.map((t) => t.name)).toEqual(AGENT_TOOLS.map((t) => t.function.name));
    for (const t of FIXTURE.tools) {
      expect(t.host).toBe("core");
      expect(t.read_only, t.name).toBe(READ_ONLY_AGENT_TOOLS.has(t.name));
    }
  });

  it("covers every observable behavior of harness.ts createAgentProvider", () => {
    const ids = new Set(FIXTURE.scenarios.map((s) => s.id));
    for (const required of [
      "final_answer_no_tools",
      "single_tool_then_answer",
      "multiple_tools_one_step",
      "text_alongside_tool_calls_is_not_final",
      "tool_error_fed_back",
      "declined_approval_fed_back",
      "turn_cap_exhausted",
      "answer_on_last_allowed_turn",
      "provider_error",
      "provider_error_after_tool",
      "history_carried_into_request",
      "empty_reply",
      "malformed_tool_arguments",
      "unknown_tool",
      "per_step_tool_cap",
      "stop_requested_during_tool",
      "unparseable_model_message",
      "missing_tool_call_id",
      "missing_tool_arguments",
      "object_tool_arguments",
      "nameless_tool_call",
    ]) {
      expect(ids.has(required), required).toBe(true);
    }
  });
});

describe("parity-v1 — harness.ts against every scenario (HUP-S1.9)", () => {
  for (const s of FIXTURE.scenarios) {
    it(`${s.id}${s.known_divergence?.ts ? " (known divergence: ts override)" : ""}`, async () => {
      const ex = effective(s);
      const r = await runTs(s);

      if (ex.outcome) expect(r.outcome, "outcome").toBe(ex.outcome);
      if (ex.final !== undefined && ex.final !== null) {
        expect(r.final, "final").toBe(ex.final);
        expect(r.streamed, "streamed final").toBe(ex.final);
      }
      if (ex.terminal) expect(r.statuses[r.statuses.length - 1], "terminal status").toBe(ex.terminal);
      expect(r.statuses[0]).toBe("thinking");
      if (ex.model_calls !== undefined) expect(r.requests.length, "model_calls").toBe(ex.model_calls);
      if (ex.host_calls) expect(r.hostCalls.map((c) => c.name), "host_calls").toEqual(ex.host_calls);
      if (ex.host_arguments) expect(r.hostCalls.map((c) => c.arguments), "host_arguments").toEqual(ex.host_arguments);
      if (ex.tool_events !== undefined) expect(r.statuses.filter((x) => x === "tool").length, "tool_events").toBe(ex.tool_events);

      const last = r.requests[r.requests.length - 1] ?? [];
      const lastNonSystem = last.filter((m) => m.role !== "system");
      if (ex.last_request_roles) expect(lastNonSystem.map((m) => m.role), "last_request_roles").toEqual(ex.last_request_roles);
      if (ex.last_request_assistant_content !== undefined) {
        const asst = lastNonSystem.filter((m) => m.role === "assistant");
        expect(asst[asst.length - 1]?.content).toBe(ex.last_request_assistant_content);
      }
      if (ex.tool_messages) {
        const tools = lastNonSystem.filter((m) => m.role === "tool");
        expect(tools.length, "tool_messages count").toBe(ex.tool_messages.length);
        ex.tool_messages.forEach((tm, i) => {
          const got = tools[i];
          if (tm.tool_call_id !== undefined) expect(got.tool_call_id).toBe(tm.tool_call_id);
          if (tm.content !== undefined) expect(got.content).toBe(tm.content);
          if (tm.content_contains !== undefined) expect(String(got.content)).toContain(tm.content_contains);
          if (tm.id_synthesized) {
            expect(typeof got.tool_call_id === "string" && got.tool_call_id.length > 0).toBe(true);
            expect(got.tool_call_id).toBe(r.hostCalls[i]?.id);
          }
        });
      }
      if (ex.first_request_messages) {
        const first = (r.requests[0] ?? []).filter((m) => m.role !== "system").map((m) => ({ role: m.role, content: m.content }));
        expect(first).toEqual(ex.first_request_messages);
      }
      if (r.outcome !== "answered" && ex.error_contains !== undefined) {
        const needle = typeof ex.error_contains === "string" ? ex.error_contains : ex.error_contains.ts;
        if (needle !== undefined) expect(r.error).toContain(needle);
      }
    });
  }
});
