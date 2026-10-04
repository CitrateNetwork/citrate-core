// =====================================================================
// HUP-S1.1c — the sidecar chat provider (ADR loop-in-sidecar: brain in the sidecar, hands in core).
//
// The loop runs in the Hermes sidecar. This provider is a VIEW over it: it opens one session, sends
// each user turn, long-polls the session's events, and for every core-hosted tool call runs the
// store's gated handler (`onToolCall` → `store.handleTool`, i.e. the same approval gates as today)
// and posts the result back. It never talks to the model and never decides an approval.
// =====================================================================
import { parseFileChange, isFileTool } from "./fileChanges";
import { TurnStopped, untilStopped, usageEventOf, type ChatProvider, type ReattachResult, type SendOpts, type ToolCall, type ToolCallMeta, type TurnActivityEvent, type WorkflowRunOpts } from "./harness";
import type { McpPendingView, SessionPersonaChoice, ShellPendingView } from "../bridge/domains";
import type { WorkflowRunView } from "./learn";

/** The session calls this provider needs (bridge.agentHarness in the app; a fake in tests). */
export interface SidecarSessionApi {
  /** `persona` is passed only when the member chose one (HUP-S3.3). */
  open(systemPrompt: string, toolsJson: string, persona?: SessionPersonaChoice): Promise<string>;
  send(id: string, text: string): Promise<void>;
  events(id: string, after: number, waitMs: number): Promise<{ events: { seq: number; event: Record<string, unknown> }[]; lastSeq: number; busy: boolean; pendingCoreCalls?: string[] }>;
  toolResult(id: string, callId: string, status: "ok" | "denied" | "error", content: string): Promise<void>;
  stop(id: string): Promise<void>;
  /** Close a session this provider no longer uses (absent = sessions are left to the sidecar). */
  close?(id: string): Promise<void>;
  /** HUP-S3.3 — start a track's catalog workflow in the session (absent = workflows unavailable). */
  trackWorkflowRun?(id: string, workflowId: string): Promise<{ run_id: string }>;
  /** HUP-S3.3 — a workflow run's state (`running` until the sidecar's verifiers have decided). */
  workflowStatus?(id: string, runId: string): Promise<WorkflowRunView>;
  /** HUP-S1.1 — the session's last event sequence number, without reading its events (absent =
   *  the provider does not skip turns other clients ran in the session). */
  position?(id: string): Promise<number>;
  /** HUP-S2.2 — the shell_run commands the sidecar holds for the member (absent = not supported). */
  shellPending?(id: string): Promise<ShellPendingView[]>;
  /** HUP-S2.2 — the member's decision, bound to the exact argv and folder shown. */
  shellDecide?(id: string, approvalId: string, allow: boolean, argv: string[], cwd: string): Promise<void>;
  /** HUP-S4.1: the MCP requests the sidecar holds for the member (absent = not supported). */
  mcpPending?(id: string): Promise<McpPendingView[]>;
  /** HUP-S4.1: the member's decision, bound to the subject (arguments or URL) shown. */
  mcpDecide?(id: string, approvalId: string, allow: boolean, subject: string): Promise<void>;
}

/** HUP-S4.1: every MCP tool the sidecar offers is named `mcp__<server>__<tool>`. */
export const MCP_TOOL_PREFIX = "mcp__";

/** HUP-S2.2 — the sidecar-hosted tool whose every call the member decides. */
export const SHELL_RUN_TOOL = "shell_run";

/** HUP-S2.2 (US-2.2 AC3) — sidecar tools whose results are command runs for the Activity log. */
const COMMAND_RUN_TOOLS = new Set([SHELL_RUN_TOOL, "forge_test", "slither_scan", "aderyn_scan", "medusa_fuzz"]);

/**
 * HUP-S1.1 — what is saved so the view can pick its session back up after a reload: the session
 * id, the last event sequence number this view finished acting on, and the core-hosted calls it
 * had started but not yet answered. No prompt, no messages, no keys.
 */
export interface SavedSidecarSession {
  v: 1;
  id: string;
  lastSeq: number;
  inFlight: string[];
}

/** Where the saved session lives (localStorage in the app; memory in tests). */
export interface SidecarSessionStore {
  load(): SavedSidecarSession | null;
  save(s: SavedSidecarSession | null): void;
}

/** Read a saved session, refusing anything that is not exactly the saved shape. */
export function parseSavedSession(raw: unknown): SavedSidecarSession | null {
  if (!isRecord(raw) || raw.v !== 1) return null;
  const { id, lastSeq, inFlight } = raw;
  if (typeof id !== "string" || !/^[A-Za-z0-9-]{1,64}$/.test(id)) return null;
  if (typeof lastSeq !== "number" || !Number.isSafeInteger(lastSeq) || lastSeq < 0) return null;
  if (!Array.isArray(inFlight) || inFlight.length > 64 || !inFlight.every((c) => typeof c === "string" && c.length <= 128)) return null;
  return { v: 1, id, lastSeq, inFlight: inFlight as string[] };
}

/** The app's store: one small localStorage entry, written at once (a reload must not lose it). */
export function localSessionStore(key = "citrate.hermes.sidecarSession.v1"): SidecarSessionStore {
  return {
    load() {
      try {
        return parseSavedSession(JSON.parse(localStorage.getItem(key) ?? "null"));
      } catch {
        return null;
      }
    },
    save(v) {
      try {
        if (v) localStorage.setItem(key, JSON.stringify(v));
        else localStorage.removeItem(key);
      } catch {
        /* storage unavailable: the session simply is not picked back up after a reload */
      }
    },
  };
}

/** Timing for finding a held shell_run command (tests shorten it), and the session store. */
export interface SidecarProviderOptions {
  /** Pause between looks for the held command (ms). */
  shellPollMs?: number;
  /** How long to look before leaving the decision to the sidecar's own timeout (ms). */
  shellWaitMs?: number;
  /** HUP-S4.1: pause between looks for MCP requests held for an in-flight MCP call (ms). */
  mcpPollMs?: number;
  /** HUP-S4.1: how long to keep looking while an MCP call is in flight (a task can run long). */
  mcpWaitMs?: number;
  /** HUP-S1.1: save the session so a reloaded view can pick it back up (absent = not saved). */
  store?: SidecarSessionStore;
}

/** HUP-S1.1 — said when the saved session is gone because Hermes restarted. */
export const SESSION_GONE_NOTICE =
  "Hermes restarted, so your earlier conversation with it has ended. Your next message starts a new one.";

/** HUP-S1.1 — posted for a core call this view had started when it reloaded. Its outcome is unknown. */
export const INTERRUPTED_RESULT =
  "interrupted: the app reloaded while this call was in progress, so its outcome is unknown. Check before retrying it.";

/** A bridge error that means the sidecar no longer has this session. */
export function isSessionGone(e: unknown): boolean {
  const m = e instanceof Error ? e.message : String(e);
  return /returned 404\b/.test(m);
}

const SUMMARY_CHARS = 300;
const clipText = (t: string) => (t.length > SUMMARY_CHARS ? t.slice(0, SUMMARY_CHARS) + "…" : t);
const isRecord = (v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null && !Array.isArray(v);
const numOrNull = (v: unknown): number | null => (typeof v === "number" && Number.isFinite(v) ? v : null);

/**
 * HUP-S2.2 (US-2.2 AC3) — a command run from its `tool_result` event. The content is the run's
 * envelope JSON (prefixed with "tool error: " when the run failed), or "declined: <why>. Nothing
 * was done." when the member said no. Anything unreadable is reported as it is, never invented.
 */
export function commandRunOf(callId: string, tool: string, status: string, content: string): Extract<TurnActivityEvent, { kind: "command_run" }> {
  const base = { kind: "command_run" as const, callId, tool, exitCode: null, durationMs: null, timedOut: false, sandbox: null };
  if (status === "denied") {
    const why = content.replace(/^declined:\s*/, "").replace(/\.?\s*Nothing was done\.\s*$/, "");
    return { ...base, status: "declined", summary: clipText(why) };
  }
  const text = content.replace(/^tool error:\s*/, "");
  let env: unknown = null;
  try {
    env = JSON.parse(text);
  } catch {
    env = null;
  }
  if (!isRecord(env) || typeof env.status !== "string") {
    return { ...base, status: status === "ok" ? "completed" : "failed", summary: clipText(text) };
  }
  const run = isRecord(env.run) ? env.run : {};
  const sandbox = isRecord(run.sandbox) && typeof run.sandbox.summary === "string" ? run.sandbox.summary : null;
  return {
    ...base,
    status: env.status,
    summary: clipText(typeof env.summary === "string" ? env.summary : ""),
    exitCode: numOrNull(run.exit_code),
    durationMs: numOrNull(run.duration_ms),
    timedOut: run.timed_out === true || env.status === "timed_out",
    sandbox,
  };
}

const sleep = (ms: number) => new Promise<void>((r) => setTimeout(r, ms));

/** Said when the session api cannot run track workflows. */
export const WORKFLOWS_NEED_SIDECAR = "track workflows run in the Hermes sidecar loop, which this chat is not using";

const POLL_WAIT_MS = 15_000;
/** Consecutive empty, not-busy polls after which the turn is considered lost (sidecar restarted). */
const MAX_IDLE_POLLS = 3;

/** A gated handler's reply that means "the member said no" → the loop sees `denied`, not `ok`. */
function statusOf(result: string): "ok" | "denied" {
  return /\bdeclined\b/i.test(result) ? "denied" : "ok";
}

/** Posted back (status "error") for a core call whose arguments are not a JSON object. */
export const NON_OBJECT_ARGUMENTS =
  "the arguments must be a JSON object; nothing was run. Retry with a JSON object.";

/**
 * A core call's arguments, normalized for the gated handler: blank means `{}` (as the loop treats
 * it); anything else must parse to a JSON object. `null` means "do not run this call".
 */
function objectArguments(raw: unknown): string | null {
  // The loop always sends arguments as a JSON string; any other shape is not run.
  if (typeof raw !== "string") return null;
  const text = raw;
  if (text.trim() === "") return "{}";
  try {
    const v: unknown = JSON.parse(text);
    return v !== null && typeof v === "object" && !Array.isArray(v) ? text : null;
  } catch {
    return null;
  }
}

export function createSidecarProvider(
  api: SidecarSessionApi,
  systemPrompt: () => string,
  tools: () => readonly unknown[],
  // HUP-S3.3: the persona the session applies (skill allowlist + tool emphasis); null = none.
  persona: () => SessionPersonaChoice | null = () => null,
  options: SidecarProviderOptions = {},
): ChatProvider {
  const shellPollMs = options.shellPollMs ?? 250;
  const shellWaitMs = options.shellWaitMs ?? 10_000;
  const mcpPollMs = options.mcpPollMs ?? 250;
  const mcpWaitMs = options.mcpWaitMs ?? 20 * 60_000;
  const store = options.store;
  let sessionId: string | null = null;
  let lastSeq = 0;
  // HUP-S1.1: core calls started and not yet answered (saved, so a reload can report them).
  const started = new Set<string>();
  /** Save where this view is (or forget the session). Never throws. */
  function persist(seq: number = lastSeq): void {
    if (!store) return;
    try {
      store.save(sessionId ? { v: 1, id: sessionId, lastSeq: seq, inFlight: [...started] } : null);
    } catch {
      /* a store that fails only costs the reattach */
    }
  }
  /** Forget the session (it was stopped, or the sidecar no longer has it). */
  function dropSession(id: string): void {
    if (sessionId === id) sessionId = null;
    started.clear();
    persist();
  }
  // Replay safety: core calls already run whose result the loop has not yet recorded (a
  // `tool_result` event for that id). A second announcement of one of these is ignored. Ids leave
  // the set once recorded, because the sidecar may reuse ids (call_0, call_1, …) on later steps.
  const inFlight = new Set<string>();
  // HUP-S2.9: sidecar-hosted file tool calls of the current turn (call id -> tool name), so their
  // results can be reported as undoable file changes. Core never runs these calls.
  const sidecarFileCalls = new Map<string, string>();
  // HUP-S2.2: sidecar-hosted command calls of the current turn (call id -> tool name), so their
  // results are reported to the Activity log as command runs.
  const sidecarRunCalls = new Map<string, string>();

  // HUP-S7.6: turns run one at a time. A stopped turn keeps draining its session events (to its
  // `done`) after the caller has moved on, so the next turn starts from the right sequence number
  // and never sees the stopped turn's events.
  let previous: Promise<unknown> = Promise.resolve();

  /**
   * Run `call` on this provider's session; when the sidecar no longer holds that session (404),
   * open a fresh one and run it once more there. Any other failure, or a second 404, is reported.
   */
  async function onSession<T>(call: (id: string) => Promise<T>): Promise<{ id: string; value: T }> {
    const first = await ensureSession();
    try {
      return { id: first, value: await call(first) };
    } catch (e) {
      // The sidecar answers 404 for a session it no longer holds (it restarted, or its table was
      // full and it replaced this idle session with a newer one).
      if (!isSessionGone(e)) throw e;
      dropSession(first);
      const id = await ensureSession();
      return { id, value: await call(id) };
    }
  }

  async function ensureSession(): Promise<string> {
    if (!sessionId) {
      const p = persona();
      sessionId = p ? await api.open(systemPrompt(), JSON.stringify(tools()), p) : await api.open(systemPrompt(), JSON.stringify(tools()));
      lastSeq = 0;
      started.clear();
      persist();
    }
    return sessionId;
  }

  /**
   * Long-poll the session's events and act on them until the work ends: a chat turn ends at its
   * `done` event; a workflow run (`runId`) ends only when the session is idle and the sidecar says
   * the run left `running` (each step attempt has its own `done`). Core-hosted calls run through
   * `onToolCall` (the store's gated handler) and their results are posted back; verifier verdicts
   * are reported as activity. Returns the last final answer and the failure, if any.
   */
  async function drive(
    id: string,
    opts: { callbacks: SendOpts["callbacks"]; signal?: AbortSignal },
    runId: string | null,
    resumed = false,
  ): Promise<{ final: string; failure: string | null; stopping: boolean; view: WorkflowRunView | null }> {
    const { callbacks, signal } = opts;
    // HUP-S1.1 (g1-render): streamed text of the current step. A resumed view missed the start of
    // the step it came back into, so it shows that step's answer from its final event instead.
    let stepText = "";
    let streamDeltas = !resumed;
    let shownText = false;
    const show = (text: string) => {
      if (!text) return;
      callbacks.onStatus("streaming");
      callbacks.onToken(text);
      shownText = true;
    };
    // HUP-S7.6: Stop goes through the session's own stop route (it ends the turn and releases a
    // waiting tool); from then on this turn only drains its events to `done` and runs nothing.
    let stopping = false;
    // A session this turn stopped is not reused; it is closed once its events are drained.
    let abandoned = false;
    const abandon = () => {
      dropSession(id);
      abandoned = true;
    };
    const stopOnce = () => {
      if (stopping) return;
      stopping = true;
      api.stop(id).catch(() => undefined);
    };
    signal?.addEventListener("abort", stopOnce, { once: true });
    if (signal?.aborted) stopOnce();

    /**
     * HUP-S2.2 (US-2.2 AC2): find the command the sidecar holds for `callId`, ask the member, and
     * send the decision with exactly the argv and folder shown. Stops looking once the sidecar has
     * answered the call (it refused before holding it) or after `shellWaitMs`. A failed decision is
     * reported and never retried as an approval.
     */
    async function decideShell(callId: string, laterInPage: boolean): Promise<void> {
      if (!api.shellPending || !api.shellDecide || laterInPage) return;
      const deadline = Date.now() + shellWaitMs;
      let pending: ShellPendingView | undefined;
      for (;;) {
        if (stopping) return;
        const list = await api.shellPending(id).catch(() => [] as ShellPendingView[]);
        pending = list.find((p) => p.callId === callId);
        if (pending) break;
        // Peek (without consuming) whether the sidecar already answered this call.
        const peek = await api.events(id, lastSeq, 0).catch(() => null);
        if (peek?.events.some((e) => e.event.type === "tool_result" && e.event.call_id === callId)) return;
        if (Date.now() >= deadline) return;
        await sleep(shellPollMs);
      }
      let allow = false;
      // HUP-S7.6: the held command is an approval the member is asked for.
      callbacks.onActivity?.({ kind: "approval", callId, tool: pending.tool, state: "pending" });
      if (callbacks.onCommandApproval) {
        try {
          allow = (await untilStopped(callbacks.onCommandApproval(pending), signal)) === true;
        } catch (e) {
          if (e instanceof TurnStopped) stopOnce();
          allow = false;
        }
      }
      if (stopping) allow = false;
      try {
        await api.shellDecide(id, pending.id, allow, pending.argv, pending.cwd);
        callbacks.onActivity?.({ kind: "approval", callId, tool: pending.tool, state: allow ? "approved" : "declined" });
      } catch (e) {
        callbacks.onActivity?.({ kind: "approval", callId, tool: pending.tool, state: "failed" });
        callbacks.onActivity?.({
          kind: "notice",
          text: `Your decision on the command ${pending.argv[0]} did not reach Hermes (${e instanceof Error ? e.message : String(e)}). It was not run.`,
        });
      }
    }

    /**
     * HUP-S4.1 (US-4.1 AC2): while the MCP call `callId` is in flight, put every request the
     * sidecar holds for it in front of the member (an effectful call after taint, or a page the
     * server asks to open) and send each decision bound to the subject shown. Stops once the
     * sidecar answered the call, the turn stopped, or after `mcpWaitMs`. A failed decision is
     * reported and never retried as an approval.
     */
    async function decideMcp(callId: string, laterInPage: boolean): Promise<void> {
      if (!api.mcpPending || !api.mcpDecide || laterInPage) return;
      const deadline = Date.now() + mcpWaitMs;
      const decided = new Set<string>();
      for (;;) {
        if (stopping) return;
        const list = await api.mcpPending(id).catch(() => [] as McpPendingView[]);
        for (const card of list.filter((p) => p.callId === callId && !decided.has(p.id))) {
          decided.add(card.id);
          let allow = false;
          if (callbacks.onMcpApproval) {
            try {
              allow = (await untilStopped(callbacks.onMcpApproval(card), signal)) === true;
            } catch (e) {
              if (e instanceof TurnStopped) stopOnce();
              allow = false;
            }
          }
          if (stopping) allow = false;
          try {
            await api.mcpDecide(id, card.id, allow, card.subject);
          } catch (e) {
            const msg = e instanceof Error ? e.message : String(e);
            callbacks.onActivity?.({
              kind: "notice",
              text: msg.startsWith("MCP_PAGE_NOT_OPENED")
                ? `You allowed the page for MCP server ${card.server}, but it could not be opened (${msg.replace(/^MCP_PAGE_NOT_OPENED:\s*/, "")}). Open ${card.subject} yourself if you still want to.`
                : `Your decision on ${card.remoteTool} (MCP server ${card.server}) did not reach Hermes (${msg}). It was not run.`,
            });
          }
        }
        const peek = await api.events(id, lastSeq, 0).catch(() => null);
        if (peek?.events.some((e) => e.event.type === "tool_result" && e.event.call_id === callId)) return;
        if (Date.now() >= deadline) return;
        await sleep(mcpPollMs);
      }
    }

    try {
      let final = "";
      let failure: string | null = null;
      let idle = 0;
      for (;;) {
        const page = await api.events(id, lastSeq, POLL_WAIT_MS);
        const seen = lastSeq;
        lastSeq = Math.max(lastSeq, page.lastSeq);
        // Events at or below what this provider already processed are re-deliveries: skip them.
        const fresh = page.events.filter((e) => e.seq > seen);
        let finished = false;
        for (const { seq, event: ev } of fresh) {
          const type = String(ev.type);
          if (type === "done") {
            if (ev.outcome === "stopped") {
              // A session's stop switch stays on, so every later turn in it would end at once with
              // an empty answer. Leave it (closed once drained); the next turn opens a fresh session.
              abandon();
              if (!stopping) failure = failure ?? "the agent session was stopped; send again to start a fresh one";
            } else if (runId === null && !stopping && ev.outcome !== "answered") failure = failure ?? `turn ended: ${String(ev.outcome)}`;
            // A workflow step attempt ends with its own `done`; only the run state ends a workflow.
            if (runId === null) finished = true;
            continue;
          }
          if (type === "tool_result") {
            const callId = String(ev.call_id);
            inFlight.delete(callId);
            const runTool = sidecarRunCalls.get(callId);
            if (runTool !== undefined) {
              sidecarRunCalls.delete(callId);
              // A run that happened is reported even while a stopped turn drains (US-2.2 AC3).
              callbacks.onActivity?.(commandRunOf(callId, runTool, String(ev.status), String(ev.content ?? "")));
            }
            const fileTool = sidecarFileCalls.get(callId);
            if (fileTool !== undefined) {
              sidecarFileCalls.delete(callId);
              // A change that happened is reported even while a stopped turn drains: it can be undone.
              const change = ev.status === "ok" ? parseFileChange(fileTool, ev.content) : null;
              if (change) callbacks.onActivity?.({ kind: "file_change", change });
            }
            continue;
          }
          if (type === "tool_call" && ev.host === "sidecar") {
            const c = ev.call as { id?: unknown; name?: unknown } | undefined;
            if (c && typeof c.id === "string" && typeof c.name === "string" && isFileTool(c.name)) sidecarFileCalls.set(c.id, c.name);
            if (c && typeof c.id === "string" && typeof c.name === "string" && COMMAND_RUN_TOOLS.has(c.name)) sidecarRunCalls.set(c.id, c.name);
            if (!stopping && c && typeof c.id === "string" && c.name === SHELL_RUN_TOOL) {
              const cid = c.id;
              callbacks.onStatus("tool");
              const answered = fresh.some((e) => e.event.type === "tool_result" && e.event.call_id === cid);
              await decideShell(cid, answered);
              continue;
            }
            if (!stopping && c && typeof c.id === "string" && typeof c.name === "string" && c.name.startsWith(MCP_TOOL_PREFIX)) {
              const cid = c.id;
              callbacks.onStatus("tool");
              const answered = fresh.some((e) => e.event.type === "tool_result" && e.event.call_id === cid);
              await decideMcp(cid, answered);
              continue;
            }
          }
          if (stopping) continue; // draining a stopped turn: nothing else is acted on
          if (type === "assistant_delta") {
            // HUP-S1.1 (g1-render): the model's text as it is written (workflow runs show verdicts).
            if (runId === null && streamDeltas) {
              const t = String(ev.text ?? "");
              // Text that led into an earlier step's tool calls stays; a new step starts a paragraph.
              const lead = stepText === "" && shownText ? "\n\n" : "";
              stepText += t;
              show(lead + t);
            }
          } else if (type === "step_start") {
            stepText = "";
            streamDeltas = true;
            callbacks.onStatus("thinking");
            const step = Number(ev.step);
            if (Number.isFinite(step)) callbacks.onActivity?.({ kind: "step", step });
          } else if (type === "usage") {
            // HUP-S7.6 (US-7.4 AC1): the model server's own usage for one model call.
            const usage = usageEventOf(ev);
            if (usage) callbacks.onActivity?.(usage);
          } else if (type === "plan") {
            // HUP-S7.6: a workflow run's step ids, once before its first step.
            const steps = Array.isArray(ev.steps) ? ev.steps.filter((x): x is string => typeof x === "string") : [];
            callbacks.onActivity?.({ kind: "plan", steps });
          } else if (type === "verifier") {
            // HUP-S1.3 / S3.3: one verifier's verdict on a workflow step attempt.
            callbacks.onActivity?.({
              kind: "verifier",
              step: String(ev.step ?? ""),
              name: String(ev.name ?? ""),
              passed: ev.passed === true,
              detail: String(ev.detail ?? ""),
            });
          } else if (type === "tool_call") {
            callbacks.onStatus("tool");
            const call = ev.call as ToolCall;
            // Only calls the loop dispatched to core carry host "core"; refused ones carry null.
            // A resumed view never runs a call the loop already has an answer for (it timed out
            // waiting while the view was gone).
            const answered = resumed && fresh.some((e) => e.event.type === "tool_result" && e.event.call_id === call.id);
            if (ev.host === "core" && !inFlight.has(call.id) && !answered) {
              inFlight.add(call.id);
              // HUP-S1.1: saved before it runs, so a reload while it runs is reported, never re-run.
              started.add(call.id);
              persist(seq);
              const args = objectArguments(call.arguments);
              let result: string;
              let status: "ok" | "denied" | "error";
              if (args === null) {
                result = NON_OBJECT_ARGUMENTS;
                status = "error";
              } else {
                try {
                  // HUP-S2.4: a call the loop marked hic:"required" reaches the store's handler
                  // with that mark, and the handler holds it for an explicit member decision.
                  const meta: ToolCallMeta | undefined =
                    ev.hic === "required"
                      ? { hic: "required", hicReason: typeof ev.hic_reason === "string" ? ev.hic_reason : undefined }
                      : undefined;
                  const normalized = { ...call, arguments: args };
                  result = await untilStopped(meta ? callbacks.onToolCall(normalized, meta) : callbacks.onToolCall(normalized), signal);
                  status = statusOf(result);
                } catch (e) {
                  if (e instanceof TurnStopped) {
                    // The stop route already released this call; no result is posted for it.
                    stopOnce();
                    continue;
                  }
                  result = e instanceof Error ? e.message : String(e);
                  status = "error";
                }
              }
              if (stopping) {
                started.delete(call.id);
                persist(seq);
                continue;
              }
              try {
                await api.toolResult(id, call.id, status, result);
              } catch (e) {
                // 409: the loop stopped waiting for this call (it timed out) and went on without it.
                if (!/returned 409\b/.test(e instanceof Error ? e.message : String(e))) throw e;
                callbacks.onActivity?.({
                  kind: "notice",
                  text: `Hermes stopped waiting for ${call.name} before its result arrived, so the result was not used.`,
                });
              }
              started.delete(call.id);
              persist(seq);
            }
          } else if (type === "final") {
            final = String(ev.content ?? "");
            if (runId === null) {
              // Show only what the streamed text has not shown yet (the whole answer when nothing
              // was streamed), so an answer never appears twice.
              const rest =
                stepText !== "" && final.startsWith(stepText) ? final.slice(stepText.length) : (shownText ? "\n\n" : "") + final;
              callbacks.onStatus("streaming");
              if (rest !== "" || !shownText) callbacks.onToken(rest);
              if (rest !== "") shownText = true;
              stepText = "";
            }
          } else if (type === "error") failure = String(ev.message ?? "the agent failed");
        }
        persist();
        if (finished) return { final, failure, stopping, view: null };
        if (runId !== null && !page.busy && api.workflowStatus) {
          // The session is idle: the run has a verdict unless it has not started yet.
          const view = await api.workflowStatus(id, runId);
          if (view.state !== "running") {
            if (stopping) abandon();
            return { final, failure, stopping, view };
          }
        }
        if (fresh.length === 0) {
          idle = page.busy ? 0 : idle + 1;
          if (idle >= MAX_IDLE_POLLS) {
            if (stopping) {
              // The stop route was called on this session, so it is not reused (see `done` above).
              abandon();
              return { final, failure, stopping, view: null };
            }
            throw new Error(runId === null ? "the agent session stopped responding" : "the workflow run stopped responding");
          }
        } else idle = 0;
      }
    } finally {
      signal?.removeEventListener("abort", stopOnce);
      // Close it in the sidecar too, so stopped sessions never fill the sidecar's table.
      if (abandoned && sessionId !== id) api.close?.(id).catch(() => undefined);
    }
  }

  async function runTurn(opts: SendOpts): Promise<{ role: string; content: string }> {
    const { callbacks, signal } = opts;
    const last = [...opts.messages].reverse().find((m) => m.role === "user");
    const text = last?.content ?? "";
    if (signal?.aborted) throw new TurnStopped();
    callbacks.onStatus("thinking");
    // A finished (or failed) turn leaves no core call legitimately waiting, so an id left over
    // from an earlier turn must not block a new call with the same id. Replays are still dropped
    // by seq below.
    inFlight.clear();
    started.clear();
    sidecarFileCalls.clear();
    sidecarRunCalls.clear();
    const reused = sessionId !== null;
    let id = await ensureSession();
    // Start from the session's current end: a turn sent from elsewhere (the CLI, an MCP client)
    // while this view was idle is not this turn. A session opened just now has nothing to skip.
    if (reused) {
      lastSeq = await catchUp(id, callbacks);
      if (sessionId === null) id = await ensureSession();
    }
    try {
      await api.send(id, text);
    } catch (e) {
      if (!isSessionGone(e)) throw e;
      // HUP-S1.1: Hermes restarted since the last turn. Say so, and continue in a new session.
      dropSession(id);
      callbacks.onActivity?.({ kind: "notice", text: SESSION_GONE_NOTICE });
      id = await ensureSession();
      await api.send(id, text);
    }
    persist();
    const { final, failure, stopping } = await drive(id, opts, null);
    if (stopping) throw new TurnStopped();
    if (failure) {
      callbacks.onStatus("error");
      throw new Error(failure);
    }
    callbacks.onStatus("done");
    return { role: "assistant", content: final };
  }

  /**
   * The session's current last sequence number (no wait). A session the sidecar no longer has is
   * forgotten (the caller then opens a new one); any other failure keeps the position as it is.
   */
  async function catchUp(id: string, callbacks: SendOpts["callbacks"]): Promise<number> {
    if (!api.position) return lastSeq;
    try {
      return Math.max(lastSeq, await api.position(id));
    } catch (e) {
      if (isSessionGone(e)) {
        dropSession(id);
        callbacks.onActivity?.({ kind: "notice", text: SESSION_GONE_NOTICE });
        return 0;
      }
      return lastSeq;
    }
  }

  /**
   * HUP-S1.1 (US-1.1 AC3): pick the saved session back up after the view reloaded. Events are read
   * from the saved sequence number, so nothing is shown or run twice and nothing is skipped. Core
   * calls this view had started and not answered are closed with an honest "interrupted" result
   * (their outcome is unknown, so they are never re-run); calls the loop announced after the
   * saved point run now through the same gated handler as always.
   */
  async function reattachSaved(opts: WorkflowRunOpts): Promise<ReattachResult> {
    const saved = store?.load() ?? null;
    if (!saved) return { kind: "none" };
    let page: Awaited<ReturnType<SidecarSessionApi["events"]>>;
    try {
      page = await api.events(saved.id, saved.lastSeq, 0);
    } catch (e) {
      if (isSessionGone(e)) {
        store?.save(null);
        return { kind: "gone", notice: SESSION_GONE_NOTICE };
      }
      return { kind: "unavailable", reason: e instanceof Error ? e.message : String(e) };
    }
    if (page.lastSeq < saved.lastSeq) {
      // A session with fewer events than this view saw is not the same session.
      store?.save(null);
      return { kind: "gone", notice: SESSION_GONE_NOTICE };
    }
    sessionId = saved.id;
    lastSeq = saved.lastSeq;
    started.clear();
    inFlight.clear();
    // Calls this view had started are never run again, whatever the event log shows.
    for (const c of saved.inFlight) inFlight.add(c);
    sidecarFileCalls.clear();
    sidecarRunCalls.clear();
    const fresh = page.events.filter((e) => e.seq > saved.lastSeq);
    // The sidecar keeps a bounded event log: if events after the saved point were dropped while
    // the view was away, say so instead of skipping them silently.
    const lost = fresh.length > 0 ? fresh[0].seq - saved.lastSeq - 1 : 0;
    if (lost > 0) {
      opts.callbacks.onActivity?.({
        kind: "notice",
        text: `${lost === 1 ? "1 event" : `${lost} events`} from Hermes while the app was away ${lost === 1 ? "is" : "are"} no longer kept, so part of that activity cannot be shown.`,
      });
    }
    let interrupted = 0;
    for (const callId of saved.inFlight) {
      if (fresh.some((e) => e.event.type === "tool_result" && e.event.call_id === callId)) continue;
      try {
        await api.toolResult(saved.id, callId, "error", INTERRUPTED_RESULT);
        interrupted += 1;
      } catch {
        // Nothing waits for it any more (the loop timed out or the turn ended): nothing to close.
      }
    }
    if (interrupted > 0) {
      opts.callbacks.onActivity?.({
        kind: "notice",
        text: `The app reloaded while Hermes was waiting on ${interrupted === 1 ? "a tool call" : `${interrupted} tool calls`}. ${interrupted === 1 ? "It was" : "They were"} reported to Hermes as interrupted, with an unknown outcome.`,
      });
    }
    persist();
    if (!page.busy && fresh.length === 0 && interrupted === 0) return { kind: "idle", sessionId: saved.id };
    const id = saved.id;
    const work = previous.catch(() => undefined).then(async () => {
      opts.callbacks.onStatus("thinking");
      return drive(id, opts, null, true);
    });
    previous = work;
    const { final, failure, stopping } = await untilStopped(work, opts.signal);
    if (stopping) throw new TurnStopped();
    opts.callbacks.onStatus(failure ? "error" : "done");
    return { kind: "resumed", sessionId: id, content: final, interrupted, failure };
  }

  /** HUP-S3.3 (US-3.3 AC2): run a track's catalog workflow in this provider's session. */
  async function runTrackWorkflow(workflowId: string, opts: WorkflowRunOpts): Promise<WorkflowRunView> {
    const { callbacks, signal } = opts;
    if (!api.trackWorkflowRun || !api.workflowStatus) throw new Error(WORKFLOWS_NEED_SIDECAR);
    if (signal?.aborted) throw new TurnStopped();
    callbacks.onStatus("thinking");
    inFlight.clear();
    started.clear();
    sidecarFileCalls.clear();
    sidecarRunCalls.clear();
    const start = api.trackWorkflowRun.bind(api);
    const {
      id,
      value: { run_id },
    } = await onSession((sid) => start(sid, workflowId));
    const { failure, stopping, view } = await drive(id, opts, run_id);
    if (stopping) throw new TurnStopped();
    if (!view) {
      callbacks.onStatus("error");
      throw new Error(failure ?? "the workflow run ended without a verdict");
    }
    callbacks.onStatus("done");
    return view;
  }

  return {
    kind: "sidecar",
    label: "local model · Hermes sidecar · agentic",
    send(opts: SendOpts) {
      const work = previous.catch(() => undefined).then(() => runTurn(opts));
      previous = work;
      // The caller hears about a Stop at once; the drain above finishes in the background.
      return untilStopped(work, opts.signal);
    },
    runWorkflow(workflowId: string, opts: WorkflowRunOpts) {
      // Runs after (never alongside) the turn or run before it, in the same session.
      const work = previous.catch(() => undefined).then(() => runTrackWorkflow(workflowId, opts));
      previous = work;
      return untilStopped(work, opts.signal);
    },
    reattach(opts: WorkflowRunOpts) {
      return reattachSaved(opts);
    },
  };
}
