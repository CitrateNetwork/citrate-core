// 0.5.4 (owner request 2026-11-14) — the composer:
//   * mic and "Plan it first" are compact icon buttons with tooltips, on their own row, so the text
//     field keeps the composer's full width;
//   * the text field grows with its content (a large paste shows) up to a cap, then scrolls inside;
//   * Enter sends, Shift+Enter does not (a new line), and the text is kept as typed.
import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { AgentChat } from "./AgentChat";
import { COMPOSER_MAX_HEIGHT, COMPOSER_MIN_HEIGHT, composerHeight, composerMax } from "./chatLayout";
import { store } from "../shell/store";
import { freshState } from "../shell/state";

(globalThis as unknown as { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

describe("composerHeight / composerMax (pure)", () => {
  it("is one line when empty, follows the content, and caps then scrolls", () => {
    expect(composerHeight(0)).toEqual({ height: COMPOSER_MIN_HEIGHT, scroll: false });
    expect(composerHeight(120)).toEqual({ height: 120, scroll: false });
    expect(composerHeight(5000)).toEqual({ height: COMPOSER_MAX_HEIGHT, scroll: true });
    expect(composerHeight(NaN)).toEqual({ height: COMPOSER_MIN_HEIGHT, scroll: false });
  });
  it("never takes more than 40% of a short pane, nor less than a line", () => {
    expect(composerMax(2000)).toBe(COMPOSER_MAX_HEIGHT);
    expect(composerMax(400)).toBe(160);
    expect(composerMax(10)).toBe(COMPOSER_MIN_HEIGHT);
  });
});

/** jsdom has no layout: a text area's scrollHeight is 20 px per line plus 16 px of padding. */
const LINE = 20;
function fakeScrollHeight() {
  return vi.spyOn(HTMLTextAreaElement.prototype, "scrollHeight", "get").mockImplementation(function (this: HTMLTextAreaElement) {
    return this.value.split("\n").length * LINE + 16;
  });
}

/** The borders fitComposer adds (scrollHeight leaves them out). */
const borders = (el: HTMLElement) => {
  const cs = getComputedStyle(el);
  return (parseFloat(cs.borderTopWidth) || 0) + (parseFloat(cs.borderBottomWidth) || 0);
};

/** Type or paste into a React-controlled field (the native setter, then an input event). */
function typeInto(el: HTMLTextAreaElement, value: string) {
  const setter = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!;
  act(() => {
    setter.call(el, value);
    el.dispatchEvent(new Event("input", { bubbles: true }));
  });
}

describe("AgentChat composer", () => {
  let host: HTMLDivElement;
  let root: Root;
  beforeEach(() => {
    host = document.createElement("div");
    document.body.appendChild(host);
    root = createRoot(host);
    store.setState({ ...freshState("p1"), chatMsgs: [], chatStatus: "ready" });
  });
  afterEach(() => {
    act(() => root.unmount());
    host.remove();
    vi.restoreAllMocks();
  });
  const render = () => act(() => root.render(<AgentChat store={store} s={store.state} />));
  const q = (id: string) => host.querySelector(`[data-testid="${id}"]`) as HTMLElement;

  it("keeps the text field at full width with the mic and Plan buttons present", () => {
    render();
    const input = q("chat-input") as HTMLTextAreaElement;
    expect(input.tagName).toBe("TEXTAREA");
    const row = q("chat-input-row");
    // The field is alone on its row: no button shares (and narrows) it.
    expect(input.parentElement).toBe(row);
    expect(row.children.length).toBe(1);
    expect(row.querySelectorAll("button").length).toBe(0);
    expect(row.style.width).toBe("100%");
    expect(input.style.width).toBe("100%");
    expect(input.style.minWidth).toMatch(/^0(px)?$/);
    // The buttons are on the actions row, compact icons with an accessible name and a tooltip.
    const actions = q("chat-composer-actions");
    expect(actions.parentElement).toBe(input.parentElement!.parentElement);
    for (const [id, name] of [["chat-mic", "Dictate (on-device)"], ["plan-it-first", "Plan it first"]] as const) {
      const b = q(id);
      expect(actions.contains(b)).toBe(true);
      expect(b.className).toContain("btn-icon");
      expect(b.getAttribute("aria-label")).toBe(name);
      expect(b.getAttribute("data-tip")).toContain(name);
      expect(b.textContent).toBe("");
      expect(b.querySelector("svg")).toBeTruthy();
    }
  });

  it("grows the text area with a large paste, then caps it and scrolls inside", () => {
    fakeScrollHeight();
    render();
    const input = q("chat-input") as HTMLTextAreaElement;
    typeInto(input, "one line");
    const b = borders(input);
    const one = parseFloat(input.style.height);
    expect(one).toBe(Math.max(COMPOSER_MIN_HEIGHT, LINE + 16 + b));
    expect(input.style.overflowY).toBe("hidden");

    const five = Array.from({ length: 5 }, (_, i) => "line " + i).join("\n");
    typeInto(input, five);
    expect(parseFloat(input.style.height)).toBe(5 * LINE + 16 + b);
    expect(parseFloat(input.style.height)).toBeGreaterThan(one);
    expect(input.style.overflowY).toBe("hidden");

    const paste = Array.from({ length: 80 }, (_, i) => "pasted " + i).join("\n");
    typeInto(input, paste);
    const max = parseFloat(input.style.maxHeight);
    expect(max).toBeLessThanOrEqual(COMPOSER_MAX_HEIGHT);
    expect(parseFloat(input.style.height)).toBe(max);
    expect(input.style.overflowY).toBe("auto");
    // The pasted text is kept, line breaks included.
    expect(input.value).toBe(paste);
  });

  it("Enter sends the whole text; Shift+Enter does not send", () => {
    render();
    const send = vi.spyOn(store, "sendChat").mockResolvedValue(undefined);
    const input = q("chat-input") as HTMLTextAreaElement;
    typeInto(input, "first line\nsecond line");
    act(() => void input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", shiftKey: true, bubbles: true, cancelable: true })));
    expect(send).not.toHaveBeenCalled();
    const enter = new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true });
    act(() => void input.dispatchEvent(enter));
    expect(send).toHaveBeenCalledTimes(1);
    expect(send).toHaveBeenCalledWith("first line\nsecond line", {});
    expect(enter.defaultPrevented).toBe(true);
  });

  it("shrinks back to one line when a send clears it", () => {
    fakeScrollHeight();
    render();
    const input = q("chat-input") as HTMLTextAreaElement;
    typeInto(input, "a\nb\nc\nd");
    const b = borders(input);
    expect(parseFloat(input.style.height)).toBe(4 * LINE + 16 + b);
    // store.sendChat clears the field and appends the member's message.
    act(() => {
      input.value = "";
      store.setState({ chatMsgs: [{ id: "m1", who: "You", text: "a b c d", chips: [], streaming: false }] });
    });
    render();
    expect(parseFloat(input.style.height)).toBe(Math.max(COMPOSER_MIN_HEIGHT, LINE + 16 + b));
  });
});
