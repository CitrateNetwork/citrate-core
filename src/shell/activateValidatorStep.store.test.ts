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
      reject: async () => {},
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
import { SignedTxExpired } from "../bridge/types";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";

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
    const opened = await store.requestValidatorActivation();
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
    await store.requestValidatorActivation();
    expect(store.state.walletReview?.label).toContain("10,000 SALT");
    expect(store.state.walletReview?.label).not.toContain("32,000");
  });

  it("records the tx hash on disk BEFORE the send starts", async () => {
    const store = memberStore();
    await store.refreshActivation();
    (store as unknown as { validatorArmed: boolean }).validatorArmed = true;
    await store.requestValidatorActivation();
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
    await store.requestValidatorActivation();
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
    expect(await reopened.requestValidatorActivation()).toBe(false);
    expect(registerValidatorMock.mock.calls.length).toBe(calls);
  });

  it("the node's returned hash must equal the hash S7 saved, or the send is reported as failed", async () => {
    const store = memberStore();
    await store.refreshActivation();
    (store as unknown as { validatorArmed: boolean }).validatorArmed = true;
    await store.requestValidatorActivation();
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
    await store.requestValidatorActivation();
    const resolved = vi.fn();
    const r = store.state.walletReview;
    if (r) store.setState({ walletReview: { ...r, onResolved: resolved } });
    await store.approveWalletReview(false);
    expect(resolved).toHaveBeenCalledWith(true, store.state.s7TxHash ?? "0xfeed");
    expect(resolved).toHaveBeenCalledWith(true, "0xfeed");
  });

  it("a failed send keeps the recorded hash (the chain decides) and reports the failure", async () => {
    const store = memberStore();
    await store.refreshActivation();
    (store as unknown as { validatorArmed: boolean }).validatorArmed = true;
    await store.requestValidatorActivation();
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
    expect(await store.requestValidatorActivation()).toBe(false);
    expect(registerValidatorMock).not.toHaveBeenCalled();
  });

  it("fails closed when the gate is not met: no ceremony is built", async () => {
    chain.current = readyStatus({ membershipActive: false });
    const store = memberStore();
    await store.refreshActivation();
    (store as unknown as { validatorArmed: boolean }).validatorArmed = true;
    const opened = await store.requestValidatorActivation();
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

describe("follow-up: a signed tx that expired before it was sent", () => {
  it("clears the saved hash with a truthful message (nothing was sent)", async () => {
    const store = memberStore();
    await store.refreshActivation();
    (store as unknown as { validatorArmed: boolean }).validatorArmed = true;
    await store.requestValidatorActivation();
    sendPreparedMock.mockRejectedValueOnce(new SignedTxExpired("ceremony: the signed transaction expired before it was sent; nothing was sent"));
    await store.approveWalletReview(false);
    expect(store.state.s7TxHash).toBeNull();
    expect(store.state.s7TxAt).toBeNull();
    expect(persisted().s7TxHash).toBeNull();
    expect(String(store.state.toast ?? "")).toMatch(/nothing was sent/i);
    expect(String(store.state.toast ?? "")).not.toMatch(/not settled/i);
  });

  it("B-1: a node error after the send that QUOTES the expiry sentence keeps the saved hash", async () => {
    const store = memberStore();
    await store.refreshActivation();
    (store as unknown as { validatorArmed: boolean }).validatorArmed = true;
    await store.requestValidatorActivation();
    // DGX's probe: the text is node-supplied; only the structured error type means "not sent".
    sendPreparedMock.mockRejectedValueOnce(
      new Error("ceremony: broadcast failed: rpc error -32000: ceremony: the signed transaction expired before it was sent; nothing was sent"),
    );
    await store.approveWalletReview(false);
    expect(store.state.s7TxHash).toBe("0xfeed");
    expect(persisted().s7TxHash).toBe("0xfeed");
    expect(String(store.state.toast ?? "")).not.toMatch(/You can try again/i);
    expect(String(store.state.toast ?? "")).toMatch(/not settled/i);
  });

  it("any other send failure still keeps the saved hash (it may have been sent)", async () => {
    const store = memberStore();
    await store.refreshActivation();
    (store as unknown as { validatorArmed: boolean }).validatorArmed = true;
    await store.requestValidatorActivation();
    sendPreparedMock.mockRejectedValueOnce(new Error("ceremony: broadcast failed: timeout"));
    await store.approveWalletReview(false);
    expect(store.state.s7TxHash).toBe("0xfeed");
  });
});

describe("follow-up: Try again after a revert resets the read generation", () => {
  it("a read in flight from before Try again cannot bring the reverted tx back", async () => {
    const store = memberStore();
    store.recordActivationTx("0xabc");
    const failed = { hash: "0xabc", included: true, blockNumber: 501, success: false };
    chain.current = readyStatus({ tx: failed });
    await store.refreshActivation();
    expect(activationPhase(store.state.activation, true, store.state.s7TxHash)).toBe("failed");
    // A slow read (asked with the reverted tx) is still in flight when the member retries.
    const stale = deferred<ActivationStatusView>();
    activationStatusMock.mockImplementationOnce(() => stale.promise);
    const r = store.refreshActivation();
    store.onS7RetryAfterRevert();
    stale.resolve(readyStatus({ tx: failed }));
    await r;
    expect(store.state.activation?.tx ?? null).toBeNull();
    expect(activationPhase(store.state.activation, true, store.state.s7TxHash)).not.toBe("failed");
    expect(persisted().s7TxHash).toBeNull(); // written now, not on the debounced timer
  });
});

describe("follow-up: the Node surface's Activate is guarded like S7", () => {
  it("refuses while an activation tx is recorded", async () => {
    const store = memberStore();
    store.setState({ stage: "done" });
    store.recordActivationTx("0xfeed");
    (store as unknown as { validatorArmed: boolean }).validatorArmed = true;
    expect(await store.requestValidatorActivation()).toBe(false);
    expect(registerValidatorMock).not.toHaveBeenCalled();
  });

  it("reads the chain first and refuses when the gate is not met", async () => {
    const store = memberStore();
    store.setState({ stage: "done", activation: null });
    chain.current = readyStatus({ membershipActive: false });
    (store as unknown as { validatorArmed: boolean }).validatorArmed = true;
    expect(await store.requestValidatorActivation()).toBe(false);
    expect(activationStatusMock).toHaveBeenCalled();
    expect(registerValidatorMock).not.toHaveBeenCalled();
  });

  it("when allowed, it saves the hash before sending, exactly as S7 does", async () => {
    const store = memberStore();
    store.setState({ stage: "done", activation: null });
    (store as unknown as { validatorArmed: boolean }).validatorArmed = true;
    expect(await store.requestValidatorActivation()).toBe(true);
    let onDiskAtSend: unknown = "unset";
    sendPreparedMock.mockImplementationOnce(async () => {
      onDiskAtSend = persisted().s7TxHash;
      return { txHash: "0xfeed", blockNumber: 501 };
    });
    await store.approveWalletReview(false);
    expect(prepareMock).toHaveBeenCalledTimes(1);
    expect(onDiskAtSend).toBe("0xfeed");
  });
});

/** Surface files (non-test .ts/.tsx under src/, except the store) that bypass the guarded
 *  activation entry: a direct activateValidator / registerValidator call in any spelling. */
function guardOffenders(sources: [string, string][]): string[] {
  const bypass = [
    /\.activateValidator\b/, // method access, called or not (`store.activateValidator`)
    /\.registerValidator\s*\(/, // the bridge call itself
    /\[\s*["'`](activate|register)Validator["'`]\s*\]/, // bracket access
  ];
  return sources.filter(([rel, src]) => rel !== "shell/store.ts" && bypass.some((re) => re.test(src))).map(([rel]) => rel);
}

/** Store lines that decide a ceremony outcome by matching its prose (a regex literal or a
 *  string search over ceremony text). Outcomes must come from a structured error type. */
function proseDecisions(src: string): string[] {
  const prose = /(ceremony:|nothing was sent|expired before it was sent|broadcast failed)/;
  return src
    .split("\n")
    .filter((l) => !l.trim().startsWith("//") && !l.trim().startsWith("*"))
    .filter((l) => prose.test(l) && (/\/[^/\n]*(ceremony:|nothing was sent|expired before it was sent|broadcast failed)[^/\n]*\/[a-z]*/.test(l) || /\.(includes|startsWith|endsWith|match|search|indexOf)\(/.test(l)));
}

describe("follow-up tripwires", () => {
  const root = join(__dirname, "..");
  const files: string[] = [];
  const walk = (d: string) => {
    for (const f of readdirSync(d)) {
      const p = join(d, f);
      if (statSync(p).isDirectory()) walk(p);
      else if (/\.(ts|tsx)$/.test(f) && !/\.test\.(ts|tsx)$/.test(f)) files.push(p);
    }
  };
  walk(root);
  const rel = (p: string) => p.slice(root.length + 1);

  it("only the store calls activateValidator (every surface goes through the guarded entry)", () => {
    const offenders = files.filter((f) => rel(f) !== "shell/store.ts" && /\.activateValidator\(/.test(readFileSync(f, "utf8")));
    expect(offenders.map(rel)).toEqual([]);
  });

  it("N-4: no surface reaches registerValidator or activateValidator in any spelling", () => {
    const sources: [string, string][] = files.filter((f) => !rel(f).startsWith("bridge/")).map((f) => [rel(f), readFileSync(f, "utf8")]);
    expect(guardOffenders(sources)).toEqual([]);
    // Fixtures: each bypass spelling is flagged.
    expect(guardOffenders([["surfaces/X.tsx", "void bridge.node.registerValidator();"]])).toEqual(["surfaces/X.tsx"]);
    expect(guardOffenders([["surfaces/X.tsx", 'void (store as any)["activateValidator"]();']])).toEqual(["surfaces/X.tsx"]);
    expect(guardOffenders([["surfaces/X.tsx", 'void bridge.node["registerValidator"]();']])).toEqual(["surfaces/X.tsx"]);
    expect(guardOffenders([["shell/store.ts", "bridge.node.registerValidator();"]])).toEqual([]);
  });

  it("B-1 tripwire: the store never decides a ceremony outcome by matching its prose", () => {
    const store = readFileSync(join(root, "shell/store.ts"), "utf8");
    expect(proseDecisions(store)).toEqual([]);
    expect(proseDecisions("if (/nothing was sent/.test(msg)) clear();")).toHaveLength(1);
    expect(proseDecisions('if (msg.includes("ceremony: broadcast failed")) x();')).toHaveLength(1);
    expect(proseDecisions("if (err instanceof SignedTxExpired) clear();")).toHaveLength(0);
  });

  it("only the store writes s7TxHash (no surface resets it around the read generation)", () => {
    const offenders = files.filter((f) => !["shell/store.ts", "shell/state.ts"].includes(rel(f)) && /s7TxHash\s*:/.test(readFileSync(f, "utf8")));
    expect(offenders.map(rel)).toEqual([]);
  });
});
