// HUP-S6.7 (US-6.3) — the Contract reader view: open an address, show the verified source and
// ABI (or take a pasted ABI), run reads, send writes to the ceremony, ask Hermes to explain.
import { describe, it, expect, afterEach, vi } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { ContractReader } from "./ContractReader";
import type { ContractClient, ContractOp, ContractOpArgs, ContractOpResults, ReaderSource } from "./contractChannel";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const ADDR = "0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed";
const ABI = [
  { type: "function", name: "totalSupply", inputs: [], outputs: [{ type: "uint256" }], stateMutability: "view" },
  { type: "function", name: "mint", inputs: [{ name: "quantity", type: "uint256" }], outputs: [], stateMutability: "payable" },
];
const VERIFIED: ReaderSource = { status: "verified", isContract: true, codeSize: 4000, contractName: "LemonDrops", compilerVersion: "0.8.36", abi: ABI, source: "contract LemonDrops {}", note: null };

type Calls = { op: ContractOp; args: unknown }[];
function client(answers: Partial<{ [K in ContractOp]: (a: ContractOpArgs[K]) => ContractOpResults[K] | Promise<ContractOpResults[K]> }>): { c: ContractClient; calls: Calls } {
  const calls: Calls = [];
  const c: ContractClient = {
    async call(op, args) {
      calls.push({ op, args });
      const f = answers[op] as ((a: unknown) => unknown) | undefined;
      if (!f) throw new Error(`no answer for ${op}`);
      return (await f(args)) as never;
    },
    close() {},
  };
  return { c, calls };
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
  const el = input as HTMLInputElement | HTMLTextAreaElement;
  const setter = Object.getOwnPropertyDescriptor(Object.getPrototypeOf(el), "value")?.set;
  setter?.call(el, value);
  el.dispatchEvent(new Event("input", { bubbles: true }));
}
async function click(el: HTMLElement | null) {
  await act(async () => { el?.click(); });
  await act(async () => { await new Promise((r) => setTimeout(r, 0)); });
}

describe("Feature: read any contract (US-6.3)", () => {
  it("AC1: a verified 40204 address shows its source, ABI functions and verified state", async () => {
    const { c } = client({ initial: () => null, source: () => VERIFIED, codeSize: () => 4000 });
    const el = await render(<ContractReader client={c} />);
    await act(async () => { type($(el, "cr-address"), ADDR); });
    await click($(el, "cr-load"));
    expect($(el, "cr-status")?.textContent).toMatch(/verified on CitrateScan/i);
    expect($(el, "cr-status")?.textContent).toContain("LemonDrops");
    expect($(el, "cr-fn-totalSupply()")).not.toBeNull();
    expect($(el, "cr-fn-mint(uint256)")).not.toBeNull();
    expect($(el, "cr-source")?.textContent).toContain("contract LemonDrops");
  });

  it("AC2: a view call runs read-only and shows the decoded answer", async () => {
    const { c, calls } = client({ initial: () => null, source: () => VERIFIED, codeSize: () => 4000, view: () => "0x" + "0".repeat(61) + "1f4" });
    const el = await render(<ContractReader client={c} />);
    await act(async () => { type($(el, "cr-address"), ADDR); });
    await click($(el, "cr-load"));
    await click($(el, "cr-read-totalSupply()"));
    expect(calls.find((x) => x.op === "view")?.args).toEqual({ target: "citrate", address: ADDR, calldata: "0x18160ddd" });
    expect($(el, "cr-result-totalSupply()")?.textContent).toContain("500");
  });

  it("AC2: a write goes to the Signature Ceremony in the main window, with the value in wei", async () => {
    const { c, calls } = client({ initial: () => null, source: () => VERIFIED, codeSize: () => 4000, write: () => ({ proposed: true }) });
    const el = await render(<ContractReader client={c} />);
    await act(async () => { type($(el, "cr-address"), ADDR); });
    await click($(el, "cr-load"));
    await act(async () => {
      type($(el, "cr-in-mint(uint256)-0"), "2");
      type($(el, "cr-value-mint(uint256)"), "10");
    });
    await click($(el, "cr-write-mint(uint256)"));
    expect(calls.find((x) => x.op === "write")?.args).toEqual({
      address: ADDR,
      calldata: "0xa0712d680000000000000000000000000000000000000000000000000000000000000002",
      valueWei: "10000000000000000000",
      label: "mint(uint256)",
    });
    expect($(el, "cr-result-mint(uint256)")?.textContent).toMatch(/Signature Ceremony/);
  });

  it("AC2: Explain asks Hermes and shows whose explanation it is", async () => {
    const { c, calls } = client({ initial: () => null, source: () => VERIFIED, codeSize: () => 4000, explain: () => ({ text: "Mints up to 10 tokens.", by: "local model" }) });
    const el = await render(<ContractReader client={c} />);
    await act(async () => { type($(el, "cr-address"), ADDR); });
    await click($(el, "cr-load"));
    await click($(el, "cr-explain-mint(uint256)"));
    const prompt = (calls.find((x) => x.op === "explain")?.args as { prompt: string }).prompt;
    expect(prompt).toContain("mint(uint256)");
    expect($(el, "cr-explanation-mint(uint256)")?.textContent).toContain("Mints up to 10 tokens.");
    expect($(el, "cr-explanation-mint(uint256)")?.textContent).toMatch(/Hermes \(local model\)/);
  });

  it("an unverified contract asks for an ABI, and a pasted ABI works", async () => {
    const { c } = client({ initial: () => null, source: () => ({ ...VERIFIED, status: "unverified", abi: null, source: null, contractName: null }), codeSize: () => 10 });
    const el = await render(<ContractReader client={c} />);
    await act(async () => { type($(el, "cr-address"), ADDR); });
    await click($(el, "cr-load"));
    expect($(el, "cr-status")?.textContent).toMatch(/not verified/i);
    expect($(el, "cr-fn-totalSupply()")).toBeNull();
    await act(async () => { type($(el, "cr-abi"), JSON.stringify(ABI)); });
    await click($(el, "cr-use-abi"));
    expect($(el, "cr-fn-totalSupply()")).not.toBeNull();
    expect($(el, "cr-abi-origin")?.textContent).toMatch(/pasted/i);
  });

  it("a fork target reads from the fork, skips CitrateScan, and never proposes writes", async () => {
    const { c, calls } = client({ initial: () => null, codeSize: () => 4000, view: () => "0x" + "0".repeat(63) + "1" });
    const el = await render(<ContractReader client={c} />);
    await click($(el, "cr-target-fork"));
    await act(async () => { type($(el, "cr-address"), ADDR); });
    await click($(el, "cr-load"));
    expect(calls.some((x) => x.op === "source")).toBe(false);
    expect($(el, "cr-status")?.textContent).toMatch(/fork/i);
    await act(async () => { type($(el, "cr-abi"), JSON.stringify(ABI)); });
    await click($(el, "cr-use-abi"));
    await click($(el, "cr-read-totalSupply()"));
    expect(calls.find((x) => x.op === "view")?.args).toMatchObject({ target: "http://127.0.0.1:8545" });
    expect(($(el, "cr-write-mint(uint256)") as HTMLButtonElement).disabled).toBe(true);
  });

  it("an address with no code says so; a bad address is refused before any request", async () => {
    const { c, calls } = client({ initial: () => null, source: () => ({ ...VERIFIED, status: "notContract", isContract: false, abi: null }), codeSize: () => 0 });
    const el = await render(<ContractReader client={c} />);
    await act(async () => { type($(el, "cr-address"), "0x123"); });
    await click($(el, "cr-load"));
    expect($(el, "cr-error")?.textContent).toMatch(/address/i);
    expect(calls.filter((x) => x.op !== "initial")).toEqual([]);
    await act(async () => { type($(el, "cr-address"), ADDR); });
    await click($(el, "cr-load"));
    expect($(el, "cr-status")?.textContent).toMatch(/no contract code/i);
  });

  it("opens the address the main window asked for", async () => {
    const source = vi.fn(() => VERIFIED);
    const { c } = client({ initial: () => ({ address: ADDR, target: "citrate" }), source, codeSize: () => 4000 });
    const el = await render(<ContractReader client={c} />);
    await act(async () => { await new Promise((r) => setTimeout(r, 0)); });
    expect(source).toHaveBeenCalled();
    expect(($(el, "cr-address") as HTMLInputElement).value).toBe(ADDR);
  });
});
