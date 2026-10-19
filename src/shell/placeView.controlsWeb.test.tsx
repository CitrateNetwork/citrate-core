// Hermes › Controls mounts two writers of the same web settings: the Controls panel's Web search
// switch (top) and the Settings › Web search & decisions form (below it). Each must change only its
// own fields, so neither can silently undo the other (review finding on #343: the browser could be
// re-enabled after the member turned it off). This drives the real PlaceView composition with both
// mounted, against one in-memory copy of what core stores, at the Tauri invoke boundary.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { DEFAULT_WEB_SETTINGS, type HermesWebSettings, type HermesWebStatus } from "../agent/webSearch";

vi.mock("../bridge/mode", () => ({ BRIDGE_MODE: "tauri", assertSimAllowed: () => {} }));

// What core has saved. Every get returns it; every set replaces it with exactly what was sent.
const saved: { settings: HermesWebSettings } = { settings: DEFAULT_WEB_SETTINGS };
const status = (settings: HermesWebSettings): HermesWebStatus => ({
  settings,
  managedChromium: null,
  searxngFound: true,
  jinaKeyFileFound: false,
  jevKeyFileFound: false,
  notices: [],
  appliesOnRestart: true,
});

vi.mock("../bridge/tauri/invoke", async (importOriginal) => {
  const real = await importOriginal<typeof import("../bridge/tauri/invoke")>();
  return {
    ...real,
    invoke: async (cmd: string, args?: Record<string, unknown>) => {
      if (cmd === "hermes_web_settings_get") return status(saved.settings);
      if (cmd === "hermes_web_settings_set") {
        saved.settings = (args as { settings: HermesWebSettings }).settings;
        return status(saved.settings);
      }
      // Everything else on this page (learning, daemons, budgets, MCP, …) is outside this test.
      throw new Error(`not served in this test: ${cmd}`);
    },
  };
});

import { PlaceView } from "./PlaceView";
import { store } from "./store";
import { freshState } from "./state";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement | null = null;
let root: Root | null = null;

const $ = <T extends HTMLElement = HTMLInputElement>(id: string) => host?.querySelector(`[data-testid="${id}"]`) as T | null;
async function flush() {
  for (let i = 0; i < 5; i++) await act(async () => new Promise((r) => setTimeout(r, 0)));
}
async function click(id: string) {
  const el = $(id);
  expect(el, id).toBeTruthy();
  await act(async () => {
    el?.click();
  });
  await flush();
}

async function mountControls() {
  const s = { ...freshState("p1"), stage: "done" as const, route: "hermes/controls", sSec: "web" };
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  await act(async () => {
    root?.render(<PlaceView store={store} s={s} />);
  });
  await flush();
  // Both writers are on the page, and both loaded what core has.
  expect($("hermes-controls"), "Controls panel").toBeTruthy();
  expect($("web-settings"), "Web search form").toBeTruthy();
}

beforeEach(() => {
  saved.settings = { ...DEFAULT_WEB_SETTINGS };
});

afterEach(() => {
  act(() => root?.unmount());
  host?.remove();
  host = null;
  root = null;
});

describe("Hermes › Controls with the Controls panel and the Web search form mounted together", () => {
  it("the form turns the browser off and saves; then the top Web search switch is toggled; the browser stays off", async () => {
    saved.settings = { ...DEFAULT_WEB_SETTINGS, browserEnabled: true, searchEnabled: false };
    await mountControls();
    expect($("browser-toggle")?.checked).toBe(true);
    expect($("toggle-web-search")?.checked).toBe(false);

    // Lower form: browser off, save.
    await click("browser-toggle");
    await click("web-save");
    expect(saved.settings.browserEnabled).toBe(false);

    // Top switch (loaded before the form saved): search on.
    await click("toggle-web-search");
    expect(saved.settings.searchEnabled).toBe(true);
    expect(saved.settings.browserEnabled).toBe(false);
    expect(saved.settings).toEqual({ ...DEFAULT_WEB_SETTINGS, browserEnabled: false, searchEnabled: true });
  });

  it("the top switch turns search off; then the form turns the browser off and saves; search stays off", async () => {
    saved.settings = { ...DEFAULT_WEB_SETTINGS, browserEnabled: true, searchEnabled: true };
    await mountControls();
    expect($("toggle-web-search")?.checked).toBe(true);
    expect($("web-search-toggle")?.checked).toBe(true);

    // Top switch: search off.
    await click("toggle-web-search");
    expect(saved.settings.searchEnabled).toBe(false);

    // Lower form (its draft still shows search on): browser off, save.
    await click("browser-toggle");
    await click("web-save");
    expect(saved.settings.browserEnabled).toBe(false);
    expect(saved.settings.searchEnabled).toBe(false);
    expect(saved.settings).toEqual({ ...DEFAULT_WEB_SETTINGS, browserEnabled: false, searchEnabled: false });
    // The form now shows what core saved, not its stale draft.
    expect($("web-search-toggle")?.checked).toBe(false);
  });
});
