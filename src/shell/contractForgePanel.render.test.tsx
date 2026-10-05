// HUP-S6.2 / S6.3 / S6.9 (US-6.4) — the forge panel: the toolchain switch, the template form,
// and the deploy gate from Hermes's toolchain runs. Each step shows exactly what core answered.
import { describe, it, expect, afterEach, vi } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { ContractForgePanel, type ContractForgeOps } from "./ContractForgePanel";
import type { TemplateCatalog, ToolchainGateResult, ToolchainStatus } from "../agent/contractForge";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const OWNER = "0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed";
const HASH = "0x" + "ab".repeat(32);

const budget = { test_limit: 10000, workers: 2, call_sequence_length: 50, timeout_secs: 1200, coverage_plateau_calls: 2500, min_coverage_pct: 60 };

const CATALOG: TemplateCatalog = {
  tier: "T0",
  medusaBudget: budget,
  templates: [
    {
      id: "erc20",
      kind: "contract",
      title: "ERC-20 fixed supply (OpenZeppelin)",
      description: "An OpenZeppelin ERC-20.",
      medusa: true,
      fields: [
        { key: "name", label: "Name", help: "Letters.", default: null, min: null, max: null, required: true },
        { key: "symbol", label: "Symbol", help: "Caps.", default: null, min: null, max: null, required: true },
        { key: "supply", label: "Supply", help: "Whole.", default: "1000000", min: 1, max: 1000000000000, required: false },
        { key: "owner", label: "Owner address", help: "0x.", default: null, min: null, max: null, required: true },
      ],
    },
    { id: "erc1155", kind: "contract", title: "ERC-1155", description: "Multi.", medusa: true, fields: [{ key: "owner", label: "Owner address", help: "", default: null, min: null, max: null, required: true }] },
  ],
};

function status(enabled: boolean): ToolchainStatus {
  return {
    settings: { enabled },
    programs: [
      { program: "forge", tool: "forge_test", path: "/u/.foundry/bin/forge" },
      { program: "slither", tool: "slither_scan", path: "/u/.local/bin/slither" },
      { program: "aderyn", tool: "aderyn_scan", path: null },
      { program: "medusa", tool: "medusa_fuzz", path: null },
    ],
    solc: "/u/solc",
    searchPath: [],
    notices: ["Not installed: aderyn, medusa. Their deploy gate items fail (NOT READY) until they are installed."],
    appliesOnRestart: true,
    loadError: null,
  };
}

const GATE: ToolchainGateResult = {
  artifact: "Token.sol/LemonDrops.json",
  sourcesSha256: "5ea1",
  medusa: { tier: "T0", requiredCalls: 10000, minCoveragePct: 60, runTestLimit: 10000, linesHit: 3, linesTotal: 3, coveragePct: 100, problems: [] },
  record: {
    initcodeHash: HASH,
    bindingHash: "0x" + "cd".repeat(32),
    compiler: { solcVersion: "0.8.36", optimizer: true, optimizerRuns: 200, evmVersion: "cancun", viaIr: false },
    verdict: "NOT_READY",
    evaluatedAtMs: 1,
    items: [
      { id: "fork_dry_run", label: "Fork dry run", pass: false, reason: "fork dry-run did not produce a report: no fork dry run was produced for this bytecode yet", evidence: { counts: {}, outputSha256: null, durationMs: null, toolVersion: null } },
    ],
  },
} as unknown as ToolchainGateResult;

function ops(over: Partial<ContractForgeOps> = {}): ContractForgeOps {
  return {
    templateList: vi.fn(async () => CATALOG),
    templateRender: vi.fn(async (input) => ({
      template: input.template, tier: "T0", outDir: input.outDir, digest: "d", files: ["src/Token.sol", "foundry.toml"],
      params: { ...input.params, contract: "LemonDrops" }, medusaBudget: budget, contractDir: input.outDir,
      deps: [{ name: "openzeppelin-contracts", commit: "c", dir: "lib/openzeppelin-contracts", installed: false, note: "no library cache on this machine yet" }],
    })),
    toolchainSettings: vi.fn(async () => status(false)),
    toolchainSetEnabled: vi.fn(async (on: boolean) => status(on)),
    gateFromToolchain: vi.fn(async () => GATE),
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
  await act(async () => { await new Promise((r) => setTimeout(r, 0)); });
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

describe("Feature: build a contract from a template (US-6.4)", () => {
  it("shows the toolchain off by default with missing programs named, and turns it on", async () => {
    const o = ops();
    const el = await render(<ContractForgePanel ops={o} />);
    const toggle = $(el, "cf-tc-toggle") as HTMLInputElement;
    expect(toggle.checked).toBe(false);
    expect($(el, "cf-tc-medusa")?.textContent).toMatch(/not installed/);
    expect($(el, "cf-tc-forge")?.textContent).toMatch(/found/);
    await click(toggle);
    expect(o.toolchainSetEnabled).toHaveBeenCalledWith(true);
    expect(($(el, "cf-tc-toggle") as HTMLInputElement).checked).toBe(true);
  });

  it("renders the form from the catalog with defaults and sends only filled values", async () => {
    const o = ops();
    const el = await render(<ContractForgePanel ops={o} />);
    expect(($(el, "cf-field-supply") as HTMLInputElement).value).toBe("1000000");
    expect(el.textContent).toMatch(/Tier T0: Medusa runs 10,000 calls and needs 60% line coverage/);
    await act(async () => {
      type($(el, "cf-field-name"), "Lemon Drops");
      type($(el, "cf-field-symbol"), "LEMON");
      type($(el, "cf-field-owner"), OWNER);
      type($(el, "cf-outdir"), "/work/dapps/lemon");
    });
    await click($(el, "cf-render"));
    expect(o.templateRender).toHaveBeenCalledWith({
      template: "erc20",
      params: { name: "Lemon Drops", symbol: "LEMON", supply: "1000000", owner: OWNER },
      outDir: "/work/dapps/lemon",
    });
    expect($(el, "cf-rendered")?.textContent).toMatch(/Wrote 2 files/);
    expect($(el, "cf-rendered")?.textContent).toMatch(/Libraries not installed yet: openzeppelin-contracts/);
    // The gate step is prefilled with the new project and its artifact.
    expect(($(el, "cf-project") as HTMLInputElement).value).toBe("/work/dapps/lemon");
    expect(($(el, "cf-artifact") as HTMLInputElement).value).toBe("Token.sol/LemonDrops.json");
  });

  it("names missing required fields without calling core, and shows core's refusal", async () => {
    const o = ops({ templateRender: vi.fn(async () => { throw new Error("/x is not inside a folder you granted for writing. Grant the folder in Settings > Folder access first."); }) });
    const el = await render(<ContractForgePanel ops={o} />);
    await act(async () => { type($(el, "cf-outdir"), "/x"); });
    await click($(el, "cf-render"));
    expect($(el, "cf-render-error")?.textContent).toBe("Fill in: Name, Symbol, Owner address.");
    expect(o.templateRender).not.toHaveBeenCalled();
    await act(async () => {
      type($(el, "cf-field-name"), "A");
      type($(el, "cf-field-symbol"), "A");
      type($(el, "cf-field-owner"), OWNER);
    });
    await click($(el, "cf-render"));
    expect($(el, "cf-render-error")?.textContent).toMatch(/not inside a folder you granted/);
  });

  it("switching templates resets the form to that template's fields", async () => {
    const el = await render(<ContractForgePanel ops={ops()} />);
    const sel = $(el, "cf-template") as HTMLSelectElement;
    await act(async () => {
      sel.value = "erc1155";
      sel.dispatchEvent(new Event("change", { bubbles: true }));
    });
    expect($(el, "cf-field-name")).toBeNull();
    expect($(el, "cf-field-owner")).not.toBeNull();
  });

  it("runs the deploy gate on the session's toolchain runs and shows core's verdict and the tier check", async () => {
    const o = ops();
    const el = await render(<ContractForgePanel ops={o} />);
    await act(async () => {
      type($(el, "cf-session"), "s1-ab");
      type($(el, "cf-project"), "/work/dapps/lemon");
      type($(el, "cf-artifact"), "Token.sol/LemonDrops.json");
    });
    await click($(el, "cf-gate"));
    expect(o.gateFromToolchain).toHaveBeenCalledWith({ sessionId: "s1-ab", project: "/work/dapps/lemon", artifact: "Token.sol/LemonDrops.json", forkInCore: {} });
    expect($(el, "deploy-gate-verdict")?.textContent).toBe("NOT READY");
    expect($(el, "cf-medusa")?.textContent).toMatch(/T0: at least 10,000 calls/);
  });

  it("an unavailable catalog or toolchain says so instead of an empty form", async () => {
    const o = ops({
      templateList: vi.fn(async () => { throw new Error("the contract templates are not bundled in this build"); }),
      toolchainSettings: vi.fn(async () => { throw new Error("no hermes folder"); }),
    });
    const el = await render(<ContractForgePanel ops={o} />);
    expect($(el, "cf-cat-error")?.textContent).toMatch(/not bundled/);
    expect($(el, "cf-tc-error")?.textContent).toMatch(/no hermes folder/);
    expect($(el, "cf-template")).toBeNull();
  });
});
