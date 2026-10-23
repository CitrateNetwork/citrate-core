// UI truth contract v1.2 (citrate-chain #354) on the Node page, onboarding S6 and Claim. These are
// the MAC independent review's B-1 to B-3 at f501345, from DGX on 4595dcb.
//   B-1 §1 Membership and bond: the rewards gate is MemberBond.isKycVerified() on chain
//       (MemberBond.claimRewards reverts KycRequired without it, MemberBond.sol:207). The
//       kyc_status userinfo claim "is never the source for a withdraw or rewards gate".
//   B-2 §1 Unactivated grants: chain#331 (activation closes at the lock end) is not deployed, so no
//       copy says activation closes. An unlocked bond that was never activated is
//       "Unlocked · not activated" with "Only an activated bond can be withdrawn."
//   B-3 rule 2: an unread MemberBond.isUnlocked() is "—" with the reason, never
//       "Bond locked · not staked".
import { afterEach, describe, expect, it, vi } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";

vi.mock("../../bridge/mode", () => ({ BRIDGE_MODE: "tauri" as const, assertSimAllowed: () => {} }));
vi.mock("wagmi", () => ({
  useBlockNumber: () => ({ data: undefined, isPending: true, error: null }),
  useReadContract: () => ({ data: undefined }),
}));
vi.mock("../../components/AgentChat", () => ({ AgentChat: () => null }));
vi.mock("../../widgets/WidgetsPanel", () => ({ WidgetsPanel: () => null }));
vi.mock("../../daemons/DaemonsPanel", () => ({ DaemonsPanel: () => null }));

import { Onboarding } from "../../onboarding/Onboarding";
import { store, memberBondFromGrant, type Store } from "../../shell/store";
import { freshState, type AppState } from "../../shell/state";
import { canOfferActivation, validatorDash, validatorWord, type MemberBondState } from "../../shell/chainTruth";
import { bridge } from "../../bridge";
import type { GrantStatus } from "../../bridge/domains";
import { NodeStatusView } from "./NodeStatus";
import { NodeRewards } from "./NodeRewards";

const WEI = 10n ** 18n;
const wei = (salt: number) => (BigInt(salt) * WEI).toString();
const NONE = { isActive: false, status: 0, actEpoch: 0, currentEpoch: 546 };
const KEY = "0x" + "77".repeat(32);
const LOCKED: MemberBondState = { deployed: true, activated: false, pubkey: null, unlocked: false };
const UNLOCKED: MemberBondState = { deployed: true, activated: false, pubkey: null, unlocked: true };
const LOCK_UNREAD: MemberBondState = { deployed: true, activated: false, pubkey: null, unlocked: null };
const LOCK_ABSENT: MemberBondState = { deployed: true, activated: false, pubkey: null };
const activatedBond = (kycVerified: boolean | null): MemberBondState => ({ deployed: true, activated: true, pubkey: KEY, unlocked: false, kycVerified });

function stubStore(over: Record<string, unknown> = {}): Store {
  return {
    walletIsLinked: () => true,
    refreshEarnings: vi.fn(async () => {}),
    claimRewards: vi.fn(async () => {}),
    refreshWallet: () => {},
    refreshPendingWithdrawals: () => {},
    refreshActivity: () => {},
    refreshChainParams: () => {},
    refreshProducers: () => {},
    activateValidator: vi.fn(async () => true),
    linkWallet: () => {},
    toast: () => {},
    go: () => {},
    startNode: () => {},
    stopNode: () => {},
    identity: () => ({ sub: "sub-1", wallet: "0x" + "0".repeat(40) }),
    ...over,
  } as unknown as Store;
}

const member = (over: Partial<AppState> = {}): AppState => ({
  ...freshState("p1"),
  signedIn: true,
  stage: "done",
  node: "synced",
  peers: 3,
  syncPct: 100,
  height: 546_000,
  hasGrant: true,
  s5StakeWei: wei(32000),
  bondedStake: 0,
  validatorActivation: NONE,
  ...over,
});

const statusPage = (s: AppState) =>
  renderToStaticMarkup(
    <NodeStatusView store={stubStore()} s={s} publicHead={s.height} publicHeadAgeMs={1_000} publicHeadError={false} statusAgeMs={1_000} compat="compatible" mode="tauri" statusError={null} />,
  );

afterEach(() => {
  vi.restoreAllMocks();
  store.setState({ memberBond: null, bondedStake: 0, validatorActivation: null, toast: null });
});

describe("B-3 — an unread isUnlocked() is '—', never 'Bond locked · not staked'", () => {
  it("the word is unknown with the reason, registry read or not", () => {
    for (const bond of [LOCK_UNREAD, LOCK_ABSENT]) {
      for (const activation of [NONE, null]) {
        expect(validatorWord(0, activation, true, bond)).toBeNull();
        expect(validatorDash(0, activation, true, bond)).toBe("— · bond lock not read yet");
        expect(canOfferActivation(0, activation, true, bond)).toBe(false);
      }
    }
  });

  it("the Node page and onboarding S6 say so", () => {
    const html = statusPage(member({ memberBond: LOCK_UNREAD }));
    expect(html).not.toContain("Bond locked · not staked");
    expect(html).toContain("— · bond lock not read yet");
    const s6 = renderToStaticMarkup(<Onboarding store={stubStore()} s={member({ stage: "s6", memberBond: LOCK_UNREAD })} />);
    expect(s6).not.toContain("Bond locked · not staked");
    expect(s6).not.toContain("Stake my bond");
  });

  it("a locked bond that was never activated is still 'Bond locked · not staked', and offered activation", () => {
    expect(validatorWord(0, NONE, true, LOCKED)).toBe("Bond locked · not staked");
    expect(canOfferActivation(0, NONE, true, LOCKED)).toBe(true);
  });
});

describe("B-2 — no activation-window copy until chain#331 is deployed (v1.2 §1)", () => {
  it("an unlocked bond that was never activated is 'Unlocked · not activated'", () => {
    for (const activation of [NONE, null]) {
      expect(validatorWord(0, activation, true, UNLOCKED)).toBe("Unlocked · not activated");
    }
  });

  it("the Node page shows the word with its required detail, and no window copy", () => {
    const html = statusPage(member({ memberBond: UNLOCKED }));
    expect(html).toContain("Unlocked · not activated");
    expect(html).toContain("Only an activated bond can be withdrawn.");
    expect(html).not.toMatch(/activation window|window closed|can no longer be staked|ActivationWindowClosed/i);
    expect(html).not.toContain("Activate validator");
  });

  it("onboarding S6 has no window copy for an unlocked bond", () => {
    const s6 = renderToStaticMarkup(<Onboarding store={stubStore()} s={member({ stage: "s6", memberBond: UNLOCKED })} />);
    expect(s6).not.toMatch(/activation window|window closed|can no longer be staked/i);
    expect(s6).toContain("Only an activated bond can be withdrawn.");
  });

  it("activateValidator's refusal for an unlocked bond names the word, not a closed window", async () => {
    store.setState({ ...member({ memberBond: UNLOCKED }) });
    const arm = vi.spyOn(bridge.node, "armMining");
    expect(await store.activateValidator()).toBe(false);
    expect(arm).not.toHaveBeenCalled();
    expect(String(store.getSnapshot().toast)).toContain("Unlocked · not activated");
    expect(String(store.getSnapshot().toast)).not.toMatch(/window/i);
  });

  it("tripwire: no shipped source says activation closes, or where an unactivated grant goes", () => {
    const walk = (d: string): string[] => readdirSync(d).flatMap((f) => (statSync(join(d, f)).isDirectory() ? walk(join(d, f)) : [join(d, f)]));
    const files = walk(join(process.cwd(), "src")).filter((f) => /\.tsx?$/.test(f) && !/\.test\.tsx?$/.test(f));
    for (const f of files) {
      const code = readFileSync(f, "utf8")
        .split("\n")
        .filter((l) => !/^\s*(\/\/|\*|\/\*)/.test(l))
        .join("\n");
      expect(code, f).not.toMatch(/activation window closed|can no longer be staked|returns? to the treasury|is reclaimed/i);
    }
  });
});

describe("B-1 — Claim is gated on MemberBond.isKycVerified(), not the userinfo kyc_status claim", () => {
  const grant = (over: Partial<GrantStatus> = {}): GrantStatus =>
    ({
      attributedStakeWei: wei(32000),
      attributedPrincipalWei: wei(32000),
      hasSbt: true,
      bondAddress: "0x" + "b".repeat(40),
      bondDeployed: true,
      bondedStakeWei: wei(32000),
      hasValidator: true,
      bondActivated: true,
      bondPubkey: KEY,
      unlockBlock: 16_000_000,
      isUnlocked: false,
      isKycVerified: true,
      ...over,
    }) as GrantStatus;

  it("the grant read carries isKycVerified into the bond state", () => {
    expect(memberBondFromGrant(grant({ isKycVerified: false })).kycVerified).toBe(false);
    expect(memberBondFromGrant(grant({ isKycVerified: true })).kycVerified).toBe(true);
    expect(memberBondFromGrant(grant({ bondDeployed: false })).kycVerified ?? null).toBeNull();
  });

  it("userinfo verified, bond not verified on chain: no claim ceremony, and the reason", async () => {
    store.setState({ s2: "verified" });
    vi.spyOn(store as never, "authUserinfo").mockResolvedValue(undefined as never);
    vi.spyOn(bridge.membership, "grantStatus").mockResolvedValue(grant({ isKycVerified: false }));
    const claim = vi.spyOn(bridge.agent, "claim");
    await store.claimRewards();
    expect(claim).not.toHaveBeenCalled();
    expect(String(store.getSnapshot().toast)).toContain("MemberBond.isKycVerified()");
  });

  it("bond verified on chain: the claim goes ahead whatever the userinfo claim says", async () => {
    store.setState({ s2: "none" as AppState["s2"] });
    vi.spyOn(store as never, "authUserinfo").mockResolvedValue(undefined as never);
    vi.spyOn(bridge.membership, "grantStatus").mockResolvedValue(grant({ isKycVerified: true }));
    const claim = vi.spyOn(bridge.agent, "claim").mockResolvedValue({ kind: "nothing", claimableWei: "0" });
    await store.claimRewards();
    expect(claim).toHaveBeenCalled();
  });

  it("an activated bond that isn't KYC-verified on chain gets no enabled Claim on Node › Rewards", () => {
    const rewards = { totalWei: wei(30), claimableWei: wei(10) };
    const unverified = renderToStaticMarkup(<NodeRewards store={stubStore()} s={member({ bondedStake: 32_000, memberBond: activatedBond(false), rewards })} mode="tauri" />);
    expect(unverified).toMatch(/<button[^>]*disabled=""[^>]*>\s*Claim/);
    expect(unverified).toContain("MemberBond.isKycVerified()");
    const unread = renderToStaticMarkup(<NodeRewards store={stubStore()} s={member({ bondedStake: 32_000, memberBond: activatedBond(null), rewards })} mode="tauri" />);
    expect(unread).toMatch(/<button[^>]*disabled=""[^>]*>\s*Claim/);
    const verified = renderToStaticMarkup(<NodeRewards store={stubStore()} s={member({ bondedStake: 32_000, memberBond: activatedBond(true), rewards })} mode="tauri" />);
    expect(verified).toMatch(/<button[^>]*>\s*Claim/);
    expect(verified).not.toMatch(/<button[^>]*disabled=""[^>]*>\s*Claim/);
  });
});
