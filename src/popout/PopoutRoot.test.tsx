// HUP-S5.4 — the pop-out window's root: it announces itself ready, renders the monitor from the
// snapshots it receives (and says it is waiting until the first one), and Stop goes back over the
// bridge. A pop-out kind without a view yet says so plainly.
import { describe, it, expect, afterEach } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { PopoutRoot } from "./PopoutRoot";
import type { BridgeTransport } from "./bridge";
import { buildMonitorSnapshot } from "./monitorSnapshot";
import { IDLE_ACTIVITY } from "../shell/slices/turnActivity";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function fake() {
  let handler: ((p: unknown) => void) | null = null;
  const sent: { to: string; payload: any }[] = [];
  const t: BridgeTransport = {
    async send(to, payload) { sent.push({ to, payload }); },
    async listen(h) { handler = h; return () => { handler = null; }; },
  };
  return { t, sent, deliver: (p: unknown) => handler?.(p), listening: () => handler !== null };
}

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

const running = buildMonitorSnapshot({
  activity: { ...IDLE_ACTIVITY, state: "running", providerKind: "local", providerLabel: "local", phase: "thinking", startedAt: Date.now() },
  providerKind: "local",
  providerLabel: "local",
  modelLabel: "Gemma",
  modelId: null,
  tier: "T1",
  localCtxTokens: 8192,
  now: Date.now(),
});

describe("HUP-S5.4 pop-out root", () => {
  it("monitor: ready → waiting → snapshot → Stop over the bridge; unmount stops listening", async () => {
    const f = fake();
    const el = await render(<PopoutRoot kind="monitor" transport={async () => f.t} />);
    expect(f.sent[0]).toEqual({ to: "main", payload: { v: 1, type: "popout.ready", kind: "monitor" } });
    expect(el.textContent).toMatch(/waiting for the main window/i);
    await act(async () => { f.deliver({ v: 1, type: "monitor.snapshot", snapshot: running }); });
    expect(el.querySelector('[data-testid="mon-model"]')?.textContent).toContain("Gemma");
    const stop = el.querySelector('[data-testid="mon-stop"]') as HTMLButtonElement;
    await act(async () => { stop.click(); });
    expect(f.sent.at(-1)).toEqual({ to: "main", payload: { v: 1, type: "monitor.stop" } });
    act(() => root?.unmount());
    root = null;
    expect(f.listening()).toBe(false);
  });

  it("drops a malformed snapshot", async () => {
    const f = fake();
    const el = await render(<PopoutRoot kind="monitor" transport={async () => f.t} />);
    await act(async () => { f.deliver({ v: 1, type: "monitor.snapshot", snapshot: { model: "x" } }); });
    expect(el.textContent).toMatch(/waiting for the main window/i);
  });

  it("a kind without a view says it is not built yet", async () => {
    const f = fake();
    const el = await render(<PopoutRoot kind="diff" transport={async () => f.t} />);
    expect(el.textContent).toMatch(/not built yet/i);
  });

  it("a transport that cannot start is reported, not hidden", async () => {
    const el = await render(<PopoutRoot kind="monitor" transport={async () => { throw new Error("no ipc"); }} />);
    expect(el.textContent).toMatch(/could not connect/i);
  });
});

describe("HUP-S6.7 pop-out root: the Contract reader", () => {
  it("renders the reader, asks the main window what to open (through the relay), and stops listening on unmount", async () => {
    const f = fake();
    const relayed: { type?: string; op?: string }[] = [];
    const relay = async () => ({ send: async (m: unknown) => void relayed.push(m as { type?: string; op?: string }) });
    const el = await render(<PopoutRoot kind="contract" transport={async () => f.t} contractRelay={relay} />);
    expect(el.querySelector('[data-testid="contract-reader"]')).not.toBeNull();
    expect(el.textContent).not.toMatch(/not built yet/i);
    expect(relayed[0]?.type).toBe("contract.request");
    expect(relayed[0]?.op).toBe("initial");
    expect(f.sent.find((s) => s.payload?.type === "contract.request"), "never over the event bus").toBeUndefined();
    act(() => root?.unmount());
    root = null;
    expect(f.listening()).toBe(false);
  });

  it("a transport that cannot start is reported, not hidden", async () => {
    const el = await render(<PopoutRoot kind="contract" transport={async () => { throw new Error("no ipc"); }} contractRelay={async () => ({ send: async () => undefined })} />);
    expect(el.textContent).toMatch(/could not connect/i);
  });
});
