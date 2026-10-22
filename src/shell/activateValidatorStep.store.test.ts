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

vi.mock("../bridge", () => ({
  bindSimHost: () => {},
  bridge: {
    mode: "tauri",
    node: {
      activationStatus: (m: string, tx?: string | null) => activationStatusMock(m, tx),
      registerValidator: () => registerValidatorMock(),
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
  activationStatusMock.mockClear();
  registerValidatorMock.mockClear();
  armMiningMock.mockClear();
  broadcastMock.mockClear();
  prepareMock.mockClear();
  sendPreparedMock.mockClear();
  approveMock.mockClear();
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
  it("persists the step and the sent activation's tx hash", () => {
    expect(PERSIST_KEYS).toContain("stage");
    expect(PERSIST_KEYS).toContain("s7TxHash");
    expect(PERSIST_KEYS).toContain("validatorSkipped");
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
    expect(store.state.validatorSkipped).toBe(true);
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

  it("records the tx hash on disk BEFORE the send starts", async () => {
    const store = memberStore();
    await store.refreshActivation();
    (store as unknown as { validatorArmed: boolean }).validatorArmed = true;
    await store.onS7Activate();
    let onDiskAtSend: unknown = "unset";
    sendPreparedMock.mockImplementationOnce(async () => {
      onDiskAtSend = persisted().s7TxHash;
      return { txHash: "0xfeed", blockNumber: 501 };
    });
    await store.approveWalletReview(false);
    expect(prepareMock).toHaveBeenCalledTimes(1);
    expect(sendPreparedMock).toHaveBeenCalledTimes(1);
    expect(broadcastMock).not.toHaveBeenCalled(); // the one-call path would poll before returning the hash
    expect(onDiskAtSend).toBe("0xfeed");
  });

  it("QUIT DURING RECEIPT POLLING: the send never returns, the relaunch resumes to Pending", async () => {
    const store = memberStore();
    await store.refreshActivation();
    (store as unknown as { validatorArmed: boolean }).validatorArmed = true;
    await store.onS7Activate();
    // sign_send_prepared polls for the receipt before returning; the member quits meanwhile.
    sendPreparedMock.mockImplementationOnce(() => new Promise(() => {}));
    void store.approveWalletReview(false);
    await flush();
    expect(sendPreparedMock).toHaveBeenCalledTimes(1);
    expect(persisted().s7TxHash).toBe("0xfeed");
    expect(persisted().stage).toBe("s7");

    const reopened = new Store();
    reopened.setState({ hasGrant: true, walletAddr: WALLET, custodyAddr: WALLET, signedIn: true, walletFromClaim: true });
    expect(reopened.state.stage).toBe("s7");
    expect(reopened.state.s7TxHash).toBe("0xfeed");
    chain.current = readyStatus({ tx: { hash: "0xfeed", included: false, blockNumber: null, success: null } });
    await reopened.refreshActivation();
    expect(activationStatusMock).toHaveBeenLastCalledWith(WALLET, "0xfeed");
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

  it("STORAGE FAILS: the hash can't be saved, so nothing is sent and the signed tx is discarded", async () => {
    const store = memberStore();
    await store.refreshActivation();
    (store as unknown as { validatorArmed: boolean }).validatorArmed = true;
    await store.onS7Activate();
    const resolved = vi.fn();
    const r = store.state.walletReview;
    if (r) store.setState({ walletReview: { ...r, onResolved: resolved } });
    rejectMock.mockClear();
    const setItem = vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new DOMException("quota exceeded", "QuotaExceededError");
    });
    try {
      await store.approveWalletReview(false);
    } finally {
      setItem.mockRestore();
    }
    expect(prepareMock).toHaveBeenCalledTimes(1);
    expect(sendPreparedMock).not.toHaveBeenCalled();
    expect(rejectMock).toHaveBeenCalledWith("cer_1"); // the prepared (signed, unsent) tx is dropped
    expect(store.state.s7TxHash).toBeNull(); // nothing was sent: the member is not blocked
    expect(store.state.walletReview).toBeNull();
    expect(resolved).toHaveBeenCalledWith(false);
    expect(String(store.state.toast ?? "")).toMatch(/nothing was sent/i);
  });

  it("STORAGE SILENTLY DROPS THE WRITE: a save that can't be read back counts as failed", async () => {
    const store = memberStore();
    await store.refreshActivation();
    (store as unknown as { validatorArmed: boolean }).validatorArmed = true;
    await store.onS7Activate();
    const setItem = vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {});
    try {
      await store.approveWalletReview(false);
    } finally {
      setItem.mockRestore();
    }
    expect(sendPreparedMock).not.toHaveBeenCalled();
    expect(store.state.s7TxHash).toBeNull();
  });

  it("after a storage failure, activation can be offered again once storage works", async () => {
    const store = memberStore();
    await store.refreshActivation();
    (store as unknown as { validatorArmed: boolean }).validatorArmed = true;
    await store.onS7Activate();
    const setItem = vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("storage unavailable");
    });
    try {
      await store.approveWalletReview(false);
    } finally {
      setItem.mockRestore();
    }
    expect(sendPreparedMock).not.toHaveBeenCalled();
    expect(await store.onS7Activate()).toBe(true);
    await store.approveWalletReview(false);
    expect(sendPreparedMock).toHaveBeenCalledTimes(1);
    expect(persisted().s7TxHash).toBe("0xfeed");
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

describe("tripwire: a signed tx is sent only after its hash is durably saved", () => {
  it("every onSigned call in the store gates the send on its result", async () => {
    const { readFileSync } = await import("node:fs");
    const { join } = await import("node:path");
    const src = readFileSync(join(__dirname, "store.ts"), "utf8");
    const calls = [...src.matchAll(/\bonSigned\s*\(/g)].map((m) => src.slice(Math.max(0, m.index - 12), m.index + 9));
    expect(calls.length).toBeGreaterThan(0);
    for (const c of calls) expect(c).toMatch(/if \(!r\.onSigned\(/);
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
