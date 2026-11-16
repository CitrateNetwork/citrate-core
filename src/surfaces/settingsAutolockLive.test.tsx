// 0.5.4 — Settings › Keys auto-lock: the choice is saved to core, which applies it to the open
// vault at once (config_write → apply_live_settings); the toast says so only after the save.
import { describe, it, expect, vi, afterEach } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";

vi.mock("viem", async (orig) => {
  const real = await orig<typeof import("viem")>();
  return { ...real, createPublicClient: () => ({ getBlock: async () => ({ hash: null }), getBlockNumber: async () => 1n }) };
});

import { Settings } from "./Settings";
import { bridge } from "../bridge";
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
async function mount() {
  const host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  await act(async () => {
    root!.render(<Settings store={stubStore} s={{ ...freshState("p1"), sSec: "keys", signedIn: true }} />);
  });
  return host;
}
afterEach(async () => {
  await act(async () => root?.unmount());
  root = null;
  toasts.length = 0;
  vi.restoreAllMocks();
});

const click = async (host: HTMLElement, label: string) => {
  const b = [...host.querySelectorAll("button")].find((x) => x.textContent === label);
  expect(b).toBeTruthy();
  await act(async () => {
    b!.click();
  });
  await act(async () => {});
};

describe("Settings › Keys auto-lock applies live", () => {
  it("Given a new window, then it is saved to core and the toast says it applies now", async () => {
    const write = vi.spyOn(bridge.config, "write").mockResolvedValue({} as never);
    const host = await mount();
    await click(host, "15 min");
    expect(write).toHaveBeenCalledWith({ autolock: 15 });
    expect(toasts).toEqual(["Auto-lock set to 15 minutes. It applies now, with no restart"]);
    expect(host.textContent).toMatch(/A change applies at once, with no restart/);
  });

  it("Given the save fails, then the toast says it was not saved", async () => {
    vi.spyOn(bridge.config, "write").mockRejectedValue(new Error("disk full"));
    const host = await mount();
    await click(host, "60 min");
    expect(toasts).toEqual(["Auto-lock was not saved: disk full"]);
  });
});
