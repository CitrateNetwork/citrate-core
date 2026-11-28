// L0-WP07d: the two FL proposal cards Hermes puts in the chat, moved out of the Store class body
// into the L8 federated learning lane: the worker-terms proposal card (Board 01, HIC-1: core checks
// the terms, the member confirms, the terms are sealed in core) and the round draft card (Board 07:
// a draft the member opens in Group rounds or declines). Each Store method of the same name is a
// one-line delegate to the function here, so every call site (`store.confirmProposal(id)`) is
// unchanged. The code is moved as it was: `this` became `store`, and the Store's private members
// are reached through storeInternals(store).
//
// Data sources (Rule 7): AppState (`chatMsgs`: the message's `proposal` / `roundDraft`), the sealed
// worker terms in core (saveWorkerTerms, fl_work.rs) and the round draft store (setRoundDraft,
// src/fl/roundDraft.ts). Unchanged.

import { dataClassProblem, newProposalId } from "../../../agent/workerProposal";
import { BLANK_DRAFT, draftProblems, setRoundDraft, type RoundDraft } from "../../../fl/roundDraft";
import { saveWorkerTerms, termsProblem, type WorkerTerms } from "../../../fl/workerTerms";
import type { ChatProposal, ChatRoundDraft } from "../../state";
import type { Store } from "../../store";
import { storeInternals } from "../../storeInternals";

/**
 * Put a worker-terms proposal card in the transcript, under reply `msgId` (or as its own Hermes
 * message). Core checks the terms first; malformed terms are refused with the reason and no card
 * is shown. Returns what the caller (the proposal tool, once it ships) tells Hermes.
 * Hermes never confirms: only the card's Confirm calls saveWorkerTerms. The tool that calls this
 * ships with core's worker-policy storage (core #689).
 */
export function proposeWorkerTerms(store: Store, terms: WorkerTerms, msgId?: string): string {
  const internals = storeInternals(store);
  const bad = termsProblem(terms) ?? dataClassProblem(terms.dataClasses);
  if (bad) return `refused: ${bad}. Nothing was shown to the member.`;
  // Unique across launches, so a card restored from history never shares an id with a new one.
  const proposal: ChatProposal = { id: newProposalId(), kind: "worker_terms", terms, state: "pending" };
  if (msgId && store.state.chatMsgs.some((m) => m.id === msgId)) {
    store.setState((s) => ({ chatMsgs: s.chatMsgs.map((m) => (m.id === msgId ? { ...m, proposal } : m)) }));
  } else {
    store.setState((s) => ({ chatMsgs: s.chatMsgs.concat([{ id: "m" + ++internals.mid, who: "Agent", text: "", chips: [], streaming: false, proposal }]) }));
  }
  return "The proposal is on a card in the chat. Nothing changes until the member confirms it there.";
}

export function patchProposal(store: Store, id: string, patch: Partial<ChatProposal>): void {
  store.setState((s) => ({ chatMsgs: s.chatMsgs.map((m) => (m.proposal?.id === id ? { ...m, proposal: { ...m.proposal, ...patch } } : m)) }));
}

/** The member pressed Confirm on the card (HIC-1): save the terms, then show the outcome. */
export async function confirmProposal(store: Store, id: string): Promise<void> {
  const internals = storeInternals(store);
  const p = store.state.chatMsgs.find((m) => m.proposal?.id === id)?.proposal;
  if (!p || (p.state !== "pending" && p.state !== "failed")) return;
  internals.patchProposal(id, { state: "saving", error: undefined });
  try {
    const receipt = await saveWorkerTerms(p.terms);
    internals.patchProposal(id, { state: "confirmed", at: receipt.savedAt || Date.now(), receipt });
  } catch (e) {
    internals.patchProposal(id, { state: "failed", error: e instanceof Error ? e.message : String(e) });
  }
}

export function declineProposal(store: Store, id: string): void {
  const internals = storeInternals(store);
  internals.patchProposal(id, { state: "declined", at: Date.now() });
}

export function reopenProposal(store: Store, id: string): void {
  const internals = storeInternals(store);
  const p = store.state.chatMsgs.find((m) => m.proposal?.id === id)?.proposal;
  // An expired card stays expired: only a new proposal can be confirmed.
  if (!p || p.state === "expired") return;
  internals.patchProposal(id, { state: "pending", at: undefined, error: undefined });
}

/** Show Hermes's round draft on a card under reply `msgId`. Nothing is saved until the member
 *  presses "Review in Open a round". */
export function proposeRoundDraft(store: Store, draft: Partial<RoundDraft>, msgId?: string): string {
  const internals = storeInternals(store);
  const card: ChatRoundDraft = { id: newProposalId(), draft, state: "pending" };
  if (msgId && store.state.chatMsgs.some((m) => m.id === msgId)) {
    store.setState((s) => ({ chatMsgs: s.chatMsgs.map((m) => (m.id === msgId ? { ...m, roundDraft: card } : m)) }));
  } else {
    store.setState((s) => ({ chatMsgs: s.chatMsgs.concat([{ id: "m" + ++internals.mid, who: "Agent", text: "", chips: [], streaming: false, roundDraft: card }]) }));
  }
  const missing = draftProblems({ ...BLANK_DRAFT, ...draft, source: "hermes" });
  return (
    "The round draft is on a card in the chat. Nothing is saved or opened until the member presses Review in Open a round and signs it there." +
    (missing.length ? " Still to fill in: " + missing.join("; ") + "." : "")
  );
}

export function patchRoundDraft(store: Store, id: string, patch: Partial<ChatRoundDraft>): void {
  store.setState((s) => ({ chatMsgs: s.chatMsgs.map((m) => (m.roundDraft?.id === id ? { ...m, roundDraft: { ...m.roundDraft, ...patch } } : m)) }));
}

/** The member pressed "Review in Open a round": the draft goes to that board, marked Hermes's. */
export function openRoundDraft(store: Store, id: string): void {
  const internals = storeInternals(store);
  const c = store.state.chatMsgs.find((m) => m.roundDraft?.id === id)?.roundDraft;
  if (!c || c.state === "expired" || c.state === "declined") return;
  try {
    setRoundDraft({ ...c.draft, draftedFor: "this Hermes chat" }, "hermes");
  } catch (e) {
    store.toast("The draft could not be saved on this device: " + (e instanceof Error ? e.message : String(e)));
    return;
  }
  internals.patchRoundDraft(id, { state: "opened", at: Date.now() });
  store.go("groups/rounds/open");
}

export function declineRoundDraft(store: Store, id: string): void {
  const internals = storeInternals(store);
  const c = store.state.chatMsgs.find((m) => m.roundDraft?.id === id)?.roundDraft;
  if (!c || c.state !== "pending") return;
  internals.patchRoundDraft(id, { state: "declined", at: Date.now() });
}
