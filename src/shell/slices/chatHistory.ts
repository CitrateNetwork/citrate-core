// =====================================================================
// 0.5.4 — Hermes chat history (owner request 2026-11-14: reach earlier conversations and projects).
//
// Data source (Rule 7): the app store's own chat thread, `AppState.chatMsgs` (written by
// store.sendChat, store.acceptBrief and store.runTrackWorkflow), kept per thread in core's SEALED
// history store (src-tauri/src/hermes_history.rs). There, each conversation is a file under
// <app data>/hermes-history/, sealed with the federation's at-rest envelope: AES-256-GCM, with a
// per-file key wrapped under an ML-KEM-768 encapsulation. The key is device-sealed in the OS
// keyring (ADR-2026-10-26-byok-provider-governance P-11; owner decision 2026-11-14).
//
// The webview never writes chat text to localStorage. It holds the decrypted threads in memory
// only, for list, search and Projects. Every save, delete, retention change and full erase goes
// through a `hermes_history_*` command. A plaintext entry that an earlier development build
// wrote (`citrate.hermes.chatHistory.v1`) is moved into the sealed store and removed on load.
//
// Projects are not a separate store. A project is a conversation in which the member accepted a
// "Plan it first" brief (`ChatMsg.brief`, from the sidecar's brief_create via store.acceptBrief).
//
// The web preview has no sealed store. There, conversations are kept for the session only, and
// the panel says so (Rule 1).
// =====================================================================
import { createSlice } from "./createSlice";
import type { ChatMsg } from "../state";
import { parseRun } from "../../agent/runTimeline";

/** The key an earlier development build used for PLAINTEXT history. Read once to move it into the
 *  sealed store, then removed. Nothing writes it. */
export const LEGACY_PLAINTEXT_KEY = "citrate.hermes.chatHistory.v1";
/** Must match `hermes_history::MAX_THREADS` (core erases beyond it). */
export const MAX_THREADS = 50;
export const MAX_MSGS = 300;
/** Must match `hermes_history::{MIN,MAX}_RETENTION_DAYS`. */
export const MIN_RETENTION_DAYS = 1;
export const MAX_RETENTION_DAYS = 3650;

/** Board 01: where a branch came from: the parent thread, and how many of its messages it kept. */
export interface ThreadParent {
  threadId: string;
  atIndex: number;
}

export interface ChatThread {
  id: string;
  /** Set on a branch (edit, retry or "Branch from here"). Absent on a conversation started fresh. */
  parent?: ThreadParent;
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
  /** The sealed store answered at least once this session. */
  loaded: boolean;
  /** Kept on this device (sealed); false in the web preview (session only). */
  persisted: boolean;
  /** Ids of sealed files that did not open (tampered, moved, or under a key no longer here). */
  unreadable: string[];
  retentionDays: number | null;
  /** The sealed retention setting does not open. Nothing is erased by age, `retentionDays` keeps
   *  the last value this session knew (null if none), and choosing a setting replaces it. */
  retentionUnreadable: boolean;
  /** The last error from the sealed store, for the panel (null = fine). */
  error: string | null;
}

/** What `hermes_history_list` answers (src-tauri/src/hermes_history.rs `HistoryList`). */
interface HistoryListView {
  threads: ChatThread[];
  unreadable: string[];
  retentionDays: number | null;
  retentionUnreadable?: boolean;
  purged: number;
}

/** The IO seam: the runtime and the timeout-wrapped invoke. */
export interface HistoryIo {
  mode: "tauri" | "sim";
  invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T>;
}

let io: HistoryIo | null = null;

async function historyIo(): Promise<HistoryIo> {
  if (io) return io;
  const { BRIDGE_MODE } = await import("../../bridge/mode");
  io = {
    mode: BRIDGE_MODE,
    invoke: async <T,>(cmd: string, args?: Record<string, unknown>) => {
      const { invoke } = await import("../../bridge/tauri/invoke");
      return invoke<T>(cmd, args);
    },
  };
  return io;
}

/** Point the slice at another IO (tests; the desktop IO is the default). */
export function setHistoryIo(next: HistoryIo | null): void {
  io = next;
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

/** An open proposal (pending, saving or failed) becomes expired; a decided one is kept as it is. */
export function expireOpenProposal(p: NonNullable<ChatMsg["proposal"]>): NonNullable<ChatMsg["proposal"]> {
  return p.state === "pending" || p.state === "saving" || p.state === "failed" ? { ...p, state: "expired", error: undefined } : p;
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
      // Board 01: the reply's run timeline and model are kept with it (bounded, re-checked).
      const run = m.run ? parseRun(m.run) : undefined;
      if (run) keep.run = run;
      if (typeof m.model === "string" && m.model) keep.model = m.model.slice(0, 120);
      if (m.steer === "taken") keep.steer = "taken";
      // A proposal that was still open is kept as expired: a card restored from history can never
      // be confirmed with terms proposed in another session (Hermes proposes again).
      if (m.proposal) keep.proposal = expireOpenProposal(m.proposal);
      return keep;
    });
}

const sameMsgs = (a: ChatMsg[], b: ChatMsg[]) =>
  a.length === b.length && a.every((m, i) => m.id === b[i].id && m.text === b[i].text && m.who === b[i].who && m.error === b[i].error);

const byRecency = (x: ChatThread, y: ChatThread) => y.updatedAt - x.updatedAt;

/** Put `id`'s current messages into the history (newest first). Unchanged threads keep their place. */
export function upsertThread(threads: ChatThread[], id: string, msgs: ChatMsg[], now: number = Date.now(), parent?: ThreadParent): ChatThread[] {
  if (!hasMemberTurn(msgs)) return threads;
  const kept = archivable(msgs);
  const prev = threads.find((t) => t.id === id);
  const link = parent ?? prev?.parent;
  if (prev && sameMsgs(prev.msgs, kept) && (prev.parent?.threadId ?? null) === (link?.threadId ?? null)) return threads;
  const thread: ChatThread = { id, title: threadTitle(kept), createdAt: prev?.createdAt ?? now, updatedAt: now, msgs: kept, ...(link ? { parent: link } : {}) };
  return [thread, ...threads.filter((t) => t.id !== id)].sort(byRecency).slice(0, MAX_THREADS);
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

/** A thread's parent link as kept: a well-formed one stays, anything else is dropped. */
const cleanParent = (t: ChatThread): ChatThread => {
  const p = t.parent as unknown as { threadId?: unknown; atIndex?: unknown } | undefined;
  if (p === undefined) return t;
  const ok = typeof p?.threadId === "string" && /^[A-Za-z0-9_-]{1,64}$/.test(p.threadId) && Number.isSafeInteger(p.atIndex) && (p.atIndex as number) >= 0;
  if (ok) return t;
  const { parent: _drop, ...rest } = t;
  return rest;
};

const initial: ChatHistoryState = { threads: [], loaded: false, persisted: false, unreadable: [], retentionDays: null, retentionUnreadable: false, error: null };
export const chatHistory = createSlice<ChatHistoryState>({ ...initial });

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

/** The thread as core keeps it (`ThreadDoc`): exactly these fields. */
const toDoc = (t: ChatThread) => ({ id: t.id, title: t.title, createdAt: t.createdAt, updatedAt: t.updatedAt, msgs: t.msgs, ...(t.parent ? { parent: t.parent } : {}) });

/** Saves are serialized, so a later version of a thread never lands before an earlier one. */
let saving: Promise<void> = Promise.resolve();

async function sealThread(t: ChatThread): Promise<void> {
  const h = await historyIo();
  if (h.mode !== "tauri") return;
  try {
    const out = await h.invoke<{ evicted: string[] }>("hermes_history_save", { thread: toDoc(t) });
    const gone = new Set(out?.evicted ?? []);
    chatHistory.set((s) => ({ error: null, persisted: true, threads: gone.size ? s.threads.filter((x) => !gone.has(x.id)) : s.threads }));
  } catch (e) {
    chatHistory.set({ error: "This conversation could not be saved: " + message(e) });
  }
}

/** Move a plaintext history left by an earlier development build into the sealed store, then
 *  remove it. It is removed only once every conversation in it is sealed (a failed seal keeps
 *  it, and the next launch tries again), or when it does not parse (nothing in it can be kept).
 *  The web preview has no sealed store, so it leaves the entry for the desktop app. */
async function retireLegacyPlaintext(h: HistoryIo): Promise<void> {
  let raw: string | null = null;
  try {
    raw = typeof localStorage !== "undefined" ? localStorage.getItem(LEGACY_PLAINTEXT_KEY) : null;
  } catch {
    raw = null;
  }
  if (raw === null) return;
  const drop = () => {
    try {
      localStorage.removeItem(LEGACY_PLAINTEXT_KEY);
    } catch {
      /* storage unavailable: nothing can be removed */
    }
  };
  let threads: ChatThread[];
  try {
    const parsed = JSON.parse(raw) as { threads?: unknown[] } | null;
    threads = Array.isArray(parsed?.threads) ? parsed.threads.filter(isThread).map(cleanParent).slice(0, MAX_THREADS) : [];
  } catch {
    drop();
    return;
  }
  if (h.mode !== "tauri") return;
  try {
    for (const t of threads) await h.invoke("hermes_history_save", { thread: toDoc({ ...t, msgs: archivable(t.msgs) }) });
  } catch (e) {
    chatHistory.set({ error: "An older, unencrypted copy of your history could not be sealed yet (it is kept and retried next launch): " + message(e) });
    return;
  }
  drop();
}

let loading: Promise<void> | null = null;

/** Open the sealed history (once per session unless `force`). Threads recorded before it answered
 *  are kept when they are newer. */
export function loadChatHistory(force = false): Promise<void> {
  if (loading && !force) return loading;
  loading = (async () => {
    const h = await historyIo();
    await retireLegacyPlaintext(h);
    if (h.mode !== "tauri") {
      chatHistory.set({ loaded: true, persisted: false, error: null });
      return;
    }
    try {
      const view = await h.invoke<HistoryListView>("hermes_history_list");
      const sealed = (view?.threads ?? []).filter(isThread).map(cleanParent);
      chatHistory.set((s) => {
        const merged = new Map<string, ChatThread>();
        for (const t of sealed) merged.set(t.id, t);
        for (const t of s.threads) {
          const prev = merged.get(t.id);
          if (!prev || prev.updatedAt < t.updatedAt) merged.set(t.id, t);
        }
        const retentionUnreadable = view?.retentionUnreadable === true;
        return {
          threads: [...merged.values()].sort(byRecency).slice(0, MAX_THREADS),
          unreadable: view?.unreadable ?? [],
          // An unreadable setting is unknown, not "until deleted": keep the last known value.
          retentionDays: retentionUnreadable ? s.retentionDays : view?.retentionDays ?? null,
          retentionUnreadable,
          loaded: true,
          persisted: true,
          error: s.error && s.error.startsWith("An older, unencrypted copy") ? s.error : null,
        };
      });
    } catch (e) {
      chatHistory.set({ loaded: true, persisted: false, error: "The chat history could not be opened: " + message(e) });
    }
  })();
  return loading;
}

/** Record `id`'s messages (no-op for a greeting-only or unchanged thread) and seal them. */
export function recordThread(id: string, msgs: ChatMsg[], now: number = Date.now(), parent?: ThreadParent): void {
  const cur = chatHistory.get().threads;
  const next = upsertThread(cur, id, msgs, now, parent);
  if (next === cur) return;
  chatHistory.set({ threads: next });
  const t = next.find((x) => x.id === id);
  if (t) saving = saving.then(() => sealThread(t));
}

/** Erase one conversation (core removes its file and rotates the history key). */
export async function forgetThread(id: string): Promise<void> {
  chatHistory.set((s) => ({ threads: s.threads.filter((t) => t.id !== id) }));
  const h = await historyIo();
  if (h.mode !== "tauri") return;
  await saving;
  try {
    await h.invoke("hermes_history_delete", { id });
    chatHistory.set((s) => ({ unreadable: s.unreadable.filter((u) => u !== id), error: null }));
  } catch (e) {
    chatHistory.set({ error: "That conversation could not be erased: " + message(e) });
  }
}

/** Erase every conversation and the history key. `keep` is the current thread, which stays on
 *  screen (and is sealed again under a new key the next time it changes). */
export async function forgetAllThreads(): Promise<boolean> {
  const h = await historyIo();
  if (h.mode === "tauri") {
    await saving;
    try {
      await h.invoke("hermes_history_delete_all");
    } catch (e) {
      chatHistory.set({ error: "The chat history could not be erased: " + message(e) });
      return false;
    }
  }
  chatHistory.set({ threads: [], unreadable: [], error: null });
  return true;
}

/** Keep conversations `days` after their last update, or until deleted (null). */
export async function setHistoryRetention(days: number | null): Promise<boolean> {
  if (days !== null && (!Number.isInteger(days) || days < MIN_RETENTION_DAYS || days > MAX_RETENTION_DAYS)) {
    chatHistory.set({ error: `Keep history between ${MIN_RETENTION_DAYS} and ${MAX_RETENTION_DAYS} days, or until deleted.` });
    return false;
  }
  const h = await historyIo();
  if (h.mode !== "tauri") {
    chatHistory.set({ error: "The web preview keeps conversations for this session only." });
    return false;
  }
  try {
    // The member choosing a setting is the reset of an unreadable one (the panel said so).
    await h.invoke("hermes_history_set_retention", { days, replaceUnreadable: chatHistory.get().retentionUnreadable });
  } catch (e) {
    chatHistory.set({ error: "The retention setting could not be saved: " + message(e) });
    return false;
  }
  // The list applies it (and erases what is already past it).
  await loadChatHistory(true);
  return true;
}

/** Wait for every queued save (tests; before an erase). */
export function historySettled(): Promise<void> {
  return saving;
}

/** Forget the session's in-memory state (tests). */
export function resetChatHistoryForTests(): void {
  loading = null;
  saving = Promise.resolve();
  chatHistory.set({ ...initial });
}
