// HUP-S5.1 + S5.6 — the Browser pop-out: the screencast with the element Hermes is acting on
// outlined, an always-visible Stop, and honest states (off, Hermes not running, no Chromium,
// stopped, a site without consent, an action waiting for the member in the main window).
import { describe, it, expect, afterEach, vi } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { BrowserPopout } from "./BrowserPopout";
import { BROWSER_OFF, parseBrowserFrame, parseBrowserStatus, type BrowserView } from "./browserView";
import { PopoutRoot } from "./PopoutRoot";
import type { BridgeTransport } from "./bridge";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let root: Root | null = null;
let host: HTMLDivElement | null = null;
afterEach(() => {
  act(() => root?.unmount());
  host?.remove();
  root = null;
  host = null;
});
async function render(el: React.ReactElement) {
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  await act(async () => { root?.render(el); });
  await act(async () => { await new Promise((r) => setTimeout(r, 0)); });
  return host;
}

const status = (over: Record<string, unknown> = {}) =>
  parseBrowserStatus({ enabled: true, chromium: { state: "system", path: "/c" }, mode: "managed", stopped: false, url: "https://a.example/page", consentedOrigins: [], excludedCategories: [], consentNeeded: null, pendingAction: null, ...over });
const frame = (over: Record<string, unknown> = {}) =>
  parseBrowserFrame({ version: 4, mime: "image/jpeg", data: "/9j/", viewportWidth: 1000, viewportHeight: 500, url: "https://a.example/page", withheld: false, highlight: { ref: "e2", label: 'button "Go"', x: 100, y: 50, width: 200, height: 25, state: "acted" }, ...over });

const q = (el: HTMLElement, id: string) => el.querySelector(`[data-testid="${id}"]`) as HTMLElement | null;

describe("HUP-S5.1 Browser pop-out view", () => {
  it("shows the screencast with the acted-on element outlined and a working Stop", async () => {
    const onStop = vi.fn();
    const view: BrowserView = { state: status(), frame: frame() };
    const el = await render(<BrowserPopout view={view} onStop={onStop} />);
    const img = q(el, "browser-frame") as HTMLImageElement;
    expect(img.getAttribute("src")).toBe("data:image/jpeg;base64,/9j/");
    const box = q(el, "browser-highlight");
    expect(box?.style.left).toBe("10%");
    expect(box?.style.top).toBe("10%");
    expect(box?.textContent).toContain("[e2]");
    expect(q(el, "browser-url")?.textContent).toContain("https://a.example/page");
    expect(q(el, "browser-mode")?.textContent).toContain("Managed");
    const stop = q(el, "browser-stop") as HTMLButtonElement;
    expect(stop.disabled).toBe(false);
    await act(async () => { stop.click(); });
    expect(onStop).toHaveBeenCalledTimes(1);
  });

  it("says plainly when the browser is off, Hermes is not running, or no Chromium is installed", async () => {
    let el = await render(<BrowserPopout view={{ state: BROWSER_OFF, frame: null }} onStop={() => undefined} />);
    expect(q(el, "browser-note")?.textContent).toMatch(/off/i);
    expect((q(el, "browser-stop") as HTMLButtonElement).disabled).toBe(true);
    act(() => root?.unmount());

    el = await render(<BrowserPopout view={{ state: { ...BROWSER_OFF, running: false }, frame: null }} onStop={() => undefined} />);
    expect(q(el, "browser-note")?.textContent).toMatch(/Hermes is not running/);
    act(() => root?.unmount());

    el = await render(<BrowserPopout view={{ state: status({ mode: "off", chromium: { state: "not_installed", searched: ["/a"] } }), frame: null }} onStop={() => undefined} />);
    expect(q(el, "browser-note")?.textContent).toMatch(/No Chromium is installed/);
  });

  it("a stopped browser says how to resume", async () => {
    const el = await render(<BrowserPopout view={{ state: status({ stopped: true, mode: "off" }), frame: null }} onStop={() => undefined} />);
    expect(q(el, "browser-note")?.textContent).toMatch(/Stopped/);
    expect(q(el, "browser-note")?.textContent).toMatch(/main window/);
  });

  it("an action waiting for the member points to the main window and outlines the element", async () => {
    const view: BrowserView = {
      state: status({ pendingAction: { id: "b1", tool: "browser_act", summary: 'Click [e2] button "Go" on https://a.example/page', reason: "untrusted" } }),
      frame: frame({ highlight: { ref: "e2", label: 'button "Go"', x: 1, y: 1, width: 5, height: 5, state: "pending" } }),
    };
    const el = await render(<BrowserPopout view={view} onStop={() => undefined} />);
    expect(q(el, "browser-pending")?.textContent).toContain('Click [e2] button "Go"');
    expect(q(el, "browser-pending")?.textContent).toMatch(/main window/);
    expect(q(el, "browser-highlight")?.getAttribute("data-state")).toBe("pending");
    // The pop-out itself offers no Allow button.
    expect(el.textContent).not.toMatch(/\bAllow\b/);
  });

  it("a withheld frame shows no pixels and says why; consent requests are shown", async () => {
    const view: BrowserView = {
      state: status({ mode: "attached", attachPort: 9222, consentNeeded: { origin: "https://www.chase.com", category: "banking" }, excludedCategories: [{ id: "banking", label: "Banking, payments and exchanges" }] }),
      frame: frame({ withheld: true, data: "", highlight: null }),
    };
    const el = await render(<BrowserPopout view={view} onStop={() => undefined} />);
    expect(q(el, "browser-frame")).toBeNull();
    expect(q(el, "browser-withheld")?.textContent).toMatch(/consent/);
    expect(q(el, "browser-consent")?.textContent).toContain("https://www.chase.com");
    expect(q(el, "browser-consent")?.textContent).toContain("Banking, payments and exchanges");
    expect(q(el, "browser-mode")?.textContent).toContain("9222");
  });
});

describe("HUP-S5.1 PopoutRoot for the Browser", () => {
  function fake() {
    let handler: ((p: unknown) => void) | null = null;
    const sent: { to: string; payload: any }[] = [];
    const t: BridgeTransport = {
      async send(to, payload) { sent.push({ to, payload }); },
      async listen(h) { handler = h; return () => { handler = null; }; },
    };
    return { t, sent, deliver: (p: unknown) => handler?.(p) };
  }

  it("announces itself, renders views and sends the browser Stop", async () => {
    const f = fake();
    const el = await render(<PopoutRoot kind="browser" transport={async () => f.t} />);
    expect(f.sent[0]).toEqual({ to: "main", payload: { v: 1, type: "popout.ready", kind: "browser" } });
    expect(el.textContent).toMatch(/Waiting for the main window/);
    await act(async () => { f.deliver({ v: 1, type: "browser.view", view: { state: status(), frame: frame() } }); });
    expect(q(el, "browser-frame")).not.toBeNull();
    await act(async () => { (q(el, "browser-stop") as HTMLButtonElement).click(); });
    expect(f.sent.some((s) => s.payload.type === "browser.stop")).toBe(true);
  });
});
