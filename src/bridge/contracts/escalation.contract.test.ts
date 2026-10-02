// HUP-S1.5 bridge contract — the escalation router (endpoints, budget, quote, run, registry status).
import { describe, it, expect, vi, beforeEach } from "vitest";
import { bridge } from "../index";

const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));
import { tauriEscalation } from "../tauri/escalation";

describe("bridge contract — escalation (HUP-S1.5)", () => {
  it("exposes the escalation domain", () => {
    for (const k of ["endpoints", "addEndpoint", "removeEndpoint", "budget", "setBudget", "quote", "confirmPrepare", "run", "registryStatus"] as const) {
      expect(typeof bridge.escalation[k]).toBe("function");
    }
  });

  it("sim mode is honest (Rule 1): no endpoints, no key custody, no runs, registry disabled", async () => {
    if (bridge.mode !== "sim") return;
    expect(await bridge.escalation.endpoints()).toEqual([]);
    await expect(bridge.escalation.addEndpoint({ label: "x", baseUrl: "https://x.example/v1", model: "m", inputMicrosPerMtok: 1, outputMicrosPerMtok: 1 }, "k")).rejects.toThrow(/desktop app/);
    await expect(bridge.escalation.run("q", 1, null, false)).rejects.toThrow(/desktop app/);
    await expect(bridge.escalation.confirmPrepare("q", 1)).rejects.toThrow(/desktop app/);
    expect((await bridge.escalation.registryStatus()).enabled).toBe(false);
  });
});

describe("tauri escalation invokes the registered commands", () => {
  beforeEach(() => invokeMock.mockReset());

  it("endpoints / budget / registryStatus take no args", async () => {
    invokeMock.mockResolvedValue([]);
    await tauriEscalation.endpoints();
    expect(invokeMock).toHaveBeenLastCalledWith("escalation_endpoints");
    await tauriEscalation.budget();
    expect(invokeMock).toHaveBeenLastCalledWith("escalation_budget");
    await tauriEscalation.registryStatus();
    expect(invokeMock).toHaveBeenLastCalledWith("escalation_registry_status");
  });

  it("addEndpoint → escalation_endpoint_add with { input, apiKey }", async () => {
    const input = { label: "Mine", baseUrl: "https://api.example.com/v1", model: "big", inputMicrosPerMtok: 3_000_000, outputMicrosPerMtok: 15_000_000 };
    invokeMock.mockResolvedValueOnce({ ...input, id: "ep-1", destination: "Mine · api.example.com" });
    await tauriEscalation.addEndpoint(input, "sk-1");
    expect(invokeMock).toHaveBeenCalledWith("escalation_endpoint_add", { input, apiKey: "sk-1" });
  });

  it("removeEndpoint / setBudget pass their single argument", async () => {
    invokeMock.mockResolvedValue(undefined);
    await tauriEscalation.removeEndpoint("ep-1");
    expect(invokeMock).toHaveBeenLastCalledWith("escalation_endpoint_remove", { id: "ep-1" });
    await tauriEscalation.setBudget(250_000);
    expect(invokeMock).toHaveBeenLastCalledWith("escalation_budget_set", { capMicros: 250_000 });
  });

  it("quote → escalation_quote; omitted optionals become null", async () => {
    invokeMock.mockResolvedValueOnce({});
    await tauriEscalation.quote("ep-1", "plan it");
    expect(invokeMock).toHaveBeenCalledWith("escalation_quote", { endpointId: "ep-1", prompt: "plan it", system: null, maxTokens: null });
  });

  it("confirmPrepare → escalation_confirm_prepare with the quote and the shown price", async () => {
    invokeMock.mockResolvedValueOnce({ confirmId: "c-1", quoteId: "q-1", costMicros: 1234, expiresMs: 0 });
    await tauriEscalation.confirmPrepare("q-1", 1234);
    expect(invokeMock).toHaveBeenCalledWith("escalation_confirm_prepare", { quoteId: "q-1", shownCostMicros: 1234 });
  });

  it("run → escalation_run with the shown price, core's confirmation id and the taint flag", async () => {
    invokeMock.mockResolvedValueOnce({});
    await tauriEscalation.run("q-1", 1234, "c-1", false);
    expect(invokeMock).toHaveBeenCalledWith("escalation_run", { quoteId: "q-1", shownCostMicros: 1234, confirmId: "c-1", tainted: false });
  });
});
