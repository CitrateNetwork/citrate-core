// HUP-S1.9 (live parity): the live runner's own logic, without a sidecar. The session body must be
// the one core's build_session_body sends; the live expectation adaptations must be exactly the two
// the report states; the comparator must catch every class of difference; and the scenario driver
// must run the provider against a recording session API the way the live run does.
import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import {
  buildSessionBody,
  compareLive,
  liveExpectation,
  livePerStepCap,
  liveStepCap,
  runLiveScenario,
  SCRIPTED_ERROR_STATUS,
  SESSION_CONFIG,
  type EventEnvelope,
  type Fixture,
  type LiveResult,
  type ModelScript,
  type RecordedRequest,
  type RecordingSessionApi,
  type ScriptEntry,
} from "./liveRunner";
import { annotatedAgentTools } from "../../toolAnnotations";

const FIXTURE = JSON.parse(readFileSync(resolve(process.cwd(), "src/agent/parity/parity-v1.json"), "utf8")) as Fixture;
const scn = (id: string) => {
  const s = FIXTURE.scenarios.find((x) => x.id === id);
  if (!s) throw new Error(`no scenario ${id}`);
  return s;
};

describe("live session body (mirrors build_session_body)", () => {
  const body = JSON.parse(
    buildSessionBody(SESSION_CONFIG, "You are Hermes.", JSON.stringify(annotatedAgentTools()), { baseUrl: "http://127.0.0.1:9/v1", bearer: "b" }, "m.gguf", 16_384),
  ) as Record<string, unknown> & { tools: Record<string, unknown>[] };

  it("has exactly the pinned top-level keys and constants", () => {
    expect(Object.keys(body).sort()).toEqual([...SESSION_CONFIG.bodyKeys].sort());
    expect(body.maxToolsPerRequest).toBe(SESSION_CONFIG.maxToolsPerRequest);
    expect(body.hicAware).toBe(true);
    expect(body.maxTokens).toBe(Math.min(SESSION_CONFIG.aiMaxTokens, 16_384 / 4));
    expect(body.llm).toEqual({ baseUrl: "http://127.0.0.1:9/v1", bearer: "b" });
  });

  it("stamps every app tool host core with effect, trust and read_only", () => {
    expect(body.tools.length).toBe(annotatedAgentTools().length);
    for (const t of body.tools) {
      expect(Object.keys(t).sort()).toEqual([...SESSION_CONFIG.toolKeys].sort());
      expect(t.host).toBe("core");
      const a = t.annotations as { effect: string; trust: string; read_only: boolean };
      expect(a.read_only).toBe(a.effect === "none");
    }
  });

  it("refuses a tool without annotations, as core does", () => {
    expect(() => buildSessionBody(SESSION_CONFIG, "p", JSON.stringify([{ name: "x", parameters: {} }]), { baseUrl: "u", bearer: "" }, "m", 4096)).toThrow(/annotations/);
  });

  it("small windows cap the reply at a quarter of the context", () => {
    const small = JSON.parse(buildSessionBody(SESSION_CONFIG, "p", "[]", { baseUrl: "u", bearer: "" }, "m", 4096)) as { maxTokens: number };
    expect(small.maxTokens).toBe(1024);
  });

  it("sends an explicit step cap only when the pinned config names one", () => {
    expect(body).not.toHaveProperty("maxSteps");
    const six = JSON.parse(buildSessionBody({ ...SESSION_CONFIG, maxSteps: 6 } as typeof SESSION_CONFIG, "p", "[]", { baseUrl: "u", bearer: "" }, "m", 4096)) as { maxSteps?: number };
    expect(six.maxSteps).toBe(6);
  });
});

describe("live caps", () => {
  it("core sends no step cap today, so the sidecar default applies (default_turn_cap owner decision)", () => {
    expect(liveStepCap()).toBe(SESSION_CONFIG.sidecarDefaults.maxSteps);
    expect(liveStepCap({ ...SESSION_CONFIG, maxSteps: 6 } as typeof SESSION_CONFIG)).toBe(6);
    expect(livePerStepCap()).toBe(SESSION_CONFIG.sidecarDefaults.maxToolCallsPerStep);
  });
});

describe("liveExpectation", () => {
  it("leaves scenarios that are not cap-bound or model-error-bound unchanged (sidecar override applied)", () => {
    const s = scn("unknown_tool");
    expect(liveExpectation(FIXTURE, s, 8)).toEqual({ ...s.expect, ...(s.known_divergence?.sidecar ?? {}) });
    const e = scn("empty_reply");
    expect(liveExpectation(FIXTURE, e, 8).outcome).toBe("failed");
    expect(liveExpectation(FIXTURE, e, 8).error_contains).toBe("empty assistant message");
  });

  it("rescales the step-cap scenario to the live cap", () => {
    const ex = liveExpectation(FIXTURE, scn("turn_cap_exhausted"), 8);
    expect(ex.model_calls).toBe(8);
    expect(ex.tool_events).toBe(8);
    expect(ex.host_calls).toEqual(Array(8).fill("memory_search"));
    expect(ex.last_request_roles?.length).toBe(15);
    expect(ex.error_contains).toEqual({ sidecar: "step budget of 8 exhausted" });
  });

  it("does not rescale when the live cap equals the fixture's", () => {
    const s = scn("turn_cap_exhausted");
    expect(liveExpectation(FIXTURE, s, FIXTURE.limits.rust_max_steps).model_calls).toBe(s.expect.model_calls);
  });

  it("reads a scripted model error as its HTTP status", () => {
    expect(liveExpectation(FIXTURE, scn("provider_error"), 8).error_contains).toEqual({ sidecar: `HTTP ${SCRIPTED_ERROR_STATUS}` });
    expect(liveExpectation(FIXTURE, scn("provider_error_after_tool"), 8).error_contains).toEqual({ sidecar: `HTTP ${SCRIPTED_ERROR_STATUS}` });
  });
});

function goodResult(): LiveResult {
  const ev = (seq: number, event: Record<string, unknown>): EventEnvelope => ({ seq, event });
  return {
    outcome: "answered",
    finalEvent: "Staking locks 32,000 SALT.",
    provider: { resolved: "Staking locks 32,000 SALT.", rejected: null },
    statuses: ["thinking", "thinking", "tool", "thinking", "streaming", "done"],
    events: [
      ev(1, { type: "step_start", step: 1 }),
      ev(2, { type: "tool_call", host: "core", call: { id: "c1", name: "memory_search", arguments: '{"query":"staking"}' } }),
      ev(3, { type: "tool_result", call_id: "c1", status: "ok" }),
      ev(4, { type: "step_start", step: 2 }),
      ev(5, { type: "final", content: "Staking locks 32,000 SALT." }),
      ev(6, { type: "done", outcome: "answered" }),
    ],
    requests: [
      { path: "/v1/chat/completions", authorization: "Bearer k", body: { messages: [{ role: "system", content: "s" }, { role: "user", content: "how does staking work?" }], tools: [] } },
      {
        path: "/v1/chat/completions",
        authorization: "Bearer k",
        body: {
          messages: [
            { role: "system", content: "s" },
            { role: "user", content: "how does staking work?" },
            { role: "assistant", content: null, tool_calls: [] },
            { role: "tool", tool_call_id: "c1", content: "[1] Staking: lock 32000 SALT" },
          ],
          tools: [],
        },
      },
    ],
    hostCalls: [{ call: { id: "c1", name: "memory_search", arguments: '{"query":"staking"}' } }],
  };
}

describe("compareLive", () => {
  const opts = { maxToolsPerRequest: 8, llmBearer: "k" };
  const ex = liveExpectation(FIXTURE, scn("single_tool_then_answer"), 8);

  it("a faithful run has no mismatches", () => {
    expect(compareLive(ex, goodResult(), opts)).toEqual([]);
  });

  it("catches a wrong outcome, final, host call, model call count and tool message", () => {
    const r = goodResult();
    r.outcome = "failed";
    r.finalEvent = "other";
    r.hostCalls = [];
    r.requests = r.requests.slice(0, 1);
    const bad = compareLive(ex, r, opts).join("\n");
    for (const k of ["outcome", "final event", "host_calls", "model_calls", "tool_messages count"]) expect(bad).toContain(k);
  });

  it("catches a provider that disagrees with the event log", () => {
    const r = goodResult();
    r.provider = { resolved: null, rejected: "boom" };
    expect(compareLive(ex, r, opts).join("\n")).toContain("provider answer");
  });

  it("catches more tools offered than the session config allows, and a missing model bearer", () => {
    const r = goodResult();
    r.requests[0].body.tools = Array(9).fill({});
    r.requests[1].authorization = null;
    const bad = compareLive(ex, r, opts).join("\n");
    expect(bad).toContain("tools offered");
    expect(bad).toContain("llm bearer");
  });

  it("requires the log to end in done and start with step_start", () => {
    const r = goodResult();
    r.events = r.events.slice(1, -1);
    const bad = compareLive(ex, r, opts).join("\n");
    expect(bad).toContain("done event");
    expect(bad).toContain("first event");
  });

  it("checks the error needle on failed turns", () => {
    const pe = liveExpectation(FIXTURE, scn("provider_error"), 8);
    const r: LiveResult = {
      outcome: "failed",
      finalEvent: null,
      provider: { resolved: null, rejected: "model error: HTTP 500" },
      statuses: ["thinking", "error"],
      events: [
        { seq: 1, event: { type: "step_start", step: 1 } },
        { seq: 2, event: { type: "error", message: "model error: HTTP 500" } },
        { seq: 3, event: { type: "done", outcome: "failed" } },
      ],
      requests: [{ path: "/v1/chat/completions", authorization: "Bearer k", body: { messages: [{ role: "user", content: "hi" }] } }],
      hostCalls: [],
    };
    expect(compareLive(pe, r, opts).join("\n")).toContain("error_contains");
  });
});

/** An in-memory sidecar stand-in: replays a fixed event script per turn, records tool results. */
class FakeSession implements RecordingSessionApi {
  events_seen: EventEnvelope[] = [];
  opened: string[] = [];
  closed: string[] = [];
  bodies: string[] = [];
  results: { callId: string; status: string; content: string }[] = [];
  private log: EventEnvelope[] = [];
  constructor(
    private readonly build: (p: string, t: string) => string,
    private readonly turns: Record<string, unknown>[][],
  ) {}
  async open(p: string, t: string) {
    this.bodies.push(this.build(p, t));
    this.opened.push("s1-a");
    return "s1-a";
  }
  async send() {
    const next = this.turns.shift() ?? [];
    for (const event of next) this.log.push({ seq: this.log.length + 1, event });
  }
  async events(_id: string, after: number) {
    const events = this.log.filter((e) => e.seq > after);
    for (const e of events) if (!this.events_seen.some((s) => s.seq === e.seq)) this.events_seen.push(e);
    return { events, lastSeq: this.log.length, busy: false };
  }
  async toolResult(_id: string, callId: string, status: "ok" | "denied" | "error", content: string) {
    this.results.push({ callId, status, content });
  }
  async stop() {}
  async close(id: string) {
    this.closed.push(id);
  }
}

class FakeModel implements ModelScript {
  requests: RecordedRequest[] = [];
  scripts: ScriptEntry[][] = [];
  script(entries: ScriptEntry[]) {
    this.scripts.push(entries);
    this.requests = [];
  }
}

describe("runLiveScenario (driver)", () => {
  it("replays history as earlier turns of the same session, runs core calls through the host script and closes the session", async () => {
    const s = { ...scn("single_tool_then_answer"), history: [{ role: "user", content: "hello" }, { role: "assistant", content: "Hi." }] };
    let fake: FakeSession | null = null;
    const model = new FakeModel();
    const call = (s.model[0].message as { tool_calls: { id: string; function: { name: string; arguments: string } }[] }).tool_calls[0];
    const r = await runLiveScenario(s, {
      model,
      makeApi: (build) => {
        fake = new FakeSession(build, [
          [{ type: "step_start", step: 1 }, { type: "final", content: "Hi." }, { type: "done", outcome: "answered" }],
          [
            { type: "step_start", step: 1 },
            { type: "tool_call", host: "core", call: { id: call.id, name: call.function.name, arguments: call.function.arguments } },
            { type: "tool_result", call_id: call.id, status: "ok" },
            { type: "final", content: "done" },
            { type: "done", outcome: "answered" },
          ],
        ]);
        return fake;
      },
      modelBaseUrl: "http://127.0.0.1:9/v1",
      llmBearer: "k",
      systemPrompt: "p",
      tools: annotatedAgentTools(),
    });
    expect(model.scripts.length).toBe(2);
    expect(model.scripts[0]).toEqual([{ message: { role: "assistant", content: "Hi." } }]);
    expect(r.outcome).toBe("answered");
    expect(r.provider.resolved).toBe("done");
    expect(r.hostCalls.map((c) => c.call.name)).toEqual([call.function.name]);
    expect(r.events[0].event.type).toBe("step_start");
    expect(r.events.length).toBe(5);
    const f = fake as unknown as FakeSession;
    expect(f.results).toEqual([{ callId: call.id, status: "ok", content: s.tool_results[0].ok }]);
    expect(f.closed).toEqual(["s1-a"]);
    expect(JSON.parse(f.bodies[0]).llm).toEqual({ baseUrl: "http://127.0.0.1:9/v1", bearer: "k" });
  });
});
