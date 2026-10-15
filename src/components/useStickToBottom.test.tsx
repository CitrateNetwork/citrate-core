// HGW (v0.5.1): the Hermes chat stays on the latest message.
//
// The follow state machine (pure) and the hook as mounted in <AgentChat>, with the list's
// geometry and ResizeObserver mocked (jsdom has no layout). The real-layout half of this
// contract (no page scroll, no scroll chaining, a real stream) is screenshots/layout.spec.ts.
import { describe, it, expect, beforeEach, afterEach } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { AgentChat } from "./AgentChat";
import { STICK_INITIAL, stickReducer, isNewTurn, isUpwardIntentKey, isDownwardKey, type StickState } from "./useStickToBottom";
import { store, useStore } from "../shell/store";
import { freshState, type ChatMsg } from "../shell/state";

(globalThis as unknown as { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

describe("stickReducer (pure)", () => {
  const pinned: StickState = { pinned: true, unseen: 0 };
  const reading: StickState = { pinned: false, unseen: 2 };

  it("starts pinned with nothing unseen", () => {
    expect(STICK_INITIAL).toEqual(pinned);
  });
  it("new messages while pinned change nothing (the list follows them)", () => {
    expect(stickReducer(pinned, { type: "messages", added: 3 })).toBe(pinned);
  });
  it("new messages while reading back are counted as unseen", () => {
    expect(stickReducer(reading, { type: "messages", added: 3 })).toEqual({ pinned: false, unseen: 5 });
    expect(stickReducer(reading, { type: "messages", added: 0 })).toBe(reading);
  });
  it("a programmatic scroll away from the bottom never detaches (content grew first)", () => {
    expect(stickReducer(pinned, { type: "scroll", userIntent: false, atBottom: false })).toBe(pinned);
  });
  it("a user scroll away from the bottom detaches", () => {
    expect(stickReducer(pinned, { type: "scroll", userIntent: true, atBottom: false })).toEqual({ pinned: false, unseen: 0 });
  });
  it("a user scroll that stays within the threshold keeps the follow", () => {
    expect(stickReducer(pinned, { type: "scroll", userIntent: true, atBottom: true })).toBe(pinned);
  });
  it("a scroll back to the bottom (no upward gesture) re-pins and clears the unseen count", () => {
    expect(stickReducer(reading, { type: "scroll", userIntent: false, atBottom: true })).toEqual(pinned);
  });
  it("mid upward gesture, passing through the bottom zone does not re-pin", () => {
    expect(stickReducer(reading, { type: "scroll", userIntent: true, atBottom: true })).toBe(reading);
  });
  it("a scroll in the middle while reading back changes nothing", () => {
    expect(stickReducer(reading, { type: "scroll", userIntent: true, atBottom: false })).toBe(reading);
  });
  it("a new turn and Jump to latest both re-pin", () => {
    expect(stickReducer(reading, { type: "turn" })).toEqual(pinned);
    expect(stickReducer(reading, { type: "jump" })).toEqual(pinned);
    expect(stickReducer(pinned, { type: "turn" })).toBe(pinned);
  });
});

describe("isNewTurn (pure)", () => {
  it("a turn starts when Hermes goes from ready to busy, whatever started it", () => {
    expect(isNewTurn({ busy: false, lastOutgoingId: "a" }, { busy: true, lastOutgoingId: "a" })).toBe(true);
  });
  it("finishing a turn is not a new turn", () => {
    expect(isNewTurn({ busy: true, lastOutgoingId: "a" }, { busy: false, lastOutgoingId: "a" })).toBe(false);
  });
  it("a new message of the member's own is a new turn", () => {
    expect(isNewTurn({ busy: true, lastOutgoingId: "a" }, { busy: true, lastOutgoingId: "b" })).toBe(true);
    expect(isNewTurn({ busy: false, lastOutgoingId: undefined }, { busy: false, lastOutgoingId: "b" })).toBe(true);
  });
  it("nothing changed is no turn", () => {
    expect(isNewTurn({ busy: true, lastOutgoingId: "a" }, { busy: true, lastOutgoingId: "a" })).toBe(false);
    expect(isNewTurn({ busy: false, lastOutgoingId: "a" }, { busy: false, lastOutgoingId: undefined })).toBe(false);
  });
});

describe("isUpwardIntentKey", () => {
  it("keys that scroll toward older messages count as reading back", () => {
    for (const k of ["ArrowUp", "PageUp", "Home"]) expect(isUpwardIntentKey(k, false)).toBe(true);
    expect(isUpwardIntentKey(" ", true)).toBe(true);
  });
  it("other keys do not", () => {
    for (const k of ["ArrowDown", "PageDown", "End", "a", "Enter"]) expect(isUpwardIntentKey(k, false)).toBe(false);
    expect(isUpwardIntentKey(" ", false)).toBe(false);
  });
  it("keys toward the latest message end an upward gesture", () => {
    for (const k of ["ArrowDown", "PageDown", "End"]) expect(isDownwardKey(k, false)).toBe(true);
    expect(isDownwardKey(" ", false)).toBe(true);
    expect(isDownwardKey(" ", true)).toBe(false);
    expect(isDownwardKey("a", false)).toBe(false);
  });
});

// ---------------------------------------------------------------------------------------------
// The hook inside <AgentChat>, with mocked geometry and a ResizeObserver we can trigger.
// ---------------------------------------------------------------------------------------------

type ROEntry = { cb: ResizeObserverCallback; els: Set<Element> };
const observers: ROEntry[] = [];
class FakeResizeObserver {
  entry: ROEntry;
  constructor(cb: ResizeObserverCallback) {
    this.entry = { cb, els: new Set() };
    observers.push(this.entry);
  }
  observe(el: Element) {
    this.entry.els.add(el);
  }
  unobserve(el: Element) {
    this.entry.els.delete(el);
  }
  disconnect() {
    this.entry.els.clear();
  }
}
function triggerResize() {
  act(() => {
    for (const o of observers) if (o.els.size) o.cb([], o as unknown as ResizeObserver);
  });
}

function msgs(n: number): ChatMsg[] {
  return Array.from({ length: n }, (_, i) => ({ id: "m" + i, who: i % 2 ? "Agent" : "You", text: "message " + i, chips: [], streaming: false }) as ChatMsg);
}

describe("useStickToBottom in AgentChat", () => {
  let host: HTMLDivElement;
  let root: Root;
  let list: HTMLElement;
  const geo = { scrollHeight: 2000, clientHeight: 400, scrollTop: 0 };
  const max = () => geo.scrollHeight - geo.clientHeight;
  const savedRO = (globalThis as { ResizeObserver?: unknown }).ResizeObserver;

  function fire(type: string, init: Record<string, unknown> = {}) {
    act(() => {
      let ev: Event;
      if (type === "wheel") ev = new WheelEvent("wheel", { bubbles: true, ...init });
      else if (type === "keydown") ev = new KeyboardEvent("keydown", { bubbles: true, ...init });
      else ev = new Event(type, { bubbles: true });
      list.dispatchEvent(ev);
    });
  }
  /** The member moves the list (wheel / key first, then the scroll it causes). */
  function userScrollTo(top: number, how: "wheel" | "key" = "wheel") {
    if (how === "wheel") fire("wheel", { deltaY: top < geo.scrollTop ? -120 : 120 });
    else fire("keydown", { key: "PageUp" });
    geo.scrollTop = Math.max(0, Math.min(top, max()));
    fire("scroll");
  }
  function grow(px: number) {
    geo.scrollHeight += px;
  }
  function setMsgs(m: ChatMsg[], extra: Record<string, unknown> = {}) {
    act(() => store.setState({ chatMsgs: m, ...extra }));
  }
  const jumpBtn = () => host.querySelector('[data-testid="chat-jump-latest"]') as HTMLButtonElement | null;

  beforeEach(() => {
    observers.length = 0;
    (globalThis as { ResizeObserver?: unknown }).ResizeObserver = FakeResizeObserver;
    geo.scrollHeight = 2000;
    geo.clientHeight = 400;
    geo.scrollTop = 0;
    store.setState({ ...freshState("p1"), chatMsgs: msgs(30), chatStatus: "ready" });
    host = document.createElement("div");
    document.body.appendChild(host);
    root = createRoot(host);
    // Geometry lives on the prototype so it is in place before the first commit's effects run.
    Object.defineProperty(HTMLElement.prototype, "scrollHeight", {
      configurable: true,
      get(this: HTMLElement) {
        return this.dataset.testid === "chat-messages" ? geo.scrollHeight : 0;
      },
    });
    Object.defineProperty(HTMLElement.prototype, "clientHeight", {
      configurable: true,
      get(this: HTMLElement) {
        return this.dataset.testid === "chat-messages" ? geo.clientHeight : 0;
      },
    });
    Object.defineProperty(HTMLElement.prototype, "scrollTop", {
      configurable: true,
      get(this: HTMLElement) {
        return this.dataset.testid === "chat-messages" ? geo.scrollTop : 0;
      },
      set(this: HTMLElement, v: number) {
        if (this.dataset.testid === "chat-messages") geo.scrollTop = Math.max(0, Math.min(v, max()));
      },
    });
    act(() => root.render(<Live />));
    list = host.querySelector('[data-testid="chat-messages"]') as HTMLElement;
  });

  afterEach(() => {
    act(() => root.unmount());
    host.remove();
    delete (HTMLElement.prototype as unknown as Record<string, unknown>).scrollHeight;
    delete (HTMLElement.prototype as unknown as Record<string, unknown>).clientHeight;
    delete (HTMLElement.prototype as unknown as Record<string, unknown>).scrollTop;
    (globalThis as { ResizeObserver?: unknown }).ResizeObserver = savedRO;
  });

  it("opens on the latest message", () => {
    expect(geo.scrollTop).toBe(max());
    expect(jumpBtn()).toBeNull();
  });

  it("pinned: a streamed delta (new chatMsgs) keeps the end in view", () => {
    const m = msgs(30);
    grow(180);
    setMsgs(m.map((x, i) => (i === 29 ? { ...x, text: x.text + " and more streamed text" } : x)));
    expect(geo.scrollTop).toBe(max());
  });

  it("pinned: a size change with no chatMsgs change (thinking row, prompt card, reflow) re-pins", () => {
    grow(64);
    expect(geo.scrollTop).toBe(max() - 64);
    triggerResize();
    expect(geo.scrollTop).toBe(max());
  });

  it("pinned: the thinking row appearing re-pins on the commit itself", () => {
    grow(50);
    act(() => store.setState({ chatStatus: "thinking" }));
    expect(geo.scrollTop).toBe(max());
  });

  it("a programmatic scroll event while the content grew does NOT detach the follow", () => {
    grow(300);
    fire("scroll"); // the browser reports a scroll position 300 px from the new bottom, no user input
    triggerResize();
    expect(geo.scrollTop).toBe(max());
    expect(jumpBtn()).toBeNull();
  });

  it("the member scrolling up detaches: deltas leave the list alone and Jump to latest appears", () => {
    userScrollTo(max() - 400);
    const held = geo.scrollTop;
    grow(200);
    setMsgs(msgs(30).map((x, i) => (i === 29 ? { ...x, text: x.text + " streamed" } : x)));
    triggerResize();
    expect(geo.scrollTop).toBe(held);
    const btn = jumpBtn();
    expect(btn).toBeTruthy();
    expect(btn?.getAttribute("aria-label")).toBe("Jump to latest message");
    expect(btn?.textContent ?? "").not.toMatch(/\u2014/);
  });

  it("counts messages that arrive while reading back, and Jump to latest re-pins", () => {
    userScrollTo(200);
    const reply = (id: string) => ({ id, who: "Agent", text: "a reply", chips: [], streaming: false }) as ChatMsg;
    grow(120);
    setMsgs(msgs(30).concat([reply("r1")]));
    grow(120);
    setMsgs(msgs(30).concat([reply("r1"), reply("r2")]));
    expect(jumpBtn()?.textContent).toMatch(/2 new/);
    act(() => jumpBtn()?.click());
    expect(geo.scrollTop).toBe(max());
    expect(jumpBtn()).toBeNull();
    grow(90);
    triggerResize();
    expect(geo.scrollTop).toBe(max());
  });

  it("keyboard reading back (PageUp) detaches too", () => {
    userScrollTo(300, "key");
    grow(100);
    triggerResize();
    expect(geo.scrollTop).toBe(300);
    expect(jumpBtn()).toBeTruthy();
  });

  it("scrolling back to within the threshold re-pins without the button", () => {
    userScrollTo(400);
    expect(jumpBtn()).toBeTruthy();
    userScrollTo(max() - 10);
    expect(jumpBtn()).toBeNull();
    grow(150);
    triggerResize();
    expect(geo.scrollTop).toBe(max());
  });

  it("a turn started outside send() (a workflow run) re-arms the follow", () => {
    userScrollTo(100);
    grow(200);
    setMsgs(msgs(30).concat([{ id: "wf", who: "Workflow", text: "Running the workflow.", chips: [], streaming: true } as ChatMsg]), { chatStatus: "thinking" });
    expect(geo.scrollTop).toBe(max());
    expect(jumpBtn()).toBeNull();
  });

  it("a new message of the member's own (retry, skill run) re-arms the follow", () => {
    userScrollTo(100);
    act(() => store.setState({ chatStatus: "thinking" }));
    userScrollTo(100);
    grow(80);
    setMsgs(msgs(30).concat([{ id: "retry", who: "You", text: "again", chips: [], streaming: false } as ChatMsg]));
    expect(geo.scrollTop).toBe(max());
  });

  it("finishing a turn does not yank a member who is reading back", () => {
    act(() => store.setState({ chatStatus: "thinking" }));
    userScrollTo(100);
    grow(80);
    act(() => store.setState({ chatStatus: "ready" }));
    expect(geo.scrollTop).toBe(100);
  });

  it("the store's scrollChat follows only while pinned", () => {
    expect(typeof store.chatFollow).toBe("function");
    userScrollTo(100);
    grow(100);
    act(() => store.chatFollow?.());
    expect(geo.scrollTop).toBe(100);
    userScrollTo(max());
    grow(100);
    act(() => store.chatFollow?.());
    expect(geo.scrollTop).toBe(max());
  });
});

function Live() {
  // The real binding (useSyncExternalStore), so store.setState re-renders like the app.
  const s = useStore();
  return <AgentChat store={store} s={s} />;
}
