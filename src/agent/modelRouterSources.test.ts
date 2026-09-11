// Hermes ModelRouter live-source adapters (P0 / WP0.2).
import { describe, it, expect } from "vitest";
import { modelLabel, gatewayConfigured, liveEnumerateInput, choicesFromSources } from "./modelRouterSources";
import { resolveActive, GATEWAY_ID } from "./modelRouter";
import type { ModelDescriptor, AiProviderStatus } from "../bridge/domains";

const md = (over: Partial<ModelDescriptor>): ModelDescriptor => ({
  id: "loc1",
  source: "hf",
  repo: "ggml-org/gemma-4-E4B-it-GGUF",
  file: "gemma-4-E4B-it-Q4_0.gguf",
  sizeBytes: 1,
  sha256: "abc",
  kind: "gguf",
  ...over,
});

describe("modelLabel — readable label from a descriptor", () => {
  it("prefers the GGUF filename, then repo, then id", () => {
    expect(modelLabel(md({}))).toBe("gemma-4-E4B-it-Q4_0.gguf");
    expect(modelLabel(md({ file: "" }))).toBe("ggml-org/gemma-4-E4B-it-GGUF");
    expect(modelLabel(md({ file: "", repo: "" }))).toBe("loc1");
  });
});

describe("gatewayConfigured — label hint only, never the fallback readiness", () => {
  const prov = (over: Partial<AiProviderStatus>): AiProviderStatus =>
    ({ id: "gateway", baseURL: "https://gw", model: "m", configured: false, isDefault: true, ...over }) as AiProviderStatus;
  it("true iff some provider is configured", () => {
    expect(gatewayConfigured([prov({ configured: true })])).toBe(true);
    expect(gatewayConfigured([prov({ configured: false })])).toBe(false);
    expect(gatewayConfigured([])).toBe(false);
  });
});

describe("liveEnumerateInput / choicesFromSources — wire local + gateway (registry TODO)", () => {
  it("maps local descriptors to ready choices and always includes a ready gateway terminal", () => {
    const choices = choicesFromSources([md({ id: "loc1" }), md({ id: "loc2", file: "other.gguf" })]);
    expect(choices.find((c) => c.id === "loc1")).toMatchObject({ source: "local", ready: true, label: "gemma-4-E4B-it-Q4_0.gguf" });
    expect(choices.find((c) => c.id === "loc2")).toMatchObject({ source: "local", ready: true, label: "other.gguf" });
    const gw = choices.find((c) => c.source === "gateway")!;
    expect(gw).toMatchObject({ id: GATEWAY_ID, ready: true });
  });

  it("registry defaults empty (no on-chain READ yet — WP0.2b)", () => {
    const choices = choicesFromSources([md({})]);
    expect(choices.some((c) => c.source === "registry")).toBe(false);
  });

  it("surfaces registry choices (not ready) when provided", () => {
    const choices = choicesFromSources([], [{ id: "reg1", label: "Registry model" }]);
    expect(choices.find((c) => c.id === "reg1")).toMatchObject({ source: "registry", ready: false });
  });

  it("INV-Router-2 end-to-end: a not-ready registry pick resolves to the ready gateway terminal", () => {
    const choices = choicesFromSources([], [{ id: "reg1", label: "R" }]);
    const resolved = resolveActive("reg1", choices);
    expect(resolved.id).toBe(GATEWAY_ID);
    expect(resolved.ready).toBe(true);
  });
});
