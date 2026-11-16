// =====================================================================
// citrate-core: Hermes's offers and join tools (0.5.4, Hermes x FL WP-H5, gap analysis §5.3)
//
//   fl_offers_list   WorkMarket offers, matched to the member's terms and machine   (read)
//   fl_join_propose  propose joining one offer: the member signs the join bond       (HIC-1 wallet review)
//
// Data sources (Rule 7), the same ones Rounds › Matches uses (src/fl/Matches.tsx, market.ts):
//  - which FL contracts this network has: address book plus eth_getCode (networkState.ts);
//  - offers: core's fl_market_offers (FLM-CORE-1, #625), read only once WorkMarket is deployed;
//  - the member's worker terms: core's sealed store (workerTerms.ts, lane 1);
//  - this machine's GPU memory: core's tier probe (bridge.tier.recommend);
//  - the join itself: Matches' joinIntent (WorkMarket.join at the offer's own rate, msg.value =
//    the join bond read from getOffer), then store.proposeContractCall, which opens the wallet
//    review. Hermes never signs: the member reads the decoded call and signs it, or does not.
//
// Policy stays in the shared code, not here: joinIntent refuses until payouts open (G-SEC) and
// refuses a full or closed offer; core's payouts gate replaces its payouts check when it lands.
// Matching is never by amount (rankMatches). On r1007 WorkMarket is not pinned, so both tools say
// "Group rounds not live yet" from the data and read nothing else.
//
// Offer text (requirements data classes, poster addresses) is written by whoever posted the offer,
// so the per-offer lines reach the model fenced as untrusted data.
// =====================================================================
import { NOT_AVAILABLE, ROUNDS_NOT_LIVE, type FlNetworkState } from "../fl/networkState";
import { hiddenWords, matchOffer, rankMatches, splitOffers, type MachineFacts, type MarketView, type OfferView } from "../fl/market";
import { joinIntent } from "../fl/Matches";
import type { WorkerTerms } from "../fl/workerTerms";
import { saltText } from "../fl/format";
import { fenceUntrusted } from "./untrusted";

export const FL_MARKET_TOOL_NAMES = ["fl_offers_list", "fl_join_propose"] as const;
export type FlMarketToolName = (typeof FL_MARKET_TOOL_NAMES)[number];

export function isFlMarketTool(name: string): name is FlMarketToolName {
  return (FL_MARKET_TOOL_NAMES as readonly string[]).includes(name);
}

export interface JoinIntent {
  address: string;
  calldata: string;
  valueWei: string;
  label: string;
}

export interface FlMarketDeps {
  networkState: () => Promise<FlNetworkState>;
  readMarket: () => Promise<MarketView>;
  /** The member's worker terms, or null when not set. Throws when they cannot be read. */
  terms: () => Promise<WorkerTerms | null>;
  machine: () => Promise<MachineFacts>;
  /** Opens the wallet review for exactly this call (store.proposeContractCall). */
  propose: (intent: JoinIntent) => Promise<unknown>;
}

export interface FlMarketOutcome {
  status: string;
  result: string;
}

const msg = (e: unknown) => (e instanceof Error ? e.message : String(e));

function offerIdOf(v: unknown): number | null {
  const n = typeof v === "number" ? v : typeof v === "string" && v.trim() !== "" ? Number(v) : NaN;
  return Number.isInteger(n) && n >= 1 && n <= Number.MAX_SAFE_INTEGER ? n : null;
}

/** "Group rounds not live yet" from the data, or null when WorkMarket is there. */
function notLive(net: FlNetworkState): string | null {
  const wm = net.contracts.WorkMarket;
  if (wm.kind === "deployed") return null;
  if (wm.kind === "unknown") return `Offers: — (whether WorkMarket is on this network is unknown: ${wm.reason})`;
  if (wm.kind === "reading") return "Offers: — (the WorkMarket code read has not answered yet)";
  return `${ROUNDS_NOT_LIVE}. WorkMarket is ${NOT_AVAILABLE.toLowerCase()} [${net.networkLabel}, address book and eth_getCode], so there are no offers to list or join.`;
}

function offerLine(o: OfferView, why: string): string {
  const req = o.requirements.status === "verified" ? o.requirements.requirements : null;
  return [
    `offer ${o.opened.id} (${o.opened.kind === "flRound" ? "federated round" : "task"}) by ${o.opened.poster}`,
    `rate ${saltText(BigInt(o.opened.pricePerUnit))} per unit`,
    o.terms ? `workers ${o.terms.workers} of ${o.terms.maxWorkers}, join bond ${saltText(BigInt(o.terms.joinBond))}` : "terms not read",
    req ? `needs ${req.minVramGb} GB, about ${req.estHours} h, data ${req.dataClasses.join(", ") || "none named"}` : "requirements not read",
    `deadline ${new Date(o.opened.deadline * 1000).toISOString()}`,
    why,
  ].join("; ");
}

async function context(deps: FlMarketDeps): Promise<{ terms: WorkerTerms | null; termsNote: string | null; machine: MachineFacts }> {
  let terms: WorkerTerms | null = null;
  let termsNote: string | null = null;
  try {
    terms = await deps.terms();
  } catch (e) {
    termsNote = `your worker terms could not be read (${msg(e)}), so offers are matched without them`;
  }
  let machine: MachineFacts = { memoryGb: null, memoryKind: null };
  try {
    machine = await deps.machine();
  } catch {
    /* matchOffer says "this machine's GPU memory was not read" */
  }
  return { terms, termsNote, machine };
}

async function offersList(args: Record<string, unknown>, deps: FlMarketDeps): Promise<FlMarketOutcome> {
  let net: FlNetworkState;
  try {
    net = await deps.networkState();
  } catch (e) {
    return { status: "ok", result: "couldn't read which FL contracts this network has: " + msg(e) };
  }
  const nl = notLive(net);
  if (nl) return { status: "ok", result: nl };
  let view: MarketView;
  try {
    view = await deps.readMarket();
  } catch (e) {
    return { status: "ok", result: "Offers: — (core's offers read failed: " + msg(e) + ")" };
  }
  if (!view.available) return { status: "ok", result: `Offers: — (${view.reason ?? "the offers read model is not available"})` };
  const { terms, termsNote, machine } = await context(deps);
  const { shown, hidden } = splitOffers(view);
  const ranked = rankMatches(shown.map((o) => matchOffer(o, terms, machine)));
  const lines = ranked.map((m) => offerLine(m.offer, `${m.chip}. ${m.whyLabel} ${m.why}`));
  if (args.includeHidden === true || args.includeHidden === "true") for (const o of hidden) lines.push(offerLine(o, `hidden: ${hiddenWords(o.hidden!)}`));
  const head = [
    `${shown.length} offers shown, ${hidden.length} hidden [core fl_market_offers: WorkMarket logs and getOffer, requirements checked against their hash]`,
    "Ranked joinable first, then by deadline; never by amount.",
    ...(termsNote ? [termsNote] : []),
    ...(terms ? [] : termsNote ? [] : ["You have not set worker terms, so only each offer's own rules apply."]),
  ];
  return { status: "ok", result: head.join("\n") + (lines.length ? "\n" + fenceUntrusted("WorkMarket offers, written by their posters", lines.join("\n")) : "\nNo offers to show.") };
}

async function joinPropose(args: Record<string, unknown>, deps: FlMarketDeps): Promise<FlMarketOutcome> {
  const id = offerIdOf(args.offerId);
  if (id === null) return { status: "ok", result: "fl_join_propose needs the offer id as a whole number from 1. Nothing was proposed." };
  let net: FlNetworkState;
  try {
    net = await deps.networkState();
  } catch (e) {
    return { status: "ok", result: "couldn't read which FL contracts this network has, so nothing was proposed: " + msg(e) };
  }
  const nl = notLive(net);
  if (nl) return { status: "ok", result: nl };
  let view: MarketView;
  try {
    view = await deps.readMarket();
  } catch (e) {
    return { status: "ok", result: "couldn't read the offers, so nothing was proposed: " + msg(e) };
  }
  const o = view.offers.find((x) => x.opened.id === id);
  if (!o) return { status: "ok", result: `There is no offer ${id} in the offers core has read. Nothing was proposed.` };
  if (o.hidden) return { status: "ok", result: `Offer ${id} is not shown to this member: ${hiddenWords(o.hidden)}. Nothing was proposed.` };
  const { terms, machine } = await context(deps);
  const m = matchOffer(o, terms, machine);
  if (!m.joinable) return { status: "ok", result: `Offer ${id} is not one this machine can join. ${m.whyLabel} ${m.why} Nothing was proposed.` };
  let intent: JoinIntent;
  try {
    intent = joinIntent(o, net);
  } catch (e) {
    return { status: "ok", result: `Offer ${id} cannot be joined from here yet: ${msg(e)} Nothing was proposed.` };
  }
  try {
    await deps.propose(intent);
  } catch (e) {
    return { status: "error", result: "The join was not proposed: " + msg(e) };
  }
  return {
    status: "ok",
    result: `Proposed joining offer ${id}: WorkMarket.join at the offer's own rate, with the ${saltText(BigInt(intent.valueWei))} join bond. It is waiting in your wallet review; you read the decoded call and sign it, or not. Hermes cannot sign it, and nothing is sent until you do.`,
  };
}

export async function runFlMarketTool(name: FlMarketToolName, args: Record<string, unknown>, deps: FlMarketDeps): Promise<FlMarketOutcome> {
  return name === "fl_offers_list" ? offersList(args, deps) : joinPropose(args, deps);
}
