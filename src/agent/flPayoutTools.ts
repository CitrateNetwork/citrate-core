// =====================================================================
// citrate-core: Hermes's payouts read (0.5.4, Hermes x FL WP-H10, gap analysis §5.6), shipped dark
//
//   fl_payouts_list   the member's payout on each WorkMarket offer they joined, and their pull balance   (read)
//
// Gating, from data only: nothing is read unless the address book pins WorkMarket with code on
// chain AND the network profile's `payoutsOpen` is set (G-SEC). On r1007 WorkMarket is not
// pinned; once it is, payouts stay dark until the profile flips, and the answer is the required
// "Payouts open after the security review". No app update is needed for either step.
//
// Data sources (Rule 7), the Payouts board's own (src/fl/myWork.ts, workMarketReads.ts, #701):
//  - readMyWork: the offers this wallet joined (WorkMarket.joined/getOffer), whether it settled
//    each (WorkMarket.settled), and its pull balance (WorkMarket.balanceOf);
//  - payoutStatusOf: every WorkMarket.settle precondition, so "Claimable" is said only when a claim
//    with a valid leaf would not revert;
//  - the dispute window against the chain's own clock (chainTime.ts latestBlockTime), never the
//    device clock; unread is said.
// What the chain does not hold is said, never filled in: the member's units and amount are leaves
// of the coordinator's payout list (NOT_ON_CHAIN.units), so each line shows the rate and "—".
// =====================================================================
import { NOT_AVAILABLE, PAYOUTS_DARK, type FlNetworkState } from "../fl/networkState";
import { disputeEnd, NOT_ON_CHAIN, PAYOUT_STATUS_WORDS, payoutStatusOf, readMyWork, type MyWork } from "../fl/myWork";
import type { ViewCall } from "../fl/workMarketReads";
import { windowWords } from "../fl/chainTime";
import { saltText } from "../fl/format";

export const FL_PAYOUT_TOOL_NAMES = ["fl_payouts_list"] as const;
export type FlPayoutToolName = (typeof FL_PAYOUT_TOOL_NAMES)[number];

export function isFlPayoutTool(name: string): name is FlPayoutToolName {
  return (FL_PAYOUT_TOOL_NAMES as readonly string[]).includes(name);
}

export interface FlPayoutDeps {
  networkState: () => Promise<FlNetworkState>;
  /** This device's wallet address ("" when there is none yet). */
  wallet: () => string;
  call: ViewCall;
  /** The latest block's timestamp (seconds). Throws when it cannot be read. */
  chainNow: () => Promise<number>;
  /** Test seam; defaults to the shared readMyWork. */
  readMyWork?: typeof readMyWork;
}

const msg = (e: unknown) => (e instanceof Error ? e.message : String(e));

/** Why payouts are not read on this network, in the required words, or null when they are open. */
export function payoutsGate(net: FlNetworkState): string | null {
  const tag = "[address book, eth_getCode and the network profile]";
  switch (net.payouts) {
    case "not-deployed":
      return `Payouts: none to read. WorkMarket is ${NOT_AVAILABLE.toLowerCase()} ${tag}.`;
    case "dark":
      return `Payouts: ${PAYOUTS_DARK}. ${tag}`;
    case "unknown":
      return `Payouts: — (whether WorkMarket is on this network is unknown${net.phaseReason ? `: ${net.phaseReason}` : ""})`;
    case "open":
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
    lines.push(
      `- offer ${o.id}: ${status}; units × ${saltText(o.pricePerUnit)} per unit = — (${NOT_ON_CHAIN.units}); dispute window ${windowWords(disputeEnd(o), chainNow)} ${tag}`,
    );
  }
  if (!any) lines.push("No payout list has been posted for any offer this wallet joined.");
  lines.push("Hermes cannot claim, withdraw or dispute for you; each is your own signature.");
  return lines.join("\n");
}

async function payoutsList(deps: FlPayoutDeps): Promise<string> {
  let net: FlNetworkState;
  try {
    net = await deps.networkState();
  } catch (e) {
    return "couldn't read which FL contracts this network has: " + msg(e);
  }
  const gate = payoutsGate(net);
  if (gate) return gate;
  const wm = net.contracts.WorkMarket;
  if (wm.kind !== "deployed") return "Payouts: — (WorkMarket's code was not confirmed on chain)";
  const frl = net.contracts.FederatedRoundLedger;
  let work: MyWork;
  try {
    work = await (deps.readMyWork ?? readMyWork)({ workMarket: wm.address, frl: frl.kind === "deployed" ? frl.address : null }, deps.wallet(), deps.call);
  } catch (e) {
    return "Payouts: — (" + msg(e) + ")";
  }
  let now: number | null = null;
  try {
    now = await deps.chainNow();
  } catch {
    now = null;
  }
  return formatPayouts(work, now);
}

export async function runFlPayoutTool(_name: FlPayoutToolName, _args: Record<string, unknown>, deps: FlPayoutDeps): Promise<string> {
  return payoutsList(deps);
}
