// C1-V3 — an agent payment whose outcome is unknown is never reported to the agent as "rejected".
//
// After #347 the ceremony returns a typed outcome-unknown error carrying the tx hash ("submitted,
// outcome unknown (tx <hash>): <reason>"). The member approved the payment and it was submitted, so
// the sidecar call is released as submitted with that outcome (`resolveOutcomeUnknown`), never
// through the reject route the agent reads as "rejected by operator". Nothing retries it: one
// approval is one broadcast, and an identical payment proposed again opens a new review that names
// the earlier tx to check first.
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

vi.mock("../bridge/mode", () => ({
  BRIDGE_MODE: "tauri",
  assertSimAllowed: () => {},
}));

import { store } from "./store";
import { bridge } from "../bridge";
import type { CeremonyView } from "../bridge/types";
import { stateLabel, type NodeMcpRequest } from "../nodeMcp/nodeMcp";

const HASH = "0x" + "cd".repeat(32);
const TO = "0x" + "ef".repeat(20);
const DATA = "0xa9059cbb" + "00".repeat(64);

const view = (id: string): CeremonyView => ({
  id,
  origin: "agent:hermes",
  kind: "transaction",
  chainId: 40204,
  decoded: { action: "Pay 5.00 SALT", cost: "est. gas 0.0019 SALT", destination: TO },
  requiresRawAck: false,
});

const lastToast = () => String(store.getSnapshot().toast ?? "");
const flush = () => new Promise((r) => setTimeout(r, 0));

describe("C1-V3 — agent payment with an unknown outcome", () => {
  let resolve: ReturnType<typeof vi.spyOn>;
  let resolveUnknown: ReturnType<typeof vi.spyOn>;
  let broadcast: ReturnType<typeof vi.spyOn>;
  let bridgePending: ReturnType<typeof vi.spyOn>;

  beforeEach(() => {
    store.setState({ walletReview: null });
    (store as unknown as { unknownAgentPayments: Map<string, string | null> }).unknownAgentPayments?.clear();
    vi.spyOn(bridge.signing, "reject").mockResolvedValue(undefined);
    vi.spyOn(store, "refreshActivity").mockResolvedValue(undefined);
    vi.spyOn(store, "refreshWallet").mockResolvedValue(undefined);
    vi.spyOn(store, "save").mockImplementation(() => {});
    resolve = vi.spyOn(bridge.agentHarness, "resolve").mockResolvedValue(undefined);
    resolveUnknown = vi.spyOn(bridge.agentHarness, "resolveOutcomeUnknown").mockResolvedValue(undefined);
    bridgePending = vi.spyOn(bridge.agentHarness, "bridgePending").mockImplementation(async (id: string) => view("cer-" + id));
    broadcast = vi.spyOn(bridge.signing, "broadcast").mockRejectedValue(`ceremony: submitted, outcome unknown (tx ${HASH}): rpc transport error: down`);
  });
  afterEach(() => {
    vi.restoreAllMocks();
    store.setState({ walletReview: null });
  });

  it("releases the call as submitted with the tx hash, never through reject", async () => {
    await store.reviewAgentApproval({ id: "call-pay", kind: "chain", summary: "pay the router", to: TO, data: DATA });
    expect(store.state.walletReview?.kind).toBe("agent");
    await store.approveWalletReview(false);
    expect(resolveUnknown).toHaveBeenCalledWith("call-pay", HASH);
    expect(resolve).not.toHaveBeenCalled();
    // The member is told the same thing: sent, outcome unknown, with the hash.
    expect(lastToast()).toContain("outcome unknown");
    expect(lastToast()).toContain(HASH);
    expect(lastToast()).not.toMatch(/rejected|declined|not sent/i);
  });

  it("an unknown outcome without a hash is still released as submitted, never rejected", async () => {
    broadcast.mockRejectedValue(new Error("sign_and_broadcast timed out after 120000 ms"));
    await store.reviewAgentApproval({ id: "call-nohash", kind: "chain", summary: "pay", to: TO, data: DATA });
    await store.approveWalletReview(false);
    expect(resolveUnknown).toHaveBeenCalledWith("call-nohash", null);
    expect(resolve).not.toHaveBeenCalled();
  });

  it("never auto-retries: one approval is one broadcast, and nothing re-proposes it", async () => {
    await store.reviewAgentApproval({ id: "call-pay", kind: "chain", summary: "pay the router", to: TO, data: DATA });
    await store.approveWalletReview(false);
    await flush();
    await flush();
    expect(broadcast).toHaveBeenCalledTimes(1);
    expect(bridgePending).toHaveBeenCalledTimes(1);
    expect(store.state.walletReview).toBeNull();
  });

  it("the same payment proposed again waits for a new approval and names the earlier tx", async () => {
    await store.reviewAgentApproval({ id: "call-pay", kind: "chain", summary: "pay the router", to: TO, data: DATA });
    await store.approveWalletReview(false);
    await store.reviewAgentApproval({ id: "call-pay-2", kind: "chain", summary: "pay the router", to: TO, data: DATA });
    // Nothing was broadcast for the second proposal: the member has to approve it.
    expect(broadcast).toHaveBeenCalledTimes(1);
    const r = store.state.walletReview;
    expect(r?.kind).toBe("agent");
    expect(r?.spendSummary ?? "").toContain("outcome unknown");
    expect(r?.spendSummary ?? "").toContain(HASH);
    expect(r?.spendSummary ?? "").toMatch(/check/i);
  });

  it("a different payment carries no earlier-tx warning", async () => {
    await store.reviewAgentApproval({ id: "call-pay", kind: "chain", summary: "pay the router", to: TO, data: DATA });
    await store.approveWalletReview(false);
    await store.reviewAgentApproval({ id: "call-other", kind: "chain", summary: "pay someone else", to: TO, data: "0x1234" });
    expect(store.state.walletReview?.spendSummary ?? "").not.toContain("outcome unknown");
  });

  it("a stale call with an unknown outcome says the payment was submitted, not that a rejection was not needed", async () => {
    resolveUnknown.mockRejectedValue(new Error("STALE_APPROVAL: the agent's pending action changed"));
    await store.reviewAgentApproval({ id: "call-pay", kind: "chain", summary: "pay", to: TO, data: DATA });
    await store.approveWalletReview(false);
    expect(lastToast()).toContain("outcome unknown");
    expect(lastToast()).toContain(HASH);
    expect(lastToast()).not.toMatch(/rejection/i);
  });

  it("a confirmed payment still resolves approved, and a declined one still rejects", async () => {
    broadcast.mockResolvedValue({ txHash: HASH, blockNumber: 12 });
    await store.reviewAgentApproval({ id: "call-ok", kind: "chain", summary: "pay", to: TO, data: DATA });
    await store.approveWalletReview(false);
    expect(resolve).toHaveBeenCalledWith(true, "call-ok");
    await store.reviewAgentApproval({ id: "call-no", kind: "chain", summary: "pay", to: TO, data: DATA });
    await store.rejectWalletReview();
    expect(resolve).toHaveBeenCalledWith(false, "call-no");
    expect(resolveUnknown).not.toHaveBeenCalled();
  });
});

describe("C1-V3 — an MCP client's payment with an unknown outcome", () => {
  it("the request reads 'submitted, outcome unknown' with the tx, never failed or rejected", () => {
    const r = {
      id: "r1",
      tokenId: "t1",
      origin: "claude",
      summary: "send 5 SALT",
      kind: "signature",
      state: "outcome_unknown",
      tx_hash: HASH,
      message: `submitted, outcome unknown, tx ${HASH}, check before retrying`,
      createdMs: 1,
      decidedMs: 2,
    } as NodeMcpRequest;
    const label = stateLabel(r);
    expect(label).toContain("outcome unknown");
    expect(label).toContain(HASH);
    expect(label).not.toMatch(/failed|rejected/i);
  });
});
