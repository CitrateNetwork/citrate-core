// =====================================================================
// HUP-S1.1c — the sidecar chat provider (ADR loop-in-sidecar: brain in the sidecar, hands in core).
//
// The loop runs in the Hermes sidecar. This provider is a VIEW over it: it opens one session, sends
// each user turn, long-polls the session's events, and for every core-hosted tool call runs the
// store's gated handler (`onToolCall` → `store.handleTool`, i.e. the same approval gates as today)
// and posts the result back. It never talks to the model and never decides an approval.
// =====================================================================
import type { ChatProvider, SendOpts, ToolCall, ToolCallMeta } from "./harness";

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

export function createSidecarProvider(
  api: SidecarSessionApi,
  systemPrompt: () => string,
  tools: () => readonly unknown[],
): ChatProvider {
  let sessionId: string | null = null;
  let lastSeq = 0;

  return {
    kind: "sidecar",
    label: "Hermes (sidecar loop · preview)",
    async send(opts: SendOpts) {
      const { callbacks } = opts;
      const last = [...opts.messages].reverse().find((m) => m.role === "user");
      const text = last?.content ?? "";
      callbacks.onStatus("thinking");
      if (!sessionId) {
        sessionId = await api.open(systemPrompt(), JSON.stringify(tools()));
        lastSeq = 0;
      }
      const id = sessionId;
      await api.send(id, text);

      let final = "";
      let failure: string | null = null;
      let idle = 0;
      for (;;) {
        const page = await api.events(id, lastSeq, POLL_WAIT_MS);
        lastSeq = Math.max(lastSeq, page.lastSeq);
        if (page.events.length === 0) {
          idle = page.busy ? 0 : idle + 1;
          if (idle >= MAX_IDLE_POLLS) throw new Error("the agent session stopped responding");
          continue;
        }
        idle = 0;
        let finished = false;
        for (const { event: ev } of page.events) {
          const type = String(ev.type);
          if (type === "step_start") callbacks.onStatus("thinking");
          else if (type === "tool_call") {
            callbacks.onStatus("tool");
            const call = ev.call as ToolCall;
            if (ev.host === "core") {
              let result: string;
              let status: "ok" | "denied" | "error";
              try {
                // HUP-S2.4: a call the loop marked hic:"required" reaches the store's handler with
                // that mark, and the handler holds it for an explicit member decision.
                const meta: ToolCallMeta | undefined =
                  ev.hic === "required"
                    ? { hic: "required", hicReason: typeof ev.hic_reason === "string" ? ev.hic_reason : undefined }
                    : undefined;
                result = await (meta ? callbacks.onToolCall(call, meta) : callbacks.onToolCall(call));
                status = statusOf(result);
              } catch (e) {
                result = e instanceof Error ? e.message : String(e);
                status = "error";
              }
              await api.toolResult(id, call.id, status, result);
            }
          } else if (type === "final") {
            final = String(ev.content ?? "");
            callbacks.onStatus("streaming");
            callbacks.onToken(final);
          } else if (type === "error") failure = String(ev.message ?? "the agent failed");
          else if (type === "done") {
            finished = true;
            if (ev.outcome !== "answered" && ev.outcome !== "stopped") failure = failure ?? `turn ended: ${String(ev.outcome)}`;
          }
        }
        if (finished) break;
      }
      if (failure) {
        callbacks.onStatus("error");
        throw new Error(failure);
      }
      callbacks.onStatus("done");
      return { role: "assistant", content: final };
    },
  };
}
