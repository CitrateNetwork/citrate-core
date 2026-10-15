// HGW (v0.5.1): the Hermes section owns its scroll.
//
// Replaces the rc.5 contract (FIX 4), which measured the chat pane in JS and let the page scroll
// carry everything else. The owner asked for the opposite: on the Dashboard ("Hermes" in the
// sidebar) and the Agent surface the section is exactly the window, nothing scrolls the whole
// window, and every column (the chat, the right rail, the Agent side column, the Contracts tab)
// scrolls on its own without chaining into anything else. The chat fills its column: header,
// message list (its own scroll) and an always-visible composer.
//
// jsdom has no layout, so these are structure and inline-style contracts. The real-layout check
// (1100x720, 1440x900, 900x600; no document scroll, composer on screen, independent columns, no
// scroll chaining, pinned while a stream grows) is screenshots/layout.spec.ts, run in sim mode.
import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { renderToStaticMarkup } from "react-dom/server";
import { readFileSync, existsSync } from "node:fs";
import { join } from "node:path";

vi.mock("wagmi", async (orig) => ({
  ...(await orig<typeof import("wagmi")>()),
  useBlockNumber: () => ({ data: undefined, isPending: true, error: null }),
  useReadContract: () => ({ data: undefined }),
}));

import { AgentChat } from "./AgentChat";
import { shouldFollow } from "./chatLayout";
import { FILL_ROUTES, surfaceContainerStyle, SCROLL_COLUMN, NARROW_SECTION_PX } from "./sectionLayout";
import { Dashboard } from "../surfaces/Dashboard";
import { Agent } from "../surfaces/Agent";
import { store } from "../shell/store";
import { freshState, type ChatMsg } from "../shell/state";

(globalThis as unknown as { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

/** The inline style of an element as a kebab-case map (read from the attribute, so properties
 *  jsdom's CSSOM does not know, such as overscroll-behavior, are still visible). */
function styleOf(el: Element | null): Record<string, string> {
  const out: Record<string, string> = {};
  for (const decl of (el?.getAttribute("style") ?? "").split(";")) {
    const i = decl.indexOf(":");
    if (i > 0) out[decl.slice(0, i).trim()] = decl.slice(i + 1).trim();
  }
  return out;
}
function parse(html: string): HTMLElement {
  const d = document.createElement("div");
  d.innerHTML = html;
  return d;
}
const tid = (root: ParentNode, id: string) => root.querySelector(`[data-testid="${id}"]`) as HTMLElement | null;
const isZero = (v: string | undefined) => v === "0" || v === "0px";

function msgs(n: number): ChatMsg[] {
  return Array.from({ length: n }, (_, i) => ({ id: "m" + i, who: i % 2 ? "Agent" : "You", text: "message " + i, chips: [], streaming: false }) as ChatMsg);
}

/** Every direct child of the column grid is a bounded column: min-height 0 and its own clip or scroll. */
function expectBoundedColumns(grid: HTMLElement) {
  expect(grid.children.length).toBeGreaterThanOrEqual(2);
  for (const col of Array.from(grid.children)) {
    const st = styleOf(col);
    expect(isZero(st["min-height"]), `min-height on ${col.getAttribute("data-testid")}`).toBe(true);
    expect(["auto", "hidden"]).toContain(st.overflow);
    if (st.overflow === "auto") expect(st["overscroll-behavior"]).toBe("contain");
  }
}

function expectFillsSection(root: HTMLElement) {
  const st = styleOf(root);
  expect(st.height).toBe("100%");
  expect(st["min-height"]).not.toBe("100%");
  expect(isZero(st["min-height"])).toBe(true);
  expect(st.overflow).toBe("hidden");
  expect(st.display).toBe("flex");
  expect(st["flex-direction"]).toBe("column");
}

function expectColumnGrid(grid: HTMLElement) {
  const st = styleOf(grid);
  expect(st.display).toBe("grid");
  expect(st["grid-template-rows"]).toBe("minmax(0, 1fr)");
  expect(st["align-items"]).not.toBe("start");
  expect(st.flex).toMatch(/^1/);
  expect(isZero(st["min-height"])).toBe(true);
}

describe("the Shell stops scrolling the page only for the Hermes section", () => {
  it("Dashboard and Agent fill the content box; the surface container never scrolls", () => {
    expect([...FILL_ROUTES].sort()).toEqual(["agent", "dashboard"]);
    for (const r of ["dashboard", "agent"]) {
      const st = surfaceContainerStyle(r);
      expect(st.overflow).toBe("hidden");
      expect(st.minHeight).toBe(0);
      expect(st.display).toBe("flex");
      expect(st.flexDirection).toBe("column");
      expect(st.flex).toBe(1);
    }
  });
  it("every other surface keeps its page scroll (unchanged)", () => {
    for (const r of ["wallet", "node", "settings", "journal", "files", "models"]) {
      const st = surfaceContainerStyle(r);
      expect(st.overflow).toBe("auto");
      expect(st.minHeight).toBe(0);
    }
  });
  it("App.tsx uses the route-aware container", () => {
    const src = readFileSync(join(__dirname, "../App.tsx"), "utf8");
    expect(src).toMatch(/data-testid="surface-scroller"[^>]*style=\{surfaceContainerStyle\(s\.route\)\}|style=\{surfaceContainerStyle\(s\.route\)\}[^>]*data-testid="surface-scroller"/);
  });
  it("the viewport-measuring chat sizing from rc.5 is gone", () => {
    const src = readFileSync(join(__dirname, "chatLayout.ts"), "utf8");
    expect(src).not.toMatch(/useFitToViewport|fitChatHeight|CHAT_MIN_HEIGHT/);
    expect(readFileSync(join(__dirname, "AgentChat.tsx"), "utf8")).not.toMatch(/useFitToViewport|paneHeight/);
  });
  it("a scroll column clips nothing sideways and never chains its scroll", () => {
    expect(SCROLL_COLUMN.overflow).toBe("auto");
    expect(SCROLL_COLUMN.overscrollBehavior).toBe("contain");
    expect(SCROLL_COLUMN.minHeight).toBe(0);
    expect(SCROLL_COLUMN.minWidth).toBe(0);
  });
});

describe("shouldFollow (the 40 px bottom threshold)", () => {
  it("follows when at (or near) the bottom, not when the member scrolled up", () => {
    expect(shouldFollow({ scrollHeight: 1000, scrollTop: 600, clientHeight: 400 })).toBe(true);
    expect(shouldFollow({ scrollHeight: 1000, scrollTop: 570, clientHeight: 400 })).toBe(true);
    expect(shouldFollow({ scrollHeight: 1000, scrollTop: 200, clientHeight: 400 })).toBe(false);
  });
});

describe("AgentChat fills its column", () => {
  beforeEach(() => {
    store.setState({ ...freshState("p1"), chatMsgs: msgs(60), chatStatus: "ready" });
  });

  it("the pane takes its column's height (no measured pixel height) and clips", () => {
    const root = parse(renderToStaticMarkup(<AgentChat store={store} s={store.state} />));
    const pane = tid(root, "agent-chat") as HTMLElement;
    const st = styleOf(pane);
    expect(st.height).toBe("100%");
    expect(st.height).not.toMatch(/px$/);
    expect(isZero(st["min-height"])).toBe(true);
    expect(st.overflow).toBe("hidden");
    expect(st.display).toBe("flex");
    expect(st["flex-direction"]).toBe("column");
  });

  it("header, then the message list (its own scroll), then the composer, always last and never shrunk", () => {
    const root = parse(renderToStaticMarkup(<AgentChat store={store} s={store.state} />));
    const pane = tid(root, "agent-chat") as HTMLElement;
    const composer = tid(root, "chat-composer") as HTMLElement;
    expect(composer.parentElement).toBe(pane);
    expect(pane.lastElementChild).toBe(composer);
    expect(styleOf(composer)["flex-shrink"]).toBe("0");
    expect(composer.querySelector("input")).toBeTruthy();
    expect(styleOf(pane.firstElementChild)["flex-shrink"]).toBe("0");
  });

  it("only the message list scrolls, and its scroll never chains into the page", () => {
    const root = parse(renderToStaticMarkup(<AgentChat store={store} s={store.state} />));
    const list = tid(root, "chat-messages") as HTMLElement;
    const st = styleOf(list);
    expect(st.flex).toMatch(/^1/);
    expect(isZero(st["min-height"])).toBe(true);
    expect(st.overflow).toBe("auto");
    expect(st["overscroll-behavior"]).toBe("contain");
    // The list's content sits in one wrapper the stick-to-bottom hook observes for size changes.
    const content = tid(list, "chat-content");
    expect(content?.parentElement).toBe(list);
    expect(content?.children.length).toBeGreaterThanOrEqual(60);
  });

  it("the scrollable transcript is a keyboard-reachable, labelled region", () => {
    const root = parse(renderToStaticMarkup(<AgentChat store={store} s={store.state} />));
    const list = tid(root, "chat-messages") as HTMLElement;
    expect(list.getAttribute("tabindex")).toBe("0");
    expect(list.getAttribute("role")).toBe("region");
    expect(list.getAttribute("aria-label")).toBe("Conversation");
  });

  it("the extra panels (model picker, plan card) scroll inside a bounded area, never push the composer out", () => {
    const html = renderToStaticMarkup(<AgentChat store={store} s={store.state} />);
    expect(html).toMatch(/data-testid="chat-extras"[^>]*style="[^"]*max-height:\s*45%[^"]*overflow:\s*auto/);
  });
});

describe("Dashboard (\"Hermes\"): viewport-height section, independent columns", () => {
  beforeEach(() => {
    store.setState({ ...freshState("p1"), chatMsgs: msgs(10), chatStatus: "ready" });
  });

  it("the surface root fills the section instead of growing with its content", () => {
    const root = parse(renderToStaticMarkup(<Dashboard store={store} s={store.state} />));
    expectFillsSection(root.firstElementChild as HTMLElement);
  });

  it("the column grid has one bounded row and stretches its columns", () => {
    const root = parse(renderToStaticMarkup(<Dashboard store={store} s={store.state} />));
    const grid = tid(root, "dashboard-columns") as HTMLElement;
    expectColumnGrid(grid);
    expect(styleOf(grid)["grid-template-columns"]).toBe("minmax(0,1fr) 316px");
    expectBoundedColumns(grid);
  });

  it("the chat is in the first column and the right rail scrolls on its own", () => {
    const root = parse(renderToStaticMarkup(<Dashboard store={store} s={store.state} />));
    const grid = tid(root, "dashboard-columns") as HTMLElement;
    expect(grid.children[0].querySelector('[data-testid="agent-chat"]')).toBeTruthy();
    const rail = tid(root, "dashboard-rail") as HTMLElement;
    expect(rail.parentElement).toBe(grid);
    const st = styleOf(rail);
    expect(st.overflow).toBe("auto");
    expect(st["overscroll-behavior"]).toBe("contain");
    expect(rail.textContent).toContain("Recent transactions");
    expect(rail.textContent).toContain("Tutorials");
  });
});

describe("Agent: the Overview is a full-height chat beside a scrolling side column", () => {
  beforeEach(() => {
    store.setState({ ...freshState("p1"), chatMsgs: msgs(10), chatStatus: "ready" });
  });

  it("the surface root fills the section; the header never scrolls away", () => {
    const root = parse(renderToStaticMarkup(<Agent store={store} s={store.state} />));
    const top = root.firstElementChild as HTMLElement;
    expectFillsSection(top);
    expect(styleOf(top.firstElementChild)["flex-shrink"]).toBe("0");
  });

  it("two bounded columns: the chat, and one side column holding everything else", () => {
    const root = parse(renderToStaticMarkup(<Agent store={store} s={store.state} />));
    const grid = tid(root, "agent-columns") as HTMLElement;
    expectColumnGrid(grid);
    expectBoundedColumns(grid);
    expect(grid.children[0].querySelector('[data-testid="agent-chat"]')).toBeTruthy();
    const rail = tid(root, "agent-rail") as HTMLElement;
    expect(rail.parentElement).toBe(grid);
    expect(styleOf(rail)["overscroll-behavior"]).toBe("contain");
    const text = rail.textContent ?? "";
    for (const panel of ["Awaiting your review", "Attached to your node", "Skills", "Skill registry", "Your skills", "Recent runs", "How this stays yours"]) {
      expect(text).toContain(panel);
    }
    // Approvals lead the side column (the sidebar badge sends people here).
    expect(text.indexOf("Awaiting your review")).toBeLessThan(text.indexOf("Attached to your node"));
    // The chat is not buried in the middle of a stack any more.
    expect(grid.children[0].textContent).not.toContain("Skill registry");
  });
});

describe("Agent: the Contracts tab scrolls inside its own column", () => {
  let host: HTMLDivElement;
  let root: Root;
  const scrolled: Element[] = [];
  beforeEach(() => {
    scrolled.length = 0;
    // jsdom has no scrollIntoView; record which element the toolchain deep link brings into view.
    Element.prototype.scrollIntoView = function (this: Element) {
      scrolled.push(this);
    };
    store.setState({ ...freshState("p1"), chatMsgs: msgs(3), chatStatus: "ready", agentTabRequest: "contracts" });
    host = document.createElement("div");
    document.body.appendChild(host);
    root = createRoot(host);
  });
  afterEach(() => {
    act(() => root.unmount());
    host.remove();
    delete (Element.prototype as unknown as Record<string, unknown>).scrollIntoView;
  });

  it("the contracts panels live in a scroll column; the toolchain step is inside it", async () => {
    await act(async () => root.render(<Agent store={store} s={store.state} />));
    // The tab request was consumed; render again with the consumed state.
    await act(async () => root.render(<Agent store={store} s={store.state} />));
    const main = tid(host, "agent-contracts-scroll") as HTMLElement;
    expect(main).toBeTruthy();
    expect(main.style.overflow).toBe("auto");
    expect(main.style.minHeight).toMatch(/^0(px)?$/);
    expect(main.textContent).toContain("Deploy to 40204");
    expect(tid(host, "agent-chat")).toBeNull();
    const rail = tid(host, "agent-rail") as HTMLElement;
    expect(rail.style.overflow).toBe("auto");
    expect(rail.textContent).toContain("Awaiting your review");
    // The "Open toolchain settings" deep link scrolls the toolchain step into view inside it.
    await act(async () => {
      await new Promise((r) => setTimeout(r, 5));
    });
    const anchor = host.querySelector("#contract-toolchain");
    expect(anchor && main.contains(anchor)).toBe(true);
    expect(scrolled).toContain(anchor);
  });
});

describe("narrow windows collapse to one column with a switch", () => {
  let host: HTMLDivElement;
  let root: Root;
  const realRO = (globalThis as { ResizeObserver?: unknown }).ResizeObserver;
  beforeEach(() => {
    store.setState({ ...freshState("p1"), chatMsgs: msgs(3), chatStatus: "ready", agentTabRequest: null });
    (globalThis as { ResizeObserver?: unknown }).ResizeObserver = undefined;
    // The section reports a width below the breakpoint (the 900 px window minus the sidebar).
    Object.defineProperty(HTMLElement.prototype, "clientWidth", {
      configurable: true,
      get(this: HTMLElement) {
        return this.hasAttribute("data-section") ? NARROW_SECTION_PX - 90 : 0;
      },
    });
    host = document.createElement("div");
    document.body.appendChild(host);
    root = createRoot(host);
  });
  afterEach(() => {
    act(() => root.unmount());
    host.remove();
    delete (HTMLElement.prototype as unknown as Record<string, unknown>).clientWidth;
    (globalThis as { ResizeObserver?: unknown }).ResizeObserver = realRO;
  });

  it("Dashboard: Chat | Activity; the chat stays mounted while the rail is shown", () => {
    act(() => root.render(<Dashboard store={store} s={store.state} />));
    const sw = tid(host, "section-switch") as HTMLElement;
    expect(sw).toBeTruthy();
    const tabs = Array.from(sw.querySelectorAll('[role="tab"]')) as HTMLButtonElement[];
    expect(tabs.map((t) => t.textContent)).toEqual(["Chat", "Activity"]);
    expect(tabs[0].getAttribute("aria-selected")).toBe("true");
    const grid = tid(host, "dashboard-columns") as HTMLElement;
    expect(grid.style.gridTemplateColumns).toBe("minmax(0,1fr)");
    const rail = tid(host, "dashboard-rail") as HTMLElement;
    expect(rail.style.display).toBe("none");
    act(() => tabs[1].click());
    expect(rail.style.display).not.toBe("none");
    const chatCol = (tid(host, "agent-chat") as HTMLElement).parentElement as HTMLElement;
    expect(chatCol.style.display).toBe("none");
    expect(tid(host, "chat-composer")).toBeTruthy();
  });

  it("Agent: Chat | Workbench", () => {
    act(() => root.render(<Agent store={store} s={store.state} />));
    const tabs = Array.from((tid(host, "section-switch") as HTMLElement).querySelectorAll('[role="tab"]')) as HTMLButtonElement[];
    expect(tabs.map((t) => t.textContent?.replace(/\d+$/, ""))).toEqual(["Chat", "Workbench"]);
    const rail = tid(host, "agent-rail") as HTMLElement;
    expect(rail.style.display).toBe("none");
    act(() => tabs[1].click());
    expect(rail.style.display).not.toBe("none");
  });

  it("a wide section shows both columns and no switch", () => {
    delete (HTMLElement.prototype as unknown as Record<string, unknown>).clientWidth;
    Object.defineProperty(HTMLElement.prototype, "clientWidth", {
      configurable: true,
      get(this: HTMLElement) {
        return this.hasAttribute("data-section") ? NARROW_SECTION_PX + 200 : 0;
      },
    });
    act(() => root.render(<Dashboard store={store} s={store.state} />));
    expect(tid(host, "section-switch")).toBeNull();
    expect((tid(host, "dashboard-rail") as HTMLElement).style.display).not.toBe("none");
  });
});

describe("pop-out windows scroll their own content", () => {
  it("the pop-out root may scroll (the main window root stays clipped)", () => {
    const css = readFileSync(join(__dirname, "../styles/index.css"), "utf8");
    const main = readFileSync(join(__dirname, "../main.tsx"), "utf8");
    expect(css).toMatch(/#root\.popout-root\s*\{[^}]*overflow:\s*auto/);
    expect(main).toMatch(/classList\.add\("popout-root"\)/);
  });
  it("the real-layout check exists and covers the three window sizes", () => {
    const p = join(__dirname, "../../screenshots/layout.spec.ts");
    expect(existsSync(p)).toBe(true);
    const spec = readFileSync(p, "utf8");
    for (const w of ["width: 1100, height: 720", "width: 1440, height: 900", "width: 900, height: 600"]) expect(spec).toContain(w);
  });
});

// rc.5 FIX 5: a toolchain refusal in the chat offers a way to the toolchain step.
describe("AgentChat toolchain action", () => {
  it("shows 'Open toolchain settings' only on a toolchain refusal", () => {
    const base = { who: "Workflow", text: "", chips: [], streaming: false } as const;
    store.setState({
      ...freshState("p1"),
      chatStatus: "ready",
      chatMsgs: [
        { ...base, id: "a", error: "This workflow needs the contract toolchain.", toolchainHelp: true },
        { ...base, id: "b", error: "the session refused the workflow" },
      ],
    });
    const html = renderToStaticMarkup(<AgentChat store={store} s={store.state} />);
    expect(html.match(/data-testid="open-toolchain-settings"/g)?.length).toBe(1);
    expect(html).toContain("Open toolchain settings");
  });
});
