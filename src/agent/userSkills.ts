// =====================================================================
// Hermes P5 / WP5.1–5.2 — user-defined skills (prompt-skills).
//
// A capsule skill is compiled WASM (marketplace/Commissary); a USER skill is
// lighter: a named instruction the member writes, RUN against the active model
// (the ModelRouter's Gemma / gateway / local backend). It persists with the rest
// of the member's local state, lists in the Agent surface, and — because running
// it just drives the agentic chat — any chain action it leads to still stops at the
// SignatureCeremony (Rule 3 holds by construction; nothing here signs).
//
// This module is the PURE core: validate + normalize an add, dedupe, and cap the
// set. No storage, no DOM — unit-tested, then wired into the store (which persists
// it alongside the journal) and the Agent surface.
// =====================================================================

export interface UserSkill {
  /** Stable id (assigned by the store; the pure core never invents time/random). */
  id: string;
  /** Display name, unique (case-insensitive) within the member's set. */
  name: string;
  /** One-line description (optional; may be empty). */
  description: string;
  /** The instruction sent to the active model when the skill is run. */
  instruction: string;
}

/** Bounds — keep a skill legible and the set small enough to render + persist. */
export const SKILL_LIMITS = {
  name: 60,
  description: 200,
  instruction: 2000,
  maxSkills: 50,
} as const;

export type AddSkillResult =
  | { ok: true; skill: Omit<UserSkill, "id"> }
  | { ok: false; error: string };

/**
 * Validate + normalize a user-skill add against the existing set. Returns the
 * fields to persist (the store assigns the id), or an honest error. Rejects: blank
 * name/instruction, over-length fields, a duplicate name (case-insensitive), and a
 * full set. Trims all fields; never silently truncates (an over-length input is a
 * rejection, not a quiet cut, so the member's intent is never altered).
 */
export function validateNewSkill(
  name: string,
  instruction: string,
  description: string,
  existing: readonly UserSkill[],
): AddSkillResult {
  const n = (name ?? "").trim();
  const instr = (instruction ?? "").trim();
  const desc = (description ?? "").trim();

  if (!n) return { ok: false, error: "Name the skill." };
  if (!instr) return { ok: false, error: "Give the skill an instruction to run." };
  if (n.length > SKILL_LIMITS.name) return { ok: false, error: `Name is over ${SKILL_LIMITS.name} characters.` };
  if (desc.length > SKILL_LIMITS.description)
    return { ok: false, error: `Description is over ${SKILL_LIMITS.description} characters.` };
  if (instr.length > SKILL_LIMITS.instruction)
    return { ok: false, error: `Instruction is over ${SKILL_LIMITS.instruction} characters.` };
  if (existing.length >= SKILL_LIMITS.maxSkills)
    return { ok: false, error: `You've reached the limit of ${SKILL_LIMITS.maxSkills} skills.` };
  if (existing.some((s) => s.name.toLowerCase() === n.toLowerCase()))
    return { ok: false, error: `A skill named "${n}" already exists.` };

  return { ok: true, skill: { name: n, description: desc, instruction: instr } };
}

/**
 * The text a run sends to the chat: the skill's instruction, prefixed so the model
 * (and the transcript) show it as a named skill invocation. Kept pure so the "what
 * gets sent" contract is testable. The instruction is the member's own text — it is
 * NOT interpreted as tools or code here; it is a prompt to the active model, and any
 * chain effect the model then proposes routes through the ceremony.
 */
export function runPrompt(skill: Pick<UserSkill, "name" | "instruction">): string {
  return `Run my "${skill.name}" skill:\n\n${skill.instruction}`;
}
