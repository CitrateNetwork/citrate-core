// =====================================================================
// 0.5.4 — Hermes chat history (owner request 2026-11-14: reach prior conversations and projects).
//
// Data source (Rule 7): the app store's own chat thread, `AppState.chatMsgs` (store.sendChat,
// store.acceptBrief, store.runTrackWorkflow write it). Before 0.5.4 that thread lived in memory
// only and was lost on reload. This slice keeps each thread the member had on THIS device, in one
// localStorage entry, the same way the Groups slice keeps drained group messages
// (`citrate.groups.history.v1`). Nothing leaves the device, no backend is involved, and the
// sidecar's session table is not read (it holds no titles and is lost when Hermes restarts).
//
// Projects are not a separate store: a project is a conversation in which the member accepted a
// "Plan it first" brief (`ChatMsg.brief`, from the sidecar's brief_create via store.acceptBrief).
// The Projects list is derived from the same threads.
//
// Bounds: the newest MAX_THREADS threads, MAX_MSGS messages each, and a total size budget, so the
// entry can never crowd out the app's main saved state.
// =====================================================================
import { createSlice } from "./createSlice";
import type { ChatMsg } from "../state";

export const CHAT_HISTORY_KEY = "citrate.hermes.chatHistory.v1";
export const MAX_THREADS = 50;
export const MAX_MSGS = 300;
/** The serialized history's budget (characters); the oldest threads go first past it. */
export const MAX_HISTORY_CHARS = 1_500_000;

export interface ChatThread {
  id: string;
  /** The member's first message, one line, trimmed. */
  title: string;
  createdAt: number;
  updatedAt: number;
  msgs: ChatMsg[];
}

export interface ChatProject {
  threadId: string;
  goal: string;
  track: string;
  workflow: string;
  updatedAt: number;
}

export interface ChatHistoryState {
  threads: ChatThread[];
}

export function newThreadId(now: number = Date.now()): string {
  return "t" + now.toString(36) + Math.random().toString(36).slice(2, 8);
}

/** A member's message counts; a thread holding only the greeting is not worth keeping. */
const hasMemberTurn = (msgs: ChatMsg[]) => msgs.some((m) => m.who === "You" && m.text.trim() !== "");

export function threadTitle(msgs: ChatMsg[]): string {
  const first = msgs.find((m) => m.who === "You" && m.text.trim() !== "");
  const line = (first?.text ?? "").trim().split(/\r?\n/)[0].trim();
  return line.length > 80 ? line.slice(0, 79) + "…" : line || "Untitled conversation";
}

/** The messages as they are kept: finished ones only, without UI-only fields. */
export function archivable(msgs: ChatMsg[]): ChatMsg[] {
  return msgs
    .filter((m) => !m.streaming)
    .slice(-MAX_MSGS)
    .map((m) => {
      const keep: ChatMsg = { id: m.id, who: m.who, text: m.text, chips: m.chips ?? [], streaming: false };
      if (m.error) keep.error = m.error;
      if (m.retryText) keep.retryText = m.retryText;
      if (m.toolchainHelp) keep.toolchainHelp = true;
      if (m.brief) keep.brief = m.brief;
      return keep;
    });
}

const sameMsgs = (a: ChatMsg[], b: ChatMsg[]) =>
  a.length === b.length && a.every((m, i) => m.id === b[i].id && m.text === b[i].text && m.who === b[i].who && m.error === b[i].error);

/** Put `id`'s current messages into the history (newest first). Unchanged threads keep their place. */
export function upsertThread(threads: ChatThread[], id: string, msgs: ChatMsg[], now: number = Date.now()): ChatThread[] {
  if (!hasMemberTurn(msgs)) return threads;
  const kept = archivable(msgs);
  const prev = threads.find((t) => t.id === id);
  if (prev && sameMsgs(prev.msgs, kept)) return threads;
  const thread: ChatThread = { id, title: threadTitle(kept), createdAt: prev?.createdAt ?? now, updatedAt: now, msgs: kept };
  return [thread, ...threads.filter((t) => t.id !== id)].sort((x, y) => y.updatedAt - x.updatedAt).slice(0, MAX_THREADS);
}

/** Case-insensitive match on the title and every message's text. */
export function searchThreads(threads: ChatThread[], query: string): ChatThread[] {
  const q = query.trim().toLowerCase();
  if (!q) return threads;
  return threads.filter((t) => t.title.toLowerCase().includes(q) || t.msgs.some((m) => m.text.toLowerCase().includes(q)));
}

/** Each conversation holding an accepted brief, by its latest brief (newest conversation first). */
export function projectsFrom(threads: ChatThread[]): ChatProject[] {
  const out: ChatProject[] = [];
  for (const t of threads) {
    const b = [...t.msgs].reverse().find((m) => m.brief)?.brief;
    if (b) out.push({ threadId: t.id, goal: b.goal || t.title, track: b.track, workflow: b.workflow, updatedAt: t.updatedAt });
  }
  return out;
}

export function searchProjects(projects: ChatProject[], query: string): ChatProject[] {
  const q = query.trim().toLowerCase();
  if (!q) return projects;
  return projects.filter((p) => [p.goal, p.track, p.workflow].some((f) => (f || "").toLowerCase().includes(q)));
}

const isThread = (t: unknown): t is ChatThread => {
  const o = t as ChatThread;
  return !!o && typeof o.id === "string" && typeof o.title === "string" && typeof o.updatedAt === "number" && Array.isArray(o.msgs);
};

/** Drop the oldest threads until the serialized history fits its budget. */
export function withinBudget(threads: ChatThread[], budget: number = MAX_HISTORY_CHARS): ChatThread[] {
  let out = threads;
  while (out.length > 1 && JSON.stringify(out).length > budget) out = out.slice(0, -1);
  return out;
}

function load(): ChatThread[] {
  try {
    const raw = typeof localStorage !== "undefined" ? localStorage.getItem(CHAT_HISTORY_KEY) : null;
    const parsed = raw ? JSON.parse(raw) : null;
    return Array.isArray(parsed?.threads) ? (parsed.threads as unknown[]).filter(isThread).slice(0, MAX_THREADS) : [];
  } catch {
    return [];
  }
}

function persist(threads: ChatThread[]): void {
  try {
    if (typeof localStorage !== "undefined") localStorage.setItem(CHAT_HISTORY_KEY, JSON.stringify({ v: 1, threads }));
  } catch {
    /* quota / unavailable: the history stays in memory for this session, never throws at the caller */
  }
}

export const chatHistory = createSlice<ChatHistoryState>({ threads: load() });

/** Record `id`'s messages (no-op for a greeting-only or unchanged thread). */
export function recordThread(id: string, msgs: ChatMsg[], now: number = Date.now()): void {
  const cur = chatHistory.get().threads;
  const next = upsertThread(cur, id, msgs, now);
  if (next === cur) return;
  const fitted = withinBudget(next);
  persist(fitted);
  chatHistory.set({ threads: fitted });
}

export function forgetThread(id: string): void {
  const threads = chatHistory.get().threads.filter((t) => t.id !== id);
  persist(threads);
  chatHistory.set({ threads });
}

/** Re-read the saved history (tests; another window of the app wrote it). */
export function reloadChatHistory(): void {
  chatHistory.set({ threads: load() });
}
