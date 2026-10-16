// CX bridge contract test — contracts (Hermes P3 / WP3.2). Pins the deploy seam shape.
import { describe, it, expect, vi, beforeEach } from "vitest";
import { bridge } from "../index";

const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));
import { tauriContracts } from "../tauri/contracts";

describe("CX bridge — contracts domain (Hermes P3/WP3.2)", () => {
  it("exposes contracts.deploy", () => {
    expect(bridge.contracts).toBeDefined();
    expect(typeof bridge.contracts.deploy).toBe("function");
  });

  it("sim is honest — a deploy cannot happen without the desktop node (Rule 1)", async () => {
    if (bridge.mode === "sim") {
      await expect(bridge.contracts.deploy({ bytecodeHex: "0x6080" })).rejects.toThrow(/desktop node/i);
    }
  });
});

describe("CX bridge — tauri contracts invokes contract_deploy (P3/WP3.2)", () => {
  beforeEach(() => invokeMock.mockReset());

  it("deploy → contract_deploy with camelCase keys; omitted optionals become null", async () => {
    invokeMock.mockResolvedValueOnce({ id: "cer1", decoded: { action: "Deploy contract" } });
    await tauriContracts.deploy({ bytecodeHex: "0x6080604052" });
    expect(invokeMock).toHaveBeenCalledWith("contract_deploy", {
      bytecodeHex: "0x6080604052",
      constructorArgsHex: null,
      valueWei: null,
      gas: null,
    });
  });

  it("deploy → forwards constructor args, value, and gas when given", async () => {
    invokeMock.mockResolvedValueOnce({ id: "cer2", decoded: { action: "Deploy contract" } });
    await tauriContracts.deploy({
      bytecodeHex: "0x6080",
      constructorArgsHex: "0xabcd",
      valueWei: "1000000000000000000",
      gas: 3_000_000,
    });
    expect(invokeMock).toHaveBeenCalledWith("contract_deploy", {
      bytecodeHex: "0x6080",
      constructorArgsHex: "0xabcd",
      valueWei: "1000000000000000000",
      gas: 3_000_000,
    });
  });
});

describe("HUP-S6.4 — the D-4 deploy gate seam", () => {
  beforeEach(() => invokeMock.mockReset());

  it("gateLookup → deploy_gate_lookup with camelCase keys", async () => {
    invokeMock.mockResolvedValueOnce({ initcodeHash: "0xaa", record: null });
    const r = await tauriContracts.gateLookup("0x6080", undefined);
    expect(invokeMock).toHaveBeenCalledWith("deploy_gate_lookup", { bytecodeHex: "0x6080", constructorArgsHex: null });
    expect(r.record).toBeNull();
  });

  it("gateSubmit → deploy_gate_submit with the typed verifier inputs", async () => {
    invokeMock.mockResolvedValueOnce({ verdict: "NOT_READY" });
    const inputs = {
      bytecodeHex: "0x6000",
      compiler: { solcVersion: "0.8.28", optimizer: true, optimizerRuns: 200, evmVersion: "cancun", viaIr: false },
      forgeTests: { state: "notInstalled" as const },
      slither: { state: "notInstalled" as const },
      aderyn: { state: "notInstalled" as const },
      medusa: { run: { state: "notInstalled" as const }, callBudget: 50_000 },
      forkDryRun: { run: { state: "notInstalled" as const }, txInputHex: "0x6000", citratePrecompiles: "unknown" as const },
    };
    await tauriContracts.gateSubmit(inputs);
    expect(invokeMock).toHaveBeenCalledWith("deploy_gate_submit", { inputs });
  });

  it("gateSubmit carries forkInCore so core runs the Citrate-aware fork itself (HUP-S6.10)", async () => {
    invokeMock.mockResolvedValueOnce({ verdict: "NOT_READY" });
    const inputs = {
      bytecodeHex: "0x6000",
      compiler: { solcVersion: "0.8.36", optimizer: true, optimizerRuns: 200, evmVersion: "cancun", viaIr: false },
      forgeTests: { state: "notInstalled" as const },
      slither: { state: "notInstalled" as const },
      aderyn: { state: "notInstalled" as const },
      medusa: { run: { state: "notInstalled" as const }, callBudget: 50_000 },
      forkInCore: { stateRpc: "citrate", testMint: { quantity: 2, priceWei: "5000000000000000000" } },
    };
    await tauriContracts.gateSubmit(inputs);
    expect(invokeMock).toHaveBeenCalledWith("deploy_gate_submit", { inputs });
    const sent = invokeMock.mock.calls.at(-1)?.[1] as { inputs: Record<string, unknown> };
    expect(sent.inputs.forkDryRun).toBeUndefined();
  });

  it("gateForkDryRun → deploy_gate_fork_dry_run with the request (HUP-S6.10)", async () => {
    invokeMock.mockResolvedValueOnce({ run: { state: "notInstalled" }, txInputHex: "0x6000", citratePrecompiles: "unknown" });
    const request = {
      bytecodeHex: "0x6000",
      stateRpc: "http://127.0.0.1:8545",
      testMint: { quantity: 1, priceWei: "0" },
    };
    const r = await tauriContracts.gateForkDryRun(request);
    expect(invokeMock).toHaveBeenCalledWith("deploy_gate_fork_dry_run", { request });
    expect(r.run.state).toBe("notInstalled");
  });

  it("sim is honest — the fork dry run needs the desktop node", async () => {
    if (bridge.mode === "sim") {
      await expect(bridge.contracts.gateForkDryRun({ bytecodeHex: "0x6000" })).rejects.toThrow(/desktop node/i);
    }
  });

  it("sim is honest — no gate runs without the desktop node", async () => {
    if (bridge.mode === "sim") {
      await expect(bridge.contracts.gateLookup("0x6080")).rejects.toThrow(/desktop node/i);
    }
  });
});

describe("HUP-S4.3 — verified source seam", () => {
  beforeEach(() => invokeMock.mockReset());

  it("verifiedSource → contract_verified_source with the address", async () => {
    invokeMock.mockResolvedValueOnce({ status: "unverified", verified: false });
    const r = await tauriContracts.verifiedSource("0x" + "a".repeat(40));
    expect(invokeMock).toHaveBeenCalledWith("contract_verified_source", { address: "0x" + "a".repeat(40) });
    expect(r.status).toBe("unverified");
  });

  it("sim is honest — the lookup runs in the desktop node", async () => {
    if (bridge.mode === "sim") {
      await expect(bridge.contracts.verifiedSource("0x" + "a".repeat(40))).rejects.toThrow(/desktop node/i);
    }
  });
});

describe("HUP-S6.6 / S6.7 — the Contract reader and post-deploy seams", () => {
  beforeEach(() => invokeMock.mockReset());
  const ADDR = "0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed";

  it("reader calls map to their commands with camelCase keys", async () => {
    invokeMock.mockResolvedValue(null);
    await tauriContracts.source(ADDR);
    expect(invokeMock).toHaveBeenLastCalledWith("contract_source", { address: ADDR });
    await tauriContracts.codeSize("citrate", ADDR);
    expect(invokeMock).toHaveBeenLastCalledWith("contract_code_size", { target: "citrate", address: ADDR });
    await tauriContracts.viewCall("http://127.0.0.1:8545", ADDR, "0x06fdde03");
    expect(invokeMock).toHaveBeenLastCalledWith("contract_view_call", { target: "http://127.0.0.1:8545", address: ADDR, calldata: "0x06fdde03" });
    await tauriContracts.proposeWrite(ADDR, "0xa0712d68", "0");
    expect(invokeMock).toHaveBeenLastCalledWith("contract_write_propose", { address: ADDR, calldata: "0xa0712d68", valueWei: "0" });
  });

  it("post-deploy calls map to their commands", async () => {
    invokeMock.mockResolvedValue(null);
    await tauriContracts.postdeployStatus("/p");
    expect(invokeMock).toHaveBeenLastCalledWith("postdeploy_status", { projectDir: "/p" });
    await tauriContracts.postdeployReceipt("0xabc");
    expect(invokeMock).toHaveBeenLastCalledWith("postdeploy_receipt", { txHash: "0xabc" });
    await tauriContracts.postdeployVerify("/p", ADDR);
    expect(invokeMock).toHaveBeenLastCalledWith("postdeploy_verify", { projectDir: "/p", address: ADDR, constructorArgsHex: null });
    await tauriContracts.postdeploySwitchSite("/p", ADDR);
    expect(invokeMock).toHaveBeenLastCalledWith("postdeploy_switch_site", { projectDir: "/p", address: ADDR });
    await tauriContracts.postdeployPinSite("/p");
    expect(invokeMock).toHaveBeenLastCalledWith("postdeploy_pin_site", { projectDir: "/p" });
    await tauriContracts.postdeployVercelExport("/p");
    expect(invokeMock).toHaveBeenLastCalledWith("postdeploy_vercel_export", { projectDir: "/p" });
  });

  it("sim is honest — reads and post-deploy steps need the desktop node", async () => {
    if (bridge.mode === "sim") {
      await expect(bridge.contracts.source(ADDR)).rejects.toThrow(/desktop node/i);
      await expect(bridge.contracts.proposeWrite(ADDR, "0xa0712d68", "0")).rejects.toThrow(/desktop node/i);
      await expect(bridge.contracts.postdeployPinSite("/p")).rejects.toThrow(/desktop node/i);
    }
  });
  it("forge calls map to their commands (HUP-S6.2 / S6.3)", async () => {
    invokeMock.mockResolvedValue(null);
    await tauriContracts.templateList();
    expect(invokeMock).toHaveBeenLastCalledWith("template_list");
    const input = { template: "erc20", params: { name: "Lemon Drops" }, outDir: "/p/lemon" };
    await tauriContracts.templateRender(input);
    expect(invokeMock).toHaveBeenLastCalledWith("template_render", { input });
    await tauriContracts.toolchainSettings();
    expect(invokeMock).toHaveBeenLastCalledWith("toolchain_settings_get");
    await tauriContracts.toolchainSetEnabled(true);
    expect(invokeMock).toHaveBeenLastCalledWith("toolchain_settings_set", { settings: { enabled: true } });
    const request = { sessionId: "s1-ab", project: "/p/lemon", artifact: "Token.sol/LemonDrops.json" };
    await tauriContracts.gateFromToolchain(request);
    expect(invokeMock).toHaveBeenLastCalledWith("deploy_gate_submit_toolchain", { request });
  });

  it("sim is honest — templates, the toolchain and the gate need the desktop node", async () => {
    if (bridge.mode === "sim") {
      await expect(bridge.contracts.templateList()).rejects.toThrow(/desktop node/i);
      await expect(bridge.contracts.toolchainSetEnabled(true)).rejects.toThrow(/desktop node/i);
      await expect(bridge.contracts.gateFromToolchain({ sessionId: "s", project: "/p", artifact: "A.sol/A.json" })).rejects.toThrow(/desktop node/i);
    }
  });
});
