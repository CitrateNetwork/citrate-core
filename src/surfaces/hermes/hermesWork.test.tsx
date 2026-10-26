// K2 (0.5.2): Hermes › Work. Agents, skills, daemons, "Awaiting your review" and the run log in
// one tab, composed from the existing Agent surface and Daemons card. The run log reads the daemon
// run log (daemon_runs_between) and links to the full history in Controls › 3 · Records.
import { describe, it, expect, vi, afterEach, beforeEach } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { DaemonRunLog, WORK_RUN_LOG_LIMIT } from "../../daemons/DaemonRunLog";
import { HermesWork } from "./HermesWork";
import { recordsSlice } from "./ControlsRecords";
import { store } from "../../shell/store";
import { freshState } from "../../shell/state";
import type { DaemonRunEntry, DaemonRunLog as RunLog } from "../../daemons/api";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let root: Root | null = null;
let host: HTMLDivElement | null = null;
afterEach(() => {
  act(() => root?.unmount());
  host?.remove();
  root = null;
  host = null;
});
beforeEach(() => {
  recordsSlice.set({ view: "approvals", daemonId: null });
  store.setState({ route: "hermes/work" });
});

const $ = (id: string) => host?.querySelector(`[data-testid="${id}"]`) as HTMLElement | null;
const $$ = (id: string) => Array.from(host?.querySelectorAll(`[data-testid="${id}"]`) ?? []) as HTMLElement[];
const flush = () => act(async () => await new Promise((r) => setTimeout(r, 0)));

async function render(el: React.ReactElement) {
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  await act(async () => {
    root?.render(el);
  });
  await flush();
}

const NOW = Date.UTC(2026, 9, 19, 15, 0, 0);
const run = (i: number): DaemonRunEntry => ({
  schema: 1,
  daemonId: "d1",
  runId: "r" + i,
  name: "Digest " + i,
  startedMs: NOW - (i + 1) * 60_000,
  endedMs: NOW - i * 60_000,
  tokens: 100 + i,
  tokenSource: "estimated",
  outcome: "answered",
});

describe("Hermes › Work run log", () => {
  it("shows the newest runs from daemon_runs_between, at most a handful, with no conversation content", async () => {
    const runsBetween = vi.fn(async (): Promise<RunLog> => ({ runs: Array.from({ length: 9 }, (_, i) => run(i)), unreadable: 0 }));
    await render(<DaemonRunLog io={{ runsBetween, now: () => NOW }} onFullHistory={() => {}} />);
    expect(runsBetween).toHaveBeenCalledTimes(1);
    const rows = $$("work-run-row");
    expect(rows.length).toBe(WORK_RUN_LOG_LIMIT);
    expect(rows[0].textContent).toContain("Digest 0");
    expect($("daemon-run-log")?.textContent).toContain("no conversation content");
  });

  it("is honest when empty, when the read fails, and outside the desktop app", async () => {
    await render(<DaemonRunLog io={{ runsBetween: vi.fn(async () => ({ runs: [], unreadable: 0 })), now: () => NOW }} onFullHistory={() => {}} />);
    expect($("daemon-run-log")?.textContent).toContain("No runs yet");
    act(() => root?.unmount());
    host?.remove();

    await render(<DaemonRunLog io={{ runsBetween: vi.fn(async () => Promise.reject(new Error("boom"))), now: () => NOW }} onFullHistory={() => {}} />);
    expect($("daemon-run-log")?.textContent).toContain("daemon_runs_between");
    expect($("daemon-run-log")?.textContent).toContain("boom");
    expect($("daemon-run-log")?.textContent).not.toContain("No runs yet");
    act(() => root?.unmount());
    host?.remove();

    await render(<DaemonRunLog io={{ runsBetween: null, now: () => NOW }} onFullHistory={() => {}} />);
    expect($("daemon-run-log")?.textContent).toContain("desktop app");
  });

  // Codex review of #364 at 4c75911: daemon_runs_between counts the log lines it could not read.
  // A log with only unreadable lines is not "no runs", and a partly readable one is not complete.
  it("a wholly unreadable log says how many lines could not be read, never \"No runs yet\"", async () => {
    await render(<DaemonRunLog io={{ runsBetween: vi.fn(async () => ({ runs: [], unreadable: 3 })), now: () => NOW }} onFullHistory={() => {}} />);
    expect($("daemon-run-log")?.textContent).not.toContain("No runs yet");
    expect($("work-runs-unreadable"), "the unreadable count is shown").toBeTruthy();
    expect($("work-runs-unreadable")?.textContent).toContain("3 lines");
    expect($$("work-run-row").length).toBe(0);
  });

  it("a partly readable log shows its runs and says it is incomplete", async () => {
    await render(<DaemonRunLog io={{ runsBetween: vi.fn(async () => ({ runs: [run(0)], unreadable: 1 })), now: () => NOW }} onFullHistory={() => {}} />);
    expect($$("work-run-row").length).toBe(1);
    expect($("work-runs-unreadable"), "the unreadable count is shown").toBeTruthy();
    expect($("work-runs-unreadable")?.textContent).toContain("1 line");
  });

  it("Full history opens Controls › 3 · Records on the run history", async () => {
    const onFullHistory = vi.fn();
    await render(<DaemonRunLog io={{ runsBetween: vi.fn(async () => ({ runs: [run(0)], unreadable: 0 })), now: () => NOW }} onFullHistory={onFullHistory} />);
    await act(async () => {
      $("run-log-full-history")?.click();
    });
    expect(onFullHistory).toHaveBeenCalledTimes(1);
  });
});

describe("Hermes › Work links into Controls › 3 · Records", () => {
  it("Approval records opens the approval records; Full history opens the run history", async () => {
    const s = { ...freshState("p1"), stage: "done" as const, route: "hermes/work" };
    await render(<HermesWork store={store} s={s} />);
    await act(async () => {
      $("approval-records-link")?.click();
    });
    expect(store.state.route).toBe("hermes/controls/records");
    expect(recordsSlice.get()).toEqual({ view: "approvals", daemonId: null });

    store.setState({ route: "hermes/work" });
    await act(async () => {
      $("run-log-full-history")?.click();
    });
    expect(store.state.route).toBe("hermes/controls/records");
    expect(recordsSlice.get()).toEqual({ view: "runs", daemonId: null });
  });
});

// Tripwire: every view of the daemon run log (useDaemonRuns) shows the unreadable-line count
// through the shared RunLogUnreadable, so a new view cannot draw a partial log as complete.
describe("tripwire: run log views show unreadable lines", () => {
  it("every src/ file that calls useDaemonRuns( also renders <RunLogUnreadable", async () => {
    const fs = await import("node:fs");
    const path = await import("node:path");
    const srcDir = path.resolve(__dirname, "../..");
    const offenders: string[] = [];
    let views = 0;
    const walk = (d: string) => {
      for (const e of fs.readdirSync(d, { withFileTypes: true })) {
        const p = path.join(d, e.name);
        if (e.isDirectory()) walk(p);
        else if (/\.tsx$/.test(e.name) && !/\.test\.tsx$/.test(e.name)) {
          const t = fs.readFileSync(p, "utf8");
          // Calls only (`useDaemonRuns(io)` / `useDaemonRuns(io, ...)`), not the hook's definition.
          const calls = (t.match(/useDaemonRuns\(io[,)]/g) ?? []).length;
          views += calls;
          if (calls > 0 && (t.match(/<RunLogUnreadable\b/g) ?? []).length < calls) offenders.push(path.relative(srcDir, p));
        }
      }
    };
    walk(srcDir);
    expect(views).toBeGreaterThanOrEqual(2);
    expect(offenders).toEqual([]);
  });
});
