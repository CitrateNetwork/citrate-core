// HUP-S2.2 (US-2.2 AC3) — the Activity monitor lists the turn's command runs (tool, status, exit
// code, duration, sandbox), and the snapshot carrying them passes the pop-out's validation.
import { describe, it, expect, vi } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { ActivityMonitor } from "./ActivityMonitor";
import { buildMonitorSnapshot, isMonitorSnapshot, type MonitorInputs } from "./monitorSnapshot";
import { IDLE_ACTIVITY, type RunRow } from "../shell/slices/turnActivity";

const rows: RunRow[] = [
  { callId: "c1", tool: "shell_run", status: "completed", summary: "forge exited with code 0", exitCode: 0, durationMs: 812, timedOut: false, sandbox: "macOS Seatbelt: no network", at: 5 },
  { callId: "t2", tool: "slither_scan", status: "timed_out", summary: "slither did not finish", exitCode: null, durationMs: 300000, timedOut: true, sandbox: null, at: 6 },
];
const inputs: MonitorInputs = {
  activity: { ...IDLE_ACTIVITY, runs: rows },
  providerKind: "sidecar",
  providerLabel: "Hermes",
  modelLabel: "Gemma",
  modelId: null,
  tier: "T2",
  localCtxTokens: null,
  now: 10,
};

describe("Activity monitor: command runs", () => {
  it("the snapshot carries the runs and validates", () => {
    const snap = buildMonitorSnapshot(inputs);
    expect(snap.turn.runs).toEqual(rows);
    expect(isMonitorSnapshot(snap)).toBe(true);
    expect(isMonitorSnapshot({ ...snap, turn: { ...snap.turn, runs: [{ callId: 1 }] } })).toBe(false);
    // A snapshot from a sender without runs still validates.
    const { runs: _r, ...older } = snap.turn;
    expect(isMonitorSnapshot({ ...snap, turn: older })).toBe(true);
  });

  it("lists each run with status, exit code, duration and sandbox", () => {
    const html = renderToStaticMarkup(<ActivityMonitor snapshot={buildMonitorSnapshot(inputs)} now={10} onStop={vi.fn()} />);
    expect((html.match(/data-testid="mon-run-row"/g) ?? []).length).toBe(2);
    expect(html).toContain("shell_run");
    expect(html).toContain("exit 0");
    expect(html).toContain("timed out");
    expect(html).toContain("macOS Seatbelt: no network");
    expect(html).toContain("no OS sandbox reported");
  });

  it("says when no command ran", () => {
    const html = renderToStaticMarkup(<ActivityMonitor snapshot={buildMonitorSnapshot({ ...inputs, activity: IDLE_ACTIVITY })} now={10} onStop={vi.fn()} />);
    expect(html).toContain("Command runs");
    expect(html).toContain("No commands ran this turn.");
  });
});
