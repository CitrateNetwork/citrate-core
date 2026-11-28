// citrate-core: bridge domain `agentSkills`, moved verbatim from src/bridge/domains.ts by L0-WP05
// (planset v060). src/bridge/domains.ts re-exports it, so imports from "../bridge/domains" keep
// working. The owning lane is the row for this path in .agentile/v60-ownership.map.

// ── Local instruction-skills (Hermes "write & run skills"). A skill is a markdown playbook the agent
// authors and stores on THIS device; running it loads the instructions back into the agent's loop so it
// carries the task out with its existing (ceremony-gated) tools. Local files only; signs nothing. ──
export interface LocalSkill {
  /** The human name as authored. */
  name: string;
  /** One-line description of what the skill does. */
  description: string;
  /** Stable filename slug (the id skill_run uses). */
  slug: string;
}
export interface AgentSkillsDomain {
  /** The skills the agent has authored on this device (honest-empty, never fabricated). */
  list(): Promise<LocalSkill[]>;
  /** Author a local instruction-skill. Local file only; signs/runs nothing. PBA-L7b-002: with
   *  `overwrite` false (the default) an existing skill is NOT replaced — the call rejects with an
   *  error whose message starts `SKILL_EXISTS`; replacing it needs the member's approval first. */
  write(name: string, description: string, instructions: string, overwrite?: boolean): Promise<LocalSkill>;
  /** The instruction body of an authored skill, by slug or name. */
  read(name: string): Promise<string>;
  /** Remove an authored skill (idempotent). */
  remove(name: string): Promise<void>;
  /** HUP-S3.2: convert skills saved in the older flat-file format to SKILL.md, and say what happened. */
  migrate(): Promise<SkillMigrationReport>;
}
/** HUP-S3.2: what converting older flat-file skills to SKILL.md did (skills_local.rs). */
export interface SkillMigrationReport {
  /** Slugs now saved as SKILL.md skills. */
  converted: string[];
  /** Old files left in place, each with the reason. */
  failed: { file: string; reason: string }[];
}
