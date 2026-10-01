// HUP-S5.4 + S7.6 — the main-window side of the pop-outs: opening goes through the guarded Rust
// command, a ready monitor gets snapshots as state changes, Stop runs the existing stop path, and a
// pop-out closing never stops any work.
import { describe, it, expect, vi } from "vitest";
import { createPopoutHost, type PopoutHostDeps } from "./host";
import type { BridgeTransport } from "./bridge";
import { IDLE_ACTIVITY } from "../shell/slices/turnActivity";

function fakeTransport() {
  let handler: ((p: unknown) => void) | null = null;
  const sent: { to: string; payload: any }[] = [];
  const t: BridgeTransport = {
    async send(to, payload) { sent.push({ to, payload }); },
    async listen(h) { handler = h; return () => { handler = null; }; },
  };
  return { t, sent, deliver: (p: unknown) => handler?.(p), listening: () => handler !== null };
}

function deps(over: Partial<PopoutHostDeps> = {}) {
  const ft = fakeTransport();
  let subscriber: (() => void) | null = null;
  const d: PopoutHostDeps = {
    transport: ft.t,
    openWindow: vi.fn(async () => undefined),
    inputs: () => ({ activity: IDLE_ACTIVITY, providerKind: "local", providerLabel: "l", modelLabel: "m", modelId: null, tier: null }),
    subscribe: (fn) => { subscriber = fn; return () => { subscriber = null; }; },
    stop: vi.fn(),
    contextWindow: vi.fn(async () => 8192),
    now: () => 42,
    throttleMs: 0,
    ...over,
  };
  return { d, ft, change: () => subscriber?.() };
}
const flush = () => new Promise((r) => setTimeout(r, 0));

describe("HUP-S5.4 pop-out host", () => {
  it("open() asks Rust to open the window for an allowlisted kind", async () => {
    const { d } = deps();
    const h = await createPopoutHost(d);
    await h.open("monitor");
    expect(d.openWindow).toHaveBeenCalledWith("monitor");
    h.dispose();
  });

  it("a ready monitor gets a snapshot with the real context window, then one per change", async () => {
    const { d, ft, change } = deps();
    const h = await createPopoutHost(d);
    expect(ft.sent).toEqual([]);
    ft.deliver({ v: 1, type: "popout.ready", kind: "monitor" });
    await flush();
    expect(ft.sent.length).toBe(1);
    expect(ft.sent[0].to).toBe("popout-monitor");
    expect(ft.sent[0].payload.type).toBe("monitor.snapshot");
    expect(ft.sent[0].payload.snapshot.context.windowTokens).toBe(8192);
    change();
    await flush();
    expect(ft.sent.length).toBe(2);
    h.dispose();
  });

  it("does not publish before a monitor is open", async () => {
    const { d, ft, change } = deps();
    const h = await createPopoutHost(d);
    change();
    await flush();
    expect(ft.sent).toEqual([]);
    h.dispose();
  });

  it("an unreadable context window stays unknown", async () => {
    const { d, ft } = deps({ contextWindow: vi.fn(async () => { throw new Error("no"); }) });
    const h = await createPopoutHost(d);
    ft.deliver({ v: 1, type: "popout.ready", kind: "monitor" });
    await flush();
    expect(ft.sent[0].payload.snapshot.context.windowTokens).toBeNull();
    h.dispose();
  });

  it("Stop from the monitor runs the stop path once per message", async () => {
    const { d, ft } = deps();
    const h = await createPopoutHost(d);
    ft.deliver({ v: 1, type: "monitor.stop" });
    expect(d.stop).toHaveBeenCalledTimes(1);
    ft.deliver({ v: 1, type: "monitor.stopp" });
    expect(d.stop).toHaveBeenCalledTimes(1);
    h.dispose();
  });

  it("closing a pop-out (no more messages, dispose of the host) never stops work", async () => {
    const { d, ft } = deps();
    const h = await createPopoutHost(d);
    ft.deliver({ v: 1, type: "popout.ready", kind: "monitor" });
    h.dispose();
    expect(ft.listening()).toBe(false);
    expect(d.stop).not.toHaveBeenCalled();
  });

  it("refuses kinds outside the allowlist before reaching Rust", async () => {
    const { d } = deps();
    const h = await createPopoutHost(d);
    await expect(h.open("shell" as never)).rejects.toThrow(/not a pop-out/);
    expect(d.openWindow).not.toHaveBeenCalled();
    h.dispose();
  });

  it("HUP-S1.9: polls the worker processes while the monitor is open and republishes on change", async () => {
    let rows: unknown[] = [{ kind: "toolchain", state: "running", healthy: true, pid: 1, restarts: 0, lastExit: null, lastError: null, runningSinceMs: 1, detail: null }];
    const workers = vi.fn(async () => rows as never);
    const { d, ft } = deps({ workers, workersPollMs: 5 });
    const h = await createPopoutHost(d);
    expect(workers).not.toHaveBeenCalled();
    ft.deliver({ v: 1, type: "popout.ready", kind: "monitor" });
    await new Promise((r) => setTimeout(r, 30));
    expect(ft.sent[0].payload.snapshot.workers.rows[0].restarts).toBe(0);
    const before = ft.sent.length;
    await new Promise((r) => setTimeout(r, 30));
    expect(ft.sent.length).toBe(before);
    rows = [{ ...(rows[0] as object), restarts: 1, pid: 2, lastExit: "killed by signal 9" }];
    await new Promise((r) => setTimeout(r, 40));
    const last = ft.sent[ft.sent.length - 1].payload.snapshot;
    expect(last.workers.rows[0].restarts).toBe(1);
    h.dispose();
    const calls = workers.mock.calls.length;
    await new Promise((r) => setTimeout(r, 30));
    expect(workers.mock.calls.length).toBe(calls);
  });

  it("HUP-S1.9: a failed worker read is shown as unknown, never as no workers", async () => {
    const { d, ft } = deps({ workers: vi.fn(async () => { throw new Error("sidecar down"); }), workersPollMs: 1000 });
    const h = await createPopoutHost(d);
    ft.deliver({ v: 1, type: "popout.ready", kind: "monitor" });
    await new Promise((r) => setTimeout(r, 10));
    expect(ft.sent[0].payload.snapshot.workers.rows).toBeNull();
    h.dispose();
  });
});
