// =====================================================================
// citrate-core — one daemon turn (HUP-S10.3, US-10.3 AC2; HIC-3 in planset §4)
//
// A daemon run is a Hermes turn nobody is watching, so:
//   - it runs on the LOCAL model only, in the Hermes sidecar loop (HUP-S1.1: the webview has no
//     tool loop for the local model). A daemon's spend budget is 0, so it never uses a paid
//     gateway. While the sidecar is down, daemon runs wait and say why.
//   - it gets every agent tool except `app_navigate` (it never moves the member's screen).
//   - read-only tools run as in chat. EVERY other tool call is marked HIC-required, so it goes to
//     the member as an explicit decision (an approval card or the SignatureCeremony), with the
//     daemon's name in the reason. Nothing that changes state, and nothing that signs, happens
//     without that click. There is no budget path for these calls.
//   - on the sidecar loop the session is opened `unattended` (the sidecar marks every effectful
//     call HIC-required on its own as well) and is closed when the run ends.
// The run's tokens are measured from the model server's usage reports when every model call has
// one, otherwise estimated by the TokenMeter from characters (it says which).
// =====================================================================
import {
  AGENT_SYSTEM_PROMPT,
  AGENT_TOOLS,
  READ_ONLY_AGENT_TOOLS,
  throwIfStopped,
  type AgentContext,
  type ChatProvider,
  type ToolCall,
  type ToolCallMeta,
} from "../agent/harness";
import { createSidecarProvider, type SidecarSessionApi } from "../agent/sidecarProvider";
import { AGENT_TOOL_ANNOTATIONS, annotationFor } from "../agent/toolAnnotations";
import type { TokenMeter } from "./tokenMeter";
import type { Claim } from "./api";

/** The tools a daemon turn is offered: every agent tool except moving the UI. */
export const DAEMON_TOOLS = AGENT_TOOLS.filter((t) => t.function.name !== "app_navigate");

/** Added to the system prompt of every daemon run. */
export const DAEMON_PREAMBLE =
  "\n\nThis is a SCHEDULED DAEMON RUN: the member set this task to run on a timer and is not watching. Do the task, then answer in a few short lines. Prefer reading over changing anything. Any change you propose (a journal entry, a memory, a group action, a transaction) waits for the member's explicit approval, so say what you proposed. Never claim something was done unless a tool result says so.";

export interface SidecarDaemonApi {
  openUnattended(systemPrompt: string, toolsJson: string): Promise<string>;
  send: SidecarSessionApi["send"];
  events: SidecarSessionApi["events"];
  toolResult: SidecarSessionApi["toolResult"];
  stop: SidecarSessionApi["stop"];
  close(id: string): Promise<void>;
}

export interface DaemonTurnDeps {
  /** The chat provider's kind right now (`local` | `sidecar` run daemons; nothing else does). */
  providerKind: string | undefined;
  /** The chat system prompt with its live context (the daemon preamble is appended here). */
  systemPrompt(): string;
  context(): AgentContext;
  sidecar: SidecarDaemonApi | null;
  /** The store's gated tool handler (the same approval gates as chat). */
  /** `signal` is the run's: approval cards it raises close when the run ends. */
  handleTool(call: ToolCall, meta?: ToolCallMeta, signal?: AbortSignal): Promise<string>;
}

/** HUP-S1.1: why a daemon waits while the local model serves but the Hermes sidecar is down. */
export const DAEMON_NEEDS_HERMES = "daemons run in Hermes on the local model, and Hermes is not running right now";

/** Whether a daemon may run now, and why not. */
export function daemonAvailability(providerKind: string | undefined, mode: string): { ok: true } | { ok: false; why: string } {
  if (mode !== "tauri") return { ok: false, why: "daemons run only in the desktop app" };
  if (providerKind === "sidecar") return { ok: true };
  if (providerKind === "local") return { ok: false, why: DAEMON_NEEDS_HERMES };
  return { ok: false, why: "daemons run only on the local model, which is not serving right now (start it from Models)" };
}

/** What happens to one tool call a daemon turn makes. */
export function daemonToolDecision(
  call: ToolCall,
  daemonName: string,
  sidecarMeta?: ToolCallMeta,
): { run: false; result: string } | { run: true; meta: ToolCallMeta | undefined } {
  if (call.name === "app_navigate" || !annotationFor(call.name)) {
    return { run: false, result: `${call.name} is not available in a scheduled daemon run; nothing was done.` };
  }
  if (READ_ONLY_AGENT_TOOLS.has(call.name)) return { run: true, meta: sidecarMeta };
  return {
    run: true,
    meta: {
      hic: "required",
      hicReason: `the scheduled daemon "${daemonName}" proposed this while you were not watching; nothing changes unless you approve`,
    },
  };
}

function annotatedDaemonTools() {
  return DAEMON_TOOLS.map((t) => ({ ...t, annotations: AGENT_TOOL_ANNOTATIONS[t.function.name] }));
}

/** Run one claimed daemon turn and return its reply. Rejects when stopped or when it fails. */
export async function runDaemonTurn(claim: Claim, signal: AbortSignal, meter: TokenMeter, deps: DaemonTurnDeps): Promise<string> {
  const system = deps.systemPrompt() + DAEMON_PREAMBLE;
  const task = `Scheduled daemon "${claim.name}". Task:\n\n${claim.prompt}`;
  const toolsJson = JSON.stringify(DAEMON_TOOLS);
  const opened: string[] = [];
  let provider: ChatProvider;
  if (deps.providerKind === "sidecar" && deps.sidecar) {
    const sc = deps.sidecar;
    provider = createSidecarProvider(
      {
        open: async (p, t) => {
          const id = await sc.openUnattended(p, t);
          opened.push(id);
          return id;
        },
        send: sc.send,
        events: sc.events,
        toolResult: sc.toolResult,
        stop: sc.stop,
      },
      () => system,
      () => annotatedDaemonTools(),
    );
  } else if (deps.providerKind === "local") {
    throw new Error(DAEMON_NEEDS_HERMES);
  } else {
    throw new Error("daemons run only on the local model, which is not serving right now");
  }
  meter.begin(system.length + toolsJson.length + JSON.stringify(deps.context()).length, task.length);
  const messages = [{ role: "user", content: task }];
  try {
    const reply = await provider.send({
      messages,
      signal,
      callbacks: {
        onStatus: () => undefined,
        onToken: (t) => meter.output(t.length),
        onActivity: (ev) => {
          if (ev.kind === "step") meter.round();
          else if (ev.kind === "usage") meter.measured(ev.promptTokens, ev.completionTokens);
        },
        onToolCall: async (call, meta) => {
          throwIfStopped(signal);
          meter.context(call.arguments.length);
          const d = daemonToolDecision(call, claim.name, meta);
          const out = d.run ? await deps.handleTool(call, d.meta, signal) : d.result;
          meter.context(out.length);
          return out;
        },
      },
    });
    return reply.content;
  } finally {
    for (const id of opened) deps.sidecar?.close(id).catch(() => undefined);
  }
}

/** The chat system prompt a daemon run starts from (the same one chat uses). */
export function chatSystemPrompt(context: unknown): string {
  return AGENT_SYSTEM_PROMPT + "\n\nLive app context (JSON snapshot at session start): " + JSON.stringify(context);
}
