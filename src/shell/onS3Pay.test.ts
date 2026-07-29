// CORE-D3.C — onS3Pay wiring, the Rule-1 property under test.
//
// In the TAURI path, onS3Pay opens the REAL core-membership checkout popup
// (bridge.membership.checkout) then POLLS the on-chain membership grant
// (bridge.membership.grantStatus) until it lands. It must:
//   1. open the checkout popup (invoke the real bridge method),
//   2. set s3="paying",
//   3. NOT settle while the on-chain grant is absent — EVEN IF the KYC entitlement
//      is already a paid+active tier (the bug this fix closes: a verified member
//      holds `commercial.kyc`→`pilot` BEFORE paying, so an entitlement-only gate
//      let them walk past S3 unpaid),
//   4. settle (s3="settled") ONLY once the REAL on-chain grant (SBT + >=32k stake)
//      is present.
// The SIM path keeps the prototype's fake settle so web-dev onboarding still walks.
//
// We mock the bridge + BRIDGE_MODE so the Store's tauri branch runs headless, and
// drive the 5s poll cadence with fake timers.
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

// The evolving on-chain grant the mocked bridge returns. Tests flip `current` to
// model the server-side grant landing on chain.
type GrantShape = { attributedStakeWei: string; hasSbt: boolean; bondedStakeWei?: string; nativeBalanceWei?: string };
const NOT_GRANTED: GrantShape = { attributedStakeWei: "0", hasSbt: false };
const GRANTED: GrantShape = { attributedStakeWei: (32000n * 10n ** 18n).toString(), hasSbt: true };
const grantState = { current: NOT_GRANTED as GrantShape };

// A /userinfo that is ALREADY a paid+active tier (a KYC-verified member) — present
// from the first poll to prove S3 does NOT settle on the entitlement alone.
const userinfoState = {
  current: {
    signedIn: true,
    sub: "usr_x",
    tier: "pilot", // paid tier ACTIVE pre-grant (the commercial.kyc→pilot baseline)
    org: null,
    role: "member",
    kycStatus: "verified",
    walletAddr: "0xabc",
    expiresAt: "2999-01-01",
    email: "d@example.com",
  },
};

const checkoutMock = vi.fn(async () => {});
const grantStatusMock = vi.fn(async () => grantState.current);
const userinfoMock = vi.fn(async () => userinfoState.current);
const statusMock = vi.fn(async () => ({ signedIn: false }));

vi.mock("../bridge", () => ({
  bindSimHost: () => {},
  bridge: {
    mode: "tauri",
    membership: {
      checkout: () => checkoutMock(),
      grantStatus: (addr: string) => grantStatusMock(addr),
    },
    auth: {
      status: () => statusMock(),
      userinfo: () => userinfoMock(),
    },
    custody: { status: async () => ({ unlocked: false, autolockMins: 30 }) },
  },
}));

// Force the tauri branch of onS3Pay.
vi.mock("../bridge/mode", () => ({
  BRIDGE_MODE: "tauri",
  assertSimAllowed: () => {},
}));

import { Store } from "./store";

// Flush pending microtasks (awaited promises inside the poll tick).
async function flush(): Promise<void> {
  await Promise.resolve();
  await Promise.resolve();
  await Promise.resolve();
  await Promise.resolve();
}

describe("onS3Pay (tauri) — opens checkout, polls the on-chain grant, settles ONLY on the real grant", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    checkoutMock.mockClear();
    grantStatusMock.mockClear();
    userinfoMock.mockClear();
    grantState.current = NOT_GRANTED;
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("opens the checkout popup and enters 'paying'", async () => {
    const store = new Store();
    // Precondition (c1513cb link gate): onS3Pay only proceeds to "paying" once the
    // device wallet is LINKED (claim === custody). Provisioning + linking is the
    // seamless step ahead of pay; here we assert the pay/poll/settle logic, so we
    // start from a linked wallet.
    store.setState({ walletAddr: "0xabc", custodyAddr: "0xabc" });
    store.onS3Pay();
    expect(store.state.s3).toBe("paying");
    await flush();
    expect(checkoutMock).toHaveBeenCalledTimes(1);
  });

  it("REGRESSION: a paid+active KYC entitlement with NO on-chain grant does NOT settle (no advancing past S3 unpaid)", async () => {
    // /userinfo is already tier:pilot + active (a KYC-verified member) but the
    // on-chain grant has NOT landed — S3 must stay "paying".
    const store = new Store();
    // Precondition (c1513cb link gate): onS3Pay only proceeds to "paying" once the
    // device wallet is LINKED (claim === custody). Provisioning + linking is the
    // seamless step ahead of pay; here we assert the pay/poll/settle logic, so we
    // start from a linked wallet.
    store.setState({ walletAddr: "0xabc", custodyAddr: "0xabc" });
    store.onS3Pay();
    for (let i = 0; i < 3; i++) {
      await vi.advanceTimersByTimeAsync(5000);
      await flush();
    }
    // The grant WAS polled, the entitlement IS active, yet s3 never advanced.
    expect(grantStatusMock).toHaveBeenCalled();
    expect(store.state.s3).toBe("paying");
  });

  it("settles ONLY once the on-chain grant (SBT + >=32k stake) lands", async () => {
    const store = new Store();
    // Precondition (c1513cb link gate): onS3Pay only proceeds to "paying" once the
    // device wallet is LINKED (claim === custody). Provisioning + linking is the
    // seamless step ahead of pay; here we assert the pay/poll/settle logic, so we
    // start from a linked wallet.
    store.setState({ walletAddr: "0xabc", custodyAddr: "0xabc" });
    store.onS3Pay();

    // Tick with no grant → still paying.
    await vi.advanceTimersByTimeAsync(5000);
    await flush();
    expect(store.state.s3).toBe("paying");

    // The server-side grant lands on chain.
    grantState.current = GRANTED;
    await vi.advanceTimersByTimeAsync(5000);
    await flush();

    expect(store.state.s3).toBe("settled");
  });

  // THE 2026-07-28 STALL, end to end: the ADR 2026-07-27 grant funds the member's
  // EOA natively (32k) + mints the SBT, but the vault + registry read 0 until the
  // member self-bonds. This shape polled "paying" forever before the native leg.
  it("settles on the bond-fund grant: native-funded EOA + SBT, zero vault/registry stake", async () => {
    const store = new Store();
    store.setState({ walletAddr: "0xabc", custodyAddr: "0xabc" });
    store.onS3Pay();
    await vi.advanceTimersByTimeAsync(5000);
    await flush();
    expect(store.state.s3).toBe("paying");

    // The real bond-fund grant lands: SBT minted + EOA funded natively, NOT vaulted.
    grantState.current = {
      attributedStakeWei: "0",
      bondedStakeWei: "0",
      nativeBalanceWei: (32000n * 10n ** 18n).toString(),
      hasSbt: true,
    };
    await vi.advanceTimersByTimeAsync(5000);
    await flush();

    expect(store.state.s3).toBe("settled");
  });

  it("a grant with the SBT but BELOW the stake threshold does NOT settle (fail-closed)", async () => {
    grantState.current = { attributedStakeWei: (31999n * 10n ** 18n).toString(), hasSbt: true };
    const store = new Store();
    // Precondition (c1513cb link gate): onS3Pay only proceeds to "paying" once the
    // device wallet is LINKED (claim === custody). Provisioning + linking is the
    // seamless step ahead of pay; here we assert the pay/poll/settle logic, so we
    // start from a linked wallet.
    store.setState({ walletAddr: "0xabc", custodyAddr: "0xabc" });
    store.onS3Pay();
    for (let i = 0; i < 3; i++) {
      await vi.advanceTimersByTimeAsync(5000);
      await flush();
    }
    expect(store.state.s3).toBe("paying");
  });
});
