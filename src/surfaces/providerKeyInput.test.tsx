// Owner report (0.5.3): "the input on the API key ... would erase the input when I was trying to put
// my API key in". Hermes › Controls › Details › Providers & keys: the key, model and base URL a member
// types must survive every re-render of Settings (the store re-renders it on each node poll), and
// Save must hand core exactly what was typed.
import { afterEach, describe, expect, it, vi } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Settings } from "./Settings";
import { freshState, type AppState } from "../shell/state";
import { CONTROLS_DETAIL_SETTINGS } from "../shell/places";
import type { Store } from "../shell/store";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const aiSetProvider = vi.fn(async (..._a: unknown[]) => {});
const store = {
  identity: () => ({ name: "Member", initials: "M", email: "m@example.com", sub: "s", wallet: "", tier: "pilot", role: "member", org: null, real: true }),
  setState: () => {},
  toast: vi.fn(),
  aiSetProvider: (...a: unknown[]) => aiSetProvider(...a),
} as unknown as Store;

let host: HTMLDivElement;
let root: Root;
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  aiSetProvider.mockClear();
});

function stateFor(aiEdit: string | null, height: number): AppState {
  const s = freshState("p1");
  s.sSec = "ai";
  s.aiEdit = aiEdit;
  s.height = height;
  return s;
}
async function render(s: AppState) {
  await act(async () => {
    root.render(<Settings store={store} s={s} sections={CONTROLS_DETAIL_SETTINGS} heading="Details" />);
  });
}
async function mount(aiEdit: string) {
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  await render(stateFor(aiEdit, 1));
}
/** Type like a member does: one character at a time, through React's onChange. */
async function typeChar(input: HTMLInputElement, ch: string) {
  const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set;
  await act(async () => {
    setter?.call(input, input.value + ch);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
}
const q = (sel: string) => host.querySelector(sel) as HTMLInputElement | null;
// Selected by placeholder (what the member sees), so the same test runs against the old editor.
const KEY = 'input[placeholder="sk-…"]';
const MODEL = 'input[placeholder="model (e.g. gpt-4o-mini)"]';
const BASE_URL = 'input[placeholder="https://…/v1"]';
const button = (text: string) => Array.from(host.querySelectorAll("button")).find((b) => b.textContent?.includes(text)) as HTMLButtonElement | undefined;

describe("Providers & keys: the key field keeps what is typed", () => {
  it("every character survives the store's poll re-renders", async () => {
    await mount("openai");
    const key = "test-key-not-real-AbC-1";
    for (let i = 0; i < key.length; i++) {
      const input = q(KEY);
      expect(input, "the key field is on the page").toBeInstanceOf(HTMLInputElement);
      await typeChar(input as HTMLInputElement, key[i]);
      // A node poll lands between keystrokes: Settings re-renders with a fresh state object.
      await render(stateFor("openai", 2 + i));
      expect(q(KEY)?.value).toBe(key.slice(0, i + 1));
    }
  });

  it("an edited model name survives re-renders too, and Save sends exactly what was typed", async () => {
    await mount("openai");
    const model = q(MODEL) as HTMLInputElement;
    expect(model.value).toBe("gpt-4o-mini");
    await act(async () => {
      const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set;
      setter?.call(model, "gpt-4o");
      model.dispatchEvent(new Event("input", { bubbles: true }));
    });
    for (const ch of "test-key-not-real-0123") await typeChar(q(KEY) as HTMLInputElement, ch);
    await render(stateFor("openai", 50));
    await render(stateFor("openai", 51));
    expect(q(MODEL)?.value).toBe("gpt-4o");
    await act(async () => {
      button("Save key")?.click();
    });
    expect(aiSetProvider).toHaveBeenCalledWith("openai", "https://api.openai.com/v1", "gpt-4o", "test-key-not-real-0123");
  });

  it("the custom endpoint's base URL survives re-renders", async () => {
    await mount("custom");
    const url = "https://llm.example.com/v1";
    for (const ch of url) {
      await typeChar(q(BASE_URL) as HTMLInputElement, ch);
      await render(stateFor("custom", 3));
    }
    expect(q(BASE_URL)?.value).toBe(url);
  });

  it("closing the editor drops the typed key: reopening starts empty", async () => {
    await mount("openai");
    for (const ch of "test-key-not-real-closed") await typeChar(q(KEY) as HTMLInputElement, ch);
    await render(stateFor(null, 9)); // the edit ends (saved or cancelled)
    expect(q(KEY)).toBeNull();
    expect(host.innerHTML).not.toContain("test-key-not-real-closed");
    await render(stateFor("openai", 10));
    expect(q(KEY)?.value).toBe("");
  });
});
