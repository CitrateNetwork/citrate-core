// HUP-S10.6 — accessibility of the pop-out framework and the Activity monitor (US-7.4).
// axe-core over every state the pop-out can show, plus the structure a screen-reader or keyboard
// member relies on: a main landmark with a level-one heading, a window title, a labelled tool-call
// list, an accessible Stop button that is first in the tab order, and polite live regions.
import { describe, it, expect, vi, afterEach } from "vitest";
import { act } from "react";
import { ActivityMonitor } from "../popout/ActivityMonitor";
import { PopoutRoot } from "../popout/PopoutRoot";
import { buildMonitorSnapshot, type MonitorInputs } from "../popout/monitorSnapshot";
import type { BridgeTransport } from "../popout/bridge";
import { IDLE_ACTIVITY } from "../shell/slices/turnActivity";
import { axeFindings, cleanupMounted, mount } from "./axeHarness";

afterEach(() => cleanupMounted());

const inputs: MonitorInputs = {
  activity: IDLE_ACTIVITY,
  providerKind: "local",
  providerLabel: "local model · llama-server · agentic",
  modelLabel: "Gemma 4 E4B",
  modelId: "local:gemma",
  tier: "T0",
  localCtxTokens: 8192,
  now: 10_000,
};
const runningActivity = {
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
const snapshots = {
  idle: buildMonitorSnapshot(inputs),
  running: buildMonitorSnapshot({ ...inputs, activity: runningActivity }),
  stopping: buildMonitorSnapshot({ ...inputs, activity: { ...runningActivity, state: "stopping" as const } }),
  unknowns: buildMonitorSnapshot({ ...inputs, tier: null, providerKind: "agent", providerLabel: "provider · x · agentic" }),
};

describe("HUP-S10.6 Activity monitor: axe-core", () => {
  for (const [name, snap] of Object.entries(snapshots)) {
    it(`${name}: no axe violations`, async () => {
      const m = mount(<ActivityMonitor snapshot={snap} now={62_000} onStop={vi.fn()} />);
      expect(await axeFindings(m.host)).toEqual([]);
    });
  }
});

describe("HUP-S10.6 Activity monitor: structure and keyboard", () => {
  it("is a main landmark named by its level-one heading", () => {
    const m = mount(<ActivityMonitor snapshot={snapshots.idle} now={10_000} onStop={vi.fn()} />);
    const main = m.host.querySelector("main");
    expect(main).not.toBeNull();
    const h1 = m.host.querySelector("h1");
    expect(h1?.textContent).toBe("Activity monitor");
    expect(main?.getAttribute("aria-labelledby")).toBe(h1?.id);
  });

  it("Stop is the first control in the tab order and has a name that says what it does", () => {
    const m = mount(<ActivityMonitor snapshot={snapshots.running} now={62_000} onStop={vi.fn()} />);
    const focusables = m.host.querySelectorAll<HTMLElement>("button, [href], input, select, textarea, [tabindex]:not([tabindex='-1'])");
    expect(focusables[0]?.getAttribute("data-testid")).toBe("mon-stop");
    expect(focusables[0]?.getAttribute("aria-label")).toMatch(/stop the running turn/i);
  });

  it("the Stop button's name still says why it cannot be pressed when nothing runs", () => {
    const m = mount(<ActivityMonitor snapshot={snapshots.idle} now={10_000} onStop={vi.fn()} />);
    const stop = m.host.querySelector('[data-testid="mon-stop"]');
    expect(stop?.getAttribute("aria-label")).toMatch(/nothing is running/i);
  });

  it("the tool-call list is named and each row's state is text, not colour alone", () => {
    const m = mount(<ActivityMonitor snapshot={snapshots.running} now={62_000} onStop={vi.fn()} />);
    const list = m.host.querySelector("ul");
    const labelId = list?.getAttribute("aria-labelledby");
    expect(labelId).toBeTruthy();
    expect(document.getElementById(labelId ?? "")?.textContent).toMatch(/tool calls/i);
    const rows = m.host.querySelectorAll('[data-testid="mon-tool-row"]');
    expect(rows[1]?.textContent).toMatch(/running/);
  });

  it("each fact row is a group named by its label", () => {
    const m = mount(<ActivityMonitor snapshot={snapshots.idle} now={10_000} onStop={vi.fn()} />);
    const model = m.host.querySelector('[data-testid="mon-model"]')?.closest('[role="group"]');
    const id = model?.getAttribute("aria-labelledby");
    expect(id).toBeTruthy();
    expect(document.getElementById(id ?? "")?.textContent).toBe("Model");
  });

  it("the why-am-I-waiting line is a polite status; the ticking clock is not announced every second", () => {
    const m = mount(<ActivityMonitor snapshot={snapshots.running} now={62_000} onStop={vi.fn()} />);
    const why = m.host.querySelector('[data-testid="mon-why"]');
    expect(why?.getAttribute("role")).toBe("status");
    expect(why?.getAttribute("aria-live")).toBe("polite");
    const elapsed = m.host.querySelector('[data-testid="mon-elapsed"]');
    expect(elapsed?.closest("[aria-live]")).toBeNull();
  });

  it("decorative separators and the clock carry no motion of their own", () => {
    const m = mount(<ActivityMonitor snapshot={snapshots.running} now={62_000} onStop={vi.fn()} />);
    for (const el of m.host.querySelectorAll<HTMLElement>("*")) {
      expect(el.style.animation, el.outerHTML.slice(0, 80)).toBe("");
      expect(el.style.transition, el.outerHTML.slice(0, 80)).toBe("");
    }
  });
});

function fake() {
  let handler: ((p: unknown) => void) | null = null;
  const t: BridgeTransport = {
    async send() {},
    async listen(h) {
      handler = h;
      return () => {
        handler = null;
      };
    },
  };
  return { t, deliver: (p: unknown) => handler?.(p) };
}
async function settle() {
  await act(async () => {
    await new Promise((r) => setTimeout(r, 0));
  });
}

describe("HUP-S10.6 pop-out framework (PopoutRoot)", () => {
  it("names the window after the pop-out", async () => {
    const f = fake();
    mount(<PopoutRoot kind="monitor" transport={async () => f.t} />);
    await settle();
    expect(document.title).toBe("Activity monitor");
  });

  it("waiting: a polite status inside a main landmark, no axe violations", async () => {
    const f = fake();
    const m = mount(<PopoutRoot kind="monitor" transport={async () => f.t} />);
    await settle();
    const status = m.host.querySelector('[role="status"]');
    expect(status?.textContent).toMatch(/waiting for the main window/i);
    expect(status?.closest("main")).not.toBeNull();
    expect(await axeFindings(m.host)).toEqual([]);
  });

  it("failed: an alert inside a main landmark, no axe violations", async () => {
    const m = mount(<PopoutRoot kind="monitor" transport={async () => { throw new Error("no ipc"); }} />);
    await settle();
    const alert = m.host.querySelector('[role="alert"]');
    expect(alert?.textContent).toMatch(/could not connect/i);
    expect(alert?.closest("main")).not.toBeNull();
    expect(await axeFindings(m.host)).toEqual([]);
  });

  it("a kind without a view: a main landmark with a heading, no axe violations", async () => {
    const f = fake();
    const m = mount(<PopoutRoot kind="browser" transport={async () => f.t} />);
    await settle();
    expect(m.host.querySelector("main h1")?.textContent).toBe("Browser");
    expect(m.host.textContent).toMatch(/not built yet/i);
    expect(document.title).toBe("Browser");
    expect(await axeFindings(m.host)).toEqual([]);
  });

  it("snapshot delivered: the monitor renders with no axe violations", async () => {
    const f = fake();
    const m = mount(<PopoutRoot kind="monitor" transport={async () => f.t} />);
    await settle();
    await act(async () => {
      f.deliver({ v: 1, type: "monitor.snapshot", snapshot: snapshots.running });
    });
    expect(m.host.querySelector('[data-testid="activity-monitor"]')).not.toBeNull();
    expect(await axeFindings(m.host)).toEqual([]);
  });
});
