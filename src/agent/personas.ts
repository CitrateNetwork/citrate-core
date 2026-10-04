// =====================================================================
// HUP-S3.3 + S3.7 — Hermes personas, core half (pure).
//
// A persona is a voice: writing-style rules the runtime renders into a system-prompt fragment,
// plus a default track and workflow, a tool emphasis and an optional TTS voice id. The runtime
// (citrate-agent-runtime agent-loop, one data file) owns the shipped personas; their names are
// placeholders pending owner sign-off. The sidecar serves them with their fragments and checks a
// member-defined persona (POST /personas/check), rendering it with the same template.
//
// This module only picks, composes and bounds. With no persona chosen (the default) the prompt and
// the messages are exactly what they were before personas existed. A persona shapes tone and
// wording only: it never grants a tool and never changes an approval, a gate or the ceremony.
// =====================================================================
import type { CustomPersonaInput, HermesPersona, TrackWorkflow } from "../bridge/domains";

/** Bounds, matching the runtime's (the sidecar applies the real rules). */
export const PERSONA_LIMITS = {
  name: 40,
  summary: 200,
  voice: 300,
  rule: 300,
  rules: 12,
  maxCustom: 20,
} as const;

export const DEFAULT_VOICE_LABEL = "Hermes (default voice)";

/**
 * Longest fragment used. The runtime renders a persona from at most a 40-char name, 200-char
 * summary, 300-char voice and tone and 12 rules of 300 chars, plus its template: well under this.
 */
export const MAX_PERSONA_FRAGMENT = 8000;

/** Control characters other than tab and newline, and the text-direction controls. */
const UNSAFE_FRAGMENT_CHAR = /[\u0000-\u0008\u000B-\u001F\u007F\u202A-\u202E\u2066-\u2069\u200E\u200F\u061C]/;

/** The active persona's fragment, trimmed; "" when none. */
export function personaFragment(p: HermesPersona | null | undefined): string {
  // Saved state can be stale or hand-edited: anything but a string fragment counts as none, and the
  // fragment is bounded again here, where it enters the prompt (a saved custom persona is checked by
  // the sidecar when it is made, not each time it is used).
  const f = p && typeof p === "object" ? (p as { prompt_fragment?: unknown }).prompt_fragment : undefined;
  if (typeof f !== "string") return "";
  const t = f.trim();
  if (t.length > MAX_PERSONA_FRAGMENT || UNSAFE_FRAGMENT_CHAR.test(t)) return "";
  return t;
}

/** The sidecar session prompt: the base prompt, then the persona fragment (never before it). */
export function composeSystemPrompt(base: string, p: HermesPersona | null | undefined): string {
  const f = personaFragment(p);
  return f ? `${base}\n\n${f}` : base;
}

/** For the gateway and local agent paths: the fragment as one system message at the head of the
 *  history. Core folds system-role history into the single leading system message after the base
 *  prompt and the live context, so the base rules still come first. */
export function withPersonaMessage<M extends { role: string; content: string }>(
  messages: M[],
  p: HermesPersona | null | undefined,
): (M | { role: string; content: string })[] {
  const f = personaFragment(p);
  return f ? [{ role: "system", content: f }, ...messages] : messages;
}

/** The name as shown, with the placeholder status spelled out. */
export function nameLabel(p: HermesPersona | null | undefined): string {
  if (!p) return DEFAULT_VOICE_LABEL;
  return p.name_pending_sign_off ? `${p.name} (placeholder name, pending owner sign-off)` : p.name;
}

/** A saved shipped persona picks up the sidecar's current view (a rename, a new fragment) by id. A
 *  custom persona, or one the sidecar no longer lists, stays as saved. */
export function refreshChosen(chosen: HermesPersona | null, list: HermesPersona[]): HermesPersona | null {
  if (!chosen || chosen.custom) return chosen;
  return list.find((p) => p.id === chosen.id && !p.custom) ?? chosen;
}

/** One rule per line; blank lines dropped. */
export function parseRules(text: string): string[] {
  return (text ?? "")
    .split("\n")
    .map((r) => r.trim())
    .filter((r) => r.length > 0);
}

/** `custom-<slug>` for a name, unique against the saved personas. */
export function customPersonaId(name: string, existing: readonly HermesPersona[]): string {
  const slug =
    (name ?? "")
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, "-")
      .replace(/^-+|-+$/g, "")
      .slice(0, 48) || "persona";
  const base = "custom-" + slug;
  const taken = new Set(existing.map((p) => p.id));
  if (!taken.has(base)) return base;
  for (let i = 2; ; i++) {
    const id = `${base}-${i}`;
    if (!taken.has(id)) return id;
  }
}

export interface CustomPersonaForm {
  name: string;
  summary: string;
  voice: string;
  tone: string;
  /** One rule per line. */
  rules: string;
  default_track: string;
  tts_voice?: string;
}

export type CustomPersonaResult = { ok: true; value: CustomPersonaInput } | { ok: false; error: string };

const VOICE_ID = /^[A-Za-z0-9._-]{1,64}$/;
const TRACK_ID = /^[a-z0-9_-]{1,64}$/;

/**
 * Validate the settings form and build the sidecar input. Over-length input is refused, never cut
 * (the member's words are not altered). A name that a shipped or saved persona already has is
 * refused case-insensitively. The sidecar runs the full check afterwards.
 */
export function validateCustomPersonaInput(
  form: CustomPersonaForm,
  saved: readonly HermesPersona[],
  shipped: readonly HermesPersona[],
): CustomPersonaResult {
  const name = (form.name ?? "").trim();
  const summary = (form.summary ?? "").trim();
  const voice = (form.voice ?? "").trim();
  const tone = (form.tone ?? "").trim();
  const rules = parseRules(form.rules);
  const track = (form.default_track ?? "").trim();
  const tts = (form.tts_voice ?? "").trim();
  const need = (v: string, max: number, what: string): string | null =>
    !v ? `Give the persona ${what}.` : v.length > max ? `The ${what} is over ${max} characters.` : null;
  const err =
    need(name, PERSONA_LIMITS.name, "a name") ||
    need(summary, PERSONA_LIMITS.summary, "a summary") ||
    need(voice, PERSONA_LIMITS.voice, "a voice") ||
    need(tone, PERSONA_LIMITS.voice, "a tone");
  if (err) return { ok: false, error: err };
  if (rules.length === 0) return { ok: false, error: "Add at least one writing-style rule." };
  if (rules.length > PERSONA_LIMITS.rules) return { ok: false, error: `At most ${PERSONA_LIMITS.rules} rules.` };
  if (rules.some((r) => r.length > PERSONA_LIMITS.rule))
    return { ok: false, error: `Each rule is at most ${PERSONA_LIMITS.rule} characters.` };
  if (!TRACK_ID.test(track)) return { ok: false, error: "Pick a default track." };
  if (tts && !VOICE_ID.test(tts)) return { ok: false, error: "A voice id is letters, digits, dot, dash or underscore." };
  if (saved.length >= PERSONA_LIMITS.maxCustom)
    return { ok: false, error: `You have ${PERSONA_LIMITS.maxCustom} custom personas; remove one first.` };
  const lower = name.toLowerCase();
  if ([...shipped, ...saved].some((p) => p.name.trim().toLowerCase() === lower))
    return { ok: false, error: `A persona named "${name}" already exists.` };
  return {
    ok: true,
    value: {
      id: customPersonaId(name, saved),
      name,
      summary,
      voice,
      tone,
      style_rules: rules,
      default_track: track,
      tool_emphasis: [],
      skills: [],
      tts_voice: tts || null,
    },
  };
}

/** The human part of a refused check (drops `Error:` / `PERSONA_REFUSED:`). */
export function personaRefusal(e: unknown): string {
  const raw = e instanceof Error ? e.message : String(e ?? "");
  return raw.replace(/^Error:\s*/, "").replace(/^PERSONA_REFUSED:\s*/, "").trim() || "the check failed";
}

/** A track's workflow family, default first. */
export function workflowsForTrack(ws: readonly TrackWorkflow[], track: string): TrackWorkflow[] {
  return ws.filter((w) => w.track === track).sort((a, b) => Number(b.is_default) - Number(a.is_default));
}

export function evidenceLabel(e: string): string {
  return e === "tool-report" ? "checked by tool reports" : "checked by answer shape";
}
