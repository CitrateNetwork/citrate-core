// HUP-S6.6 (US-6.1 tail) — the After-deploy panel: find the contract from the deploy receipt,
// verify on CitrateScan, switch the site to 40204, pin to IPFS, export for Vercel. Each step
// reports exactly what core answered, including failures.
import { describe, it, expect, afterEach, vi } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { PostDeployPanel, type PostDeployOps } from "./PostDeployPanel";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const ADDR = "0x5aaeb6053f3e94c9b9a09f33669435e7ef1beaed";
const TX = "0x" + "ab".repeat(32);

function ops(over: Partial<PostDeployOps> = {}): PostDeployOps {
  return {
    postdeployStatus: vi.fn(async () => ({ contractName: "LemonDrops", siteContract: null, built: true, exportDir: null })),
    postdeployReceipt: vi.fn(async () => ({ txHash: TX, blockNumber: 12, status: 1, contractAddress: ADDR })),
    postdeployVerify: vi.fn(async () => ({ status: "verified" as const, guid: "vrf_1", matchType: "full", contractName: "LemonDrops", message: "ok" })),
    postdeploySwitchSite: vi.fn(async () => ({ envPath: "/p/app/.env.local", address: "0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed" })),
    postdeployPinSite: vi.fn(async () => ({ cid: "bafyroot", files: 2, bytes: 10, localGatewayUrl: "http://127.0.0.1:48080/ipfs/bafyroot/", publicGatewayUrl: "https://ipfs.io/ipfs/bafyroot/", note: "Pinned on this node." })),
    postdeployVercelExport: vi.fn(async () => ({ dir: "/p/vercel-export", files: 9, commands: ["cd \"/p/vercel-export\"", "npx vercel deploy --prod"] })),
    ...over,
  };
}

let root: Root | null = null;
let host: HTMLDivElement | null = null;
afterEach(() => {
  act(() => root?.unmount());
  host?.remove();
  root = null;
  host = null;
});
async function render(el: React.ReactElement) {
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  await act(async () => { root?.render(el); });
  return host;
}
const $ = (el: HTMLElement, id: string) => el.querySelector(`[data-testid="${id}"]`) as HTMLElement | null;
function type(input: HTMLElement | null, value: string) {
  const el = input as HTMLInputElement;
  const setter = Object.getOwnPropertyDescriptor(Object.getPrototypeOf(el), "value")?.set;
  setter?.call(el, value);
  el.dispatchEvent(new Event("input", { bubbles: true }));
}
async function click(el: HTMLElement | null) {
  await act(async () => { el?.click(); });
  await act(async () => { await new Promise((r) => setTimeout(r, 0)); });
}

describe("Feature: after the deploy (HUP-S6.6)", () => {
  it("says the page's mint is decoded by name only when core registered the gated ABI", async () => {
    for (const decodedCalls of [true, false]) {
      const o = ops({
        postdeploySwitchSite: vi.fn(async () => ({ envPath: "/p/app/.env.local", address: ADDR, decodedCalls })),
      });
      const el = await render(<PostDeployPanel ops={o} lastDeployTx={TX} openReader={vi.fn()} />);
      await act(async () => { type($(el, "pd-project"), "/p"); });
      await click($(el, "pd-find"));
      await click($(el, "pd-switch"));
      const text = $(el, "pd-switch-out")?.textContent ?? "";
      expect(text.includes("shows the call by name")).toBe(decodedCalls);
      act(() => root?.unmount());
      host?.remove();
      root = null;
      host = null;
    }
  });

  it("walks receipt → verify → switch → pin → export and shows each answer", async () => {
    const o = ops();
    const openReader = vi.fn();
    const el = await render(<PostDeployPanel ops={o} lastDeployTx={TX} openReader={openReader} />);
    expect(($(el, "pd-tx") as HTMLInputElement).value).toBe(TX);
    await act(async () => { type($(el, "pd-project"), "/p"); });
    await click($(el, "pd-find"));
    expect(o.postdeployReceipt).toHaveBeenCalledWith(TX);
    expect($(el, "pd-address")?.textContent).toContain(ADDR);

    await click($(el, "pd-verify"));
    expect(o.postdeployVerify).toHaveBeenCalledWith("/p", ADDR, undefined);
    expect($(el, "pd-verify-out")?.textContent).toMatch(/verified/i);

    await click($(el, "pd-switch"));
    expect(o.postdeploySwitchSite).toHaveBeenCalledWith("/p", ADDR);
    expect($(el, "pd-switch-out")?.textContent).toContain(".env.local");

    await click($(el, "pd-pin"));
    expect($(el, "pd-pin-out")?.textContent).toContain("bafyroot");
    expect($(el, "pd-pin-out")?.textContent).toContain("https://ipfs.io/ipfs/bafyroot/");

    await click($(el, "pd-export"));
    expect($(el, "pd-export-out")?.textContent).toContain("npx vercel deploy --prod");

    await click($(el, "pd-open-reader"));
    expect(openReader).toHaveBeenCalledWith(ADDR);
  });

  it("a pending or reverted deploy is said plainly, and later steps stay closed", async () => {
    const el = await render(<PostDeployPanel ops={ops({ postdeployReceipt: vi.fn(async () => null) })} lastDeployTx={TX} openReader={vi.fn()} />);
    await act(async () => { type($(el, "pd-project"), "/p"); });
    await click($(el, "pd-find"));
    expect($(el, "pd-address")?.textContent).toMatch(/not confirmed yet/i);
    expect(($(el, "pd-verify") as HTMLButtonElement).disabled).toBe(true);

    const el2 = await render(<PostDeployPanel ops={ops({ postdeployReceipt: vi.fn(async () => ({ txHash: TX, blockNumber: 3, status: 0, contractAddress: null })) })} lastDeployTx={TX} openReader={vi.fn()} />);
    await act(async () => { type($(el2, "pd-project"), "/p"); });
    await click($(el2, "pd-find"));
    expect($(el2, "pd-address")?.textContent).toMatch(/reverted/i);
  });

  it("failures from core are shown as they are", async () => {
    const o = ops({
      postdeployVerify: vi.fn(async () => ({ status: "unavailable" as const, guid: null, matchType: null, contractName: null, message: "CitrateScan is rate limiting requests" })),
      postdeployPinSite: vi.fn(async () => { throw new Error("IPFS is not running in this app. Nothing was pinned."); }),
    });
    const el = await render(<PostDeployPanel ops={o} lastDeployTx={TX} openReader={vi.fn()} />);
    await act(async () => { type($(el, "pd-project"), "/p"); });
    await click($(el, "pd-find"));
    await click($(el, "pd-verify"));
    expect($(el, "pd-verify-out")?.textContent).toMatch(/not verified.*rate limiting/i);
    await click($(el, "pd-pin"));
    expect($(el, "pd-pin-out")?.textContent).toMatch(/Nothing was pinned/);
  });

  it("without a project folder nothing runs", async () => {
    const o = ops();
    const el = await render(<PostDeployPanel ops={o} lastDeployTx={null} openReader={vi.fn()} />);
    expect(($(el, "pd-find") as HTMLButtonElement).disabled).toBe(true);
  });
});
