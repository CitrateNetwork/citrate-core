// Join-8 — the Activate-validator step in the store: resume after quit, the skip path, the
// S6 → S7 hand-off, and NO SILENT SIGNING (Rule 3: activation only OPENS the Signature Ceremony;
// the member approves it there).
import { describe, it, expect, vi, beforeEach } from "vitest";
import type { ActivationStatusView } from "../bridge/domains";

const WEI_32K = (32000n * 10n ** 18n).toString();
const WALLET = "0x1111111111111111111111111111111111111111";

function readyStatus(over: Partial<ActivationStatusView> = {}): ActivationStatusView {
  return {
    bondAddress: "0x2222222222222222222222222222222222222222",
    bondDeployed: true,
    bondActivated: false,
    principalWei: WEI_32K,
    unlockBlock: 16_200_000,
    unlockTimestamp: 1_822_000_000,
    memberTokenId: "4",
    membershipActive: true,
    validatorPubkey: "0x" + "07".repeat(32),
    pubkeySource: "node",
    nodeKeyReady: true,
    nodePubkey: "0x" + "07".repeat(32),
    bondPubkey: null,
    registryStatus: 0,
    activationEpoch: 0,
    registryActive: false,
    currentEpoch: 500,
    epochBlocks: 1000,
    headBlock: 500_000,
    blockTimeMs: 2000,
    localBlock: 499_999,
    localSyncing: false,
    tx: null,
    ...over,
  };
}

const chain = { current: readyStatus() };
const activationStatusMock = vi.fn(async (_member: string, _tx?: string | null) => chain.current);
const registerValidatorMock = vi.fn(async () => ({
  id: "cer_1",
  decoded: { action: "MemberBond.activate", cost: "gas only" },
}));
const armMiningMock = vi.fn(async () => false);
const broadcastMock = vi.fn(async () => ({ txHash: "0xfeed" }));
// Join-8: S7 signs first (prepare returns the hash, nothing sent), records the hash, then sends.
const prepareMock = vi.fn(async () => ({ txHash: "0xfeed" }));
const sendPreparedMock = vi.fn(async (): Promise<{ txHash: string; blockNumber: number | null }> => ({ txHash: "0xfeed", blockNumber: 501 }));
const approveMock = vi.fn(async () => ({}));
const rejectMock = vi.fn(async (_id: string) => {});
const activationForgetMock = vi.fn(async (_hash: string) => true);

vi.mock("../bridge", () => ({
  bindSimHost: () => {},
  bridge: {
    mode: "tauri",
    node: {
      activationStatus: (m: string, tx?: string | null) => activationStatusMock(m, tx),
      registerValidator: () => registerValidatorMock(),
      activationForget: (h: string) => activationForgetMock(h),
      armMining: () => armMiningMock(),
      status: async () => ({ state: "running", height: 1, peers: 1, syncPct: 100 }),
      logs: async () => [],
    },
    signing: {
      broadcast: () => broadcastMock(),
      approve: () => approveMock(),
      prepare: () => prepareMock(),
      sendPrepared: () => sendPreparedMock(),
      reject: (id: string) => rejectMock(id),
    },
    membership: { grantStatus: async () => ({}) },
    auth: { status: async () => ({ signedIn: false }) },
    custody: { status: async () => ({ unlocked: false, autolockMins: 30 }) },
    agentHarness: { recordDecision: async () => ({}) },
  },
}));
vi.mock("../bridge/mode", () => ({ BRIDGE_MODE: "tauri", assertSimAllowed: () => {} }));

import { Store } from "./store";
import { PERSIST_KEYS, STORAGE_KEY } from "./state";
import { activationPhase } from "../onboarding/activateValidator";

async function flush(): Promise<void> {
  for (let i = 0; i < 6; i++) await Promise.resolve();
}
function deferred<T>() {
  let resolve!: (v: T) => void;
  let reject!: (e: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}
const never = () => new Promise<void>(() => {});
function persisted(): Record<string, unknown> {
  return JSON.parse(localStorage.getItem(STORAGE_KEY) || "{}");
}

function memberStore(): Store {
  const store = new Store();
  store.setState({ stage: "s7", hasGrant: true, walletAddr: WALLET, custodyAddr: WALLET, signedIn: true, walletFromClaim: true });
  return store;
}

beforeEach(() => {
  chain.current = readyStatus();
  // Reset (not just clear): a once-queued value a test didn't consume must not leak into the next.
  activationStatusMock.mockReset();
  activationStatusMock.mockImplementation(async () => chain.current);
  registerValidatorMock.mockClear();
  armMiningMock.mockClear();
  broadcastMock.mockClear();
  prepareMock.mockClear();
  sendPreparedMock.mockClear();
  approveMock.mockClear();
  activationForgetMock.mockReset();
  activationForgetMock.mockImplementation(async () => true);
  try {
    localStorage.clear();
  } catch {
    /* jsdom */
  }
});

describe("S6 → S7 hand-off", () => {
  it("a member continues from node ignition into Activate validator", () => {
    const store = memberStore();
    store.setState({ stage: "s6" });
    store.onS6Continue();
    expect(store.state.stage).toBe("s7");
  });
  it("a non-member (no grant) enters the app; there is no bond to activate", () => {
    const store = memberStore();
    store.setState({ stage: "s6", hasGrant: false });
    store.onS6Continue();
    expect(store.state.stage).toBe("done");
  });
});

describe("resume after quit", () => {
  it("persists the step; the sent activation's record is not in localStorage", () => {
    expect(PERSIST_KEYS).toContain("stage");
    // validatorSkipped had a writer and no reader (#442): it is gone, not persisted.
    expect(PERSIST_KEYS as string[]).not.toContain("validatorSkipped");
    // The activation record is #347's fsync'd submitted-tx record in Rust, never localStorage.
    expect(PERSIST_KEYS).not.toContain("s7TxHash");
    expect(PERSIST_KEYS).not.toContain("s7TxAt");
    expect(PERSIST_KEYS).not.toContain("activation"); // chain is the truth, re-read on launch
  });
  it("re-reads the chain with the persisted tx hash, so a sent activation shows as Pending", async () => {
    const store = memberStore();
    store.setState({ s7TxHash: "0xabc" });
    chain.current = readyStatus({ tx: { hash: "0xabc", included: false, blockNumber: null, success: null } });
    await store.refreshActivation();
    expect(activationStatusMock).toHaveBeenCalledWith(WALLET, "0xabc");
    expect(store.state.activation?.tx?.included).toBe(false);
  });
  it("a failed read keeps the last honest status and records the reason (unknown is not zero)", async () => {
    const store = memberStore();
    await store.refreshActivation();
    activationStatusMock.mockRejectedValueOnce(new Error("rpc down"));
    await store.refreshActivation();
    expect(store.state.activation?.bondDeployed).toBe(true);
    expect(store.state.activationError).toContain("rpc down");
  });
  it("forgets the tx hash only once a read OBSERVED that tx's receipt and the bond activated", async () => {
    const store = memberStore();
    store.setState({ s7TxHash: "0xabc" });
    chain.current = readyStatus({ bondActivated: true, registryStatus: 1, tx: { hash: "0xabc", included: true, blockNumber: 501, success: true } });
    await store.refreshActivation();
    expect(store.state.s7TxHash).toBeNull();
  });
  it("keeps a recorded pending tx when the response did not observe its receipt", async () => {
    const store = memberStore();
    store.setState({ s7TxHash: "0xabc" });
    chain.current = readyStatus({ bondActivated: true, registryStatus: 1, tx: null });
    await store.refreshActivation();
    expect(store.state.s7TxHash).toBe("0xabc");
    chain.current = readyStatus({ bondActivated: true, registryStatus: 1, tx: { hash: "0xother", included: true, blockNumber: 501, success: true } });
    await store.refreshActivation();
    expect(store.state.s7TxHash).toBe("0xabc");
  });
});

describe("skip path: Explore free", () => {
  it("enters the app without activating and without changing the member's tier", () => {
    const store = memberStore();
    const tier = store.state.tier;
    store.onS7Skip();
    expect(store.state.stage).toBe("done");
    expect("validatorSkipped" in store.state).toBe(false); // no write-only state
    expect(store.state.tier).toBe(tier);
    expect(registerValidatorMock).not.toHaveBeenCalled();
    expect(store.state.walletReview).toBeNull();
  });
});

describe("no silent signing", () => {
  it("Activate only OPENS the Signature Ceremony; nothing is approved or broadcast", async () => {
    const store = memberStore();
    await store.refreshActivation();
    // The producer is already armed in this session, so activation goes straight to the ceremony.
    (store as unknown as { validatorArmed: boolean }).validatorArmed = true;
    const opened = await store.onS7Activate();
    expect(opened).toBe(true);
    expect(registerValidatorMock).toHaveBeenCalledTimes(1);
    expect(store.state.walletReview?.kind).toBe("stake");
    expect(store.state.walletReview?.view.id).toBe("cer_1");
    expect(broadcastMock).not.toHaveBeenCalled();
    expect(approveMock).not.toHaveBeenCalled();
    expect(prepareMock).not.toHaveBeenCalled();
    expect(sendPreparedMock).not.toHaveBeenCalled();
  });

  it("labels the review with the bond's principal as read, not a hard-coded 32,000", async () => {
    chain.current = readyStatus({ principalWei: (10000n * 10n ** 18n).toString() });
    const store = memberStore();
    await store.refreshActivation();
    (store as unknown as { validatorArmed: boolean }).validatorArmed = true;
    await store.onS7Activate();
    expect(store.state.walletReview?.label).toContain("10,000 SALT");
    expect(store.state.walletReview?.label).not.toContain("32,000");
  });

  it("shows the tx hash as Pending BEFORE the send starts (the send writes the durable record)", async () => {
    const store = memberStore();
    await store.refreshActivation();
    (store as unknown as { validatorArmed: boolean }).validatorArmed = true;
    await store.onS7Activate();
    let shownAtSend: unknown = "unset";
    sendPreparedMock.mockImplementationOnce(async () => {
      shownAtSend = store.state.s7TxHash;
      return { txHash: "0xfeed", blockNumber: 501 };
    });
    await store.approveWalletReview(false);
    expect(prepareMock).toHaveBeenCalledTimes(1);
    expect(sendPreparedMock).toHaveBeenCalledTimes(1);
    expect(broadcastMock).not.toHaveBeenCalled(); // the one-call path would poll before returning the hash
    expect(shownAtSend).toBe("0xfeed");
  });

  it("QUIT DURING RECEIPT POLLING: the send never returns, the relaunch resumes to Pending from the record", async () => {
    const store = memberStore();
    await store.refreshActivation();
    (store as unknown as { validatorArmed: boolean }).validatorArmed = true;
    await store.onS7Activate();
    // sign_send_prepared records durably, sends, then polls for the receipt; the member quits.
    sendPreparedMock.mockImplementationOnce(() => new Promise(() => {}));
    void store.approveWalletReview(false);
    await flush();
    expect(sendPreparedMock).toHaveBeenCalledTimes(1);
    store.save();
    await new Promise((res) => setTimeout(res, 450));
    expect(persisted().stage).toBe("s7");

    const reopened = new Store();
    reopened.setState({ hasGrant: true, walletAddr: WALLET, custodyAddr: WALLET, signedIn: true, walletFromClaim: true });
    expect(reopened.state.stage).toBe("s7");
    // The Rust record (written before the send) names the tx; the read returns it.
    chain.current = readyStatus({
      recordedTx: { hash: "0xfeed", atMs: Date.now() },
      tx: { hash: "0xfeed", included: false, blockNumber: null, success: null },
    } as Partial<ActivationStatusView>);
    await reopened.refreshActivation();
    expect(reopened.state.s7TxHash).toBe("0xfeed");
    expect(activationPhase(reopened.state.activation, true, reopened.state.s7TxHash)).toBe("pending");
    (reopened as unknown as { validatorArmed: boolean }).validatorArmed = true;
    const calls = registerValidatorMock.mock.calls.length;
    expect(await reopened.onS7Activate()).toBe(false);
    expect(registerValidatorMock.mock.calls.length).toBe(calls);
  });

  it("the node's returned hash must equal the hash S7 saved, or the send is reported as failed", async () => {
    const store = memberStore();
    await store.refreshActivation();
    (store as unknown as { validatorArmed: boolean }).validatorArmed = true;
    await store.onS7Activate();
    const resolved = vi.fn();
    const r = store.state.walletReview;
    if (r) store.setState({ walletReview: { ...r, onResolved: resolved } });
    sendPreparedMock.mockResolvedValueOnce({ txHash: "0xother", blockNumber: 501 });
    await store.approveWalletReview(false);
    expect(store.state.s7TxHash).toBe("0xfeed"); // the signed tx stays recorded; the chain decides
    expect(resolved).not.toHaveBeenCalledWith(true, expect.anything());
    expect(String(store.state.toast ?? "")).toMatch(/different transaction hash/i);
  });

  it("a matching returned hash resolves with exactly the saved hash", async () => {
    const store = memberStore();
    await store.refreshActivation();
    (store as unknown as { validatorArmed: boolean }).validatorArmed = true;
    await store.onS7Activate();
    const resolved = vi.fn();
    const r = store.state.walletReview;
    if (r) store.setState({ walletReview: { ...r, onResolved: resolved } });
    await store.approveWalletReview(false);
    expect(resolved).toHaveBeenCalledWith(true, store.state.s7TxHash ?? "0xfeed");
    expect(resolved).toHaveBeenCalledWith(true, "0xfeed");
  });

  it("localStorage failing does not touch the send: the record is written in Rust", async () => {
    const store = memberStore();
    await store.refreshActivation();
    (store as unknown as { validatorArmed: boolean }).validatorArmed = true;
    await store.onS7Activate();
    const setItem = vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new DOMException("quota exceeded", "QuotaExceededError");
    });
    try {
      await store.approveWalletReview(false);
    } finally {
      setItem.mockRestore();
    }
    expect(sendPreparedMock).toHaveBeenCalledTimes(1);
    expect(store.state.s7TxHash).toBe("0xfeed");
  });

  it("a failed send keeps the recorded hash (the chain decides) and reports the failure", async () => {
    const store = memberStore();
    await store.refreshActivation();
    (store as unknown as { validatorArmed: boolean }).validatorArmed = true;
    await store.onS7Activate();
    sendPreparedMock.mockRejectedValueOnce(new Error("rpc timeout"));
    await store.approveWalletReview(false);
    expect(store.state.s7TxHash).toBe("0xfeed");
    expect(store.state.walletReview).toBeNull();
  });

  it("an unconfirmed tx can be forgotten only after it has gone unseen for a while", () => {
    const store = memberStore();
    store.recordActivationTx("0xfeed");
    expect(typeof store.state.s7TxAt).toBe("number");
    expect(store.canForgetActivationTx(Date.now())).toBe(false);
    store.setState({ activation: readyStatus({ tx: { hash: "0xfeed", included: false, blockNumber: null, success: null } }) });
    expect(store.canForgetActivationTx(Date.now() + 11 * 60_000)).toBe(true);
    store.setState({ activation: readyStatus({ tx: { hash: "0xfeed", included: true, blockNumber: 501, success: true } }) });
    expect(store.canForgetActivationTx(Date.now() + 11 * 60_000)).toBe(false);
  });

  it("a pending tx blocks a fresh activation even if a read lacking it says the gate is met", async () => {
    const store = memberStore();
    store.setState({ s7TxHash: "0xfeed" });
    chain.current = readyStatus({ tx: null });
    await store.refreshActivation();
    expect(activationPhase(store.state.activation, true, store.state.s7TxHash)).toBe("pending");
    (store as unknown as { validatorArmed: boolean }).validatorArmed = true;
    expect(await store.onS7Activate()).toBe(false);
    expect(registerValidatorMock).not.toHaveBeenCalled();
  });

  it("fails closed when the gate is not met: no ceremony is built", async () => {
    chain.current = readyStatus({ membershipActive: false });
    const store = memberStore();
    await store.refreshActivation();
    (store as unknown as { validatorArmed: boolean }).validatorArmed = true;
    const opened = await store.onS7Activate();
    expect(opened).toBe(false);
    expect(registerValidatorMock).not.toHaveBeenCalled();
    expect(store.state.walletReview).toBeNull();
  });

  it("node ignition no longer opens the activation review on its own", () => {
    const store = memberStore();
    store.setState({ stage: "s6", node: "synced" });
    (store as unknown as { validatorArmed: boolean }).validatorArmed = true;
    (store as unknown as { maybeAutoBond: () => void }).maybeAutoBond();
    expect(registerValidatorMock).not.toHaveBeenCalled();
    expect(store.state.walletReview).toBeNull();
  });
});

describe("out-of-order activation reads", () => {
  it("a slower, older response never overwrites a newer one", async () => {
    const store = memberStore();
    const older = deferred<ActivationStatusView>();
    const newer = deferred<ActivationStatusView>();
    activationStatusMock.mockImplementationOnce(() => older.promise).mockImplementationOnce(() => newer.promise);
    const r1 = store.refreshActivation();
    const r2 = store.refreshActivation();
    newer.resolve(readyStatus({ bondActivated: true, registryStatus: 1, registryActive: true, activationEpoch: 502 }));
    await r2;
    older.resolve(readyStatus({ bondActivated: false }));
    await r1;
    expect(store.state.activation?.bondActivated).toBe(true);
  });

  it("a stale failure does not mark the newer read as failed", async () => {
    const store = memberStore();
    const older = deferred<ActivationStatusView>();
    activationStatusMock.mockImplementationOnce(() => older.promise);
    const r1 = store.refreshActivation();
    await store.refreshActivation();
    older.reject(new Error("timeout"));
    await r1;
    expect(store.state.activationError).toBeNull();
    expect(store.state.activation?.bondDeployed).toBe(true);
  });

  it("a read sent before the tx was recorded cannot clear or bypass it", async () => {
    const store = memberStore();
    const inflight = deferred<ActivationStatusView>();
    activationStatusMock.mockImplementationOnce(() => inflight.promise);
    const r1 = store.refreshActivation();
    // The broadcast is accepted while that read is in flight.
    store.recordActivationTx("0xfeed");
    inflight.resolve(readyStatus({ bondActivated: true, registryStatus: 1, tx: null }));
    await r1;
    expect(store.state.s7TxHash).toBe("0xfeed");
    expect(store.state.activation).toBeNull(); // the stale read was ignored
  });
});

describe("single-flight periodic reads (a slow read is still applied)", () => {
  it("a periodic read does not start while one is in flight, and the slow one lands", async () => {
    const store = memberStore();
    const slow = deferred<ActivationStatusView>();
    activationStatusMock.mockImplementationOnce(() => slow.promise);
    const first = store.refreshActivation("poll");
    // Three more 15 s ticks while the backend is still reading (~16 RPCs, 6 s timeouts each).
    await store.refreshActivation("poll");
    await store.refreshActivation("poll");
    await store.refreshActivation("poll");
    expect(activationStatusMock).toHaveBeenCalledTimes(1);
    slow.resolve(readyStatus({ bondActivated: true, registryStatus: 1, registryActive: true, activationEpoch: 502 }));
    await first;
    expect(store.state.activation?.bondActivated).toBe(true);
    // Once it lands, the next tick reads again.
    await store.refreshActivation("poll");
    expect(activationStatusMock).toHaveBeenCalledTimes(2);
  });

  it("an explicit event still supersedes an in-flight read", async () => {
    const store = memberStore();
    const slow = deferred<ActivationStatusView>();
    activationStatusMock.mockImplementationOnce(() => slow.promise);
    const first = store.refreshActivation("poll");
    await store.refreshActivation(); // mount / hand-off / broadcast
    expect(activationStatusMock).toHaveBeenCalledTimes(2);
    slow.resolve(readyStatus({ bondActivated: false, membershipActive: false }));
    await first;
    expect(store.state.activation?.membershipActive).toBe(true);
  });
});

describe("a recorded activation guards every entry, not only S7", () => {
  it("pending activation → Explore free → Node's Activate opens no second ceremony", async () => {
    const store = memberStore();
    store.recordActivationTx("0xfeed");
    chain.current = readyStatus({ tx: { hash: "0xfeed", included: false, blockNumber: null, success: null } });
    store.onS7Skip();
    (store as unknown as { validatorArmed: boolean }).validatorArmed = true;
    expect(await store.activateValidator()).toBe(false);
    expect(registerValidatorMock).not.toHaveBeenCalled();
    expect(store.state.walletReview).toBeNull();
    expect(store.state.s7TxHash).toBe("0xfeed");
    // It reconciled the recorded tx with the chain (asked with that hash) before refusing.
    expect(activationStatusMock).toHaveBeenLastCalledWith(WALLET, "0xfeed");
  });

  it("once the chain shows the recorded tx included and the bond activated, Node still opens nothing", async () => {
    const store = memberStore();
    store.recordActivationTx("0xfeed");
    store.onS7Skip();
    chain.current = readyStatus({ bondActivated: true, registryStatus: 1, tx: { hash: "0xfeed", included: true, blockNumber: 501, success: true } });
    (store as unknown as { validatorArmed: boolean }).validatorArmed = true;
    expect(await store.activateValidator()).toBe(false);
    expect(store.state.s7TxHash).toBeNull(); // reconciled
    expect(registerValidatorMock).not.toHaveBeenCalled();
  });
});

describe("tripwire: the recorded-tx guard lives in the shared entry", () => {
  it("activateValidator checks s7TxHash before it can build a ceremony", async () => {
    const { readFileSync } = await import("node:fs");
    const { join } = await import("node:path");
    const src = readFileSync(join(__dirname, "store.ts"), "utf8");
    const start = src.search(/async activateValidator\(/);
    expect(start).toBeGreaterThan(-1);
    const body = src.slice(start);
    const guard = body.indexOf("this.state.s7TxHash");
    const build = body.indexOf("bridge.node.registerValidator");
    expect(guard).toBeGreaterThan(-1);
    expect(build).toBeGreaterThan(-1);
    expect(guard).toBeLessThan(build);
    // The reconciling read is unconditional (not inside the s7TxHash check) and gates the build:
    // after a restart nothing is in memory, and only that read brings back a recorded send.
    const read = body.indexOf("if (!(await this.refreshActivation()))");
    expect(read).toBeGreaterThan(-1);
    expect(read).toBeLessThan(guard);
  });
});

describe("the activation record is the durable Rust record (#347), not localStorage", () => {
  it("a relaunch with storage cleared resumes to Pending from the recorded send", async () => {
    const at = Date.now() - 60_000;
    try {
      localStorage.clear();
    } catch {
      /* jsdom */
    }
    const reopened = memberStore();
    expect(reopened.state.s7TxHash ?? null).toBeNull();
    chain.current = readyStatus({
      recordedTx: { hash: "0xfeed", atMs: at },
      tx: { hash: "0xfeed", included: false, blockNumber: null, success: null },
    } as Partial<ActivationStatusView>);
    await reopened.refreshActivation();
    expect(reopened.state.s7TxHash).toBe("0xfeed");
    expect(reopened.state.s7TxAt).toBe(at);
    expect(activationPhase(reopened.state.activation, true, reopened.state.s7TxHash)).toBe("pending");
    (reopened as unknown as { validatorArmed: boolean }).validatorArmed = true;
    expect(await reopened.onS7Activate()).toBe(false);
    expect(registerValidatorMock).not.toHaveBeenCalled();
  });

  it("a recorded send the chain shows mined with the bond activated is not brought back", async () => {
    const store = memberStore();
    chain.current = readyStatus({
      bondActivated: true,
      registryStatus: 1,
      recordedTx: { hash: "0xfeed", atMs: Date.now() },
      tx: { hash: "0xfeed", included: true, blockNumber: 501, success: true },
    } as Partial<ActivationStatusView>);
    await store.refreshActivation();
    expect(store.state.s7TxHash ?? null).toBeNull();
  });

  it("the activation hash is not written to localStorage", async () => {
    const store = memberStore();
    await store.refreshActivation();
    (store as unknown as { validatorArmed: boolean }).validatorArmed = true;
    await store.onS7Activate();
    await store.approveWalletReview(false);
    expect(store.state.s7TxHash).toBe("0xfeed");
    (store as unknown as { writePersisted(): void }).writePersisted();
    expect(persisted().s7TxHash).toBeUndefined();
    expect(persisted().s7TxAt).toBeUndefined();
  });

  it("a send the backend refused before submission (no durable record) leaves nothing recorded", async () => {
    const store = memberStore();
    await store.refreshActivation();
    (store as unknown as { validatorArmed: boolean }).validatorArmed = true;
    await store.onS7Activate();
    sendPreparedMock.mockRejectedValueOnce(
      new Error("not submitted: ceremony: broadcast failed: the submission could not be recorded durably, so nothing was sent: disk full"),
    );
    await store.approveWalletReview(false);
    expect(store.state.s7TxHash ?? null).toBeNull(); // nothing was sent: activation is offered again
    expect(String(store.state.toast ?? "")).toMatch(/not sent/i);
  });

  it("Forget writes the durable record first; if that write fails, the hash is kept", async () => {
    const store = memberStore();
    store.recordActivationTx("0xfeed");
    store.setState({
      s7TxAt: Date.now() - 11 * 60_000,
      activation: readyStatus({ tx: { hash: "0xfeed", included: false, blockNumber: null, success: null } }),
    });
    activationForgetMock.mockRejectedValueOnce(new Error("could not write submitted-tx.json"));
    await store.onS7ForgetTx();
    expect(activationForgetMock).toHaveBeenCalledWith("0xfeed");
    expect(store.state.s7TxHash).toBe("0xfeed");
    await store.onS7ForgetTx();
    expect(store.state.s7TxHash ?? null).toBeNull();
  });
});

describe("Try again after a revert", () => {
  it("forgets the reverted send in the durable record, so a later read does not bring it back", async () => {
    const store = memberStore();
    store.recordActivationTx("0xdead");
    const reverted = { hash: "0xdead", included: true, blockNumber: 501, success: false };
    chain.current = readyStatus({ tx: reverted, recordedTx: { hash: "0xdead", atMs: Date.now() } } as Partial<ActivationStatusView>);
    await store.refreshActivation();
    expect(activationPhase(store.state.activation, true, store.state.s7TxHash)).toBe("failed");
    await store.onS7RetryAfterRevert();
    expect(activationForgetMock).toHaveBeenCalledWith("0xdead");
    expect(store.state.s7TxHash ?? null).toBeNull();
    chain.current = readyStatus({ recordedTx: null } as Partial<ActivationStatusView>); // forgotten in the record
    await store.refreshActivation();
    expect(store.state.s7TxHash ?? null).toBeNull();
    expect(activationPhase(store.state.activation, true, store.state.s7TxHash)).not.toBe("failed");
  });

  it("does nothing for a tx that has not reverted", async () => {
    const store = memberStore();
    store.recordActivationTx("0xfeed");
    chain.current = readyStatus({ tx: { hash: "0xfeed", included: false, blockNumber: null, success: null } });
    await store.refreshActivation();
    await store.onS7RetryAfterRevert();
    expect(activationForgetMock).not.toHaveBeenCalled();
    expect(store.state.s7TxHash).toBe("0xfeed");
  });
});

describe("Node's Activate reconciles the durable record on every entry (Codex P2 at f98f8ae)", () => {
  function exploredFree(): Store {
    const store = new Store();
    store.setState({ stage: "done", hasGrant: true, walletAddr: WALLET, custodyAddr: WALLET, signedIn: true, walletFromClaim: true });
    (store as unknown as { validatorArmed: boolean }).validatorArmed = true;
    return store;
  }

  it("restart after Explore free with a pending recorded send: Node opens no second ceremony, without a prior read", async () => {
    const store = exploredFree();
    expect(store.state.s7TxHash ?? null).toBeNull(); // nothing in memory after the restart
    chain.current = readyStatus({
      recordedTx: { hash: "0xfeed", atMs: Date.now() },
      tx: { hash: "0xfeed", included: false, blockNumber: null, success: null },
    } as Partial<ActivationStatusView>);
    expect(await store.activateValidator()).toBe(false);
    expect(activationStatusMock).toHaveBeenCalled();
    expect(registerValidatorMock).not.toHaveBeenCalled();
    expect(store.state.s7TxHash).toBe("0xfeed");
  });

  it("if that reconciling read fails, Node opens nothing (a record may exist: fail closed)", async () => {
    const store = exploredFree();
    activationStatusMock.mockRejectedValueOnce(new Error("rpc timeout"));
    expect(await store.activateValidator()).toBe(false);
    expect(registerValidatorMock).not.toHaveBeenCalled();
    expect(store.state.walletReview).toBeNull();
  });

  it("with no recorded send and a fresh read, Node still opens the ceremony", async () => {
    const store = exploredFree();
    chain.current = readyStatus({ recordedTx: null } as Partial<ActivationStatusView>);
    expect(await store.activateValidator()).toBe(true);
    expect(registerValidatorMock).toHaveBeenCalledTimes(1);
  });
});

describe("#405: the fresh read re-runs the FULL gate before any ceremony is built", () => {
  const blocked: [string, Partial<ActivationStatusView>][] = [
    ["membership lapsed", { membershipActive: false }],
    ["membership unread", { membershipActive: null }],
    ["local node still syncing", { localSyncing: true }],
    ["local node out of band (behind)", { localBlock: 400_000 }],
    ["local node out of band (ahead)", { localBlock: 600_000 }],
    ["local height unread", { localBlock: null }],
    ["bond not deployed", { bondDeployed: false }],
    ["bond unread", { bondDeployed: null }],
    ["key already registered", { registryStatus: 1 }],
  ];
  for (const [name, over] of blocked) {
    it(`a ${name} on the fresh read builds no ceremony, even after an earlier ready read`, async () => {
      const store = memberStore();
      await store.refreshActivation(); // the earlier read: ready
      (store as unknown as { validatorArmed: boolean }).validatorArmed = true;
      chain.current = readyStatus(over);
      expect(await store.activateValidator()).toBe(false);
      expect(registerValidatorMock).not.toHaveBeenCalled();
      expect(store.state.walletReview).toBeNull();
    });
  }

  it("a ready fresh read still builds the ceremony", async () => {
    const store = memberStore();
    (store as unknown as { validatorArmed: boolean }).validatorArmed = true;
    expect(await store.activateValidator()).toBe(true);
    expect(registerValidatorMock).toHaveBeenCalledTimes(1);
  });

  it("no key yet and the producer unarmed: it arms (signs nothing) and builds no ceremony", async () => {
    const store = memberStore();
    chain.current = readyStatus({ nodeKeyReady: false, nodePubkey: null, validatorPubkey: null });
    armMiningMock.mockResolvedValueOnce(true);
    expect(await store.activateValidator()).toBe(false);
    expect(armMiningMock).toHaveBeenCalledTimes(1);
    expect(registerValidatorMock).not.toHaveBeenCalled();
  });

  it("no key yet but already armed: it waits for the key and builds no ceremony", async () => {
    const store = memberStore();
    chain.current = readyStatus({ nodeKeyReady: false, nodePubkey: null, validatorPubkey: null });
    (store as unknown as { validatorArmed: boolean }).validatorArmed = true;
    expect(await store.activateValidator()).toBe(false);
    expect(registerValidatorMock).not.toHaveBeenCalled();
  });

  it("a key-less node whose other checks fail is never armed", async () => {
    const store = memberStore();
    chain.current = readyStatus({ nodeKeyReady: false, nodePubkey: null, validatorPubkey: null, membershipActive: false });
    expect(await store.activateValidator()).toBe(false);
    expect(armMiningMock).not.toHaveBeenCalled();
  });
});

describe("tripwire (#405): no ceremony build after a fresh read without a gate re-check", () => {
  it("activateValidator runs activationGate on the fresh read before registerValidator", async () => {
    const { readFileSync } = await import("node:fs");
    const { join } = await import("node:path");
    const src = readFileSync(join(__dirname, "store.ts"), "utf8");
    const start = src.search(/async activateValidator\(/);
    const body = src.slice(start);
    const read = body.indexOf("await this.refreshActivation()");
    const gate = body.indexOf("activationGate(this.state.activation");
    const arm = body.indexOf("bridge.node.armMining");
    const build = body.indexOf("bridge.node.registerValidator");
    expect(read).toBeGreaterThan(-1);
    expect(gate).toBeGreaterThan(read);
    expect(gate).toBeLessThan(arm);
    expect(gate).toBeLessThan(build);
  });
});
