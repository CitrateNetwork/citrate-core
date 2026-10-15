// HGW-W6 / W11 / W2: a command run's result carries how it was approved (auto-approve list or a
// card), the end of stderr when it failed, and the risky git changes the runtime undid after it.
// The Activity monitor shows each, and a snapshot from an older sender without them still validates.
import { describe, it, expect, vi } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { commandRunOf } from "./sidecarProvider";
import { ActivityMonitor } from "../popout/ActivityMonitor";
import { buildMonitorSnapshot, isMonitorSnapshot, type MonitorInputs } from "../popout/monitorSnapshot";
import { IDLE_ACTIVITY, type RunRow } from "../shell/slices/turnActivity";

const envelope = (run: Record<string, unknown>, status = "completed", summary = "npm exited with code 0") =>
  JSON.stringify({ status, summary, run: { exit_code: status === "completed" ? 0 : 1, duration_ms: 40, timed_out: false, sandbox: { summary: "macOS Seatbelt: no network" }, ...run } });

describe("command run details from the sidecar's result", () => {
  it("an auto-approved run names its rule", () => {
    const ev = commandRunOf("c1", "shell_run", "ok", envelope({ approval: { mode: "auto", rule: "npm-run", project: "/p" } }));
    expect(ev.approval).toBe("auto");
    expect(ev.autoRule).toBe("npm-run");
    expect(ev.failureTail).toBeUndefined();
    expect(ev.gitWarnings).toBeUndefined();
  });

  it("a carded run is the member's, with no rule", () => {
    const ev = commandRunOf("c1", "shell_run", "ok", envelope({ approval: { mode: "member" } }));
    expect(ev.approval).toBe("member");
    expect(ev.autoRule).toBeUndefined();
  });

  it("a failed run carries the end of stderr, else of stdout, and keeps the end when long", () => {
    const ev = commandRunOf("c1", "shell_run", "ok", envelope({ stderr_tail: "ReferenceError: greeet is not defined" }, "failed", "exited 1"));
    expect(ev.failureTail).toBe("ReferenceError: greeet is not defined");
    const out = commandRunOf("c1", "shell_run", "ok", envelope({ stderr_tail: "  ", stdout_tail: "1 test failed" }, "failed"));
    expect(out.failureTail).toBe("1 test failed");
    const long = "x".repeat(5000) + "THE CAUSE";
    const clipped = commandRunOf("c1", "shell_run", "ok", envelope({ stderr_tail: long }, "failed"));
    expect(clipped.failureTail?.endsWith("THE CAUSE")).toBe(true);
    expect(clipped.failureTail?.length).toBeLessThanOrEqual(2049);
  });

  it("risky git changes the runtime undid are listed; junk values are not", () => {
    const ev = commandRunOf("c1", "shell_run", "ok", envelope({ git_warnings: ["core.hooksPath=/tmp/h (reverted)", 7, null] }));
    expect(ev.gitWarnings).toEqual(["core.hooksPath=/tmp/h (reverted)"]);
    const odd = commandRunOf("c1", "shell_run", "ok", envelope({ approval: { mode: "sometimes" }, git_warnings: "no" }));
    expect(odd.approval).toBeUndefined();
    expect(odd.gitWarnings).toBeUndefined();
  });
});

describe("Activity monitor: run details", () => {
  const rows: RunRow[] = [
    { callId: "c1", tool: "shell_run", status: "completed", summary: "ok", exitCode: 0, durationMs: 5, timedOut: false, sandbox: "seatbelt", approval: "auto", autoRule: "npm-run", failureTail: null, gitWarnings: [], at: 1 },
    { callId: "c2", tool: "shell_run", status: "failed", summary: "exited 1", exitCode: 1, durationMs: 5, timedOut: false, sandbox: "seatbelt", approval: "member", autoRule: null, failureTail: "Error: missing lib/forge-std", gitWarnings: ["alias.x=!sh (reverted)"], at: 2 },
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

  it("labels auto runs, shows the failure tail and the undone git changes", () => {
    const html = renderToStaticMarkup(<ActivityMonitor snapshot={buildMonitorSnapshot(inputs)} now={10} onStop={vi.fn()} />);
    expect((html.match(/data-testid="mon-run-auto"/g) ?? []).length).toBe(1);
    expect(html).toContain("auto-approve list: npm-run");
    expect(html).toContain("Error: missing lib/forge-std");
    expect(html).toContain("undone after the run: alias.x=!sh (reverted)");
  });

  it("the snapshot validates with the details, rejects bad ones, and an older row still validates", () => {
    const snap = buildMonitorSnapshot(inputs);
    expect(isMonitorSnapshot(snap)).toBe(true);
    const bad = { ...snap, turn: { ...snap.turn, runs: [{ ...rows[0], approval: "always" }] } };
    expect(isMonitorSnapshot(bad)).toBe(false);
    const { approval: _a, autoRule: _r, failureTail: _f, gitWarnings: _g, ...older } = rows[0];
    expect(isMonitorSnapshot({ ...snap, turn: { ...snap.turn, runs: [older] } })).toBe(true);
  });
});
