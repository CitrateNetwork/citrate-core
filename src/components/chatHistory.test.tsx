// 0.5.4 (owner request 2026-11-14) — earlier conversations and projects are reachable from the
// Hermes chat: a History entry point lists them from the store, searches them, and reopens one.
// Data source (Rule 7): the store's own thread (AppState.chatMsgs), kept per thread on this device
// by the chat history slice; projects are the conversations holding an accepted brief.
import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { AgentChat } from "./AgentChat";
import { store } from "../shell/store";
import { freshState, type ChatMsg } from "../shell/state";
import {
  CHAT_HISTORY_KEY,
  MAX_THREADS,
  chatHistory,
  projectsFrom,
  reloadChatHistory,
  searchThreads,
  threadTitle,
  upsertThread,
  withinBudget,
  type ChatThread,
} from "../shell/slices/chatHistory";
import type { Brief } from "../bridge/domains";

(globalThis as unknown as { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const msg = (id: string, who: string, text: string, extra: Partial<ChatMsg> = {}): ChatMsg => ({ id, who, text, chips: [], streaming: false, ...extra });
const greetingOnly = [msg("g", "Agent", "Hello, I'm Hermes.")];
const brief: Brief = { track: "code", goal: "ship the faucet page", constraints: [], persona: "Builder", skills: [], workflow: "code-change", workflow_available: false, ships_in: "0.5.0", gates: ["tests pass"] };

describe("chat history (pure)", () => {
  it("keeps a thread only once the member has said something, titled by their first message", () => {
    expect(upsertThread([], "t1", greetingOnly)).toEqual([]);
    const out = upsertThread([], "t1", [...greetingOnly, msg("m1", "You", "  What is my stake?\nmore"), msg("m2", "Agent", "0 SALT")], 1000);
    expect(out).toHaveLength(1);
    expect(out[0]).toMatchObject({ id: "t1", title: "What is my stake?", createdAt: 1000, updatedAt: 1000 });
    expect(out[0].msgs).toHaveLength(3);
  });
  it("does not keep a reply that is still streaming, and an unchanged thread keeps its place", () => {
    const msgs = [msg("m1", "You", "hi"), msg("m2", "Agent", "partial", { streaming: true })];
    const a = upsertThread([], "t1", msgs, 1000);
    expect(a[0].msgs.map((m) => m.id)).toEqual(["m1"]);
    expect(upsertThread(a, "t1", msgs, 2000)).toBe(a);
  });
  it("orders newest first and caps the number of threads", () => {
    let h: ChatThread[] = [];
    for (let i = 0; i < MAX_THREADS + 5; i++) h = upsertThread(h, "t" + i, [msg("m", "You", "q" + i)], i);
    expect(h).toHaveLength(MAX_THREADS);
    expect(h[0].id).toBe("t" + (MAX_THREADS + 4));
  });
  it("drops the oldest threads past the size budget", () => {
    const big = (id: string, t: number) => ({ id, title: id, createdAt: t, updatedAt: t, msgs: [msg("m", "You", "x".repeat(400))] });
    const fitted = withinBudget([big("new", 3), big("mid", 2), big("old", 1)], 1300);
    expect(fitted.map((t) => t.id)).toEqual(["new", "mid"]);
  });
  it("search matches titles and message text, case-insensitively", () => {
    let h = upsertThread([], "a", [msg("1", "You", "Validator rewards?"), msg("2", "Agent", "Your bond is locked")], 1);
    h = upsertThread(h, "b", [msg("1", "You", "Journal entry")], 2);
    expect(searchThreads(h, "BOND").map((t) => t.id)).toEqual(["a"]);
    expect(searchThreads(h, "journal").map((t) => t.id)).toEqual(["b"]);
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
});

describe("History in the Hermes chat", () => {
  let host: HTMLDivElement;
  let root: Root;
  beforeEach(() => {
    localStorage.clear();
    chatHistory.set({ threads: [] });
    vi.spyOn(store, "rebuildProvider").mockResolvedValue(undefined);
    host = document.createElement("div");
    document.body.appendChild(host);
    root = createRoot(host);
  });
  afterEach(() => {
    act(() => root.unmount());
    host.remove();
    vi.restoreAllMocks();
  });
  const render = () => act(() => root.render(<AgentChat store={store} s={store.state} />));
  const q = (id: string) => host.querySelector(`[data-testid="${id}"]`) as HTMLElement;
  const qa = (id: string) => [...host.querySelectorAll(`[data-testid="${id}"]`)] as HTMLElement[];

  /** Two finished conversations in the store's history, then a third one open. */
  function seed() {
    store.setState({ ...freshState("p1"), chatThreadId: "t-stake", chatStatus: "ready", chatMsgs: [...greetingOnly, msg("m1", "You", "What is my staking position?"), msg("m2", "Agent", "You have 0 SALT staked.")] });
    store.archiveChat();
    store.setState({ chatThreadId: "t-faucet", chatMsgs: [msg("m3", "You", "Build a faucet page"), msg("m4", "Agent", "# Brief", { brief })] });
    store.archiveChat();
    store.setState({ chatThreadId: "t-now", chatMsgs: [msg("m5", "You", "Network status")] });
    store.archiveChat();
  }

  it("the History entry point lists the conversations from the store and reopens one", () => {
    seed();
    render();
    const open = q("open-chat-history");
    expect(open.getAttribute("aria-expanded")).toBe("false");
    act(() => open.click());
    render();
    expect(q("open-chat-history").getAttribute("aria-expanded")).toBe("true");
    const items = qa("chat-history-item");
    expect(items.map((i) => i.getAttribute("data-thread-id")).sort()).toEqual(["t-faucet", "t-now", "t-stake"]);
    expect(host.textContent).toContain("What is my staking position?");
    const current = items.find((i) => i.getAttribute("aria-current") === "true");
    expect(current?.getAttribute("data-thread-id")).toBe("t-now");

    const stake = items.find((i) => i.getAttribute("data-thread-id") === "t-stake")!;
    act(() => stake.click());
    expect(store.state.chatThreadId).toBe("t-stake");
    expect(store.state.chatMsgs.map((m) => m.text)).toEqual(["Hello, I'm Hermes.", "What is my staking position?", "You have 0 SALT staked."]);
    // Re-keyed, so a new message's id can never collide with a reopened one.
    expect(store.state.chatMsgs.every((m) => m.id.startsWith("h"))).toBe(true);
    // A fresh agent session for the reopened thread; the one left is still in the history.
    expect(store.rebuildProvider).toHaveBeenCalled();
    expect(chatHistory.get().threads.some((t) => t.id === "t-now")).toBe(true);
    render();
    expect(host.querySelector('[data-testid="chat-history"]')).toBeNull();
    expect(q("chat-messages").textContent).toContain("You have 0 SALT staked.");
  });

  it("search narrows the list; Projects lists the conversation with a brief", () => {
    seed();
    render();
    act(() => q("open-chat-history").click());
    render();
    const search = q("chat-history-search") as HTMLInputElement;
    const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!;
    act(() => {
      setter.call(search, "salt");
      search.dispatchEvent(new Event("input", { bubbles: true }));
    });
    expect(qa("chat-history-item").map((i) => i.getAttribute("data-thread-id"))).toEqual(["t-stake"]);
    act(() => {
      setter.call(search, "");
      search.dispatchEvent(new Event("input", { bubbles: true }));
    });
    act(() => q("chat-history-tab-projects").click());
    const projects = qa("chat-history-project");
    expect(projects).toHaveLength(1);
    expect(projects[0].textContent).toContain("ship the faucet page");
    act(() => projects[0].click());
    expect(store.state.chatThreadId).toBe("t-faucet");
    expect(store.state.chatMsgs.some((m) => m.brief?.goal === "ship the faucet page")).toBe(true);
  });

  it("says so honestly when there is nothing yet", () => {
    store.setState({ ...freshState("p1"), chatStatus: "ready", chatMsgs: greetingOnly });
    store.archiveChat();
    render();
    act(() => q("open-chat-history").click());
    render();
    expect(qa("chat-history-item")).toHaveLength(0);
    expect(host.textContent).toContain("No earlier conversations on this device yet");
  });

  it("New chat keeps the current conversation in the history and starts a fresh one", () => {
    store.setState({ ...freshState("p1"), chatThreadId: "t-old", chatStatus: "ready", chatMsgs: [msg("m1", "You", "old question")] });
    render();
    act(() => q("new-chat").click());
    expect(store.state.chatThreadId).not.toBe("t-old");
    expect(store.state.chatMsgs.some((m) => m.who === "You")).toBe(false);
    expect(chatHistory.get().threads.map((t) => t.id)).toContain("t-old");
    expect(store.rebuildProvider).toHaveBeenCalled();
  });

  it("never swaps the thread while a reply is running", () => {
    seed();
    store.setState({ chatStatus: "thinking" });
    const toast = vi.spyOn(store, "toast").mockImplementation(() => {});
    expect(store.openChat("t-stake")).toBe(false);
    expect(store.newChat()).toBe(false);
    expect(store.state.chatThreadId).toBe("t-now");
    expect(toast).toHaveBeenCalled();
    store.setState({ chatStatus: "ready" });
  });

  it("the history survives a reload (its own device-local entry) and Forget removes one", () => {
    seed();
    expect(JSON.parse(localStorage.getItem(CHAT_HISTORY_KEY) || "{}").threads).toHaveLength(3);
    chatHistory.set({ threads: [] });
    reloadChatHistory();
    expect(chatHistory.get().threads.map((t) => t.id).sort()).toEqual(["t-faucet", "t-now", "t-stake"]);
    store.forgetChat("t-stake");
    // The current thread cannot be forgotten from under the member.
    store.forgetChat("t-now");
    reloadChatHistory();
    expect(chatHistory.get().threads.map((t) => t.id).sort()).toEqual(["t-faucet", "t-now"]);
  });
});
