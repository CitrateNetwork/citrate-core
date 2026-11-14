// 0.5.4 (owner request 2026-11-14; ADR P-11) — earlier conversations and projects can be reached
// from the Hermes chat. A History entry point lists them, searches them and reopens one, and there
// are retention, encrypted export and real delete.
//
// Data source (Rule 7): the store's own thread (AppState.chatMsgs), sealed per thread by core's
// hermes_history_* commands (ML-KEM-768 + AES-256-GCM, device-sealed key). The Rust side is
// tested in src-tauri/src/hermes_history_tests.rs.
//
// Here the commands are a TEST DOUBLE of that contract, an in-memory map. It checks what the
// webview sends and that no chat text ever reaches localStorage.
import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { AgentChat } from "./AgentChat";
import { store } from "../shell/store";
import { freshState, STORAGE_KEY, type ChatMsg } from "../shell/state";
import {
  LEGACY_PLAINTEXT_KEY,
  MAX_THREADS,
  chatHistory,
  historySettled,
  loadChatHistory,
  projectsFrom,
  resetChatHistoryForTests,
  searchThreads,
  setHistoryIo,
  setHistoryRetention,
  threadTitle,
  upsertThread,
  type ChatThread,
  type HistoryIo,
} from "../shell/slices/chatHistory";
import { threadsAsJournalPages } from "./historyExport";
import { buildBundle, parseBundle } from "../journal/bundle";
import type { Brief } from "../bridge/domains";

(globalThis as unknown as { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const msg = (id: string, who: string, text: string, extra: Partial<ChatMsg> = {}): ChatMsg => ({ id, who, text, chips: [], streaming: false, ...extra });
const greetingOnly = [msg("g", "Agent", "Hello, I'm Hermes.")];
const brief: Brief = { track: "code", goal: "ship the faucet page", constraints: [], persona: "Builder", skills: [], workflow: "code-change", workflow_available: false, ships_in: "0.5.0", gates: ["tests pass"] };
const SECRET = "my validator bond unlocks at block 991234";

/** The hermes_history_* contract, in memory (test double for src-tauri/src/hermes_history.rs). */
function sealedStoreDouble(mode: "tauri" | "sim" = "tauri") {
  const files = new Map<string, ChatThread>();
  let retentionDays: number | null = null;
  const state = { settingsUnreadable: false, failSaves: false };
  const calls: { cmd: string; args?: Record<string, unknown> }[] = [];
  const io: HistoryIo = {
    mode,
    invoke: async <T,>(cmd: string, args?: Record<string, unknown>): Promise<T> => {
      calls.push({ cmd, args });
      switch (cmd) {
        case "hermes_history_list": {
          const threads = [...files.values()].sort((a, b) => b.updatedAt - a.updatedAt);
          return { threads, unreadable: [], retentionDays: state.settingsUnreadable ? null : retentionDays, retentionUnreadable: state.settingsUnreadable, purged: 0 } as T;
        }
        case "hermes_history_save": {
          if (state.failSaves) throw new Error("the keychain is not reachable; the chat history stays closed");
          const t = (args as { thread: ChatThread }).thread;
          expect(Object.keys(t).sort()).toEqual(["createdAt", "id", "msgs", "title", "updatedAt"]);
          files.set(t.id, structuredClone(t));
          const sorted = [...files.values()].sort((a, b) => b.updatedAt - a.updatedAt);
          const evicted = sorted.slice(MAX_THREADS).map((x) => x.id);
          for (const id of evicted) files.delete(id);
          return { evicted } as T;
        }
        case "hermes_history_delete":
          files.delete((args as { id: string }).id);
          return undefined as T;
        case "hermes_history_delete_all":
          files.clear();
          retentionDays = null;
          return undefined as T;
        case "hermes_history_set_retention": {
          const a = args as { days: number | null; replaceUnreadable?: boolean };
          if (state.settingsUnreadable && !a.replaceUnreadable) throw new Error("the saved retention setting does not open");
          state.settingsUnreadable = false;
          retentionDays = a.days;
          return undefined as T;
        }
        default:
          throw new Error("unexpected command " + cmd);
      }
    },
  };
  return { io, files, calls, state };
}

/** Every value in localStorage, joined. */
function allLocalStorage(): string {
  let out = "";
  for (let i = 0; i < localStorage.length; i++) out += (localStorage.getItem(localStorage.key(i)!) ?? "") + "\n";
  return out;
}

describe("chat history (pure)", () => {
  it("keeps a thread only once the member has said something, titled by their first message", () => {
    expect(upsertThread([], "t1", greetingOnly)).toEqual([]);
    const out = upsertThread([], "t1", [...greetingOnly, msg("m1", "You", "  What is my stake?\nmore"), msg("m2", "Agent", "0 SALT")], 1000);
    expect(out).toHaveLength(1);
    expect(out[0]).toMatchObject({ id: "t1", title: "What is my stake?", createdAt: 1000, updatedAt: 1000 });
  });
  it("does not keep a reply that is still streaming, and an unchanged thread keeps its place", () => {
    const msgs = [msg("m1", "You", "hi"), msg("m2", "Agent", "partial", { streaming: true })];
    const a = upsertThread([], "t1", msgs, 1000);
    expect(a[0].msgs.map((m) => m.id)).toEqual(["m1"]);
    expect(upsertThread(a, "t1", msgs, 2000)).toBe(a);
  });
  it("orders newest first and caps the number of threads at core's cap", () => {
    let h: ChatThread[] = [];
    for (let i = 0; i < MAX_THREADS + 5; i++) h = upsertThread(h, "t" + i, [msg("m", "You", "q" + i)], i);
    expect(h).toHaveLength(MAX_THREADS);
    expect(h[0].id).toBe("t" + (MAX_THREADS + 4));
  });
  it("search matches titles and message text, case-insensitively", () => {
    let h = upsertThread([], "a", [msg("1", "You", "Validator rewards?"), msg("2", "Agent", "Your bond is locked")], 1);
    h = upsertThread(h, "b", [msg("1", "You", "Journal entry")], 2);
    expect(searchThreads(h, "BOND").map((t) => t.id)).toEqual(["a"]);
    expect(searchThreads(h, "  ")).toBe(h);
  });
  it("a project is a conversation holding an accepted brief", () => {
    let h = upsertThread([], "plain", [msg("1", "You", "hi")], 1);
    h = upsertThread(h, "proj", [msg("1", "You", "build a faucet"), msg("2", "Agent", "# Brief", { brief })], 2);
    expect(projectsFrom(h)).toEqual([{ threadId: "proj", goal: "ship the faucet page", track: "code", workflow: "code-change", updatedAt: 2 }]);
  });
  it("an untitled thread says so", () => {
    expect(threadTitle([])).toBe("Untitled conversation");
  });
  it("export turns each conversation into a journal page the journal's encrypted export accepts", () => {
    const h = upsertThread([], "a", [msg("1", "You", SECRET), msg("2", "Agent", "noted", { error: "timeout" })], Date.UTC(2026, 10, 14));
    const pages = threadsAsJournalPages(h);
    expect(pages).toHaveLength(1);
    expect(pages[0]).toMatchObject({ id: "hermes-a", kind: "page", pinned: false });
    expect(pages[0].title).toBe("Hermes: " + SECRET);
    expect(pages[0].blocks.slice(1)).toEqual(["You: " + SECRET, "Agent: noted (did not finish: timeout)"]);
    // The journal bundle (what journal_export_encrypted seals) round-trips them.
    expect(parseBundle(buildBundle(pages, "2026-11-14T00:00:00Z"))).toEqual(pages);
  });
});

describe("History in the Hermes chat, through the sealed store", () => {
  let host: HTMLDivElement;
  let root: Root;
  let sealed: ReturnType<typeof sealedStoreDouble>;
  beforeEach(() => {
    localStorage.clear();
    resetChatHistoryForTests();
    sealed = sealedStoreDouble();
    setHistoryIo(sealed.io);
    vi.spyOn(store, "rebuildProvider").mockResolvedValue(undefined);
    host = document.createElement("div");
    document.body.appendChild(host);
    root = createRoot(host);
  });
  afterEach(() => {
    act(() => root.unmount());
    host.remove();
    vi.restoreAllMocks();
    setHistoryIo(null);
  });
  const render = async () => {
    await act(async () => root.render(<AgentChat store={store} s={store.state} />));
  };
  const q = (id: string) => host.querySelector(`[data-testid="${id}"]`) as HTMLElement;
  const qa = (id: string) => [...host.querySelectorAll(`[data-testid="${id}"]`)] as HTMLElement[];
  const click = async (el: HTMLElement) => {
    await act(async () => el.click());
  };

  /** Two finished conversations in the store's history, then a third one open. */
  async function seed() {
    await loadChatHistory();
    store.setState({ ...freshState("p1"), chatThreadId: "t-stake", chatStatus: "ready", chatMsgs: [...greetingOnly, msg("m1", "You", "What is my staking position?"), msg("m2", "Agent", SECRET)] });
    store.archiveChat();
    store.setState({ chatThreadId: "t-faucet", chatMsgs: [msg("m3", "You", "Build a faucet page"), msg("m4", "Agent", "# Brief", { brief })] });
    store.archiveChat();
    store.setState({ chatThreadId: "t-now", chatMsgs: [msg("m5", "You", "Network status")] });
    store.archiveChat();
    await historySettled();
  }

  it("each conversation is sealed through hermes_history_save, and the list comes from hermes_history_list", async () => {
    await seed();
    expect([...sealed.files.keys()].sort()).toEqual(["t-faucet", "t-now", "t-stake"]);
    expect(sealed.files.get("t-stake")!.msgs.map((m) => m.text)).toContain(SECRET);
    // A new session reads the sealed list back.
    resetChatHistoryForTests();
    await loadChatHistory();
    expect(sealed.calls.filter((c) => c.cmd === "hermes_history_list").length).toBe(2);
    expect(chatHistory.get().threads.map((t) => t.id).sort()).toEqual(["t-faucet", "t-now", "t-stake"]);
    expect(chatHistory.get().persisted).toBe(true);
  });

  it("no chat text ever reaches localStorage", async () => {
    const setItem = vi.spyOn(Storage.prototype, "setItem");
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    try {
      await seed();
      // Everything the app persists on its own: the main saved state, a layout toggle, a turn.
      store.toggleSidebar();
      store.save();
      vi.advanceTimersByTime(2000);
    } finally {
      vi.useRealTimers();
    }
    await historySettled();
    expect(localStorage.getItem(STORAGE_KEY)).not.toBeNull();
    const everything = allLocalStorage() + setItem.mock.calls.map((c) => String(c[1])).join("\n");
    for (const text of [SECRET, "What is my staking position?", "Build a faucet page", "Network status", "ship the faucet page"]) {
      expect(everything, text).not.toContain(text);
    }
    expect(localStorage.getItem(LEGACY_PLAINTEXT_KEY)).toBeNull();
  });

  it("an earlier build's plaintext history is moved into the sealed store and removed", async () => {
    const old: ChatThread = { id: "t-legacy", title: "old plaintext", createdAt: 1, updatedAt: 2, msgs: [msg("x", "You", SECRET)] };
    localStorage.setItem(LEGACY_PLAINTEXT_KEY, JSON.stringify({ v: 1, threads: [old] }));
    await loadChatHistory();
    expect(localStorage.getItem(LEGACY_PLAINTEXT_KEY)).toBeNull();
    expect(sealed.files.get("t-legacy")?.msgs[0].text).toBe(SECRET);
    expect(chatHistory.get().threads.map((t) => t.id)).toEqual(["t-legacy"]);
    expect(allLocalStorage()).not.toContain(SECRET);
  });

  it("a damaged plaintext entry is still removed, and the web preview keeps nothing (session only)", async () => {
    localStorage.setItem(LEGACY_PLAINTEXT_KEY, "{not json " + SECRET);
    resetChatHistoryForTests();
    const sim = sealedStoreDouble("sim");
    setHistoryIo(sim.io);
    await loadChatHistory();
    expect(localStorage.getItem(LEGACY_PLAINTEXT_KEY)).toBeNull();
    store.setState({ ...freshState("p1"), chatThreadId: "t-sim", chatStatus: "ready", chatMsgs: [msg("m1", "You", SECRET)] });
    store.archiveChat();
    await historySettled();
    expect(sim.calls).toEqual([]);
    expect(chatHistory.get().persisted).toBe(false);
    await render();
    await click(q("open-chat-history"));
    expect(q("chat-history-where").textContent).toBe("this session only");
    expect(allLocalStorage()).not.toContain(SECRET);
  });

  it("the History entry point lists the conversations from the store and reopens one", async () => {
    await seed();
    await render();
    const open = q("open-chat-history");
    expect(open.getAttribute("aria-expanded")).toBe("false");
    await click(open);
    expect(q("open-chat-history").getAttribute("aria-expanded")).toBe("true");
    expect(q("chat-history-where").textContent).toBe("encrypted on this device");
    const items = qa("chat-history-item");
    expect(items.map((i) => i.getAttribute("data-thread-id")).sort()).toEqual(["t-faucet", "t-now", "t-stake"]);
    expect(items.find((i) => i.getAttribute("aria-current") === "true")?.getAttribute("data-thread-id")).toBe("t-now");

    await click(items.find((i) => i.getAttribute("data-thread-id") === "t-stake")!);
    expect(store.state.chatThreadId).toBe("t-stake");
    expect(store.state.chatMsgs.map((m) => m.text)).toEqual(["Hello, I'm Hermes.", "What is my staking position?", SECRET]);
    expect(store.state.chatMsgs.every((m) => m.id.startsWith("h"))).toBe(true);
    expect(store.rebuildProvider).toHaveBeenCalled();
    await render();
    expect(host.querySelector('[data-testid="chat-history"]')).toBeNull();
    expect(q("chat-messages").textContent).toContain(SECRET);
  });

  it("search narrows the list; Projects lists the conversation with a brief and reopens it", async () => {
    await seed();
    await render();
    await click(q("open-chat-history"));
    const search = q("chat-history-search") as HTMLInputElement;
    const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!;
    act(() => {
      setter.call(search, "991234");
      search.dispatchEvent(new Event("input", { bubbles: true }));
    });
    expect(qa("chat-history-item").map((i) => i.getAttribute("data-thread-id"))).toEqual(["t-stake"]);
    act(() => {
      setter.call(search, "");
      search.dispatchEvent(new Event("input", { bubbles: true }));
    });
    await click(q("chat-history-tab-projects"));
    const projects = qa("chat-history-project");
    expect(projects).toHaveLength(1);
    expect(projects[0].textContent).toContain("ship the faucet page");
    await click(projects[0]);
    expect(store.state.chatThreadId).toBe("t-faucet");
  });

  it("erasing one conversation calls hermes_history_delete; the current one cannot be erased", async () => {
    await seed();
    await render();
    await click(q("open-chat-history"));
    const forget = qa("chat-history-forget");
    // Two rows have the button: the current thread has none.
    expect(forget).toHaveLength(2);
    await act(async () => {
      await store.forgetChat("t-stake");
    });
    expect(sealed.calls.some((c) => c.cmd === "hermes_history_delete" && c.args?.id === "t-stake")).toBe(true);
    expect(sealed.files.has("t-stake")).toBe(false);
    expect(chatHistory.get().threads.some((t) => t.id === "t-stake")).toBe(false);
    await store.forgetChat("t-now");
    expect(sealed.files.has("t-now")).toBe(true);
  });

  it("Delete all history erases everything, the current chat included, after a confirmation", async () => {
    await seed();
    await render();
    await click(q("open-chat-history"));
    await click(q("chat-history-manage"));
    await click(q("chat-history-erase-all"));
    expect(sealed.files.size).toBe(3);
    // The confirmation says what an erase cannot reach (P3).
    expect(host.textContent).toContain("Deletes are device-only");
    expect(host.textContent).toContain("Time Machine, which includes the keychain");
    expect(q("chat-history-erase-scope").textContent).toContain("restoring that backup may reopen it");
    await click(q("chat-history-erase-all-confirm"));
    expect(sealed.calls.some((c) => c.cmd === "hermes_history_delete_all")).toBe(true);
    expect(sealed.files.size).toBe(0);
    expect(chatHistory.get().threads).toEqual([]);
    expect(store.state.chatThreadId).not.toBe("t-now");
    expect(store.state.chatMsgs.some((m) => m.who === "You")).toBe(false);
  });

  it("the retention setting goes to hermes_history_set_retention and is read back from the list", async () => {
    await seed();
    expect(await setHistoryRetention(30)).toBe(true);
    expect(sealed.calls.find((c) => c.cmd === "hermes_history_set_retention")?.args).toEqual({ days: 30, replaceUnreadable: false });
    expect(chatHistory.get().retentionDays).toBe(30);
    expect(await setHistoryRetention(0)).toBe(false);
    expect(await setHistoryRetention(null)).toBe(true);
    expect(chatHistory.get().retentionDays).toBeNull();
  });

  it("New chat keeps the current conversation in the history and starts a fresh one", async () => {
    await loadChatHistory();
    store.setState({ ...freshState("p1"), chatThreadId: "t-old", chatStatus: "ready", chatMsgs: [msg("m1", "You", "old question")] });
    await render();
    await click(q("new-chat"));
    await historySettled();
    expect(store.state.chatThreadId).not.toBe("t-old");
    expect(sealed.files.has("t-old")).toBe(true);
  });

  it("never swaps or erases the thread while a reply is running", async () => {
    await seed();
    store.setState({ chatStatus: "thinking" });
    const toast = vi.spyOn(store, "toast").mockImplementation(() => {});
    expect(store.openChat("t-stake")).toBe(false);
    expect(store.newChat()).toBe(false);
    expect(await store.forgetAllChats()).toBe(false);
    expect(store.state.chatThreadId).toBe("t-now");
    expect(sealed.files.size).toBe(3);
    expect(toast).toHaveBeenCalled();
    store.setState({ chatStatus: "ready" });
  });

  it("a store failure is shown, never hidden", async () => {
    resetChatHistoryForTests();
    setHistoryIo({ mode: "tauri", invoke: async () => Promise.reject(new Error("the keychain is not reachable; the chat history stays closed")) });
    await loadChatHistory();
    expect(chatHistory.get().error).toContain("keychain is not reachable");
    expect(chatHistory.get().persisted).toBe(false);
    await render();
    await click(q("open-chat-history"));
    expect(q("chat-history-error").textContent).toContain("keychain is not reachable");
  });

  it("a plaintext copy is kept (and retried) when sealing it fails", async () => {
    const old: ChatThread = { id: "t-legacy", title: "old", createdAt: 1, updatedAt: 2, msgs: [msg("x", "You", SECRET)] };
    localStorage.setItem(LEGACY_PLAINTEXT_KEY, JSON.stringify({ v: 1, threads: [old] }));
    sealed.state.failSaves = true;
    await loadChatHistory();
    expect(localStorage.getItem(LEGACY_PLAINTEXT_KEY)).not.toBeNull();
    expect(chatHistory.get().error).toContain("could not be sealed yet");
    // The next launch, with the store working: sealed, then removed.
    sealed.state.failSaves = false;
    resetChatHistoryForTests();
    await loadChatHistory();
    expect(sealed.files.has("t-legacy")).toBe(true);
    expect(localStorage.getItem(LEGACY_PLAINTEXT_KEY)).toBeNull();
  });

  it("an unreadable retention setting is reported, keeps the last known value, and is replaced only by a new choice", async () => {
    await seed();
    expect(await setHistoryRetention(30)).toBe(true);
    sealed.state.settingsUnreadable = true;
    await loadChatHistory(true);
    expect(chatHistory.get().retentionUnreadable).toBe(true);
    expect(chatHistory.get().retentionDays).toBe(30);
    await render();
    await click(q("open-chat-history"));
    await click(q("chat-history-manage"));
    expect(q("chat-history-retention-unreadable").textContent).toContain("nothing is erased by age");
    // Choosing a setting is the reset.
    expect(await setHistoryRetention(90)).toBe(true);
    const last = sealed.calls.filter((c) => c.cmd === "hermes_history_set_retention").pop();
    expect(last?.args).toEqual({ days: 90, replaceUnreadable: true });
    expect(chatHistory.get().retentionUnreadable).toBe(false);
    expect(chatHistory.get().retentionDays).toBe(90);
  });
});
