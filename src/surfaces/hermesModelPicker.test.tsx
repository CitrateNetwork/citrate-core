// D-11 (P-5) — the Hermes model picker lists only cloud models that can run now (a stored key, an
// adapter, no halt, a confirmed custom URL; never the Cognition delegate), keeps a stored choice
// visible when its provider is gone, and says when Local model only keeps Hermes local.
import { describe, it, expect } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import type { ProviderStatus } from "../bridge/domains";
import { HermesModelPickerView, cloudModelOptions, type HermesModelPickerViewProps } from "./HermesModelPicker";

const p = (over: Partial<ProviderStatus>): ProviderStatus => ({
  id: "openai",
  name: "OpenAI",
  kind: "vendor",
  verified: true,
  baseUrl: "https://api.openai.com/v1",
  host: "api.openai.com",
  adapter: "openai-compatible",
  keyConfigured: true,
  keySetMs: 1,
  keyPage: null,
  revokePage: null,
  confirmed: true,
  halt: null,
  caps: { dailyTokens: null, monthlyTokens: null, dailyMicros: null, monthlyMicros: null },
  approvedModels: [],
  listedModels: ["gpt-4o"],
  prices: {},
  ...over,
});

const noop = () => {};
const render = (over: Partial<HermesModelPickerViewProps>) =>
  renderToStaticMarkup(<HermesModelPickerView value={null} options={[]} error={null} route="ask_first" onChoose={noop} {...over} />);

describe("Hermes model picker (D-11)", () => {
  it("offers only models that can run now", () => {
    const opts = cloudModelOptions([
      p({ approvedModels: ["gpt-mine"], listedModels: ["gpt-4o", "gpt-mine"] }),
      p({ id: "xai", name: "xAI", keyConfigured: false }),
      p({ id: "anthropic", name: "Anthropic", adapter: null }),
      p({ id: "deepseek", name: "DeepSeek", halt: { kind: "burst", message: "paused", atMs: 1 } }),
      p({ id: "cognition", name: "Cognition (Devin)", kind: "delegate", listedModels: ["devin"] }),
      p({ id: "custom-a", name: "Box", kind: "custom", confirmed: false, listedModels: [], approvedModels: ["m1"] }),
      p({ id: "custom-b", name: "Lab", kind: "custom", confirmed: true, listedModels: [], approvedModels: ["m2"] }),
    ]);
    expect(opts.map((o) => `${o.providerId}/${o.model}/${o.unverified}`)).toEqual(["openai/gpt-mine/false", "openai/gpt-4o/false", "custom-b/m2/true"]);
  });

  it("defaults to the local model and marks a custom endpoint unverified", () => {
    const html = render({ options: cloudModelOptions([p({ id: "custom-b", name: "Lab", kind: "custom", listedModels: ["m2"] })]) });
    expect(html).toContain("Local model on this computer");
    expect(html).toContain("Lab · m2 (custom endpoint, unverified)");
    expect(html).toContain("never given to the agent");
  });

  it("a stored choice whose provider is gone stays visible and says so", () => {
    const html = render({ value: { providerId: "xai", model: "grok" }, options: [] });
    expect(html).toContain("xai · grok (not connected now)");
    expect(html).toContain("is not connected now");
  });

  it("Local model only keeps Hermes local, and says so", () => {
    const html = render({ value: { providerId: "openai", model: "gpt-4o" }, options: cloudModelOptions([p({})]), route: "local_only" });
    expect(html).toContain("Local model only is on, so Hermes uses the local model");
  });

  it("an unreadable status or none connected is said plainly", () => {
    expect(render({ options: null, error: "cloud providers run in the desktop app" })).toContain("Cloud models are not available here");
    expect(render({ options: [] })).toContain("No cloud model is connected yet");
  });
});
