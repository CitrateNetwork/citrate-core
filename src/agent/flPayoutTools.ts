// =====================================================================
// citrate-core: Hermes's payout tools (0.5.4, Hermes x FL WP-H10, gap analysis §5.6), shipped dark
//
//   fl_payouts_list      the member's payout on each offer they joined, and their pull balance   (read)
//   fl_withdraw_propose  propose withdrawing the pull balance: the member signs it                (HIC-1 wallet review)
//   fl_claim_propose     whether one offer's payout can be claimed, and why not                  (named-blocked, read)
//   fl_dispute_propose   whether one offer's payout can be disputed, and why not                 (named-blocked, read)
//
// Gating (policy in core, never here): whether payouts are open is core's `fl_payouts_status`
// (the Rust payouts gate, #736): the profile's payoutsOpen AND WorkMarket and FLRoundVerifier
// pinned with matching live code. While it is not open every tool says "Payouts open after the
// security review" with the gate's own reason and reads nothing more. The withdraw proposal goes
// through store.proposeContractCall, whose Rust side runs the same gate again before the card,
// and the signer runs it once more before the signature.
//
// Data sources (Rule 7), the Payouts board's own (#701): readMyWork (WorkMarket joined, getOffer,
// settled, balanceOf), payoutStatusOf (every WorkMarket.settle precondition, so "Claimable" is
// said only when a claim with a valid leaf would not revert), and the chain's clock
// (latestBlockTime), never the device clock.
//
// Named-blocked, never faked: a claim needs the member's leaf (units, cumulative units before,
// index, Merkle proof) from the coordinator's payout list, and a dispute needs the
// coordinator-signed receipt; the app reads neither (the training worker that keeps them is not
// bundled). fl_claim_propose and fl_dispute_propose therefore open no card in this release: they
// check the gate and the line's status and name what is missing. They are reviewed read-only so the
// approval tripwires hold; wiring a card into either fails the annotation test until it is
// re-annotated as a spend.
// =====================================================================
import { NOT_AVAILABLE, PAYOUTS_DARK, type FlNetworkState } from "../fl/networkState";
import { disputeEnd, NOT_ON_CHAIN, PAYOUT_STATUS_WORDS, payoutStatusOf, readMyWork, type MyWork } from "../fl/myWork";
import { pullBalance, WITHDRAW_SIGNATURE, withdrawCalldata, type ChainOffer, type ViewCall } from "../fl/workMarketReads";
import type { PayoutsStatus } from "../fl/payoutsStatus";
import { windowWords } from "../fl/chainTime";
import { saltText } from "../fl/format";

export const FL_PAYOUT_TOOL_NAMES = ["fl_payouts_list", "fl_withdraw_propose", "fl_claim_propose", "fl_dispute_propose"] as const;
export type FlPayoutToolName = (typeof FL_PAYOUT_TOOL_NAMES)[number];

export function isFlPayoutTool(name: string): name is FlPayoutToolName {
  return (FL_PAYOUT_TOOL_NAMES as readonly string[]).includes(name);
}

export interface WithdrawIntent {
  address: string;
  calldata: string;
  valueWei: string;
  label: string;
}

export interface FlPayoutDeps {
  networkState: () => Promise<FlNetworkState>;
  /** Core's fl_payouts_status (the Rust gate's answer). */
  payoutsStatus: () => Promise<PayoutsStatus>;
  /** This device's wallet address ("" when there is none yet). */
  wallet: () => string;
  call: ViewCall;
  /** The latest block's timestamp (seconds). Throws when it cannot be read. */
  chainNow: () => Promise<number>;
  /** Opens the wallet review for exactly this call (store.proposeContractCall; core gates it again). */
  propose: (intent: WithdrawIntent) => Promise<unknown>;
  /** Test seams; default to the shared reads. */
  readMyWork?: typeof readMyWork;
  pullBalance?: typeof pullBalance;
}

export interface FlPayoutOutcome {
  status: string;
  result: string;
}

/** Why a claim cannot be signed from the app in this release, even on a Claimable line. */
export const CLAIM_NEEDS_LEAF =
  "Claiming needs your leaf from the coordinator's payout list (your units, the units before you, your index and its proof), which the app does not read yet, so it cannot build the claim.";
/** Why a dispute cannot be sent from the app in this release. */
export const DISPUTE_NEEDS_RECEIPT =
  "Disputing needs your coordinator-signed receipt, which the training worker keeps; it is not bundled in this release.";

const msg = (e: unknown) => (e instanceof Error ? e.message : String(e));
const TAG = "[core fl_payouts_status, the Rust payouts gate]";

type Opened = { wm: string; frl: string | null };

/** The gate, from data: not on this network, dark with the gate's reason, unknown, or open. */
async function gate(deps: FlPayoutDeps): Promise<{ text: string } | Opened> {
  let net: FlNetworkState;
  try {
    net = await deps.networkState();
  } catch (e) {
    return { text: "couldn't read which FL contracts this network has: " + msg(e) };
  }
  const wm = net.contracts.WorkMarket;
  if (wm.kind === "not-in-book" || wm.kind === "no-code") return { text: `Payouts: none to read. WorkMarket is ${NOT_AVAILABLE.toLowerCase()} [address book and eth_getCode].` };
  if (wm.kind === "unknown") return { text: `Payouts: — (whether WorkMarket is on this network is unknown: ${wm.reason})` };
  if (wm.kind === "reading") return { text: "Payouts: — (the WorkMarket code read has not answered yet)" };
  let st: PayoutsStatus;
  try {
    st = await deps.payoutsStatus();
  } catch (e) {
    return { text: `Payouts: — (core could not say whether payouts are open: ${msg(e)})` };
  }
  if (!st.open) return { text: `Payouts: ${PAYOUTS_DARK}. ${st.reason ?? "core's payouts gate is closed"} ${TAG}` };
  const frl = net.contracts.FederatedRoundLedger;
  return { wm: wm.address, frl: frl.kind === "deployed" ? frl.address : null };
}

async function readWork(o: Opened, deps: FlPayoutDeps): Promise<MyWork> {
  return (deps.readMyWork ?? readMyWork)({ workMarket: o.wm, frl: o.frl }, deps.wallet(), deps.call);
}

async function chainNowOrNull(deps: FlPayoutDeps): Promise<number | null> {
  try {
    return await deps.chainNow();
  } catch {
    return null;
  }
}

export function formatPayouts(work: MyWork, chainNow: number | null): string {
  const tag = "[WorkMarket, on chain]";
  const lines = [`Pull balance, ready to withdraw: ${saltText(work.pullWei, 4)} ${tag}`];
  let any = false;
  for (const o of work.offers) {
    // Without the chain's time, "waiting" versus "claimable" cannot be told; say so.
    const st = chainNow === null ? null : payoutStatusOf(o, { joined: true, settled: work.settled[o.id] === true }, chainNow);
    if (st === null && chainNow !== null) continue; // no payout list posted yet for this offer
    any = true;
    const status = st === null ? "— (the chain's time was not read, so the window cannot be checked)" : PAYOUT_STATUS_WORDS[st];
    lines.push(`- offer ${o.id}: ${status}; units × ${saltText(o.pricePerUnit)} per unit = — (${NOT_ON_CHAIN.units}); dispute window ${windowWords(disputeEnd(o), chainNow)} ${tag}`);
  }
  if (!any) lines.push("No payout list has been posted for any offer this wallet joined.");
  lines.push("Hermes cannot claim, withdraw or dispute for you; each is your own signature.");
  return lines.join("\n");
}

async function payoutsList(deps: FlPayoutDeps): Promise<FlPayoutOutcome> {
  const g = await gate(deps);
  if ("text" in g) return { status: "ok", result: g.text };
  let work: MyWork;
  try {
    work = await readWork(g, deps);
  } catch (e) {
    return { status: "ok", result: "Payouts: — (" + msg(e) + ")" };
  }
  return { status: "ok", result: formatPayouts(work, await chainNowOrNull(deps)) };
}

async function withdrawPropose(deps: FlPayoutDeps): Promise<FlPayoutOutcome> {
  const g = await gate(deps);
  if ("text" in g) return { status: "ok", result: g.text + " Nothing was proposed." };
  const wallet = deps.wallet();
  if (!/^0x[0-9a-fA-F]{40}$/.test(wallet)) return { status: "ok", result: "There is no wallet on this device yet, so there is no balance to withdraw. Nothing was proposed." };
  let pullWei: bigint;
  try {
    pullWei = await (deps.pullBalance ?? pullBalance)(deps.call, g.wm, wallet);
  } catch (e) {
    return { status: "ok", result: "couldn't read your pull balance, so nothing was proposed: " + msg(e) };
  }
  if (pullWei === 0n) return { status: "ok", result: "Your WorkMarket pull balance is 0 SALT, so there is nothing to withdraw. Nothing was proposed. [WorkMarket.balanceOf, on chain]" };
  try {
    await deps.propose({ address: g.wm, calldata: withdrawCalldata(), valueWei: "0", label: WITHDRAW_SIGNATURE });
  } catch (e) {
    // Core's gate and its wallet checks speak here, in their own words.
    return { status: "error", result: "The withdrawal was not proposed: " + msg(e) };
  }
  return {
    status: "ok",
    result: `Proposed WorkMarket.withdraw() for your whole pull balance, ${saltText(pullWei, 4)}. It is waiting in your wallet review; you read the decoded call and sign it, or not. Hermes cannot sign it.`,
  };
}

function offerIdOf(v: unknown): number | null {
  const n = typeof v === "number" ? v : typeof v === "string" && v.trim() !== "" ? Number(v) : NaN;
  return Number.isInteger(n) && n >= 1 && n <= Number.MAX_SAFE_INTEGER ? n : null;
}

/** Claim and dispute: the gate, then the line's status, then what this release cannot supply. */
async function blockedOnLine(name: "fl_claim_propose" | "fl_dispute_propose", args: Record<string, unknown>, deps: FlPayoutDeps): Promise<FlPayoutOutcome> {
  const id = offerIdOf(args.offerId);
  if (id === null) return { status: "ok", result: `${name} needs the offer id as a whole number from 1. Nothing was proposed.` };
  const g = await gate(deps);
  if ("text" in g) return { status: "ok", result: g.text + " Nothing was proposed." };
  let work: MyWork;
  try {
    work = await readWork(g, deps);
  } catch (e) {
    return { status: "ok", result: "couldn't read your payouts, so nothing was proposed: " + msg(e) };
  }
  const o: ChainOffer | undefined = work.offers.find((x) => x.id === id);
  if (!o) return { status: "ok", result: `This wallet did not join offer ${id} (among the newest offers the app reads). Nothing was proposed.` };
  const now = await chainNowOrNull(deps);
  const st = now === null ? null : payoutStatusOf(o, { joined: true, settled: work.settled[id] === true }, now);
  const line = st === null ? (now === null ? "— (the chain's time was not read)" : "no payout list has been posted yet") : PAYOUT_STATUS_WORDS[st];
  if (name === "fl_claim_propose") {
    if (st !== "claimable") return { status: "ok", result: `Offer ${id}: ${line}. A claim would be refused on chain now, so nothing was proposed.` };
    return { status: "ok", result: `Offer ${id}: ${line}. ${CLAIM_NEEDS_LEAF} Nothing was proposed.` };
  }
  return { status: "ok", result: `Offer ${id}: ${line}. ${DISPUTE_NEEDS_RECEIPT} Nothing was proposed.` };
}

export async function runFlPayoutTool(name: FlPayoutToolName, args: Record<string, unknown>, deps: FlPayoutDeps): Promise<FlPayoutOutcome> {
  switch (name) {
    case "fl_payouts_list":
      return payoutsList(deps);
    case "fl_withdraw_propose":
      return withdrawPropose(deps);
    case "fl_claim_propose":
    case "fl_dispute_propose":
      return blockedOnLine(name, args, deps);
  }
}
