// The wallet review must never report an outcome it does not know. Once Approve has
// handed the ceremony to the signer, Reject / Escape can no longer stop it: the review
// stays open in a "signing" state, the decline path is a no-op, and the member is told
// the REAL result when the signer returns (signed, or not), never "nothing was signed".
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";

// Force the desktop (tauri) branch: that is where a real signature can complete.
vi.mock("../bridge/mode", () => ({
  BRIDGE_MODE: "tauri",
  assertSimAllowed: () => {},
}));

import { store } from "./store";
import { bridge } from "../bridge";
import { WalletReviewModal } from "./Chrome";
import { freshState } from "./state";
import type { CeremonyView } from "../bridge/types";

const view: CeremonyView = {
  id: "wcer-inflight",
  origin: "local-user",
  kind: "transaction",
  chainId: 40204,
  decoded: { action: "Send 1.00 SALT", cost: "est. gas 0.0019 SALT", destination: "0x" + "ab".repeat(20) },
  requiresRawAck: false,
};

function deferred<T>() {
  let resolve!: (v: T) => void;
  let reject!: (e: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

const lastToast = () => String(store.getSnapshot().toast ?? "");

describe("Feature: a decline cannot claim nothing was signed once signing has started", () => {
  let rejectSpy: ReturnType<typeof vi.spyOn>;
  let linkRejectSpy: ReturnType<typeof vi.spyOn>;
  let onResolved: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    rejectSpy = vi.spyOn(bridge.signing, "reject").mockResolvedValue(undefined);
    linkRejectSpy = vi.spyOn(bridge.wallet, "linkReject").mockResolvedValue(undefined);
    vi.spyOn(store, "refreshWallet").mockResolvedValue(undefined);
    vi.spyOn(store, "refreshActivity").mockResolvedValue(undefined);
    vi.spyOn(store, "authUserinfo").mockResolvedValue(undefined as never);
    vi.spyOn(store, "save").mockImplementation(() => {});
    onResolved = vi.fn();
    store.setState({ walletReview: { kind: "send", label: "Send", view, rawAck: false, onResolved } });
  });
  afterEach(() => {
    vi.restoreAllMocks();
    store.setState({ walletReview: null });
  });

  it("Given a broadcast in flight, when Reject/Escape fires, then nothing is released and the review shows signing", async () => {
    const d = deferred<{ txHash: string; blockNumber: number }>();
    const broadcast = vi.spyOn(bridge.signing, "broadcast").mockReturnValue(d.promise);
    const approving = store.approveWalletReview(false);
    expect(broadcast).toHaveBeenCalledTimes(1);
    expect(store.state.walletReview?.approving).toBe(true);

    await store.rejectWalletReview();
    expect(rejectSpy).not.toHaveBeenCalled();
    expect(onResolved).not.toHaveBeenCalled();
    expect(store.state.walletReview).not.toBeNull();
    expect(lastToast()).not.toMatch(/nothing was signed/i);

    // A second Approve while signing must not start another signature.
    await store.approveWalletReview(false);
    expect(broadcast).toHaveBeenCalledTimes(1);

    // The raw-ack toggle is frozen while signing.
    store.setWalletReviewRawAck(true);
    expect(store.state.walletReview?.rawAck).toBe(false);

    // The signer finishes: the member sees the TRUE outcome (signed and sent).
    d.resolve({ txHash: "0xfeedbeef00", blockNumber: 1 });
    await approving;
    expect(lastToast()).toMatch(/broadcast/i);
    expect(lastToast()).not.toMatch(/nothing was signed/i);
    expect(store.state.walletReview).toBeNull();
    expect(onResolved).toHaveBeenCalledTimes(1);
    expect(onResolved).toHaveBeenCalledWith(true);
  });

  it("Given a broadcast in flight that then fails, then the outcome reported is the failure, once", async () => {
    const d = deferred<{ txHash: string; blockNumber: number }>();
    vi.spyOn(bridge.signing, "broadcast").mockReturnValue(d.promise);
    const approving = store.approveWalletReview(false);
    await store.rejectWalletReview();
    d.reject(new Error("not submitted: ceremony: broadcast failed: rpc unreachable"));
    await approving;
    expect(lastToast()).toMatch(/not sent/i);
    expect(onResolved).toHaveBeenCalledTimes(1);
    expect(onResolved).toHaveBeenCalledWith(false);
  });

  it("Given a wallet link in flight, when Reject fires, then the link is not dropped and the true result is shown", async () => {
    const d = deferred<{ address: string; linked: boolean; canonical: boolean }>();
    vi.spyOn(bridge.wallet, "linkApprove").mockReturnValue(d.promise);
    store.setState({ walletReview: { kind: "wallet-link", label: "Link wallet", view: { ...view, id: "wlink-x", kind: "personal_sign" } as CeremonyView, rawAck: false } });
    const approving = store.approveWalletReview(false);
    await store.rejectWalletReview();
    expect(linkRejectSpy).not.toHaveBeenCalled();
    expect(lastToast()).not.toMatch(/nothing was signed/i);
    d.resolve({ address: "0x" + "cd".repeat(20), linked: true, canonical: true });
    await approving;
    expect(lastToast()).toMatch(/wallet linked/i);
  });

  it("Given no signing has started, then Reject still declines and releases the ceremony", async () => {
    await store.rejectWalletReview();
    expect(rejectSpy).toHaveBeenCalledWith("wcer-inflight");
    expect(lastToast()).toMatch(/nothing was signed/i);
    expect(onResolved).toHaveBeenCalledWith(false);
  });
});

describe("Feature: the review modal shows the signing state", () => {
  it("Given signing in flight, then Reject and Approve are disabled and the modal says it is signing", () => {
    const s = freshState("p1");
    s.walletReview = { kind: "send", label: "Send", view, rawAck: false, approving: true };
    const html = renderToStaticMarkup(<WalletReviewModal store={store} s={s} />);
    expect(html).toMatch(/<button[^>]*disabled=""[^>]*>Reject<\/button>/);
    expect(html).toMatch(/<button[^>]*disabled=""[^>]*>Signing…<\/button>/);
    expect(html).not.toContain("nothing has been signed yet");
    expect(html).toMatch(/signing — wait for the result/i);
  });

  it("Given no signing yet, then Reject is enabled", () => {
    const s = freshState("p1");
    s.walletReview = { kind: "send", label: "Send", view, rawAck: false };
    const html = renderToStaticMarkup(<WalletReviewModal store={store} s={s} />);
    expect(html).not.toMatch(/<button[^>]*disabled=""[^>]*>Reject<\/button>/);
    expect(html).toContain("nothing has been signed yet");
  });
});

// C1-V review round 2 (#347): wallet transaction toasts use the UI truth contract §1 words
// (Pending / Included in block N), never "settled".
describe("C1-V §1 — wallet transaction toasts use the transaction words", () => {
  beforeEach(() => {
    vi.spyOn(bridge.signing, "reject").mockResolvedValue(undefined);
    vi.spyOn(store, "refreshWallet").mockResolvedValue(undefined);
    vi.spyOn(store, "refreshActivity").mockResolvedValue(undefined);
    vi.spyOn(store, "save").mockImplementation(() => {});
    store.setState({ walletReview: { kind: "send", label: "Send", view, rawAck: false } });
  });
  afterEach(() => {
    vi.restoreAllMocks();
    store.setState({ walletReview: null });
  });

  it("a broadcast with no receipt yet says Pending", async () => {
    vi.spyOn(bridge.signing, "broadcast").mockResolvedValue({ txHash: "0xfeedbeef00", blockNumber: null });
    await store.approveWalletReview(false);
    expect(lastToast()).toContain("Pending");
    expect(lastToast()).not.toMatch(/settle/i);
  });

  it("a broadcast with a receipt says Included in block N", async () => {
    vi.spyOn(bridge.signing, "broadcast").mockResolvedValue({ txHash: "0xfeedbeef00", blockNumber: 501_234 });
    await store.approveWalletReview(false);
    expect(lastToast()).toContain("Included in block 501,234");
    expect(lastToast()).not.toMatch(/settle/i);
  });

  it("a failed broadcast says not sent, never 'not settled'", async () => {
    vi.spyOn(bridge.signing, "broadcast").mockRejectedValue(new Error("not submitted: ceremony: broadcast failed: rpc unreachable"));
    await store.approveWalletReview(false);
    expect(lastToast()).toContain("not sent");
    expect(lastToast()).not.toMatch(/settle/i);
  });
});

// C1-V #347 review: "not sent" only when the failure is established BEFORE submission. After the
// node accepted the tx (or may have), the outcome is unknown and the member gets the tx hash.
describe("C1-V #347 — a broadcast failure after submission is an unknown outcome", () => {
  const HASH = "0x" + "ab".repeat(32);
  let onResolved: ReturnType<typeof vi.fn>;
  beforeEach(() => {
    vi.spyOn(bridge.signing, "reject").mockResolvedValue(undefined);
    vi.spyOn(store, "refreshActivity").mockResolvedValue(undefined);
    vi.spyOn(store, "save").mockImplementation(() => {});
    onResolved = vi.fn();
    store.setState({ walletReview: { kind: "send", label: "Send", view, rawAck: false, onResolved } });
  });
  afterEach(() => {
    vi.restoreAllMocks();
    store.setState({ walletReview: null });
  });

  it("a receipt error after acceptance shows the tx hash and an unknown outcome, never 'not sent'", async () => {
    vi.spyOn(store, "refreshWallet").mockResolvedValue(undefined);
    // Tauri rejects a command with the plain error string.
    vi.spyOn(bridge.signing, "broadcast").mockRejectedValue(`ceremony: submitted, outcome unknown (tx ${HASH}): rpc transport error: down`);
    await store.approveWalletReview(false);
    expect(lastToast()).toContain("outcome unknown");
    expect(lastToast()).toContain(HASH);
    expect(lastToast()).toContain("block explorer");
    expect(lastToast()).not.toMatch(/not sent/i);
    expect(onResolved).toHaveBeenCalledWith(false);
  });

  it("an error with no pre-submission marker (e.g. the IPC deadline) is never 'not sent'", async () => {
    vi.spyOn(store, "refreshWallet").mockResolvedValue(undefined);
    vi.spyOn(bridge.signing, "broadcast").mockRejectedValue(new Error("sign_and_broadcast timed out after 120000 ms"));
    await store.approveWalletReview(false);
    expect(lastToast()).toContain("outcome unknown");
    expect(lastToast()).not.toMatch(/not sent/i);
  });

  it("a refresh error after a successful broadcast is not a broadcast failure", async () => {
    vi.spyOn(bridge.signing, "broadcast").mockResolvedValue({ txHash: "0xfeedbeef00", blockNumber: null });
    vi.spyOn(store, "refreshWallet").mockRejectedValue(new Error("balance read failed"));
    await store.approveWalletReview(false);
    expect(lastToast()).toContain("0xfeedbeef");
    expect(lastToast()).toContain("balance read failed");
    expect(lastToast()).not.toMatch(/not sent|outcome unknown/i);
    expect(onResolved).toHaveBeenCalledWith(true);
    expect(store.state.walletReview).toBeNull();
  });

  // Contract v1.1 §1: a NonceTooLow on a first send means the nonce is already used; say so and
  // point the member at their recent transactions before offering another send.
  it("a first-send NonceTooLow says 'nonce already used'", async () => {
    vi.spyOn(store, "refreshWallet").mockResolvedValue(undefined);
    vi.spyOn(bridge.signing, "broadcast").mockRejectedValue(
      "not submitted: ceremony: broadcast failed: rpc node error: Failed to submit transaction: NonceTooLow { expected: 3, got: 0 }",
    );
    await store.approveWalletReview(false);
    expect(lastToast()).toContain("not sent — nonce already used");
    expect(lastToast()).toContain("check your recent transactions");
  });

  it("a genuine pre-submission rejection says not sent", async () => {
    vi.spyOn(store, "refreshWallet").mockResolvedValue(undefined);
    vi.spyOn(bridge.signing, "broadcast").mockRejectedValue("not submitted: ceremony: broadcast failed: rpc node error: nonce too low");
    await store.approveWalletReview(false);
    expect(lastToast()).toContain("not sent");
    expect(lastToast()).toContain("nonce too low");
    expect(lastToast()).not.toContain("not submitted:");
    expect(onResolved).toHaveBeenCalledWith(false);
  });
});
