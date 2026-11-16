// Owner report (0.5.3): "the model router in the hermes agent should pull all of the models from the
// local and the APIs installed ... I have the OpenAI API installed and haven't been able to use it."
// D-11: the router lists the local models AND every model of every provider connected in
// Settings › Models (the six governed vendors, a confirmed custom endpoint); never the Cognition
// delegate, an unconnected or paused provider, or one this build has no adapter for.
import { describe, it, expect } from "vitest";
import { vendor } from "../surfaces/providers/fixtures/providerStates.fixture";
import { routerChoicesFor, cloudChoiceInput, LOCAL_ONLY_CLOUD_NOTE } from "./modelRouterSources";
import { activeRouterId, cloudChoiceId, resolveActive, canSelect, GATEWAY_ID } from "./modelRouter";
import type { ModelDescriptor } from "../bridge/domains";

const gemma: ModelDescriptor = { id: "loc1", source: "hf", repo: "ggml-org/gemma", file: "gemma-4-E4B-it-Q4_0.gguf", sizeBytes: 1, sha256: "abc", kind: "gguf" };
const connected = (id: string, name: string, models: string[]) => vendor({ id, name, keyConfigured: true, keySetMs: 1, listedModels: models });

const SIX = [
  connected("openai", "OpenAI", ["gpt-4o", "gpt-4o-mini"]),
  connected("anthropic", "Anthropic", ["claude-sonnet-4-5"]),
  connected("gemini", "Google Gemini", ["gemini-2.5-pro"]),
  connected("xai", "xAI", ["grok-4"]),
  connected("deepseek", "DeepSeek", ["deepseek-chat"]),
  connected("meta", "Meta", ["llama-4-maverick"]),
];

describe("model router: local + every connected provider's models (D-11 BYOK)", () => {
  it("lists the local model, all six vendors' models and the gateway terminal", () => {
    const choices = routerChoicesFor({ local: [gemma], registry: [], cloud: SIX });
    expect(choices.filter((c) => c.source === "local").map((c) => c.id)).toEqual(["loc1"]);
    expect(choices.filter((c) => c.source === "cloud").map((c) => c.id)).toEqual([
      cloudChoiceId("openai", "gpt-4o"),
      cloudChoiceId("openai", "gpt-4o-mini"),
      cloudChoiceId("anthropic", "claude-sonnet-4-5"),
      cloudChoiceId("gemini", "gemini-2.5-pro"),
      cloudChoiceId("xai", "grok-4"),
      cloudChoiceId("deepseek", "deepseek-chat"),
      cloudChoiceId("meta", "llama-4-maverick"),
    ]);
    const gpt = choices.find((c) => c.id === cloudChoiceId("openai", "gpt-4o"))!;
    expect(gpt).toMatchObject({ source: "cloud", ready: true, label: "OpenAI · gpt-4o", cloud: { providerId: "openai", model: "gpt-4o" } });
    expect(choices[choices.length - 1]).toMatchObject({ id: GATEWAY_ID, source: "gateway" });
  });

  it("an OpenAI key connected with only approved models still lists them (no duplicates)", () => {
    const choices = routerChoicesFor({ local: [], registry: [], cloud: [vendor({ keyConfigured: true, approvedModels: ["gpt-4o"], listedModels: ["gpt-4o", "o3"] })] });
    expect(choices.filter((c) => c.source === "cloud").map((c) => c.label)).toEqual(["OpenAI · gpt-4o", "OpenAI · o3"]);
  });

  it("leaves out what cannot take a call: no key, no adapter, paused, Cognition, an unconfirmed custom URL", () => {
    const choices = routerChoicesFor({
      local: [],
      registry: [],
      cloud: [
        vendor({ id: "xai", name: "xAI", keyConfigured: false, listedModels: ["grok-4"] }),
        { ...connected("anthropic", "Anthropic", ["claude"]), adapter: null },
        { ...connected("deepseek", "DeepSeek", ["deepseek-chat"]), halt: { kind: "burst", message: "paused", atMs: 1 } },
        { ...connected("cognition", "Cognition (Devin)", ["devin"]), kind: "delegate" },
        { ...connected("custom-a", "Box", ["m1"]), kind: "custom", confirmed: false },
        { ...connected("custom-b", "Lab", ["m2"]), kind: "custom", confirmed: true },
      ],
    });
    expect(choices.filter((c) => c.source === "cloud").map((c) => c.label)).toEqual(["Lab · m2 (unverified)"]);
  });

  it("under Local model only the cloud models are listed, not ready, and never served", () => {
    const choices = routerChoicesFor({ local: [gemma], registry: [], cloud: SIX }, "local_only");
    const gpt = choices.find((c) => c.id === cloudChoiceId("openai", "gpt-4o"))!;
    expect(gpt).toMatchObject({ ready: false, note: LOCAL_ONLY_CLOUD_NOTE });
    expect(resolveActive(gpt.id, choices).source).not.toBe("cloud");
  });

  it("a connected cloud model is selectable and is what the send path resolves (INV-Router-2/3)", () => {
    const choices = routerChoicesFor({ local: [gemma], registry: [], cloud: SIX });
    const id = cloudChoiceId("anthropic", "claude-sonnet-4-5");
    expect(canSelect(id, choices)).toBe(true);
    expect(resolveActive(id, choices)).toMatchObject({ source: "cloud", cloud: { providerId: "anthropic", model: "claude-sonnet-4-5" } });
  });

  it("the stored cloud model (core's preferred model) is the router's active selection", () => {
    expect(activeRouterId("loc1", { providerId: "openai", model: "gpt-4o" })).toBe(cloudChoiceId("openai", "gpt-4o"));
    expect(activeRouterId("loc1", null)).toBe("loc1");
  });

  it("no connected provider: no cloud choices (the web preview lists none)", () => {
    expect(cloudChoiceInput([])).toEqual([]);
    expect(routerChoicesFor({ local: [gemma], registry: [], cloud: [] }).some((c) => c.source === "cloud")).toBe(false);
  });
});
