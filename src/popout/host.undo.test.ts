// HUP-S2.9 — the main-window host keeps a ready Activity monitor's undo panel current and runs
// the monitor's undo requests through the main window's own path (the pop-out holds no commands).
import { describe, it, expect, vi } from "vitest";
import { createPopoutHost, type PopoutHostDeps } from "./host";
import type { BridgeTransport } from "./bridge";
import type { UndoPanel } from "./undoPanel";
import { IDLE_ACTIVITY } from "../shell/slices/turnActivity";

const PANEL: UndoPanel = { session: "s4-cafe", enabled: true, note: null, busy: false, steps: [{ seq: 1, status: "committed", paths: ["a.txt"] }], last: null };

function setup(withUndo = true) {
  let handler: ((p: unknown) => void) | null = null;
  const sent: { to: string; payload: any }[] = [];
  const t: BridgeTransport = {
    async send(to, payload) { sent.push({ to, payload }); },
    async listen(h) { handler = h; return () => { handler = null; }; },
  };
  let sub: (() => void) | null = null;
  const request = vi.fn();
  const refresh = vi.fn();
  const d: PopoutHostDeps = {
    transport: t,
    openWindow: vi.fn(async () => undefined),
    inputs: () => ({ activity: IDLE_ACTIVITY, providerKind: "local", providerLabel: "l", modelLabel: "m", modelId: null, tier: null }),
    subscribe: (fn) => { sub = fn; return () => { sub = null; }; },
    stop: vi.fn(),
    contextWindow: vi.fn(async () => 8192),
    now: () => 1,
    throttleMs: 0,
    ...(withUndo ? { undo: { panel: () => PANEL, request, refresh } } : {}),
  };
  return { d, sent, request, refresh, deliver: (p: unknown) => handler?.(p), change: () => sub?.() };
}
const flush = () => new Promise((r) => setTimeout(r, 0));

describe("HUP-S2.9 pop-out host undo", () => {
  it("a ready monitor gets the undo panel with its snapshot, refreshed from the sidecar", async () => {
    const s = setup();
    const h = await createPopoutHost(s.d);
    s.deliver({ v: 1, type: "popout.ready", kind: "monitor" });
    await flush();
    expect(s.refresh).toHaveBeenCalledTimes(1);
    const types = s.sent.map((m) => m.payload.type);
    expect(types).toContain("monitor.snapshot");
    expect(types).toContain("monitor.undo");
    expect(s.sent.find((m) => m.payload.type === "monitor.undo")?.payload.panel).toEqual(PANEL);
    s.change();
    await flush();
    expect(s.sent.filter((m) => m.payload.type === "monitor.undo").length).toBe(2);
    h.dispose();
  });

  it("runs an undo request from the monitor", async () => {
    const s = setup();
    const h = await createPopoutHost(s.d);
    s.deliver({ v: 1, type: "monitor.undo.request", session: "s4-cafe", seq: 1 });
    s.deliver({ v: 1, type: "monitor.undo.request", session: "s4-cafe", seq: null });
    s.deliver({ v: 1, type: "monitor.undo.request", session: "s4-cafe", seq: -1 });
    expect(s.request.mock.calls).toEqual([["s4-cafe", 1], ["s4-cafe", null]]);
    h.dispose();
  });

  it("without undo wiring nothing extra is sent and requests are ignored", async () => {
    const s = setup(false);
    const h = await createPopoutHost(s.d);
    s.deliver({ v: 1, type: "popout.ready", kind: "monitor" });
    await flush();
    expect(s.sent.map((m) => m.payload.type)).toEqual(["monitor.snapshot"]);
    s.deliver({ v: 1, type: "monitor.undo.request", session: "s4-cafe", seq: 1 });
    h.dispose();
  });
});
