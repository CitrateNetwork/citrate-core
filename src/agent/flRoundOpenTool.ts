// =====================================================================
// citrate-core: Hermes's open-a-round proposal (0.5.4, Hermes x FL WP-H5, gap analysis §5.3)
//
//   fl_round_open_propose   propose opening the saved Open a round draft: the member signs the escrow   (HIC-1 wallet review)
//
// Data sources (Rule 7), the Open a round board's own (lane 1, src/fl/roundDraft.ts):
//  - the draft on this device (loadRoundDraft); Hermes never edits it here, the member's own or the
//    one handed in from round_draft_propose's card;
//  - which FL contracts this network has, and whether payouts are open as core's Rust payouts gate
//    says (store.flNetworkState, the boards' read);
//  - the round binding (the coordinator's on-chain address and FederatedRoundLedger cluster,
//    verifier, job spec, requirements file): no source exists yet, because the training
//    coordinator does not publish one. store.flRoundBinding() answers null until it does;
//  - openIntent: lane 1's one encoding of WorkMarket.open (msg.value = the budget). It refuses, in
//    order, on payouts dark (with the gate's reason), WorkMarket absent, no binding, a bad draft.
//
// Opening escrows the member's SALT, so it is always HIC-1: the wallet review shows the decoded
// call and the member signs it, or not. It never runs inside an HIC-2 envelope. Core's payouts gate
// runs again before the card and at the signer (contract_write_propose, ceremony.rs).
// =====================================================================
import { NOT_AVAILABLE, ROUNDS_NOT_LIVE, type FlNetworkState } from "../fl/networkState";
import { openIntent, type OpenBinding, type RoundDraft } from "../fl/roundDraft";
import { saltText } from "../fl/format";

export const FL_ROUND_OPEN_TOOL = "fl_round_open_propose";

export interface OpenIntent {
  address: string;
  calldata: string;
  valueWei: string;
  label: string;
}

export interface FlRoundOpenDeps {
  loadDraft: () => RoundDraft | null;
  networkState: () => Promise<FlNetworkState>;
  /** The coordinator's round binding, or null while no source publishes one. */
  binding: () => Promise<OpenBinding | null>;
  /** Opens the wallet review for exactly this call (store.proposeContractCall). */
  propose: (intent: OpenIntent) => Promise<unknown>;
}

export interface FlRoundOpenOutcome {
  status: string;
  result: string;
}

const msg = (e: unknown) => (e instanceof Error ? e.message : String(e));

export async function runFlRoundOpen(deps: FlRoundOpenDeps): Promise<FlRoundOpenOutcome> {
  let d: RoundDraft | null;
  try {
    d = deps.loadDraft();
  } catch (e) {
    return { status: "ok", result: "couldn't read the round draft on this device, so nothing was proposed: " + msg(e) };
  }
  if (!d) return { status: "ok", result: "There is no round draft on this device. Propose one with round_draft_propose, or the member writes one on Groups › Rounds › Open a round. Nothing was proposed." };
  let net: FlNetworkState;
  try {
    net = await deps.networkState();
  } catch (e) {
    return { status: "ok", result: "couldn't read which FL contracts this network has, so nothing was proposed: " + msg(e) };
  }
  const wm = net.contracts.WorkMarket;
  if (wm.kind === "not-in-book" || wm.kind === "no-code") {
    return { status: "ok", result: `${ROUNDS_NOT_LIVE}. WorkMarket is ${NOT_AVAILABLE.toLowerCase()} [${net.networkLabel}, address book and eth_getCode], so a round cannot be opened. The draft stays saved. Nothing was proposed.` };
  }
  let binding: OpenBinding | null;
  try {
    binding = await deps.binding();
  } catch (e) {
    return { status: "ok", result: "couldn't read the round binding, so nothing was proposed: " + msg(e) };
  }
  let intent: OpenIntent;
  try {
    intent = openIntent(d, binding, net);
  } catch (e) {
    return { status: "ok", result: `The round cannot be opened from here yet: ${msg(e)}. The draft stays saved. Nothing was proposed.` };
  }
  try {
    await deps.propose(intent);
  } catch (e) {
    return { status: "error", result: "Opening the round was not proposed: " + msg(e) };
  }
  return {
    status: "ok",
    result: `Proposed opening the round in your draft: WorkMarket.open with ${saltText(BigInt(intent.valueWei), 4)} escrowed from your wallet. It is waiting in your wallet review; you read the decoded call and sign it, or not (HIC-1). Hermes cannot sign it.`,
  };
}
