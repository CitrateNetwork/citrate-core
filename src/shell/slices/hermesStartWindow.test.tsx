// K2 (0.5.2): Hermes reads the saved web search and learning settings AT START, so what the app
// records as Hermes's running values must be the saved values from before the start, and nothing
// may change them while a start or restart is in flight. Regression for the start window
// (up to ~12 s): learning off -> on -> restart/start -> off before the start is confirmed must not
// hide the notice while Hermes records. One test per path: Controls restart, Agent Start, terminal
// restart; plus the Settings and Train writers held during the window.
import { describe, it, expect, vi, afterEach, beforeEach } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { bridge } from "../../bridge";
import { store } from "../store";
import { agentSlice, startRuntime } from "./agent";
import { hermesRunningSlice } from "./hermesRunning";
import { ControlsPanel, type ControlsIo } from "../../surfaces/hermes/ControlsPanel";
import { WebSearchSettings } from "../../surfaces/WebSearchSettings";
import { TrajectoryConsent } from "../../fl/TrajectoryConsent";
import { DEFAULT_WEB_SETTINGS, type HermesWebStatus, type WebSettingsIo } from "../../agent/webSearch";
import type { FlRoundsDomain, FlTrajectoryStatus } from "../../bridge/domains";
import type { TerminalAccessIo } from "../../agent/terminalAccess";
import type { HermesRestartResult } from "./agent";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let root: Root | null = null;
let host: HTMLDivElement | null = null;
beforeEach(() => {
  hermesRunningSlice.set({ webSearch: null, learn: null });
  store.setState({ hermesTerminal: false, hermesSidecarLoop: false });
  agentSlice.set({ busy: false, error: null, attached: agentSlice.get().attached.map((r) => (r.id === "hermes" ? { ...r, state: "off", error: null } : r)) });
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
function gate() {
  let release: () => void = () => {};
  const done = new Promise<void>((r) => (release = r));
  return { done, release };
}

const traj = (enabled: boolean): FlTrajectoryStatus => ({ settings: { enabled, changedAtMs: null }, recordedFiles: 0, datasets: 0, appliesOnRestart: true, loadError: null });

/** Core's saved trajectory switch, shared by every card and by bridge.flRounds. */
function learnCore(initial: boolean) {
  const core = { saved: initial, writes: [] as boolean[] };
  const status = async () => traj(core.saved);
  const set = async (enabled: boolean) => {
    core.writes.push(enabled);
    core.saved = enabled;
    return traj(enabled);
  };
  vi.spyOn(bridge.flRounds, "trajectoryStatus").mockImplementation(status);
  vi.spyOn(bridge.flRounds, "setTrajectoryConsent").mockImplementation(set);
  return { core, status, set };
}

/** Core's saved web settings. */
function webCore(searchEnabled: boolean) {
  const core = { current: { settings: { ...DEFAULT_WEB_SETTINGS, searchEnabled }, managedChromium: null, searxngFound: false, jinaKeyFileFound: false, jevKeyFileFound: false, notices: [], appliesOnRestart: true } as HermesWebStatus, writes: 0 };
  const io: WebSettingsIo = {
    mode: "tauri",
    invoke: (async (cmd: string, args?: Record<string, unknown>) => {
      if (cmd === "hermes_web_settings_set") {
        core.writes++;
        core.current = { ...core.current, settings: args?.settings as HermesWebStatus["settings"] };
      }
      return core.current;
    }) as WebSettingsIo["invoke"],
  };
  return { core, io };
}

function harnessUp() {
  vi.spyOn(bridge.agentHarness, "skills").mockResolvedValue([]);
  vi.spyOn(bridge.agentHarness, "pendingApprovals").mockResolvedValue([]);
  vi.spyOn(bridge.agentHarness, "registrySkills").mockResolvedValue([]);
}

async function mountPanel(learn: ReturnType<typeof learnCore>, restartHermes?: () => Promise<HermesRestartResult>) {
  const io: ControlsIo = {
    web: { mode: "sim", invoke: (async () => ({})) as WebSettingsIo["invoke"] },
    fl: { trajectoryStatus: learn.status, setTrajectoryConsent: (e: boolean) => learn.set(e) },
    daemons: { available: false, refresh: async () => {}, setAllPaused: async () => null },
    escalation: { budget: async () => ({ capMicros: 0, usedMicros: 0, remainingMicros: 0, confirmedMicros: 0, periodStartMs: 0, periodEndMs: 0, maxCapMicros: 0, unreadable: false, history: [] }) },
    ...(restartHermes ? { restartHermes } : {}),
  };
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  await act(async () => {
    root?.render(<ControlsPanel store={store} s={{ hermesTerminal: store.state.hermesTerminal, hermesReadAloud: false }} io={io} />);
  });
  await flush();
}

describe("the Hermes start window", () => {
  it("Controls restart: off -> on -> Restart -> off before the confirm: the late write is held and Hermes is recorded as recording", async () => {
    const learn = learnCore(false);
    const g = gate();
    let readAtStart: boolean | null = null;
    await mountPanel(learn, async () => {
      readAtStart = learn.core.saved; // Hermes reads the saved switch when it starts
      await g.done;
      return { ok: true };
    });
    await click($("toggle-learn")); // on (pending)
    expect($("pending-learn")).toBeTruthy();
    await click($("restart-learn")); // restart in flight
    expect($("toggle-learn")?.disabled).toBe(true);
    await click($("toggle-learn")); // try to turn it off before the confirm
    await act(async () => g.release());
    await flush();
    expect(readAtStart).toBe(true);
    expect(learn.core.writes).toEqual([true]);
    expect(hermesRunningSlice.get().learn).toBe(true);
    // After the restart the member can turn it off, and the notice then says Hermes still records.
    await click($("toggle-learn"));
    expect($("pending-learn")?.textContent).toContain("still records sessions");
  });

  it("Agent Start: off -> on -> Start -> off before the confirm: the late write is held and Hermes is recorded as recording", async () => {
    const learn = learnCore(false);
    harnessUp();
    const g = gate();
    let running = false;
    vi.spyOn(bridge.agentHarness, "start").mockImplementation(async () => {
      await g.done;
      running = true;
    });
    vi.spyOn(bridge.agentHarness, "status").mockImplementation(async () => ({ running, skills: 0, pendingApprovals: 0 }));
    await mountPanel(learn);
    await click($("toggle-learn")); // on
    let started: Promise<void> = Promise.resolve();
    await act(async () => {
      started = startRuntime("hermes");
    });
    await flush();
    expect($("toggle-learn")?.disabled).toBe(true);
    await click($("toggle-learn")); // try off during the start window
    await act(async () => {
      g.release();
      await started;
    });
    await flush();
    expect(learn.core.writes).toEqual([true]);
    expect(hermesRunningSlice.get().learn).toBe(true);
  });

  it("terminal restart: off -> on -> terminal switch (restarts Hermes) -> off before core answers: held, recorded as recording", async () => {
    const learn = learnCore(false);
    const g = gate();
    const io: TerminalAccessIo = {
      mode: "tauri",
      invoke: (async (_c: string, a?: Record<string, unknown>) => {
        await g.done;
        return { enabled: a?.enabled, restarted: true };
      }) as TerminalAccessIo["invoke"],
    };
    vi.spyOn(store as unknown as { terminalIo: () => TerminalAccessIo }, "terminalIo").mockReturnValue(io);
    await mountPanel(learn);
    await click($("toggle-learn")); // on
    let flipped: Promise<void> = Promise.resolve();
    await act(async () => {
      flipped = store.setHermesTerminal(true);
    });
    await flush();
    expect($("toggle-learn")?.disabled).toBe(true);
    await click($("toggle-learn")); // try off during the restart
    await act(async () => {
      g.release();
      await flipped;
    });
    await flush();
    expect(learn.core.writes).toEqual([true]);
    expect(hermesRunningSlice.get().learn).toBe(true);
  });

  it("the record is the saved value from BEFORE the start, even if the saved file changes while Hermes starts", async () => {
    const learn = learnCore(true);
    harnessUp();
    let running = false;
    vi.spyOn(bridge.agentHarness, "start").mockImplementation(async () => {
      // Hermes has read "on"; the file then changes before the start is confirmed.
      learn.core.saved = false;
      running = true;
    });
    vi.spyOn(bridge.agentHarness, "status").mockImplementation(async () => ({ running, skills: 0, pendingApprovals: 0 }));
    hermesRunningSlice.set({ webSearch: null, learn: false });
    await startRuntime("hermes");
    expect(hermesRunningSlice.get().learn).toBe(true);
  });

  it("Settings › Web search: Save is held while Hermes starts", async () => {
    harnessUp();
    learnCore(false);
    const web = webCore(false);
    const g = gate();
    let running = false;
    vi.spyOn(bridge.agentHarness, "start").mockImplementation(async () => {
      await g.done;
      running = true;
    });
    vi.spyOn(bridge.agentHarness, "status").mockImplementation(async () => ({ running, skills: 0, pendingApprovals: 0 }));
    host = document.createElement("div");
    document.body.appendChild(host);
    root = createRoot(host);
    await act(async () => {
      root?.render(<WebSearchSettings io={web.io} />);
    });
    await flush();
    await click($("web-search-toggle"));
    let started: Promise<void> = Promise.resolve();
    await act(async () => {
      started = startRuntime("hermes");
    });
    await flush();
    expect((host.querySelector('[data-testid="web-save"]') as HTMLButtonElement).disabled).toBe(true);
    await click($("web-save"));
    expect(web.core.writes).toBe(0);
    await act(async () => {
      g.release();
      await started;
    });
    await flush();
    await click($("web-save"));
    expect(web.core.writes).toBe(1);
  });

  it("Train › Training data: the switch is held while Hermes starts", async () => {
    harnessUp();
    const learn = learnCore(false);
    const g = gate();
    let running = false;
    vi.spyOn(bridge.agentHarness, "start").mockImplementation(async () => {
      await g.done;
      running = true;
    });
    vi.spyOn(bridge.agentHarness, "status").mockImplementation(async () => ({ running, skills: 0, pendingApprovals: 0 }));
    host = document.createElement("div");
    document.body.appendChild(host);
    root = createRoot(host);
    await act(async () => {
      root?.render(<TrajectoryConsent fl={bridge.flRounds as FlRoundsDomain} toast={() => {}} />);
    });
    await flush();
    let started: Promise<void> = Promise.resolve();
    await act(async () => {
      started = startRuntime("hermes");
    });
    await flush();
    expect($("fl-traj-toggle")?.disabled).toBe(true);
    await click($("fl-traj-toggle"));
    expect(learn.core.writes).toEqual([]);
    await act(async () => {
      g.release();
      await started;
    });
    await flush();
    expect($("fl-traj-toggle")?.disabled).toBe(false);
  });
});
