// L0-WP07a: the Hermes chat history actions (0.5.4, slices/chatHistory.ts), moved out of the
// Store class body (store.ts "0.5.4 — Hermes chat history") into the L4 conversations lane. Each
// Store method of the same name is a one-line delegate to the function here, so every call site
// (`store.newChat()`) is unchanged. The code is moved as it was: `this` became `store`, and the
// Store's private members are reached through storeInternals(store).
//
// Data sources (Rule 7): the current thread is AppState (`chatMsgs`, `chatThreadId`, `chatStatus`,
// `chatRouteHold`); the kept threads are the sealed chat history slice (slices/chatHistory.ts:
// recordThread, forgetThread, forgetAllThreads), which core seals on disk.

import { chatHistory, forgetAllThreads, forgetThread, newThreadId, recordThread } from "../../slices/chatHistory";
import { greeting, type ChatMsg } from "../../state";
import type { Store } from "../../store";
import { storeInternals } from "../../storeInternals";

export function scheduleChatArchive(store: Store): void {
  const internals = storeInternals(store);
  if (internals.archiveT) clearTimeout(internals.archiveT);
  internals.archiveT = setTimeout(() => {
    internals.archiveT = null;
    store.archiveChat();
  }, 500);
}

/** Keep the current thread in the history, once no turn is running (a streaming reply is kept
 *  when it has finished). A greeting-only thread is not kept. */
export function archiveChat(store: Store): void {
  const s = store.state;
  if (s.chatStatus !== "ready" || s.chatMsgs.some((m) => m.streaming)) return;
  recordThread(s.chatThreadId, s.chatMsgs, Date.now(), storeInternals(store).threadParents.get(s.chatThreadId));
}

/** Why the thread can't be switched right now, or null. A running turn writes into the current
 *  thread, so it is never swapped from under it. */
export function chatSwitchBlocked(store: Store): string | null {
  if (store.state.chatStatus !== "ready" || store.state.chatRouteHold) return "Finish or stop the current reply first";
  return null;
}

/** Start a new conversation. The current one stays in the history. Hermes's agent session is
 *  rebuilt, so the new conversation does not carry the previous one's context. */
export function newChat(store: Store): boolean {
  const blocked = storeInternals(store).chatSwitchBlocked();
  if (blocked) {
    store.toast(blocked);
    return false;
  }
  store.archiveChat();
  // A follow-up queued in the old conversation belongs to it: it is dropped, never sent here.
  store.setState({ chatMsgs: [greeting({ ...store.persona(), name: store.identity().real ? store.identity().name : "" })], chatThreadId: newThreadId(), chatContextOff: [], chatQueue: [] });
  if (store.chatInputEl) store.chatInputEl.value = "";
  void store.rebuildProvider();
  return true;
}

/** Reopen a conversation from the history. Its messages come back as they were kept; ids are
 *  re-keyed so they never collide with new messages. Hermes's agent session is rebuilt (the
 *  sidecar session has no record of this thread). */
export function openChat(store: Store, threadId: string): boolean {
  if (threadId === store.state.chatThreadId) return true;
  const thread = chatHistory.get().threads.find((t) => t.id === threadId);
  if (!thread) {
    store.toast("That conversation is no longer in this device's history");
    return false;
  }
  const blocked = storeInternals(store).chatSwitchBlocked();
  if (blocked) {
    store.toast(blocked);
    return false;
  }
  store.archiveChat();
  const msgs: ChatMsg[] = thread.msgs.map((m, i) => ({ ...m, id: "h" + i + "-" + thread.id, streaming: false, chips: m.chips ?? [] }));
  store.setState({ chatMsgs: msgs, chatThreadId: thread.id, chatContextOff: [], chatQueue: [] });
  // The reopened thread is recorded under its own id with the re-keyed messages, so nothing
  // changed means nothing moves in the list.
  recordThread(thread.id, msgs, thread.updatedAt);
  if (store.chatInputEl) store.chatInputEl.value = "";
  void store.rebuildProvider();
  return true;
}

/** Erase a conversation from this device's history: core removes its sealed file and rotates the
 *  history key, so no copy of it opens again. The current thread cannot be erased from under the
 *  member. */
export function forgetChat(store: Store, threadId: string): Promise<void> {
  if (threadId === store.state.chatThreadId) return Promise.resolve();
  return forgetThread(threadId);
}

/** Erase every conversation, the current one included, and the history key. The chat starts
 *  over (a fresh thread and agent session), so nothing erased is sealed again. */
export async function forgetAllChats(store: Store): Promise<boolean> {
  const internals = storeInternals(store);
  const blocked = internals.chatSwitchBlocked();
  if (blocked) {
    store.toast(blocked);
    return false;
  }
  if (internals.archiveT) {
    clearTimeout(internals.archiveT);
    internals.archiveT = null;
  }
  const ok = await forgetAllThreads();
  if (!ok) return false;
  store.setState({ chatMsgs: [greeting({ ...store.persona(), name: store.identity().real ? store.identity().name : "" })], chatThreadId: newThreadId(), chatContextOff: [], chatQueue: [] });
  if (store.chatInputEl) store.chatInputEl.value = "";
  void store.rebuildProvider();
  return true;
}
