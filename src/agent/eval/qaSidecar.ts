// =====================================================================
// citrate-core: Citrate QA through a REAL Hermes sidecar (HUP-S7.7, US-9.2 AC1, gate g2-knowledge)
//
// With the sidecar loop on, the app answers a question in a sidecar session: the sidecar puts the
// skills that match the question in the system prompt (its one SKILL.md loader, ranked per turn,
// read with `skill_load`) and calls core's memory_search tool, which core answers from the local
// memory daemon. `scripts/eval-qa.mjs --retrieval-mode sidecar` measures that path: it starts the
// real citrate-agent-sidecar with the bundled skills (`CITRATE_HERMES_SKILLS`, and the reviewed
// third-party skills when given), opens one session per question with the QA instruction and the
// memory_search tool (host core, the reviewed annotations), and plays core's part: memory_search
// runs on a mem-mcp daemon holding the imported corpus, formatted with the app's formatter.
//
// This module holds the parts that are not process plumbing, so they are unit-tested. It imports
// only .ts modules Node can load with type stripping (the CLI runs without a bundler).
// =====================================================================
import { AGENT_TOOL_ANNOTATIONS } from "../toolAnnotations.ts";
import { MEMORY_SEARCH_TOOL, formatMemoryHits, memorySearchBudget, memorySearchTarget } from "../knowledgeSearch.ts";
import { memoryResultFromSearchText } from "./retrieval.ts";
import type { CoreAnswer, SidecarEvent } from "./sidecar.ts";
import type { RetrievedNode } from "./toolLoop.ts";

/** The sidecar's skill reader (agent-loop skills.rs SKILL_LOAD_TOOL). */
export const SKILL_LOAD_TOOL = "skill_load";

/** The `POST /sessions` body for one QA question: core's session shape with one core tool. */
export function qaSessionBody(o: {
  model: string;
  baseUrl: string;
  bearer?: string;
  contextTokens: number;
  maxTokens: number;
  systemPrompt: string;
}): Record<string, unknown> {
  const t = MEMORY_SEARCH_TOOL.function;
  const ann = AGENT_TOOL_ANNOTATIONS[t.name];
  return {
    model: o.model,
    systemPrompt: o.systemPrompt,
    llm: { baseUrl: o.baseUrl, bearer: o.bearer ?? "" },
    tools: [
      {
        name: t.name,
        description: t.description,
        parameters: t.parameters,
        host: "core",
        annotations: { effect: ann.effect, trust: ann.trust, read_only: ann.effect === "none" },
      },
    ],
    maxToolsPerRequest: 8,
    contextTokens: o.contextTokens,
    maxTokens: o.maxTokens,
    hicAware: true,
  };
}

function parseArgs(raw: string | undefined): Record<string, unknown> | null {
  try {
    const v: unknown = JSON.parse(raw && raw.trim() ? raw : "{}");
    return v && typeof v === "object" && !Array.isArray(v) ? (v as Record<string, unknown>) : null;
  } catch {
    return null;
  }
}

/** What core's part recorded for one question. */
export interface QaCoreLog {
  calls: { tenant: string; query: string }[];
  retrieved: RetrievedNode[];
}

/**
 * Core's part for a QA session: memory_search runs on the daemon (same tenant rule, hit budget and
 * formatter as the app); any other core tool is an error, since the session offers only that one.
 * A daemon failure rejects, which aborts the run (no partial scorecard).
 */
export function qaCoreAnswerer(
  search: (tenant: string, query: string, k: number, passages: boolean) => Promise<string>,
  log: QaCoreLog,
): (name: string, rawArgs: string) => Promise<CoreAnswer> {
  return async (name, rawArgs) => {
    if (name !== MEMORY_SEARCH_TOOL.function.name) {
      return { status: "error", content: `${name || "(unnamed)"} is not available in this evaluation; only memory_search is offered` };
    }
    const args = parseArgs(rawArgs);
    if (!args) return { status: "error", content: "arguments are not valid JSON" };
    const query = typeof args.query === "string" ? args.query : "";
    const { tenant, passages } = memorySearchTarget(args.tenant);
    log.calls.push({ tenant, query });
    const res = memoryResultFromSearchText(tenant, await search(tenant, query, memorySearchBudget(passages), passages));
    for (const h of res.hits) log.retrieved.push(h.cite ? { id: h.id, cite: h.cite } : { id: h.id });
    return { status: "ok", content: formatMemoryHits(res) };
  };
}

/** The answer and the skill reads of one session, from its own events. */
export interface QaSessionOutcome {
  /** The last `final` reply, or "" when the session ended without one. */
  text: string;
  /** Skill names the model read with skill_load, in order. */
  skillLoads: string[];
  /** The session's `done` outcome, when it reported one. */
  outcome?: string;
}

export function qaOutcomeFromEvents(events: SidecarEvent[]): QaSessionOutcome {
  let text = "";
  let outcome: string | undefined;
  const skillLoads: string[] = [];
  for (const { event: ev } of events) {
    if (ev.type === "final" && typeof ev.content === "string") text = ev.content;
    else if (ev.type === "done" && typeof ev.outcome === "string") outcome = ev.outcome;
    else if (ev.type === "tool_call" && ev.call && typeof ev.call === "object") {
      const call = ev.call as { name?: unknown; arguments?: unknown };
      if (call.name === SKILL_LOAD_TOOL) {
        const args = parseArgs(typeof call.arguments === "string" ? call.arguments : undefined);
        skillLoads.push(typeof args?.name === "string" ? args.name : "(unnamed)");
      }
    }
  }
  return outcome === undefined ? { text, skillLoads } : { text, skillLoads, outcome };
}
