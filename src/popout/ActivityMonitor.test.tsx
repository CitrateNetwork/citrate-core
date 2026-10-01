// HUP-S7.6 (US-7.4) — the Activity monitor pop-out renders a snapshot: model + tier, the context
// meter, the "why am I waiting" line, steps and tool calls, elapsed time, spend, and a Stop button
// that is always visible. Unknown values say "unknown"; nothing is invented.
import { describe, it, expect, vi, afterEach } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { ActivityMonitor } from "./ActivityMonitor";
import { buildMonitorSnapshot, type MonitorInputs } from "./monitorSnapshot";
import { IDLE_ACTIVITY } from "../shell/slices/turnActivity";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const inputs: MonitorInputs = {
  activity: IDLE_ACTIVITY,
  providerKind: "local",
  providerLabel: "local model · llama-server · agentic",
  modelLabel: "Gemma 3 4B",
  modelId: "local:gemma",
  tier: "T2",
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

describe("HUP-S7.6 Activity monitor", () => {
  it("idle: model, tier, honest unknowns, Stop visible but disabled", () => {
    const el = render(<ActivityMonitor snapshot={buildMonitorSnapshot(inputs)} now={10_000} onStop={vi.fn()} />);
    expect(byTestId(el, "mon-model")?.textContent).toContain("Gemma 3 4B");
    expect(byTestId(el, "mon-tier")?.textContent).toContain("T2");
    expect(byTestId(el, "mon-ctx")?.textContent).toMatch(/unknown/i);
    expect(byTestId(el, "mon-ctx")?.textContent).toContain("8,192");
    expect(byTestId(el, "mon-why")?.textContent).toMatch(/idle/i);
    expect(byTestId(el, "mon-spend")?.textContent).toMatch(/0/);
    const stop = byTestId(el, "mon-stop") as HTMLButtonElement;
    expect(stop).not.toBeNull();
    expect(stop.disabled).toBe(true);
  });

  it("an unknown tier and gateway spend say unknown", () => {
    const snap = buildMonitorSnapshot({ ...inputs, tier: null, providerKind: "agent", providerLabel: "provider · x · agentic" });
    const el = render(<ActivityMonitor snapshot={snap} now={10_000} onStop={vi.fn()} />);
    expect(byTestId(el, "mon-tier")?.textContent).toMatch(/unknown/i);
    expect(byTestId(el, "mon-spend")?.textContent).toMatch(/unknown/i);
    expect(byTestId(el, "mon-ctx")?.textContent).not.toContain("8,192");
  });

  it("running: why-am-I-waiting, step, tool rows, elapsed, and Stop calls back", () => {
    const onStop = vi.fn();
    const activity = {
      ...IDLE_ACTIVITY,
      state: "running" as const,
      providerKind: "sidecar",
      providerLabel: "Hermes (sidecar loop · preview)",
      phase: "tool" as const,
      currentTool: "node_status",
      step: 2,
      startedAt: 1_000,
      tools: [
        { id: "c0", name: "memory_search", state: "done" as const, startedAt: 1_500, endedAt: 2_000 },
        { id: "c1", name: "node_status", state: "running" as const, startedAt: 3_000, endedAt: null },
      ],
    };
    const el = render(<ActivityMonitor snapshot={buildMonitorSnapshot({ ...inputs, activity })} now={62_000} onStop={onStop} />);
    expect(byTestId(el, "mon-why")?.textContent).toContain("node_status");
    expect(byTestId(el, "mon-step")?.textContent).toContain("2");
    expect(byTestId(el, "mon-elapsed")?.textContent).toBe("1m 01s");
    const rows = el.querySelectorAll('[data-testid="mon-tool-row"]');
    expect(rows.length).toBe(2);
    expect(rows[1].textContent).toMatch(/running/);
    const stop = byTestId(el, "mon-stop") as HTMLButtonElement;
    expect(stop.disabled).toBe(false);
    act(() => stop.click());
    expect(onStop).toHaveBeenCalledTimes(1);
  });

  it("stopping: the button says so and cannot be pressed twice", () => {
    const activity = { ...IDLE_ACTIVITY, state: "stopping" as const, providerKind: "local", providerLabel: "l", phase: "thinking" as const, startedAt: 0 };
    const el = render(<ActivityMonitor snapshot={buildMonitorSnapshot({ ...inputs, activity })} now={1000} onStop={vi.fn()} />);
    const stop = byTestId(el, "mon-stop") as HTMLButtonElement;
    expect(stop.disabled).toBe(true);
    expect(stop.textContent).toMatch(/stopping/i);
  });

  it("styles with the register tokens, not hard-coded text colours", () => {
    const el = render(<ActivityMonitor snapshot={buildMonitorSnapshot(inputs)} now={10_000} onStop={vi.fn()} />);
    const rootEl = byTestId(el, "activity-monitor");
    expect(rootEl?.getAttribute("data-register")).toBe("instrument");
    expect(rootEl?.style.color).toBe("var(--tx-1)");
    expect(rootEl?.style.background).toBe("var(--srf-0)");
  });

  it("HUP-S1.9: lists each worker process with its restarts and how it last ended", () => {
    const el = render(
      <ActivityMonitor
        snapshot={buildMonitorSnapshot({
          ...inputs,
          workers: [
            { kind: "toolchain", state: "running", healthy: true, pid: 7, restarts: 1, lastExit: "killed by signal 9", lastError: null, runningSinceMs: 1, detail: null },
            { kind: "browser", state: "not_built", healthy: null, pid: null, restarts: null, lastExit: null, lastError: null, runningSinceMs: null, detail: "arrives with HUP-S5.1" },
          ],
        })}
        now={10_000}
        onStop={vi.fn()}
      />,
    );
    const rows = Array.from(el.querySelectorAll('[data-testid="mon-worker-row"]')).map((r) => r.textContent);
    expect(rows).toHaveLength(2);
    expect(rows[0]).toContain("toolchain");
    expect(rows[0]).toContain("restarted 1 time (last exit: killed by signal 9)");
    expect(rows[1]).toContain("not built yet");
  });

  it("HUP-S1.9: unread workers say so instead of showing an empty list", () => {
    const el = render(<ActivityMonitor snapshot={buildMonitorSnapshot(inputs)} now={10_000} onStop={vi.fn()} />);
    expect(el.querySelectorAll('[data-testid="mon-worker-row"]')).toHaveLength(0);
    expect(byTestId(el, "mon-workers")?.textContent).toMatch(/could not be read/i);
  });
});
