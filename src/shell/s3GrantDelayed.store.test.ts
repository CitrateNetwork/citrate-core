// The S3 delayed-grant wiring in the Store, under the tauri branch (onS3Pay → pollMembership).
//
// s3GrantDelayed.test.tsx pins the card and the pure isGrantDelayed decision, but under vitest
// BRIDGE_MODE is not "tauri", so the real pollMembership returns at once and nothing there runs
// the per-tick wiring. This file mocks the bridge + BRIDGE_MODE (as onS3Pay.test.ts does) and
// drives the 5 s cadence with fake timers (Date included), so it fails if store.ts loses:
//   - the signer-health read on the poll (readSignerHealth → s3SignerReady),
//   - the per-tick re-decision (updateGrantDelayed), by signer state and by elapsed time,
//   - the reset in onS3Pay (s3GrantDelayed / s3SignerReady / the checkout time),
//   - the poll generation guard (Check again ends the running loop: one loop at a time),
//   - the settle clearing the delayed flag.
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

type GrantShape = { attributedStakeWei: string; hasSbt: boolean };
const NOT_GRANTED: GrantShape = { attributedStakeWei: "0", hasSbt: false };
const GRANTED: GrantShape = { attributedStakeWei: (32000n * 10n ** 18n).toString(), hasSbt: true };
const grantState = { current: NOT_GRANTED as GrantShape };
type Health = { signerReady: boolean | null; signerState: string | null };
const health = { current: { signerReady: true, signerState: "ready" } as Health | Error };

const checkoutMock = vi.fn(async () => {});
const grantStatusMock = vi.fn(async () => grantState.current);
const signerHealthMock = vi.fn(async () => {
  if (health.current instanceof Error) throw health.current;
  return health.current;
});

vi.mock("../bridge", () => ({
  bindSimHost: () => {},
  bridge: {
    mode: "tauri",
    membership: {
      checkout: () => checkoutMock(),
      grantStatus: () => grantStatusMock(),
      signerHealth: () => signerHealthMock(),
    },
    auth: {
      status: async () => ({ signedIn: false }),
      userinfo: async () => ({ signedIn: true, sub: "usr_x", tier: "pilot", walletAddr: "0xabc" }),
    },
    custody: { status: async () => ({ unlocked: false, autolockMins: 30 }) },
  },
}));

vi.mock("../bridge/mode", () => ({ BRIDGE_MODE: "tauri", assertSimAllowed: () => {} }));

import { Store } from "./store";

async function flush(): Promise<void> {
  for (let i = 0; i < 6; i++) await Promise.resolve();
}

/** Advance the fake clock in 5 s steps (the fast poll cadence), flushing each tick's reads. */
async function advance(ms: number): Promise<void> {
  for (let t = 0; t < ms; t += 5_000) {
    await vi.advanceTimersByTimeAsync(Math.min(5_000, ms - t));
    await flush();
  }
}

async function payingStore(): Promise<Store> {
  const store = new Store();
  store.setState({ walletAddr: "0xabc", custodyAddr: "0xabc" });
  store.onS3Pay();
  await flush();
  expect(store.state.s3).toBe("paying");
  return store;
}

describe("Store S3 delayed-grant wiring (tauri)", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-11-17T12:00:00Z"));
    checkoutMock.mockClear();
    grantStatusMock.mockClear();
    signerHealthMock.mockClear();
    grantState.current = NOT_GRANTED;
    health.current = { signerReady: true, signerState: "ready" };
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("a not-ready signer read on the first tick shows the delayed card at once", async () => {
    health.current = { signerReady: false, signerState: "wrong_signer" };
    const store = await payingStore();
    expect(store.state.s3GrantDelayed).toBe(false);
    await advance(5_000);
    expect(signerHealthMock).toHaveBeenCalled();
    expect(store.state.s3SignerReady).toBe(false);
    expect(store.state.s3GrantDelayed).toBe(true);
    expect(store.state.s3).toBe("paying"); // the signer read never settles S3
  });

  it("a ready signer and no grant: not delayed before 10 minutes, delayed at 10", async () => {
    const store = await payingStore();
    await advance(9 * 60_000);
    expect(store.state.s3SignerReady).toBe(true);
    expect(store.state.s3GrantDelayed).toBe(false);
    await advance(90_000); // past 10 min; the slow cadence is 30 s
    expect(store.state.s3GrantDelayed).toBe(true);
    expect(store.state.s3).toBe("paying");
  });

  it("an unreadable signer: the poll tick alone shows the delayed card at 10 minutes", async () => {
    health.current = new Error("health unreachable");
    const store = await payingStore();
    await advance(9 * 60_000);
    expect(store.state.s3SignerReady).toBe(null);
    expect(store.state.s3GrantDelayed).toBe(false);
    await advance(90_000);
    expect(store.state.s3GrantDelayed).toBe(true);
  });

  it("a failed signer read keeps the last read (no read is not evidence)", async () => {
    health.current = { signerReady: false, signerState: "unreachable" };
    const store = await payingStore();
    await advance(5_000);
    expect(store.state.s3SignerReady).toBe(false);
    health.current = new Error("health down");
    await advance(30_000); // the next read (each 6th fast tick) fails
    expect(signerHealthMock.mock.calls.length).toBeGreaterThanOrEqual(2);
    expect(store.state.s3SignerReady).toBe(false);
    expect(store.state.s3GrantDelayed).toBe(true);
  });

  it("the signer recovering clears the delayed card inside the 10-minute window", async () => {
    health.current = { signerReady: false, signerState: "underfunded" };
    const store = await payingStore();
    await advance(5_000);
    expect(store.state.s3GrantDelayed).toBe(true);
    health.current = { signerReady: true, signerState: "ready" };
    await advance(30_000);
    expect(store.state.s3SignerReady).toBe(true);
    expect(store.state.s3GrantDelayed).toBe(false);
  });

  it("a grant on chain settles S3 and clears the delayed flag", async () => {
    health.current = { signerReady: false, signerState: "wrong_signer" };
    const store = await payingStore();
    await advance(5_000);
    expect(store.state.s3GrantDelayed).toBe(true);
    grantState.current = GRANTED;
    await advance(5_000);
    expect(store.state.s3).toBe("settled");
    expect(store.state.s3GrantDelayed).toBe(false);
  });

  it("onS3Pay resets the delayed flag, the signer read and the checkout clock", async () => {
    const store = await payingStore();
    await advance(9 * 60_000); // 9 min into the first checkout
    expect(store.state.s3GrantDelayed).toBe(false);
    // A stale delayed state from the earlier attempt must not carry over.
    store.setState({ s3GrantDelayed: true, s3SignerReady: false });
    store.onS3Pay();
    await flush();
    expect(store.state.s3).toBe("paying");
    expect(store.state.s3GrantDelayed).toBe(false);
    expect(store.state.s3SignerReady).toBe(null);
    // 11 min after the first checkout but 2 min after this one: the clock restarted.
    await advance(2 * 60_000);
    expect(store.state.s3GrantDelayed).toBe(false);
  });

  it("Check again ends the running poll: one loop at a time", async () => {
    const store = await payingStore();
    await advance(12_500); // two ticks in, mid-interval
    // The money-guard read before checkout, then one read per tick.
    expect(grantStatusMock).toHaveBeenCalledTimes(3);
    store.recheckMembership();
    grantStatusMock.mockClear();
    await advance(20_000);
    // One loop at 5 s = 4 reads in 20 s. Two loops (the old one not ended) would read 8.
    expect(grantStatusMock).toHaveBeenCalledTimes(4);
    expect(store.state.s3).toBe("paying");
  });
});
