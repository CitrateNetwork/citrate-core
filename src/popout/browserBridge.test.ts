// HUP-S5.1 — the Browser pop-out over the bridge: the main window polls the sidecar's browser
// (through Rust) only while a Browser pop-out is alive (it re-announces itself as a heartbeat),
// sends it checked views, and runs the browser's Stop when the pop-out asks. The pop-out can ask
// for nothing else: decisions and consent stay in the main window.
import { describe, it, expect, vi } from "vitest";
import { parseToMain, parseToPopout, createPopoutEnd, type BridgeTransport } from "./bridge";
import { createPopoutHost, type PopoutHostDeps } from "./host";
import { IDLE_ACTIVITY } from "../shell/slices/turnActivity";

const STATUS = { enabled: true, chromium: { state: "system", path: "/c" }, mode: "managed", stopped: false, url: "https://a.example/", consentedOrigins: [], excludedCategories: [], consentNeeded: null, pendingAction: null };
const frame = (version: number) => ({ version, mime: "image/jpeg", data: "/9j/", viewportWidth: 1280, viewportHeight: 800, url: "https://a.example/", withheld: false, highlight: null });

function fakeTransport() {
  let handler: ((p: unknown) => void) | null = null;
  const sent: { to: string; payload: any }[] = [];
  const t: BridgeTransport = {
    async send(to, payload) { sent.push({ to, payload: JSON.parse(JSON.stringify(payload)) }); },
    async listen(h) { handler = h; return () => { handler = null; }; },
  };
  return { t, sent, deliver: (p: unknown) => handler?.(p) };
}

const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));

describe("HUP-S5.1 browser bridge messages", () => {
  it("accepts browser.stop from a pop-out and nothing that decides or consents", () => {
    expect(parseToMain({ v: 1, type: "browser.stop" })).toEqual({ v: 1, type: "browser.stop" });
    for (const raw of [
      { v: 1, type: "browser.decide", id: "b1", allow: true },
      { v: 1, type: "browser.allowOrigin", origin: "https://x" },
      { v: 1, type: "browser.attach", port: 9222 },
      { v: 2, type: "browser.stop" },
    ]) {
      expect(parseToMain(raw)).toBeNull();
    }
  });

  it("accepts a checked browser view and drops a broken one", () => {
    const ok = parseToPopout({ v: 1, type: "browser.view", view: { state: STATUS, frame: frame(3) } });
    expect(ok?.type).toBe("browser.view");
    expect(parseToPopout({ v: 1, type: "browser.view", view: { state: STATUS, frame: { ...frame(3), data: "<img>" } } })).toBeNull();
    expect(parseToPopout({ v: 1, type: "browser.view" })).toBeNull();
  });

  it("the pop-out end hands views to the browser view and sends Stop", async () => {
    const ft = fakeTransport();
    const views: unknown[] = [];
    // The fourth argument is the S2.9 undo-panel callback; the browser view is the fifth.
    const end = await createPopoutEnd(ft.t, "browser", () => undefined, undefined, (v) => views.push(v));
    ft.deliver({ v: 1, type: "browser.view", view: { state: STATUS, frame: null } });
    ft.deliver({ v: 1, type: "monitor.snapshot", snapshot: {} });
    expect(views.length).toBe(1);
    await end.stopBrowser();
    expect(ft.sent).toEqual([{ to: "main", payload: { v: 1, type: "browser.stop" } }]);
    end.close();
  });
});

function hostDeps(over: Partial<PopoutHostDeps> = {}) {
  const ft = fakeTransport();
  let clock = 1000;
  const browser = {
    status: vi.fn(async () => STATUS as unknown),
    frame: vi.fn(async (after: number) => (after < 5 ? (frame(5) as unknown) : null)),
    stop: vi.fn(async () => undefined),
  };
  const d: PopoutHostDeps = {
    transport: ft.t,
    openWindow: vi.fn(async () => undefined),
    inputs: () => ({ activity: IDLE_ACTIVITY, providerKind: "local", providerLabel: "l", modelLabel: "m", modelId: null, tier: null }),
    subscribe: () => () => undefined,
    stop: vi.fn(),
    contextWindow: vi.fn(async () => 8192),
    now: () => clock,
    throttleMs: 0,
    browser,
    browserPollMs: 5,
    ...over,
  };
  return { d, ft, browser, advance: (ms: number) => { clock += ms; } };
}

describe("HUP-S5.1 browser pop-out host", () => {
  it("polls only after a Browser pop-out is ready, and sends it the view", async () => {
    const { d, ft, browser } = hostDeps();
    const h = await createPopoutHost(d);
    await wait(30);
    expect(browser.frame).not.toHaveBeenCalled();
    ft.deliver({ v: 1, type: "popout.ready", kind: "browser" });
    await wait(40);
    expect(browser.frame).toHaveBeenCalledWith(0);
    expect(browser.frame).toHaveBeenCalledWith(5);
    const views = ft.sent.filter((s) => s.payload.type === "browser.view");
    expect(views.length).toBeGreaterThan(0);
    expect(views[0].to).toBe("popout-browser");
    const last = views[views.length - 1].payload.view;
    expect(last.state.mode).toBe("managed");
    expect(last.frame.version).toBe(5);
    h.dispose();
  });

  it("stops polling when the pop-out stops announcing itself", async () => {
    const { d, ft, browser, advance } = hostDeps();
    const h = await createPopoutHost(d);
    ft.deliver({ v: 1, type: "popout.ready", kind: "browser" });
    await wait(20);
    advance(60_000);
    await wait(20);
    const calls = browser.frame.mock.calls.length;
    await wait(40);
    expect(browser.frame.mock.calls.length).toBe(calls);
    h.dispose();
  });

  it("a status that cannot be read is shown as off, never as working", async () => {
    const { d, ft } = hostDeps({
      browser: { status: vi.fn(async () => { throw new Error("hermes is not running"); }), frame: vi.fn(async () => null), stop: vi.fn(async () => undefined) },
    });
    const h = await createPopoutHost(d);
    ft.deliver({ v: 1, type: "popout.ready", kind: "browser" });
    await wait(30);
    const view = ft.sent.filter((s) => s.payload.type === "browser.view").pop()?.payload.view;
    expect(view.state.enabled).toBe(false);
    h.dispose();
  });

  it("the pop-out's Stop runs the browser's stop, not the turn's", async () => {
    const { d, ft, browser } = hostDeps();
    const h = await createPopoutHost(d);
    ft.deliver({ v: 1, type: "browser.stop" });
    await wait(5);
    expect(browser.stop).toHaveBeenCalledTimes(1);
    expect(d.stop).not.toHaveBeenCalled();
    h.dispose();
  });

  it("without browser deps nothing is polled and Stop is a no-op", async () => {
    const { d, ft } = hostDeps({ browser: undefined });
    const h = await createPopoutHost(d);
    ft.deliver({ v: 1, type: "popout.ready", kind: "browser" });
    ft.deliver({ v: 1, type: "browser.stop" });
    await wait(20);
    expect(ft.sent.filter((s) => s.payload.type === "browser.view").length).toBe(1);
    expect(ft.sent[0].payload.view.state.enabled).toBe(false);
    h.dispose();
  });
});
