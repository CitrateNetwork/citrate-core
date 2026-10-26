// K2 (0.5.2), Codex review of #341 at 98d152e: withHermesStart held NEW writes of the settings
// Hermes reads at start, but did not wait for a learning write already in flight. Enabling
// learning and starting Hermes (Agent Start, the terminal switch's restart) before core answered
// let the start read the old saved value while the app recorded the new one, or the reverse.
// A start that begins while a write of a start-time setting is in flight now waits for it, so
// what Hermes reads and what the app records are the same value.
import { describe, it, expect, vi, afterEach, beforeEach } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { bridge } from "../../bridge";
import { store } from "../store";
import { agentSlice, startRuntime } from "./agent";
import { hermesRunningSlice, hermesStarting } from "./hermesRunning";
import { ControlsPanel, type ControlsIo } from "../../surfaces/hermes/ControlsPanel";
import { TrajectoryConsent } from "../../fl/TrajectoryConsent";
import type { WebSettingsIo } from "../../agent/webSearch";
import type { FlRoundsDomain, FlTrajectoryStatus } from "../../bridge/domains";
import type { TerminalAccessIo } from "../../agent/terminalAccess";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let root: Root | null = null;
let host: HTMLDivElement | null = null;
beforeEach(() => {
  hermesRunningSlice.set({ webSearch: null, learn: null, starting: 0 });
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

/** Core's saved trajectory switch. A write is in flight until `answer` is released: core saves
 *  the value only when it handles the command, as IPC does. */
function slowLearnCore(initial: boolean) {
  const core = { saved: initial, writes: [] as boolean[] };
  const answer = gate();
  const status = async () => traj(core.saved);
  const set = async (enabled: boolean) => {
    await answer.done;
    core.writes.push(enabled);
    core.saved = enabled;
    return traj(enabled);
  };
  vi.spyOn(bridge.flRounds, "trajectoryStatus").mockImplementation(status);
  vi.spyOn(bridge.flRounds, "setTrajectoryConsent").mockImplementation(set);
  vi.spyOn(bridge.agentHarness, "skills").mockResolvedValue([]);
  vi.spyOn(bridge.agentHarness, "pendingApprovals").mockResolvedValue([]);
  vi.spyOn(bridge.agentHarness, "registrySkills").mockResolvedValue([]);
  return { core, status, set, answer };
}

/** Agent Start; `read` records the switch Hermes reads when its process starts. */
function hermesStartReads(learn: ReturnType<typeof slowLearnCore>, read: { value: boolean | null }) {
  let running = false;
  vi.spyOn(bridge.agentHarness, "start").mockImplementation(async () => {
    read.value = learn.core.saved;
    running = true;
  });
  vi.spyOn(bridge.agentHarness, "status").mockImplementation(async () => ({ running, skills: 0, pendingApprovals: 0 }));
}

async function mountPanel(learn: ReturnType<typeof slowLearnCore>) {
  const io: ControlsIo = {
    web: { mode: "sim", invoke: (async () => ({})) as WebSettingsIo["invoke"] },
    fl: { trajectoryStatus: learn.status, setTrajectoryConsent: (e: boolean) => learn.set(e) },
    daemons: { available: false, refresh: async () => {}, setAllPaused: async () => null },
    escalation: { budget: async () => ({ capMicros: 0, usedMicros: 0, remainingMicros: 0, confirmedMicros: 0, periodStartMs: 0, periodEndMs: 0, maxCapMicros: 0, unreadable: false, history: [] }) },
  };
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  await act(async () => {
    root?.render(<ControlsPanel store={store} s={{ hermesTerminal: store.state.hermesTerminal, hermesReadAloud: false }} io={io} />);
  });
  await flush();
}

async function mountTrain() {
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  await act(async () => {
    root?.render(<TrajectoryConsent fl={bridge.flRounds as FlRoundsDomain} toast={() => {}} />);
  });
  await flush();
}

/** Start Hermes while the learning write is in flight, then let core answer the write. */
async function startDuringWrite(learn: ReturnType<typeof slowLearnCore>, start: () => Promise<unknown>) {
  let started: Promise<unknown> = Promise.resolve();
  await act(async () => {
    started = start();
  });
  await flush();
  await act(async () => {
    learn.answer.release();
    await started;
  });
  await flush();
}

describe("a Hermes start waits for a start-time setting write already in flight", () => {
  it("Controls: learning on (core has not answered) -> Agent Start: Hermes reads on and the app records on", async () => {
    const learn = slowLearnCore(false);
    const read = { value: null as boolean | null };
    hermesStartReads(learn, read);
    await mountPanel(learn);
    await click($("toggle-learn")); // the write is in flight
    expect(learn.core.writes).toEqual([]);
    await startDuringWrite(learn, () => startRuntime("hermes"));
    expect(learn.core.writes).toEqual([true]);
    expect(read.value).toBe(true);
    expect(hermesRunningSlice.get().learn).toBe(read.value);
    expect(hermesStarting()).toBe(false);
  });

  it("Train: learning on (core has not answered) -> Agent Start: Hermes reads on and the app records on", async () => {
    const learn = slowLearnCore(false);
    const read = { value: null as boolean | null };
    hermesStartReads(learn, read);
    await mountTrain();
    await click($("fl-traj-toggle")); // the write is in flight
    expect(learn.core.writes).toEqual([]);
    await startDuringWrite(learn, () => startRuntime("hermes"));
    expect(learn.core.writes).toEqual([true]);
    expect(read.value).toBe(true);
    expect(hermesRunningSlice.get().learn).toBe(read.value);
  });

  it("Controls: learning on (in flight) -> terminal switch restarts Hermes: the restart reads on and the app records on", async () => {
    const learn = slowLearnCore(false);
    const read = { value: null as boolean | null };
    const io: TerminalAccessIo = {
      mode: "tauri",
      invoke: (async (_c: string, a?: Record<string, unknown>) => {
        read.value = learn.core.saved; // Hermes restarts here and reads the saved switch
        return { enabled: a?.enabled, restarted: true };
      }) as TerminalAccessIo["invoke"],
    };
    vi.spyOn(store as unknown as { terminalIo: () => TerminalAccessIo }, "terminalIo").mockReturnValue(io);
    await mountPanel(learn);
    await click($("toggle-learn"));
    await startDuringWrite(learn, () => store.setHermesTerminal(true));
    expect(learn.core.writes).toEqual([true]);
    expect(read.value).toBe(true);
    expect(hermesRunningSlice.get().learn).toBe(true);
  });

  it("a learning write asked for while a start is in flight is held, not run (the existing hold still applies)", async () => {
    const learn = slowLearnCore(false);
    learn.answer.release();
    const g = gate();
    let running = false;
    vi.spyOn(bridge.agentHarness, "start").mockImplementation(async () => {
      await g.done;
      running = true;
    });
    vi.spyOn(bridge.agentHarness, "status").mockImplementation(async () => ({ running, skills: 0, pendingApprovals: 0 }));
    await mountTrain();
    let started: Promise<void> = Promise.resolve();
    await act(async () => {
      started = startRuntime("hermes");
    });
    await flush();
    await click($("fl-traj-toggle"));
    await act(async () => {
      g.release();
      await started;
    });
    await flush();
    expect(learn.core.writes).toEqual([]);
  });
});

// Tripwire: every app write of a setting Hermes reads at start (web settings, learning, the MCP
// allowlist) goes
// through the start-time settings lock (`...UnlessStarting(`), so a start waits for it and a write
// asked for during a start is held. A new writer that calls core directly fails here.
describe("tripwire: start-time setting writes go through the lock", () => {
  it("every saveWebSettings / setTrajectoryConsent / mcpSet call in src/ sits inside an ...UnlessStarting( block", async () => {
    const fs = await import("node:fs");
    const path = await import("node:path");
    const srcDir = path.resolve(__dirname, "../..");
    const files: string[] = [];
    const walk = (d: string) => {
      for (const e of fs.readdirSync(d, { withFileTypes: true })) {
        const p = path.join(d, e.name);
        if (e.isDirectory()) walk(p);
        else if (/\.(ts|tsx)$/.test(e.name) && !/\.test\.(ts|tsx)$/.test(e.name)) files.push(p);
      }
    };
    walk(srcDir);
    const offenders: string[] = [];
    let calls = 0;
    for (const f of files) {
      const rel = path.relative(srcDir, f);
      // The bridge adapters and the one save helper are the IPC itself, not app writers.
      if (rel.startsWith("bridge" + path.sep) || rel === path.join("agent", "webSearch.ts")) continue;
      const lines = fs.readFileSync(f, "utf8").split("\n");
      lines.forEach((l: string, i: number) => {
        if (!/(\bsaveWebSettings|\.setTrajectoryConsent|\.mcpSet)\(/.test(l) || /^\s*(\/\/|\*|import\b)/.test(l)) return;
        calls++;
        const window = lines.slice(Math.max(0, i - 8), i + 1).join("\n");
        if (!/UnlessStarting\(/.test(window)) offenders.push(`${rel}:${i + 1}`);
      });
    }
    // patchWebSettings (the one web writer) and the two learning writers.
    expect(calls).toBeGreaterThanOrEqual(3);
    expect(offenders).toEqual([]);
  });
});
