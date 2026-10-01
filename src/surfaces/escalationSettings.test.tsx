// HUP-S1.5 — Settings › Escalation renders only what core reports (Rule 1): endpoints without keys,
// today's budget, the zero-budget default flagged as pending owner sign-off, the registry route as
// not available with what is missing, and form validation a person can act on.
import { describe, it, expect } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { EMPTY_FORM, EscalationSettingsView, formToInput, type EscalationViewProps } from "./EscalationSettings";

const noop = () => {};
const base: EscalationViewProps = {
  desktop: true,
  endpoints: [],
  budget: {
    capMicros: 0,
    usedMicros: 0,
    remainingMicros: 0,
    confirmedMicros: 0,
    periodStartMs: 1_727_740_800_000,
    periodEndMs: 1_727_827_200_000,
    maxCapMicros: 100_000_000,
    unreadable: false,
    history: [],
  },
  registry: { enabled: false, reason: "Registry escalation is not deployed yet. Escalations use your own endpoints.", missing: ["InferenceRouter is not deployed on chain 40204 yet"] },
  form: EMPTY_FORM,
  capInput: "",
  busy: false,
  error: null,
  onForm: noop,
  onAdd: noop,
  onRemove: noop,
  onCapInput: noop,
  onSetCap: noop,
};

const render = (p: Partial<EscalationViewProps> = {}) => renderToStaticMarkup(<EscalationSettingsView {...base} {...p} />);

describe("Settings › Escalation (HUP-S1.5)", () => {
  it("with no endpoints: says escalation is not offered, and the $0 default asks every time (pending owner sign-off)", () => {
    const html = render();
    expect(html).toContain("Hermes does not offer escalation until you add one");
    expect(html).toContain("every escalation asks you first");
    expect(html).toContain("pending owner sign-off");
  });

  it("lists endpoints by destination and price, never a key", () => {
    const html = render({
      endpoints: [{ id: "ep-1", label: "Planner", baseUrl: "https://api.example.com/v1", model: "big", inputMicrosPerMtok: 3_000_000, outputMicrosPerMtok: 15_000_000, destination: "Planner · api.example.com" }],
    });
    expect(html).toContain("Planner · api.example.com");
    expect(html).toContain("$3.00 in / $15.00 out per 1M tokens");
    expect(html).toContain("key sealed");
    expect(html).not.toMatch(/sk-/);
  });

  it("shows today's use from core's ledger and member-approved spend separately", () => {
    const html = render({ budget: { ...base.budget!, capMicros: 500_000, usedMicros: 120_000, remainingMicros: 380_000, confirmedMicros: 40_000 } });
    expect(html).toContain("$0.12 used of $0.50 today · $0.38 left");
    expect(html).toContain("$0.04 more approved by you one at a time");
    expect(html).not.toContain("every escalation asks you first");
  });

  it("an unreadable ledger is said out loud", () => {
    expect(render({ budget: { ...base.budget!, unreadable: true } })).toContain("could not be read, so every escalation asks");
  });

  it("the registry route is shown as not available, with what is missing", () => {
    const html = render();
    expect(html).toContain("not available yet");
    expect(html).toContain("InferenceRouter is not deployed on chain 40204 yet");
  });

  it("the web preview disables adding (no OS keyring)", () => {
    const html = render({ desktop: false });
    expect(html).toContain("only be added in the desktop app");
    expect(html).toMatch(/<button[^>]*disabled[^>]*>Add endpoint/);
  });

  it("no em-dashes in the visible copy", () => {
    expect(render()).not.toContain("—");
  });
});

describe("formToInput", () => {
  const good = { label: "Planner", baseUrl: "https://api.example.com/v1", model: "big", inputUsd: "3", outputUsd: "15", apiKey: "sk-1" };
  it("converts dollars per million tokens to micro-USD", () => {
    const r = formToInput(good);
    expect("input" in r && r.input).toEqual({ label: "Planner", baseUrl: "https://api.example.com/v1", model: "big", inputMicrosPerMtok: 3_000_000, outputMicrosPerMtok: 15_000_000 });
  });
  it("refuses remote plain http, missing prices, or a missing key", () => {
    expect(formToInput({ ...good, baseUrl: "http://api.example.com/v1" })).toHaveProperty("error");
    expect(formToInput({ ...good, baseUrl: "http://127.0.0.1:1234/v1" })).not.toHaveProperty("error");
    expect(formToInput({ ...good, outputUsd: "lots" })).toHaveProperty("error");
    expect(formToInput({ ...good, apiKey: " " })).toHaveProperty("error");
    expect(formToInput({ ...good, label: "" })).toHaveProperty("error");
  });
});
