// v0.5.0-rc.5 FIX 4 — the Hermes chat fits the window; the composer is never off-screen.
//
// Before: on the Dashboard the chat was a grid item stretched to the height of the right rail
// (widgets + daemons + transactions + tutorials), so the composer sat below the fold; on the
// Agent surface it was a fixed 460 px box. Now the chat pane sizes itself to the space between
// its top edge (with the page scrolled to the top) and the bottom of the visible scroll area,
// grows with the window, and only the message list scrolls. The page scroll carries the rest.
import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { renderToStaticMarkup } from "react-dom/server";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { AgentChat } from "./AgentChat";
import { fitChatHeight, CHAT_MIN_HEIGHT, CHAT_BOTTOM_GAP, shouldFollow } from "./chatLayout";
import { store } from "../shell/store";
import { freshState, type ChatMsg } from "../shell/state";

(globalThis as unknown as { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

describe("fitChatHeight (pure)", () => {
  it("fills from the chat's top (at scroll 0) to the bottom of the visible area, minus the page gap", () => {
    expect(fitChatHeight({ viewportBottom: 600, chatTop: 150, scrollTop: 0 })).toBe(600 - 150 - CHAT_BOTTOM_GAP);
  });
  it("is stable while the page scrolls (uses the top at scroll 0)", () => {
    expect(fitChatHeight({ viewportBottom: 600, chatTop: 50, scrollTop: 100 })).toBe(600 - 150 - CHAT_BOTTOM_GAP);
  });
  it("grows with the window", () => {
    const small = fitChatHeight({ viewportBottom: 600, chatTop: 150, scrollTop: 0 });
    const big = fitChatHeight({ viewportBottom: 1000, chatTop: 150, scrollTop: 0 });
    expect(big - small).toBe(400);
  });
  it("never collapses below a usable minimum", () => {
    expect(fitChatHeight({ viewportBottom: 300, chatTop: 250, scrollTop: 0 })).toBe(CHAT_MIN_HEIGHT);
    expect(fitChatHeight({ viewportBottom: NaN, chatTop: 0, scrollTop: 0 })).toBe(CHAT_MIN_HEIGHT);
  });
});

describe("shouldFollow (auto-scroll)", () => {
  it("follows when at (or near) the bottom, not when the member scrolled up", () => {
    expect(shouldFollow({ scrollHeight: 1000, scrollTop: 600, clientHeight: 400 })).toBe(true);
    expect(shouldFollow({ scrollHeight: 1000, scrollTop: 570, clientHeight: 400 })).toBe(true);
    expect(shouldFollow({ scrollHeight: 1000, scrollTop: 200, clientHeight: 400 })).toBe(false);
  });
});

function msgs(n: number): ChatMsg[] {
  return Array.from({ length: n }, (_, i) => ({ id: "m" + i, who: i % 2 ? "Hermes" : "You", text: "message " + i }) as ChatMsg);
}

describe("AgentChat in a 600 px window", () => {
  let host: HTMLDivElement;
  let root: Root;
  let innerHeight: number;

  beforeEach(() => {
    innerHeight = window.innerHeight;
    Object.defineProperty(window, "innerHeight", { configurable: true, value: 600 });
    host = document.createElement("div");
    host.style.overflow = "auto";
    document.body.appendChild(host);
    // jsdom has no layout: the scroll area spans the window and the chat starts 150 px down.
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement) {
      if (this === host) return { top: 0, bottom: 600, left: 0, right: 800, width: 800, height: 600, x: 0, y: 0, toJSON: () => ({}) } as DOMRect;
      return { top: 150, bottom: 150, left: 0, right: 800, width: 800, height: 0, x: 0, y: 150, toJSON: () => ({}) } as DOMRect;
    });
    store.setState({ ...freshState("p1"), chatMsgs: msgs(60), chatStatus: "ready" });
    root = createRoot(host);
  });
  afterEach(() => {
    act(() => root.unmount());
    host.remove();
    vi.restoreAllMocks();
    Object.defineProperty(window, "innerHeight", { configurable: true, value: innerHeight });
  });

  it("sizes the pane to the visible space, so the composer's bottom is inside the window", () => {
    act(() => root.render(<AgentChat store={store} s={store.state} />));
    const pane = host.querySelector('[data-testid="agent-chat"]') as HTMLElement;
    expect(pane).toBeTruthy();
    const h = parseFloat(pane.style.height);
    expect(h).toBe(600 - 150 - CHAT_BOTTOM_GAP);
    expect(150 + h).toBeLessThanOrEqual(600);
    // The composer is a direct, non-shrinking child at the bottom of the pane.
    const composer = host.querySelector('[data-testid="chat-composer"]') as HTMLElement;
    expect(composer.parentElement).toBe(pane);
    expect(pane.lastElementChild).toBe(composer);
    expect(composer.style.flexShrink).toBe("0");
    expect(composer.querySelector("input")).toBeTruthy();
  });

  it("only the message list scrolls (flex: 1, min-height: 0, overflow: auto); the pane clips", () => {
    act(() => root.render(<AgentChat store={store} s={store.state} />));
    const pane = host.querySelector('[data-testid="agent-chat"]') as HTMLElement;
    const list = host.querySelector('[data-testid="chat-messages"]') as HTMLElement;
    expect(pane.style.display).toBe("flex");
    expect(pane.style.flexDirection).toBe("column");
    expect(pane.style.overflow).toBe("hidden");
    expect(list.style.flex).toMatch(/^1/);
    expect(list.style.minHeight).toMatch(/^0(px)?$/);
    expect(list.style.overflow).toBe("auto");
  });

  it("re-fits when the window grows", () => {
    act(() => root.render(<AgentChat store={store} s={store.state} />));
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement) {
      if (this === host) return { top: 0, bottom: 900, left: 0, right: 800, width: 800, height: 900, x: 0, y: 0, toJSON: () => ({}) } as DOMRect;
      return { top: 150, bottom: 150, left: 0, right: 800, width: 800, height: 0, x: 0, y: 150, toJSON: () => ({}) } as DOMRect;
    });
    Object.defineProperty(window, "innerHeight", { configurable: true, value: 900 });
    act(() => {
      window.dispatchEvent(new Event("resize"));
    });
    const pane = host.querySelector('[data-testid="agent-chat"]') as HTMLElement;
    expect(parseFloat(pane.style.height)).toBe(900 - 150 - CHAT_BOTTOM_GAP);
  });

  it("the extra panels (model picker, plan card) scroll inside a bounded area, never push the composer out", () => {
    const html = renderToStaticMarkup(<AgentChat store={store} s={store.state} />);
    // Every non-list block between header and composer is bounded.
    expect(html).toMatch(/data-testid="chat-extras"[^>]*style="[^"]*max-height:\s*45%[^"]*overflow:\s*auto/);
  });
});

describe("the Dashboard and Agent surfaces do not stretch or box the chat", () => {
  it("Dashboard grid aligns items to the start (the right rail scrolls with the page)", () => {
    const src = readFileSync(join(__dirname, "../surfaces/Dashboard.tsx"), "utf8");
    expect(src).toMatch(/gridTemplateColumns: "minmax\(0,1fr\) 316px"[^}]*alignItems: "start"/);
  });
  it("the Agent surface no longer pins the chat in a fixed 460 px box", () => {
    const src = readFileSync(join(__dirname, "../surfaces/Agent.tsx"), "utf8");
    expect(src).not.toMatch(/height: 460/);
  });
});

describe("pop-out windows scroll their own content", () => {
  it("the pop-out root may scroll (the main window root stays clipped)", () => {
    const css = readFileSync(join(__dirname, "../styles/index.css"), "utf8");
    const main = readFileSync(join(__dirname, "../main.tsx"), "utf8");
    expect(css).toMatch(/#root\.popout-root\s*\{[^}]*overflow:\s*auto/);
    expect(main).toMatch(/classList\.add\("popout-root"\)/);
  });
});
