// HUP-S3.3 + S3.7 (US-3.3) — the persona picker in Settings and onboarding: the shipped personas
// from the sidecar (names labeled as placeholders pending owner sign-off), the default voice, the
// member's custom personas, each track's workflows with how they are judged, and a custom-persona
// form whose save runs the sidecar's check.
import { describe, it, expect, vi } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { PersonaPicker, type PersonaApi } from "./PersonaPicker";
import type { HermesPersona, TrackWorkflow } from "../bridge/domains";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const GRAFT: HermesPersona = {
  id: "builder",
  role: "Builder",
  name: "Graft",
  name_status: "placeholder, pending owner sign-off",
  summary: "Ships code and dApps.",
  voice: "Direct and terse.",
  tone: "Practical.",
  style_rules: ["Lead with the change."],
  default_track: "full-project",
  default_workflow: "hello-mint",
  tool_emphasis: ["forge_test"],
  skills: ["solidity"],
  tts_voice: null,
  prompt_fragment: "## Persona: Graft\n",
  name_pending_sign_off: true,
  custom: false,
};
const PITH: HermesPersona = { ...GRAFT, id: "auditor", role: "Auditor", name: "Pith", default_track: "smart-contract", default_workflow: "audit-a-contract" };
const OWL: HermesPersona = { ...GRAFT, id: "custom-night-owl", role: "Custom", name: "Night Owl", name_status: "owner-approved", name_pending_sign_off: false, custom: true, default_track: "code", default_workflow: "code-change" };

const WF = (id: string, track: string, is_default: boolean, evidence: string): TrackWorkflow => ({
  id,
  track,
  title: id,
  summary: id + " summary",
  is_default,
  evidence,
  tools: [],
  verifier_names: [],
  steps: [],
});
const WORKFLOWS: TrackWorkflow[] = [
  WF("hello-mint", "full-project", true, "tool-report"),
  WF("launch-checklist", "full-project", false, "answer-shape"),
  WF("code-change", "code", true, "answer-shape"),
  WF("contract-build", "smart-contract", true, "tool-report"),
  WF("audit-a-contract", "smart-contract", false, "tool-report"),
];

function api(over: Partial<PersonaApi> = {}): PersonaApi {
  return {
    personas: vi.fn(async () => [GRAFT, PITH]),
    workflows: vi.fn(async () => WORKFLOWS),
    personaCheck: vi.fn(async (p) => ({ ...OWL, id: p.id, name: p.name, style_rules: p.style_rules, default_track: p.default_track })),
    ...over,
  };
}

type Props = Parameters<typeof PersonaPicker>[0];
function props(over: Partial<Props> = {}): Props {
  return {
    api: api(),
    chosen: null,
    custom: [],
    onChoose: vi.fn(),
    onAddCustom: vi.fn(),
    onRemoveCustom: vi.fn(),
    ...over,
  };
}

async function mount(p: Props): Promise<{ host: HTMLDivElement; root: Root }> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(<PersonaPicker {...p} />);
  });
  await act(async () => {});
  return { host, root };
}
const q = <T extends Element = HTMLElement>(host: HTMLElement, id: string) => host.querySelector(`[data-testid="${id}"]`) as T | null;
async function click(el: Element | null) {
  expect(el).toBeTruthy();
  await act(async () => {
    (el as HTMLElement).click();
  });
  await act(async () => {});
}
async function type(el: Element | null, value: string) {
  expect(el).toBeTruthy();
  const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
  const setter = Object.getOwnPropertyDescriptor(proto, "value")?.set;
  await act(async () => {
    setter?.call(el, value);
    el!.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
  });
}

describe("PersonaPicker", () => {
  it("lists the default voice and the shipped personas, names labeled as placeholders", async () => {
    const { host } = await mount(props());
    expect(q(host, "persona-option-default")).toBeTruthy();
    expect(q<HTMLInputElement>(host, "persona-radio-default")?.checked).toBe(true);
    expect(host.textContent).toContain("Graft");
    expect(host.textContent).toContain("pending owner sign-off");
    expect(host.textContent).toContain("Direct and terse.");
    expect(q(host, "persona-option-auditor")).toBeTruthy();
  });

  it("choosing a persona hands its sidecar view to the store; the default voice hands null", async () => {
    const p = props();
    const { host } = await mount(p);
    await click(q(host, "persona-radio-builder"));
    expect(p.onChoose).toHaveBeenCalledWith(GRAFT);
    const p2 = props({ chosen: GRAFT });
    const { host: h2 } = await mount(p2);
    await click(q(h2, "persona-radio-default"));
    expect(p2.onChoose).toHaveBeenLastCalledWith(null);
  });

  it("shows the chosen persona's default track and its workflows with how they are judged", async () => {
    const { host } = await mount(props({ chosen: GRAFT }));
    const wf = q(host, "persona-workflows")?.textContent ?? "";
    expect(wf).toContain("hello-mint");
    expect(wf).toContain("launch-checklist");
    expect(wf).toContain("checked by tool reports");
    expect(wf).toContain("checked by answer shape");
    expect(wf.indexOf("hello-mint")).toBeLessThan(wf.indexOf("launch-checklist"));
  });

  it("without the sidecar it says so, keeps the custom personas and the saved choice", async () => {
    const down = api({
      personas: vi.fn(async () => {
        throw new Error("hermes is not running (no session bearer)");
      }),
      workflows: vi.fn(async () => {
        throw new Error("hermes is not running (no session bearer)");
      }),
    });
    const { host } = await mount(props({ api: down, custom: [OWL], chosen: GRAFT }));
    expect(q(host, "persona-unavailable")?.textContent).toMatch(/Hermes sidecar/);
    expect(q(host, "persona-option-custom-night-owl")).toBeTruthy();
    // The saved shipped choice still shows (its fragment was saved with it).
    expect(q<HTMLInputElement>(host, "persona-radio-builder")?.checked).toBe(true);
  });

  it("a custom persona is checked by the sidecar, then saved", async () => {
    const p = props();
    const { host } = await mount(p);
    await click(q(host, "persona-custom-open"));
    await type(q(host, "persona-custom-name"), "Night Owl");
    await type(q(host, "persona-custom-summary"), "Late-night pair programmer.");
    await type(q(host, "persona-custom-voice"), "Quiet.");
    await type(q(host, "persona-custom-tone"), "Dry.");
    await type(q(host, "persona-custom-rules"), "Lead with the answer.\nNo exclamation marks.");
    await type(q(host, "persona-custom-track"), "code");
    await click(q(host, "persona-custom-save"));
    expect(p.api.personaCheck).toHaveBeenCalledTimes(1);
    const sent = (p.api.personaCheck as ReturnType<typeof vi.fn>).mock.calls[0][0];
    expect(sent.id).toBe("custom-night-owl");
    expect(sent.style_rules).toEqual(["Lead with the answer.", "No exclamation marks."]);
    expect(p.onAddCustom).toHaveBeenCalledTimes(1);
  });

  it("an invalid form or a sidecar refusal shows the reason inline and saves nothing", async () => {
    const refusing = api({
      personaCheck: vi.fn(async () => {
        throw new Error("PERSONA_REFUSED: no track \"nope\"");
      }),
    });
    const p = props({ api: refusing });
    const { host } = await mount(p);
    await click(q(host, "persona-custom-open"));
    await click(q(host, "persona-custom-save"));
    expect(q(host, "persona-custom-error")?.textContent).toMatch(/name/i);
    expect(refusing.personaCheck).not.toHaveBeenCalled();
    await type(q(host, "persona-custom-name"), "Night Owl");
    await type(q(host, "persona-custom-summary"), "s");
    await type(q(host, "persona-custom-voice"), "v");
    await type(q(host, "persona-custom-tone"), "t");
    await type(q(host, "persona-custom-rules"), "r");
    await type(q(host, "persona-custom-track"), "code");
    await click(q(host, "persona-custom-save"));
    expect(q(host, "persona-custom-error")?.textContent).toBe("no track \"nope\"");
    expect(p.onAddCustom).not.toHaveBeenCalled();
  });

  it("a saved shipped choice picks up a rename from the sidecar", async () => {
    const renamed = { ...GRAFT, name: "Scion", prompt_fragment: "## Persona: Scion\n" };
    const p = props({ chosen: GRAFT, api: api({ personas: vi.fn(async () => [renamed, PITH]) }) });
    await mount(p);
    expect(p.onChoose).toHaveBeenCalledWith(renamed);
  });

  it("an unchanged saved choice is left alone", async () => {
    const p = props({ chosen: GRAFT });
    await mount(p);
    expect(p.onChoose).not.toHaveBeenCalled();
  });

  it("a custom persona can be removed", async () => {
    const p = props({ custom: [OWL] });
    const { host } = await mount(p);
    await click(q(host, "persona-remove-custom-night-owl"));
    expect(p.onRemoveCustom).toHaveBeenCalledWith("custom-night-owl");
  });

  it("compact (onboarding) shows the choice only, and says it can change later", async () => {
    const { host } = await mount(props({ compact: true }));
    expect(q(host, "persona-custom-open")).toBeNull();
    expect(q(host, "persona-workflows")).toBeNull();
    expect(host.textContent).toMatch(/change this later in Settings/);
  });

  it("uses no em-dashes in what it shows", async () => {
    const { host } = await mount(props({ chosen: GRAFT, custom: [OWL] }));
    expect(host.textContent).not.toContain("—");
  });

  it("says a speech voice id is stored but not used by speech yet (no TTS wiring in this lane)", async () => {
    const withVoice = { ...GRAFT, tts_voice: "en-calm" };
    const { host } = await mount(props({ api: api({ personas: vi.fn(async () => [withVoice]) }) }));
    expect(q(host, "persona-option-builder")?.textContent).toContain("en-calm (stored, not used by speech yet)");
    await click(q(host, "persona-custom-open"));
    expect(host.textContent).toContain("Speech voice id (optional; stored for later, not used by speech yet)");
  });
});

