// =====================================================================
// HUP-S3.3 (US-3.3 AC2) — track workflows from chat, core half (pure).
//
// A member runs a track's workflow with `/run <workflow>` in the chat or from a saved brief's card.
// The sidecar runs the bundled catalog workflow in the chat's session (steps and verifiers come
// from its catalog, never from here) and only its verifiers decide the result: this module words
// "verified" only for a run the sidecar marked verified. Workflows need the sidecar loop (Settings,
// off by default); without it the chat says how to turn it on and runs nothing (Rule 1).
//
// It also maps the chosen persona to what a sidecar session applies (skill allowlist + tool
// emphasis): a shipped persona by id, a member-defined one by its own fields.
// =====================================================================
import type { CustomPersonaInput, HermesPersona, SessionPersonaChoice } from "../bridge/domains";
import type { WorkflowRunView } from "./learn";
import type { TurnActivityEvent } from "./harness";

export const WORKFLOW_ID = /^[a-z0-9_-]{1,64}$/;

export const RUN_USAGE =
  "Run a track workflow with `/run <workflow>`, for example `/run creative-project`, `/run status-note` or `/run hello-mint`. The workflows of each track are listed under Settings, Hermes persona.";

/** What the chat says when the active provider cannot run workflows. */
export function sidecarLoopNeeded(workflowId: string): string {
  return (
    `Track workflows run in the Hermes sidecar loop, which this chat is not using. ` +
    `To run \`${workflowId}\`, turn on "Run Hermes's loop in the agent sidecar" in Settings (it needs the local model running), then send \`/run ${workflowId}\` again. Nothing was run.`
  );
}

export type RunCommand = { kind: "none" } | { kind: "usage" } | { kind: "run"; id: string };

/** `/run <workflow>` → run; `/run` alone or a malformed id → usage; anything else → none. */
export function parseRunCommand(text: string): RunCommand {
  const t = (text ?? "").trim();
  const m = /^\/run(?:\s+([\s\S]*))?$/.exec(t);
  if (!m) return { kind: "none" };
  const id = (m[1] ?? "").trim();
  return WORKFLOW_ID.test(id) ? { kind: "run", id } : { kind: "usage" };
}

/** The verdict as the chat shows it. "Verified" only when the sidecar's verifiers all passed. */
export function workflowSummary(workflowId: string, view: WorkflowRunView): string {
  if (view.state === "verified") {
    const answers = (view.answers ?? []).filter((a) => a.trim() !== "");
    const body = answers.length ? "\n\n" + answers.join("\n\n---\n\n") : "";
    return `**Verified:** \`${workflowId}\` passed every check its steps declare.${body}`;
  }
  if (view.state === "unverified") {
    return `**Not verified:** \`${workflowId}\` did not pass its checks. ${view.reason ? `Reason: ${view.reason}` : "No reason was given."}`;
  }
  return `\`${workflowId}\` is still running.`;
}

/** The human part of a refused start (drops `Error:` / `WORKFLOW_REFUSED:`). */
export function workflowRefusal(e: unknown): string {
  const raw = e instanceof Error ? e.message : String(e ?? "");
  return raw.replace(/^Error:\s*/, "").replace(/^WORKFLOW_REFUSED:\s*/, "").trim() || "the workflow could not start";
}

/** A verifier verdict as a chat chip. */
export function verifierChip(ev: Extract<TurnActivityEvent, { kind: "verifier" }>): { label: string; status: string } {
  return { label: `${ev.step}: ${ev.name}`, status: ev.passed ? "approved" : "declined" };
}

/** What the sidecar session applies for the chosen persona; null = none (nothing changes). */
export function personaChoice(p: HermesPersona | null | undefined): SessionPersonaChoice | null {
  if (!p || typeof p !== "object" || typeof p.id !== "string" || !p.id) return null;
  if (!p.custom) return { persona: p.id };
  const custom: CustomPersonaInput = {
    id: p.id,
    name: p.name,
    summary: p.summary,
    voice: p.voice,
    tone: p.tone,
    style_rules: p.style_rules,
    default_track: p.default_track,
    tool_emphasis: p.tool_emphasis ?? [],
    skills: p.skills ?? [],
    tts_voice: p.tts_voice ?? null,
  };
  return { customPersona: custom };
}
