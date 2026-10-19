// K2 (0.5.2): every path that restarts Hermes refreshes what the app records as Hermes's running
// web search and learning values from the saved settings, so the Controls restart notice follows
// the restart that really happened (the terminal switch's automatic restart, the Agent page's
// Start, the Controls restart), and nothing else clears it.
import { describe, it, expect, vi, afterEach, beforeEach } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";

const invokeMock = vi.hoisted(() => vi.fn());
vi.mock("../../bridge/tauri/invoke", async (orig) => ({
  ...(await orig<typeof import("../../bridge/tauri/invoke")>()),
  invoke: invokeMock,
}));

import { bridge } from "../../bridge";
import { store } from "../store";
import { agentSlice, startRuntime } from "./agent";
import { hermesRunningSlice, refreshHermesRunningFromSaved } from "./hermesRunning";
import { ControlsPanel, type ControlsIo } from "../../surfaces/hermes/ControlsPanel";
import { DEFAULT_WEB_SETTINGS, type WebSettingsIo } from "../../agent/webSearch";
import type { FlTrajectoryStatus } from "../../bridge/domains";
import type { TerminalAccessIo } from "../../agent/terminalAccess";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let root: Root | null = null;
let host: HTMLDivElement | null = null;
beforeEach(() => {
  hermesRunningSlice.set({ webSearch: null, learn: null });
  invokeMock.mockReset();
  invokeMock.mockRejectedValue(new Error("no desktop app"));
  store.setState({ hermesTerminal: false, hermesSidecarLoop: false });
});
afterEach(() => {
  act(() => root?.unmount());
  host?.remove();
  root = null;
  host = null;
  vi.restoreAllMocks();
});
const $ = (id: string) => host?.querySelector(`[data-testid="${id}"]`) as HTMLInputElement | null;
const flush = () => act(async () => await new Promise((r) => setTimeout(r, 0)));
async function click(el: Element | null) {
  expect(el).toBeTruthy();
  await act(async () => {
    (el as HTMLElement).click();
  });
  await flush();
}

const traj = (enabled: boolean): FlTrajectoryStatus => ({ settings: { enabled, changedAtMs: null }, recordedFiles: 0, datasets: 0, appliesOnRestart: true, loadError: null });

/** One saved trajectory switch shared by the panel and bridge.flRounds (as in core). */
function learnStore(initial: boolean) {
  let saved = initial;
  const status = async () => traj(saved);
  const set = async (enabled: boolean) => {
    saved = enabled;
    return traj(enabled);
  };
  vi.spyOn(bridge.flRounds, "trajectoryStatus").mockImplementation(status);
  vi.spyOn(bridge.flRounds, "setTrajectoryConsent").mockImplementation(set);
  return { status, set };
}

/** The terminal switch's core side: `restarted` says whether a running Hermes was restarted. */
function terminalCore(restarted: boolean) {
  const io: TerminalAccessIo = { mode: "tauri", invoke: (async (_c: string, a?: Record<string, unknown>) => ({ enabled: a?.enabled, restarted })) as TerminalAccessIo["invoke"] };
  vi.spyOn(store as unknown as { terminalIo: () => TerminalAccessIo }, "terminalIo").mockReturnValue(io);
}

async function mountPanel(learn: { status: () => Promise<FlTrajectoryStatus>; set: (e: boolean) => Promise<FlTrajectoryStatus> }) {
  const web: WebSettingsIo = { mode: "sim", invoke: (async () => ({})) as WebSettingsIo["invoke"] };
  const io: ControlsIo = {
    web,
    fl: { trajectoryStatus: learn.status, setTrajectoryConsent: (e: boolean) => learn.set(e) },
    daemons: { available: false, refresh: async () => {}, setAllPaused: async () => null },
    escalation: { budget: async () => ({ capMicros: 0, usedMicros: 0, remainingMicros: 0, confirmedMicros: 0, periodStartMs: 0, periodEndMs: 0, maxCapMicros: 0, unreadable: false, history: [] }) },
    restartHermes: async () => ({ ok: true }),
  };
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  const render = async () =>
    act(async () => {
      root?.render(<ControlsPanel store={store} s={{ hermesTerminal: store.state.hermesTerminal, hermesReadAloud: false }} io={io} />);
    });
  await render();
  await flush();
  return render;
}

describe("Hermes running settings follow every restart", () => {
  it("learning off, turned on here (pending), then the terminal switch restarts Hermes: the notice clears and the app records learning on", async () => {
    const learn = learnStore(false);
    terminalCore(true);
    const render = await mountPanel(learn);
    await click($("toggle-learn"));
    expect($("pending-learn")?.textContent).toContain("does not record sessions");
    await click($("toggle-terminal"));
    await render();
    expect(hermesRunningSlice.get().learn).toBe(true);
    expect($("pending-learn")).toBeNull();
  });

  it("the reverse: learning on, turned off here, then the terminal restart: the notice clears and the app records learning off", async () => {
    const learn = learnStore(true);
    terminalCore(true);
    const render = await mountPanel(learn);
    await click($("toggle-learn"));
    expect($("pending-learn")?.textContent).toContain("still records sessions");
    await click($("toggle-terminal"));
    await render();
    expect(hermesRunningSlice.get().learn).toBe(false);
    expect($("pending-learn")).toBeNull();
  });

  it("a terminal change that did not restart Hermes (it was not running) leaves the notice", async () => {
    const learn = learnStore(false);
    terminalCore(false);
    const render = await mountPanel(learn);
    await click($("toggle-learn"));
    await click($("toggle-terminal"));
    await render();
    expect(hermesRunningSlice.get().learn).toBe(false);
    expect($("pending-learn")).toBeTruthy();
  });

  it("refresh reads the saved web search value through hermes_web_settings_get", async () => {
    learnStore(false);
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === "hermes_web_settings_get") return { settings: { ...DEFAULT_WEB_SETTINGS, searchEnabled: true } };
      throw new Error("unexpected " + cmd);
    });
    hermesRunningSlice.set({ webSearch: false, learn: true });
    await refreshHermesRunningFromSaved();
    expect(invokeMock).toHaveBeenCalledWith("hermes_web_settings_get");
    expect(hermesRunningSlice.get()).toEqual({ webSearch: true, learn: false });
  });

  it("a saved value that cannot be read becomes unknown, never a guess", async () => {
    vi.spyOn(bridge.flRounds, "trajectoryStatus").mockRejectedValue(new Error("no desktop app"));
    hermesRunningSlice.set({ webSearch: true, learn: true });
    await refreshHermesRunningFromSaved();
    expect(hermesRunningSlice.get()).toEqual({ webSearch: null, learn: null });
  });

  it("the Agent page's Start (Hermes was off, now confirmed running) refreshes from the saved settings", async () => {
    learnStore(true);
    let running = false;
    agentSlice.set({ busy: false, attached: agentSlice.get().attached.map((r) => (r.id === "hermes" ? { ...r, state: "off", error: null } : r)) });
    vi.spyOn(bridge.agentHarness, "start").mockImplementation(async () => {
      running = true;
    });
    vi.spyOn(bridge.agentHarness, "status").mockImplementation(async () => ({ running, skills: 0, pendingApprovals: 0 }));
    vi.spyOn(bridge.agentHarness, "skills").mockResolvedValue([]);
    vi.spyOn(bridge.agentHarness, "pendingApprovals").mockResolvedValue([]);
    vi.spyOn(bridge.agentHarness, "registrySkills").mockResolvedValue([]);
    hermesRunningSlice.set({ webSearch: null, learn: false });
    await startRuntime("hermes");
    expect(hermesRunningSlice.get().learn).toBe(true);
  });

  it("a Start that finds Hermes already running did not restart it, so nothing is refreshed", async () => {
    learnStore(true);
    agentSlice.set({ busy: false, attached: agentSlice.get().attached.map((r) => (r.id === "hermes" ? { ...r, state: "off", error: null } : r)) });
    vi.spyOn(bridge.agentHarness, "start").mockRejectedValue(new Error("Hermes is already running"));
    vi.spyOn(bridge.agentHarness, "status").mockResolvedValue({ running: true, skills: 0, pendingApprovals: 0 });
    vi.spyOn(bridge.agentHarness, "skills").mockResolvedValue([]);
    vi.spyOn(bridge.agentHarness, "pendingApprovals").mockResolvedValue([]);
    vi.spyOn(bridge.agentHarness, "registrySkills").mockResolvedValue([]);
    hermesRunningSlice.set({ webSearch: null, learn: false });
    await startRuntime("hermes");
    expect(hermesRunningSlice.get().learn).toBe(false);
  });
});
