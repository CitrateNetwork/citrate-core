// L0-WP07c: the Hermes turn engine, moved out of the Store class body into the L1 agent runtime
// lane: sending a message (sendChat, the route ask for a held message, the sidecar checks and
// session resume, the daemon turn), Stop / Steer now / Queue for after (Board 01), the tool
// dispatcher (handleTool) with its deploy-claim checks, and the FL and everyday tool APIs the
// dispatcher hands out. Each Store method of the same name is a one-line delegate to the function
// here, so every call site (`store.sendChat(...)`) is unchanged. The code is moved as it was:
// `this` became `store`, and the Store's private members are reached through
// storeInternals(store). The module-level turn helpers moved with it; store.ts re-exports them.
//
// Data sources (Rule 7): the chat provider the Store built (store.provider: the Hermes sidecar,
// the agent loop, the local model or the gateway), AppState (chatMsgs, chatStatus, chatQueue,
// chatRouteHold), the tool handlers' own sources (bridge.*, chain 40204 reads, the memory daemon,
// the journal) and the member's approvals (the Signature Ceremony, HIC cards). Unchanged.

import { AGENT_SYSTEM_PROMPT, ChatProvider, ToolCall, ToolCallMeta, TurnStopped, createAgentProvider } from "../../../agent/harness";
import type { AppState, CerSpec, ChatMsg } from "../../state";
import { type ApprovalCard, type HicRequirement, MCP_CALL_HIC_REASON, MCP_OPEN_URL_HIC_REASON, SHELL_RUN_HIC_REASON, cardForCall, chainCard, diffCard, fieldsCard, mcpRequestCard, shellRunCard } from "../../../agent/approvalCards";
import { BRIDGE_MODE } from "../../../bridge/mode";
import type { Brief, ChatConsent, FlRoundPlan, McpPendingView, ShellPendingView } from "../../../bridge/domains";
import { type ChatAttachment, MAX_ATTACHMENTS, contextPreamble } from "../../../agent/chatAttachments";
import type { Claim } from "../../../daemons/api";
import { DEFAULT_PROPOSAL, approveAndStartRound, formatPlanForAgent, proposalFromToolArgs } from "../../../fl/flRounds";
import { DELEGATE_TOOL_NAME, DELEGATION_STATUS_TOOL_NAME, delegationApproval, isDelegationDeclined, runDelegationStatusTool, runDelegationTool } from "../../../agent/cognition";
import { DEPLOY_CLAIM_CHIP, type DeployEvidence, type DeployLedger, type TurnToolOutput, checkDeployClaims, collectDeployEvidence, newDeployLedger, sidecarToolOrigin } from "../../../agent/deployClaims";
import { ENVELOPE_NEEDS_DESKTOP, approveEnvelope, envelopeOff } from "../../../fl/authority";
import { ESCALATE_TOOL_NAME, escalationApproval, isEscalationDeclined, runEscalationTool } from "../../../agent/escalation";
import { type EverydayInvoke, isEverydayTool, runEverydayTool } from "../../../agent/everydayTools";
import { FL_DATASET_PUBLISH_TOOL, isFlDatasetTool, runFlDatasetTool } from "../../../agent/flDatasetTools";
import { FL_ROUND_DRAFT_READ_TOOL, runFlRoundDraftRead } from "../../../agent/flRoundDraftTool";
import { FL_ROUND_OPEN_TOOL, runFlRoundOpen } from "../../../agent/flRoundOpenTool";
import { type FlCollectPlan, attestationCardRows, flCollect, planCardRows, runForAgent, sourceCheckForAgent, sourceLine } from "../../../fl/collect";
import { type FlEnvelopeApi, type FlEnvelopeStatus, isFlEnvelopeTool, runFlEnvelopeTool } from "../../../agent/flEnvelopeTools";
import { type FlNetworkState, type PayoutsGateStatus, readFlNetworkState } from "../../../fl/networkState";
import { type MarketView, machineFrom, readMarket } from "../../../fl/market";
import { type OpenBinding, type RoundDraft, loadRoundDraft } from "../../../fl/roundDraft";
import { RUN_USAGE, parseRunCommand } from "../../../agent/trackWorkflows";
import { type RunOutcome, type RunRecord, newRun, runDraft, runEnd, runModelTime, runThink, runToolEnd, runToolStart } from "../../../agent/runTimeline";
import type { TokenMeter } from "../../../daemons/tokenMeter";
import { WIDGET_QUERIES, isWidgetQuery } from "../../../widgets/catalog";
import { type WorkerTerms, loadWorkerTerms } from "../../../fl/workerTerms";
import { annotationFor } from "../../../agent/toolAnnotations";
import { approvalNoted, beginTurn, commandRan, endTurn, markStopping, notePhase, noteStep, planReported, toolFinished, toolStarted, usageReported, verifierReported } from "../../slices/turnActivity";
import { belnapCodecTool } from "../../../agent/belnap";
import { bridge } from "../../../bridge";
import { chatSystemPrompt, daemonAvailability, runDaemonTurn as runDaemonTurnWith } from "../../../daemons/turn";
import { contextKindOf, offTitles } from "../../../agent/hermesContext";
import { datasetGate, licensesForAgent, scanForAgent } from "../../../fl/datasetGate";
import { downReason, readSidecar, sameReason } from "../../../agent/sidecarLiveness";
import { fenceUntrusted } from "../../../agent/untrusted";
import flAddressBook from "../../../../src-tauri/addresses/40204.json";
import { formatJournalForAgent } from "../../../agent/journalRead";
import { formatMemoryHits, memorySearchBudget, memorySearchTarget } from "../../../agent/knowledgeSearch";
import { formatVerifiedSourceForAgent, isAddress } from "../../../agent/verifiedSource";
import { invoke } from "../../../bridge/tauri/invoke";
import { isFlAdapterTool, runFlAdapterTool } from "../../../agent/flAdapterTools";
import { isFlHistoryTool, runFlHistoryTool } from "../../../agent/flHistoryTools";
import { isFlMarketTool, runFlMarketTool } from "../../../agent/flMarketTools";
import { isFlPayoutTool, runFlPayoutTool } from "../../../agent/flPayoutTools";
import { isFlRoundTool, runFlRoundTool } from "../../../agent/flRoundTools";
import { isFlWorkTool, runFlWorkTool } from "../../../agent/flWorkTools";
import { isGroupClusterTool, runGroupClusterTool } from "../../../agent/groupClusterTools";
import { latestBlockTime } from "../../../fl/chainTime";
import { modelsSlice } from "../../slices/models";
import { recordFileChange, refreshUndoPanel } from "../../slices/agentUndo";
import { refreshWidgets, widgetsApi } from "../../../widgets/api";
import { routerChoicesFor } from "../../../agent/modelRouterSources";
import { runAddressBookLookup, runDecodeCalldata, runDecodeLog, runExplainTx, runGetCode, runIdentifyContract } from "../../../agent/contractIntel";
import { runContractView } from "../../../agent/contractView";
import { runPublishTool } from "../../../fl/datasetPublishTool";
import { validatorWord } from "../../chainTruth";
import { withPersonaMessage } from "../../../agent/personas";
import type { LocalModelChoice, Store } from "../../store";
import type { FlAttestation } from "../../../fl/collect";
import { storeInternals } from "../../storeInternals";

/** Tools whose handler asks on its own one ceremony (a hic reason rides that card). Exported for the gate tripwires. */
export const SELF_GATED_TOOLS: ReadonlySet<string> = new Set(["memory_assert", "journal_append", "group_create", "group_invite", "skill_write", "contract_deploy", "fl_round_start", ESCALATE_TOOL_NAME, DELEGATE_TOOL_NAME, "widget_create", "gsheets_append", "schedule_add", "fl_adapter_load_propose", "fl_dataset_create", "fl_dataset_clean_propose", "fl_envelope_propose", "fl_join_propose", "fl_withdraw_propose", "fl_round_open_propose", "fl_collect_plan", "fl_dataset_publish_propose", "group_join_propose", "group_role_propose", "cluster_mesh_propose"]);

/**
 * SCL-S7.5a — how long a held chat message waits for the app's own startup start of the local
 * server (and for a member-chosen restart) before the chat asks or fails honestly. It matches the
 * llama-server cold-load grace (`LLAMA_START_GRACE`, 180 s, serve.rs).
 */
export const LOCAL_START_WAIT_MS = 180_000;
/** SCL-S7.5a — how often a held message re-reads the routing state while it waits. */
export const LOCAL_ROUTE_POLL_MS = 1_000;

/** SCL-S7.5a — the error shown on a held message the member cancelled (Retry asks again). */
export const LOCAL_ASK_NOT_SENT = "not sent; you cancelled before choosing where it goes";
/** K2: core could not mint the consent for this message, so nothing was sent to the gateway. */
export const GATEWAY_CONSENT_UNAVAILABLE = "not sent; the app could not confirm this message for the gateway. Try again";
/** K2: local only is on and there is no local model to start, so the message is not sent. */
export const LOCAL_ONLY_NOT_SENT =
  "Local only is on and there is no local model running, so this message was not sent. To chat, start the local model (Models), or turn off Local model only in Hermes › Controls.";

/** SCL-S7.5a — wait `ms`, or less if `signal` aborts first. */
function sleepOrAbort(ms: number, signal?: AbortSignal): Promise<void> {
  return new Promise<void>((resolve) => {
    if (signal?.aborted) return resolve();
    const t = setTimeout(resolve, ms);
    signal?.addEventListener(
      "abort",
      () => {
        clearTimeout(t);
        resolve();
      },
      { once: true },
    );
  });
}

/** D-11: the chat provider id core reads as "the member's preferred cloud model". */
export const PREFERRED_CHAT_ID = "preferred";

/** WP-H8: the member's license attestation from a tool call, when both parts are given. */
function attestationFrom(args: Record<string, unknown>): FlAttestation | null {
  const license = typeof args.license === "string" ? args.license.trim() : "";
  const statement = typeof args.licenseStatement === "string" ? args.licenseStatement.trim() : "";
  return license && statement ? { license, statement } : null;
}

/** HUP-S1.4 — keep a brief the sidecar accepted: a card in the thread + persisted state. This
 *  starts nothing; the card's button runs its workflow (HUP-S3.3, `runBriefWorkflow`). */
export function acceptBrief(store: Store, brief: Brief, markdown: string): void {
  const internals = storeInternals(store);
  const msg: ChatMsg = {
    id: "m" + ++internals.mid,
    who: "Brief",
    text: markdown,
    chips: [{ label: brief.workflow_available ? "brief saved · run its workflow from this card" : "brief saved · nothing is built until the workflow ships", status: "approved" }],
    streaming: false,
    brief,
  };
  store.setState((s) => ({ chatMsgs: s.chatMsgs.concat([msg]), hermesBrief: { brief, markdown, acceptedAt: Date.now() } }));
  store.save();
  store.scrollChat();
}

/** HUP-S3.3 — the sidecar session's system prompt: the base prompt and the live context only. The
 *  chosen persona travels as `sidecarPersonaChoice()`; the sidecar checks it and renders its
 *  fragment itself, so a fragment kept in app state never reaches the prompt (L-23). */
export function sidecarSystemPrompt(store: Store): string {
  return AGENT_SYSTEM_PROMPT + "\n\nLive app context (JSON snapshot at session start): " + JSON.stringify(store.snapshot());
}

/** HUP-S0.7 — retry a failed turn: drop the failed reply and its user message, then resend. */
export async function retryChat(store: Store, failedId: string): Promise<void> {
  const msgs = store.state.chatMsgs;
  const idx = msgs.findIndex((m) => m.id === failedId && m.error);
  if (idx < 0) return;
  const text = msgs[idx].retryText ?? "";
  const userIdx = idx - 1 >= 0 && msgs[idx - 1].who === "You" && msgs[idx - 1].text === text ? idx - 1 : -1;
  // The retried message keeps its attachments.
  const attachments = userIdx >= 0 ? msgs[userIdx].attachments : undefined;
  store.setState({ chatMsgs: msgs.filter((_, i) => i !== idx && i !== userIdx) });
  await store.sendChat(text, attachments?.length ? { attachments } : {});
}

/**
 * 0.5.4 — before a message goes out on the no-tools fallback, read Hermes again. Hermes may have
 * come up (or its status become readable) since the provider was built: then the provider is
 * rebuilt, so the message goes to Hermes and no stale "not running" notice is shown. With the
 * loop switched off nothing is read. Returns the provider to send with.
 */
export async function recheckSidecarBeforeSend(store: Store, provider: ChatProvider): Promise<ChatProvider> {
  const internals = storeInternals(store);
  const down = internals.sidecarDown;
  if (!down || down.why === "loop_off" || provider !== store.provider || BRIDGE_MODE !== "tauri") return provider;
  const now = await readSidecar(() => bridge.agentHarness.status());
  if (!now.up && sameReason(down, downReason(true, now))) return provider;
  await store.rebuildProvider();
  return store.provider ?? provider;
}

/**
 * HUP-S1.1 (US-1.1 AC3) — after a reload, pick the saved Hermes session back up: a turn that was
 * running (or finished while the view was gone) is shown to its end, core calls the loop asks for
 * run through the same gates, calls that were in progress at the reload are reported as
 * interrupted, and a session Hermes no longer has is let go with a plain notice.
 */
export async function resumeSidecarSession(store: Store, provider: ChatProvider): Promise<void> {
  const internals = storeInternals(store);
  if (!provider.reattach || store.state.chatStatus !== "ready") return;
  const ac = new AbortController();
  const msgId = "m" + ++internals.mid;
  let shown = false;
  const ensure = () => {
    if (shown) return;
    shown = true;
    store.setState((s) => ({ chatMsgs: s.chatMsgs.concat([{ id: msgId, who: "Agent", text: "", chips: [], streaming: true }]) }));
  };
  const patch = (fn: (m: ChatMsg) => ChatMsg) =>
    store.setState((s) => ({ chatMsgs: s.chatMsgs.map((m) => (m.id === msgId ? fn(m) : m)) }));
  let began = false;
  const begin = () => {
    if (began) return;
    began = true;
    internals.turnAbort = ac;
    beginTurn(provider.kind, `${provider.label} · resumed after reload`);
  };
  // #643: the resumed turn's tool results (what this view saw), for the final-answer check.
  const toolOutputs: TurnToolOutput[] = [];
  // #681: whether the sidecar's `node` MCP server is core's own node entry.
  const nodeIsCore = await internals.nodeMcpTrusted();
  try {
    const r = await provider.reattach({
      signal: ac.signal,
      callbacks: {
        onStatus: (st) => {
          if (ac.signal.aborted) return;
          begin();
          notePhase(st);
          if (st === "streaming") ensure();
          store.setState({ chatStatus: st === "done" || st === "error" ? "ready" : (st as AppState["chatStatus"]) });
        },
        onToken: (tk) => {
          if (ac.signal.aborted) return;
          ensure();
          patch((m) => ({ ...m, text: m.text + tk }));
          store.scrollChat();
        },
        onToolCall: async (call, meta) => {
          if (ac.signal.aborted) return "stopped by the member before this ran; nothing was done.";
          toolStarted(call.id, call.name);
          try {
            const result = await store.handleTool(call, msgId, ensure, meta);
            toolOutputs.push({ tool: call.name, content: result, origin: "core" });
            toolFinished(call.id, true);
            return result;
          } catch (e) {
            toolFinished(call.id, false);
            throw e;
          }
        },
        onCommandApproval: (p) => (ac.signal.aborted ? Promise.resolve(false) : store.approveShellRun(p)),
        onActivity: (ev) => {
          if (ev.kind === "file_change") {
            ensure();
            recordFileChange(ev.change, msgId);
            void refreshUndoPanel(bridge.agentHarness);
          } else if (ev.kind === "command_run") {
            commandRan(ev);
          } else if (ev.kind === "notice") {
            store.toast(ev.text);
          } else if (ev.kind === "tool_output") {
            toolOutputs.push({ tool: ev.tool, status: ev.status, content: ev.content, origin: sidecarToolOrigin(ev.tool, nodeIsCore) });
          } else if (ev.kind === "plan" && !ac.signal.aborted) {
            // HUP-S7.6: a resumed chat turn's plan, from the tool calls replayed after the saved point.
            planReported(ev.steps, ev.source ?? "workflow");
          } else if (ev.kind === "step" && !ac.signal.aborted) noteStep(ev.step);
        },
      },
    });
    if (r.kind === "gone") store.toast(r.notice);
    if (r.kind === "resumed") {
      ensure();
      if (r.failure) patch((m) => ({ ...m, error: r.failure ?? undefined }));
      endTurn(r.failure ? "failed" : "answered");
    }
  } catch (e) {
    if (began) {
      ensure();
      const stopped = e instanceof TurnStopped || ac.signal.aborted;
      patch((m) => ({ ...m, error: stopped ? "stopped by you" : e instanceof Error ? e.message : String(e) }));
      endTurn(stopped ? "stopped" : "failed");
    }
  }
  if (internals.turnAbort === ac) internals.turnAbort = null;
  if (shown) await internals.checkAnswerClaims(msgId, toolOutputs);
  if (shown) patch((m) => ({ ...m, streaming: false }));
  if (began) store.setState({ chatStatus: "ready" });
  store.scrollChat();
}

/** HUP-S7.6 — the active provider's kind (`ChatProvider.kind`), for the Activity monitor. */
export function activeProviderKind(store: Store): string {
  return store.provider?.kind ?? "demo";
}

/**
 * HUP-S7.6 — Stop the running agent turn (the Activity monitor's Stop button). The chat returns
 * to ready at once; the provider stops before its next model request or tool call (the sidecar
 * through its session stop route); tool calls the stopped turn asks for afterwards are not run.
 * Approvals already open stay open for the member to decide. A no-op when nothing is running.
 */
export function stopAgentTurn(store: Store): void {
  const internals = storeInternals(store);
  const ac = internals.turnAbort;
  if (!ac || ac.signal.aborted) return;
  markStopping();
  ac.abort();
}

/**
 * SCL-S7.5a (D-16 extended, US-6.2 AC5) — decide where one held message goes while the local
 * model is configured but its server is not running. In order:
 *  1. a fresh routing read: a local server that came back on its own is used directly;
 *  2. while the app's own startup start of the local server is pending, wait (bounded by
 *     `LOCAL_START_WAIT_MS`) and send locally once it is ready, without asking;
 *  3. otherwise ask the member: restart the local model, or send this message to the gateway
 *     this time. The gateway provider is built for this message only; the held route stays, so
 *     the next message asks again. Restart never falls back to the gateway.
 * Stop (the turn's abort signal) cancels at any point; nothing is sent. A consent core minted for
 * the question is dropped in core on every exit except the gateway send that carries it.
 */
export async function routeHeldMessage(store: Store, msgId: string, text: string, signal: AbortSignal): Promise<{ ok: true; provider: ChatProvider } | { ok: false; error: string }> {
  const internals = storeInternals(store);
  const hold = (phase: "waiting" | "ask" | "restarting") => store.setState({ chatRouteHold: { msgId, phase, options: internals.routeAsk } });
  const cancelled = { ok: false as const, error: LOCAL_ASK_NOT_SENT };
  const gone = { ok: false as const, error: "the local model is no longer set up, so this message was not sent" };
  const localNow = async (): Promise<ChatProvider | null> => {
    await store.rebuildProvider({ keepSession: true });
    return internals.routeKind === "local" && store.provider ? store.provider : null;
  };
  // K2: the consent core minted for this question, until it is handed to the gateway provider.
  let consent: ChatConsent | null = null;
  try {
    let local = await localNow();
    if (local) return { ok: true, provider: local };
    if (internals.routeKind !== "ask") return gone;
    // K2: local only with no local model to start: nothing to offer, so say why and send nothing.
    if (!internals.routeAsk.restart && !internals.routeAsk.gateway) return { ok: false, error: LOCAL_ONLY_NOT_SENT };
    if (internals.launchLocalStartPending && internals.routeAsk.restart) {
      hold("waiting");
      const deadline = Date.now() + LOCAL_START_WAIT_MS;
      while (internals.launchLocalStartPending && Date.now() < deadline && !signal.aborted) {
        await sleepOrAbort(LOCAL_ROUTE_POLL_MS, signal);
        if (signal.aborted) break;
        local = await localNow();
        if (local) return { ok: true, provider: local };
        if (internals.routeKind !== "ask") return gone;
      }
      if (signal.aborted) return cancelled;
      local = await localNow();
      if (local) return { ok: true, provider: local };
      if (internals.routeKind !== "ask") return gone;
    }
    // K2: core mints the one-time consent for this message when the question opens. Core sends
    // nothing to the gateway without it, so a failed mint offers no gateway send.
    if (internals.routeAsk.gateway) {
      consent = await bridge.chat.consentPrepare(internals.remoteChatId, text).catch(() => null);
    }
    hold("ask");
    const choice = await new Promise<LocalModelChoice>((resolve) => {
      internals.localModelChoice = resolve;
      if (signal.aborted) resolve("cancel");
      signal.addEventListener("abort", () => resolve("cancel"), { once: true });
    });
    internals.localModelChoice = null;
    if (choice === "cancel" || signal.aborted) return cancelled;
    if (choice === "gateway" && internals.routeAsk.gateway) {
      // The member's explicit choice, for this one message only (never offered under local only),
      // carried to core as the consent core minted for it.
      if (!consent) return { ok: false, error: GATEWAY_CONSENT_UNAVAILABLE };
      // 0.5.4 (owner, 2026-11-17): when the remote choice is the preferred cloud model and Hermes
      // is up, `rebuildProvider` built Hermes's cloud session for it. The message goes there, so
      // Hermes runs it with its own tools (shell_run, folder files, web, MCP) behind the same
      // approval cards. The app's own loop below has only the core-hosted tools. That session
      // sends through core's proxy without a consent, so the one minted here is dropped below.
      // Hermes may have stopped while the card was open: read it again, and if it is not up the
      // message goes to the app's own loop with the consent, as it would have without Hermes.
      if (internals.remoteChatId === PREFERRED_CHAT_ID && store.provider?.kind === "sidecar-cloud") {
        const sidecar = store.provider;
        if ((await readSidecar(() => bridge.agentHarness.status())).up) return { ok: true, provider: sidecar };
      }
      const def = consent.providerId;
      const consentId = consent.consentId;
      consent = null; // carried to core with the send, so not dropped below
      return {
        ok: true,
        provider: createAgentProvider(def, () => store.snapshot(), (pid, msgs, tools, ctx) =>
          bridge.chat.inferTools(pid, msgs, tools, ctx, consentId),
        ),
      };
    }
    // Defensive: a choice that was not offered sends nothing.
    if (choice === "gateway" || !internals.routeAsk.restart) return cancelled;
    hold("restarting");
    const started = await store.restartLocalModel(signal);
    if (signal.aborted) return cancelled;
    // `restartLocalModel` is true only once the routing state is local again.
    if (started && store.provider) return { ok: true, provider: store.provider };
    return { ok: false, error: "the local model did not start, and nothing was sent to the gateway" };
  } finally {
    internals.localModelChoice = null;
    store.setState({ chatRouteHold: null });
    // K2: closed without the gateway send (cancel, stop, the local model, an error): drop the
    // consent in core so it cannot be used later. Best effort: it expires in core regardless.
    if (consent) void bridge.chat.consentDrop(consent.consentId).catch(() => undefined);
  }
}

/** SCL-S7.5a — the member's answer to the question for the held message. A no-op unless the
 *  chat is asking right now. */
export function answerLocalModelAsk(store: Store, choice: LocalModelChoice): void {
  const internals = storeInternals(store);
  const resolve = internals.localModelChoice;
  const hold = store.state.chatRouteHold;
  if (!resolve || hold?.phase !== "ask") return;
  // K2: a choice the card does not offer (the gateway under local only, a restart with no local
  // model) does nothing.
  const opts = hold.options;
  if (opts && ((choice === "gateway" && !opts.gateway) || (choice === "restart" && !opts.restart))) return;
  internals.localModelChoice = null;
  resolve(choice);
}

/**
 * SCL-S7.5a — "Restart the local model": release the llama-server supervisor (it may be failed or
 * backing off, which `serveStart` would refuse as already running), start it again, then wait
 * (bounded) until the routing state is local. True only when chat now routes locally.
 */
export async function restartLocalModel(store: Store, signal?: AbortSignal): Promise<boolean> {
  const internals = storeInternals(store);
  if (BRIDGE_MODE !== "tauri") return false;
  try {
    await bridge.model.serveStop();
  } catch {
    /* nothing to stop: the start below decides */
  }
  try {
    await bridge.model.serveStart();
  } catch {
    await store.rebuildProvider();
    return false;
  }
  const deadline = Date.now() + LOCAL_START_WAIT_MS;
  for (;;) {
    await store.rebuildProvider();
    if (internals.routeKind === "local") return true;
    if (Date.now() >= deadline || signal?.aborted) return false;
    await sleepOrAbort(LOCAL_ROUTE_POLL_MS, signal);
  }
}

/** HUP-S10.3 — whether a daemon run may start now (the local model only: daemons spend 0). */
export function daemonRunAvailability(store: Store): { ok: true } | { ok: false; why: string } {
  return daemonAvailability(store.provider?.kind, BRIDGE_MODE);
}

/**
 * HUP-S10.3 — run one claimed daemon turn on the local model (src/daemons/turn.ts). Tool calls go
 * through `handleTool`'s gates; every effectful one is HIC-required (an explicit member decision).
 */
export function runDaemonTurn(store: Store, claim: Claim, signal: AbortSignal, meter: TokenMeter): Promise<string> {
  const internals = storeInternals(store);
  const h = bridge.agentHarness;
  return runDaemonTurnWith(claim, signal, meter, {
    providerKind: store.provider?.kind,
    systemPrompt: () => chatSystemPrompt(store.snapshot()),
    context: () => store.snapshot(),
    sidecar: {
      openUnattended: (p, t, daemonId, runId) => h.sessionOpenUnattended(p, t, daemonId, runId),
      send: (id, text) => h.sessionSend(id, text),
      events: (id, after, waitMs) => h.sessionEvents(id, after, waitMs),
      toolResult: (id, callId, status, content) => h.sessionToolResult(id, callId, status, content),
      stop: (id) => h.sessionStop(id),
      close: (id) => h.sessionClose(id),
    },
    handleTool: (call, meta, signal) => store.handleTool(call, "daemon-run", () => undefined, meta, signal),
    checkReply: (reply, outputs) => internals.checkDaemonReply(reply, outputs),
    nodeMcpTrusted: () => internals.nodeMcpTrusted(),
  });
}

export async function sendChat(store: Store, text: string, opts: { attachments?: ChatAttachment[] } = {}): Promise<void> {
  const internals = storeInternals(store);
  text = (text || "").trim();
  const attachments = (opts.attachments ?? []).slice(0, MAX_ATTACHMENTS);
  if (!text || store.state.chatStatus !== "ready") return;
  // HUP-S3.3: `/run <workflow>` runs a track workflow instead of a chat turn.
  const cmd = parseRunCommand(text);
  if (cmd.kind !== "none") {
    if (store.chatInputEl) store.chatInputEl.value = "";
    store.setState((s) => ({ chatMsgs: s.chatMsgs.concat([{ id: "m" + ++internals.mid, who: "You", text, chips: [], streaming: false }]) }));
    if (cmd.kind === "usage") {
      store.setState((s) => ({ chatMsgs: s.chatMsgs.concat([{ id: "m" + ++internals.mid, who: "Workflow", text: RUN_USAGE, chips: [], streaming: false }]) }));
      store.scrollChat();
      return;
    }
    return store.runTrackWorkflow(cmd.id);
  }
  if (!store.provider) return;
  let provider = store.provider;
  const ac = new AbortController();
  internals.turnAbort = ac;
  const stopped = () => ac.signal.aborted;
  const userMsg: ChatMsg = { id: "m" + ++internals.mid, who: "You", text, chips: [], streaming: false, ...(attachments.length ? { attachments } : {}) };
  store.setState((s) => ({ chatMsgs: s.chatMsgs.concat([userMsg]), chatStatus: "thinking" }));
  if (store.chatInputEl) store.chatInputEl.value = "";
  // 0.5.4: never send on the "Hermes is down" fallback from a stale read.
  if (internals.sidecarDown) provider = await internals.recheckSidecarBeforeSend(provider);
  // SCL-S7.5a (D-16 extended): the local model is configured but its server is not running.
  // Hold this message: wait for the app's own startup start, or ask the member. Nothing is sent
  // to the gateway unless the member chooses that for this message.
  if (internals.routeKind === "ask") {
    const routed = await internals.routeHeldMessage(userMsg.id, text, ac.signal);
    if (!routed.ok) {
      store.setState((s) => ({
        chatMsgs: s.chatMsgs.concat([
          { id: "m" + ++internals.mid, who: "Agent", text: "", chips: [], streaming: false, error: routed.error, retryText: text },
        ]),
        chatStatus: "ready",
      }));
      if (internals.turnAbort === ac) internals.turnAbort = null;
      store.scrollChat();
      store.save();
      // Board 01: a Steer now that stopped the held message goes next (it is never left stuck).
      internals.drainChatQueue("stopped");
      return;
    }
    provider = routed.provider;
  }
  beginTurn(provider.kind, provider.label);
  const asstId = "m" + ++internals.mid;
  // #643: this turn's tool results, for the final-answer check on deploy claims.
  const toolOutputs: TurnToolOutput[] = [];
  // #681: whether the sidecar's `node` MCP server is core's own node entry.
  const nodeIsCore = await internals.nodeMcpTrusted();
  // Board 01: the reply's run timeline. The model is named as the header showed it at the start.
  const turnModel = store.routerActive(routerChoicesFor(modelsSlice.get(), store.state.chatRoute)).label;
  let started = false;
  const ensure = () => {
    if (started) return;
    started = true;
    store.setState((s) => ({
      chatMsgs: s.chatMsgs.concat([{ id: asstId, who: "Agent", text: "", chips: [], streaming: true, run: newRun(Date.now()), model: turnModel }]),
    }));
  };
  const patch = (fn: (m: ChatMsg) => ChatMsg) =>
    store.setState((s) => ({ chatMsgs: s.chatMsgs.map((m) => (m.id === asstId ? fn(m) : m)) }));
  // Board 01: events of one provider page arrived together, at `clock`; core-run calls are timed
  // by core itself, so a core call clears the clock until the next page.
  let clock: number | null = null;
  const evNow = () => clock ?? Date.now();
  const runPatch = (fn: (r: RunRecord) => RunRecord) => {
    ensure();
    patch((m) => ({ ...m, run: fn(m.run ?? newRun(Date.now())) }));
  };
  ensure();
  let turnOutcome: RunOutcome = "failed";
  // HUP-S0.6: coalesce streamed tokens — one render per ~frame instead of one full-app render
  // per token (Root subscribes to the whole state).
  let pending = "";
  let flushTimer: ReturnType<typeof setTimeout> | null = null;
  const flush = () => {
    if (flushTimer !== null) {
      clearTimeout(flushTimer);
      flushTimer = null;
    }
    if (!pending) return;
    const add = pending;
    pending = "";
    patch((m) => ({ ...m, text: m.text + add }));
    store.scrollChat();
  };
  // HUP-S7.6: Stop resolves this at once, so a wedged model request never holds the chat.
  // A Stop that landed while the turn was still being set up (the #681 node read above) has
  // already fired its abort event, so it is read here too.
  const stopSignal = new Promise<"stopped">((resolve) => {
    if (ac.signal.aborted) resolve("stopped");
    else ac.signal.addEventListener("abort", () => resolve("stopped"), { once: true });
  });
  try {
    const run = provider.send({
      // HUP-S3.3: the chosen persona rides as one leading system message (none = unchanged).
      messages: withPersonaMessage(
        store.state.chatMsgs
          .filter((m) => !m.streaming && !m.error)
          .map((m) => ({ role: m.who === "You" ? "user" : "assistant", content: (m.attachments ? contextPreamble(m.attachments, []) : "") + m.text }))
          // #690: what the member attached, and what they turned off here, go in front of the message.
          .concat([{ role: "user", content: contextPreamble(attachments, offTitles(store.state.chatContextOff)) + text }]),
        store.state.hermesPersona,
      ),
      signal: ac.signal,
      callbacks: {
        onStatus: (st) => {
          // HUP-S7.6: a stopped turn's late callbacks change nothing.
          if (stopped()) return;
          notePhase(st);
          if (st === "streaming") ensure();
          if (st === "thinking") runPatch((r) => runThink(r, evNow()));
          else if (st === "streaming") runPatch((r) => runDraft(r, evNow()));
          store.setState({ chatStatus: st === "done" || st === "error" ? "ready" : (st as AppState["chatStatus"]) });
        },
        onToken: (tk) => {
          if (stopped()) return;
          ensure();
          // HUP-S0.4: keep the model's markdown intact (bold used to be stripped per token).
          pending += tk;
          if (flushTimer === null) flushTimer = setTimeout(flush, 32);
        },
        // #681: the sidecar corrected the answer it streamed; the corrected text replaces it.
        onRewrite: (streamed, replacement) => {
          if (stopped()) return;
          ensure();
          flush();
          patch((m) => ({
            ...m,
            text: streamed !== "" && m.text.endsWith(streamed) ? m.text.slice(0, m.text.length - streamed.length) + replacement : m.text + (m.text ? "\n\n" : "") + replacement,
          }));
          store.scrollChat();
        },
        onToolCall: async (call, meta) => {
          // HUP-S7.6: a call from a stopped turn is not run.
          if (stopped()) return "stopped by the member before this ran; nothing was done.";
          flush();
          toolStarted(call.id, call.name);
          clock = null;
          runPatch((r) => runToolStart(r, call.id, call.name, Date.now(), "core"));
          try {
            const result = await store.handleTool(call, asstId, ensure, meta);
            toolOutputs.push({ tool: call.name, content: result, origin: "core" });
            toolFinished(call.id, true);
            runPatch((r) => runToolEnd(r, call.id, call.name, store.takeCallOutcome(call.id), result, Date.now()));
            return result;
          } catch (e) {
            toolFinished(call.id, false);
            runPatch((r) => runToolEnd(r, call.id, call.name, "error", e instanceof Error ? e.message : String(e), Date.now()));
            throw e;
          }
        },
        // HUP-S2.2: every held shell_run is decided by the member on its card.
        onCommandApproval: (p) => (stopped() ? Promise.resolve(false) : store.approveShellRun(p)),
        onMcpApproval: (p) => (stopped() ? Promise.resolve(false) : store.approveMcpRequest(p)),
        onActivity: (ev) => {
          // Board 01: the run timeline, from the provider's own reports.
          if (ev.kind === "page") {
            clock = ev.at;
            return;
          }
          if (ev.kind === "tool_start") {
            if (!stopped()) runPatch((r) => runToolStart(r, ev.callId, ev.tool, evNow(), "events"));
            return;
          }
          if (ev.kind === "tool_end") {
            runPatch((r) => runToolEnd(r, ev.callId, ev.tool, ev.status, ev.content, evNow()));
            return;
          }
          if (ev.kind === "file_change") {
            // HUP-S2.9: a change that happened is shown with Undo even if the turn was stopped.
            ensure();
            recordFileChange(ev.change, asstId);
            void refreshUndoPanel(bridge.agentHarness);
            return;
          }
          // HUP-S2.2 (US-2.2 AC3): a command run that happened is logged even if the turn stopped.
          if (ev.kind === "command_run") {
            commandRan(ev);
            return;
          }
          if (ev.kind === "notice") {
            store.toast(ev.text);
            return;
          }
          // #681: the sidecar's own check corrected this answer: say so on the reply.
          if (ev.kind === "answer_check") {
            ensure();
            patch((m) => (m.chips.some((c) => c.label === DEPLOY_CLAIM_CHIP) ? m : { ...m, chips: m.chips.concat([{ label: DEPLOY_CLAIM_CHIP, status: "declined" }]) }));
            if (ev.findings.length) console.warn("#681 sidecar answer check:", ev.findings.join("; "));
            return;
          }
          // #643: a sidecar-hosted tool's result is evidence for the final-answer check.
          if (ev.kind === "tool_output") {
            toolOutputs.push({ tool: ev.tool, status: ev.status, content: ev.content, origin: sidecarToolOrigin(ev.tool, nodeIsCore) });
            runPatch((r) => runToolEnd(r, ev.callId, ev.tool, ev.status, ev.content, evNow()));
            return;
          }
          // HUP-S7.6 (US-7.4 AC1): usage, plan, approvals and verifier verdicts for the monitor.
          if (ev.kind === "approval") {
            approvalNoted(ev.callId, ev.tool, ev.state);
            return;
          }
          if (stopped()) return;
          if (ev.kind === "step") {
            noteStep(ev.step);
            runPatch((r) => runThink(r, evNow(), ev.step));
          } else if (ev.kind === "usage") {
            usageReported(ev);
            // The model server's own time for the call is the think step's duration.
            if (ev.generationMs !== null) runPatch((r) => runModelTime(r, (ev.promptMs ?? 0) + (ev.generationMs ?? 0)));
          }
          else if (ev.kind === "plan") planReported(ev.steps, ev.source ?? "workflow");
          else if (ev.kind === "verifier") verifierReported(ev);
        },
      },
    });
    // The stopped turn may still settle later; that settlement is discarded.
    run.catch(() => undefined);
    const winner = await Promise.race([run.then(() => "done" as const), stopSignal]);
    if (winner === "stopped") throw new TurnStopped();
    flush();
    // #643: no deploy claim is kept unless this turn's tools confirmed it (receipt + code).
    await internals.checkAnswerClaims(asstId, toolOutputs);
    endTurn("answered");
    turnOutcome = "answered";
    // HUP-S3.7: read the answer aloud with the persona's voice (off by default).
    internals.readAloud(store.state.chatMsgs.find((m) => m.id === asstId)?.text ?? "");
  } catch (e) {
    flush();
    ensure();
    // #643: a stopped or failed turn's partial text is checked the same way.
    await internals.checkAnswerClaims(asstId, toolOutputs);
    if (e instanceof TurnStopped || stopped()) {
      patch((m) => ({ ...m, error: "stopped by you", retryText: text }));
      endTurn("stopped");
      turnOutcome = "stopped";
    } else {
      // HUP-S0.7: a failed/timed-out turn stays visible with Retry (it used to vanish silently).
      console.error(e);
      const reason = e instanceof Error ? e.message : String(e);
      patch((m) => ({ ...m, error: reason || "the agent did not answer", retryText: text }));
      endTurn("failed");
    }
  }
  if (internals.turnAbort === ac) internals.turnAbort = null;
  patch((m) => ({ ...m, streaming: false, run: m.run ? runEnd(m.run, turnOutcome, Date.now()) : m.run }));
  store.setState({ chatStatus: "ready" });
  store.scrollChat();
  store.save();
  // Board 01: a follow-up typed during the run goes now (a steer, or the queue after a finish).
  internals.drainChatQueue(turnOutcome);
}

/** Take (and forget) the structured outcome of the call `id` ("ok" when none was recorded). */
export function takeCallOutcome(store: Store, id: string): "ok" | "denied" | "error" {
  const internals = storeInternals(store);
  const r = internals.callOutcomes.get(id) ?? "ok";
  internals.callOutcomes.delete(id);
  return r;
}

/** #690: switch a context kind off (or back on) for the chat on screen. */
export function setChatContext(store: Store, id: string, on: boolean): void {
  store.setState((s) => ({ chatContextOff: on ? s.chatContextOff.filter((x) => x !== id) : [...new Set([...s.chatContextOff, id])] }));
}

/** Whether the call `id` was refused (and forget its outcome). */
export function takeCallRefusal(store: Store, id: string): boolean {
  return store.takeCallOutcome(id) === "denied";
}

/** Whether a run is live (a message typed now is a steer or a queued follow-up). */
export function chatRunLive(store: Store): boolean {
  const internals = storeInternals(store);
  return store.state.chatStatus !== "ready" && internals.turnAbort !== null;
}

/** Queue a follow-up to send when the live run finishes. Empty text is ignored. */
export function queueChat(store: Store, text: string, attachments: ChatAttachment[] = []): void {
  const internals = storeInternals(store);
  const t = (text || "").trim();
  if (!t) return;
  store.setState((s) => ({ chatQueue: s.chatQueue.concat([{ id: "q" + ++internals.queueSeq, text: t, steer: false, threadId: s.chatThreadId, ...(attachments.length ? { attachments } : {}) }]) }));
  if (store.chatInputEl) store.chatInputEl.value = "";
}

/**
 * Steer now: stop the live run and send this message as soon as it has ended. Hermes's loop takes
 * no message in the middle of a run (core #691), so steering is Stop plus the next turn; what the
 * run did before the stop stays in the transcript with its steps.
 */
export function steerChat(store: Store, text: string): void {
  const internals = storeInternals(store);
  const t = (text || "").trim();
  if (!t) return;
  store.setState((s) => ({ chatQueue: [{ id: "q" + ++internals.queueSeq, text: t, steer: true, threadId: s.chatThreadId }, ...s.chatQueue.filter((q) => !q.steer)] }));
  if (store.chatInputEl) store.chatInputEl.value = "";
  if (store.chatRunLive()) store.stopAgentTurn();
  else internals.drainChatQueue("answered");
}

/** Take a queued follow-up back out (it is not sent). */
export function dropQueuedChat(store: Store, id: string): void {
  store.setState((s) => ({ chatQueue: s.chatQueue.filter((q) => q.id !== id) }));
}

/** After a run: a steer goes at once; queued follow-ups go one per finished run, and wait (for
 *  the member to send or drop them) when the member stopped the run or it failed. */
export function drainChatQueue(store: Store, outcome: RunOutcome): void {
  // Only this conversation's follow-ups: one typed elsewhere is never sent here.
  const q = store.state.chatQueue.filter((x) => x.threadId === store.state.chatThreadId);
  if (q.length === 0 || store.state.chatStatus !== "ready") return;
  const next = q.find((x) => x.steer) ?? (outcome === "answered" ? q[0] : undefined);
  if (!next) return;
  store.setState((s) => ({ chatQueue: s.chatQueue.filter((x) => x.id !== next.id) }));
  void store.sendChat(next.text, { attachments: next.attachments });
}

/** Send a queued follow-up now (only when no run is live). */
export function sendQueuedNow(store: Store, id: string): void {
  const item = store.state.chatQueue.find((q) => q.id === id);
  if (!item || item.threadId !== store.state.chatThreadId || store.state.chatStatus !== "ready") return;
  store.setState((s) => ({ chatQueue: s.chatQueue.filter((x) => x.id !== id) }));
  void store.sendChat(item.text, { attachments: item.attachments });
}

/** #643 — the deploy ledger of the conversation on screen. */
export function deployLedger(store: Store): DeployLedger {
  const internals = storeInternals(store);
  const id = store.state.chatThreadId || "";
  let l = internals.deployLedgers.get(id);
  if (!l) {
    l = newDeployLedger();
    internals.deployLedgers.set(id, l);
  }
  return l;
}

/** #681: whether the MCP server Hermes knows as `node` is core's own node entry (core checked
 *  the allowlist it started Hermes with). Anything else, an error included, is "no". */
export async function nodeMcpTrusted(_store: Store): Promise<boolean> {
  try {
    return (await bridge.agentHarness.nodeMcpTrusted()) === true;
  } catch {
    return false;
  }
}

/**
 * #643 — read a turn's tool results into the conversation's deploy ledger and return the
 * evidence for the final-answer check (src/agent/deployClaims.ts).
 */
export function deployEvidence(store: Store, outputs: TurnToolOutput[], answerId?: string): DeployEvidence {
  const internals = storeInternals(store);
  // #681 review 3: Hermes's earlier replies (before the answer being checked), so "yes" to
  // "Shall I deploy it?" reads as a deploy request.
  const end = answerId === undefined ? -1 : store.state.chatMsgs.findIndex((m) => m.id === answerId);
  const before = end < 0 ? store.state.chatMsgs : store.state.chatMsgs.slice(0, end);
  return collectDeployEvidence(outputs, {
    ledger: internals.deployLedger(),
    memberTexts: store.state.chatMsgs.filter((m) => m.who === "You").map((m) => m.text),
    assistantTexts: before.filter((m) => m.who !== "You").map((m) => m.text),
    memberWallet: store.state.walletAddr || null,
  });
}

/**
 * #643 — the final-answer check. Before an answer is kept (shown as final, read aloud, saved,
 * and sent back as context next turn), every deploy claim in it is checked: an address stays
 * only when a trusted receipt in this conversation names it and `eth_getCode` on 40204 (core
 * `contract_get_code`) returns code there, or, for a contract that is not the member's own
 * deploy, when there is code at it; a malformed address is removed; a claimed deploy with no
 * receipt is replaced by where the deploy really stands (refused, waiting for approval,
 * declined, unfunded, ...).
 */
export async function checkAnswerClaims(store: Store, msgId: string, outputs: TurnToolOutput[]): Promise<void> {
  const internals = storeInternals(store);
  const ev = internals.deployEvidence(outputs, msgId);
  const msg = store.state.chatMsgs.find((m) => m.id === msgId);
  if (!msg || !msg.text) return;
  const checked = await checkDeployClaims(msg.text, ev, (a) => bridge.contracts.getCode(a));
  if (!checked.changed) return;
  console.warn("#643 answer check:", checked.findings.join("; "));
  store.setState((s) => ({
    chatMsgs: s.chatMsgs.map((m) =>
      m.id === msgId ? { ...m, text: checked.text, chips: m.chips.concat([{ label: DEPLOY_CLAIM_CHIP, status: "declined" }]) } : m,
    ),
  }));
}

/** #643 — a scheduled daemon run's reply, checked the same way before it is shown or logged. */
export async function checkDaemonReply(store: Store, reply: string, outputs: TurnToolOutput[]): Promise<string> {
  const ev = collectDeployEvidence(outputs, { memberWallet: store.state.walletAddr || null });
  const checked = await checkDeployClaims(reply, ev, (a) => bridge.contracts.getCode(a));
  if (checked.changed) console.warn("#643 daemon reply check:", checked.findings.join("; "));
  return checked.text;
}

export function scrollChat(store: Store): void {
  requestAnimationFrame(() => {
    if (store.chatScrollEl) store.chatScrollEl.scrollTop = store.chatScrollEl.scrollHeight;
    if (store.jChatScrollEl) store.jChatScrollEl.scrollTop = store.jChatScrollEl.scrollHeight;
  });
}

/**
 * HUP-S2.2 (US-2.2 AC2) — put a held shell_run command in front of the member on the approval
 * card (exact argv, folder, program, timeout, OS sandbox) with the HIC banner. True only when the
 * member pressed Approve; the caller sends that decision bound to this command's id.
 */
export async function approveShellRun(store: Store, p: ShellPendingView): Promise<boolean> {
  const r = await store.requestSig({
    origin: "chat agent",
    requester: "Hermes · tool " + p.tool,
    title: "Approve a command",
    chainless: true,
    rows: [],
    cost: "none, no chain transaction",
    sponsor: "explicit decision required",
    sponsorColor: "var(--tx-3)",
    card: shellRunCard(p),
    hic: { reason: SHELL_RUN_HIC_REASON },
  });
  return r === "approved";
}

/** 0.5.4 WP-H4 — the envelope in core: fl_envelope.rs status, fl_work.rs approve and off. */
export function flEnvelopeApi(_store: Store): FlEnvelopeApi {
  return {
    status: async () => {
      if (BRIDGE_MODE !== "tauri") throw new Error(ENVELOPE_NEEDS_DESKTOP);
      const { invoke } = await import("../../../bridge/tauri/invoke");
      return invoke<FlEnvelopeStatus>("fl_envelope_status", {});
    },
    approve: approveEnvelope,
    off: envelopeOff,
  };
}

/** 0.5.4 WP-H3 — the sealed worker terms in core (fl_work.rs via src/fl/workerTerms.ts). */
export function flWorkTerms(_store: Store): { load: typeof loadWorkerTerms } {
  return { load: loadWorkerTerms };
}

/** 0.5.4 Hermes x FL: the FL network state from the shipped address book and eth_getCode (the
 *  boards' own read). A method so the gate tripwires can stand in a 0.6.0 network. */
export function flNetworkState(_store: Store): Promise<FlNetworkState> {
  return readFlNetworkState(flAddressBook, {
    codeSize: (a) => bridge.contracts.codeSize("citrate", a),
    viewCall: (a, d) => bridge.contracts.viewCall("citrate", a, d),
    // G-SEC: core's Rust payouts gate (#736) decides "open", with its reason; the boards' same read.
    payoutsStatus: () =>
      BRIDGE_MODE === "tauri" ? invoke<PayoutsGateStatus>("fl_payouts_status") : Promise.reject(new Error("the payouts gate is read by the desktop app")),
  });
}

/** 0.5.4 Hermes x FL: the coordinator's round binding for WorkMarket.open. No source publishes one
 *  yet (the training coordinator does not), so there is none; openIntent then says so. A method
 *  so a source can be added in one place and the gate tripwires can stand one in. */
export async function flRoundBinding(_store: Store): Promise<OpenBinding | null> {
  return null;
}

/** 0.5.4 Hermes x FL: core's WorkMarket offers read model (fl_market_offers). */
export function flMarketOffers(_store: Store): Promise<MarketView> {
  return readMarket();
}

/** HUP-S10.2 — the Rust command seam for the everyday tools (null in the web preview). */
export function everydayInvoke(_store: Store): EverydayInvoke | null {
  if (BRIDGE_MODE !== "tauri") return null;
  return async <T,>(cmd: string, a: Record<string, unknown>) => {
    const { invoke } = await import("../../../bridge/tauri/invoke");
    return invoke<T>(cmd, a);
  };
}

/**
 * HUP-S4.1 (US-4.1 AC2): put a held MCP request in front of the member with the HIC banner: an
 * effectful MCP call after the session read untrusted content (server, tool, the exact
 * arguments, the server's hints), or a server asking to open a page (the full address and its
 * host; the app opens it only on Approve). True only when the member pressed Approve.
 */
export async function approveMcpRequest(store: Store, p: McpPendingView): Promise<boolean> {
  const page = p.kind === "open_url";
  const r = await store.requestSig({
    origin: "chat agent",
    requester: `Hermes · MCP server ${p.server}`,
    title: page ? "Open a page for an MCP server" : "Approve an MCP action",
    chainless: true,
    rows: [],
    cost: "none, no chain transaction",
    sponsor: "explicit decision required",
    sponsorColor: "var(--tx-3)",
    card: mcpRequestCard(p),
    hic: { reason: page ? MCP_OPEN_URL_HIC_REASON : p.reason || MCP_CALL_HIC_REASON },
  });
  return r === "approved";
}

export async function handleTool(store: Store, call: ToolCall, asstId: string, ensure: () => void, meta?: ToolCallMeta, signal?: AbortSignal): Promise<string> {
  const internals = storeInternals(store);
  let args: Record<string, string> = {};
  try {
    args = JSON.parse(call.arguments || "{}");
  } catch {
    /* ignore */
  }
  let status = "ok";
  let result = "ok";
  // HUP-S2.4 — every approval carries a card generated from the tool's annotations. A call the
  // sidecar marked hic:"required" (its session read untrusted content) is decided only by the
  // member's click: tools that ask anyway carry the HIC reason on their one ceremony; any other
  // tool is held behind an explicit card first (fail closed).
  const ann = annotationFor(call.name);
  const hic: HicRequirement | undefined =
    meta?.hic === "required" ? { reason: meta.hicReason || "this action needs your explicit approval" } : undefined;
  // HUP-S7.6 (US-7.4 AC1): every approval this call asks for shows in the Activity monitor,
  // pending until the member decides. `signal`: a daemon run's; its cards close when the run ends.
  // Board 01: the member's last decision on this call, the structured source of "refused".
  let lastDecision: string | null = null;
  const tracked = async (decide: () => Promise<string>): Promise<string> => {
    approvalNoted(call.id, call.name, "pending");
    const r = await decide();
    lastDecision = r;
    approvalNoted(call.id, call.name, r === "approved" ? "approved" : "declined");
    return r;
  };
  const ask = (spec: CerSpec, card: ApprovalCard): Promise<string> =>
    tracked(() => store.requestSig(hic ? { ...spec, card, hic } : { ...spec, card }, signal));
  // #690: a context kind the member turned off for this chat is refused before anything runs.
  const kind = contextKindOf(call.name);
  const contextOff = kind && store.state.chatContextOff.includes(kind.id) ? kind : null;
  let held = contextOff !== null;
  if (!held && hic && !SELF_GATED_TOOLS.has(call.name)) {
    const r = await ask(
      {
        origin: "chat agent",
        requester: "dashboard agent · tool " + call.name,
        title: "Approve an agent action",
        chainless: true,
        rows: [],
        cost: "none — no chain transaction",
        sponsor: "explicit decision required",
        sponsorColor: "var(--tx-3)",
      },
      cardForCall(call.name, ann, args as Record<string, unknown>),
    );
    if (r !== "approved") {
      held = true;
      status = r;
      result =
        r === "expired"
          ? `The approval for ${call.name} expired because the run ended; nothing was done.`
          : `The member declined ${call.name}; nothing was done.`;
    }
  }
  if (held) {
    // declined at the HIC card — the tool did not run
  } else if (call.name === ESCALATE_TOOL_NAME) {
    // HUP-S1.5 — price first: a notice within today's budget (HIC-2), or the member's approval when
    // over budget or when the task holds untrusted content (HIC-1). Core enforces the same order.
    const e = bridge.escalation;
    result = await runEscalationTool(
      {
        endpoints: () => e.endpoints(),
        quote: (id, prompt, system, maxTokens) => e.quote(id, prompt, system, maxTokens),
        prepareConfirm: async (q, shown) => (await e.confirmPrepare(q, shown)).confirmId,
        run: (q, shown, confirmId, tainted) => e.run(q, shown, confirmId, tainted),
        confirm: (q, reason, question) => {
          const a = escalationApproval(q, reason, question);
          return ask(
            {
              origin: "chat agent",
              requester: "dashboard agent · tool " + ESCALATE_TOOL_NAME,
              title: a.title,
              chainless: true,
              rows: a.rows,
              cost: a.cost,
              sponsor: "paid by you to your provider, not a chain transaction",
              sponsorColor: "var(--warn)",
            },
            a.card,
          );
        },
        notice: (text) => store.toast(text),
      },
      args as Record<string, unknown>,
      hic,
    );
    status = isEscalationDeclined(result) ? "declined" : "ok";
  } else if (call.name === DELEGATE_TOOL_NAME) {
    // D-11 (P-8): every delegation is the member's decision on a card that shows the exact
    // prompt; core sends only what it prepared (the card's hash binds it).
    const d = bridge.delegation;
    result = await runDelegationTool(
      {
        prepare: (scope) => d.prepare(scope),
        decline: (id) => d.decline(id),
        approve: (id, hash) => d.approve(id, hash),
        status: (id) => d.status(id),
        confirm: (view) => {
          const a = delegationApproval(view, hic?.reason);
          return ask(
            {
              origin: "chat agent",
              requester: "dashboard agent · tool " + DELEGATE_TOOL_NAME,
              title: a.title,
              chainless: true,
              rows: a.rows,
              cost: a.cost,
              sponsor: "billed by Cognition to your Devin account, not a chain transaction",
              sponsorColor: "var(--warn)",
            },
            a.card,
          );
        },
      },
      args as Record<string, unknown>,
    );
    status = isDelegationDeclined(result) ? "declined" : "ok";
  } else if (call.name === DELEGATION_STATUS_TOOL_NAME) {
    result = await runDelegationStatusTool({ status: (id) => bridge.delegation.status(id) }, args as Record<string, unknown>);
  } else if (call.name === "memory_assert") {
    // Hermes "remember this fact": the card is the HIC-1 gate; on approve the fact is a
    // durable write into the personal tenant of the bundled memory daemon (mem-mcp).
    const fact = (args.fact || "").trim();
    const daemonUp = store.state.memDaemon === "running";
    const r = await ask({
      origin: "chat agent",
      requester: "dashboard agent · tool memory_assert",
      title: "Remember this in your memory",
      chainless: true,
      rows: [
        { k: "Fact", v: "“" + fact + "”" },
        { k: "Tenant", v: "personal" },
        { k: "Store", v: daemonUp ? "memory daemon on this device · stored on this computer, not encrypted at rest yet" : "memory daemon offline · approving will fail and nothing is saved" },
      ],
      cost: "none, a local memory write",
      sponsor: "no chain transaction",
      sponsorColor: "var(--tx-3)",
    }, fieldsCard("memory_assert", ann, { fact }, "remember a fact in your personal memory"));
    status = r;
    if (r !== "approved") {
      result = "The member declined; nothing was saved to memory.";
    } else {
      try {
        const saved = await bridge.memory.assert(fact);
        result = "Saved to the member's " + saved.tenant + " memory" + (saved.nodeId ? " as node " + saved.nodeId : "") + ", with the member's approval.";
      } catch (e) {
        result = "The member approved, but the memory write failed, so nothing was saved: " + (e instanceof Error ? e.message : String(e));
      }
    }
  } else if (call.name === "journal_append") {
    const entry = args.entry || "work note";
    const today = new Date().toISOString().slice(0, 10);
    const page = (store.state.jPages || []).find((p) => p.id === "d-" + today);
    const before = page ? page.blocks.join("\n") : "";
    const after = (before ? before + "\n" : "") + "@agent " + entry;
    const r = await ask({
      origin: "chat agent",
      requester: "dashboard agent · tool journal_append",
      title: "Write to your journal",
      chainless: true,
      rows: [
        { k: "Entry", v: "“" + entry + "”" },
        { k: "Page", v: today + " · daily note" },
        { k: "Store", v: "local journal — off-chain, encrypted at rest" },
      ],
      cost: "none — local journal write",
      sponsor: "no chain transaction",
      sponsorColor: "var(--tx-3)",
    }, diffCard("journal_append", ann, "journal/" + today, before, after));
    if (r === "approved") {
      store.setState((st) => {
        const pages = st.jPages.slice();
        let pg = pages.find((p) => p.id === "d-" + today);
        if (!pg) {
          pg = { id: "d-" + today, title: today, kind: "daily", pinned: false, blocks: [] };
          pages.unshift(pg);
        }
        const upd = { ...pg, blocks: pg.blocks.concat(["@agent " + entry]) };
        return { jPages: pages.map((p) => (p.id === upd.id ? upd : p)) };
      });
      store.save();
    }
    status = r;
    result = r;
  } else if (call.name === "app_navigate") {
    const route = ["wallet", "node", "storage", "comms", "commissary", "settings", "dashboard"].indexOf(args.route) >= 0 ? args.route : "dashboard";
    store.go(route);
    result = "ok";
  } else if (call.name === "chain_read") {
    result = JSON.stringify(store.snapshot());
  } else if (call.name === "memory_search") {
    // W3.3 — REAL semantic search over the mem graph (docs or personal). Returns
    // real hits or honest emptiness (Rule 1 — never a fabricated Citrate fact).
    // HUP-S3.1: knowledge tenants (the bundled corpus) answer with passages + citations.
    const { tenant, passages } = memorySearchTarget(args.tenant);
    try {
      const res = await bridge.memory.search(tenant, args.query || "", memorySearchBudget(passages), passages ? { passages } : undefined);
      result = formatMemoryHits(res);
    } catch (e) {
      result = "memory search unavailable: " + (e instanceof Error ? e.message : String(e));
    }
  } else if (call.name === "memory_recall") {
    const tenant = args.tenant === "personal" ? "personal" : "citrate-docs";
    try {
      const res = await bridge.memory.recall(tenant, 6);
      result = formatMemoryHits(res);
    } catch (e) {
      result = "memory recall unavailable: " + (e instanceof Error ? e.message : String(e));
    }
  } else if (call.name === "journal_read") {
    // WP4.2 — READ the local journal (read-only, immediate). Honest-empty on no
    // pages / no match; never fabricates entries (Rule 1).
    const today = new Date().toISOString().slice(0, 10);
    result = formatJournalForAgent(store.state.jPages || [], args.page, today);
  } else if (call.name === "docs_link") {
    result = "ok";
  } else if (call.name === "node_status") {
    try {
      // C1-V v1.1 §1: the validator word and its registry inputs (null = not read).
      const a = store.state.validatorActivation;
      result = JSON.stringify({
        ...(await bridge.node.status()),
        validator: validatorWord(store.state.bondedStake, a, store.state.hasGrant, store.state.bondActivated, store.state.bondUnlocked),
        validatorActivation: a,
      });
    } catch (e) {
      result = "node status unavailable: " + (e instanceof Error ? e.message : String(e));
    }
  } else if (call.name === "staking_status") {
    try {
      result = JSON.stringify(await bridge.wallet.balances());
    } catch (e) {
      result = "wallet read unavailable: " + (e instanceof Error ? e.message : String(e));
    }
  } else if (call.name === "groups_list") {
    try {
      result = JSON.stringify(await bridge.groups.list());
    } catch (e) {
      result = "groups unavailable: " + (e instanceof Error ? e.message : String(e));
    }
  } else if (call.name === "group_roster") {
    try {
      result = JSON.stringify(await bridge.groups.roster(args.group || ""));
    } catch (e) {
      result = "roster unavailable: " + (e instanceof Error ? e.message : String(e));
    }
  } else if (call.name === "group_create") {
    // PBA-L7b-002: creating a group is a state change the member approves; the agent proposes.
    const kind = ["channel", "dm", "forum"].indexOf(args.kind) >= 0 ? (args.kind as "channel" | "dm" | "forum") : "channel";
    const name = args.name || "New group";
    const r = await ask({
      origin: "chat agent",
      requester: "dashboard agent · tool group_create",
      title: "Create an encrypted group",
      chainless: true,
      rows: [
        { k: "Name", v: "“" + name + "”" },
        { k: "Kind", v: kind },
        { k: "Store", v: "your comms daemon — end-to-end encrypted, no chain transaction" },
      ],
      cost: "none — local group creation",
      sponsor: "no chain transaction",
      sponsorColor: "var(--tx-3)",
    }, fieldsCard("group_create", ann, { name, kind }, "create the encrypted group “" + name + "”"));
    status = r;
    if (r !== "approved") {
      result = "the member declined creating the group; nothing was created.";
    } else {
      try {
        const g = await bridge.groups.create(kind, name);
        result = "created group “" + g.name + "” (id " + g.id + "). It's end-to-end encrypted; invite people next.";
      } catch (e) {
        result = "couldn't create the group: " + (e instanceof Error ? e.message : String(e));
      }
    }
  } else if (call.name === "group_invite") {
    // D-12: the same invite the member makes in Groups. Whoever opens the link only ASKS to join;
    // the member approves or declines each request. The member approves making it, and the link
    // goes to the member (clipboard), never back into the model context.
    const group = args.group || "";
    const forHandle = args.forHandle || "";
    const r = await ask({
      origin: "chat agent",
      requester: "dashboard agent · tool group_invite",
      title: "Make an invite link",
      chainless: true,
      rows: [
        { k: "Group", v: group || "(none given)" },
        { k: "For", v: forHandle || "anyone you send it to" },
        { k: "Effect", v: "whoever opens the link asks to join; you approve or decline them in Groups (link works 14 days)" },
      ],
      cost: "none — no chain transaction",
      sponsor: "no chain transaction",
      sponsorColor: "var(--tx-3)",
    }, fieldsCard("group_invite", ann, { group: group || "(none given)", for: forHandle || "(none)" }, "make an invite link to group " + (group || "(none given)")));
    status = r;
    if (r !== "approved") {
      result = "the member declined making an invite link; no link exists.";
    } else {
      try {
        const made = await bridge.invites.create(group, forHandle, store.identity().name);
        store.copy(made.link, "Invite link copied. When they open it, approve their request in Groups.");
        result =
          "Made an invite link for the group and copied it to the member's clipboard. The link is deliberately NOT shown to you; tell the member it is on their clipboard, and that whoever opens it asks to join and appears in Groups for them to approve.";
      } catch (e) {
        result = "couldn't make an invite: " + (e instanceof Error ? e.message : String(e));
      }
    }
  } else if (call.name === "directory_find") {
    try {
      const platform = args.platform === "discord" ? "discord" : "x";
      const hits = await bridge.social.directorySearch(platform, (args.query || "").replace(/^@/, ""));
      result = hits.length ? JSON.stringify(hits) : "nobody with that handle has opted into the directory (Citrate never resolves a handle without consent).";
    } catch (e) {
      result = "directory unavailable: " + (e instanceof Error ? e.message : String(e));
    }
  } else if (call.name === "skills_list") {
    // Both catalogs: the on-chain SkillRegistry AND the local instruction-skills the agent authored.
    // PBA-L7b-002: the on-chain registry is permissionless — its strings are fenced as UNTRUSTED data.
    const [onChain, local] = await Promise.all([
      bridge.agentHarness.registrySkills().catch(() => []),
      bridge.agentSkills.list().catch(() => []),
    ]);
    result = JSON.stringify({ local }) + "\n" + fenceUntrusted("on-chain SkillRegistry entries", onChain);
  } else if (call.name === "skill_write") {
    // WRITE (local file only — no chain/keys). Author a reusable instruction-skill on this device.
    // PBA-L7b-002: a saved skill is a persistent instruction the agent later runs. V3 (Hermes x FL
    // gap analysis, 0.5.4): every save waits for the member, a new skill as well as a replacement,
    // who sees the diff against whatever is saved under that name. Names starting `citrate-` are
    // Citrate's bundled skills; core refuses them (skills_local.rs), so a saved skill can never
    // stand in for one.
    const name = String(args.name || "");
    const description = String(args.description || "");
    const instructions = String(args.instructions || "");
    const before = await bridge.agentSkills.read(name).catch(() => "");
    const r = await ask(
      {
        origin: "chat agent",
        requester: "dashboard agent · tool skill_write",
        title: before ? "Overwrite a saved skill" : "Save a new skill",
        chainless: true,
        rows: [
          { k: "Skill", v: "“" + name + "”" + (before ? " (already exists — this REPLACES it)" : "") },
          { k: "Store", v: "local file on this device" },
        ],
        cost: "none — local file",
        sponsor: "no chain transaction",
        sponsorColor: "var(--tx-3)",
      },
      diffCard("skill_write", ann, "skills/" + name, before, instructions),
    );
    status = r;
    if (r !== "approved") {
      result = before
        ? `The member declined; the existing skill "${name}" was not overwritten.`
        : `The member declined; the skill "${name}" was not saved.`;
    } else {
      try {
        const skill = await bridge.agentSkills.write(name, description, instructions, before !== "");
        result = before
          ? `Replaced the skill "${skill.name}" (id: ${skill.slug}) with the member's approval.`
          : `Saved the skill "${skill.name}" (id: ${skill.slug}) with the member's approval. Run it later with skill_run, or list it with skills_list.`;
        void store.refreshLocalSkills();
      } catch (e) {
        result = "couldn't save the skill: " + (e instanceof Error ? e.message : String(e));
      }
    }
  } else if (call.name === "widget_create") {
    // HUP-S10.3 — WRITE (a local widget file). Always asks: the member sees the source and the
    // data it declares before anything is saved. It runs only in the widget sandbox.
    const name = String(args.name || "").trim();
    const description = String(args.description || "");
    const html = String(args.html || "");
    let rawQueries: unknown = (args as Record<string, unknown>).queries;
    if (typeof rawQueries === "string") {
      try {
        rawQueries = JSON.parse(rawQueries);
      } catch {
        rawQueries = [rawQueries];
      }
    }
    const queries = Array.isArray(rawQueries) ? rawQueries.map(String) : [];
    const unknown = queries.filter((q) => !isWidgetQuery(q));
    if (!name || !html.trim()) {
      result = "A widget needs a name and its HTML; nothing was saved.";
    } else if (unknown.length) {
      result = `Widgets can only read ${WIDGET_QUERIES.join(", ")}; ${unknown.join(", ")} is not available. Nothing was saved.`;
    } else {
      const r = await ask(
        {
          origin: "chat agent",
          requester: "dashboard agent · tool widget_create",
          title: "Add a widget to your home",
          chainless: true,
          rows: [
            { k: "Widget", v: "“" + name + "”" },
            { k: "Reads", v: queries.length ? queries.join(", ") : "no data" },
            { k: "Runs", v: "in a sandbox: no network, no app commands, read-only data it declared" },
          ],
          cost: "none, a local file",
          sponsor: "no chain transaction",
          sponsorColor: "var(--tx-3)",
        },
        diffCard("widget_create", ann, "widgets/" + name, "", html),
      );
      status = r;
      const api = widgetsApi();
      if (r !== "approved") {
        result = `The member declined; the widget "${name}" was not saved.`;
      } else if (!api) {
        result = "Widgets need the desktop app; nothing was saved.";
      } else {
        try {
          const w = await api.save({ name, description, html, queries, author: "hermes" });
          void refreshWidgets();
          result = `Saved the widget "${w.name}" on the member's Hermes home with their approval.`;
        } catch (e) {
          result = "couldn't save the widget: " + (e instanceof Error ? e.message : String(e));
        }
      }
    }
  } else if (call.name === "skill_run") {
    // Load the authored playbook back into the loop; the model then executes it with its tools.
    try {
      const body = await bridge.agentSkills.read(String(args.name || ""));
      result = `Running skill "${args.name}". Follow these steps now, using your tools; any chain/write step still needs the member's approval:\n\n${body}`;
    } catch (e) {
      result = "couldn't load that skill: " + (e instanceof Error ? e.message : String(e));
    }
  } else if (call.name === "models_list") {
    try {
      const [reg, local] = await Promise.all([
        bridge.modelsCatalog.registry().catch(() => []),
        bridge.modelsCatalog.local().catch(() => []),
      ]);
      result = JSON.stringify({ local }) + "\n" + fenceUntrusted("on-chain ModelRegistry entries", reg);
    } catch (e) {
      result = "model list unavailable: " + (e instanceof Error ? e.message : String(e));
    }
  } else if (call.name === "get_verified_source") {
    // HUP-S4.3 — READ: CitrateScan's verified source/ABI/compiler, fenced as untrusted data.
    if (!isAddress(args.address)) {
      result = "get_verified_source needs a contract address (0x + 40 hex). Nothing was looked up.";
    } else {
      try {
        result = formatVerifiedSourceForAgent(await bridge.contracts.verifiedSource(args.address));
      } catch (e) {
        result = "verified-source lookup unavailable: " + (e instanceof Error ? e.message : String(e));
      }
      }
  } else if (call.name === "contract_view") {
    // HUP-S6.7 / US-6.3 AC2 — READ: an eth_call through core's contract_view_call (no effect,
    // no approval); a non-view function is refused, the result is fenced as untrusted data.
    result = await runContractView(bridge.contracts, args as Record<string, unknown>);
  } else if (call.name === "address_book_lookup") {
    // CORE-A1 — READ: the embedded 40204 book + registry reads (core); registry text fenced.
    result = await runAddressBookLookup(bridge.contracts, args as Record<string, unknown>);
  } else if (call.name === "get_code") {
    // CORE-A1 — READ: eth_getCode / eth_getStorageAt + CitrateScan status, through core.
    result = await runGetCode(bridge.contracts, args as Record<string, unknown>);
  } else if (call.name === "identify_contract") {
    // CORE-A1 — READ: book name, ABI source, ERC-165 / ERC-20 probes (eth_call), code selectors.
    result = await runIdentifyContract(bridge.contracts, args as Record<string, unknown>);
  } else if (call.name === "decode_calldata") {
    // CORE-A2 — READ: verified ABI, then bundled book ABI, then the bundled selector subset.
    result = await runDecodeCalldata(bridge.contracts, args as Record<string, unknown>);
  } else if (call.name === "decode_log") {
    // CORE-A2 — READ: the emitter's ABI, then the bundled topic subset.
    result = await runDecodeLog(bridge.contracts, args as Record<string, unknown>);
  } else if (call.name === "explain_tx") {
    // CORE-A2 — READ: the tx + receipt from the 40204 RPC, decoded; values fenced.
    result = await runExplainTx(bridge.contracts, args as Record<string, unknown>);
  } else if (call.name === "belnap_codec") {
    // US-9.2 AC2 — READ: local 0x0110 input encoding / output decoding over the call's own args.
    result = belnapCodecTool(args as Record<string, unknown>);
  } else if (call.name === "worker_terms_propose") {
    // Board 01: core checks the terms and renders the card; only the member's Confirm saves.
    const a = args as Record<string, unknown>;
    const hm = (v: unknown) => (typeof v === "string" ? v.trim() : "");
    const start = hm(a.hoursStart);
    const end = hm(a.hoursEnd);
    const terms: WorkerTerms = {
      floorSaltPerUnit: typeof a.floorSaltPerUnit === "number" ? String(a.floorSaltPerUnit) : hm(a.floorSaltPerUnit),
      hours: start === "" && end === "" ? "any" : { start, end },
      weekdaysOnly: a.weekdaysOnly === true,
      mainsOnly: a.mainsOnly === true,
      dataClasses: Array.isArray(a.dataClasses) ? a.dataClasses.map((c) => (typeof c === "string" ? c.trim() : "")) : [],
    };
    ensure();
    result = store.proposeWorkerTerms(terms, asstId);
  } else if (call.name === "round_draft_propose") {
    // Board 07: core shows the draft; only the member hands it to Open a round and signs there.
    const a = args as Record<string, unknown>;
    const str = (v: unknown) => (typeof v === "string" && v.trim() !== "" ? v.trim().slice(0, 200) : typeof v === "number" && Number.isFinite(v) ? String(v) : undefined);
    const draft: Partial<RoundDraft> = {};
    if (a.template === "tune" || a.template === "eval" || a.template === "data") draft.template = a.template;
    if (str(a.dataClass)) draft.dataClass = str(a.dataClass);
    if (str(a.budgetSalt)) draft.budgetSalt = str(a.budgetSalt);
    if (str(a.rateSalt)) draft.rateSalt = str(a.rateSalt);
    if (typeof a.maxWorkers === "number" && Number.isInteger(a.maxWorkers)) draft.maxWorkers = a.maxWorkers;
    if (str(a.deadline)) draft.deadline = str(a.deadline);
    if (a.who === "members" || a.who === "group" || a.who === "anyone") draft.who = a.who;
    if (typeof a.groupId === "string" && a.groupId.trim()) draft.groupId = a.groupId.trim().slice(0, 128);
    ensure();
    result = store.proposeRoundDraft(draft, asstId);
  } else if (call.name === "fl_round_plan") {
    // HUP-S9.4: a read. Core reads the coordinator and this device and explains the plan.
    try {
      const plan = await bridge.flRounds.plan(proposalFromToolArgs(args as Record<string, unknown>));
      result = formatPlanForAgent(plan);
    } catch (e) {
      result = "couldn't plan a round: " + (e instanceof Error ? e.message : String(e));
    }
  } else if (call.name === "fl_dataset_pii_scan" || call.name === "fl_dataset_license_check") {
    // WP-H7: core decides (fl_dataset_gate.rs); Hermes gets counts and verdicts in plain words.
    const dataset = String(args.dataset || "");
    const version = Number(args.version);
    try {
      result =
        call.name === "fl_dataset_pii_scan"
          ? scanForAgent(dataset, version, await datasetGate.piiScan(dataset, version))
          : licensesForAgent(await datasetGate.licenseCheck(dataset, version));
    } catch (e) {
      result = `couldn't check ${dataset} v${version}: ` + (e instanceof Error ? e.message : String(e));
    }
  } else if (call.name === "fl_source_check") {
    // WP-H8: core's verdict on one URL (fl_collect.rs); fetches the host's robots.txt only.
    try {
      result = sourceCheckForAgent(await flCollect.sourceCheck(String(args.url || ""), attestationFrom(args)));
    } catch (e) {
      result = "couldn't check that page: " + (e instanceof Error ? e.message : String(e));
    }
  } else if (call.name === "fl_collect_plan") {
    // WP-H8 (HIC-1): core builds and checks the plan; the member decides on the card; only an
    // approved plan runs, once, and core checks every page again before it fetches it.
    const urls = Array.isArray(args.urls) ? (args.urls as unknown[]).map(String) : [];
    // Review of #739: an attestation comes from the model's arguments, so the member confirms
    // it, word for word, on its own card first. Declined = the plan is built without it.
    let attestation = attestationFrom(args);
    if (attestation) {
      const a = attestation;
      const ok = await ask(
        {
          origin: "chat agent",
          requester: "dashboard agent · tool fl_collect_plan (license attestation)",
          title: "Confirm a license you are attesting",
          chainless: true,
          rows: attestationCardRows(a, urls),
          cost: "none",
          sponsor: "no chain transaction",
          sponsorColor: "var(--tx-3)",
        },
        fieldsCard("fl_collect_plan", ann, { license: a.license, statement: a.statement }, `attest ${a.license} for pages that state no license`),
      );
      if (ok !== "approved") attestation = null;
    }
    let plan: FlCollectPlan | null = null;
    try {
      plan = await flCollect.plan(String(args.dataset || ""), Number(args.baseVersion), urls, attestation);
    } catch (e) {
      result = "couldn't plan the collection: " + (e instanceof Error ? e.message : String(e));
    }
    if (plan && plan.pages === 0) {
      result = "Nothing on that list can be collected, so no plan was shown. " + plan.sources.map(sourceLine).join(" ");
      await flCollect.decide(plan.id, false).catch(() => undefined);
    } else if (plan) {
      const p = plan;
      const r = await ask(
        {
          origin: "chat agent",
          requester: "dashboard agent · tool fl_collect_plan",
          title: `Collect ${p.pages} web page${p.pages === 1 ? "" : "s"} into a dataset`,
          chainless: true,
          rows: planCardRows(p),
          cost: "none — pages are fetched by this device",
          sponsor: "no chain transaction",
          sponsorColor: "var(--tx-3)",
        },
        fieldsCard("fl_collect_plan", ann, { dataset: p.dataset, pages: String(p.pages), consent: p.statement }, `collect ${p.pages} page${p.pages === 1 ? "" : "s"} into ${p.dataset}`),
      );
      status = r;
      if (r !== "approved") {
        await flCollect.decide(p.id, false).catch(() => undefined);
        result = "The member declined the collection plan; nothing was fetched.";
      } else {
        try {
          await flCollect.decide(p.id, true);
          result = runForAgent(await flCollect.run(p.id));
        } catch (e) {
          result = "the collection did not run: " + (e instanceof Error ? e.message : String(e));
        }
      }
    }
  } else if (call.name === "fl_round_start") {
    // HUP-S9.4 (HIC-1): the member decides on a card showing core's plan; only an Approve asks
    // core to record the start for that exact hash. A hic:"required" reason rides the same card.
    let plan: FlRoundPlan | null = null;
    try {
      plan = await bridge.flRounds.lookupPlan(String(args.planHash || ""));
    } catch (e) {
      result = "couldn't find that plan: " + (e instanceof Error ? e.message : String(e));
    }
    if (plan) {
      const p = plan;
      const out = await approveAndStartRound(
        { requestSig: (spec) => tracked(() => store.requestSig(spec)), start: (h) => bridge.flRounds.start(h) },
        p,
        "chat agent",
        (spec) => {
          const fields: Record<string, unknown> = { coordinator: spec.rows[0]?.v ?? "", plan: p.planHash };
          if (p.proposal.roundId) fields.round = p.proposal.roundId + " (writes consent for this round only)";
          const card = fieldsCard("fl_round_start", ann, fields, "join the federated round of plan " + p.planHash.slice(0, 12));
          return hic ? { ...spec, card, hic } : { ...spec, card };
        },
      );
      status = out.status;
      result = out.message;
    }
  } else if (isFlEnvelopeTool(call.name)) {
    // 0.5.4 WP-H4: the HIC-2 envelope. Turning one on asks on a card; off applies at once. Core
    // enforces the caps (fl_envelope.rs); on a network without WorkMarket it authorizes nothing.
    const out = await runFlEnvelopeTool(call.name, args as Record<string, unknown>, ann, {
      api: store.flEnvelopeApi(),
      ask,
      toast: (text) => store.toast(text),
    });
    status = out.status;
    result = out.result;
  } else if (isFlWorkTool(call.name)) {
    // 0.5.4 WP-H3: worker terms (read; sealed in core by fl_work.rs), consent and eligibility.
    // Withdrawing a consent lowers authority and applies at once; reads read.
    const out = await runFlWorkTool(call.name, args as Record<string, unknown>, ann, {
      terms: store.flWorkTerms(),
      flRounds: bridge.flRounds,
      tier: bridge.tier,
      book: flAddressBook,
      toast: (text) => store.toast(text),
    });
    status = out.status;
    result = out.result;
  } else if (isFlRoundTool(call.name)) {
    // 0.5.4 WP-H5: reads only. Which contracts exist comes from the address book and eth_getCode;
    // WorkMarket is read only once it is there; the coordinator's counts are labelled as its own.
    const view = (address: string, data: string) => bridge.contracts.viewCall("citrate", address, data);
    result = await runFlRoundTool(call.name, args as Record<string, unknown>, {
      networkState: () => readFlNetworkState(flAddressBook, { codeSize: (a) => bridge.contracts.codeSize("citrate", a), viewCall: view }),
      call: view,
      coordinator: async () => {
        const ov = await bridge.flRounds.overview();
        if (!ov.config.url) return null;
        return (await bridge.flRounds.plan(DEFAULT_PROPOSAL)).coordinator;
      },
    });
  } else if (call.name === FL_DATASET_PUBLISH_TOOL) {
    // 0.5.4 WP-H6: publishing is irreversible, so always HIC-1 and never inside an envelope. Core
    // refuses on its gates (license, PII scan, Public only); IPFS is real; the DatasetRegistry
    // record opens in the wallet review only where the book pins it (not on r1007).
    const out = await runPublishTool(args as Record<string, unknown>, ann, {
      ds: bridge.flDatasets,
      ask,
      book: flAddressBook,
      reads: { codeSize: (a) => bridge.contracts.codeSize("citrate", a), viewCall: (a, d) => bridge.contracts.viewCall("citrate", a, d) },
      publisher: () => store.state.custodyAddr,
      propose: (i) => store.proposeContractCall(i),
    });
    status = out.status;
    result = out.result;
  } else if (isGroupClusterTool(call.name)) {
    // 0.5.4 WP-H2: groups and cluster. A join request, a role change, an offboard and mesh-on ask
    // on a card; revoking an invite and mesh-off lower authority and apply at once; reads read.
    const out = await runGroupClusterTool(call.name, args as Record<string, unknown>, ann, {
      invites: bridge.invites,
      groups: bridge.groups,
      cluster: bridge.cluster,
      ask,
      go: (route) => store.go(route),
      toast: (text) => store.toast(text),
    });
    status = out.status;
    result = out.result;
  } else if (isFlAdapterTool(call.name)) {
    // 0.5.4 WP-H9: the eval gate's records (read), loading an ACCEPTED adapter (HIC-1 card, core
    // re-checks the gate), and unloading (lowers authority, no card).
    const out = await runFlAdapterTool(call.name, args as Record<string, unknown>, ann, { fl: bridge.flRounds, ask });
    status = out.status;
    result = out.result;
  } else if (isFlDatasetTool(call.name)) {
    // 0.5.4 WP-H6: the sealed local dataset store. Reads run as reads; create and clean ask on a
    // card (HIC-1) and core re-checks the grant, the switch and the approved rows hash.
    const out = await runFlDatasetTool(call.name, args as Record<string, unknown>, ann, { ds: bridge.flDatasets, ask });
    status = out.status;
    result = out.result;
  } else if (call.name === FL_ROUND_DRAFT_READ_TOOL) {
    // 0.5.4 WP-H5: read the board 07 draft. Writing one is lane 3's card hand-off (#725).
    result = runFlRoundDraftRead();
  } else if (call.name === FL_ROUND_OPEN_TOOL) {
    // 0.5.4 WP-H5: the saved draft through lane 1's openIntent; the member signs the escrow in
    // the wallet review (HIC-1, never an envelope). Core's payouts gate runs before the card.
    const out = await runFlRoundOpen({
      loadDraft: () => loadRoundDraft(),
      networkState: () => store.flNetworkState(),
      roleIn: async (groupId) => {
        const self = (store.state.commsAddr || (await bridge.groups.selfAddress())).toLowerCase();
        const g = (await bridge.groups.list()).find((x) => x.id === groupId);
        return g?.members.find((m) => m.address.toLowerCase() === self)?.role ?? null;
      },
      binding: () => store.flRoundBinding(),
      propose: (intent) => store.proposeContractCall(intent),
    });
    status = out.status;
    result = out.result;
  } else if (isFlPayoutTool(call.name)) {
    // 0.5.4 WP-H10 (dark): open only when core's Rust payouts gate says so (fl_payouts_status).
    // Withdraw opens the wallet review, which runs the gate again; claim and dispute are
    // named-blocked (the leaf and the signed receipt are not read in this release).
    const out = await runFlPayoutTool(call.name, args as Record<string, unknown>, {
      networkState: () => store.flNetworkState(),
      wallet: () => store.state.walletAddr,
      call: (address, data) => bridge.contracts.viewCall("citrate", address, data),
      chainNow: () => latestBlockTime(),
      propose: (intent) => store.proposeContractCall(intent),
    });
    status = out.status;
    result = out.result;
  } else if (isFlMarketTool(call.name)) {
    // 0.5.4 WP-H5: offers (read) and join (the member signs the bond in the wallet review; the
    // shared joinIntent refuses until WorkMarket is here and payouts open).
    const out = await runFlMarketTool(call.name, args as Record<string, unknown>, {
      networkState: () => store.flNetworkState(),
      readMarket: () => store.flMarketOffers(),
      terms: () => loadWorkerTerms(),
      machine: async () => machineFrom((await bridge.tier.recommend())?.facts),
      propose: (intent) => store.proposeContractCall(intent),
    });
    status = out.status;
    result = out.result;
  } else if (isFlHistoryTool(call.name)) {
    // 0.5.4 WP-H10: reads only, from fl_overview, PatronageLedger (training_reward) and, for
    // payouts, the address book, eth_getCode and the network profile.
    result = await runFlHistoryTool(call.name, args as Record<string, unknown>, {
      desktop: BRIDGE_MODE === "tauri",
      overview: () => bridge.flRounds.overview(),
      networkState: () =>
        readFlNetworkState(flAddressBook, { codeSize: (a) => bridge.contracts.codeSize("citrate", a), viewCall: (a, d) => bridge.contracts.viewCall("citrate", a, d) }),
      reward: (g) => bridge.training.reward(g),
    });
  } else if (isEverydayTool(call.name)) {
    // HUP-S10.2: Google Sheets, Hermes's schedule and Google Calendar. Reads run as reads; the two
    // writes (gsheets_append, schedule_add) ask on an approval card and run only on Approve.
    const out = await runEverydayTool(call.name, args as Record<string, unknown>, ann, {
      invoke: store.everydayInvoke(),
      ask,
      nowSecs: () => Math.floor(Date.now() / 1000),
    });
    status = out.status;
    result = out.result;
  } else if (call.name === "contract_deploy") {
    // WRITE: assemble the creation tx as a PENDING ceremony the member approves (Rule 3).
    try {
      const view = await bridge.contracts.deploy({
        bytecodeHex: args.bytecodeHex || "",
        ...(args.constructorArgsHex ? { constructorArgsHex: args.constructorArgsHex } : {}),
      });
      store.openWalletReview("deploy", "Deploy contract", view, undefined, undefined, {
        card: chainCard("contract_deploy", ann, view, "network gas"),
        hic,
        // HUP-S6.4: the READY D-4 gate record core checked for exactly this bytecode.
        ...(view.gate ? { deployGate: view.gate } : {}),
      });
      result = "Proposed the contract deploy — it's waiting in the Signature Ceremony. Approve it to broadcast the creation tx to 40204; I never deploy on your behalf. Nothing is deployed yet: there is no contract address until the member approves, the creation tx is sent, and its receipt names one.";
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      // HUP-S6.4 (D-4): core refused because the deploy gate is not READY for this bytecode.
      result = /Deploy refused/.test(msg)
        ? msg + " Explain this to the member in plain words; do not retry the deploy until the deploy gate passes for this exact bytecode."
        : "couldn't prepare the deploy: " + msg;
    }
  }
  // A daemon run's card closed with its run: whatever the branch did with the outcome, nothing ran.
  if (status === "expired") result = `The approval for ${call.name} expired because the run ended; nothing was done.`;
  if (contextOff) {
    // #690: the member turned this kind of context off for this chat; nothing was read or run.
    status = "declined";
    result = `declined: the member turned off ${contextOff.title} for this chat, so ${call.name} was not run. Nothing was read or changed.`;
  }
  // Board 01: refused when the member's decision (or the tool's own structured status) said no.
  const refused = status === "declined" || status === "expired" || (lastDecision !== null && lastDecision !== "approved");
  internals.callOutcomes.set(call.id, refused ? "denied" : status === "error" || status === "failed" ? "error" : "ok");
  if (internals.callOutcomes.size > 64) internals.callOutcomes.delete(internals.callOutcomes.keys().next().value as string);
  const label =
    call.name.replace("_", ".") +
    (args.query ? " · " + args.query : "") +
    (status === "approved" ? " · approved" : status === "declined" ? " · declined" : status === "expired" ? " · expired" : " ✓");
  ensure();
  store.setState((s) => ({
    chatMsgs: s.chatMsgs.map((m) => (m.id === asstId ? { ...m, chips: m.chips.concat([{ label, status }]) } : m)),
  }));
  return result;
}
