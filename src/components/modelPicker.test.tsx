// Hermes ModelPicker (P0 / WP0.3).
import { describe, it, expect, vi } from "vitest";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { renderToStaticMarkup } from "react-dom/server";
import { ModelPicker, isSelectedChoice } from "./ModelPicker";
import type { ModelChoice } from "../agent/modelRouter";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const CHOICES: ModelChoice[] = [
  { id: "loc1", label: "gemma-4-E4B-it-Q4_0.gguf", source: "local", ready: true },
  { id: "reg1", label: "Registry model", source: "registry", ready: false },
  { id: "gateway", label: "Citrate gateway", source: "gateway", ready: true },
];

describe("isSelectedChoice — null active defaults to the gateway", () => {
  it("selects the explicit active id", () => {
    expect(isSelectedChoice(CHOICES[0], "loc1")).toBe(true);
    expect(isSelectedChoice(CHOICES[2], "loc1")).toBe(false);
  });
  it("selects the gateway when nothing is active (out of the box)", () => {
    expect(isSelectedChoice(CHOICES[2], null)).toBe(true);
    expect(isSelectedChoice(CHOICES[0], null)).toBe(false);
  });
});

describe("ModelPicker — render", () => {
  it("lists every choice with its label", () => {
    const html = renderToStaticMarkup(<ModelPicker choices={CHOICES} activeId="loc1" onSelect={() => {}} />);
    expect(html).toContain("gemma-4-E4B-it-Q4_0.gguf");
    expect(html).toContain("Registry model");
    expect(html).toContain("Citrate gateway");
  });
  it("marks the active choice aria-selected, others not", () => {
    const html = renderToStaticMarkup(<ModelPicker choices={CHOICES} activeId="loc1" onSelect={() => {}} />);
    // exactly one aria-selected="true"
    expect((html.match(/aria-selected="true"/g) || []).length).toBe(1);
  });
  it("defaults selection to the gateway when activeId is null", () => {
    const html = renderToStaticMarkup(<ModelPicker choices={CHOICES} activeId={null} onSelect={() => {}} />);
    // the gateway row (data-source=gateway) carries aria-selected="true"
    expect(html).toMatch(/data-source="gateway"[^>]*aria-selected="true"|aria-selected="true"[^>]*data-source="gateway"/);
  });
  it("shows an honest not-ready hint for a not-downloaded registry model", () => {
    const html = renderToStaticMarkup(<ModelPicker choices={CHOICES} activeId="gateway" onSelect={() => {}} />);
    expect(html).toContain("pull to use");
  });
});

describe("ModelPicker — selection", () => {
  it("calls onSelect with the choice id on click", async () => {
    const onSelect = vi.fn();
    const host = document.createElement("div");
    document.body.appendChild(host);
    const root = createRoot(host);
    await act(async () => { root.render(<ModelPicker choices={CHOICES} activeId={null} onSelect={onSelect} />); });
    const regBtn = Array.from(host.querySelectorAll("button")).find((b) => /Registry model/.test(b.textContent ?? ""));
    expect(regBtn).toBeTruthy();
    await act(async () => { regBtn!.click(); });
    expect(onSelect).toHaveBeenCalledWith("reg1");
    root.unmount();
    host.remove();
  });
});

// D-11 (owner report 0.5.3): BYOK cloud models are rows in the same picker, marked as the member's
// own key, selected while core holds them as the preferred model; under Local model only they say why.
describe("ModelPicker — cloud (BYOK) rows", () => {
  const cloud: ModelChoice[] = [
    { id: "loc1", label: "gemma.gguf", source: "local", ready: true },
    { id: "cloud:openai::gpt-4o", label: "OpenAI · gpt-4o", source: "cloud", ready: true, cloud: { providerId: "openai", model: "gpt-4o" } },
    { id: "cloud:xai::grok-4", label: "xAI · grok-4", source: "cloud", ready: false, cloud: { providerId: "xai", model: "grok-4" }, note: "Local model only is on" },
    { id: "gateway", label: "Citrate gateway", source: "gateway", ready: true },
  ];
  it("renders a connected cloud model as selectable and selected when active", () => {
    const html = renderToStaticMarkup(<ModelPicker choices={cloud} activeId="cloud:openai::gpt-4o" onSelect={() => {}} />);
    expect(html).toContain("OpenAI · gpt-4o");
    expect(html).toContain("your key");
    expect(html).toMatch(/aria-selected="true"[^>]*data-source="cloud"/);
  });
  it("a not-ready cloud model shows why", () => {
    const html = renderToStaticMarkup(<ModelPicker choices={cloud} activeId={null} onSelect={() => {}} />);
    expect(html).toContain("Local model only is on");
  });
});
