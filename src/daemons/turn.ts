// =====================================================================
// citrate-core — one daemon turn (HUP-S10.3, US-10.3 AC2; HIC-3 in planset §4)
//
// A daemon run is a Hermes turn nobody is watching, so:
//   - it runs on the LOCAL model only, in the Hermes sidecar loop (HUP-S1.1: the webview has no
//     tool loop for the local model). A daemon's spend budget is 0, so it never uses a paid
//     gateway. While the sidecar is down, daemon runs wait and say why.
//   - it gets every agent tool except `app_navigate` (it never moves the member's screen). A daemon
//     installed from the Store is bound to the tools its approved permissions grant (the claim's
//     `allowedTools`, set in core at install): DENY BY DEFAULT, only those are offered and only
//     those run; an empty list is no tools at all.
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
import { createSidecarProvider, isSessionGone, type SidecarSessionApi } from "../agent/sidecarProvider";
import { AGENT_TOOL_ANNOTATIONS, annotationFor } from "../agent/toolAnnotations";
import type { TokenMeter } from "./tokenMeter";
import type { Claim } from "./api";

/** The tools a daemon turn is offered: every agent tool except moving the UI. */
export const DAEMON_TOOLS = AGENT_TOOLS.filter((t) => t.function.name !== "app_navigate");

/** The tools offered to one run: the member-daemon set, or exactly the approved ones. */
export function daemonToolsFor(allowed: readonly string[] | null | undefined) {
  if (allowed == null) return DAEMON_TOOLS;
  const ok = new Set(allowed);
  return DAEMON_TOOLS.filter((t) => ok.has(t.function.name));
}

/** Added to the system prompt of every daemon run. */
export const DAEMON_PREAMBLE =
  "\n\nThis is a SCHEDULED DAEMON RUN: the member set this task to run on a timer and is not watching. Do the task, then answer in a few short lines. Prefer reading over changing anything. Any change you propose (a journal entry, a memory, a group action, a transaction) waits for the member's explicit approval, so say what you proposed. Never claim something was done unless a tool result says so.";

export interface SidecarDaemonApi {
  /** `daemonId`: core re-checks a Store daemon against Hermes's current tools before opening;
   *  `runId`: the claimed run, which core checks is in flight for that daemon. */
  openUnattended(systemPrompt: string, toolsJson: string, daemonId?: string, runId?: string): Promise<string>;
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
  /** How long a stopped run waits for the sidecar to confirm its turn ended, and how often it
   *  asks (defaults: [`DAEMON_STOP_CONFIRM`]). */
  stopConfirm?: { deadlineMs: number; pollMs: number };
}

/** A stopped or failed run waits up to 30 s for the sidecar to report its session idle. */
export const DAEMON_STOP_CONFIRM = { deadlineMs: 30_000, pollMs: 250 };

/** A run whose sidecar session did not confirm it stopped (its stop or close failed, or its turn
 *  was still running at the deadline). The run is not over: it is not recorded as ended, and a
 *  Store Remove waiting on it is refused. */
export class DaemonNotTerminated extends Error {
  constructor(why: string) {
    super(`the daemon run's Hermes session did not confirm it stopped (${why}); it is kept until it does`);
    this.name = "DaemonNotTerminated";
  }
}

const errText = (e: unknown) => (e instanceof Error ? e.message : String(e));

/** Stop the session's turn, wait until the sidecar reports it idle, then close it. A session the
 *  sidecar no longer has (404) has nothing left to run. Throws [`DaemonNotTerminated`] otherwise. */
async function terminateSession(sc: SidecarDaemonApi, id: string, cfg: { deadlineMs: number; pollMs: number }): Promise<void> {
  try {
    await sc.stop(id);
  } catch (e) {
    if (isSessionGone(e)) return;
    throw new DaemonNotTerminated(`stop: ${errText(e)}`);
  }
  const end = Date.now() + cfg.deadlineMs;
  for (;;) {
    let busy: boolean;
    try {
      busy = (await sc.events(id, Number.MAX_SAFE_INTEGER, 0)).busy;
    } catch (e) {
      if (isSessionGone(e)) return;
      throw new DaemonNotTerminated(`status: ${errText(e)}`);
    }
    if (!busy) break;
    if (Date.now() >= end) throw new DaemonNotTerminated("its turn was still running");
    await new Promise((res) => setTimeout(res, cfg.pollMs));
  }
  try {
    await sc.close(id);
  } catch (e) {
    if (!isSessionGone(e)) throw new DaemonNotTerminated(`close: ${errText(e)}`);
  }
}

/** A daemon run core refused to open (an open-time check): recorded as `not_started`. */
export class DaemonNotStarted extends Error {
  constructor(message: string) {
    super(message);
    this.name = "DaemonNotStarted";
  }
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
  allowed?: readonly string[] | null,
): { run: false; result: string } | { run: true; meta: ToolCallMeta | undefined } {
  if (call.name === "app_navigate" || !annotationFor(call.name) || (allowed != null && !allowed.includes(call.name))) {
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

function annotatedDaemonTools(allowed: readonly string[] | null | undefined) {
  return daemonToolsFor(allowed).map((t) => ({ ...t, annotations: AGENT_TOOL_ANNOTATIONS[t.function.name] }));
}

/** Run one claimed daemon turn and return its reply. Rejects when stopped or when it fails. */
export async function runDaemonTurn(claim: Claim, signal: AbortSignal, meter: TokenMeter, deps: DaemonTurnDeps): Promise<string> {
  const allowed = claim.allowedTools ?? null;
  // K5a: a Store daemon gets the plain agent prompt with NO live app snapshot (balances, wallet,
  // node state); only what its approved tools return reaches it. A member daemon keeps the chat
  // prompt with its context, as before.
  const system = allowed != null ? AGENT_SYSTEM_PROMPT + DAEMON_PREAMBLE : deps.systemPrompt() + DAEMON_PREAMBLE;
  const task = `Scheduled daemon "${claim.name}". Task:\n\n${claim.prompt}`;
  const toolsJson = JSON.stringify(daemonToolsFor(allowed));
  const opened: string[] = [];
  // An open core refused (an open-time check) means the run never started (DGX R5).
  let openRefused: string | null = null;
  const baseChars = system.length + toolsJson.length + (allowed != null ? 0 : JSON.stringify(deps.context()).length);
  let metered = false;
  let pendingOpen: Promise<string> | null = null;
  let provider: ChatProvider;
  if (deps.providerKind === "sidecar" && deps.sidecar) {
    const sc = deps.sidecar;
    provider = createSidecarProvider(
      {
        open: async (p, t) => {
          let id: string;
          try {
            pendingOpen = sc.openUnattended(p, t, claim.daemonId, claim.runId);
            id = await pendingOpen;
          } catch (e) {
            openRefused = e instanceof Error ? e.message : String(e);
            throw e;
          }
          opened.push(id);
          // Stopped or paused while the open was pending: nothing is sent in this session (the
          // run's end below stops and closes it, with confirmation).
          throwIfStopped(signal);
          // The meter starts at the first open: a run that never got a session charges nothing,
          // whatever ended it (a refusal, a stop, a pause).
          if (!metered) {
            metered = true;
            meter.begin(baseChars, task.length);
          }
          return id;
        },
        send: sc.send,
        events: sc.events,
        toolResult: sc.toolResult,
        stop: sc.stop,
      },
      () => system,
      () => annotatedDaemonTools(allowed),
    );
  } else if (deps.providerKind === "local") {
    throw new Error(DAEMON_NEEDS_HERMES);
  } else {
    throw new Error("daemons run only on the local model, which is not serving right now");
  }
  const messages = [{ role: "user", content: task }];
  let answered = false;
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
          const d = daemonToolDecision(call, claim.name, meta, allowed);
          const out = d.run ? await deps.handleTool(call, d.meta, signal) : d.result;
          meter.context(out.length);
          return out;
        },
      },
    });
    answered = true;
    return reply.content;
  } catch (e) {
    if (openRefused !== null) throw new DaemonNotStarted(openRefused);
    throw e;
  } finally {
    const sc = deps.sidecar;
    if (answered) {
      // The turn reported done: the session is idle, closing it is housekeeping.
      for (const id of opened) sc?.close(id).catch(() => undefined);
    } else if (sc) {
      // Stopped or failed: the provider gives up at once while the sidecar may still be running
      // the turn. The run ends only once each session confirms it stopped (an open still in
      // flight is waited for first); otherwise DaemonNotTerminated replaces the error.
      const late = await (pendingOpen as Promise<string> | null)?.catch(() => null);
      if (late && !opened.includes(late)) opened.push(late);
      for (const id of opened) await terminateSession(sc, id, deps.stopConfirm ?? DAEMON_STOP_CONFIRM);
    }
  }
}

/** The chat system prompt a daemon run starts from (the same one chat uses). */
export function chatSystemPrompt(context: unknown): string {
  return AGENT_SYSTEM_PROMPT + "\n\nLive app context (JSON snapshot at session start): " + JSON.stringify(context);
}
