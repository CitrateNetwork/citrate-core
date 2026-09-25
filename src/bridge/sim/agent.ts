// CX bridge impl — agentHarness (C-22), SIM. Owned by lane s6 after S0. Honest-empty (Rule 1).
import type { AgentHarnessDomain, AgentHarnessStatus, AgentSkillsDomain, LocalSkill } from "../domains";
import type { SimHost } from "./index";

const slugify = (name: string): string =>
  name.trim().toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "") || "skill";

export function simAgentHarness(_host: SimHost): AgentHarnessDomain {
  return {
    async start() {
      /* sim: no sidecar */
    },
    async status(): Promise<AgentHarnessStatus> {
      return { running: false, skills: 0, pendingApprovals: 0 };
    },
    async skills() {
      return [];
    },
    async registrySkills() {
      return []; // honest-empty: web/dev has no chain to read the SkillRegistry from
    },
    async runSkill() {
      return { ok: false };
    },
    async pendingApprovals() {
      return [];
    },
    async stop() {
      /* sim: no-op */
    },
    async bridgePending() {
      // sim: no sidecar, so no chain effect to bridge — honest null (Rule 1).
      return null;
    },
    async resolve() {
      /* sim: no sidecar effect to release */
    },
  };
}

// Local instruction-skills in web/dev: an in-memory store (no filesystem). Real, not fabricated —
// it holds exactly what the agent authored this session.
export function simAgentSkills(_host: SimHost): AgentSkillsDomain {
  const store = new Map<string, { skill: LocalSkill; body: string }>();
  return {
    async list() {
      return [...store.values()].map((v) => v.skill).sort((a, b) => a.name.localeCompare(b.name));
    },
    async write(name, description, instructions, overwrite = false) {
      const slug = slugify(name);
      // PBA-L7b-002: same contract as the Rust command — never silently overwrite.
      if (!overwrite && store.has(slug)) throw new Error(`SKILL_EXISTS: a skill named "${name}" already exists`);
      const skill: LocalSkill = { name: name.trim(), description: description.trim(), slug };
      store.set(slug, { skill, body: instructions.trim() });
      return skill;
    },
    async read(name) {
      const hit = store.get(slugify(name));
      if (!hit) throw new Error(`no local skill named "${name}"`);
      return hit.body;
    },
    async remove(name) {
      store.delete(slugify(name));
    },
  };
}
