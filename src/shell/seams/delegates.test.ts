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

// Every export of a lane module becomes a pass-through spy, so a test can stub one and check the call.
vi.mock("../actions/l4/chatHistory", async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>();
  return Object.fromEntries(Object.entries(actual).map(([k, v]) => [k, typeof v === "function" ? vi.fn(v as (...a: unknown[]) => unknown) : v]));
});

vi.mock("../actions/l3/auth", async (importOriginal) => {
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
];

afterEach(() => {
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
