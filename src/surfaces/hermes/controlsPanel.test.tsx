// K2 (0.5.2): Hermes › Controls, first depth. Each toggle reads the config its Settings (or home)
// card reads today and writes through the same call; nothing is switched on by the panel itself
// (no daemon created, no spend raised, no terminal or web search turned on at mount); a toggle with
// no backing store renders disabled with its reason (Rule 7).
import { describe, it, expect, vi, afterEach, beforeEach } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { ControlsPanel, CONTROL_ROWS, type ControlsIo } from "./ControlsPanel";
import { DEFAULT_WEB_SETTINGS, type HermesWebStatus, type WebSettingsIo } from "../../agent/webSearch";
import type { EscalationBudget, FlTrajectoryStatus } from "../../bridge/domains";
import { daemonsSlice } from "../../daemons/slice";
import type { DaemonsView } from "../../daemons/api";
import { simFlRounds } from "../../bridge/sim/flRounds";
import { simEscalation } from "../../bridge/sim/escalation";
import type { SimHost } from "../../bridge/sim/index";
import { store } from "../../shell/store";
import { WebSearchSettings } from "../WebSearchSettings";
import type { HermesRestartResult } from "../../shell/slices/agent";
import { hermesRunningSlice } from "../../shell/slices/hermesRunning";
import { TrajectoryConsent } from "../../fl/TrajectoryConsent";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let root: Root | null = null;
let host: HTMLDivElement | null = null;
afterEach(() => {
  act(() => root?.unmount());
  host?.remove();
  root = null;
  host = null;
  vi.restoreAllMocks();
});
beforeEach(() => {
  daemonsSlice.set({ view: null, loaded: false, error: null });
  // A fresh app session: nothing known yet about what the running Hermes started with.
  hermesRunningSlice.set({ webSearch: null, learn: null });
});

const $ = (id: string) => host?.querySelector(`[data-testid="${id}"]`) as HTMLInputElement | null;
const flush = () => act(async () => await new Promise((r) => setTimeout(r, 0)));

type Calls = { cmd: string; args?: unknown }[];

/** A stand-in for core's hermes_web.rs store: one saved settings object that any writer (this
 *  panel, or the Settings form next to it) reads and replaces. */
interface WebBacking {
  current: HermesWebStatus;
}
function webIo(backing: WebBacking, calls: Calls, mode: "tauri" | "sim" = "tauri"): WebSettingsIo {
  return {
    mode,
    invoke: (async (cmd: string, args?: Record<string, unknown>) => {
      calls.push({ cmd, args });
      if (cmd === "hermes_web_settings_get") return backing.current;
      backing.current = { ...backing.current, settings: (args?.settings ?? DEFAULT_WEB_SETTINGS) as HermesWebStatus["settings"] };
      return backing.current;
    }) as WebSettingsIo["invoke"],
  };
}

const webStatus = (over: Partial<HermesWebStatus["settings"]> = {}): HermesWebStatus => ({
  settings: { ...DEFAULT_WEB_SETTINGS, ...over },
  managedChromium: null,
  searxngFound: false,
  jinaKeyFileFound: false,
  jevKeyFileFound: false,
  notices: [],
  appliesOnRestart: true,
});

const trajStatus = (enabled: boolean): FlTrajectoryStatus => ({
  settings: { enabled, changedAtMs: null },
  recordedFiles: 0,
  datasets: 0,
  appliesOnRestart: true,
  loadError: null,
});

const zeroBudget: EscalationBudget = {
  capMicros: 0,
  usedMicros: 0,
  remainingMicros: 0,
  confirmedMicros: 0,
  periodStartMs: 0,
  periodEndMs: 0,
  maxCapMicros: 100_000_000,
  unreadable: false,
  history: [],
};

const daemonView = (allPaused: boolean, n: number): DaemonsView => ({
  allPaused,
  daemons: Array.from({ length: n }, (_, i) => ({
    id: "d" + i,
    name: "Digest " + i,
    prompt: "Summarise.",
    schedule: "0 9 * * *",
    budget: { maxRunsPerDay: 4, maxTokensPerDay: 20_000, maxTokensPerRun: 6_000, maxSpendSalt: "0" },
    paused: allPaused,
    status: allPaused ? "paused" : "scheduled",
    running: false,
    runsToday: 0,
    tokensToday: 0,
    skippedToday: 0,
    spendTodaySalt: "0",
    nextRunMs: null,
    lastRunMs: null,
    lastOutcome: null,
    lastNote: null,
  })),
});

interface Harness {
  io: ControlsIo;
  calls: Calls;
  web: WebBacking;
  restart: ReturnType<typeof vi.fn>;
  fl: { status: ReturnType<typeof vi.fn>; set: ReturnType<typeof vi.fn> };
  daemons: { refresh: ReturnType<typeof vi.fn>; setAllPaused: ReturnType<typeof vi.fn> };
  budget: ReturnType<typeof vi.fn>;
}

function harness(
  o: { web?: HermesWebStatus; webMode?: "tauri" | "sim"; learn?: boolean; learnAppliesOnRestart?: boolean; daemons?: DaemonsView | null; daemonsAvailable?: boolean; withRestart?: boolean } = {},
): Harness {
  const calls: Calls = [];
  const web: WebBacking = { current: o.web ?? webStatus() };
  const restart = vi.fn(async (): Promise<HermesRestartResult> => ({ ok: true }));
  let learnOn = o.learn ?? false;
  const fl = {
    status: vi.fn(async () => ({ ...trajStatus(learnOn), appliesOnRestart: o.learnAppliesOnRestart ?? true })),
    set: vi.fn(async (enabled: boolean, _del: boolean) => {
      learnOn = enabled;
      return { ...trajStatus(enabled), appliesOnRestart: o.learnAppliesOnRestart ?? true };
    }),
  };
  const daemons = {
    refresh: vi.fn(async () => {
      daemonsSlice.set({ view: o.daemons ?? { allPaused: false, daemons: [] }, loaded: true, error: null });
    }),
    setAllPaused: vi.fn(async (paused: boolean) => {
      const v = daemonsSlice.get().view ?? { allPaused: false, daemons: [] };
      daemonsSlice.set({ view: { ...v, allPaused: paused } });
      return null;
    }),
  };
  const budget = vi.fn(async () => zeroBudget);
  return {
    calls,
    web,
    restart,
    fl,
    daemons,
    budget,
    io: {
      ...(o.withRestart ? { restartHermes: restart } : {}),
      web: webIo(web, calls, o.webMode ?? "tauri"),
      fl: { trajectoryStatus: fl.status, setTrajectoryConsent: fl.set },
      daemons: { available: o.daemonsAvailable ?? true, refresh: daemons.refresh, setAllPaused: daemons.setAllPaused },
      escalation: { budget },
    },
  };
}

const fakeStore = () => ({ setHermesTerminal: vi.fn(async () => {}), setHermesReadAloud: vi.fn(), setChatRoute: vi.fn(async () => {}) });

async function mount(h: Harness, s: { hermesTerminal: boolean; hermesReadAloud: boolean; chatRoute?: "ask_first" | "local_only" } = { hermesTerminal: false, hermesReadAloud: false }, st: ReturnType<typeof fakeStore> | typeof store = fakeStore()) {
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  await act(async () => {
    root?.render(<ControlsPanel store={st} s={s} io={h.io} />);
  });
  await flush();
  return st;
}

async function click(el: Element | null) {
  expect(el).toBeTruthy();
  await act(async () => {
    (el as HTMLElement).click();
  });
  await flush();
}

describe("Hermes › Controls, first depth (K2)", () => {
  it("lists the six first-depth controls, each with a one-line description, a data source and an inert 'more'", async () => {
    await mount(harness());
    expect(CONTROL_ROWS.map((r) => r.id)).toEqual(["local-model", "web-search", "terminal", "daemons", "learn", "voice"]);
    for (const r of CONTROL_ROWS) {
      expect(r.description.length).toBeGreaterThan(10);
      expect(r.description).not.toContain("—");
      expect(r.source.length).toBeGreaterThan(5);
      const row = $(`control-${r.id}`);
      expect(row).toBeTruthy();
      expect(row?.getAttribute("data-source")).toBe(r.source);
      const more = $(`more-${r.id}`);
      expect(more).toBeTruthy();
      expect((more as unknown as HTMLButtonElement).disabled).toBe(true);
      expect(more?.textContent).toContain("coming next");
    }
  });

  it("mounting only reads: no write call to any store, so nothing new is allowed by the panel itself", async () => {
    const h = harness();
    const st = await mount(h);
    expect(h.calls.map((c) => c.cmd)).toEqual(["hermes_web_settings_get"]);
    expect(h.fl.set).not.toHaveBeenCalled();
    expect(h.daemons.setAllPaused).not.toHaveBeenCalled();
    expect((st as ReturnType<typeof fakeStore>).setHermesTerminal).not.toHaveBeenCalled();
    expect((st as ReturnType<typeof fakeStore>).setHermesReadAloud).not.toHaveBeenCalled();
    expect((st as ReturnType<typeof fakeStore>).setChatRoute).not.toHaveBeenCalled();
  });

  // ---------- Local model only / ask before the gateway ----------
  // ---------- Local model only (K2 defaults: a real stored setting, chat_route.rs) ----------
  it("local model: reads the stored route; ask first (the default) shows off, and the spend line reads zero", async () => {
    const h = harness();
    await mount(h, { hermesTerminal: false, hermesReadAloud: false, chatRoute: "ask_first" });
    const t = $("toggle-local-model");
    expect(t?.disabled).toBe(false);
    expect(t?.checked).toBe(false);
    expect($("reason-local-model")).toBeNull();
    expect($("control-local-model")?.getAttribute("data-source")).toContain("chat_route_get");
    expect(h.budget).toHaveBeenCalledTimes(1);
    expect($("local-model-spend")?.textContent).toContain("$0.00");
    expect($("local-model-spend")?.textContent).toContain("asks you first");
  });

  it("local model: turning it on stores local_only through store.setChatRoute", async () => {
    const h = harness();
    const st = (await mount(h, { hermesTerminal: false, hermesReadAloud: false, chatRoute: "ask_first" })) as ReturnType<typeof fakeStore>;
    await click($("toggle-local-model"));
    expect(st.setChatRoute).toHaveBeenCalledWith("local_only");
  });

  it("local model: a stored local_only shows on, and turning it off stores ask_first", async () => {
    const h = harness();
    const st = (await mount(h, { hermesTerminal: false, hermesReadAloud: false, chatRoute: "local_only" })) as ReturnType<typeof fakeStore>;
    expect($("toggle-local-model")?.checked).toBe(true);
    await click($("toggle-local-model"));
    expect(st.setChatRoute).toHaveBeenCalledWith("ask_first");
  });

  it("local model: in the web preview the switch is disabled (the setting lives in the desktop app)", async () => {
    const h = harness({ webMode: "sim" });
    await mount(h);
    expect($("toggle-local-model")?.disabled).toBe(true);
    expect($("reason-local-model")?.textContent).toContain("desktop app");
  });

  it("local model: the spend line also reads zero through the sim escalation bridge (web preview)", async () => {
    const h = harness();
    h.io.escalation = simEscalation({} as SimHost);
    await mount(h);
    expect($("local-model-spend")?.textContent).toContain("$0.00");
  });

  // ---------- Web search ----------
  it("web search: reads hermes_web_settings_get, off by default", async () => {
    const h = harness();
    await mount(h);
    expect(h.calls[0].cmd).toBe("hermes_web_settings_get");
    expect($("toggle-web-search")?.checked).toBe(false);
    expect($("toggle-web-search")?.disabled).toBe(false);
  });

  it("web search: turning it on writes hermes_web_settings_set with only searchEnabled changed (same call as Settings › Web search)", async () => {
    const h = harness({ web: webStatus({ browserEnabled: true, reader: "jina" }) });
    await mount(h);
    await click($("toggle-web-search"));
    const set = h.calls.find((c) => c.cmd === "hermes_web_settings_set");
    expect(set).toBeTruthy();
    expect((set?.args as { settings: HermesWebStatus["settings"] }).settings).toEqual({ ...DEFAULT_WEB_SETTINGS, browserEnabled: true, reader: "jina", searchEnabled: true });
    expect($("toggle-web-search")?.checked).toBe(true);
  });

  it("web search: reflects a stored 'on' and turns it off through the same call", async () => {
    const h = harness({ web: webStatus({ searchEnabled: true }) });
    await mount(h);
    expect($("toggle-web-search")?.checked).toBe(true);
    await click($("toggle-web-search"));
    const set = h.calls.find((c) => c.cmd === "hermes_web_settings_set");
    expect((set?.args as { settings: HermesWebStatus["settings"] }).settings.searchEnabled).toBe(false);
  });

  it("web search: in the web preview it is disabled and reads nothing", async () => {
    const h = harness({ webMode: "sim" });
    await mount(h);
    expect($("toggle-web-search")?.disabled).toBe(true);
    expect($("toggle-web-search")?.checked).toBe(false);
    expect(h.calls).toEqual([]);
    expect($("reason-web-search")?.textContent).toContain("desktop app");
  });

  // ---------- Terminal ----------
  it("terminal: reads the stored choice and writes through store.setHermesTerminal (same call as Settings › App)", async () => {
    const h = harness();
    const st = (await mount(h, { hermesTerminal: false, hermesReadAloud: false })) as ReturnType<typeof fakeStore>;
    expect($("toggle-terminal")?.checked).toBe(false);
    await click($("toggle-terminal"));
    expect(st.setHermesTerminal).toHaveBeenCalledWith(true);
  });

  it("terminal: a stored 'on' shows on, and turning it off sends false", async () => {
    const h = harness();
    const st = (await mount(h, { hermesTerminal: true, hermesReadAloud: false })) as ReturnType<typeof fakeStore>;
    expect($("toggle-terminal")?.checked).toBe(true);
    await click($("toggle-terminal"));
    expect(st.setHermesTerminal).toHaveBeenCalledWith(false);
  });

  it("terminal: the real store's setter updates the same AppState field Settings reads", async () => {
    const h = harness();
    store.setState({ hermesTerminal: false });
    const spy = vi.spyOn(store, "setHermesTerminal");
    await mount(h, { hermesTerminal: false, hermesReadAloud: false }, store);
    await click($("toggle-terminal"));
    expect(spy).toHaveBeenCalledWith(true);
    expect(store.state.hermesTerminal).toBe(true);
    store.setState({ hermesTerminal: false });
  });

  // ---------- Daemons ----------
  it("daemons: with none created the switch is disabled and says nothing runs until one is created", async () => {
    const h = harness({ daemons: { allPaused: false, daemons: [] } });
    await mount(h);
    expect(h.daemons.refresh).toHaveBeenCalled();
    expect($("toggle-daemons")?.disabled).toBe(true);
    expect($("toggle-daemons")?.checked).toBe(false);
    expect($("reason-daemons")?.textContent).toContain("Nothing runs until you create one");
  });

  it("daemons: with daemons created, on = not paused; turning it off calls the same 'pause all' as the Daemons card", async () => {
    const h = harness({ daemons: daemonView(false, 2) });
    await mount(h);
    expect($("toggle-daemons")?.checked).toBe(true);
    await click($("toggle-daemons"));
    expect(h.daemons.setAllPaused).toHaveBeenCalledWith(true);
    expect($("toggle-daemons")?.checked).toBe(false);
  });

  it("daemons: when all are paused, turning it on resumes through the same call", async () => {
    const h = harness({ daemons: daemonView(true, 1) });
    await mount(h);
    expect($("toggle-daemons")?.checked).toBe(false);
    await click($("toggle-daemons"));
    expect(h.daemons.setAllPaused).toHaveBeenCalledWith(false);
  });

  it("daemons: in the web preview the switch is disabled (daemons need the desktop app)", async () => {
    const h = harness({ daemonsAvailable: false });
    await mount(h);
    expect(h.daemons.refresh).not.toHaveBeenCalled();
    expect($("toggle-daemons")?.disabled).toBe(true);
    expect($("reason-daemons")?.textContent).toContain("desktop app");
  });

  // ---------- Learn from my sessions ----------
  it("learn: reads trajectories_settings_get through the federated-rounds bridge, off by default", async () => {
    const h = harness();
    await mount(h);
    expect(h.fl.status).toHaveBeenCalledTimes(1);
    expect($("toggle-learn")?.checked).toBe(false);
    expect($("toggle-learn")?.disabled).toBe(false);
  });

  it("learn: turning it on writes setTrajectoryConsent(true, false), the same call as Train › Training data", async () => {
    const h = harness();
    await mount(h);
    await click($("toggle-learn"));
    expect(h.fl.set).toHaveBeenCalledWith(true, false);
    expect($("toggle-learn")?.checked).toBe(true);
  });

  it("learn: turning it off keeps recorded sessions (deleting them is a second-depth choice)", async () => {
    const h = harness({ learn: true });
    await mount(h);
    expect($("toggle-learn")?.checked).toBe(true);
    await click($("toggle-learn"));
    expect(h.fl.set).toHaveBeenCalledWith(false, false);
  });

  it("learn: with the sim bridge (web preview) it is disabled with the bridge's own reason", async () => {
    const h = harness();
    h.io.fl = simFlRounds({} as SimHost);
    await mount(h);
    expect($("toggle-learn")?.disabled).toBe(true);
    expect($("toggle-learn")?.checked).toBe(false);
    expect($("reason-learn")?.textContent).toContain("desktop app");
  });

  // ---------- Voice ----------
  it("voice: reads hermesReadAloud (off by default) and writes through store.setHermesReadAloud (same call as Settings › App › persona)", async () => {
    const h = harness();
    const st = (await mount(h, { hermesTerminal: false, hermesReadAloud: false })) as ReturnType<typeof fakeStore>;
    expect($("toggle-voice")?.checked).toBe(false);
    await click($("toggle-voice"));
    expect(st.setHermesReadAloud).toHaveBeenCalledWith(true);
  });

  it("voice: the real store's setter flips the same persisted field, and off sends false", async () => {
    const h = harness();
    store.setState({ hermesReadAloud: true });
    const spy = vi.spyOn(store, "setHermesReadAloud");
    await mount(h, { hermesTerminal: false, hermesReadAloud: true }, store);
    expect($("toggle-voice")?.checked).toBe(true);
    await click($("toggle-voice"));
    expect(spy).toHaveBeenCalledWith(false);
    expect(store.state.hermesReadAloud).toBe(false);
  });

  // ---------- saved vs. effective (Codex P2): web search and learn apply when Hermes restarts ----------
  it("web search: after turning it off, the row says it is saved and takes effect when Hermes restarts", async () => {
    const h = harness({ web: webStatus({ searchEnabled: true }) });
    await mount(h);
    expect($("pending-web-search")).toBeNull();
    await click($("toggle-web-search"));
    expect($("toggle-web-search")?.checked).toBe(false);
    const pending = $("pending-web-search")?.textContent ?? "";
    expect(pending).toContain("Saved. Takes effect when Hermes restarts");
    expect(pending).toContain("still has web search on");
  });

  it("learn: after turning it off, the row says it is saved and takes effect when Hermes restarts", async () => {
    const h = harness({ learn: true });
    await mount(h);
    expect($("pending-learn")).toBeNull();
    await click($("toggle-learn"));
    expect($("toggle-learn")?.checked).toBe(false);
    const pending = $("pending-learn")?.textContent ?? "";
    expect(pending).toContain("Saved. Takes effect when Hermes restarts");
    expect(pending).toContain("still records sessions");
  });

  it("pending restart: switching back to the value Hermes started with clears the notice", async () => {
    const h = harness({ web: webStatus({ searchEnabled: true }) });
    await mount(h);
    await click($("toggle-web-search"));
    expect($("pending-web-search")).toBeTruthy();
    await click($("toggle-web-search"));
    expect($("pending-web-search")).toBeNull();
  });

  it("pending restart: no notice when core says the change applies now", async () => {
    const h = harness({ learn: true, learnAppliesOnRestart: false });
    await mount(h);
    await click($("toggle-learn"));
    expect($("pending-learn")).toBeNull();
  });

  it("pending restart: without a restart action there is no restart button (none is invented)", async () => {
    const h = harness({ learn: true });
    await mount(h);
    await click($("toggle-learn"));
    expect($("restart-learn")).toBeNull();
  });

  it("pending restart: the existing Hermes restart (stop + start) clears both notices", async () => {
    const h = harness({ web: webStatus({ searchEnabled: true }), learn: true, withRestart: true });
    await mount(h);
    await click($("toggle-web-search"));
    await click($("toggle-learn"));
    expect($("restart-learn")).toBeTruthy();
    await click($("restart-web-search"));
    expect(h.restart).toHaveBeenCalledTimes(1);
    expect($("pending-web-search")).toBeNull();
    expect($("pending-learn")).toBeNull();
  });

  it("restart: stop fails, so both notices stay and the error shows with a retry", async () => {
    const h = harness({ web: webStatus({ searchEnabled: true }), learn: true, withRestart: true });
    h.restart.mockResolvedValueOnce({ ok: false, stage: "stop", error: "the sidecar did not exit" });
    await mount(h);
    await click($("toggle-web-search"));
    await click($("toggle-learn"));
    await click($("restart-web-search"));
    expect($("pending-web-search")?.textContent).toContain("Saved. Takes effect when Hermes restarts");
    expect($("pending-learn")?.textContent).toContain("Saved. Takes effect when Hermes restarts");
    const err = $("restart-error-web-search")?.textContent ?? "";
    expect(err).toContain("could not stop");
    expect(err).toContain("the sidecar did not exit");
    expect(err).toContain("still running with its previous settings");
    expect($("restart-web-search")?.textContent).toContain("Retry");
    // Retry, now confirmed: the notices clear.
    await click($("restart-web-search"));
    expect(h.restart).toHaveBeenCalledTimes(2);
    expect($("pending-web-search")).toBeNull();
    expect($("pending-learn")).toBeNull();
    expect($("restart-error-web-search")).toBeNull();
  });

  it("restart: start fails, so the notice stays and says Hermes is stopped, with a retry", async () => {
    const h = harness({ learn: true, withRestart: true });
    h.restart.mockResolvedValueOnce({ ok: false, stage: "start", error: "llama-server is not running" });
    await mount(h);
    await click($("toggle-learn"));
    await click($("restart-learn"));
    expect($("pending-learn")).toBeTruthy();
    const err = $("restart-error-learn")?.textContent ?? "";
    expect(err).toContain("could not start again");
    expect(err).toContain("llama-server is not running");
    expect(err).toContain("Hermes is stopped");
    expect($("restart-learn")?.textContent).toContain("Retry");
  });

  it("restart: a thrown error is a failure too, never a confirmed restart", async () => {
    const h = harness({ learn: true, withRestart: true });
    h.restart.mockRejectedValueOnce(new Error("bridge down"));
    await mount(h);
    await click($("toggle-learn"));
    await click($("restart-learn"));
    expect($("pending-learn")).toBeTruthy();
    expect($("restart-error-learn")?.textContent).toContain("bridge down");
  });

  // ---------- pending restart survives remounts (Codex P2 at 6be691a4) ----------
  const remount = async (h: Harness) => {
    act(() => root?.unmount());
    host?.remove();
    await mount(h);
  };

  it("remount: after turning web search off, leaving and coming back keeps the restart notice", async () => {
    const h = harness({ web: webStatus({ searchEnabled: true }), withRestart: true });
    await mount(h);
    await click($("toggle-web-search"));
    expect($("pending-web-search")).toBeTruthy();
    await remount(h);
    expect($("toggle-web-search")?.checked).toBe(false);
    expect($("pending-web-search")?.textContent).toContain("still has web search on");
    expect($("restart-web-search")).toBeTruthy();
  });

  it("remount: after turning learn off, leaving and coming back keeps the restart notice", async () => {
    const h = harness({ learn: true, withRestart: true });
    await mount(h);
    await click($("toggle-learn"));
    await remount(h);
    expect($("toggle-learn")?.checked).toBe(false);
    expect($("pending-learn")?.textContent).toContain("still records sessions");
    expect($("restart-learn")).toBeTruthy();
  });

  it("remount: a confirmed restart clears the notice, and it stays cleared after coming back", async () => {
    const h = harness({ web: webStatus({ searchEnabled: true }), learn: true, withRestart: true });
    await mount(h);
    await click($("toggle-web-search"));
    await click($("toggle-learn"));
    await remount(h);
    await click($("restart-learn"));
    expect($("pending-web-search")).toBeNull();
    expect($("pending-learn")).toBeNull();
    await remount(h);
    expect($("pending-web-search")).toBeNull();
    expect($("pending-learn")).toBeNull();
  });

  it("remount: a failed restart keeps the notice across remounts", async () => {
    const h = harness({ learn: true, withRestart: true });
    h.restart.mockResolvedValueOnce({ ok: false, stage: "start", error: "llama-server is not running" });
    await mount(h);
    await click($("toggle-learn"));
    await click($("restart-learn"));
    await remount(h);
    expect($("pending-learn")).toBeTruthy();
  });

  it("a change saved in Settings › Web search before Controls ever opened still shows as pending", async () => {
    const h = harness({ web: webStatus({ searchEnabled: true }) });
    host = document.createElement("div");
    document.body.appendChild(host);
    root = createRoot(host);
    await act(async () => {
      root?.render(<WebSearchSettings io={h.io.web} />);
    });
    await flush();
    await click($("web-search-toggle"));
    await click($("web-save"));
    expect(h.web.current.settings.searchEnabled).toBe(false);
    await remount(h);
    expect($("pending-web-search")?.textContent).toContain("still has web search on");
  });

  it("a change saved in Train › Training data before Controls ever opened still shows as pending", async () => {
    const h = harness({ learn: true });
    host = document.createElement("div");
    document.body.appendChild(host);
    root = createRoot(host);
    const fl = { ...h.io.fl, overview: vi.fn(), buildTrainingSet: vi.fn() } as unknown as Parameters<typeof TrajectoryConsent>[0]["fl"];
    await act(async () => {
      root?.render(<TrajectoryConsent fl={fl} toast={() => {}} />);
    });
    await flush();
    await click($("fl-traj-toggle"));
    expect(h.fl.set).toHaveBeenCalledWith(false, false);
    await remount(h);
    expect($("pending-learn")?.textContent).toContain("still records sessions");
  });

  // ---------- field-specific web writes (#343 race) ----------
  it("web search: re-reads the saved settings right before writing, so another form's change is kept", async () => {
    const h = harness({ web: webStatus({ browserEnabled: true }) });
    await mount(h);
    // Another writer (the Settings › Web search form mounted below) turns the browser off.
    h.web.current = { ...h.web.current, settings: { ...h.web.current.settings, browserEnabled: false, reader: "jina" } };
    await click($("toggle-web-search"));
    const cmds = h.calls.map((c) => c.cmd);
    expect(cmds.slice(-2)).toEqual(["hermes_web_settings_get", "hermes_web_settings_set"]);
    const written = (h.calls[h.calls.length - 1].args as { settings: HermesWebStatus["settings"] }).settings;
    expect(written).toEqual({ ...DEFAULT_WEB_SETTINGS, browserEnabled: false, reader: "jina", searchEnabled: true });
    expect(h.web.current.settings.browserEnabled).toBe(false);
  });

  it("web search: if the re-read fails, nothing is written and the error shows", async () => {
    const h = harness();
    await mount(h);
    const inv = h.io.web.invoke;
    h.io.web.invoke = (async (cmd: string, args?: Record<string, unknown>) => {
      if (cmd === "hermes_web_settings_get") throw new Error("core went away");
      return inv(cmd, args);
    }) as WebSettingsIo["invoke"];
    await click($("toggle-web-search"));
    expect(h.calls.some((c) => c.cmd === "hermes_web_settings_set")).toBe(false);
    expect($("reason-web-search")?.textContent).toContain("core went away");
  });

  it("mounted next to the Settings › Web search form: Controls turns search off, the form saves a browser change, search stays off", async () => {
    const h = harness({ web: webStatus({ searchEnabled: true }) });
    host = document.createElement("div");
    document.body.appendChild(host);
    root = createRoot(host);
    await act(async () => {
      root?.render(
        <>
          <ControlsPanel store={fakeStore()} s={{ hermesTerminal: false, hermesReadAloud: false }} io={h.io} />
          <WebSearchSettings io={h.io.web} />
        </>,
      );
    });
    await flush();
    // Both loaded with search on.
    expect($("toggle-web-search")?.checked).toBe(true);
    expect($("web-search-toggle")?.checked).toBe(true);
    // The top toggle turns search off.
    await click($("toggle-web-search"));
    expect(h.web.current.settings.searchEnabled).toBe(false);
    // The lower form (still showing its loaded draft) turns the browser on and saves.
    await click($("browser-toggle"));
    await click($("web-save"));
    expect(h.web.current.settings).toEqual({ ...DEFAULT_WEB_SETTINGS, browserEnabled: true, searchEnabled: false });
    expect($("web-search-toggle")?.checked).toBe(false);
  });

  it("uses HIC wording, never HITL", async () => {
    await mount(harness());
    expect(host?.textContent ?? "").not.toMatch(/HITL/);
  });
});
