// HUP-S3.3 + S3.7 — Hermes personas, core half (pure). The sidecar serves the shipped personas and
// renders every fragment; this module only picks, composes and bounds. No persona chosen means the
// prompt is exactly what it was before personas existed.
import { describe, it, expect } from "vitest";
import {
  composeSystemPrompt,
  customPersonaId,
  MAX_PERSONA_FRAGMENT,
  evidenceLabel,
  nameLabel,
  parseRules,
  personaFragment,
  personaRefusal,
  PERSONA_LIMITS,
  refreshChosen,
  validateCustomPersonaInput,
  withPersonaMessage,
  workflowsForTrack,
} from "./personas";
import type { HermesPersona, TrackWorkflow } from "../bridge/domains";

export const GRAFT: HermesPersona = {
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
  prompt_fragment: "## Persona: Graft\n\nWriting style:\n- Lead with the change.\n",
  name_pending_sign_off: true,
  custom: false,
};

const OWL: HermesPersona = {
  ...GRAFT,
  id: "custom-night-owl",
  role: "Custom",
  name: "Night Owl",
  name_status: "owner-approved",
  prompt_fragment: "## Persona: Night Owl\n",
  name_pending_sign_off: false,
  custom: true,
};

const input = (over: Partial<Record<string, unknown>> = {}) => ({
  name: "Night Owl",
  summary: "Late-night pair programmer.",
  voice: "Quiet.",
  tone: "Dry.",
  rules: "Lead with the answer.\nNo exclamation marks.",
  default_track: "code",
  ...over,
});

describe("prompt composition", () => {
  it("no persona leaves the prompt and the messages exactly as they were", () => {
    expect(composeSystemPrompt("BASE", null)).toBe("BASE");
    const msgs = [{ role: "user", content: "hi" }];
    expect(withPersonaMessage(msgs, null)).toEqual(msgs);
    expect(personaFragment(undefined)).toBe("");
  });

  it("a persona's fragment is appended after the base prompt, never before it", () => {
    const p = composeSystemPrompt("BASE", GRAFT);
    expect(p.startsWith("BASE\n\n")).toBe(true);
    expect(p).toContain("## Persona: Graft");
  });

  it("a persona rides as one system message at the head of the history", () => {
    const out = withPersonaMessage([{ role: "user", content: "hi" }], GRAFT);
    expect(out[0]).toEqual({ role: "system", content: GRAFT.prompt_fragment.trim() });
    expect(out[1]).toEqual({ role: "user", content: "hi" });
  });

  it("a malformed saved persona adds nothing", () => {
    const junk = { id: "x", prompt_fragment: 42 } as unknown as HermesPersona;
    expect(composeSystemPrompt("BASE", junk)).toBe("BASE");
    expect(personaFragment("nope" as unknown as HermesPersona)).toBe("");
  });

  it("a blank fragment adds nothing", () => {
    const blank = { ...GRAFT, prompt_fragment: "   " };
    expect(composeSystemPrompt("BASE", blank)).toBe("BASE");
    expect(withPersonaMessage([], blank)).toEqual([]);
  });
});

describe("names pending owner sign-off", () => {
  it("labels a placeholder name honestly", () => {
    expect(nameLabel(GRAFT)).toBe("Graft (placeholder name, pending owner sign-off)");
    expect(nameLabel(OWL)).toBe("Night Owl");
    expect(nameLabel(null)).toBe("Hermes (default voice)");
  });

  it("a saved shipped persona picks up a rename by id; a custom one is kept as saved", () => {
    const renamed = { ...GRAFT, name: "Scion", prompt_fragment: "## Persona: Scion\n" };
    expect(refreshChosen(GRAFT, [renamed])?.name).toBe("Scion");
    expect(refreshChosen(OWL, [renamed])).toBe(OWL);
    expect(refreshChosen(null, [renamed])).toBeNull();
    // A shipped persona that disappeared from the list stays as saved (the sidecar may be older).
    expect(refreshChosen(GRAFT, [])).toBe(GRAFT);
  });
});

describe("custom personas (US-3.3 AC3)", () => {
  it("accepts a well-formed persona and builds the sidecar input", () => {
    const r = validateCustomPersonaInput(input(), [], [GRAFT]);
    expect(r.ok).toBe(true);
    if (r.ok) {
      expect(r.value.id).toBe("custom-night-owl");
      expect(r.value.style_rules).toEqual(["Lead with the answer.", "No exclamation marks."]);
      expect(r.value.tool_emphasis).toEqual([]);
      expect(r.value.tts_voice).toBeNull();
    }
  });

  it("refuses blanks, over-length fields, too many rules and a full set", () => {
    expect(validateCustomPersonaInput(input({ name: " " }), [], [])).toMatchObject({ ok: false });
    expect(validateCustomPersonaInput(input({ rules: "" }), [], [])).toMatchObject({ ok: false });
    expect(validateCustomPersonaInput(input({ name: "n".repeat(PERSONA_LIMITS.name + 1) }), [], [])).toMatchObject({ ok: false });
    expect(validateCustomPersonaInput(input({ rules: "r".repeat(PERSONA_LIMITS.rule + 1) }), [], [])).toMatchObject({ ok: false });
    expect(validateCustomPersonaInput(input({ rules: Array(PERSONA_LIMITS.rules + 1).fill("r").join("\n") }), [], [])).toMatchObject({ ok: false });
    const full = Array.from({ length: PERSONA_LIMITS.maxCustom }, (_, i) => ({ ...OWL, id: "custom-" + i, name: "P" + i }));
    expect(validateCustomPersonaInput(input(), full, [])).toMatchObject({ ok: false });
  });

  it("refuses a name a shipped or saved persona already has, case-insensitively", () => {
    expect(validateCustomPersonaInput(input({ name: "GRAFT" }), [], [GRAFT])).toMatchObject({ ok: false });
    expect(validateCustomPersonaInput(input({ name: "night owl" }), [OWL], [])).toMatchObject({ ok: false });
  });

  it("refuses a malformed TTS voice id and keeps a good one", () => {
    expect(validateCustomPersonaInput(input({ tts_voice: "en US; x" }), [], [])).toMatchObject({ ok: false });
    const r = validateCustomPersonaInput(input({ tts_voice: "en-US-calm.1" }), [], []);
    expect(r.ok && r.value.tts_voice).toBe("en-US-calm.1");
  });

  it("ids are custom-<slug>, unique against saved personas", () => {
    expect(customPersonaId("Night Owl!", [])).toBe("custom-night-owl");
    expect(customPersonaId("Night Owl", [OWL])).toBe("custom-night-owl-2");
    expect(customPersonaId("???", [])).toBe("custom-persona");
  });

  it("rules are one per line, blank lines dropped", () => {
    expect(parseRules(" a \n\n b\n")).toEqual(["a", "b"]);
  });

  it("a sidecar refusal reads as its reason", () => {
    expect(personaRefusal(new Error("PERSONA_REFUSED: \"Graft\" is a shipped persona's name"))).toBe("\"Graft\" is a shipped persona's name");
    expect(personaRefusal("hermes is not running (no session bearer)")).toBe("hermes is not running (no session bearer)");
  });
});

describe("track workflows", () => {
  const ws: TrackWorkflow[] = [
    { id: "b", track: "code", title: "B", summary: "", is_default: false, evidence: "tool-report", tools: [], verifier_names: [], steps: [] },
    { id: "a", track: "code", title: "A", summary: "", is_default: true, evidence: "answer-shape", tools: [], verifier_names: [], steps: [] },
    { id: "c", track: "creative", title: "C", summary: "", is_default: true, evidence: "answer-shape", tools: [], verifier_names: [], steps: [] },
  ];
  it("lists a track's family with its default first", () => {
    expect(workflowsForTrack(ws, "code").map((w) => w.id)).toEqual(["a", "b"]);
  });
  it("says how each workflow is judged", () => {
    expect(evidenceLabel("tool-report")).toBe("checked by tool reports");
    expect(evidenceLabel("answer-shape")).toBe("checked by answer shape");
  });
});

describe("a saved fragment is bounded again where it is used", () => {
  const base = { prompt_fragment: "Write plainly." } as unknown as Parameters<typeof personaFragment>[0];
  it("an over-long fragment, or one with control or text-direction characters, is not used", () => {
    expect(personaFragment(base)).toBe("Write plainly.");
    const long = { prompt_fragment: "x".repeat(MAX_PERSONA_FRAGMENT + 1) } as unknown as Parameters<typeof personaFragment>[0];
    expect(personaFragment(long)).toBe("");
    for (const bad of ["ok\u0007bell", "ok‮evil", "ok⁦iso", "ok\u0000nul"]) {
      expect(personaFragment({ prompt_fragment: bad } as unknown as Parameters<typeof personaFragment>[0])).toBe("");
    }
    // Ordinary line breaks and tabs stay.
    expect(personaFragment({ prompt_fragment: "a\n\tb" } as unknown as Parameters<typeof personaFragment>[0])).toBe("a\n\tb");
    expect(composeSystemPrompt("BASE", long)).toBe("BASE");
  });
});
