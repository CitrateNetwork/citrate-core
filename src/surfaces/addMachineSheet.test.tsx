// 0.5.4 FL design, board 04: Add a machine, on both devices. Pending (waiting for the other machine),
// Success (the machine that used THIS link joined), expiry, every join outcome in plain words, the
// six-digit code before and after pairing, Remove this machine on a code mismatch, and group links
// (shown first, dialled only on confirm, never with the mesh off).
import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { AddMachineSheet, GROUP_LINK_MESH_OFF, ROSTER_POLL_MS } from "./AddMachineSheet";
import { CODE_MISMATCH } from "./clusterBoard";
import type { FleetApi, FleetDevice, JoinResult, PairClaim } from "../bridge/tauri/fleet";
import type { MeshStatus } from "../bridge/domains";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;
const NOW = 1000;
const dev = (id: string, label: string, code?: string, over: Partial<FleetDevice> = {}): FleetDevice => ({ id, label, tier: "T1", role: "worker", addr: null, pairedAt: NOW + 1, via: "issued", ...(code ? { code } : {}), ...over });
const claim: PairClaim = { v: 1, nonce: "n", issuerPub: "p", issuerLabel: "Studio Mac", issuerTier: "T2", issuedAt: 0, expiresAt: 1600, hints: [], code: "654321" };
const MESH_OFF: MeshStatus = { on: false, source: "off", signedOff: true, note: "Off on this machine." };
const MESH_ON: MeshStatus = { on: true, source: "default", signedOff: true, note: "On." };

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
    forget: vi.fn(async () => []),
    tailscale: vi.fn(),
    ...over,
  } as FleetApi;
}
const flush = async () => {
  for (let i = 0; i < 4; i++) await act(async () => new Promise((r) => setTimeout(r, 0)));
};
const button = (label: string) => [...document.querySelectorAll("button")].find((b) => b.textContent === label) as HTMLButtonElement;
const byTest = (id: string) => document.querySelector(`[data-testid="${id}"]`);
const setValue = async (input: HTMLInputElement, v: string) => {
  const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!;
  await act(async () => {
    set.call(input, v);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
};

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
    await act(async () => root.render(<AddMachineSheet mode="add" api={api()} groupLabel="Orchard Lab" onClose={onClose} nowSecs={() => NOW} />));
    await flush();
    const dlg = document.querySelector('[role="dialog"]')!;
    expect(dlg.getAttribute("aria-modal")).toBe("true");
    expect(document.getElementById("add-machine-h")?.textContent).toBe("Add a machine to Orchard Lab");
    expect(document.activeElement?.textContent).toBe("Close");
    await act(async () => dlg.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })));
    expect(onClose).toHaveBeenCalled();
  });

  it("Pending: shows the QR, the link, its expiry and waits for the other machine", async () => {
    await act(async () => root.render(<AddMachineSheet mode="add" api={api()} groupLabel={null} onClose={() => {}} nowSecs={() => NOW} />));
    await flush();
    expect(byTest("add-machine-qr")?.getAttribute("aria-label")).toBe("QR code with the pairing link");
    expect(byTest("add-machine-link")?.textContent).toBe("citrate://pair?c=abc");
    expect(document.body.textContent).toContain("Single use, expires in 10 min.");
    expect(byTest("add-machine-waiting")).not.toBeNull();
  });

  it("Success: the machine that used this link appears with its code, the mismatch advice and Remove", async () => {
    vi.useFakeTimers({ toFake: ["setInterval", "clearInterval"] });
    let rosterNow: FleetDevice[] = [dev("old", "Old")];
    const onPaired = vi.fn();
    const forget = vi.fn(async () => []);
    await act(async () => root.render(<AddMachineSheet mode="add" api={api({ roster: async () => rosterNow, forget })} groupLabel={null} onClose={() => {}} onPaired={onPaired} nowSecs={() => NOW} />));
    await flush();
    rosterNow = [dev("old", "Old"), dev("new", "MacBook Air", "482913")];
    await act(async () => vi.advanceTimersByTime(ROSTER_POLL_MS));
    await flush();
    const j = byTest("add-machine-joined")!;
    expect(j.textContent).toContain("MacBook Air joined");
    expect(byTest("add-machine-code")?.textContent).toContain("This machine shows code 482913. Codes match when the other machine shows the same six digits.");
    expect(j.textContent).toContain(CODE_MISMATCH);
    expect(byTest("add-machine-waiting")).toBeNull();
    expect(onPaired).toHaveBeenCalled();
    await act(async () => button("Remove this machine").click());
    await flush();
    expect(forget).toHaveBeenCalledWith("new");
    expect(byTest("machine-removed")?.textContent).toContain("MacBook Air is removed from this machine's paired machines");
  });

  it("a machine that paired some other way is not reported as joining through this link", async () => {
    vi.useFakeTimers({ toFake: ["setInterval", "clearInterval"] });
    let rosterNow: FleetDevice[] = [];
    await act(async () => root.render(<AddMachineSheet mode="add" api={api({ roster: async () => rosterNow })} groupLabel={null} onClose={() => {}} nowSecs={() => NOW} />));
    await flush();
    rosterNow = [dev("wiz", "Older pairing", "111111", { pairedAt: NOW - 600 }), dev("joined", "I joined it", "222222", { via: "joined" })];
    await act(async () => vi.advanceTimersByTime(ROSTER_POLL_MS));
    await flush();
    expect(byTest("add-machine-joined")).toBeNull();
    expect(byTest("add-machine-waiting")).not.toBeNull();
  });

  it("an expired link says so, hides Waiting, and Make a new link makes another", async () => {
    let now = NOW;
    const a = api({ createLink: vi.fn(async () => ({ link: "citrate://pair?c=x", qr: { size: 2, rows: ["10", "01"] }, expiresAt: now + 600, hints: [] })) });
    const clock = () => now;
    await act(async () => root.render(<AddMachineSheet mode="add" api={a} groupLabel={null} onClose={() => {}} nowSecs={clock} />));
    await flush();
    expect(byTest("add-machine-waiting")).not.toBeNull();
    now = NOW + 601;
    await act(async () => root.render(<AddMachineSheet mode="add" api={a} groupLabel={null} onClose={() => {}} nowSecs={() => now} />));
    expect(byTest("add-machine-waiting")).toBeNull();
    expect(byTest("add-machine-qr")).toBeNull();
    expect(byTest("add-machine-expired")?.textContent).toContain("This link has expired");
    await act(async () => button("Make a new link").click());
    await flush();
    expect(a.createLink).toHaveBeenCalledTimes(2);
  });

  it("a link core could not make is said, not faked", async () => {
    await act(async () => root.render(<AddMachineSheet mode="add" api={api({ createLink: async () => { throw new Error("pairing service not running"); } })} groupLabel={null} onClose={() => {}} />));
    await flush();
    expect(document.querySelector('[role="alert"]')?.textContent).toBe("Couldn't make a pairing link: pairing service not running");
    expect(byTest("add-machine-qr")).toBeNull();
  });
});

describe("Join with a link (the other device)", () => {
  async function readLink(a: FleetApi, linkText = "citrate://pair?c=abc") {
    await act(async () => root.render(<AddMachineSheet mode="join" api={a} groupLabel={null} onClose={() => {}} nowSecs={() => NOW} />));
    await setValue(document.querySelector("input") as HTMLInputElement, linkText);
    await act(async () => button("Read the link").click());
    await flush();
  }
  async function readAndPair(result: JoinResult | Error, forget = vi.fn(async () => [])) {
    const joinLink = vi.fn(async () => {
      if (result instanceof Error) throw result;
      return result;
    });
    await readLink(api({ joinLink, forget }));
    expect(byTest("join-claim")?.textContent).toContain("Studio Mac wants to pair");
    expect(joinLink).not.toHaveBeenCalled(); // reading a link sends nothing
    await act(async () => button("Pair with this machine").click());
    await flush();
    return { outcome: byTest("join-outcome")!, joinLink, forget };
  }
  const fail = (errorKind: JoinResult["errorKind"]): JoinResult => ({ ok: false, device: null, errorKind, message: null, tried: ["a"] });

  it("before pairing, it says which code both machines will show", async () => {
    await readLink(api());
    expect(byTest("join-code")?.textContent).toBe("Pairing will show code 654321 on both machines. Check the other machine shows the same code after you pair.");
  });

  it("an older core gives no code: it still says to compare after pairing", async () => {
    const { code: _drop, ...old } = claim;
    void _drop;
    await readLink(api({ inspectLink: async () => old }));
    expect(byTest("join-code")).toBeNull();
    expect(byTest("join-claim")?.textContent).toContain("both machines show a six-digit code; check they match");
  });

  it("ok: names the machine, gives the mismatch advice and can remove the other machine", async () => {
    const { outcome, forget } = await readAndPair({ ok: true, device: dev("issuer-id", "Studio Mac", "654321"), errorKind: null, message: null, tried: [] });
    expect(outcome.getAttribute("data-ok")).toBe("yes");
    expect(outcome.textContent).toContain("Paired with Studio Mac. Both machines show code 654321; check they match.");
    expect(outcome.textContent).toContain(CODE_MISMATCH);
    await act(async () => button("Remove this machine").click());
    await flush();
    expect(forget).toHaveBeenCalledWith("issuer-id");
  });

  it("pairs with the link that was read, not what the field holds later; editing resets", async () => {
    const joinLink = vi.fn(async (): Promise<JoinResult> => fail("refused"));
    await readLink(api({ joinLink }), "citrate://pair?c=first");
    const input = document.querySelector("input") as HTMLInputElement;
    await setValue(input, "citrate://pair?c=second");
    // The read claim no longer applies: the member has to read the new link.
    expect(byTest("join-claim")).toBeNull();
    expect(button("Read the link")).toBeDefined();
    await setValue(input, "citrate://pair?c=first");
    await act(async () => button("Read the link").click());
    await flush();
    await act(async () => button("Pair with this machine").click());
    await flush();
    expect(joinLink).toHaveBeenCalledWith("citrate://pair?c=first");
  });

  it.each([
    ["link", "not a valid pairing link"],
    ["refused", "refused the pairing"],
    ["unreachable", "Couldn't reach the other machine"],
    ["protocol", "does not understand"],
  ] as const)("%s", async (kind, words) => {
    const { outcome } = await readAndPair(fail(kind));
    expect(outcome.getAttribute("data-ok")).toBe("no");
    expect(outcome.textContent).toContain(words);
    expect(outcome.textContent).not.toContain("Remove this machine");
  });

  it("a thrown join is reported, not swallowed", async () => {
    const { outcome } = await readAndPair(new Error("socket closed"));
    expect(outcome.textContent).toContain("socket closed");
  });
});

describe("Group links on the join side", () => {
  async function pasteSeed(mesh: MeshStatus | null, addGroupSeed = vi.fn(async () => 3)) {
    await act(async () => root.render(<AddMachineSheet mode="join" api={api()} groupLabel="Orchard Lab" addGroupSeed={addGroupSeed} mesh={mesh} onClose={() => {}} />));
    await setValue(document.querySelector("input") as HTMLInputElement, "citrate-cluster://seed?v=1&g=x&a=y");
    await act(async () => button("Read the link").click());
    await flush();
    return addGroupSeed;
  }

  it("Read the link only shows what it is; it dials on the explicit confirm", async () => {
    const addGroupSeed = await pasteSeed(MESH_ON);
    expect(byTest("join-seed")?.textContent).toContain("This is a group link");
    expect(addGroupSeed).not.toHaveBeenCalled();
    await act(async () => button("Reach this group's machines").click());
    await flush();
    expect(addGroupSeed).toHaveBeenCalledWith("citrate-cluster://seed?v=1&g=x&a=y");
    expect(byTest("join-outcome")?.textContent).toContain("Reaching 3 addresses from the group link");
  });

  it("with the mesh off it never dials and says why", async () => {
    const addGroupSeed = await pasteSeed(MESH_OFF);
    const off = byTest("join-seed-mesh-off")!;
    expect(off.textContent).toContain(GROUP_LINK_MESH_OFF);
    expect(off.textContent).toContain("Off on this machine.");
    expect(off.textContent).toContain("Private mesh");
    expect(button("Reach this group's machines")).toBeUndefined();
    expect(addGroupSeed).not.toHaveBeenCalled();
    expect(document.body.textContent).not.toMatch(/Reaching \d/);
  });
});
