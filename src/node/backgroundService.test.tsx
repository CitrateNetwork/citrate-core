// V60 SVC-2 — the onboarding step (on by default, exact opt-out sentence) and Settings › Node.
import { afterEach, describe, expect, it } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { renderToStaticMarkup } from "react-dom/server";
import { BackgroundServicePanel } from "./BackgroundServicePanel";
import { svcLine, type PowerView, type SvcIo, type SvcView } from "./svc";
import { SVC_OPT_OUT_COPY } from "../shell/svcCopy";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const POLICY = { trainOnBattery: false, pauseBelowPct: 30, nightly: null };

function view(over: Partial<SvcView> = {}): SvcView {
  return {
    supported: true,
    enabled: null,
    unitInstalled: false,
    attached: false,
    inProcessReason: "the background service is not running",
    service: null,
    runsLoggedOut: null,
    notes: [],
    optOutCopy: SVC_OPT_OUT_COPY,
    power: POLICY,
    ...over,
  };
}

/** A scripted IO that records each command (test-only; the app's IO is `desktopSvcIo`). */
function scripted(first: SvcView) {
  const calls: [string, Record<string, unknown>][] = [];
  let current = first;
  const io: SvcIo = {
    mode: "tauri",
    invoke: async <T,>(cmd: string, args: Record<string, unknown>) => {
      calls.push([cmd, args]);
      if (cmd === "svc_set_enabled") {
        current = { ...current, enabled: args.enabled as boolean, unitInstalled: args.enabled as boolean };
      }
      if (cmd === "svc_power_set") current = { ...current, power: args.policy as SvcView["power"] };
      if (cmd === "svc_power_status") {
        const p: PowerView = { state: { onAc: false, batteryPct: 80, hasBattery: true }, training: { allowed: false, reason: "on battery; training runs only on AC power" }, source: "pmset -g batt" };
        return p as T;
      }
      return current as T;
    },
  };
  return { io, calls };
}

let host: HTMLDivElement | null = null;
let root: Root | null = null;
afterEach(() => {
  act(() => root?.unmount());
  host?.remove();
  host = null;
  root = null;
});

async function mount(io: SvcIo, variant: "setup" | "settings") {
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  await act(async () => {
    root?.render(<BackgroundServicePanel io={io} variant={variant} />);
  });
  // Let the status read and the default apply settle.
  await act(async () => {
    await new Promise((r) => setTimeout(r, 0));
  });
}

describe("background service step", () => {
  it("opt_out_copy_exact: the sentence is the owner's, word for word", () => {
    expect(SVC_OPT_OUT_COPY).toBe("If the app is closed and the service is off, your node won't produce blocks.");
  });

  it("is on by default at setup: an undecided member gets the service turned on", async () => {
    const { io, calls } = scripted(view());
    await mount(io, "setup");
    expect(calls.map((c) => c[0])).toEqual(["svc_status", "svc_set_enabled"]);
    expect(calls[1][1]).toEqual({ enabled: true });
    const toggle = host?.querySelector<HTMLInputElement>('[data-testid="svc-toggle"]');
    expect(toggle?.checked).toBe(true);
    expect(host?.textContent).not.toContain(SVC_OPT_OUT_COPY);
  });

  it("does not override a member who already chose off", async () => {
    const { io, calls } = scripted(view({ enabled: false }));
    await mount(io, "setup");
    expect(calls.map((c) => c[0])).toEqual(["svc_status"]);
    expect(host?.querySelector('[data-testid="svc-opt-out"]')?.textContent).toBe(SVC_OPT_OUT_COPY);
  });

  it("turning it off shows the exact sentence and starts the node in the app again", async () => {
    const { io, calls } = scripted(view({ enabled: true, unitInstalled: true }));
    await mount(io, "setup");
    const toggle = host?.querySelector<HTMLInputElement>('[data-testid="svc-toggle"]');
    await act(async () => {
      toggle?.click();
    });
    await act(async () => {
      await new Promise((r) => setTimeout(r, 0));
    });
    expect(calls.map((c) => c[0])).toEqual(["svc_status", "svc_set_enabled", "node_start"]);
    expect(calls[1][1]).toEqual({ enabled: false });
    expect(host?.querySelector('[data-testid="svc-opt-out"]')?.textContent).toBe(SVC_OPT_OUT_COPY);
  });

  it("lists what runs in the background and the power rules", async () => {
    const { io } = scripted(view({ enabled: true }));
    await mount(io, "setup");
    const btn = Array.from(host?.querySelectorAll("button") ?? []).find((b) => b.textContent === "What runs in the background");
    await act(async () => {
      btn?.click();
    });
    const list = host?.querySelector('[data-testid="svc-what-runs"]')?.textContent ?? "";
    expect(list).toContain("Your node");
    expect(list).toContain("IPFS");
    expect(list).toContain("only on AC power by default");
  });

  it("Settings › Node shows the power rules, defaulting to AC only, and saves a change", async () => {
    const { io, calls } = scripted(view({ enabled: true, unitInstalled: true }));
    await mount(io, "settings");
    // Settings never applies the default by itself.
    expect(calls.map((c) => c[0])).toEqual(["svc_status", "svc_power_status"]);
    const battery = host?.querySelector<HTMLInputElement>('[data-testid="svc-train-battery"]');
    expect(battery?.checked).toBe(false);
    expect(host?.querySelector('[data-testid="svc-power-now"]')?.textContent).toContain("training now: paused");
    await act(async () => {
      battery?.click();
    });
    await act(async () => {
      await new Promise((r) => setTimeout(r, 0));
    });
    const save = calls.find((c) => c[0] === "svc_power_set");
    expect(save?.[1]).toEqual({ policy: { trainOnBattery: true, pauseBelowPct: 30, nightly: null } });
  });

  it("the web preview says the service is desktop-only and calls nothing", () => {
    const calls: string[] = [];
    const io: SvcIo = { mode: "sim", invoke: async (c) => { calls.push(c); return undefined as never; } };
    const html = renderToStaticMarkup(<BackgroundServicePanel io={io} variant="setup" />);
    expect(html).toContain("desktop app");
    expect(calls).toEqual([]);
  });

  it("the status line says what is true", () => {
    expect(svcLine(view({ supported: false }))).toContain("Not available");
    expect(svcLine(view({ enabled: false }))).toContain("only while the app is open");
    expect(svcLine(view({ enabled: true, attached: false }))).toContain("not answering");
    const svc = { node: { state: "running", height: 5, peers: 2, miningArmed: true }, ipfsRunning: true, nodeHold: null, drained: false, coinbase: null };
    expect(svcLine(view({ enabled: true, attached: true, service: svc }))).toBe("Node running in background · producing");
    expect(svcLine(view({ enabled: true, attached: true, service: { ...svc, nodeHold: "keyring unavailable" } }))).toContain("waiting to start the node: keyring unavailable");
    expect(svcLine(view({ enabled: true, attached: true, service: { ...svc, drained: true } }))).toContain("paused for an update");
  });
});
