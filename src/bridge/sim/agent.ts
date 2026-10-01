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
    // HUP-S1.1c — no sidecar in web/dev: refuse honestly (Rule 1), never fake a turn.
    async sessionOpen() {
      throw new Error("the sidecar agent loop needs the desktop app");
    },
    async sessionSend() {
      throw new Error("the sidecar agent loop needs the desktop app");
    },
    async sessionEvents() {
      return { events: [], lastSeq: 0, busy: false };
    },
    async sessionToolResult() {
      throw new Error("the sidecar agent loop needs the desktop app");
    },
    async sessionStop() {
      /* nothing running */
    },
    // HUP-S1.4 — the tracks and briefs are served by the sidecar; web/dev has none. Refuse honestly
    // rather than inventing a question set or a brief (Rule 1).
    async tracks() {
      throw new Error("the interview needs the Hermes sidecar in the desktop app");
    },
    async briefCreate() {
      throw new Error("the interview needs the Hermes sidecar in the desktop app");
    },
    async briefCheck() {
      throw new Error("the interview needs the Hermes sidecar in the desktop app");
    },
    // HUP-S3.3 — personas and track workflows are served by the sidecar; web/dev has none. Refuse
    // honestly rather than inventing a persona list (Rule 1).
    async personas() {
      throw new Error("personas need the Hermes sidecar in the desktop app");
    },
    async personaCheck() {
      throw new Error("personas need the Hermes sidecar in the desktop app");
    },
    async workflows() {
      throw new Error("track workflows need the Hermes sidecar in the desktop app");
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
