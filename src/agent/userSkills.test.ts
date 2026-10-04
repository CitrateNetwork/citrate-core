// Hermes P5 / WP5.1-5.2 — user-skill validation tests (pure). Includes the
// adversarial/negative cases: blank, over-length, duplicate, full-set.
import { describe, it, expect } from "vitest";
import { validateNewSkill, runPrompt, migrateLegacyUserSkills, SKILL_LIMITS, type UserSkill } from "./userSkills";

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

// HUP-S3.2 (US-3.2 AC2): skills kept in app state (the older format) move once onto the SKILL.md
// loader through the agentSkills bridge; nothing is lost, and what cannot move says why.
describe("migrateLegacyUserSkills", () => {
  const legacy = (name: string, instruction: string): UserSkill => ({ id: "usk-" + name, name, description: "d " + name, instruction });
  function fakeSkills(existing: Record<string, string>, failOn?: string) {
    const writes: string[] = [];
    return {
      writes,
      async write(name: string, _d: string, instructions: string, overwrite = false) {
        if (name === failOn) throw new Error("disk full");
        if (!overwrite && name.toLowerCase() in existing) throw new Error(`SKILL_EXISTS: a skill named "${name}" already exists`);
        existing[name.toLowerCase()] = instructions;
        writes.push(name);
        return { name, description: "", slug: name.toLowerCase() };
      },
      async read(name: string) {
        const b = existing[name.toLowerCase()];
        if (b === undefined) throw new Error("no local skill");
        return b + "\n";
      },
    };
  }

  it("moves every legacy skill and reports each one", async () => {
    const f = fakeSkills({});
    const r = await migrateLegacyUserSkills([legacy("Digest", "summarize"), legacy("Stake", "check stake")], f);
    expect(r.moved).toEqual(["Digest", "Stake"]);
    expect(r.kept).toEqual([]);
    expect(f.writes).toEqual(["Digest", "Stake"]);
  });

  it("treats a skill already saved with the same instructions as moved, and never overwrites a different one", async () => {
    const f = fakeSkills({ digest: "summarize", stake: "something else" });
    const r = await migrateLegacyUserSkills([legacy("Digest", "summarize"), legacy("Stake", "check stake")], f);
    expect(r.moved).toEqual(["Digest"]);
    expect(r.kept.map((k) => k.skill.name)).toEqual(["Stake"]);
    expect(r.kept[0].reason).toMatch(/already exists/);
    expect(f.writes).toEqual([]);
  });

  it("keeps a skill whose save failed, with the reason", async () => {
    const f = fakeSkills({}, "Digest");
    const r = await migrateLegacyUserSkills([legacy("Digest", "summarize")], f);
    expect(r.moved).toEqual([]);
    expect(r.kept[0].reason).toBe("disk full");
  });

  it("does nothing for an empty list", async () => {
    expect(await migrateLegacyUserSkills([], fakeSkills({}))).toEqual({ moved: [], kept: [] });
  });
});
