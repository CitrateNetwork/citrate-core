// =====================================================================
// HUP-S3.4 — "Teach Hermes": launch a verified workflow from the app (US-3.4).
//
// The member writes a task and the checks its answer must pass. The sidecar runs it in a fresh
// session as a declarative workflow judged by its own verifiers (`answer_contains`), never by the
// model's claim. Only when every check passed is Hermes asked, in the same session, whether
// anything is worth keeping; it proposes through its `learn_propose` tool, and the member then
// accepts or rejects each proposal on its card. Nothing here persists or decides anything.
//
// Rule 1: every failure is reported as it is (model not running, a run that did not finish, a
// refused workflow); the session is always closed.
// =====================================================================

import type { SessionEventsPage } from "../bridge/domains";
import type { WorkflowRunView, WorkflowSpec } from "./learn";

export const TEACH_LIMITS = {
  /** Longest task text, in characters (the sidecar allows 8 KiB per instruction). */
  maxTask: 2000,
  /** Most checks on the one step (the sidecar allows 8 verifiers per step). */
  maxChecks: 6,
  /** Longest check text, in characters (the sidecar allows 256 bytes). */
  maxCheck: 120,
  /** Attempts the sidecar allows per step. */
  maxAttempts: 3,
} as const;

export const TEACH_SYSTEM_PROMPT =
  "You are Hermes. The member is teaching you a task. Do it carefully and answer in plain text. Your answer is checked automatically.";

export const PROPOSE_PROMPT =
  "Your answer passed every check. If something from this task is worth keeping for next time, call learn_propose once: a memory (one key and one value) for a fact, or a skill (a complete SKILL.md with name and description frontmatter) for a procedure. If nothing is worth keeping, say so and do not call it.";

export interface TeachInput {
  task: string;
  checks: string[];
  attempts?: number;
}

export type TeachBuild = { ok: true; spec: WorkflowSpec } | { ok: false; reason: string };

// Control characters other than newline and tab (the sidecar refuses them in ids and texts).
// eslint-disable-next-line no-control-regex
const CONTROL = /[\u0000-\u0008\u000b-\u001f\u007f]/;

/** The one-step workflow for a member's task: every check is an `answer_contains` verifier. */
export function buildTeachWorkflow(input: TeachInput, now: number = Date.now()): TeachBuild {
  const task = input.task.trim();
  if (!task) return { ok: false, reason: "Write the task for Hermes." };
  if (task.length > TEACH_LIMITS.maxTask) return { ok: false, reason: `Keep the task under ${TEACH_LIMITS.maxTask} characters.` };
  if (CONTROL.test(task)) return { ok: false, reason: "The task has characters that cannot be sent." };
  const checks = input.checks.map((c) => c.trim()).filter((c) => c.length > 0);
  if (checks.length === 0) return { ok: false, reason: "Add at least one check: a phrase the answer must contain." };
  if (checks.length > TEACH_LIMITS.maxChecks) return { ok: false, reason: `Use at most ${TEACH_LIMITS.maxChecks} checks.` };
  const seen = new Set<string>();
  for (const c of checks) {
    if (c.length > TEACH_LIMITS.maxCheck) return { ok: false, reason: `Keep each check under ${TEACH_LIMITS.maxCheck} characters.` };
    if (CONTROL.test(c) || c.includes("\n") || c.includes("\t")) return { ok: false, reason: "A check is one line of plain text." };
    const k = c.toLowerCase();
    if (seen.has(k)) return { ok: false, reason: `The check "${c}" is listed twice.` };
    seen.add(k);
  }
  const attempts = input.attempts ?? 2;
  if (!Number.isInteger(attempts) || attempts < 1 || attempts > TEACH_LIMITS.maxAttempts) {
    return { ok: false, reason: `Attempts must be between 1 and ${TEACH_LIMITS.maxAttempts}.` };
  }
  return {
    ok: true,
    spec: {
      id: `teach-${Math.max(0, Math.floor(now))}`,
      steps: [
        {
          id: "task",
          instruction: `${task}\n\nAnswer in plain text.`,
          max_attempts: attempts,
          verifiers: checks.map((text) => ({ kind: "answer_contains" as const, text })),
        },
      ],
    },
  };
}

/** What the launcher needs from the bridge (the `agentHarness` methods of the same names). */
export interface TeachDeps {
  sessionOpen(systemPrompt: string, toolsJson: string): Promise<string>;
  workflowRun(sessionId: string, workflow: WorkflowSpec): Promise<string>;
  workflowStatus(sessionId: string, runId: string): Promise<WorkflowRunView>;
  sessionSend(id: string, text: string): Promise<void>;
  sessionEvents(id: string, after: number, waitMs: number): Promise<SessionEventsPage>;
  sessionClose(id: string): Promise<void>;
  /** Wait between status polls (tests pass a no-op). */
  sleep?(ms: number): Promise<void>;
}

export type TeachPhase = "opening" | "running" | "verified" | "unverified" | "proposing" | "done" | "failed";

export interface TeachProgress {
  phase: TeachPhase;
  runId?: string;
  verdicts?: { label: string; passed: boolean }[];
  /** Why the run stopped short (unverified, failed). */
  reason?: string;
  /** How many proposals Hermes made (its `learn_propose` calls that succeeded). */
  proposals?: number;
}

export interface TeachOptions {
  pollMs?: number;
  /** Most status polls before giving up (default: 10 minutes at 1 s). */
  maxPolls?: number;
  eventWaitMs?: number;
  /** Most event pages to read for the propose turn (default: about 10 minutes). */
  maxEventPages?: number;
}

const defaultSleep = (ms: number) => new Promise<void>((r) => setTimeout(r, ms));

function message(e: unknown): string {
  const m = e instanceof Error ? e.message : String(e);
  return m.replace(/^(LEARN_REFUSED): /, "");
}

function verdictsOf(v: WorkflowRunView): { label: string; passed: boolean }[] {
  return (v.evidence?.verdicts ?? []).map((x) => ({ label: `${x.step}: ${x.name}`, passed: x.passed === true }));
}

/**
 * Run a member-taught workflow end to end. Reports each phase through `onProgress` and resolves
 * with the final state. The session is always closed.
 */
export async function runTeach(deps: TeachDeps, spec: WorkflowSpec, onProgress: (p: TeachProgress) => void, opts: TeachOptions = {}): Promise<TeachProgress> {
  const pollMs = opts.pollMs ?? 1000;
  const maxPolls = opts.maxPolls ?? 600;
  const eventWaitMs = opts.eventWaitMs ?? 5000;
  const maxEventPages = opts.maxEventPages ?? 120;
  const sleep = deps.sleep ?? defaultSleep;
  const report = (p: TeachProgress) => {
    onProgress(p);
    return p;
  };

  report({ phase: "opening" });
  let sid: string;
  try {
    sid = await deps.sessionOpen(TEACH_SYSTEM_PROMPT, "[]");
  } catch (e) {
    return report({ phase: "failed", reason: message(e) });
  }
  try {
    const runId = await deps.workflowRun(sid, spec);
    report({ phase: "running", runId });
    let view: WorkflowRunView | null = null;
    for (let i = 0; i < maxPolls; i++) {
      const v = await deps.workflowStatus(sid, runId);
      if (v.state !== "running") {
        view = v;
        break;
      }
      await sleep(pollMs);
    }
    if (!view) return report({ phase: "failed", runId, reason: "The workflow did not finish in time; nothing was learned." });
    const verdicts = verdictsOf(view);
    if (view.state !== "verified") {
      return report({ phase: "unverified", runId, verdicts, reason: view.reason ?? "Not every check passed." });
    }
    report({ phase: "verified", runId, verdicts });

    report({ phase: "proposing", runId, verdicts, proposals: 0 });
    await deps.sessionSend(sid, PROPOSE_PROMPT);
    const proposeCalls = new Set<string>();
    let proposals = 0;
    let after = 0;
    for (let i = 0; i < maxEventPages; i++) {
      const page = await deps.sessionEvents(sid, after, eventWaitMs);
      let done = false;
      for (const { seq, event } of page.events) {
        after = Math.max(after, seq);
        const type = event.type;
        if (type === "tool_call") {
          const call = event.call as { id?: unknown; name?: unknown } | undefined;
          if (call?.name === "learn_propose" && typeof call.id === "string") proposeCalls.add(call.id);
        } else if (type === "tool_result") {
          if (typeof event.call_id === "string" && proposeCalls.has(event.call_id) && event.status === "ok") proposals += 1;
        } else if (type === "done") {
          done = true;
        }
      }
      if (done) return report({ phase: "done", runId, verdicts, proposals });
      after = Math.max(after, page.lastSeq);
    }
    return report({ phase: "failed", runId, verdicts, proposals, reason: "Hermes did not finish proposing in time. Any proposal it made is listed below." });
  } catch (e) {
    return report({ phase: "failed", reason: message(e) });
  } finally {
    await deps.sessionClose(sid).catch(() => undefined);
  }
}

/** One line for the member about how a teach run ended. */
export function teachSummary(p: TeachProgress): string {
  switch (p.phase) {
    case "opening":
      return "Opening a session on your local model...";
    case "running":
      return "Hermes is working on the task. Its answer is checked when it is done.";
    case "verified":
    case "proposing":
      return "Every check passed. Asking Hermes whether anything is worth keeping...";
    case "unverified":
      return `Not every check passed, so nothing can be learned from this run. ${p.reason ?? ""}`.trim();
    case "done":
      return p.proposals && p.proposals > 0
        ? `Every check passed. Hermes proposed ${p.proposals} thing${p.proposals === 1 ? "" : "s"} to keep; review ${p.proposals === 1 ? "it" : "them"} below.`
        : "Every check passed. Hermes found nothing worth keeping from this run.";
    case "failed":
      return `The run stopped: ${p.reason ?? "unknown error"}`;
  }
}
