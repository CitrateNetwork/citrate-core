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
const saved: { settings: HermesWebSettings; searxngFound: boolean } = { settings: DEFAULT_WEB_SETTINGS, searxngFound: true };
// Every command the page sent, in order.
const calls: string[] = [];
// Reads to hold (by 1-based order of hermes_web_settings_get) until the test releases them: a
// delayed core response. A held read still answers with what was saved when it was made.
const hold: { reads: Set<number>; count: number; release: Map<number, () => void> } = { reads: new Set(), count: 0, release: new Map() };
const status = (settings: HermesWebSettings): HermesWebStatus => ({
  settings,
  managedChromium: null,
  searxngFound: saved.searxngFound,
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
      calls.push(cmd);
      // Like core over IPC, each call answers on a later turn: a read returns what was saved when
      // it was made, a write lands when it is answered. Two writers can therefore interleave.
      if (cmd === "hermes_web_settings_get") {
        const n = ++hold.count;
        const snapshot = saved.settings;
        if (hold.reads.has(n)) await new Promise<void>((r) => hold.release.set(n, r));
        else await new Promise((r) => setTimeout(r, 0));
        return status(snapshot);
      }
      if (cmd === "hermes_web_settings_set") {
        await new Promise((r) => setTimeout(r, 0));
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
import { hermesRunningSlice, withHermesStart, type HermesSavedSnapshot } from "./slices/hermesRunning";

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
  saved.searxngFound = true;
  calls.length = 0;
  hold.reads = new Set();
  hold.count = 0;
  hold.release = new Map();
  hermesRunningSlice.set({ webSearch: null, learn: null, starting: 0 });
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

  // Review finding on #343 (P2): the two views held separate snapshots, so the form kept showing
  // search off after the top switch turned it on, and its dirty-tracking baseline was stale too.
  it("the top switch enables search; the form shows it on; the member turns it off in the form and saves; search is off", async () => {
    saved.settings = { ...DEFAULT_WEB_SETTINGS, searchEnabled: false };
    await mountControls();
    expect($("web-search-toggle")?.checked).toBe(false);

    await click("toggle-web-search");
    expect(saved.settings.searchEnabled).toBe(true);
    // One source of truth: the lower form now shows what the top switch saved.
    expect($("web-search-toggle")?.checked).toBe(true);

    // The repro: the member works the lower checkbox until it shows off (on → off from a stale
    // "off" took two clicks), then saves. What they see is what gets saved.
    let clicks = 0;
    while ($("web-search-toggle")?.checked !== false && clicks < 3) {
      await click("web-search-toggle");
      clicks++;
    }
    expect($("web-search-toggle")?.checked).toBe(false);
    await click("web-save");
    expect(saved.settings.searchEnabled).toBe(false);
    expect($("toggle-web-search")?.checked).toBe(false);
  });

  it("the stale-form sequence (on, then off, then save) after the top switch enabled search saves off", async () => {
    saved.settings = { ...DEFAULT_WEB_SETTINGS, searchEnabled: false };
    await mountControls();
    await click("toggle-web-search");
    // Exactly the reported clicks: on, back off, save.
    if ($("web-search-toggle")?.checked === false) await click("web-search-toggle");
    await click("web-search-toggle");
    expect($("web-search-toggle")?.checked).toBe(false);
    await click("web-save");
    expect(saved.settings.searchEnabled).toBe(false);
  });

  it("the form saves search on; the top switch shows it on", async () => {
    saved.settings = { ...DEFAULT_WEB_SETTINGS, searchEnabled: false };
    await mountControls();
    await click("web-search-toggle");
    await click("web-save");
    expect(saved.settings.searchEnabled).toBe(true);
    expect($("toggle-web-search")?.checked).toBe(true);
  });

  it("with search on and no SearXNG program, the hint points to Web search in Hermes › Controls, not Settings", async () => {
    saved.settings = { ...DEFAULT_WEB_SETTINGS, searchEnabled: true };
    saved.searxngFound = false;
    await mountControls();
    const hint = $("reason-web-search")?.textContent ?? "";
    expect(hint).toContain("no SearXNG program is set yet");
    expect(hint).toContain("Hermes › Controls › Web search");
    expect(hint).not.toContain("Settings ›");
  });

  // Review finding on #343 (P2): each writer re-read before writing, but the two could run at the
  // same time, so both read before either wrote and the second write put back a stale field.
  it("the form saves the browser off while the top switch turns search on, at the same time: both changes survive", async () => {
    saved.settings = { ...DEFAULT_WEB_SETTINGS, browserEnabled: true, searchEnabled: false };
    await mountControls();
    await click("browser-toggle"); // form draft: browser off (not saved yet)
    // Save the form and toggle the top switch in the same turn, before either has answered.
    await act(async () => {
      $("web-save")?.click();
      $("toggle-web-search")?.click();
    });
    await flush();
    expect(saved.settings.browserEnabled).toBe(false);
    expect(saved.settings.searchEnabled).toBe(true);
    // And both views show what core saved.
    expect($("browser-toggle")?.checked).toBe(false);
    expect($("web-search-toggle")?.checked).toBe(true);
    expect($("toggle-web-search")?.checked).toBe(true);
  });

  it("the same race in the other order (top switch first, then the form): both changes survive", async () => {
    saved.settings = { ...DEFAULT_WEB_SETTINGS, browserEnabled: true, searchEnabled: false };
    await mountControls();
    await click("browser-toggle");
    await act(async () => {
      $("toggle-web-search")?.click();
      $("web-save")?.click();
    });
    await flush();
    expect(saved.settings).toEqual({ ...DEFAULT_WEB_SETTINGS, browserEnabled: false, searchEnabled: true });
  });

  // K2's start hold and K0's write lock together (review on #343): a Hermes start that begins while
  // the form's save is on the wire waits for it and snapshots the saved value; both writers hold
  // during the start; nothing deadlocks, and the switches work again once Hermes is up.
  it("a Hermes start during a save waits for it, both writers hold during the start, and writes resume after", async () => {
    saved.settings = { ...DEFAULT_WEB_SETTINGS, searchEnabled: false };
    await mountControls();
    await click("web-search-toggle"); // form draft: search on (not saved yet)

    let release: () => void = () => {};
    const confirmed = new Promise<void>((r) => (release = r));
    let snapshot: HermesSavedSnapshot | null = null;
    let start: Promise<void> | null = null;
    const reads = () => calls.filter((c) => c === "hermes_web_settings_get").length;
    const before = reads();
    await act(async () => {
      $("web-save")?.click();
      // Let the save take the lock and send its re-read, then begin a start.
      for (let i = 0; i < 50 && reads() === before; i++) await Promise.resolve();
      expect(reads()).toBe(before + 1);
      start = withHermesStart(async (snap) => {
        snapshot = snap;
        await confirmed;
      });
    });
    await flush();
    // The start waited for the save and took the saved value.
    expect(saved.settings.searchEnabled).toBe(true);
    expect(snapshot).not.toBeNull();
    expect(snapshot?.webSearch).toBe(true);
    // During the start both writers hold, and say why.
    expect($("toggle-web-search")?.disabled).toBe(true);
    expect($("web-save")?.disabled).toBe(true);
    expect($("web-held")).toBeTruthy();
    expect($("reason-web-search")?.textContent).toContain("Hermes is starting");

    release();
    await act(async () => {
      await start;
    });
    await flush();
    // No deadlock: the start ended, and the top switch writes again.
    expect(hermesRunningSlice.get().starting).toBe(0);
    expect($("toggle-web-search")?.disabled).toBe(false);
    await click("toggle-web-search");
    expect(saved.settings.searchEnabled).toBe(false);
  });

  // Review finding on #343 (P2): a view's INITIAL read can answer after the other view already saved
  // and announced a newer value. That late answer is older than what the view adopted, so it must
  // be dropped, not shown and used as the baseline.
  // On this page the Controls panel sends the first read and the form the second.
  const PANEL_INITIAL_READ = 1;
  const FORM_INITIAL_READ = 2;
  async function release(n: number) {
    await act(async () => {
      hold.release.get(n)?.();
    });
    await flush();
  }

  it("the form's initial read answers late, after the top switch enabled search: the form keeps showing on, and turning it off saves off", async () => {
    saved.settings = { ...DEFAULT_WEB_SETTINGS, searchEnabled: false };
    hold.reads = new Set([FORM_INITIAL_READ]);
    await mountControls();
    expect(hold.release.has(FORM_INITIAL_READ)).toBe(true);

    await click("toggle-web-search");
    expect(saved.settings.searchEnabled).toBe(true);
    expect($("web-search-toggle")?.checked).toBe(true);

    // The delayed initial answer (search off, from before the switch) arrives now.
    await release(FORM_INITIAL_READ);
    expect($("web-search-toggle")?.checked).toBe(true);

    // The member turns search off in the form and saves: what they see is what is saved.
    await click("web-search-toggle");
    expect($("web-search-toggle")?.checked).toBe(false);
    await click("web-save");
    expect(saved.settings.searchEnabled).toBe(false);
    expect($("toggle-web-search")?.checked).toBe(false);
  });

  it("the reported sequence: late initial read, then on, off and save in the form, saves off", async () => {
    saved.settings = { ...DEFAULT_WEB_SETTINGS, searchEnabled: false };
    hold.reads = new Set([FORM_INITIAL_READ]);
    await mountControls();
    await click("toggle-web-search");
    await release(FORM_INITIAL_READ);
    // On and back off, as the member did when the form showed off.
    if ($("web-search-toggle")?.checked === false) await click("web-search-toggle");
    await click("web-search-toggle");
    expect($("web-search-toggle")?.checked).toBe(false);
    await click("web-save");
    expect(saved.settings.searchEnabled).toBe(false);
  });

  it("the Controls panel's initial read answers late, after the form saved: the top switch shows the saved value", async () => {
    saved.settings = { ...DEFAULT_WEB_SETTINGS, searchEnabled: false };
    hold.reads = new Set([PANEL_INITIAL_READ]);
    await mountControls();
    await click("web-search-toggle");
    await click("web-save");
    expect(saved.settings.searchEnabled).toBe(true);
    expect($("toggle-web-search")?.checked).toBe(true);
    await release(PANEL_INITIAL_READ);
    expect($("toggle-web-search")?.checked).toBe(true);
  });

  it("a late initial read with nothing published meanwhile is still applied", async () => {
    saved.settings = { ...DEFAULT_WEB_SETTINGS, searchEnabled: true };
    hold.reads = new Set([FORM_INITIAL_READ]);
    await mountControls();
    expect($("web-search-toggle")?.checked).toBe(false); // not loaded yet: defaults
    await release(FORM_INITIAL_READ);
    expect($("web-search-toggle")?.checked).toBe(true);
  });
});
