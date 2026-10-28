// C1-V (UI truth contract v1, citrate-chain docs/ui-truth/UI_TRUTH_CONTRACT.md) — validator,
// stake and rewards. Live 40204: ~2 s blocks, 5 active validators with 1 producing,
// blockSubsidy 10 SALT, minStake 32,000.
//
//   V4 "validating" from stake >= 32k (incl. pool self-stake, retired by D-9), "producing again",
//      "every full node builds blocks"     -> ValidatorRegistry.isActive only
//   V6 withdrawal time assumes 12 s blocks -> blocks x measured block time
//   V7 four definitions of "staked", one hard-coding 32,000 -> stakeOf only; minStake is read
//   V8 liquid captioned "earned rewards"; rewardsOf labelled "off-chain estimate"; Claimable
//      caption names ContributionAccounting; "Today" has no source
//   V9 "source · local node" when reads go to rpc.citrate.ai; stale vault/SBT addresses
import { describe, it, expect, vi, afterEach } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";

vi.mock("../bridge/mode", () => ({
  BRIDGE_MODE: "tauri" as const,
  assertSimAllowed: () => {},
}));
vi.mock("wagmi", () => ({
  useBlockNumber: () => ({ data: undefined, isPending: true, error: null }),
  useReadContract: () => ({ data: undefined }),
}));
vi.mock("../components/AgentChat", () => ({ AgentChat: () => null }));
vi.mock("../widgets/WidgetsPanel", () => ({ WidgetsPanel: () => null }));
vi.mock("../daemons/DaemonsPanel", () => ({ DaemonsPanel: () => null }));

import { Dashboard } from "./Dashboard";
import { Node } from "./Node";
import { Wallet } from "./Wallet";
import { S5, Onboarding } from "../onboarding/Onboarding";
import { blocksWithTime, measuredBlockSeconds, validatorWord, type ChainReads } from "../shell/chainTruth";
import { mapNodeState, store, type Store } from "../shell/store";
import { freshState, nodeLabel, type AppState } from "../shell/state";
import { createDemoProvider, type AgentContext, type ToolCall } from "../agent/harness";
import { bridge } from "../bridge";
import type { GrantStatus } from "../bridge/domains";
import book from "../../src-tauri/addresses/40204.json";

const WEI = 10n ** 18n;
// Contract v1.1 §1: the registry activation readings (isActive, validatorInfo.actEpoch, currentEpoch).
const ACT_ON = { isActive: true, status: 1, actEpoch: 499, currentEpoch: 499 };
const ACT_PENDING = { isActive: true, status: 1, actEpoch: 501, currentEpoch: 499 };
// What the registry returns for a key it never registered (status None).
const ACT_NONE = { isActive: false, status: 0, actEpoch: 0, currentEpoch: 499 };
const wei = (salt: number) => (BigInt(salt) * WEI).toString();

const stub = {
  walletIsLinked: () => true,
  refreshEarnings: () => {},
  refreshWallet: () => {},
  refreshActivity: () => {},
  refreshChainParams: () => {},
  toast: () => {},
  identity: () => ({ sub: "sub-1", wallet: "0x" + "0".repeat(40) }),
} as unknown as Store;

function member(over: Partial<AppState> = {}): AppState {
  return { ...freshState("p1"), signedIn: true, node: "synced", peers: 3, syncPct: 100, height: 499_058, hasGrant: true, s5StakeWei: wei(32000), ...over };
}

afterEach(() => {
  vi.restoreAllMocks();
  vi.useRealTimers();
});

describe("chainTruth helpers", () => {
  it("measures the block time from two block timestamps", () => {
    expect(measuredBlockSeconds({ number: 499_100n, timestamp: 1_000_200n }, { number: 499_000n, timestamp: 1_000_000n })).toBe(2);
    expect(measuredBlockSeconds({ number: 5n, timestamp: 9n }, { number: 5n, timestamp: 9n })).toBeNull();
  });

  it("gives a duration in blocks both ways, from the measured time (V6)", () => {
    expect(blocksWithTime(50_400, 2)).toBe("50,400 blocks (~28 h)");
    expect(blocksWithTime(50_400, null)).toBe("50,400 blocks");
  });

  it("derives the §1 validator word from the registry only (V4)", () => {
    expect(validatorWord(0, null, false)).toBe("Not a validator");
    // #348 review: the bond-locked word needs MemberBond.activated() = false; unread is null ("—").
    expect(validatorWord(0, null, true, false)).toBe("Bond locked · not staked");
    expect(validatorWord(0, null, true)).toBeNull();
    expect(validatorWord(32_000, ACT_PENDING, true)).toBe("Registered · active from epoch 501");
    expect(validatorWord(32_000, ACT_ON, true)).toBe("Validator active");
    expect(validatorWord(32_000, null, true)).toBeNull();
  });
});

describe("V4 — validator state comes from ValidatorRegistry.isActive", () => {
  it("mapNodeState takes the registry activation, not a stake amount", () => {
    expect(mapNodeState("running", 100, ACT_ON)).toBe("validating");
    expect(mapNodeState("running", 100, ACT_PENDING)).toBe("synced");
    expect(mapNodeState("running", 100, null)).toBe("synced");
    expect(mapNodeState("running", 40, ACT_ON)).toBe("syncing");
  });

  it("the node label uses the §1 word", () => {
    expect(nodeLabel("validating")).toBe("validator active");
  });

  function grant(stakeSalt: number): GrantStatus {
    return {
      attributedStakeWei: wei(32000),
      attributedPrincipalWei: wei(32000),
      hasSbt: true,
      bondAddress: "0x" + "b".repeat(40),
      bondDeployed: true,
      bondedStakeWei: wei(stakeSalt),
      hasValidator: stakeSalt > 0,
      unlockBlock: 16_000_000,
      isUnlocked: false,
      isKycVerified: false,
    };
  }

  async function poll(stakeSalt: number, active: boolean, over: Partial<AppState> = {}) {
    const reads = {
      validatorActivation: vi.fn(async () => (stakeSalt > 0 ? (active ? ACT_ON : ACT_PENDING) : ACT_NONE)),
      minStakeWei: async () => 32_000n * WEI,
      blockSeconds: async () => 2,
      producerSample: async () => ({ head: 1, window: 100, proposers: [] }),
      validatorCounts: async () => ({ registered: 5, eligible: 5, pending: 0 }),
    } as unknown as ChainReads & { validatorActivation: ReturnType<typeof vi.fn> };
    (store as unknown as { chainReads: ChainReads }).chainReads = reads;
    (store as unknown as { lastBondReadAt: number }).lastBondReadAt = 0;
    (store as unknown as { lastActiveReadAt: number }).lastActiveReadAt = 0;
    (store as unknown as { validatorArmed: boolean }).validatorArmed = true;
    store.setState({ node: "synced", hasGrant: false, bondedStake: 0, validatorActivation: null, ...over });
    vi.spyOn(bridge.node, "status").mockResolvedValue({ state: "running", peers: 3, height: 499_058, syncPct: 100, miningArmed: true });
    vi.spyOn(bridge.node, "logs").mockResolvedValue([]);
    vi.spyOn(bridge.node, "validatorEarnings").mockResolvedValue({ totalWei: "0", claimableWei: "0", proposerPubkey: "0x" + "11".repeat(32) });
    vi.spyOn(bridge.membership, "grantStatus").mockResolvedValue(grant(stakeSalt));
    await store.refreshNode();
    return reads;
  }

  it("registered stake whose activation epoch is not reached is not 'validating'", async () => {
    await poll(32_000, false);
    expect(store.state.validatorActivation).toEqual(ACT_PENDING);
    expect(store.state.node).toBe("synced");
  });

  it("an activated validator (actEpoch reached) makes the node a validator", async () => {
    const reads = await poll(32_000, true);
    expect(reads.validatorActivation).toHaveBeenCalledWith("0x" + "11".repeat(32));
    expect(store.state.node).toBe("validating");
  });

  it("no stake figure but the registry makes a validator", async () => {
    // C1-V2: the registry is read for the key whatever the stake (so Exiting/Slashed show after a
    // full exit), and status None is not a validator. (The pool self-stake this once covered is
    // retired by D-9; a legacy one in state is covered in store.test.ts.)
    await poll(0, true);
    expect(store.state.node).toBe("synced");
    expect(validatorWord(store.state.bondedStake, store.state.validatorActivation, false)).toBe("Not a validator");
  });

  it("the re-arm toast does not say 'producing again'", async () => {
    const toast = vi.spyOn(store, "toast");
    vi.spyOn(bridge.node, "armMining").mockResolvedValue(true);
    await poll(32_000, true);
    (store as unknown as { validatorArmed: boolean }).validatorArmed = false;
    (store as unknown as { lastActiveReadAt: number }).lastActiveReadAt = 0;
    await store.refreshNode();
    await Promise.resolve();
    await Promise.resolve();
    const said = toast.mock.calls.map((c) => String(c[0])).join(" ");
    expect(said).not.toContain("producing again");
  });

  it("the Node surface shows the §1 word and no made-up election odds", () => {
    const html = renderToStaticMarkup(<Node store={stub} s={member({ bondedStake: 32_000, validatorActivation: ACT_PENDING })} />);
    expect(html).toContain("Registered · active from epoch 501");
    expect(html).not.toContain("Election odds");
    const active = renderToStaticMarkup(<Node store={stub} s={member({ node: "validating", bondedStake: 32_000, validatorActivation: ACT_ON })} />);
    expect(active).toContain("Validator active");
  });

  it("onboarding never says every full node builds blocks", () => {
    const html = renderToStaticMarkup(<Onboarding store={stub} s={member({ stage: "s6", node: "synced" })} />);
    expect(html).not.toContain("builds blocks");
    const v = renderToStaticMarkup(<Onboarding store={stub} s={member({ stage: "s6", node: "validating", bondedStake: 32_000, validatorActivation: ACT_ON })} />);
    expect(v).toContain("Validator active");
    expect(v).not.toContain(">Validating<");
  });
});

describe("V7 — one 'staked': ValidatorRegistry.stakeOf", () => {
  it("Wallet: the grant in a locked bond is not 'staked'", () => {
    const html = renderToStaticMarkup(<Wallet store={stub} s={member({ wTab: "overview", bondedStake: 0, bondedStakeRead: true, bondActivated: false })} />);
    expect(html).not.toContain("32,000 grant vaulted");
    expect(html).toContain("Bond locked · not staked");
  });

  it("Wallet: staked equals the registry stake once activated", () => {
    const html = renderToStaticMarkup(<Wallet store={stub} s={member({ wTab: "staking", bondedStake: 32_000, bondedStakeRead: true })} />);
    expect(html).toContain("ValidatorRegistry.stakeOf");
    expect(html).toContain("32,000 SALT");
  });

  it("Wallet: the validator minimum is read from minStake, not hard-coded", () => {
    const read = renderToStaticMarkup(<Wallet store={stub} s={member({ wTab: "staking", minStakeWei: wei(50_000) })} />);
    expect(read).toContain("50,000 SALT");
    const unread = renderToStaticMarkup(<Wallet store={stub} s={member({ wTab: "staking", minStakeWei: null })} />);
    expect(unread).toContain("— · reading minStake");
  });

  it("Dashboard: Staked is stakeOf, never the grant", () => {
    const html = renderToStaticMarkup(<Dashboard store={stub} s={member({ bondedStake: 0, bondedStakeRead: true, bondActivated: false })} />);
    expect(html).not.toContain(">32,000<");
    expect(html).toMatch(/bond locked · not staked/i);
  });

  it("the model context's staked is the registry stake only", () => {
    store.setState({ hasGrant: true, s5StakeWei: wei(32000), bondedStake: 0 });
    expect(store.snapshot().staked).toBe(0);
  });
});

describe("V8 — reward and balance captions per §1 Rewards", () => {
  it("the liquid balance is not captioned as earned rewards", () => {
    const html = renderToStaticMarkup(<Wallet store={stub} s={member({ wTab: "overview" })} />);
    expect(html).not.toContain("earned rewards");
  });

  it("the Node Earning tab names rewardsOf, splits claimable/maturing, and has no off-chain estimates", () => {
    const s = member({ nTab: "earn", bondedStake: 32_000, validatorActivation: ACT_ON, rewards: { totalWei: wei(30), claimableWei: wei(20) } });
    const html = renderToStaticMarkup(<Node store={stub} s={s} />);
    expect(html).not.toContain("off-chain estimate");
    expect(html).not.toContain("ContributionAccounting.claimable");
    expect(html).toContain("Validator rewards");
    expect(html).toContain("Claimable now");
    expect(html).toContain("Maturing");
    expect(html).toContain("ValidatorRegistry.rewardsOf");
    expect(html).toContain("30.00");
    expect(html).toContain("20.00");
    expect(html).toContain("10.00");
  });

  it("a member who is not a validator sees no rewards figure", () => {
    const html = renderToStaticMarkup(<Node store={stub} s={member({ nTab: "earn", bondedStake: 0, rewards: null, claimable: 9.41 })} />);
    expect(html).not.toContain("9.41");
    expect(html).toContain("Not a validator");
  });

  it("the Dashboard has no 'Today' tile", () => {
    const html = renderToStaticMarkup(<Dashboard store={stub} s={member()} />);
    expect(html).not.toContain(">Today<");
    expect(html).not.toContain("SALT earned");
  });

  it("the model context has no earnings-today figure", () => {
    expect(Object.keys(store.snapshot())).not.toContain("earningsToday");
  });

  it("the built-in agent never reports today's earnings or calls the liquid balance earned rewards", async () => {
    vi.useFakeTimers();
    const ctx: AgentContext = { height: 499_058, peers: 3, nodeState: "synced", staked: 0, liquid: 12.5, claimable: null, rewardsTotal: null, walletAddr: "0xabc0000000000000000000000000000000000001", tier: "pilot" };
    const ask = async (q: string) => {
      let text = "";
      const done = createDemoProvider(() => ctx).send({
        messages: [{ role: "user", content: q }],
        callbacks: { onStatus: () => {}, onToken: (t: string) => (text += t), onToolCall: async (_c: ToolCall) => "ok" },
      });
      await vi.runAllTimersAsync();
      await done;
      return text;
    };
    const earn = await ask("what did i earn?");
    expect(earn).not.toMatch(/today/i);
    expect(earn).not.toMatch(/pinning and compute/);
    expect(earn).toContain("not a registered validator");
    const bal = await ask("what's my wallet balance?");
    expect(bal).not.toContain("earned rewards");
    expect(bal).not.toContain("local node");
    expect(bal).toContain("rpc.citrate.ai");
  });
});

describe("V9 — captions name the real source and the address book", () => {
  it("Wallet says its reads come from rpc.citrate.ai, also while the node runs", () => {
    const html = renderToStaticMarkup(<Wallet store={stub} s={member({ wTab: "overview", node: "synced" })} />);
    expect(html).toContain("source · rpc.citrate.ai");
    expect(html).not.toContain("source · local node");
  });

  it("the grant step's data-source line uses the bundled address book", () => {
    const html = renderToStaticMarkup(<S5 store={stub} s={member({ s5: "idle" })} />);
    expect(html).not.toContain("0x0ace…267e");
    expect(html).not.toContain("0x7be0…a7c4");
    expect(html).toContain(book.addresses.MembershipStakeVault.slice(0, 6));
    expect(html).toContain(book.addresses.CitrateMemberSBT.slice(0, 6));
  });
});

// ---------------------------------------------------------------------------------------------
// Review follow-up on #348 (chain team).
// ---------------------------------------------------------------------------------------------
import { readFileSync } from "node:fs";
import { Dashboard as Dash } from "./Dashboard";
import { distinctProposers, producingWord, PRODUCER_SAMPLE_BLOCKS } from "../shell/chainTruth";
import { AGENT_SYSTEM_PROMPT, AGENT_TOOLS } from "../agent/harness";
import { WIDGET_TEMPLATES } from "../widgets/gallery";
import { greeting, PERSONAS } from "../shell/state";

/** Quoted string/template literals in a source file, comments stripped. */
function stringLiterals(path: string): string {
  const src = readFileSync(path, "utf8")
    .replace(/\/\*[\s\S]*?\*\//g, "")
    .split("\n")
    .filter((l) => !/^\s*\/\//.test(l))
    .map((l) => l.replace(/\s\/\/ .*$/, ""))
    .join("\n");
  return (src.match(/"(?:[^"\\\n]|\\.)*"|`(?:[^`\\]|\\.)*`|'(?:[^'\\\n]|\\.)*'/g) || []).join("\n");
}

describe("#348 review — §1 Rewards words", () => {
  it("the Node tab is 'Rewards', not 'Earning'", () => {
    const html = renderToStaticMarkup(<Node store={stub} s={member()} />);
    expect(html).toContain(">Rewards<");
    expect(html).not.toContain(">Earning<");
  });

  it("no 'earnings' / 'keep accruing' in agent prompts, tools, suggestions, greeting or toasts", () => {
    expect(AGENT_SYSTEM_PROMPT.toLowerCase()).not.toMatch(/\bearning/);
    expect(JSON.stringify(AGENT_TOOLS).toLowerCase()).not.toContain("earnings");
    expect(greeting(PERSONAS.p2).text.toLowerCase()).not.toContain("earnings");
    for (const f of ["src/components/AgentChat.tsx", "src/shell/store.ts", "src/agent/harness.ts", "src/shell/state.ts"]) {
      const lits = stringLiterals(f).toLowerCase();
      expect(lits, f).not.toMatch(/\bearnings\b/);
      expect(lits, f).not.toContain("keep accruing");
    }
  });
});

describe("#348 review — widget unknowns", () => {
  async function runWidget(key: string, data: unknown): Promise<string> {
    const t = WIDGET_TEMPLATES.find((w) => w.key === key)!;
    // Parse the template as DOM (no regex over HTML): take the script's text, render the rest.
    const tpl = document.createElement("template");
    tpl.innerHTML = t.html;
    const scripts = Array.from(tpl.content.querySelectorAll("script"));
    expect(scripts).toHaveLength(1);
    const script = scripts[0].textContent || "";
    scripts[0].remove();
    const host = document.createElement("div");
    host.appendChild(tpl.content.cloneNode(true));
    const doc = { getElementById: (id: string) => host.querySelector("#" + id), createElement: (n: string) => document.createElement(n) };
    vi.useFakeTimers();
    new Function("citrate", "document", script)({ query: () => Promise.resolve(data) }, doc);
    await vi.advanceTimersByTimeAsync(0);
    vi.clearAllTimers();
    return host.textContent || "";
  }
  it("the SALT widget shows 'claimable —' for an unread claimable, never 'claimable 0'", async () => {
    const t = await runWidget("salt-balance", { liquidSalt: 1.5, stakedSalt: 0, claimableSalt: null });
    expect(t).toContain("claimable —");
    expect(t).not.toContain("claimable 0");
  });
});

describe("#348 review — the activation ceremony states the bond principal, not 32,000", () => {
  // Join-8 (#354): the principal is MemberBond.principal() from the activation read (state.activation),
  // which every activation entry refreshes first.
  it("labels the review from the bond's principal read", async () => {
    (store as unknown as { validatorArmed: boolean }).validatorArmed = true;
    vi.spyOn(store, "refreshActivation").mockImplementation(async () => {
      // A read that passes #408's full activation gate (contract §1), with a 33,500 principal.
      store.setState({
        activation: {
          bondAddress: "0x2222222222222222222222222222222222222222",
          bondDeployed: true,
          bondActivated: false,
          principalWei: wei(33_500),
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
        } as never,
        s7TxHash: null as never,
      });
      return true;
    });
    vi.spyOn(store, "walletIsLinked").mockReturnValue(true);
    store.setState({ walletReview: null });
    vi.spyOn(bridge.node, "registerValidator").mockResolvedValue({ id: "c1", decoded: { action: "MemberBond.activate", cost: "0", destination: "0x" } } as never);
    // Join-8 follow-ups (#378): the guarded entry reads the activation fresh, then opens the review.
    await store.requestValidatorActivation();
    const r = store.state.walletReview!;
    expect(r.label).not.toContain("32,000");
    expect(String(r.spendSummary)).toContain("33,500");
    store.setState({ walletReview: null });
  });
});

describe("#348 review — 'Producing blocks' from a sampled proposer set", () => {
  const ME = "0x" + "11".repeat(32);
  const OTHER = "0x" + "22".repeat(32);

  it("collects the distinct proposer keys of the sampled blocks", () => {
    expect(distinctProposers([{ proposerPubkey: OTHER }, { proposerPubkey: OTHER.toUpperCase().replace("0X", "0x") }, { proposerPubkey: ME }, {}, null])).toEqual([OTHER, ME]);
    expect(PRODUCER_SAMPLE_BLOCKS).toBe(100);
  });

  it("says 'Producing blocks' only when this node's key is in the sample", () => {
    const sample = { head: 500_000, window: 100, proposers: [OTHER] };
    expect(producingWord(sample, OTHER)).toBe("Producing blocks");
    expect(producingWord(sample, ME)).toBe("Not seen producing in the last 100 blocks");
    expect(producingWord(null, ME)).toBeNull();
    expect(producingWord(sample, null)).toBeNull();
  });

  it("samples at most once per 5 minutes", async () => {
    const producerSample = vi.fn(async () => ({ head: 500_000, window: 100, proposers: [OTHER] }));
    (store as unknown as { chainReads: Partial<ChainReads> }).chainReads = { producerSample } as never;
    (store as unknown as { lastProducersAt: number }).lastProducersAt = 0;
    await store.refreshProducers();
    await store.refreshProducers();
    expect(producerSample).toHaveBeenCalledTimes(1);
    expect(store.state.producers?.proposers).toEqual([OTHER]);
  });

  it("a failed sample is unavailable with a reason, never 'not producing'", async () => {
    (store as unknown as { chainReads: Partial<ChainReads> }).chainReads = { producerSample: async () => { throw new Error("rpc 503"); } } as never;
    (store as unknown as { lastProducersAt: number }).lastProducersAt = 0;
    await store.refreshProducers();
    expect(store.state.producers).toBeNull();
    expect(store.state.producersError).toContain("rpc 503");
    const html = renderToStaticMarkup(<Node store={stub} s={member({ bondedStake: 32_000, validatorActivation: ACT_ON, validatorPubkey: ME, producers: null, producersError: "rpc 503" })} />);
    expect(html).toContain("— · producer sample unavailable: rpc 503");
    expect(html.toLowerCase()).not.toContain("not producing");
  });

  it("the Node card shows the word and names the window", () => {
    const seen = renderToStaticMarkup(<Node store={stub} s={member({ bondedStake: 32_000, validatorActivation: ACT_ON, validatorPubkey: OTHER, producers: { head: 1, window: 100, proposers: [OTHER] } })} />);
    expect(seen).toContain("Producing blocks");
    expect(seen).toContain("sampled over the last 100 blocks");
    const unseen = renderToStaticMarkup(<Node store={stub} s={member({ bondedStake: 32_000, validatorActivation: ACT_ON, validatorPubkey: ME, producers: { head: 1, window: 100, proposers: [OTHER] } })} />);
    expect(unseen).toContain("Not seen producing in the last 100 blocks");
    expect(unseen).not.toContain(">Producing blocks<");
  });

  it("the Dashboard shows the producing count next to the active count", () => {
    const html = renderToStaticMarkup(<Dash store={stub} s={member({ producers: { head: 1, window: 100, proposers: [OTHER] } })} />);
    expect(html).toContain("1 producing · sampled over the last 100 blocks");
  });
});

// ---------------------------------------------------------------------------------------------
// Review round 2 on #348 (chain team).
// ---------------------------------------------------------------------------------------------
import { producerWindowLabel, producerShortfall } from "../shell/chainTruth";

describe("#348 round 2 — the producer labels state the N actually sampled", () => {
  const ME = "0x" + "11".repeat(32);
  it("labels a full sample with its real window", () => {
    expect(producerWindowLabel({ head: 1, window: 100, proposers: [] })).toBe("sampled over the last 100 blocks");
    expect(producerShortfall({ head: 1, window: 100, proposers: [] })).toBeNull();
  });
  it("a short sample (N < 100) is '—' with a reason, on Node and the Dashboard", () => {
    const short = { head: 1, window: 37, proposers: [ME] };
    expect(producerShortfall(short)).toBe("only 37 of 100 blocks came back");
    expect(producingWord(short, ME)).toBeNull();
    const node = renderToStaticMarkup(<Node store={stub} s={member({ bondedStake: 32_000, validatorActivation: ACT_ON, validatorPubkey: ME, producers: short })} />);
    expect(node).toContain("— · producer sample incomplete: only 37 of 100 blocks came back");
    expect(node).not.toContain(">Producing blocks<");
    expect(node).not.toContain("last 100 blocks");
    const dash = renderToStaticMarkup(<Dash store={stub} s={member({ producers: short })} />);
    expect(dash).toContain("producing: — · only 37 of 100 blocks came back");
    expect(dash).not.toContain("1 producing");
  });
});

describe("#348 round 2 — no 'earnings' on ALF or in the handbook", () => {
  it("ALF.tsx and seed.ts carry no 'earnings' outside comments", () => {
    for (const f of ["src/surfaces/ALF.tsx", "src/data/seed.ts"]) {
      const code = readFileSync(f, "utf8")
        .replace(/\/\*[\s\S]*?\*\//g, "")
        .split("\n")
        .filter((l) => !/^\s*\/\//.test(l))
        .join("\n")
        .toLowerCase();
      expect(code, f).not.toMatch(/\bearnings\b/);
    }
  });
});

describe("#348 round 2 — the claim path is chosen from the same signal as Rust (has_validator)", () => {
  const grant = (over: Record<string, unknown>) =>
    ({ bondDeployed: true, hasValidator: true, bondedStakeWei: "0", unlockBlock: null, isUnlocked: false, ...over }) as never;
  afterEach(() => vi.restoreAllMocks());

  it("an exited validator (registry stake 0, has_validator) with claimable rewards takes the validator path", async () => {
    store.setState({ bondedStake: 0 });
    vi.spyOn(store as never, "assertPayoutKyc").mockResolvedValue(true as never);
    vi.spyOn(bridge.membership, "grantStatus").mockResolvedValue(grant({}));
    const claim = vi.spyOn(bridge.agent, "claim").mockResolvedValue({ kind: "nothing", claimableWei: "0" });
    await store.claimRewards();
    expect(claim).toHaveBeenCalledWith({ validator: true });
  });

  it("a bonded stake without has_validator does not take the validator path", async () => {
    store.setState({ bondedStake: 32_000 });
    vi.spyOn(store as never, "assertPayoutKyc").mockResolvedValue(true as never);
    vi.spyOn(bridge.membership, "grantStatus").mockResolvedValue(grant({ hasValidator: false }));
    const claim = vi.spyOn(bridge.agent, "claim").mockResolvedValue({ kind: "nothing", claimableWei: "0" });
    await store.claimRewards();
    expect(claim).toHaveBeenCalledWith({ validator: false });
    store.setState({ bondedStake: 0 });
  });

  it("an unreadable grant status stops the claim with a reason instead of guessing", async () => {
    vi.spyOn(store as never, "assertPayoutKyc").mockResolvedValue(true as never);
    vi.spyOn(bridge.membership, "grantStatus").mockRejectedValue(new Error("rpc 503"));
    const claim = vi.spyOn(bridge.agent, "claim");
    await store.claimRewards();
    expect(claim).not.toHaveBeenCalled();
    expect(String(store.getSnapshot().toast)).toContain("rpc 503");
  });
});
