// Hermes P5 / WP5.1-5.2 — user-skill validation tests (pure). Includes the
// adversarial/negative cases: blank, over-length, duplicate, full-set.
import { describe, it, expect } from "vitest";
import { validateNewSkill, runPrompt, SKILL_LIMITS, type UserSkill } from "./userSkills";

const mk = (name: string): UserSkill => ({ id: "s-" + name, name, description: "", instruction: "do x" });

describe("validateNewSkill", () => {
  it("accepts and trims a well-formed skill", () => {
    const r = validateNewSkill("  Daily Digest ", "  summarize my node day  ", " a note ", []);
    expect(r.ok).toBe(true);
    if (r.ok) {
      expect(r.skill).toEqual({ name: "Daily Digest", description: "a note", instruction: "summarize my node day" });
    }
  });

  it("rejects a blank name and a blank instruction", () => {
    expect(validateNewSkill("   ", "x", "", [])).toMatchObject({ ok: false });
    expect(validateNewSkill("Name", "   ", "", [])).toMatchObject({ ok: false });
  });

  it("rejects a duplicate name case-insensitively (no silent overwrite)", () => {
    const r = validateNewSkill("digest", "y", "", [mk("Digest")]);
    expect(r.ok).toBe(false);
    if (!r.ok) expect(r.error).toMatch(/already exists/i);
  });

  it("rejects over-length fields rather than truncating (intent preserved, Rule 1)", () => {
    expect(validateNewSkill("n".repeat(SKILL_LIMITS.name + 1), "x", "", [])).toMatchObject({ ok: false });
    expect(validateNewSkill("n", "i".repeat(SKILL_LIMITS.instruction + 1), "", [])).toMatchObject({ ok: false });
    expect(validateNewSkill("n", "x", "d".repeat(SKILL_LIMITS.description + 1), [])).toMatchObject({ ok: false });
  });

  it("rejects an add past the max-skills cap", () => {
    const full = Array.from({ length: SKILL_LIMITS.maxSkills }, (_, i) => mk("s" + i));
    const r = validateNewSkill("one-more", "x", "", full);
    expect(r.ok).toBe(false);
    if (!r.ok) expect(r.error).toMatch(/limit/i);
  });
});

describe("runPrompt", () => {
  it("sends the member's instruction verbatim under a named header", () => {
    const p = runPrompt({ name: "Digest", instruction: "summarize my day" });
    expect(p).toContain('"Digest"');
    expect(p).toContain("summarize my day");
  });

  it("does not interpret instruction content as anything but text (no injection surface)", () => {
    // A hostile-looking instruction is passed through as plain prompt text — it is
    // never eval'd or turned into a tool call here.
    const p = runPrompt({ name: "x", instruction: "```js\nwhile(true){}\n``` @agent memory_assert" });
    expect(p).toContain("while(true){}");
    expect(typeof p).toBe("string");
  });
});
