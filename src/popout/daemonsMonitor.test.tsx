// HUP-S10.3 (US-10.3 AC2) — daemons are visible in the Activity monitor and can be paused from it.
// The snapshot carries one row per daemon (status, today's budget use, next run, last outcome); the
// pop-out can ask the main window to pause or resume a daemon, or stop the run in flight, through
// the same validated bridge. Nothing else crosses.
import { describe, it, expect, vi, afterEach } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { ActivityMonitor } from "./ActivityMonitor";
import { buildMonitorSnapshot, daemonsSection, isMonitorSnapshot, type MonitorInputs } from "./monitorSnapshot";
import { createMainEnd, parseToMain, type BridgeTransport } from "./bridge";
import { createPopoutHost } from "./host";
import { IDLE_ACTIVITY } from "../shell/slices/turnActivity";
import type { DaemonsView } from "../daemons/api";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const ID = "d0123456789abcdef";
const view: DaemonsView = {
  allPaused: false,
  daemons: [
    {
      id: ID,
      name: "Node digest",
      prompt: "Summarise my node.",
      schedule: "0 9 * * *",
      budget: { maxRunsPerDay: 4, maxTokensPerDay: 20_000, maxTokensPerRun: 6_000, maxSpendSalt: "0" },
      paused: false,
      status: "running",
      running: true,
      runsToday: 2,
      tokensToday: 4_100,
      skippedToday: 0,
      spendTodaySalt: "0",
      nextRunMs: 1_790_845_200_000,
      lastRunMs: 1_790_812_800_000,
      lastOutcome: "answered",
      lastNote: "Height 120,345.",
    },
    {
      id: "d1111111111111111",
      name: "Budget hog",
      prompt: "x",
      schedule: "* * * * *",
      budget: { maxRunsPerDay: 1, maxTokensPerDay: 1_000, maxTokensPerRun: 1_000, maxSpendSalt: "0" },
      paused: false,
      status: "budget used up today",
      running: false,
      runsToday: 1,
      tokensToday: 1_000,
      skippedToday: 5,
      spendTodaySalt: "0",
      nextRunMs: null,
      lastRunMs: null,
      lastOutcome: "over_budget",
      lastNote: null,
    },
  ],
};

const base: MonitorInputs = {
  activity: IDLE_ACTIVITY,
  providerKind: "local",
  providerLabel: "local model",
  modelLabel: "Gemma 4 E4B",
  modelId: "local:gemma",
  tier: "T0",
  localCtxTokens: 8192,
  now: 0,
};

describe("daemons in the monitor snapshot", () => {
  it("is empty and valid when nothing is scheduled", () => {
    const s = buildMonitorSnapshot(base);
    expect(s.daemons).toEqual({ allPaused: false, blocked: null, error: null, rows: [] });
    expect(isMonitorSnapshot(s)).toBe(true);
  });

  it("carries one row per daemon with today's budget use and spend fixed at zero", () => {
    const s = buildMonitorSnapshot({ ...base, daemons: daemonsSection(view, { blocked: null, error: null }) });
    expect(isMonitorSnapshot(s)).toBe(true);
    expect(s.daemons.rows).toEqual([
      { id: ID, name: "Node digest", status: "running", paused: false, running: true, runsToday: 2, maxRuns: 4, tokensToday: 4_100, maxTokens: 20_000, nextRunAt: 1_790_845_200_000, lastOutcome: "answered", lastNote: "Height 120,345.", lastTokenSource: null },
      { id: "d1111111111111111", name: "Budget hog", status: "budget used up today", paused: false, running: false, runsToday: 1, maxRuns: 1, tokensToday: 1_000, maxTokens: 1_000, nextRunAt: null, lastOutcome: "over_budget", lastNote: null, lastTokenSource: null },
    ]);
  });

  it("rejects a snapshot whose daemon rows are malformed", () => {
    const s = buildMonitorSnapshot({ ...base, daemons: daemonsSection(view, { blocked: null, error: null }) });
    const bad = JSON.parse(JSON.stringify(s));
    bad.daemons.rows[0].runsToday = "two";
    expect(isMonitorSnapshot(bad)).toBe(false);
    const bad2 = JSON.parse(JSON.stringify(s));
    delete bad2.daemons;
    expect(isMonitorSnapshot(bad2)).toBe(false);
  });
});

describe("daemon messages on the bridge", () => {
  it("accepts pause, resume and stop, with a well-formed daemon id only", () => {
    expect(parseToMain({ v: 1, type: "daemon.pause", id: ID, paused: true })).toEqual({ v: 1, type: "daemon.pause", id: ID, paused: true });
    expect(parseToMain({ v: 1, type: "daemon.pause", id: ID, paused: false })).toEqual({ v: 1, type: "daemon.pause", id: ID, paused: false });
    expect(parseToMain({ v: 1, type: "daemon.stop" })).toEqual({ v: 1, type: "daemon.stop" });
    for (const bad of [
      { v: 1, type: "daemon.pause", id: "../x", paused: true },
      { v: 1, type: "daemon.pause", id: ID, paused: "yes" },
      { v: 1, type: "daemon.pause", id: ID },
      { v: 1, type: "daemon.delete", id: ID },
      { v: 1, type: "daemon.save", input: {} },
      { v: 2, type: "daemon.stop" },
    ]) {
      expect(parseToMain(bad)).toBeNull();
    }
  });

  it("the main end routes them to the daemon handlers", async () => {
    let handler: (p: unknown) => void = () => undefined;
    const t: BridgeTransport = { send: async () => undefined, listen: async (h) => ((handler = h), () => undefined) };
    const onDaemonPause = vi.fn();
    const onDaemonStop = vi.fn();
    const onStop = vi.fn();
    await createMainEnd(t, { onReady: () => undefined, onStop, onDaemonPause, onDaemonStop });
    handler({ v: 1, type: "daemon.pause", id: ID, paused: true });
    handler({ v: 1, type: "daemon.stop" });
    expect(onDaemonPause).toHaveBeenCalledWith(ID, true);
    expect(onDaemonStop).toHaveBeenCalledTimes(1);
    expect(onStop).not.toHaveBeenCalled();
  });

  it("the host passes them to the app's daemon controls", async () => {
    let handler: (p: unknown) => void = () => undefined;
    const t: BridgeTransport = { send: async () => undefined, listen: async (h) => ((handler = h), () => undefined) };
    const pauseDaemon = vi.fn();
    const stopDaemon = vi.fn();
    await createPopoutHost({
      transport: t,
      openWindow: async () => undefined,
      inputs: () => ({ ...base }),
      subscribe: () => () => undefined,
      stop: () => undefined,
      contextWindow: async () => 8192,
      now: () => 0,
      pauseDaemon,
      stopDaemon,
    });
    handler({ v: 1, type: "daemon.pause", id: ID, paused: false });
    handler({ v: 1, type: "daemon.stop" });
    expect(pauseDaemon).toHaveBeenCalledWith(ID, false);
    expect(stopDaemon).toHaveBeenCalledTimes(1);
  });
});

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

describe("the Daemons section of the Activity monitor", () => {
  it("lists each daemon with its status, budget use and spend, and pauses one", () => {
    const s = buildMonitorSnapshot({ ...base, daemons: daemonsSection(view, { blocked: null, error: null }) });
    const onPause = vi.fn();
    const onStopDaemon = vi.fn();
    const el = render(<ActivityMonitor snapshot={s} now={0} onStop={() => undefined} onPauseDaemon={onPause} onStopDaemon={onStopDaemon} />);
    const rows = el.querySelectorAll('[data-testid="mon-daemon-row"]');
    expect(rows).toHaveLength(2);
    expect(rows[0].textContent).toContain("Node digest");
    expect(rows[0].textContent).toContain("running");
    expect(rows[0].textContent).toContain("2 of 4 runs");
    expect(rows[0].textContent).toContain("4,100 of 20,000 tokens (estimated)");
    expect(rows[0].textContent).toContain("spend 0 SALT");
    expect(rows[1].textContent).toContain("budget used up today");
    act(() => (rows[0].querySelector('[data-testid="mon-daemon-pause"]') as HTMLButtonElement).click());
    expect(onPause).toHaveBeenCalledWith(ID, true);
    act(() => (el.querySelector('[data-testid="mon-daemon-stop"]') as HTMLButtonElement).click());
    expect(onStopDaemon).toHaveBeenCalledTimes(1);
  });

  it("says when no daemon exists, and why runs are held", () => {
    const s = buildMonitorSnapshot({ ...base, daemons: { allPaused: false, blocked: "daemons run only on the local model", error: null, rows: [] } });
    const el = render(<ActivityMonitor snapshot={s} now={0} onStop={() => undefined} />);
    expect(el.querySelector('[data-testid="mon-daemons"]')?.textContent).toContain("No daemons");
    expect(el.querySelector('[data-testid="mon-daemons"]')?.textContent).toContain("daemons run only on the local model");
  });
});
