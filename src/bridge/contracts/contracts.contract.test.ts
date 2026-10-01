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
