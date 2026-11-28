// L0-WP07c: Board 01 edit, retry and branch, moved out of the Store class body into the L4
// conversations lane (next to actions/l4/chatHistory.ts). Each Store method of the same name is a
// one-line delegate to the function here, so every call site (`store.branchChat(...)`) is
// unchanged. The code is moved as it was: `this` became `store`, and the Store's private members
// are reached through storeInternals(store).
//
// Data sources (Rule 7): AppState (chatMsgs, chatThreadId) and the sealed chat history slice
// (slices/chatHistory.ts: the branch's parent is recorded through the Store's threadParents);
// the branched message is sent with store.sendChat. Unchanged.

import type { ChatAttachment } from "../../../agent/chatAttachments";
import { expireOpenProposal, newThreadId } from "../../slices/chatHistory";
import type { Store } from "../../store";
import { storeInternals } from "../../storeInternals";

/**
 * Branch the conversation into a new thread. The original stays in History unchanged. The new
 * thread keeps the messages before `fromMsgId` (`includeFrom` keeps that message too), and when
 * `text` is given it is sent there as the next message. Returns false when no switch is allowed
 * (a run is live) or the message is not in this thread.
 */
export async function branchChat(store: Store, fromMsgId: string, opts: { text?: string; includeFrom?: boolean; attachments?: ChatAttachment[] } = {}): Promise<boolean> {
  const internals = storeInternals(store);
  const blocked = internals.chatSwitchBlocked();
  if (blocked) {
    store.toast(blocked);
    return false;
  }
  const msgs = store.state.chatMsgs;
  const idx = msgs.findIndex((m) => m.id === fromMsgId);
  if (idx < 0) return false;
  store.archiveChat();
  const fromThread = store.state.chatThreadId;
  const kept = msgs.slice(0, opts.includeFrom ? idx + 1 : idx).filter((m) => !m.streaming);
  const threadId = newThreadId();
  // A branch never carries an open proposal: its card is expired there (Hermes proposes again).
  store.setState({
    chatMsgs: kept.map((m) => ({
      ...m,
      id: "b" + ++internals.mid,
      ...(m.proposal ? { proposal: expireOpenProposal(m.proposal) } : {}),
      ...(m.roundDraft?.state === "pending" ? { roundDraft: { ...m.roundDraft, state: "expired" as const } } : {}),
    })),
    chatThreadId: threadId,
    chatQueue: [],
  });
  // The sealed history records where this branch came from.
  internals.threadParents.set(threadId, { threadId: fromThread, atIndex: opts.includeFrom ? idx + 1 : idx });
  if (store.chatInputEl) store.chatInputEl.value = "";
  await store.rebuildProvider();
  // Earlier messages are never put in the system prompt. The in-app loops send them as earlier
  // turns; a Hermes sidecar session starts the branch without them (core #731 passes them as
  // session history once the bundled sidecar accepts it), and the member is told so.
  const sidecar = store.activeProviderKind() === "sidecar" || store.activeProviderKind() === "sidecar-cloud";
  store.toast(
    sidecar && kept.some((m) => m.who === "You")
      ? "Branched into a new chat. The original stays in History. Hermes starts this branch without the earlier messages."
      : "Branched into a new chat. The original stays in History.",
  );
  // An edited or retried message keeps what was attached to the original (unless given anew).
  const from = msgs[idx];
  const attachments = opts.attachments ?? (from.who === "You" ? from.attachments : undefined);
  if (opts.text !== undefined) await store.sendChat(opts.text, attachments?.length ? { attachments } : {});
  return true;
}

/** Retry an answer as a branch: its member message is sent again, on the current model. */
export async function retryAsBranch(store: Store, answerId: string): Promise<boolean> {
  const msgs = store.state.chatMsgs;
  const idx = msgs.findIndex((m) => m.id === answerId);
  if (idx < 0) return false;
  const user = [...msgs.slice(0, idx)].reverse().find((m) => m.who === "You");
  if (!user) return false;
  return store.branchChat(user.id, { text: user.text, attachments: user.attachments ?? [] });
}
