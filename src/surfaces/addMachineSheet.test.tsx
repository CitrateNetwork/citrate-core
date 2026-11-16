// 0.5.4 FL design, board 04: Add a machine, on both devices. Pending (waiting for the other machine),
// Success (the machine joined), and every join outcome in plain words.
import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { AddMachineSheet, ROSTER_POLL_MS } from "./AddMachineSheet";
import type { FleetApi, FleetDevice, JoinResult, PairClaim } from "../bridge/tauri/fleet";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;
const dev = (id: string, label: string, code?: string): FleetDevice => ({ id, label, tier: "T1", role: "worker", addr: null, pairedAt: 1, via: "issued", ...(code ? { code } : {}) });
const claim: PairClaim = { v: 1, nonce: "n", issuerPub: "p", issuerLabel: "Studio Mac", issuerTier: "T2", issuedAt: 0, expiresAt: 1600, hints: [] };

function api(over: Partial<FleetApi> = {}): FleetApi {
  return {
    probe: vi.fn(),
    setLabel: vi.fn(),
    roster: async () => [],
    setDiscovery: vi.fn(),
    browse: vi.fn(),
    createLink: async () => ({ link: "citrate://pair?c=abc", qr: { size: 2, rows: ["10", "01"] }, expiresAt: 1600, hints: [] }),
    inspectLink: async () => claim,
    joinLink: vi.fn(),
    tailscale: vi.fn(),
    ...over,
  } as FleetApi;
}
const flush = async () => {
  for (let i = 0; i < 4; i++) await act(async () => new Promise((r) => setTimeout(r, 0)));
};
const button = (label: string) => [...document.querySelectorAll("button")].find((b) => b.textContent === label) as HTMLButtonElement;
const byTest = (id: string) => document.querySelector(`[data-testid="${id}"]`);

beforeEach(() => {
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(async () => {
  vi.useRealTimers();
  await act(async () => root.unmount());
  host.remove();
});

describe("Add a machine (this device)", () => {
  it("is a modal that starts on Close and closes on Escape", async () => {
    const onClose = vi.fn();
    await act(async () => root.render(<AddMachineSheet mode="add" api={api()} groupLabel="Orchard Lab" onClose={onClose} nowSecs={() => 1000} />));
    await flush();
    const dlg = document.querySelector('[role="dialog"]')!;
    expect(dlg.getAttribute("aria-modal")).toBe("true");
    expect(document.getElementById("add-machine-h")?.textContent).toBe("Add a machine to Orchard Lab");
    expect(document.activeElement?.textContent).toBe("Close");
    await act(async () => dlg.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })));
    expect(onClose).toHaveBeenCalled();
  });

  it("Pending: shows the QR, the link, its expiry and waits for the other machine", async () => {
    await act(async () => root.render(<AddMachineSheet mode="add" api={api()} groupLabel={null} onClose={() => {}} nowSecs={() => 1000} />));
    await flush();
    expect(byTest("add-machine-qr")?.getAttribute("aria-label")).toBe("QR code with the pairing link");
    expect(byTest("add-machine-link")?.textContent).toBe("citrate://pair?c=abc");
    expect(document.body.textContent).toContain("Single use, expires in 10 min.");
    expect(byTest("add-machine-waiting")).not.toBeNull();
  });

  it("Success: the machine that pairs appears with the code to compare", async () => {
    vi.useFakeTimers({ toFake: ["setInterval", "clearInterval"] });
    let rosterNow: FleetDevice[] = [dev("old", "Old")];
    const onPaired = vi.fn();
    await act(async () => root.render(<AddMachineSheet mode="add" api={api({ roster: async () => rosterNow })} groupLabel={null} onClose={() => {}} onPaired={onPaired} nowSecs={() => 1000} />));
    await flush();
    rosterNow = [dev("old", "Old"), dev("new", "MacBook Air", "482913")];
    await act(async () => vi.advanceTimersByTime(ROSTER_POLL_MS));
    await flush();
    expect(byTest("add-machine-joined")?.textContent).toContain("MacBook Air joined");
    expect(byTest("add-machine-joined")?.textContent).toContain("code 482913");
    expect(onPaired).toHaveBeenCalled();
  });

  it("a link core could not make is said, not faked", async () => {
    await act(async () => root.render(<AddMachineSheet mode="add" api={api({ createLink: async () => { throw new Error("pairing service not running"); } })} groupLabel={null} onClose={() => {}} />));
    await flush();
    expect(document.querySelector('[role="alert"]')?.textContent).toBe("Couldn't make a pairing link: pairing service not running");
    expect(byTest("add-machine-qr")).toBeNull();
  });
});

describe("Join with a link (the other device)", () => {
  async function readAndPair(result: JoinResult | Error) {
    const joinLink = vi.fn(async () => {
      if (result instanceof Error) throw result;
      return result;
    });
    await act(async () => root.render(<AddMachineSheet mode="join" api={api({ joinLink })} groupLabel={null} onClose={() => {}} nowSecs={() => 1000} />));
    const input = document.querySelector("input") as HTMLInputElement;
    const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!;
    await act(async () => {
      set.call(input, "citrate://pair?c=abc");
      input.dispatchEvent(new Event("input", { bubbles: true }));
    });
    await act(async () => button("Read the link").click());
    await flush();
    expect(byTest("join-claim")?.textContent).toContain("Studio Mac wants to pair");
    expect(joinLink).not.toHaveBeenCalled(); // reading a link sends nothing
    await act(async () => button("Pair with this machine").click());
    await flush();
    return byTest("join-outcome")!;
  }
  const fail = (errorKind: JoinResult["errorKind"]): JoinResult => ({ ok: false, device: null, errorKind, message: null, tried: ["a"] });

  it("ok", async () => {
    const o = await readAndPair({ ok: true, device: dev("s", "Studio Mac", "111222"), errorKind: null, message: null, tried: [] });
    expect(o.getAttribute("data-ok")).toBe("yes");
    expect(o.textContent).toBe("Paired with Studio Mac. Both machines show code 111222; check they match.");
  });
  it.each([
    ["link", "not a valid pairing link"],
    ["refused", "refused the pairing"],
    ["unreachable", "Couldn't reach the other machine"],
    ["protocol", "does not understand"],
  ] as const)("%s", async (kind, words) => {
    const o = await readAndPair(fail(kind));
    expect(o.getAttribute("data-ok")).toBe("no");
    expect(o.textContent).toContain(words);
  });
  it("a thrown join is reported, not swallowed", async () => {
    const o = await readAndPair(new Error("socket closed"));
    expect(o.textContent).toContain("socket closed");
  });

  it("a group link dials the group's machines instead", async () => {
    const addGroupSeed = vi.fn(async () => 3);
    await act(async () => root.render(<AddMachineSheet mode="join" api={api()} groupLabel="Orchard Lab" addGroupSeed={addGroupSeed} onClose={() => {}} />));
    const input = document.querySelector("input") as HTMLInputElement;
    const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!;
    await act(async () => {
      set.call(input, "citrate-cluster://seed?v=1&g=x&a=y");
      input.dispatchEvent(new Event("input", { bubbles: true }));
    });
    await act(async () => button("Read the link").click());
    await flush();
    expect(addGroupSeed).toHaveBeenCalledWith("citrate-cluster://seed?v=1&g=x&a=y");
    expect(byTest("join-outcome")?.textContent).toContain("Reaching 3 addresses from the group link");
  });
});
