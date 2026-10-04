// HUP-S1.9 (live parity): run parity-v1.json end to end: core's sidecar provider
// (`../../sidecarProvider.ts`) → a real sidecar process (the binary shipped in the packaged app) →
// a scripted model over real HTTP, and compare against the fixture's `sidecar` expectations.
//
// TEST SUPPORT ONLY (imported by *.test.ts files, never by the app).
//
// What the loop-level suites (parity.test.ts here, parity_tests.rs + parity_wire_tests.rs in the
// runtime) cannot see and this run does: the session layer's own config (default step cap, per-step
// cap, tool selection), the `tool_results` round trip, the event log the provider long-polls, the
// provider's handling of `done`/`error`/`final`, and the model client's HTTP behaviour.
//
// Two adaptations, both stated in the report:
//   1. The step cap is the one the shipped session config yields (core sends no `maxSteps`, so the
//      sidecar default applies). Cap-bound expectations are rescaled from the fixture's
//      `rust_max_steps`; the difference from harness.ts (6) is the `default_turn_cap` owner call.
//   2. A scripted `{error}` model entry is served as HTTP 502. The sidecar's model client (like
//      core's own ai.rs client) reports only the status, never the provider's body, so the live
//      needle is "HTTP 502" rather than the scripted text.
import { createSidecarProvider, type SidecarSessionApi } from "../../sidecarProvider";
import type { ChatStatus, ToolCall, ToolCallMeta } from "../../harness";
import SESSION_CONFIG from "./session-config.json";

// The HTTP pieces (scripts/parity-live/*.mjs) need node APIs; this module stays node-free so it
// typechecks with the app and is unit-tested with in-memory fakes.

/** The status the scripted endpoint answers a scripted `{error}` entry with. */
export const SCRIPTED_ERROR_STATUS = 502;

export interface ScriptEntry {
  message?: Record<string, unknown>;
  raw?: string;
  error?: string;
}

export interface RecordedRequest {
  path: string;
  authorization: string | null;
  body: { messages?: { role: string; content?: string | null; tool_call_id?: string; tool_calls?: unknown[] }[]; tools?: unknown[]; [k: string]: unknown };
}

/** The scripted model endpoint (scripts/parity-live/scripted-model.mjs). */
export interface ModelScript {
  script(entries: ScriptEntry[], repeatLast?: boolean): void;
  requests: RecordedRequest[];
}

export interface EventEnvelope {
  seq: number;
  event: Record<string, unknown>;
}

/** core's session API plus what the run records (scripts/parity-live/sidecar-http.mjs). */
export interface RecordingSessionApi extends SidecarSessionApi {
  events_seen: EventEnvelope[];
  opened: string[];
  close(id: string): Promise<void>;
}

export type SessionConfig = typeof SESSION_CONFIG;
export { SESSION_CONFIG };

type ToolResult = { ok?: string; denied?: string; error?: string };
type ToolMsgExpect = { tool_call_id?: string; content?: string; content_contains?: string; id_synthesized?: boolean };
export type Expect = {
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
export type Scenario = {
  id: string;
  layer: "loop" | "wire";
  behavior: string;
  history?: { role: string; content: string }[];
  user: string;
  model: ScriptEntry[];
  model_repeat_last?: boolean;
  tool_results: ToolResult[];
  tool_result_default?: ToolResult;
  stop_after_host_calls?: number;
  expect: Expect;
  known_divergence?: { reason: string; verdict: string; ts?: Expect; loop?: Expect; sidecar?: Expect };
};
export type Fixture = {
  version: number;
  tools: { name: string; host: string; read_only: boolean }[];
  limits: { ts_max_turns: number; rust_max_steps: number; rust_max_tool_calls_per_step: number };
  scenarios: Scenario[];
};

/** The step cap a session opened with the shipped config runs under. */
export function liveStepCap(cfg: SessionConfig = SESSION_CONFIG): number {
  const explicit = (cfg as { maxSteps?: unknown }).maxSteps;
  return typeof explicit === "number" ? explicit : cfg.sidecarDefaults.maxSteps;
}

/** The per-reply tool cap a session opened with the shipped config runs under. */
export function livePerStepCap(cfg: SessionConfig = SESSION_CONFIG): number {
  const explicit = (cfg as { maxToolCallsPerStep?: unknown }).maxToolCallsPerStep;
  return typeof explicit === "number" ? explicit : cfg.sidecarDefaults.maxToolCallsPerStep;
}

/**
 * The `POST /sessions` body, built the way `build_session_body` does from the pinned config: the
 * webview's prompt and tools, every tool stamped host "core" with `{effect, trust, read_only}`.
 */
export function buildSessionBody(cfg: SessionConfig, systemPrompt: string, toolsJson: string, llm: { baseUrl: string; bearer: string }, model: string, contextTokens: number): string {
  const raw = JSON.parse(toolsJson) as unknown[];
  const tools = raw.map((t) => {
    const w = t as { function?: Record<string, unknown>; annotations?: { effect?: string; trust?: string } } & Record<string, unknown>;
    const f = (w.function ?? w) as { name?: string; description?: string; parameters?: unknown; annotations?: { effect?: string; trust?: string } };
    const ann = w.annotations ?? f.annotations;
    if (!f.name || !ann?.effect || !ann.trust) throw new Error(`tool ${String(f.name)} needs a name and effect/trust annotations`);
    return {
      name: f.name,
      description: String(f.description ?? "").slice(0, 2000),
      parameters: f.parameters ?? { type: "object" },
      host: "core",
      annotations: { effect: ann.effect, trust: ann.trust, read_only: ann.effect === "none" },
    };
  });
  const body: Record<string, unknown> = {
    model,
    systemPrompt,
    llm,
    tools,
    maxToolsPerRequest: cfg.maxToolsPerRequest,
    contextTokens,
    maxTokens: Math.min(cfg.aiMaxTokens, Math.floor(contextTokens / cfg.maxTokensContextDivisor)),
    hicAware: cfg.hicAware,
  };
  for (const k of ["maxSteps", "maxToolCallsPerStep"] as const) {
    const v = (cfg as Record<string, unknown>)[k];
    if (typeof v === "number") body[k] = v;
  }
  return JSON.stringify(body);
}

/**
 * The fixture's `sidecar` expectation (expect + the sidecar override), adapted to the live session:
 * cap-bound expectations rescaled to `cap`, and a scripted model error read as its HTTP status.
 */
export function liveExpectation(fx: Fixture, s: Scenario, cap: number): Expect {
  const ex: Expect = { ...s.expect, ...(s.known_divergence?.sidecar ?? {}) };
  const rust = fx.limits.rust_max_steps;
  if (ex.outcome === "step_limit" && ex.model_calls === rust && cap !== rust) {
    ex.model_calls = cap;
    if (ex.tool_events === rust) ex.tool_events = cap;
    if (ex.host_calls && ex.host_calls.length === rust && new Set(ex.host_calls).size === 1) {
      ex.host_calls = Array.from({ length: cap }, () => ex.host_calls![0]);
    }
    if (ex.last_request_roles && ex.last_request_roles.length === 1 + 2 * (rust - 1)) {
      ex.last_request_roles = ["user", ...Array.from({ length: cap - 1 }, () => ["assistant", "tool"]).flat()];
    }
    const needle = typeof ex.error_contains === "string" ? ex.error_contains : ex.error_contains?.sidecar;
    if (needle !== undefined) ex.error_contains = { sidecar: needle.replace(`of ${rust}`, `of ${cap}`) };
  }
  if (ex.outcome === "failed" && s.model.some((m) => m.error !== undefined)) {
    ex.error_contains = { sidecar: `HTTP ${SCRIPTED_ERROR_STATUS}` };
  }
  return ex;
}

export interface LiveResult {
  outcome: string | null;
  finalEvent: string | null;
  provider: { resolved: string | null; rejected: string | null };
  statuses: ChatStatus[];
  events: EventEnvelope[];
  requests: RecordedRequest[];
  hostCalls: { call: ToolCall; meta?: ToolCallMeta }[];
}

function needleOf(ex: Expect): string | undefined {
  if (ex.error_contains === undefined) return undefined;
  return typeof ex.error_contains === "string" ? ex.error_contains : ex.error_contains.sidecar;
}

/** Every difference between a live run and its expectation (empty = parity). */
export function compareLive(ex: Expect, r: LiveResult, opts: { maxToolsPerRequest: number; llmBearer: string }): string[] {
  const bad: string[] = [];
  const check = (what: string, ok: boolean, detail: string) => {
    if (!ok) bad.push(`${what}: ${detail}`);
  };
  const j = (v: unknown) => JSON.stringify(v);
  const types = r.events.map((e) => String(e.event.type));
  const errors = r.events.filter((e) => e.event.type === "error").map((e) => String(e.event.message ?? ""));

  if (ex.outcome) check("outcome", r.outcome === ex.outcome, `got ${r.outcome}, want ${ex.outcome}`);
  if (typeof ex.final === "string") {
    check("final event", r.finalEvent === ex.final, `got ${j(r.finalEvent)}, want ${j(ex.final)}`);
    check("provider answer", r.provider.resolved === ex.final, `got ${j(r.provider.resolved)}, rejected ${j(r.provider.rejected)}`);
  }
  if (ex.terminal) {
    const terminal = r.outcome === "answered" || r.outcome === "stopped" ? "done" : "error";
    check("terminal", terminal === ex.terminal, `got ${terminal}, want ${ex.terminal}`);
    if (ex.terminal === "error") {
      check("error event", errors.length > 0, "no error event");
      check("provider rejected", r.provider.rejected !== null, `resolved ${j(r.provider.resolved)}`);
      check("provider status", r.statuses[r.statuses.length - 1] === "error", `statuses ${j(r.statuses)}`);
    } else if (r.outcome === "answered") {
      check("provider status", r.statuses[r.statuses.length - 1] === "done", `statuses ${j(r.statuses)}`);
    } else if (r.outcome === "stopped") {
      check("provider stopped", r.provider.rejected === "stopped by you", `rejected ${j(r.provider.rejected)}`);
    }
  }
  check("done event", types[types.length - 1] === "done", `last event ${j(types[types.length - 1])}`);
  if (r.requests.length > 0) check("first event", types[0] === "step_start", `first event ${j(types[0])}`);
  if (ex.model_calls !== undefined) check("model_calls", r.requests.length === ex.model_calls, `got ${r.requests.length}, want ${ex.model_calls}`);
  if (ex.host_calls) check("host_calls", j(r.hostCalls.map((c) => c.call.name)) === j(ex.host_calls), `got ${j(r.hostCalls.map((c) => c.call.name))}, want ${j(ex.host_calls)}`);
  if (ex.host_arguments) check("host_arguments", j(r.hostCalls.map((c) => c.call.arguments)) === j(ex.host_arguments), `got ${j(r.hostCalls.map((c) => c.call.arguments))}, want ${j(ex.host_arguments)}`);
  if (ex.tool_events !== undefined) {
    const n = types.filter((t) => t === "tool_call").length;
    check("tool_events", n === ex.tool_events, `got ${n}, want ${ex.tool_events}`);
  }

  const last = (r.requests[r.requests.length - 1]?.body.messages ?? []).filter((m) => m.role !== "system");
  if (ex.last_request_roles) check("last_request_roles", j(last.map((m) => m.role)) === j(ex.last_request_roles), `got ${j(last.map((m) => m.role))}, want ${j(ex.last_request_roles)}`);
  if (ex.last_request_assistant_content !== undefined) {
    const asst = last.filter((m) => m.role === "assistant");
    const got = asst[asst.length - 1]?.content;
    check("last_request_assistant_content", got === ex.last_request_assistant_content, `got ${j(got)}`);
  }
  if (ex.tool_messages) {
    const tools = last.filter((m) => m.role === "tool");
    check("tool_messages count", tools.length === ex.tool_messages.length, `got ${tools.length}, want ${ex.tool_messages.length}`);
    ex.tool_messages.forEach((tm, i) => {
      const got = tools[i];
      if (!got) return;
      if (tm.tool_call_id !== undefined) check(`tool_messages[${i}].tool_call_id`, got.tool_call_id === tm.tool_call_id, `got ${j(got.tool_call_id)}`);
      if (tm.content !== undefined) check(`tool_messages[${i}].content`, got.content === tm.content, `got ${j(got.content)}, want ${j(tm.content)}`);
      if (tm.content_contains !== undefined) check(`tool_messages[${i}].content_contains`, String(got.content ?? "").includes(tm.content_contains), `got ${j(got.content)}, want ⊇ ${j(tm.content_contains)}`);
      if (tm.id_synthesized) {
        const hostId = r.hostCalls[i]?.call.id;
        check(`tool_messages[${i}].id_synthesized`, typeof got.tool_call_id === "string" && got.tool_call_id.length > 0 && got.tool_call_id === hostId, `tool msg id ${j(got.tool_call_id)}, host id ${j(hostId)}`);
      }
    });
  }
  if (ex.first_request_messages) {
    const first = (r.requests[0]?.body.messages ?? []).filter((m) => m.role !== "system").map((m) => ({ role: m.role, content: m.content }));
    check("first_request_messages", j(first) === j(ex.first_request_messages), `got ${j(first)}, want ${j(ex.first_request_messages)}`);
  }
  if (r.outcome === "failed" || r.outcome === "step_limit") {
    const needle = needleOf(ex);
    if (needle !== undefined) {
      const text = [...errors, r.provider.rejected ?? ""].join(" | ");
      check("error_contains", text.includes(needle), `got ${j(text)}, want ⊇ ${j(needle)}`);
    }
  }
  // Live-only: what the shipped session config promises on the wire.
  r.requests.forEach((q, i) => {
    const offered = Array.isArray(q.body.tools) ? q.body.tools.length : 0;
    check(`request[${i}] tools offered`, offered <= opts.maxToolsPerRequest, `offered ${offered} > ${opts.maxToolsPerRequest}`);
    check(`request[${i}] llm bearer`, q.authorization === `Bearer ${opts.llmBearer}`, "the model request did not carry the session's llm bearer");
  });
  return bad;
}

export interface LiveDeps {
  model: ModelScript;
  /** Builds the recording session API around a `POST /sessions` body builder. */
  makeApi: (buildOpenBody: (systemPrompt: string, toolsJson: string) => string) => RecordingSessionApi;
  modelBaseUrl: string;
  llmBearer: string;
  systemPrompt: string;
  tools: readonly unknown[];
  cfg?: SessionConfig;
  contextTokens?: number;
}

const DRAIN_TIMEOUT_MS = 15_000;

/** Run one scenario live (fresh session, closed afterwards). */
export async function runLiveScenario(s: Scenario, d: LiveDeps): Promise<LiveResult> {
  const cfg = d.cfg ?? SESSION_CONFIG;
  const api = d.makeApi((prompt, toolsJson) =>
    buildSessionBody(cfg, prompt, toolsJson, { baseUrl: d.modelBaseUrl, bearer: d.llmBearer }, "parity-model", d.contextTokens ?? 16_384),
  );
  const provider = createSidecarProvider(api, () => d.systemPrompt, () => d.tools);
  const noTools = async (): Promise<string> => {
    throw new Error("parity: no tool expected while replaying history");
  };
  const quiet = { onStatus: () => undefined, onToken: () => undefined, onToolCall: noTools };

  // History: earlier turns in the SAME session (the session keeps the transcript; core sends only
  // the new user text). The scripted model answers each with the recorded assistant reply.
  const transcript: { role: string; content: string }[] = [];
  const hist = s.history ?? [];
  for (let i = 0; i < hist.length; i++) {
    const m = hist[i];
    if (m.role !== "user") continue;
    const reply = hist[i + 1]?.role === "assistant" ? hist[i + 1].content : "";
    d.model.script([{ message: { role: "assistant", content: reply } }]);
    transcript.push(m);
    await provider.send({ messages: [...transcript], callbacks: quiet });
    transcript.push({ role: "assistant", content: reply });
  }

  d.model.script(s.model, s.model_repeat_last ?? false);
  const startSeq = api.events_seen.length;
  const statuses: ChatStatus[] = [];
  const hostCalls: LiveResult["hostCalls"] = [];
  const ctrl = new AbortController();
  const onToolCall = async (call: ToolCall, meta?: ToolCallMeta): Promise<string> => {
    const r = s.tool_results[hostCalls.length] ?? s.tool_result_default;
    hostCalls.push({ call: { ...call }, meta });
    if (s.stop_after_host_calls !== undefined && hostCalls.length === s.stop_after_host_calls) ctrl.abort();
    if (!r) throw new Error("parity script: no tool result scripted");
    if (r.error !== undefined) throw new Error(r.error);
    if (r.denied !== undefined) return r.denied;
    return r.ok ?? "";
  };
  let resolved: string | null = null;
  let rejected: string | null = null;
  try {
    const out = await provider.send({
      messages: [...transcript, { role: "user", content: s.user }],
      signal: ctrl.signal,
      callbacks: { onStatus: (st) => statuses.push(st), onToken: () => undefined, onToolCall },
    });
    resolved = out.content;
  } catch (e) {
    rejected = e instanceof Error ? e.message : String(e);
  }
  // A stopped turn keeps draining in the background; wait for its `done` before reading the log.
  const id = api.opened[api.opened.length - 1];
  const deadline = Date.now() + DRAIN_TIMEOUT_MS;
  const doneSeen = () => api.events_seen.slice(startSeq).some((e) => e.event.type === "done");
  while (!doneSeen() && Date.now() < deadline && id) {
    const after = api.events_seen.length ? Math.max(...api.events_seen.map((e) => e.seq)) : 0;
    await api.events(id, after, 1_000).catch(() => undefined);
  }
  if (id) await api.close(id).catch(() => undefined);

  const events = api.events_seen.slice(startSeq).sort((a, b) => a.seq - b.seq);
  const done = events.find((e) => e.event.type === "done");
  const fin = events.find((e) => e.event.type === "final");
  return {
    outcome: done ? String(done.event.outcome) : null,
    finalEvent: fin ? String(fin.event.content ?? "") : null,
    provider: { resolved, rejected },
    statuses,
    events,
    requests: [...d.model.requests],
    hostCalls,
  };
}
