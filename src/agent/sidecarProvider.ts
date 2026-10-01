// =====================================================================
// HUP-S1.1c — the sidecar chat provider (ADR loop-in-sidecar: brain in the sidecar, hands in core).
//
// The loop runs in the Hermes sidecar. This provider is a VIEW over it: it opens one session, sends
// each user turn, long-polls the session's events, and for every core-hosted tool call runs the
// store's gated handler (`onToolCall` → `store.handleTool`, i.e. the same approval gates as today)
// and posts the result back. It never talks to the model and never decides an approval.
// =====================================================================
import { TurnStopped, untilStopped, type ChatProvider, type SendOpts, type ToolCall, type ToolCallMeta } from "./harness";

/** The session calls this provider needs (bridge.agentHarness in the app; a fake in tests). */
export interface SidecarSessionApi {
  open(systemPrompt: string, toolsJson: string): Promise<string>;
  send(id: string, text: string): Promise<void>;
  events(id: string, after: number, waitMs: number): Promise<{ events: { seq: number; event: Record<string, unknown> }[]; lastSeq: number; busy: boolean }>;
  toolResult(id: string, callId: string, status: "ok" | "denied" | "error", content: string): Promise<void>;
  stop(id: string): Promise<void>;
}

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
): ChatProvider {
  let sessionId: string | null = null;
  let lastSeq = 0;
  // Replay safety: core calls already run whose result the loop has not yet recorded (a
  // `tool_result` event for that id). A second announcement of one of these is ignored. Ids leave
  // the set once recorded, because the sidecar may reuse ids (call_0, call_1, …) on later steps.
  const inFlight = new Set<string>();

  // HUP-S7.6: turns run one at a time. A stopped turn keeps draining its session events (to its
  // `done`) after the caller has moved on, so the next turn starts from the right sequence number
  // and never sees the stopped turn's events.
  let previous: Promise<unknown> = Promise.resolve();

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
    if (!sessionId) {
      sessionId = await api.open(systemPrompt(), JSON.stringify(tools()));
      lastSeq = 0;
    }
    const id = sessionId;
    await api.send(id, text);

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
        if (fresh.length === 0) {
          idle = page.busy ? 0 : idle + 1;
          if (idle >= MAX_IDLE_POLLS) {
            if (stopping) {
              // The stop route was called on this session, so it is not reused (see `done` below).
              if (sessionId === id) sessionId = null;
              break;
            }
            throw new Error("the agent session stopped responding");
          }
          continue;
        }
        idle = 0;
        let finished = false;
        for (const { event: ev } of fresh) {
          const type = String(ev.type);
          if (type === "done") {
            finished = true;
            if (ev.outcome === "stopped") {
              // A session's stop switch stays on, so every later turn in it would end at once with
              // an empty answer. Leave it; the next turn opens a fresh session.
              if (sessionId === id) sessionId = null;
              if (!stopping) failure = failure ?? "the agent session was stopped; send again to start a fresh one";
            } else if (!stopping && ev.outcome !== "answered") failure = failure ?? `turn ended: ${String(ev.outcome)}`;
            continue;
          }
          if (type === "tool_result") {
            inFlight.delete(String(ev.call_id));
            continue;
          }
          if (stopping) continue; // draining a stopped turn: nothing else is acted on
          if (type === "step_start") {
            callbacks.onStatus("thinking");
            const step = Number(ev.step);
            if (Number.isFinite(step)) callbacks.onActivity?.({ kind: "step", step });
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
            callbacks.onStatus("streaming");
            callbacks.onToken(final);
          } else if (type === "error") failure = String(ev.message ?? "the agent failed");
        }
        if (finished) break;
      }
      if (stopping) throw new TurnStopped();
      if (failure) {
        callbacks.onStatus("error");
        throw new Error(failure);
      }
      callbacks.onStatus("done");
      return { role: "assistant", content: final };
    } finally {
      signal?.removeEventListener("abort", stopOnce);
    }
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
  };
}
