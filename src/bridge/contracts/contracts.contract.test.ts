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
