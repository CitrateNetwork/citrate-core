// L0-WP07 (lanes/L0_INTEGRATION_SEAMS.md, L0-WP07): Store actions move into per-lane modules under
// src/shell/actions/l<N>/, and each Store method keeps its name and becomes a one-line delegate, so
// every call site (`store.newChat()`) is unchanged. These tests pin the delegation itself: the Store
// method calls the lane module's export with the Store as `this` (the first argument) and the same
// arguments, and returns what the export returns. SEAM-STORE (store.characterization.test.ts) pins
// the names and arities; the behavior suites (chatHistory.test.tsx, branchChat.store.test.ts, ...)
// run unchanged against the moved code.
//
// Each WP07 section (a to f) adds its rows to the table below.
import { afterEach, describe, expect, it, vi } from "vitest";
import { store } from "../store";
import * as l4ChatHistory from "../actions/l4/chatHistory";
import * as l3Auth from "../actions/l3/auth";
import * as l1Turn from "../actions/l1/turn";
import * as l4ThreadEdit from "../actions/l4/threadEdit";
import * as l8Proposals from "../actions/l8/proposals";

// Every export of a lane module becomes a pass-through spy, so a test can stub one and check the call.
vi.mock("../actions/l4/chatHistory", async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>();
  return Object.fromEntries(Object.entries(actual).map(([k, v]) => [k, typeof v === "function" ? vi.fn(v as (...a: unknown[]) => unknown) : v]));
});

vi.mock("../actions/l3/auth", async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>();
  return Object.fromEntries(Object.entries(actual).map(([k, v]) => [k, typeof v === "function" ? vi.fn(v as (...a: unknown[]) => unknown) : v]));
});
vi.mock("../actions/l1/turn", async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>();
  return Object.fromEntries(Object.entries(actual).map(([k, v]) => [k, typeof v === "function" ? vi.fn(v as (...a: unknown[]) => unknown) : v]));
});
vi.mock("../actions/l4/threadEdit", async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>();
  return Object.fromEntries(Object.entries(actual).map(([k, v]) => [k, typeof v === "function" ? vi.fn(v as (...a: unknown[]) => unknown) : v]));
});
vi.mock("../actions/l8/proposals", async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>();
  return Object.fromEntries(Object.entries(actual).map(([k, v]) => [k, typeof v === "function" ? vi.fn(v as (...a: unknown[]) => unknown) : v]));
});

/** Calls a Store member by name (private ones included: TS privacy is compile-time only). */
function callStore(name: string, args: unknown[]): unknown {
  const fn = (store as unknown as Record<string, (...a: unknown[]) => unknown>)[name];
  return fn.apply(store, args);
}

interface Row {
  /** The Store method (its name is unchanged by the move). */
  method: string;
  /** The lane module export it delegates to. */
  target: (...a: never[]) => unknown;
  /** Arguments to call it with (after `store`). */
  args: unknown[];
}

// 07a: store.ts "0.5.4 — Hermes chat history" -> src/shell/actions/l4/chatHistory.ts (lane L4).
const ROWS: Row[] = [
  { method: "scheduleChatArchive", target: l4ChatHistory.scheduleChatArchive, args: [] },
  { method: "archiveChat", target: l4ChatHistory.archiveChat, args: [] },
  { method: "chatSwitchBlocked", target: l4ChatHistory.chatSwitchBlocked, args: [] },
  { method: "newChat", target: l4ChatHistory.newChat, args: [] },
  { method: "openChat", target: l4ChatHistory.openChat, args: ["t-delegate-1"] },
  { method: "forgetChat", target: l4ChatHistory.forgetChat, args: ["t-delegate-2"] },
  { method: "forgetAllChats", target: l4ChatHistory.forgetAllChats, args: [] },
  // 07b: store.ts "CORE-A3 auth" -> src/shell/actions/l3/auth.ts (lane L3).
  { method: "refreshAuth", target: l3Auth.refreshAuth, args: [] },
  { method: "resumeSession", target: l3Auth.resumeSession, args: [] },
  { method: "applyAuthStatus", target: l3Auth.applyAuthStatus, args: [{ signedIn: false, tier: null, org: null, role: null, kycStatus: null }] },
  { method: "authLogin", target: l3Auth.authLogin, args: [] },
  { method: "cancelSignIn", target: l3Auth.cancelSignIn, args: [] },
  { method: "authUserinfo", target: l3Auth.authUserinfo, args: [] },
  { method: "refreshEntitlement", target: l3Auth.refreshEntitlement, args: [] },
  { method: "startEntitlementPoll", target: l3Auth.startEntitlementPoll, args: [] },
  { method: "entitlementPollActive", target: l3Auth.entitlementPollActive, args: [] },
  { method: "stopEntitlementPoll", target: l3Auth.stopEntitlementPoll, args: [] },
  { method: "entitlementPollTick", target: l3Auth.entitlementPollTick, args: [] },
  { method: "installEntitlementFocusRecheck", target: l3Auth.installEntitlementFocusRecheck, args: [] },
  { method: "recheckEntitlementOnFocus", target: l3Auth.recheckEntitlementOnFocus, args: [] },
  { method: "authLogout", target: l3Auth.authLogout, args: [] },
  { method: "kycStart", target: l3Auth.kycStart, args: [] },
  // 07c: store.ts "chat" and "Board 01 — Stop, Steer now, Queue for after" (and sendChat with its
  // helpers) -> src/shell/actions/l1/turn.ts (lane L1); "Board 01 — edit, retry and branch" ->
  // src/shell/actions/l4/threadEdit.ts (lane L4).
  { method: "acceptBrief", target: l1Turn.acceptBrief, args: [{ id: "b-delegate" }, "# brief"] },
  { method: "sidecarSystemPrompt", target: l1Turn.sidecarSystemPrompt, args: [] },
  { method: "branchChat", target: l4ThreadEdit.branchChat, args: ["m-delegate", { text: "again", includeFrom: true }] },
  { method: "retryAsBranch", target: l4ThreadEdit.retryAsBranch, args: ["a-delegate"] },
  { method: "retryChat", target: l1Turn.retryChat, args: ["f-delegate"] },
  { method: "recheckSidecarBeforeSend", target: l1Turn.recheckSidecarBeforeSend, args: [{ kind: "delegate" }] },
  { method: "resumeSidecarSession", target: l1Turn.resumeSidecarSession, args: [{ kind: "delegate" }] },
  { method: "activeProviderKind", target: l1Turn.activeProviderKind, args: [] },
  { method: "stopAgentTurn", target: l1Turn.stopAgentTurn, args: [] },
  { method: "routeHeldMessage", target: l1Turn.routeHeldMessage, args: ["m-held", "held text", new AbortController().signal] },
  { method: "answerLocalModelAsk", target: l1Turn.answerLocalModelAsk, args: ["cancel"] },
  { method: "restartLocalModel", target: l1Turn.restartLocalModel, args: [new AbortController().signal] },
  { method: "daemonRunAvailability", target: l1Turn.daemonRunAvailability, args: [] },
  { method: "runDaemonTurn", target: l1Turn.runDaemonTurn, args: [{ id: "claim-delegate" }, new AbortController().signal, { add: () => {} }] },
  { method: "sendChat", target: l1Turn.sendChat, args: ["hello", { attachments: [] }] },
  { method: "takeCallOutcome", target: l1Turn.takeCallOutcome, args: ["c-delegate-1"] },
  { method: "setChatContext", target: l1Turn.setChatContext, args: ["journal", false] },
  { method: "takeCallRefusal", target: l1Turn.takeCallRefusal, args: ["c-delegate-2"] },
  { method: "chatRunLive", target: l1Turn.chatRunLive, args: [] },
  { method: "queueChat", target: l1Turn.queueChat, args: ["later", []] },
  { method: "steerChat", target: l1Turn.steerChat, args: ["now"] },
  { method: "dropQueuedChat", target: l1Turn.dropQueuedChat, args: ["q-delegate-1"] },
  { method: "drainChatQueue", target: l1Turn.drainChatQueue, args: ["done"] },
  { method: "sendQueuedNow", target: l1Turn.sendQueuedNow, args: ["q-delegate-2"] },
  { method: "deployLedger", target: l1Turn.deployLedger, args: [] },
  { method: "nodeMcpTrusted", target: l1Turn.nodeMcpTrusted, args: [] },
  { method: "deployEvidence", target: l1Turn.deployEvidence, args: [[], "a-delegate-2"] },
  { method: "checkAnswerClaims", target: l1Turn.checkAnswerClaims, args: ["m-delegate-2", []] },
  { method: "checkDaemonReply", target: l1Turn.checkDaemonReply, args: ["reply", []] },
  { method: "scrollChat", target: l1Turn.scrollChat, args: [] },
  { method: "approveShellRun", target: l1Turn.approveShellRun, args: [{ id: "shell-delegate" }] },
  { method: "flEnvelopeApi", target: l1Turn.flEnvelopeApi, args: [] },
  { method: "flWorkTerms", target: l1Turn.flWorkTerms, args: [] },
  { method: "flNetworkState", target: l1Turn.flNetworkState, args: [] },
  { method: "flRoundBinding", target: l1Turn.flRoundBinding, args: [] },
  { method: "flMarketOffers", target: l1Turn.flMarketOffers, args: [] },
  { method: "everydayInvoke", target: l1Turn.everydayInvoke, args: [] },
  { method: "approveMcpRequest", target: l1Turn.approveMcpRequest, args: [{ id: "mcp-delegate" }] },
  { method: "handleTool", target: l1Turn.handleTool, args: [{ id: "call-delegate", name: "noop", args: {} }, "a-delegate-3", () => {}, { hic: "none" }, new AbortController().signal] },
  // 07d: store.ts "Board 01 — the worker-terms proposal card" and "Board 07 — a round draft from
  // Hermes" -> src/shell/actions/l8/proposals.ts (lane L8).
  { method: "proposeWorkerTerms", target: l8Proposals.proposeWorkerTerms, args: [{ dataClass: "public" }, "m-terms"] },
  { method: "patchProposal", target: l8Proposals.patchProposal, args: ["p-delegate-1", { state: "declined" }] },
  { method: "confirmProposal", target: l8Proposals.confirmProposal, args: ["p-delegate-2"] },
  { method: "declineProposal", target: l8Proposals.declineProposal, args: ["p-delegate-3"] },
  { method: "reopenProposal", target: l8Proposals.reopenProposal, args: ["p-delegate-4"] },
  { method: "proposeRoundDraft", target: l8Proposals.proposeRoundDraft, args: [{ title: "draft" }, "m-draft"] },
  { method: "patchRoundDraft", target: l8Proposals.patchRoundDraft, args: ["d-delegate-1", { state: "declined" }] },
  { method: "openRoundDraft", target: l8Proposals.openRoundDraft, args: ["d-delegate-2"] },
  { method: "declineRoundDraft", target: l8Proposals.declineRoundDraft, args: ["d-delegate-3"] },
];afterEach(() => {
  vi.clearAllMocks();
});

describe("L0-WP07: Store methods delegate to their lane modules", () => {
  for (const row of ROWS) {
    it(`store_method_${row.method}_delegates`, () => {
      const spy = vi.mocked(row.target as (...a: unknown[]) => unknown);
      const sentinel = { delegated: row.method };
      spy.mockImplementationOnce(() => sentinel);
      const out = callStore(row.method, row.args);
      expect(spy).toHaveBeenCalledTimes(1);
      expect(spy).toHaveBeenCalledWith(store, ...row.args);
      // The Store passes the lane module's result back unchanged (a Promise stays the same Promise).
      expect(out).toBe(sentinel);
    });
  }
});
