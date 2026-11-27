// 0.6.0 L4-WP01: characterization suite for the Hermes chat pane (AgentChat).
//
// Every assertion here describes how the chat pane behaves TODAY (citrate-core main a826e441) and
// passes on day 1. These are not red tests and not a statement of what is right: they pin current
// behaviour so the later L4 work packages change it on purpose. LQ runs this file on every merge;
// a later WP may replace an assertion here only with a stricter one, and says so in its PR.
//
// Data source (Rule 7): the shipped components with the same store setup as hermesChatBoard.test.tsx
// (the app store, fresh state, fixture runs built with src/agent/runTimeline.ts), and the shipped
// stylesheet src/styles/tokens.css read from disk.
import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { act } from "react";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { createRoot, type Root } from "react-dom/client";
import { renderToStaticMarkup } from "react-dom/server";
import { AgentChat } from "./AgentChat";
import { Markdown } from "./Markdown";
import { shouldFollow } from "./chatLayout";
import { store } from "../shell/store";
import { freshState, type ChatMsg } from "../shell/state";
import { newRun, runDraft, runEnd, runThink, runToolEnd, runToolStart } from "../agent/runTimeline";
import { setEnvelopeForAuthority } from "../fl/authority";

(globalThis as unknown as { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function doneRun() {
  let r = runThink(newRun(0), 0, 1);
  r = runToolStart(r, "c0", "node_status", 10, "core");
  r = runToolEnd(r, "c0", "node_status", "ok", '{"height":1204311}', 420);
  r = runToolStart(r, "c1", "fl_round_plan", 500, "core");
  r = runToolEnd(r, "c1", "fl_round_plan", "ok", "no rounds", 3300);
  r = runDraft(r, 3400);
  return runEnd(r, "answered", 6000);
}
const you = (text: string, id = "u1"): ChatMsg => ({ id, who: "You", text, chips: [], streaming: false });
const agent = (patch: Partial<ChatMsg>): ChatMsg => ({ id: "a1", who: "Agent", text: "", chips: [], streaming: false, ...patch });

describe("L4 characterization: the Hermes chat pane", () => {
  let host: HTMLDivElement;
  let root: Root;
  beforeEach(() => {
    host = document.createElement("div");
    document.body.appendChild(host);
    root = createRoot(host);
    store.setState({ ...freshState("p1"), chatMsgs: [], chatStatus: "ready", chatQueue: [], chatHermesDown: null });
  });
  afterEach(() => {
    act(() => root.unmount());
    host.remove();
    setEnvelopeForAuthority(null);
    vi.restoreAllMocks();
  });
  const render = () => act(() => root.render(<AgentChat store={store} s={store.state} />));
  const q = (id: string) => host.querySelector(`[data-testid="${id}"]`) as HTMLElement | null;
  const qa = (id: string) => Array.from(host.querySelectorAll(`[data-testid="${id}"]`)) as HTMLElement[];

  it("char_follow_within_40px", () => {
    // Follow while the bottom is less than 40 px away; at 40 px or more the member is reading.
    expect(shouldFollow({ scrollHeight: 1000, scrollTop: 600, clientHeight: 400 })).toBe(true);
    expect(shouldFollow({ scrollHeight: 1000, scrollTop: 561, clientHeight: 400 })).toBe(true);
    expect(shouldFollow({ scrollHeight: 1000, scrollTop: 560, clientHeight: 400 })).toBe(false);
    expect(shouldFollow({ scrollHeight: 1000, scrollTop: 0, clientHeight: 400 })).toBe(false);
    // Content shorter than the pane always follows.
    expect(shouldFollow({ scrollHeight: 200, scrollTop: 0, clientHeight: 400 })).toBe(true);
  });

  it("char_follow_effect_sets_scrolltop_bottom", () => {
    store.setState({ chatMsgs: [you("first")] });
    render();
    const el = q("chat-messages")!;
    expect(store.chatScrollEl).toBe(el);
    // jsdom has no layout: give the pane a height and record what the effect writes.
    let top = 0;
    Object.defineProperty(el, "scrollHeight", { configurable: true, get: () => 900 });
    Object.defineProperty(el, "clientHeight", { configurable: true, get: () => 300 });
    Object.defineProperty(el, "scrollTop", { configurable: true, get: () => top, set: (v: number) => { top = v; } });
    store.setState({ chatMsgs: [you("first"), agent({ text: "second" })] });
    render();
    // Following: a new message puts the pane at the very bottom (scrollTop = scrollHeight).
    expect(top).toBe(900);
    // The member scrolls up to read (100 + 300 is 500 px from the bottom): the follow stops.
    top = 100;
    act(() => {
      el.dispatchEvent(new Event("scroll"));
    });
    store.setState({ chatMsgs: [you("first"), agent({ text: "second" }), agent({ id: "a2", text: "third" })] });
    render();
    expect(top).toBe(100);
    // Back within 40 px of the bottom: the next message follows again.
    top = 570;
    act(() => {
      el.dispatchEvent(new Event("scroll"));
    });
    store.setState({ chatMsgs: [you("first"), agent({ text: "second" }), agent({ id: "a2", text: "third" }), agent({ id: "a3", text: "fourth" })] });
    render();
    expect(top).toBe(900);
  });

  it("char_actions_always_visible_for_finished_reply", () => {
    store.setState({ chatMsgs: [you("Network status"), agent({ text: "Your node is synced.", run: doneRun(), model: "Local 8B" })] });
    render();
    const reply = qa("msg-agent")[0];
    const labels = Array.from(reply.querySelectorAll("button")).map((b) => b.textContent);
    // Copy, Retry, Retry with another model and Branch from here are in the page for a finished
    // reply, with no hover or focus needed to reveal them.
    for (const l of ["Copy", "Retry", "Retry with another model", "Branch from here"]) expect(labels).toContain(l);
    const row = reply.querySelector("[data-testid=msg-retry]")!.parentElement as HTMLElement;
    expect(row.style.display).toBe("flex");
    expect(row.style.opacity).toBe("");
    expect(row.style.visibility).toBe("");
    expect(row.hidden).toBe(false);
    // The member's own message keeps Edit and Copy.
    const mine = Array.from(qa("msg-you")[0].querySelectorAll("button")).map((b) => b.textContent);
    expect(mine).toEqual(["Edit", "Copy"]);
  });

  it("char_actions_hidden_while_streaming", () => {
    // The same row is absent while the reply is still streaming (the counterpart of the test above).
    store.setState({ chatStatus: "thinking", chatMsgs: [you("Network status"), agent({ text: "Your node", streaming: true })] });
    render();
    expect(q("msg-retry")).toBeNull();
    expect(q("msg-branch")).toBeNull();
  });

  it("char_run_timeline_open_by_default_after_end", () => {
    store.setState({ chatMsgs: [you("Network status"), agent({ text: "Your node is synced.", run: doneRun() })] });
    render();
    const toggle = q("run-toggle")!;
    expect(toggle.getAttribute("aria-expanded")).toBe("true");
    expect(toggle.textContent).toBe("Hide steps");
    expect(qa("run-step")).toHaveLength(4);
    expect(q("run-stop")).toBeNull();
  });

  it("char_markdown_no_bare_url_link", () => {
    const bare = renderToStaticMarkup(<Markdown text="Read https://citrate.ai/docs and www.citrate.ai for more." />);
    expect(bare).not.toContain("<a");
    expect(bare).toContain("https://citrate.ai/docs");
    // Only a written [label](url) link with a safe scheme becomes a link.
    const written = renderToStaticMarkup(<Markdown text="Read [the docs](https://citrate.ai/docs)." />);
    expect(written).toContain('<a href="https://citrate.ai/docs" target="_blank" rel="noreferrer"');
    const unsafe = renderToStaticMarkup(<Markdown text="[x](javascript:alert(1))" />);
    expect(unsafe).not.toContain("<a");
  });

  it("char_fence_renders_plain_pre", () => {
    const html = renderToStaticMarkup(<Markdown text={"```ts\nconst a = 1;\nconst b = a + 1;\n```"} />);
    const box = document.createElement("div");
    box.innerHTML = html;
    const pre = box.querySelector("pre")!;
    expect(pre.className).toBe("mono");
    const code = pre.querySelector("code")!;
    // No language class, no highlighting markup and no copy button: the text as written.
    expect(code.className).toBe("");
    expect(code.children).toHaveLength(0);
    expect(code.textContent).toBe("const a = 1;\nconst b = a + 1;");
    expect(box.querySelector("button")).toBeNull();
    // The info string ("ts") is not shown either.
    expect(box.textContent).toBe(code.textContent);
  });

  it("char_reduced_motion_kills_animations", () => {
    const css = readFileSync(join(__dirname, "../styles/tokens.css"), "utf8");
    const at = css.indexOf("@media (prefers-reduced-motion: reduce)");
    expect(at).toBeGreaterThan(-1);
    // The block, up to its closing brace at the start of a line.
    const block = css.slice(at, css.indexOf("\n}", at) + 2);
    expect(block).toContain(".run-pulse { animation: none; }");
    expect(block).toContain("*, *::before, *::after {");
    for (const rule of ["animation-duration: .01ms !important;", "animation-iteration-count: 1 !important;", "transition-duration: .01ms !important;", "scroll-behavior: auto !important;"]) {
      expect(block).toContain(rule);
    }
  });
});
