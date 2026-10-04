// @vitest-environment node
// HUP-S1.9 — LIVE parity: parity-v1.json run end to end through the shipped path.
//
// parity.test.ts proves the TypeScript loop; the runtime's parity_tests / parity_wire_tests prove
// the Rust loop and the sidecar's wire parser. None of them runs the sidecar PROCESS: its session
// layer (default max_steps, the core-host tool_results round trip, the events long-poll, stop) or
// this repo's sidecarProvider.ts. This file does:
//
//   createSidecarProvider (the chat view the app uses)
//     -> HTTP control API, the same routes and bodies as src-tauri/src/hermes.rs HermesManager
//       -> the sidecar binary from the packaged app (CITRATE_HERMES_LIVE_BIN)
//         -> a scripted OpenAI-compatible model server on loopback (one per scenario)
//
// Sessions open with the body build_session_body makes (pinned by session-body-v1.json, which the
// Rust test build_session_body_matches_the_live_parity_fixture checks too), the app's annotated
// tool list, and no max_steps override, exactly as the app does today.
//
// The live suite runs only when CITRATE_HERMES_LIVE_BIN names a sidecar binary
// (scripts/hermes-live-parity.sh sets it from a built Citrate Core.app). With
// CITRATE_PARITY_LIVE_OUT set, the per-scenario results are written there as JSON.
// The checks that need no binary (the body builder, the scripted model server, the override
// table) always run.
import { describe, it, expect, beforeAll, afterAll } from "vitest";
import { createServer, type IncomingMessage, type ServerResponse } from "node:http";
import type { AddressInfo } from "node:net";
import { spawn, type ChildProcess } from "node:child_process";
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync, existsSync } from "node:fs";
import { createHash, randomBytes } from "node:crypto";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { createSidecarProvider, type SidecarSessionApi } from "../sidecarProvider";
import { TurnStopped, type ChatStatus, type ToolCall } from "../harness";
import { annotatedAgentTools } from "../toolAnnotations";

// ---------------------------------------------------------------------------------------------
// Fixture types (the same schema parity.test.ts reads).
// ---------------------------------------------------------------------------------------------
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
  history?: { role: string; content: string }[];
  user: string;
  model: ModelEntry[];
  model_repeat_last?: boolean;
  tool_results: ToolResult[];
  tool_result_default?: ToolResult;
  stop_after_host_calls?: number;
  expect: Expect;
  known_divergence?: { verdict: string; sidecar?: Expect };
};
type Fixture = { scenarios: Scenario[]; config_divergences: { id: string; verdict: string }[] };

const PARITY_RAW = readFileSync(resolve(process.cwd(), "src/agent/parity/parity-v1.json"));
const FIXTURE = JSON.parse(PARITY_RAW.toString("utf8")) as Fixture;
const BODY_FIXTURE = JSON.parse(readFileSync(resolve(process.cwd(), "src/agent/parity/session-body-v1.json"), "utf8")) as {
  input: { systemPrompt: string; tools: unknown[]; baseUrl: string; bearer: string; model: string; contextTokens: number };
  body: Record<string, unknown>;
};

// ---------------------------------------------------------------------------------------------
// The session body (mirror of hermes.rs build_session_body, pinned by session-body-v1.json).
// ---------------------------------------------------------------------------------------------
/** hermes.rs AI_MAX_TOKENS: the per-turn reply cap, at most a quarter of the context window. */
const AI_MAX_TOKENS = 2048;

export function liveSessionBody(systemPrompt: string, toolsJson: string, baseUrl: string, bearer: string, model: string, contextTokens: number) {
  const raw = JSON.parse(toolsJson) as unknown;
  if (!Array.isArray(raw)) throw new Error("tools must be a JSON array");
  if (raw.length > 64) throw new Error("at most 64 tools");
  const tools = raw.map((t: Record<string, unknown>) => {
    const f = (t.function as Record<string, unknown> | undefined) ?? t;
    const name = f.name;
    if (typeof name !== "string" || !/^[A-Za-z0-9_-]{1,64}$/.test(name)) throw new Error(`invalid tool name ${String(name)}`);
    const description = typeof f.description === "string" ? [...f.description].slice(0, 2000).join("") : "";
    const parameters = f.parameters ?? { type: "object" };
    const ann = (t.annotations ?? f.annotations) as { effect?: string; trust?: string } | undefined;
    if (!ann || !["none", "write", "spend", "sign"].includes(String(ann.effect)) || !["trusted", "untrusted"].includes(String(ann.trust))) {
      throw new Error(`tool ${name} needs effect/trust annotations`);
    }
    return { name, description, parameters, host: "core", annotations: { effect: ann.effect, trust: ann.trust, read_only: ann.effect === "none" } };
  });
  return {
    model,
    systemPrompt,
    llm: { baseUrl, bearer },
    tools,
    maxToolsPerRequest: 8,
    contextTokens,
    maxTokens: Math.min(AI_MAX_TOKENS, Math.floor(contextTokens / 4)),
    hicAware: true,
  };
}

// ---------------------------------------------------------------------------------------------
// A scripted OpenAI-compatible model on loopback (test fixture: replays a scenario's model[]).
// ---------------------------------------------------------------------------------------------
type WireMsg = { role: string; content?: string | null; tool_call_id?: string; tool_calls?: unknown[] };

interface ScriptedModel {
  baseUrl: string;
  /** The `messages` of every request, in order. */
  requests: WireMsg[][];
  close(): Promise<void>;
}

/** HTTP status the scripted model answers for an `{error}` entry (a provider-side failure). */
export const SCRIPTED_ERROR_STATUS = 503;

function readBody(req: IncomingMessage): Promise<string> {
  return new Promise((ok, fail) => {
    const chunks: Buffer[] = [];
    req.on("data", (c: Buffer) => chunks.push(c));
    req.on("end", () => ok(Buffer.concat(chunks).toString("utf8")));
    req.on("error", fail);
  });
}

export async function startScriptedModel(entries: ModelEntry[], repeatLast: boolean): Promise<ScriptedModel> {
  const requests: WireMsg[][] = [];
  let idx = 0;
  const server = createServer((req: IncomingMessage, res: ServerResponse) => {
    void (async () => {
      const text = await readBody(req);
      if (req.method !== "POST" || !(req.url ?? "").endsWith("/chat/completions")) {
        res.writeHead(404).end();
        return;
      }
      let body: { messages?: WireMsg[] } = {};
      try {
        body = JSON.parse(text) as { messages?: WireMsg[] };
      } catch {
        res.writeHead(400).end();
        return;
      }
      requests.push(body.messages ?? []);
      if (idx >= entries.length && !repeatLast) {
        res.writeHead(500, { "content-type": "application/json" }).end(JSON.stringify({ error: { message: "parity script exhausted" } }));
        return;
      }
      const entry = entries[Math.min(idx, entries.length - 1)];
      idx++;
      if (entry.error !== undefined) {
        res.writeHead(SCRIPTED_ERROR_STATUS, { "content-type": "application/json" }).end(JSON.stringify({ error: { message: entry.error } }));
        return;
      }
      if (entry.raw !== undefined) {
        res.writeHead(200, { "content-type": "application/json" }).end(entry.raw);
        return;
      }
      res.writeHead(200, { "content-type": "application/json" }).end(
        JSON.stringify({
          id: `parity-${idx}`,
          object: "chat.completion",
          choices: [{ index: 0, message: entry.message, finish_reason: "stop" }],
          usage: { prompt_tokens: 1, completion_tokens: 1, total_tokens: 2 },
        }),
      );
    })();
  });
  await new Promise<void>((ok) => server.listen(0, "127.0.0.1", ok));
  const port = (server.address() as AddressInfo).port;
  return {
    baseUrl: `http://127.0.0.1:${port}/v1`,
    requests,
    close: () => new Promise<void>((ok) => server.close(() => ok())),
  };
}

// ---------------------------------------------------------------------------------------------
// The control API, the way hermes.rs HermesManager calls it.
// ---------------------------------------------------------------------------------------------
/** hermes.rs: the events long-poll is capped at two thirds of HERMES_CONTROL_TIMEOUT (30 s). */
const EVENTS_WAIT_CAP_MS = 20_000;

export interface HttpSessionApi extends SidecarSessionApi {
  opened: string[];
  close(id: string): Promise<void>;
}

export function httpSessionApi(controlUrl: string, token: string, bodyFor: (systemPrompt: string, toolsJson: string) => unknown): HttpSessionApi {
  const call = async (method: string, path: string, body?: unknown): Promise<unknown> => {
    const res = await fetch(`${controlUrl}${path}`, {
      method,
      headers: { authorization: `Bearer ${token}`, ...(body === undefined ? {} : { "content-type": "application/json" }) },
      body: body === undefined ? undefined : JSON.stringify(body),
    });
    const text = await res.text();
    if (!res.ok) throw new Error(`hermes ${method} ${path.split("?")[0]}: HTTP ${res.status} ${text}`);
    return text ? (JSON.parse(text) as unknown) : null;
  };
  const opened: string[] = [];
  return {
    opened,
    async open(systemPrompt, toolsJson) {
      const v = (await call("POST", "/sessions", bodyFor(systemPrompt, toolsJson))) as { id: string };
      opened.push(v.id);
      return v.id;
    },
    async send(id, text) {
      await call("POST", `/sessions/${id}/messages`, { text });
    },
    async events(id, after, waitMs) {
      const wait = Math.min(waitMs, EVENTS_WAIT_CAP_MS);
      return (await call("GET", `/sessions/${id}/events?after=${after}&wait_ms=${wait}`)) as {
        events: { seq: number; event: Record<string, unknown> }[];
        lastSeq: number;
        busy: boolean;
      };
    },
    async toolResult(id, callId, status, content) {
      await call("POST", `/sessions/${id}/tool_results`, { callId, status, content });
    },
    async stop(id) {
      await call("POST", `/sessions/${id}/stop`);
    },
    async close(id) {
      await call("DELETE", `/sessions/${id}`);
    },
  };
}

// ---------------------------------------------------------------------------------------------
// Where the live path legitimately differs from the scripted-client runners.
// ---------------------------------------------------------------------------------------------
/**
 * The sidecar's default `max_steps` (agent-sidecar sessions.rs). The app opens sessions without
 * overriding it, so live turns run under it. Whether chat keeps 8 or moves to harness.ts's 6 is
 * the open owner decision `default_turn_cap`; when it is made, this value follows it.
 */
export const LIVE_SESSION_MAX_STEPS = 8;

type LiveOverride = { why: string; expect: Expect };

function turnCapOverride(steps: number): Expect {
  const roles = ["user"];
  for (let i = 0; i < steps - 1; i++) roles.push("assistant", "tool");
  return {
    model_calls: steps,
    host_calls: Array.from({ length: steps }, () => "memory_search"),
    tool_events: steps,
    last_request_roles: roles,
    error_contains: `step budget of ${steps} exhausted`,
  };
}

/**
 * Per-scenario differences of the live path. Each one names its cause: a `config_divergences`
 * entry of parity-v1.json, or `transport` (the fixture scripts provider failures at the client
 * layer; over real HTTP the sidecar's client reports the HTTP status, not the provider's text).
 */
export const LIVE_OVERRIDES: Record<string, LiveOverride> = {
  turn_cap_exhausted: { why: "config:default_turn_cap", expect: turnCapOverride(LIVE_SESSION_MAX_STEPS) },
  provider_error: { why: "transport", expect: { error_contains: `HTTP ${SCRIPTED_ERROR_STATUS}` } },
  provider_error_after_tool: { why: "transport", expect: { error_contains: `HTTP ${SCRIPTED_ERROR_STATUS}` } },
};

function effectiveLive(s: Scenario): Expect {
  return { ...s.expect, ...(s.known_divergence?.sidecar ?? {}), ...(LIVE_OVERRIDES[s.id]?.expect ?? {}) };
}

function needleFor(ex: Expect): string | undefined {
  if (ex.error_contains === undefined) return undefined;
  return typeof ex.error_contains === "string" ? ex.error_contains : ex.error_contains.sidecar;
}

// ---------------------------------------------------------------------------------------------
// The sidecar process.
// ---------------------------------------------------------------------------------------------
interface LiveSidecar {
  controlUrl: string;
  token: string;
  stop(): Promise<void>;
  stderr(): string;
}

async function freePort(): Promise<number> {
  const s = createServer();
  await new Promise<void>((ok) => s.listen(0, "127.0.0.1", ok));
  const port = (s.address() as AddressInfo).port;
  await new Promise<void>((ok) => s.close(() => ok()));
  return port;
}

async function startSidecar(bin: string, extraEnv: (dir: string) => Record<string, string> = () => ({})): Promise<LiveSidecar> {
  const dir = mkdtempSync(join(tmpdir(), "live-parity-"));
  const token = randomBytes(32).toString("hex");
  const tokenFile = join(dir, "hermes.token");
  writeFileSync(tokenFile, token, { mode: 0o600 });
  mkdirSync(join(dir, "capsules"));
  const port = await freePort();
  let err = "";
  const child: ChildProcess = spawn(bin, [], {
    env: {
      PATH: process.env.PATH ?? "/usr/bin:/bin",
      HOME: dir,
      CITRATE_HERMES_ADDR: `127.0.0.1:${port}`,
      CITRATE_HERMES_TOKEN_FILE: tokenFile,
      CITRATE_HERMES_CAPSULES: join(dir, "capsules"),
      ...extraEnv(dir),
    },
    stdio: ["ignore", "ignore", "pipe"],
  });
  child.stderr?.on("data", (c: Buffer) => {
    err = (err + c.toString("utf8")).slice(-20_000);
  });
  const controlUrl = `http://127.0.0.1:${port}`;
  const deadline = Date.now() + 20_000;
  for (;;) {
    if (child.exitCode !== null) throw new Error(`sidecar exited at start (${child.exitCode}): ${err}`);
    try {
      const r = await fetch(`${controlUrl}/health`);
      if (r.ok) break;
    } catch {
      // not listening yet
    }
    if (Date.now() > deadline) throw new Error(`sidecar did not answer /health: ${err}`);
    await new Promise((r) => setTimeout(r, 100));
  }
  return {
    controlUrl,
    token,
    stderr: () => err,
    async stop() {
      if (child.exitCode === null) {
        child.kill("SIGTERM");
        const gone = await Promise.race([
          new Promise<boolean>((ok) => child.once("exit", () => ok(true))),
          new Promise<boolean>((ok) => setTimeout(() => ok(false), 5_000)),
        ]);
        if (!gone) child.kill("SIGKILL");
      }
      rmSync(dir, { recursive: true, force: true });
    },
  };
}

// ---------------------------------------------------------------------------------------------
// One scenario through the live path.
// ---------------------------------------------------------------------------------------------
const SYSTEM_PROMPT = "You are Hermes, the Citrate Core agent.";
/** The live run's model window: the app's smallest planned --ctx-size is larger than any scenario. */
const LIVE_CONTEXT_TOKENS = 16_384;

interface LiveRun {
  outcome: "answered" | "stopped" | "step_limit" | "failed";
  final: string | null;
  streamed: string;
  error: string;
  statuses: ChatStatus[];
  requests: WireMsg[][];
  hostCalls: ToolCall[];
  /** The `done` event's outcome as the session reported it. */
  doneOutcome: string | null;
}

const quiet = { onStatus: () => undefined, onToken: () => undefined, onToolCall: async () => "" };

async function drainToDone(api: HttpSessionApi, id: string): Promise<string | null> {
  let after = 0;
  let outcome: string | null = null;
  const deadline = Date.now() + 15_000;
  while (Date.now() < deadline) {
    const page = await api.events(id, after, 1_000);
    for (const e of page.events) if (e.event.type === "done") outcome = String(e.event.outcome);
    after = Math.max(after, page.lastSeq);
    if (outcome !== null && !page.busy) return outcome;
  }
  return outcome;
}

async function runLive(s: Scenario, sidecar: LiveSidecar): Promise<LiveRun> {
  const history = s.history ?? [];
  // Earlier turns of the conversation are played as real turns of the same session.
  const replies: ModelEntry[] = [];
  for (let i = 0; i < history.length; i += 2) {
    if (history[i].role !== "user" || history[i + 1]?.role !== "assistant") throw new Error(`${s.id}: history must alternate user/assistant`);
    replies.push({ message: { role: "assistant", content: history[i + 1].content } });
  }
  const model = await startScriptedModel([...replies, ...s.model], s.model_repeat_last === true);
  const api = httpSessionApi(sidecar.controlUrl, sidecar.token, (p, t) =>
    liveSessionBody(p, t, model.baseUrl, "", "parity.gguf", LIVE_CONTEXT_TOKENS),
  );
  const provider = createSidecarProvider(api, () => SYSTEM_PROMPT, () => annotatedAgentTools());
  try {
    for (let i = 0; i < history.length; i += 2) {
      const r = await provider.send({ messages: history.slice(0, i + 1), callbacks: quiet });
      if (r.content !== history[i + 1].content) throw new Error(`${s.id}: history turn answered ${JSON.stringify(r.content)}`);
    }
    const before = model.requests.length;
    const statuses: ChatStatus[] = [];
    const hostCalls: ToolCall[] = [];
    let streamed = "";
    const controller = new AbortController();
    const onToolCall = async (c: ToolCall): Promise<string> => {
      const r = s.tool_results[hostCalls.length] ?? s.tool_result_default;
      hostCalls.push({ ...c });
      if (s.stop_after_host_calls !== undefined && hostCalls.length >= s.stop_after_host_calls) controller.abort();
      if (!r) throw new Error("parity script: no tool result scripted");
      if (r.error !== undefined) throw new Error(r.error);
      if (r.denied !== undefined) return r.denied;
      return r.ok ?? "";
    };
    let outcome: LiveRun["outcome"] = "answered";
    let final: string | null = null;
    let error = "";
    try {
      const res = await provider.send({
        messages: [...history, { role: "user", content: s.user }],
        signal: controller.signal,
        callbacks: {
          onStatus: (st) => statuses.push(st),
          onToken: (t) => {
            streamed += t;
          },
          onToolCall,
        },
      });
      final = res.content;
    } catch (e) {
      if (e instanceof TurnStopped) outcome = "stopped";
      else {
        error = e instanceof Error ? e.message : String(e);
        outcome = /step budget of \d+ exhausted/.test(error) ? "step_limit" : "failed";
      }
    }
    const id = api.opened[api.opened.length - 1];
    const doneOutcome = await drainToDone(api, id);
    return { outcome, final, streamed, error, statuses, requests: model.requests.slice(before), hostCalls, doneOutcome };
  } finally {
    for (const id of api.opened) await api.close(id).catch(() => undefined);
    await model.close();
  }
}

/** Every check parity.test.ts makes, against the live run (sidecar column). */
function checkLive(ex: Expect, r: LiveRun): void {
  if (ex.outcome) expect(r.outcome, "outcome").toBe(ex.outcome);
  if (ex.final !== undefined && ex.final !== null) {
    expect(r.final, "final").toBe(ex.final);
    expect(r.streamed, "streamed final").toBe(ex.final);
  }
  if (ex.terminal) {
    // A stopped turn rejects at once (the chat shows "stopped by you"); its terminal is the
    // session's own `done`, mapped the way the provider maps it (answered|stopped -> done).
    const terminal =
      r.outcome === "stopped"
        ? r.doneOutcome === "answered" || r.doneOutcome === "stopped"
          ? "done"
          : "error"
        : r.statuses[r.statuses.length - 1];
    expect(terminal, "terminal").toBe(ex.terminal);
  }
  expect(r.statuses[0], "first status").toBe("thinking");
  if (ex.model_calls !== undefined) expect(r.requests.length, "model_calls").toBe(ex.model_calls);
  if (ex.host_calls) expect(r.hostCalls.map((c) => c.name), "host_calls").toEqual(ex.host_calls);
  if (ex.host_arguments) expect(r.hostCalls.map((c) => c.arguments), "host_arguments").toEqual(ex.host_arguments);
  if (ex.tool_events !== undefined) expect(r.statuses.filter((x) => x === "tool").length, "tool_events").toBe(ex.tool_events);
  const last = (r.requests[r.requests.length - 1] ?? []).filter((m) => m.role !== "system");
  if (ex.last_request_roles) expect(last.map((m) => m.role), "last_request_roles").toEqual(ex.last_request_roles);
  if (ex.last_request_assistant_content !== undefined) {
    const asst = last.filter((m) => m.role === "assistant");
    expect(asst[asst.length - 1]?.content, "last assistant content").toBe(ex.last_request_assistant_content);
  }
  if (ex.tool_messages) {
    const tools = last.filter((m) => m.role === "tool");
    expect(tools.length, "tool_messages count").toBe(ex.tool_messages.length);
    ex.tool_messages.forEach((tm, i) => {
      const got = tools[i];
      if (tm.tool_call_id !== undefined) expect(got.tool_call_id, "tool_call_id").toBe(tm.tool_call_id);
      if (tm.content !== undefined) expect(got.content, "tool content").toBe(tm.content);
      if (tm.content_contains !== undefined) expect(String(got.content), "tool content").toContain(tm.content_contains);
      if (tm.id_synthesized) expect(typeof got.tool_call_id === "string" && got.tool_call_id.length > 0, "synthesized id").toBe(true);
    });
  }
  if (ex.first_request_messages) {
    const first = (r.requests[0] ?? []).filter((m) => m.role !== "system").map((m) => ({ role: m.role, content: m.content }));
    expect(first, "first_request_messages").toEqual(ex.first_request_messages);
  }
  if (r.outcome !== "answered") {
    const needle = needleFor(ex);
    if (needle !== undefined) expect(r.error, "error text").toContain(needle);
  }
}

// ---------------------------------------------------------------------------------------------
// Always-on checks.
// ---------------------------------------------------------------------------------------------
describe("live parity support (HUP-S1.9)", () => {
  it("builds the session body build_session_body makes (session-body-v1.json)", () => {
    const i = BODY_FIXTURE.input;
    expect(liveSessionBody(i.systemPrompt, JSON.stringify(i.tools), i.baseUrl, i.bearer, i.model, i.contextTokens)).toEqual(BODY_FIXTURE.body);
  });

  it("the app's annotated tools all pass the body builder, with no max_steps override", () => {
    const body = liveSessionBody(SYSTEM_PROMPT, JSON.stringify(annotatedAgentTools()), "http://127.0.0.1:1/v1", "", "m", 8192);
    expect(body.tools.length).toBe(annotatedAgentTools().length);
    expect(body.tools.every((t) => t.host === "core")).toBe(true);
    expect("maxSteps" in body).toBe(false);
    expect(body.maxTokens).toBe(2048);
  });

  it("refuses a tool without effect/trust annotations, as core does", () => {
    expect(() => liveSessionBody("p", JSON.stringify([{ name: "x" }]), "http://127.0.0.1:1/v1", "", "m", 8192)).toThrow(/annotations/);
  });

  it("the scripted model replays messages, raw bodies, errors and the repeat rule over HTTP", async () => {
    const m = await startScriptedModel(
      [{ message: { role: "assistant", content: "a" } }, { raw: "not json" }, { error: "down" }],
      false,
    );
    const post = (n: number) =>
      fetch(`${m.baseUrl}/chat/completions`, { method: "POST", body: JSON.stringify({ messages: [{ role: "user", content: String(n) }] }) });
    const r1 = await post(1);
    expect(((await r1.json()) as { choices: { message: { content: string } }[] }).choices[0].message.content).toBe("a");
    expect(await (await post(2)).text()).toBe("not json");
    expect((await post(3)).status).toBe(SCRIPTED_ERROR_STATUS);
    expect((await post(4)).status).toBe(500);
    expect(m.requests.map((x) => x[0].content)).toEqual(["1", "2", "3", "4"]);
    await m.close();

    const rep = await startScriptedModel([{ message: { role: "assistant", content: "again" } }], true);
    for (let i = 0; i < 3; i++) {
      const r = await fetch(`${rep.baseUrl}/chat/completions`, { method: "POST", body: "{}" });
      expect(r.status).toBe(200);
    }
    await rep.close();
  });

  it("every live override names a real scenario and a recorded cause", () => {
    const ids = new Set(FIXTURE.scenarios.map((s) => s.id));
    const configIds = new Set(FIXTURE.config_divergences.map((c) => c.id));
    for (const [id, o] of Object.entries(LIVE_OVERRIDES)) {
      expect(ids.has(id), id).toBe(true);
      const ok = o.why === "transport" || (o.why.startsWith("config:") && configIds.has(o.why.slice("config:".length)));
      expect(ok, `${id}: ${o.why}`).toBe(true);
    }
  });

  it("the turn-cap override is the fixture's own turn-cap shape at the sidecar default", () => {
    const s = FIXTURE.scenarios.find((x) => x.id === "turn_cap_exhausted");
    expect(s).toBeDefined();
    // At 6 steps the derived expectation equals the fixture's (so only the step count differs).
    const at6 = turnCapOverride(6);
    expect(at6.model_calls).toBe(s?.expect.model_calls);
    expect(at6.host_calls).toEqual(s?.expect.host_calls);
    expect(at6.last_request_roles).toEqual(s?.expect.last_request_roles);
    expect(at6.error_contains).toBe(needleFor(s?.expect ?? {}));
    expect(LIVE_OVERRIDES.turn_cap_exhausted.expect.model_calls).toBe(LIVE_SESSION_MAX_STEPS);
  });

  it("the control API calls the routes and bodies hermes.rs uses", async () => {
    const seen: { method: string; url: string; body: string; auth: string }[] = [];
    const server = createServer((req, res) => {
      void readBody(req).then((body) => {
        seen.push({ method: req.method ?? "", url: req.url ?? "", body, auth: String(req.headers.authorization ?? "") });
        const reply = req.url === "/sessions" ? { id: "s1" } : req.url?.includes("/events") ? { events: [], lastSeq: 0, busy: false } : { ok: true };
        res.writeHead(200, { "content-type": "application/json" }).end(JSON.stringify(reply));
      });
    });
    await new Promise<void>((ok) => server.listen(0, "127.0.0.1", ok));
    const url = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
    const token = randomBytes(8).toString("hex");
    const api = httpSessionApi(url, token, () => ({ b: 1 }));
    expect(await api.open("p", "[]")).toBe("s1");
    await api.send("s1", "hi");
    await api.events("s1", 3, 60_000);
    await api.toolResult("s1", "c1", "ok", "done");
    await api.stop("s1");
    await api.close("s1");
    await new Promise<void>((ok) => server.close(() => ok()));
    expect(seen.map((x) => `${x.method} ${x.url}`)).toEqual([
      "POST /sessions",
      "POST /sessions/s1/messages",
      `GET /sessions/s1/events?after=3&wait_ms=${EVENTS_WAIT_CAP_MS}`,
      "POST /sessions/s1/tool_results",
      "POST /sessions/s1/stop",
      "DELETE /sessions/s1",
    ]);
    expect(seen.every((x) => x.auth === `Bearer ${token}`)).toBe(true);
    expect(JSON.parse(seen[3].body)).toEqual({ callId: "c1", status: "ok", content: "done" });
    expect(JSON.parse(seen[1].body)).toEqual({ text: "hi" });
  });
});

// ---------------------------------------------------------------------------------------------
// The live suite.
// ---------------------------------------------------------------------------------------------
const LIVE_BIN = process.env.CITRATE_HERMES_LIVE_BIN ?? "";
const LIVE_OUT = process.env.CITRATE_PARITY_LIVE_OUT ?? "";

type ScenarioRow = { id: string; pass: boolean; override: string | null; outcome: string; model_calls: number; host_calls: string[]; error: string; failure?: string };
const results: ScenarioRow[] = [];
let processSplit: { pass: boolean; detail: string } | null = null;
let parityStderr = "";

// Written once every live test in this file has run (both describes).
afterAll(() => {
  if (LIVE_BIN === "" || LIVE_OUT === "") return;
  writeFileSync(
    LIVE_OUT,
    JSON.stringify(
      {
        suite: "parity-v1 live",
        ranAt: new Date().toISOString(),
        // From the app bundle down (no machine-specific prefix).
        binary: LIVE_BIN.replace(/^.*\/([^/]+\.app\/)/, "$1"),
        binarySha256: createHash("sha256").update(readFileSync(LIVE_BIN)).digest("hex"),
        parityV1Sha256: createHash("sha256").update(PARITY_RAW).digest("hex"),
        sessionMaxSteps: LIVE_SESSION_MAX_STEPS,
        passed: results.filter((r) => r.pass).length,
        failed: results.filter((r) => !r.pass).length,
        results,
        processSplit,
        sidecarStderrTail: parityStderr.slice(-2_000),
      },
      null,
      2,
    ) + "\n",
  );
});

describe.skipIf(LIVE_BIN === "")("parity-v1 LIVE: sidecarProvider -> packaged sidecar binary (HUP-S1.9)", () => {
  let sidecar: LiveSidecar | null = null;

  beforeAll(async () => {
    if (!existsSync(LIVE_BIN)) throw new Error(`CITRATE_HERMES_LIVE_BIN does not exist: ${LIVE_BIN}`);
    sidecar = await startSidecar(LIVE_BIN);
  }, 30_000);

  afterAll(async () => {
    parityStderr = sidecar?.stderr() ?? "";
    await sidecar?.stop();
  });

  for (const s of FIXTURE.scenarios) {
    const o = LIVE_OVERRIDES[s.id];
    const tag = o ? ` (live: ${o.why})` : s.known_divergence?.sidecar ? " (sidecar override)" : "";
    it(`${s.id}${tag}`, async () => {
      if (!sidecar) throw new Error("sidecar not started");
      const r = await runLive(s, sidecar);
      const row = { id: s.id, pass: false, override: o?.why ?? null, outcome: r.outcome, model_calls: r.requests.length, host_calls: r.hostCalls.map((c) => c.name), error: r.error };
      try {
        checkLive(effectiveLive(s), r);
        results.push({ ...row, pass: true });
      } catch (e) {
        results.push({ ...row, failure: e instanceof Error ? e.message.slice(0, 500) : String(e) });
        throw e;
      }
    }, 60_000);
  }
});

// ---------------------------------------------------------------------------------------------
// The process split, on the same packaged binary: the toolchain worker is its own process, a
// kill -9 of it is reported and restarted, and the control plane (the loop) keeps answering.
// ---------------------------------------------------------------------------------------------
type WorkerRow = { kind: string; state: string; pid?: number | null; restarts?: number; last_exit?: string | null; detail?: string };

describe.skipIf(LIVE_BIN === "")("process split LIVE: packaged sidecar workers (HUP-S1.9)", () => {
  it("runs the toolchain in its own process, restarts it after kill -9, and keeps the loop up", async () => {
    const sc = await startSidecar(LIVE_BIN, (dir) => {
      mkdirSync(join(dir, "root"));
      mkdirSync(join(dir, "bin"));
      return {
        CITRATE_HERMES_TOOLCHAIN: "1",
        CITRATE_HERMES_TOOLCHAIN_ROOTS: join(dir, "root"),
        CITRATE_HERMES_TOOLCHAIN_PATH: join(dir, "bin"),
      };
    });
    const workers = async (): Promise<WorkerRow[]> => {
      const r = await fetch(`${sc.controlUrl}/workers`, { headers: { authorization: `Bearer ${sc.token}` } });
      expect(r.status).toBe(200);
      return ((await r.json()) as { workers: WorkerRow[] }).workers;
    };
    const until = async (pred: (w: WorkerRow) => boolean): Promise<WorkerRow> => {
      const deadline = Date.now() + 20_000;
      for (;;) {
        const tc = (await workers()).find((w) => w.kind === "toolchain");
        if (tc && pred(tc)) return tc;
        if (Date.now() > deadline) throw new Error(`toolchain worker never reached the state: ${JSON.stringify(tc)}`);
        await new Promise((r) => setTimeout(r, 100));
      }
    };
    try {
      expect((await fetch(`${sc.controlUrl}/workers`)).status, "bearer required").toBe(401);
      const first = await until((w) => w.state === "running" && typeof w.pid === "number");
      const browser = (await workers()).find((w) => w.kind === "browser");
      expect(browser?.state).toBe("not_built");
      expect(browser?.detail ?? "").not.toMatch(/no browser tools/);
      process.kill(first.pid as number, "SIGKILL");
      const again = await until((w) => w.state === "running" && w.pid !== first.pid && (w.restarts ?? 0) >= 1);
      expect(again.last_exit).toBe("killed by signal 9");
      expect((await fetch(`${sc.controlUrl}/health`)).status, "control plane stayed up").toBe(200);
      processSplit = { pass: true, detail: `toolchain pid ${first.pid} killed (SIGKILL); restarted as pid ${again.pid}, restarts ${again.restarts}, last exit "${again.last_exit}"; /health 200 throughout; browser ${browser?.state}` };
    } catch (e) {
      processSplit = { pass: false, detail: e instanceof Error ? e.message.slice(0, 500) : String(e) };
      throw e;
    } finally {
      await sc.stop();
    }
  }, 60_000);
});
