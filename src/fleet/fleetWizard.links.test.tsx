// HUP-S8.1/S8.2 follow-on: the wired wizard takes a deep-linked pairing link (filled in, never
// auto-joined), refreshes the issuing machine's list while its link is open, and links this machine
// through the injected ceremony opener.
import { describe, it, expect, afterEach, vi } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { FleetWizard } from "./FleetWizard";
import type { FleetApi, FleetDevice, FleetProbe, PairOffer } from "../bridge/tauri/fleet";
import { sampleReport } from "../shell/slices/tierTestReport";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const probe: FleetProbe = {
  device: { deviceId: "a".repeat(32), label: "This Mac", tier: "T2", role: "heavy" },
  tier: sampleReport(),
};
const joinedBox: FleetDevice = {
  id: "b".repeat(32),
  label: "Linux box",
  tier: "T1",
  role: "worker",
  addr: "192.168.1.30",
  pairedAt: 5,
  via: "issued",
  deviceLink: "added",
};

let root: Root | null = null;
let host: HTMLDivElement | null = null;
afterEach(() => {
  act(() => root?.unmount());
  host?.remove();
  root = null;
  host = null;
  vi.useRealTimers();
});

async function flush() {
  await act(async () => {
    await new Promise((r) => setTimeout(r, 0));
  });
}

function api(over: Partial<FleetApi>): FleetApi {
  const unused = () => Promise.reject(new Error("not used in this test"));
  return {
    probe: async () => probe,
    setLabel: unused,
    roster: async () => [],
    setDiscovery: async (enabled) => ({ enabled }),
    browse: unused,
    createLink: unused,
    inspectLink: unused,
    joinLink: vi.fn(unused),
    tailscale: unused,
    ...over,
  };
}

async function mount(el: React.ReactElement) {
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  await act(async () => {
    root?.render(el);
  });
  await flush();
  await flush();
}

describe("fleet wizard: links", () => {
  it("a deep-linked pairing link is filled in on the pair step and not joined", async () => {
    const joinLink = vi.fn(async () => ({ ok: false, device: null, errorKind: null, message: null, tried: [] }));
    const taken = vi.fn();
    await mount(<FleetWizard api={api({ joinLink })} available initialLink="citrate://pair?c=a&s=b" onInitialLinkTaken={taken} />);
    const input = host?.querySelector<HTMLInputElement>('input[aria-label="Pairing link"]');
    expect(input?.value).toBe("citrate://pair?c=a&s=b");
    expect(joinLink).not.toHaveBeenCalled();
    expect(taken).toHaveBeenCalledTimes(1);
  });

  it("while a pairing link is open, the list refreshes when the other machine joins", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    let roster: FleetDevice[] = [];
    const offer: PairOffer = { link: "citrate://pair?c=x&s=y", qr: { size: 1, rows: ["1"] }, expiresAt: Math.floor(Date.now() / 1000) + 600, hints: ["127.0.0.1:1"] };
    await mount(<FleetWizard api={api({ roster: async () => roster, createLink: async () => offer })} available />);
    await act(async () => {
      host?.querySelector<HTMLButtonElement>('[data-testid="fleet-start"]')?.click();
    });
    await flush();
    const pairNav = Array.from(host?.querySelectorAll("button") ?? []).find((b) => b.textContent === "Pair");
    await act(async () => {
      pairNav?.click();
    });
    await act(async () => {
      host?.querySelector<HTMLButtonElement>('[data-testid="fleet-create-link"]')?.click();
    });
    await flush();
    expect(host?.textContent).not.toContain("Linux box");
    roster = [joinedBox];
    await act(async () => {
      vi.advanceTimersByTime(3_500);
    });
    await flush();
    expect(host?.textContent).toContain("Linux box");
    expect(host?.textContent).toContain("linked under you");
  });

  it("links this machine through the injected opener, then re-reads the link status", async () => {
    let linked = false;
    const linkThisDevice = vi.fn(async () => {
      linked = true;
    });
    const linkStatus = vi.fn(async () => ({ linked, label: linked ? "This Mac" : null }));
    await mount(<FleetWizard api={api({ linkStatus, linkThisDevice })} available />);
    await act(async () => {
      host?.querySelector<HTMLButtonElement>('[data-testid="fleet-start"]')?.click();
    });
    await flush();
    const pairNav = Array.from(host?.querySelectorAll("button") ?? []).find((b) => b.textContent === "Pair");
    await act(async () => {
      pairNav?.click();
    });
    await act(async () => {
      host?.querySelector<HTMLButtonElement>('[data-testid="fleet-link-device"]')?.click();
    });
    await flush();
    expect(linkThisDevice).toHaveBeenCalledWith("This Mac");
    expect(host?.textContent).toContain("This machine is linked as");
  });
});
