// =====================================================================
// HUP-S1.1c — the sidecar chat provider (ADR loop-in-sidecar: brain in the sidecar, hands in core).
//
// The loop runs in the Hermes sidecar. This provider is a VIEW over it: it opens one session, sends
// each user turn, long-polls the session's events, and for every core-hosted tool call runs the
// store's gated handler (`onToolCall` → `store.handleTool`, i.e. the same approval gates as today)
// and posts the result back. It never talks to the model and never decides an approval.
// =====================================================================
import { parseFileChange, isFileTool } from "./fileChanges";
import { TurnStopped, untilStopped, type ChatProvider, type SendOpts, type ToolCall, type ToolCallMeta, type TurnActivityEvent, type WorkflowRunOpts } from "./harness";
import type { SessionPersonaChoice, ShellPendingView } from "../bridge/domains";
import type { WorkflowRunView } from "./learn";

/** The session calls this provider needs (bridge.agentHarness in the app; a fake in tests). */
export interface SidecarSessionApi {
  /** `persona` is passed only when the member chose one (HUP-S3.3). */
  open(systemPrompt: string, toolsJson: string, persona?: SessionPersonaChoice): Promise<string>;
  send(id: string, text: string): Promise<void>;
  events(id: string, after: number, waitMs: number): Promise<{ events: { seq: number; event: Record<string, unknown> }[]; lastSeq: number; busy: boolean }>;
  toolResult(id: string, callId: string, status: "ok" | "denied" | "error", content: string): Promise<void>;
  stop(id: string): Promise<void>;
  /** Close a session the provider no longer uses (absent in older fakes: then nothing is closed). */
  close?(id: string): Promise<void>;
  /** HUP-S3.3 — start a track's catalog workflow in the session (absent = workflows unavailable). */
  trackWorkflowRun?(id: string, workflowId: string): Promise<{ run_id: string }>;
  /** HUP-S3.3 — a workflow run's state (`running` until the sidecar's verifiers have decided). */
  workflowStatus?(id: string, runId: string): Promise<WorkflowRunView>;
  /** HUP-S2.2 — the shell_run commands the sidecar holds for the member (absent = not supported). */
  shellPending?(id: string): Promise<ShellPendingView[]>;
  /** HUP-S2.2 — the member's decision, bound to the exact argv and folder shown. */
  shellDecide?(id: string, approvalId: string, allow: boolean, argv: string[], cwd: string): Promise<void>;
}

/** HUP-S2.2 — the sidecar-hosted tool whose every call the member decides. */
export const SHELL_RUN_TOOL = "shell_run";

/** HUP-S2.2 (US-2.2 AC3) — sidecar tools whose results are command runs for the Activity log. */
const COMMAND_RUN_TOOLS = new Set([SHELL_RUN_TOOL, "forge_test", "slither_scan", "aderyn_scan", "medusa_fuzz"]);

/** Timing for finding a held shell_run command (tests shorten it). */
export interface SidecarProviderOptions {
  /** Pause between looks for the held command (ms). */
  shellPollMs?: number;
  /** How long to look before leaving the decision to the sidecar's own timeout (ms). */
  shellWaitMs?: number;
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
  let sessionId: string | null = null;
  let lastSeq = 0;
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

  // A session whose stop switch is on is never reused; close it in the sidecar so repeated Stops
  // cannot fill the sidecar's session table.
  const leave = (id: string) => {
    if (sessionId === id) sessionId = null;
    api.close?.(id).catch(() => undefined);
  };

  async function ensureSession(): Promise<string> {
    if (!sessionId) {
      const p = persona();
      sessionId = p ? await api.open(systemPrompt(), JSON.stringify(tools()), p) : await api.open(systemPrompt(), JSON.stringify(tools()));
      lastSeq = 0;
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
  ): Promise<{ final: string; failure: string | null; stopping: boolean; view: WorkflowRunView | null }> {
    const { callbacks, signal } = opts;
    // HUP-S7.6: Stop goes through the session's own stop route (it ends the turn and releases a
    // waiting tool); from then on this turn only drains its events to `done` and runs nothing.
    let stopping = false;
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
      } catch (e) {
        callbacks.onActivity?.({
          kind: "notice",
          text: `Your decision on the command ${pending.argv[0]} did not reach Hermes (${e instanceof Error ? e.message : String(e)}). It was not run.`,
        });
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
        for (const { event: ev } of fresh) {
          const type = String(ev.type);
          if (type === "done") {
            if (ev.outcome === "stopped") {
              // A session's stop switch stays on, so every later turn in it would end at once with
              // an empty answer. Leave it; the next turn opens a fresh session.
              leave(id);
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
          }
          if (stopping) continue; // draining a stopped turn: nothing else is acted on
          if (type === "step_start") {
            callbacks.onStatus("thinking");
            const step = Number(ev.step);
            if (Number.isFinite(step)) callbacks.onActivity?.({ kind: "step", step });
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
            if (ev.host === "core" && !inFlight.has(call.id)) {
              inFlight.add(call.id);
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
              if (stopping) continue;
              await api.toolResult(id, call.id, status, result);
            }
          } else if (type === "final") {
            final = String(ev.content ?? "");
            if (runId === null) {
              callbacks.onStatus("streaming");
              callbacks.onToken(final);
            }
          } else if (type === "error") failure = String(ev.message ?? "the agent failed");
        }
        if (finished) return { final, failure, stopping, view: null };
        if (runId !== null && !page.busy && api.workflowStatus) {
          // The session is idle: the run has a verdict unless it has not started yet.
          const view = await api.workflowStatus(id, runId);
          if (view.state !== "running") {
            if (stopping) leave(id);
            return { final, failure, stopping, view };
          }
        }
        if (fresh.length === 0) {
          idle = page.busy ? 0 : idle + 1;
          if (idle >= MAX_IDLE_POLLS) {
            if (stopping) {
              // The stop route was called on this session, so it is not reused (see `done` above).
              leave(id);
              return { final, failure, stopping, view: null };
            }
            throw new Error(runId === null ? "the agent session stopped responding" : "the workflow run stopped responding");
          }
        } else idle = 0;
      }
    } finally {
      signal?.removeEventListener("abort", stopOnce);
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
    sidecarFileCalls.clear();
    sidecarRunCalls.clear();
    const id = await ensureSession();
    await api.send(id, text);
    const { final, failure, stopping } = await drive(id, opts, null);
    if (stopping) throw new TurnStopped();
    if (failure) {
      callbacks.onStatus("error");
      throw new Error(failure);
    }
    callbacks.onStatus("done");
    return { role: "assistant", content: final };
  }

  /** HUP-S3.3 (US-3.3 AC2): run a track's catalog workflow in this provider's session. */
  async function runTrackWorkflow(workflowId: string, opts: WorkflowRunOpts): Promise<WorkflowRunView> {
    const { callbacks, signal } = opts;
    if (!api.trackWorkflowRun || !api.workflowStatus) throw new Error(WORKFLOWS_NEED_SIDECAR);
    if (signal?.aborted) throw new TurnStopped();
    callbacks.onStatus("thinking");
    inFlight.clear();
    sidecarFileCalls.clear();
    sidecarRunCalls.clear();
    const id = await ensureSession();
    const { run_id } = await api.trackWorkflowRun(id, workflowId);
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
    label: "Hermes (sidecar loop · preview)",
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
  };
}
