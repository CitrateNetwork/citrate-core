// 0.5.4 — Settings › Node configuration says only what core does: the network comes from the
// build's profile (no devnet switch that nothing reads), and the CPU cap names the worker
// threads core passes to the node at its next start.
import { describe, it, expect, vi, afterEach } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";

vi.mock("viem", async (orig) => {
  const real = await orig<typeof import("viem")>();
  return { ...real, createPublicClient: () => ({ getBlock: async () => ({ hash: null }), getBlockNumber: async () => 1n }) };
});

import { Settings, cpuCapThreads } from "./Settings";
import { freshState } from "../shell/state";
import type { Store } from "../shell/store";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const toasts: string[] = [];
const stubStore = {
  identity: () => ({ name: "Member", initials: "M", email: "m@example.com", sub: "s", wallet: "", tier: "pilot", role: "member", org: null, real: true }),
  toast: (t: string) => toasts.push(t),
  save: () => {},
  setState: () => {},
  state: freshState("p1"),
} as unknown as Store;

let root: Root | null = null;
let host: HTMLDivElement;
async function mount(cpuCap = 50) {
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  await act(async () => {
    root!.render(<Settings store={stubStore} s={{ ...freshState("p1"), sSec: "node", signedIn: true, cpuCap }} />);
  });
}
afterEach(async () => {
  await act(async () => root?.unmount());
  root = null;
});

describe("cpuCapThreads mirrors core", () => {
  it("is the share of the cores rounded up, at least one, and null without a cap", () => {
    expect(cpuCapThreads(8, 25)).toBe(2);
    expect(cpuCapThreads(10, 25)).toBe(3);
    expect(cpuCapThreads(1, 25)).toBe(1);
    expect(cpuCapThreads(8, 100)).toBeNull();
    expect(cpuCapThreads(0, 50)).toBeNull();
  });
});

describe("Settings › Node configuration", () => {
  it("shows the build's network read-only, with no local devnet switch and no 'every field is wired' claim", async () => {
    await mount();
    expect(host.textContent).not.toMatch(/every field here is wired/);
    expect(host.textContent).not.toMatch(/^local devnet$/m);
    expect([...host.querySelectorAll("button")].some((b) => b.textContent === "local devnet")).toBe(false);
    expect(host.querySelector('[data-testid="node-network"]')?.textContent).toMatch(/chain/);
    expect(host.textContent).toMatch(/This build joins one network/);
  });

  it("names exactly what the cap holds (the main worker threads) and what it does not", async () => {
    Object.defineProperty(navigator, "hardwareConcurrency", { value: 8, configurable: true });
    await mount(25);
    expect(host.querySelector('[data-testid="cpu-cap-note"]')?.textContent).toBe(
      "Applies at the next node start: the node's main worker threads use at most 2 of this computer's 8 cores. Not capped: its RPC server's own 16 threads, its blocking work and its database threads, so the node can use more CPU than this while it syncs or serves many RPC calls.",
    );
  });
});
