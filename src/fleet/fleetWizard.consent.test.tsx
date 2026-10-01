// HUP-S8.2 (US-8.1) review — discovery consent is scoped to the mounted wizard: nothing turns
// discovery on without the member, and unmounting the wizard turns it off again.
import { describe, it, expect, afterEach } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { FleetWizard } from "./FleetWizard";
import type { FleetApi, FleetProbe } from "../bridge/tauri/fleet";
import { sampleReport } from "../shell/slices/tierTestReport";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const probe: FleetProbe = {
  device: { deviceId: "a".repeat(32), label: "This Mac", tier: "T2", role: "heavy" },
  tier: sampleReport(),
};

function fakeApi() {
  const discoveryCalls: boolean[] = [];
  const unused = () => Promise.reject(new Error("not used in this test"));
  const api: FleetApi = {
    probe: async () => probe,
    setLabel: unused,
    roster: async () => [],
    setDiscovery: async (enabled) => {
      discoveryCalls.push(enabled);
      return { enabled };
    },
    browse: unused,
    createLink: unused,
    inspectLink: unused,
    joinLink: unused,
    tailscale: unused,
  };
  return { api, discoveryCalls };
}

let root: Root | null = null;
let host: HTMLDivElement | null = null;
afterEach(() => {
  act(() => root?.unmount());
  host?.remove();
  root = null;
  host = null;
});

async function flush() {
  await act(async () => {
    await new Promise((r) => setTimeout(r, 0));
  });
}

function buttonByText(text: string): HTMLButtonElement {
  const b = Array.from(host?.querySelectorAll("button") ?? []).find((x) => x.textContent?.includes(text));
  if (!b) throw new Error(`no button "${text}"`);
  return b as HTMLButtonElement;
}

describe("fleet wizard discovery consent", () => {
  it("never turns discovery on by itself, and turns it off when the wizard unmounts", async () => {
    const { api, discoveryCalls } = fakeApi();
    host = document.createElement("div");
    document.body.appendChild(host);
    root = createRoot(host);
    await act(async () => {
      root?.render(<FleetWizard api={api} available />);
    });
    await act(async () => {
      host?.querySelector<HTMLButtonElement>('[data-testid="fleet-start"]')?.click();
    });
    await flush();
    await act(async () => {
      buttonByText("Next: find my machines").click();
    });
    expect(discoveryCalls).toEqual([]);
    const toggle = host.querySelector<HTMLInputElement>('[data-testid="fleet-discovery-toggle"]');
    expect(toggle?.checked).toBe(false);
    await act(async () => {
      toggle?.click();
    });
    await flush();
    expect(discoveryCalls).toEqual([true]);
    act(() => root?.unmount());
    root = null;
    await flush();
    expect(discoveryCalls).toEqual([true, false]);
  });

  it("does not call discovery off on unmount when it was never turned on", async () => {
    const { api, discoveryCalls } = fakeApi();
    host = document.createElement("div");
    document.body.appendChild(host);
    root = createRoot(host);
    await act(async () => {
      root?.render(<FleetWizard api={api} available />);
    });
    act(() => root?.unmount());
    root = null;
    await flush();
    expect(discoveryCalls).toEqual([]);
  });
});
