// HUP-S5.3 (US-5.3 AC3) — the Activity monitor shows the decide() slot's per-backend metering, as
// the sidecar reports it at GET /decide/stats (every metered decision plus each task outcome
// recorded through POST /decide/outcomes). Only measured numbers are shown; an unreadable report
// says so and is never shown as "no decisions".
import { describe, it, expect, vi, afterEach } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { ActivityMonitor } from "./ActivityMonitor";
import {
  buildMonitorSnapshot,
  decideBackendLine,
  decideFor,
  isMonitorSnapshot,
  type DecideMetering,
  type MonitorInputs,
} from "./monitorSnapshot";
import { createPopoutHost, type PopoutHostDeps } from "./host";
import type { BridgeTransport } from "./bridge";
import { IDLE_ACTIVITY } from "../shell/slices/turnActivity";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

/** The shape Rust returns, from a local web-subset-v2 run (9 of 10 tasks, 38 decisions). */
const METERING: DecideMetering = {
  jevEnabled: false,
  jevOrigins: 0,
  jevNonWeb: false,
  logging: true,
  backends: [
    { backend: "jev", decisions: 0, errors: 0, p50Ms: null, p95Ms: null, meanConfidence: null, egressBytes: 0, tasksAttempted: 0, tasksSucceeded: 0, taskSuccessBps: null },
    { backend: "local", decisions: 38, errors: 0, p50Ms: 812, p95Ms: 2210, meanConfidence: null, egressBytes: 0, tasksAttempted: 10, tasksSucceeded: 9, taskSuccessBps: 9000 },
  ],
};

const inputs: MonitorInputs = {
  activity: IDLE_ACTIVITY,
  providerKind: "local",
  providerLabel: "local model",
  modelLabel: "Gemma",
  modelId: null,
  tier: "T0",
  localCtxTokens: 8192,
  now: 10_000,
};

let root: Root | null = null;
let host: HTMLDivElement | null = null;
afterEach(() => {
  act(() => root?.unmount());
  host?.remove();
  root = null;
  host = null;
});
function render(el: React.ReactElement) {
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  act(() => root?.render(el));
  return host;
}
const byTestId = (el: HTMLElement, id: string) => el.querySelector(`[data-testid="${id}"]`) as HTMLElement | null;

describe("HUP-S5.3 decide() metering in the Activity monitor", () => {
  it("shows each backend that decided, with its measured numbers", () => {
    const el = render(<ActivityMonitor snapshot={buildMonitorSnapshot({ ...inputs, decide: METERING })} now={10_000} onStop={vi.fn()} />);
    const rows = Array.from(el.querySelectorAll('[data-testid="mon-decide-row"]'));
    expect(rows.length).toBe(1);
    expect(rows[0].textContent).toContain("local");
    expect(rows[0].textContent).toContain("38 decisions");
    expect(rows[0].textContent).toContain("median 812 ms, p95 2210 ms");
    expect(rows[0].textContent).toContain("tasks 9 of 10 succeeded (90.0%)");
    expect(byTestId(el, "mon-decide")?.textContent).toMatch(/Jev is off/);
    expect(el.textContent).not.toContain("—");
  });

  it("says plainly what leaves the machine when Jev is on", () => {
    const jev: DecideMetering = {
      ...METERING,
      jevEnabled: true,
      jevOrigins: 2,
      backends: [{ ...METERING.backends[0], decisions: 3, errors: 1, p50Ms: 140, p95Ms: 160, meanConfidence: 0.8, egressBytes: 4096 }],
    };
    const d = decideFor(jev);
    expect(d.note).toContain("Jev (TypeSafe) for 2 origins");
    expect(d.note).toContain("off this machine");
    const line = decideBackendLine(d.rows![0]);
    expect(line).toContain("1 failed");
    expect(line).toContain("mean confidence 0.80");
    expect(line).toContain("4,096 bytes sent off this machine");
    expect(line).toContain("no task outcomes recorded");
  });

  it("an unreadable report is unknown, a stopped Hermes has no decisions, and neither is invented", () => {
    expect(decideFor("error").rows).toBeNull();
    expect(decideFor("error").note).toMatch(/could not be read/);
    expect(decideFor(undefined).rows).toBeNull();
    expect(decideFor(null).rows).toEqual([]);
    expect(decideFor(null).note).toMatch(/not running/);
    const none = decideFor({ ...METERING, backends: [METERING.backends[0]] });
    expect(none.rows).toEqual([]);
    expect(none.note).toMatch(/No decisions yet/);
  });

  it("the pop-out accepts a snapshot with the section and refuses a malformed one", () => {
    const snap = buildMonitorSnapshot({ ...inputs, decide: METERING });
    expect(isMonitorSnapshot(snap)).toBe(true);
    const { decide: _omit, ...older } = snap;
    expect(isMonitorSnapshot(older)).toBe(true);
    const bad = { ...snap, decide: { ...snap.decide!, rows: [{ backend: "local", decisions: "38" }] } };
    expect(isMonitorSnapshot(bad)).toBe(false);
  });
});

describe("HUP-S5.3 the host reads the metering while the monitor is open", () => {
  function fakeTransport() {
    let handler: ((p: unknown) => void) | null = null;
    const sent: { to: string; payload: any }[] = [];
    const t: BridgeTransport = {
      async send(to, payload) { sent.push({ to, payload }); },
      async listen(h) { handler = h; return () => { handler = null; }; },
    };
    return { t, sent, deliver: (p: unknown) => handler?.(p) };
  }
  function deps(over: Partial<PopoutHostDeps>) {
    const ft = fakeTransport();
    const d: PopoutHostDeps = {
      transport: ft.t,
      openWindow: vi.fn(async () => undefined),
      inputs: () => ({ activity: IDLE_ACTIVITY, providerKind: "local", providerLabel: "l", modelLabel: "m", modelId: null, tier: null }),
      subscribe: () => () => undefined,
      stop: vi.fn(),
      contextWindow: vi.fn(async () => 8192),
      now: () => 42,
      throttleMs: 0,
      ...over,
    };
    return { d, ft };
  }
  const flush = () => new Promise((r) => setTimeout(r, 0));

  it("reads it when the monitor opens and publishes a change", async () => {
    let report: DecideMetering | null = { ...METERING, backends: [METERING.backends[1]] };
    const decide = vi.fn(async () => report);
    const { d, ft } = deps({ decide, workersPollMs: 5 });
    const h = await createPopoutHost(d);
    expect(decide).not.toHaveBeenCalled();
    ft.deliver({ v: 1, type: "popout.ready", kind: "monitor" });
    await flush();
    expect(ft.sent[0].payload.snapshot.decide.rows[0].tasksSucceeded).toBe(9);
    report = { ...METERING, backends: [{ ...METERING.backends[1], tasksAttempted: 11, tasksSucceeded: 10, taskSuccessBps: 9090 }] };
    await new Promise((r) => setTimeout(r, 30));
    const last = ft.sent[ft.sent.length - 1].payload.snapshot;
    expect(last.decide.rows[0].tasksSucceeded).toBe(10);
    h.dispose();
  });

  it("a failed read is shown as unknown", async () => {
    const { d, ft } = deps({ decide: vi.fn(async () => { throw new Error("sidecar down"); }), workersPollMs: 1000 });
    const h = await createPopoutHost(d);
    ft.deliver({ v: 1, type: "popout.ready", kind: "monitor" });
    await flush();
    expect(ft.sent[0].payload.snapshot.decide.rows).toBeNull();
    h.dispose();
  });
});
