// K3: the Node page tabs (Status, Rewards, Advanced), ported from #362 (design slice 2) onto main
// under the UI truth contract: every screen has its data, empty, error and unavailable state;
// unknown is "—", never 0; no finality, no projections, no "Earnings". Services is main's K1a-2
// registry tab (nodeServices*.test.tsx), not a second implementation.
import { afterEach, describe, expect, it, vi } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { freshState, type AppState } from "../../shell/state";
import type { Store } from "../../shell/store";
import type { ActivationStatusView } from "../../bridge/domains";
import { NodeStatusView } from "../Node";
import { NodeRewards } from "./NodeRewards";
import { NodeAdvancedView, type AdvancedReads } from "./NodeAdvanced";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let root: Root | null = null;
let host: HTMLDivElement | null = null;
afterEach(() => {
  act(() => root?.unmount());
  host?.remove();
  root = null;
  host = null;
});

async function mount(el: React.ReactElement): Promise<HTMLDivElement> {
  act(() => root?.unmount());
  host?.remove();
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  await act(async () => {
    root?.render(el);
  });
  // Let the mount effects' reads settle.
  await act(async () => {
    await new Promise((r) => setTimeout(r, 0));
  });
  return host;
}

const at = (patch: Partial<AppState> = {}): AppState => ({ ...freshState("p1"), stage: "done", ...patch });

function stubStore(over: Partial<Record<string, unknown>> = {}): Store & { calls: string[] } {
  const calls: string[] = [];
  const s = {
    calls,
    startNode: () => void calls.push("startNode"),
    stopNode: () => void calls.push("stopNode"),
    go: (r: string) => void calls.push("go:" + r),
    toast: (m: string) => void calls.push("toast:" + m),
    walletIsLinked: () => true,
    canForgetActivationTx: () => false,
    refreshProducers: async () => {},
    refreshActivation: async () => true,
    refreshEarnings: async () => {},
    linkWallet: async () => void calls.push("linkWallet"),
    requestValidatorActivation: async () => {
      calls.push("requestValidatorActivation");
      return true;
    },
    ...over,
  };
  return s as unknown as Store & { calls: string[] };
}

const click = async (el: Element | null | undefined) => {
  if (!el) throw new Error("no element to click");
  await act(async () => {
    (el as HTMLElement).click();
    await new Promise((r) => setTimeout(r, 0));
  });
};
const button = (root: HTMLElement, label: string | RegExp) =>
  [...root.querySelectorAll("button")].find((b) => (typeof label === "string" ? b.textContent?.trim() === label : label.test(b.textContent ?? "")));

const BANNED = /\bfinal(ized)?\b|validating|earning|projected|estimate|per day|\btoday\b|APY|yield/i;

// ---------------------------------------------------------------------------------------------
describe("Node › Status", () => {
  const live = {
    publicHead: 510_032,
    publicHeadAgeMs: 2_000,
    publicHeadError: false,
    statusAgeMs: 1_000,
    compat: "compatible" as const,
    mode: "tauri" as const,
    statusError: null,
  };

  it("one sentence in contract words, with direct peers and the read height", async () => {
    // A member whose bond state is not read yet: the validator word stays out of the sentence.
    const h = await mount(<NodeStatusView store={stubStore()} s={at({ node: "synced", height: 510_030, peers: 4, hasGrant: true })} {...live} />);
    expect(h.querySelector('[data-testid="node-sentence"]')?.textContent).toBe("Synced");
    expect(h.textContent).toContain("4 direct peers");
    expect(h.textContent).toContain("510,030 · 2 behind the public head");
    expect(h.textContent).not.toMatch(BANNED);
    expect(h.textContent).not.toContain("%");
  });

  it("v1.1 network rows: Registered and Eligible validators from the store's registry read, and no consensus-set number", async () => {
    const unread = await mount(<NodeStatusView store={stubStore()} s={at({ node: "synced", height: 510_030, peers: 4 })} {...live} />);
    expect(unread.querySelector('[data-testid="registered-validators"]')?.textContent).toBe("— · not read yet");
    expect(unread.querySelector('[data-testid="eligible-validators"]')?.textContent).toBe("— · not read yet");
    expect(unread.textContent).toContain("unavailable until a snapshot RPC exists");
    expect(unread.textContent).not.toContain("Active validators");
    const read = await mount(<NodeStatusView store={stubStore()} s={at({ node: "synced", height: 510_030, peers: 4, validatorCounts: { registered: 5, eligible: 4, pending: 1 } })} {...live} />);
    expect(read.querySelector('[data-testid="registered-validators"]')?.textContent).toBe("5");
    expect(read.querySelector('[data-testid="eligible-validators"]')?.textContent).toBe("4");
  });

  it("the validator word joins the sentence only when it was read; otherwise '—' with the reason", async () => {
    const s = at({ node: "synced", height: 510_030, peers: 4, hasGrant: true });
    const unread = await mount(<NodeStatusView store={stubStore()} s={s} {...live} />);
    expect(unread.querySelector('[data-testid="validator-word"]')?.textContent ?? "").toMatch(/^— · bond state not read yet/);
    expect(unread.querySelector('[data-testid="node-sentence"]')?.textContent).toBe("Synced");
    const read = await mount(<NodeStatusView store={stubStore()} s={{ ...s, bondedStake: 32_000, validatorActivation: { isActive: true, status: 1, actEpoch: 38, currentEpoch: 40 } }} {...live} />);
    expect(read.querySelector('[data-testid="node-sentence"]')?.textContent).toBe("Synced · Validator active");
  });

  it("your stake is the one stakeOf figure, '—' with no registered stake (#348)", async () => {
    const staked = await mount(<NodeStatusView store={stubStore()} s={at({ node: "synced", height: 510_030, peers: 4, bondedStake: 32_000, bondedStakeRead: true, s5StakeWei: "1" })} {...live} />);
    expect(staked.querySelector('[data-testid="your-stake"]')?.textContent).toBe("32,000 SALT");
    const none = await mount(<NodeStatusView store={stubStore()} s={at({ node: "synced", height: 510_030, peers: 4, hasGrant: true, bondedStakeRead: true, s5StakeWei: (32_000n * 10n ** 18n).toString() })} {...live} />);
    // The grant in a locked bond is not validator stake.
    expect(none.querySelector('[data-testid="your-stake"]')?.textContent).toBe("—");
  });

  it("empty: an off node offers Start, and its numbers are unknown, not 0", async () => {
    const store = stubStore();
    const h = await mount(<NodeStatusView store={store} s={at({ node: "off", height: 0, peers: 0 })} {...live} />);
    // No grant and no stake is a read word, so it joins the sentence.
    expect(h.querySelector('[data-testid="node-sentence"]')?.textContent).toBe("Off · Not a validator");
    expect(button(h, "Stop node")).toBeUndefined();
    expect(h.querySelector('[data-testid="direct-peers"]')?.textContent).toBe("— · not read");
    await click(button(h, "Start node"));
    expect(store.calls).toContain("startNode");
  });

  it("Update required shows its notice, for a retired genesis and for an incomplete profile (K6)", async () => {
    for (const compat of ["retired", "unset"] as const) {
      const h = await mount(<NodeStatusView store={stubStore()} s={at({ node: "synced", height: 10, peers: 1 })} {...live} compat={compat} />);
      expect(h.querySelector('[data-testid="node-sentence"]')?.textContent ?? "", compat).toMatch(/^Update required( · |$)/);
      expect(h.querySelector('[data-testid="node-notice"]')?.textContent ?? "", compat).toMatch(/genesis/i);
    }
  });

  it("a refused start is Error and names the holder", async () => {
    const h = await mount(<NodeStatusView store={stubStore()} s={at({ node: "error", nodeBlocked: "Another process (pid 4312) holds the chain database." })} {...live} />);
    expect(h.querySelector('[data-testid="node-sentence"]')?.textContent ?? "").toMatch(/^Error( · |$)/);
    expect(h.querySelector('[role="alert"]')?.textContent).toContain("pid 4312");
    expect(button(h, "Start node")).toBeDefined();
  });

  it("error: a failed status read makes every number unknown and says which call failed", async () => {
    const h = await mount(<NodeStatusView store={stubStore()} s={at({ node: "synced", height: 510_030, peers: 4 })} {...live} statusError="supervisor not answering" />);
    expect(h.querySelector('[data-state="error"]')?.textContent).toContain("node_status");
    expect(h.textContent).toContain("supervisor not answering");
    expect(h.querySelector('[data-testid="node-sentence"]')).toBeNull();
    expect(h.textContent).not.toContain("510,030");
  });

  it("unavailable: the web preview has no supervisor, so nothing is shown rather than sim rows", async () => {
    const h = await mount(<NodeStatusView store={stubStore()} s={at({ node: "synced", height: 131_204, peers: 23 })} {...live} mode="sim" />);
    expect(h.querySelector('[data-state="unavailable"]')).not.toBeNull();
    expect(h.textContent).not.toContain("131,204");
    expect(h.textContent).not.toContain("23 direct peers");
  });

  // Activation through the one gate: nodeActivation.test.tsx covers every read.
  const bondLocked = { hasGrant: true, bondedStake: 0, validatorActivation: { isActive: false, status: 0, actEpoch: 0, currentEpoch: 40 }, bondActivated: false, bondUnlocked: false };
  it("activation is offered for a bond the gate allows, without an amount", async () => {
    const store = stubStore();
    const h = await mount(<NodeStatusView store={store} s={at({ node: "synced", height: 510_030, peers: 4, ...bondLocked })} {...live} />);
    const b = button(h, /Activate validator/);
    expect(b, "Activate validator button").toBeTruthy();
    expect(b?.textContent ?? "").not.toMatch(/32,000|\d SALT/);
    await click(b);
    expect(store.calls).toContain("requestValidatorActivation");
  });

  it("no Activate while the registry status or the bond state is unread: '—' and the reason", async () => {
    const h = await mount(<NodeStatusView store={stubStore()} s={at({ node: "synced", height: 510_030, peers: 4, hasGrant: true })} {...live} />);
    expect(button(h, /Activate validator/)).toBeUndefined();
    expect(h.querySelector('[data-testid="activate-gate"]')?.textContent ?? "").toMatch(/^— · /);
  });

  it("Exiting, Slashed or an activated bond: no Activate, and the word is shown", async () => {
    for (const [status, bondActivated, word] of [
      [2, false, "Exiting"],
      [3, false, "Slashed"],
      [0, true, "—"],
    ] as const) {
      const s = at({ node: "synced", height: 510_030, peers: 4, ...bondLocked, validatorActivation: { ...bondLocked.validatorActivation, status }, bondActivated });
      const h = await mount(<NodeStatusView store={stubStore()} s={s} {...live} />);
      expect(button(h, /Activate validator/), word).toBeUndefined();
      expect(h.querySelector('[data-testid="activate-gate"]')?.textContent?.startsWith(word), word).toBe(true);
    }
  });

  it("a recorded activation shows its status (Try again / Forget path) even while the page isn't Synced (#406)", async () => {
    const tx = { hash: "0xdead", included: true, blockNumber: 501, success: false };
    const s = at({
      node: "syncing",
      height: 400_000,
      peers: 4,
      ...bondLocked,
      s7TxHash: "0xdead",
      activation: { bondActivated: false, bondUnlocked: false, bondDeployed: true, membershipActive: true, tx, recordedTx: null } as unknown as ActivationStatusView,
    });
    const h = await mount(<NodeStatusView store={stubStore()} s={s} {...live} />);
    expect(h.querySelector('[data-testid="node-activation-failed"]')).not.toBeNull();
    expect(button(h, /Activate validator/)).toBeUndefined();
  });

  it("an unreadable network head is Sync unknown, with the local height labelled this node", async () => {
    const h = await mount(<NodeStatusView store={stubStore()} s={at({ node: "synced", height: 510_030, peers: 4, hasGrant: true })} {...live} publicHead={null} publicHeadAgeMs={null} />);
    expect(h.querySelector('[data-testid="node-sentence"]')?.textContent).toBe("Sync unknown · network head unavailable");
    expect(h.textContent).toContain("510,030 (this node)");
    const stale = await mount(<NodeStatusView store={stubStore()} s={at({ node: "synced", height: 510_030, peers: 4, hasGrant: true })} {...live} publicHeadAgeMs={90_000} />);
    expect(stale.querySelector('[data-testid="node-sentence"]')?.textContent).toBe("Sync unknown · network head unavailable");
  });

  it("stale local reads are unknown, not current: an old node_status answer shows the error state", async () => {
    const h = await mount(<NodeStatusView store={stubStore()} s={at({ node: "synced", height: 510_030, peers: 4 })} {...live} statusAgeMs={61_000} />);
    expect(h.querySelector('[data-state="error"]')?.textContent ?? "").toMatch(/61 s ago/);
    expect(h.querySelector('[data-testid="node-sentence"]')).toBeNull();
    expect(h.textContent).not.toContain("510,030");
  });

  it("the mining hold shows its reason under the sentence", async () => {
    const h = await mount(<NodeStatusView store={stubStore()} s={at({ node: "synced", height: 510_030, peers: 4, miningHold: "Block production is off: 1 other producer(s) are live." })} {...live} />);
    expect(h.querySelector('[data-testid="mining-hold"]')?.textContent).toContain("1 other producer(s) are live");
  });
});

// ---------------------------------------------------------------------------------------------
const WEI = (salt: string) => (BigInt(Math.round(Number(salt) * 100)) * 10n ** 16n).toString();

describe("Node › Rewards", () => {
  // The store reads rewardsOf (Store.refreshEarnings) for registered stake or an activated bond.
  const validator = { bondedStake: 32_000, validatorActivation: { isActive: true, status: 1, actEpoch: 38, currentEpoch: 40 } };
  const rewards = (totalWei: string, claimableWei: string) => at({ ...validator, rewards: { totalWei, claimableWei } });
  const rStore = () => stubStore({ refreshEarnings: vi.fn(async () => {}), claimRewards: vi.fn(async () => {}) });

  it("rewardsOf total, claimable now and maturing, from the registry read", async () => {
    const h = await mount(<NodeRewards store={rStore()} s={rewards(WEI("12.5"), WEI("2.5"))} mode="tauri" />);
    expect(h.querySelector('[data-reward="total"]')?.textContent).toBe("12.50 SALT");
    expect(h.querySelector('[data-reward="claimable"]')?.textContent).toBe("2.50 SALT");
    expect(h.querySelector('[data-reward="maturing"]')?.textContent).toBe("10.00 SALT");
    expect(h.textContent).toContain("rewardsOf");
    // The panel carries no money words; the page's one disclaimer names what is never shown.
    expect(h.querySelector('[data-testid="validator-rewards"]')?.textContent).not.toMatch(BANNED);
    expect(h.textContent).not.toContain("Earnings");
  });

  it("opening the tab asks the store to read", async () => {
    const store = rStore();
    await mount(<NodeRewards store={store} s={at(validator)} mode="tauri" />);
    expect(store.refreshEarnings).toHaveBeenCalled();
  });

  it("Claim runs the store's claim (the ceremony) above the dust threshold, and is disabled below it", async () => {
    const store = rStore();
    const h = await mount(<NodeRewards store={store} s={rewards(WEI("30"), WEI("20"))} mode="tauri" />);
    await click(button(h, "Claim"));
    expect(store.claimRewards).toHaveBeenCalledTimes(1);
    const dust = await mount(<NodeRewards store={rStore()} s={rewards(WEI("3"), WEI("2"))} mode="tauri" />);
    expect((button(dust, "Claim") as HTMLButtonElement).disabled).toBe(true);
    expect(dust.textContent).toContain("3.00 SALT to go");
  });

  it("pin and round rewards are not live", async () => {
    const h = await mount(<NodeRewards store={rStore()} s={rewards("0", "0")} mode="tauri" />);
    expect(h.querySelector('[data-testid="pin-rewards"]')?.textContent).toContain("Pin bonds not live yet");
    expect(h.querySelector('[data-testid="round-rewards"]')?.textContent).toContain("Group rounds not live yet");
  });

  it("Not a validator is a word, not 0", async () => {
    const s = at({ hasGrant: false, bondedStake: 0, validatorActivation: { isActive: false, status: 0, actEpoch: 0, currentEpoch: 40 } });
    const h = await mount(<NodeRewards store={rStore()} s={s} mode="tauri" />);
    expect(h.querySelector('[data-reward="total"]')?.textContent).toBe("Not a validator");
    expect(h.querySelector('[data-reward="claimable"]')).toBeNull();
  });

  it("an unread bond state is '—' with the reason, never 'Not a validator' (#362 truth fix)", async () => {
    const h = await mount(<NodeRewards store={rStore()} s={at({ hasGrant: true, bondedStake: 0, bondActivated: null })} mode="tauri" />);
    expect(h.querySelector('[data-reward="total"]')?.textContent).toBe("— · bond state not read yet");
    expect(h.textContent).not.toContain("Not a validator");
  });

  it("empty: a zero read for an unread validator word says zero is not proof of registration", async () => {
    const h = await mount(<NodeRewards store={rStore()} s={at({ bondedStake: 32_000, validatorActivation: null, rewards: { totalWei: "0", claimableWei: "0" } })} mode="tauri" />);
    expect(h.querySelector('[data-reward="total"]')?.textContent).toBe("0.00 SALT");
    expect(h.textContent).toMatch(/also reads 0 for a key that isn't registered/);
  });

  it("an unreadable read is the error state, never 0", async () => {
    const h = await mount(<NodeRewards store={rStore()} s={at({ ...validator, rewards: null, rewardsError: "node_validator_earnings returned a figure that isn't a number" })} mode="tauri" />);
    expect(h.querySelector('[data-state="error"]')).not.toBeNull();
    expect(h.querySelector('[data-reward="total"]')).toBeNull();
    expect(h.textContent).not.toContain("0.00 SALT");
  });

  it("unavailable without a proposer key, error with '—' otherwise, and Try again re-reads", async () => {
    const noKey = await mount(<NodeRewards store={rStore()} s={at({ ...validator, rewardsError: "proposer key not minted yet" })} mode="tauri" />);
    expect(noKey.querySelector('[data-state="unavailable"]')?.textContent).toContain("this node's key");
    expect(noKey.querySelector('[data-state="unavailable"]')?.textContent).toContain("armed block production");
    const store = rStore();
    const err = await mount(<NodeRewards store={store} s={at({ ...validator, rewardsError: "eth_call timed out" })} mode="tauri" />);
    expect(err.querySelector('[data-state="error"]')?.textContent).toContain("eth_call timed out");
    expect(err.textContent).toContain("unknown, not zero");
    expect(err.querySelector('[data-reward="total"]')).toBeNull();
    const before = (store.refreshEarnings as unknown as ReturnType<typeof vi.fn>).mock.calls.length;
    await click(button(err, "Try again"));
    expect((store.refreshEarnings as unknown as ReturnType<typeof vi.fn>).mock.calls.length).toBe(before + 1);
  });

  it("unavailable in the web preview, and nothing is read", async () => {
    const store = rStore();
    const h = await mount(<NodeRewards store={store} s={at(validator)} mode="sim" />);
    expect(h.querySelector('[data-state="unavailable"]')).not.toBeNull();
    expect(store.refreshEarnings).not.toHaveBeenCalled();
  });
});

// ---------------------------------------------------------------------------------------------
describe("Node › Advanced", () => {
  const reads = (over: Partial<{ notice: string | undefined; crash: { reason: string; atUnixMs: number } | null; fail: boolean }> = {}): AdvancedReads => ({
    nodeStatus: async () => {
      if (over.fail) return Promise.reject("supervisor not answering");
      return { state: "running", peers: 3, height: 10, syncPct: 100, notice: over.notice } as Awaited<ReturnType<AdvancedReads["nodeStatus"]>>;
    },
    lastCrash: async () => (over.fail ? Promise.reject("supervisor not answering") : (over.crash ?? null)),
  });
  const GEN = "0x1dcfc4909dd9fdcb0f03c823d627e4b2032cc86bef5fc4561030d6c64b9775f1";
  const view = (over: Partial<Parameters<typeof NodeAdvancedView>[0]> = {}) => (
    <NodeAdvancedView s={at()} mode="tauri" compat="compatible" liveGenesis={GEN} expectedGenesis={GEN} reads={reads()} {...over} />
  );

  it("network identity: matches this build, or Update required, or not checked yet", async () => {
    const ok = await mount(view());
    expect(ok.querySelector('[data-testid="genesis-word"]')?.textContent).toBe("matches this build");
    expect(ok.textContent).toContain("Update required at genesis");
    // The chain id is this build's configuration, not a read.
    expect(ok.textContent).toContain("this build's network profile (expected)");
    const bad = await mount(view({ compat: "retired", liveGenesis: "0x" + "11".repeat(32) }));
    expect(bad.querySelector('[data-testid="genesis-word"]')?.textContent).toBe("Update required");
    const unset = await mount(view({ compat: "unset", liveGenesis: null, expectedGenesis: null }));
    expect(unset.querySelector('[data-testid="genesis-word"]')?.textContent).toBe("Update required");
    expect(unset.textContent).toContain("— · not set in this build's profile");
    const unk = await mount(view({ compat: "unknown", liveGenesis: null }));
    expect(unk.querySelector('[data-testid="genesis-word"]')?.textContent).toBe("not checked yet · —");
  });

  it("the withdrawn-branch notice from node_status, and an honest line when there is none", async () => {
    const h = await mount(view({ reads: reads({ notice: "This node had followed a branch of the chain that the network withdrew." }) }));
    expect(h.querySelector('[data-testid="withdrawn"]')?.textContent).toContain("followed a branch");
    const none = await mount(view());
    expect(none.querySelector('[data-testid="withdrawn"]')?.textContent ?? "").toMatch(/no notice/i);
    expect(none.querySelector('[data-testid="withdrawn"]')?.textContent).not.toMatch(/Resync required/);
  });

  it("crash records: the last record, or none", async () => {
    const h = await mount(view({ reads: reads({ crash: { reason: "exit 137", atUnixMs: Date.UTC(2026, 9, 16, 3, 12) } }) }));
    expect(h.querySelector('[data-testid="crash-record"]')?.textContent).toContain("exit 137");
    expect(h.querySelector('[data-testid="crash-record"]')?.textContent).toContain("2026-10-16 03:12 UTC");
    const none = await mount(view());
    expect(none.querySelector('[data-testid="crash-record"]')?.textContent).toContain("No crash recorded");
  });

  it("full log: the node's lines, or why there are none", async () => {
    const logs = [{ t: "12:00:01", line: "\x1b[32mimported block 510030\x1b[0m", id: 1 }];
    const h = await mount(view({ s: at({ node: "synced", logs }) }));
    expect(h.querySelector('[data-testid="full-log"]')?.textContent).toContain("imported block 510030");
    // ANSI escapes are stripped, never rendered.
    expect(h.querySelector('[data-testid="full-log"]')?.textContent).not.toContain("\x1b");
    const off = await mount(view({ s: at({ node: "off", logs: [] }) }));
    expect(off.querySelector('[data-testid="full-log"]')?.textContent ?? "").toMatch(/node is off/i);
  });

  it("error: a failed read shows the call and keeps the rest of the page", async () => {
    const h = await mount(view({ reads: reads({ fail: true }) }));
    expect(h.querySelector('[data-testid="withdrawn"] [data-state="error"]')?.textContent).toContain("supervisor not answering");
    expect(h.querySelector('[data-testid="crash-record"] [data-state="error"]')?.textContent).toContain("node_last_crash");
    expect(h.querySelector('[data-testid="full-log"]')).not.toBeNull();
  });

  it("unavailable in the web preview", async () => {
    const h = await mount(view({ mode: "sim", compat: "unknown", liveGenesis: null }));
    expect(h.querySelector('[data-state="unavailable"]')).not.toBeNull();
  });
});
