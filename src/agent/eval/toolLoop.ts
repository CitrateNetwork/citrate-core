// =====================================================================
// citrate-core: the Citrate QA eval's tool loop (gate g2-knowledge (b), US-3.1 AC2)
//
// In the app, Hermes answers a Citrate question by calling memory_search itself: the model picks
// the query and the tenant, core runs `memory.search` on the local memory daemon (passages on the
// knowledge tenants that the first-run import filled), formats the hits with formatMemoryHits, and
// feeds them back until the model answers (src/agent/harness.ts createAgentProvider, at most
// AGENT_MAX_TURNS model requests). This loop does the same against a mem-mcp daemon holding the
// imported corpus, with the same tool object, tenant rule, hit budget and formatter, so the QA
// score measures what the app does. It also records which nodes each search returned, so every
// citation in the answer can be resolved to node ids of the imported graph.
//
// Pure: the caller supplies the model request and the daemon search. It imports only .ts modules
// that Node can load with type stripping (scripts/eval-qa.mjs runs it without a bundler).
// =====================================================================
import { MEMORY_SEARCH_TOOL, formatMemoryHits, memorySearchBudget, memorySearchTarget } from "../knowledgeSearch.ts";
import { memoryResultFromSearchText } from "./retrieval.ts";

/**
 * Model requests per question: the app's AGENT_MAX_TURNS (src/agent/harness.ts). harness.ts cannot
 * be loaded by the Node CLI, so the value is repeated here and a test pins the two together.
 */
export const QA_TOOL_MAX_TURNS = 6;

export interface ToolCallLike {
  id?: string;
  type?: string;
  function?: { name?: string; arguments?: string };
}

/** One OpenAI-style chat message of the tool protocol. */
export interface ChatMessage {
  role: string;
  content?: string | null;
  tool_calls?: ToolCallLike[];
  tool_call_id?: string;
}

/** A node a search returned: the id prefix the daemon printed and its citation, when it has one. */
export interface RetrievedNode {
  id: string;
  cite?: string;
}

export interface ToolLoopDeps {
  /** One model request with the offered tools; resolves to the assistant message. */
  complete: (messages: ChatMessage[], tools: readonly unknown[]) => Promise<ChatMessage>;
  /** One `memory.search` on the daemon; resolves to its tool text. A failure aborts the run. */
  search: (tenant: string, query: string, k: number, passages: boolean) => Promise<string>;
}

export interface ToolLoopResult {
  /** The final answer, or "" when the model used every turn without answering. */
  text: string;
  calls: { tenant: string; query: string }[];
  retrieved: RetrievedNode[];
  /** True when the loop stopped at QA_TOOL_MAX_TURNS (the app shows an error in that case). */
  turnLimit: boolean;
}

function parseArgs(raw: string | undefined): Record<string, unknown> {
  try {
    const v: unknown = JSON.parse(raw || "{}");
    return v && typeof v === "object" && !Array.isArray(v) ? (v as Record<string, unknown>) : {};
  } catch {
    return {};
  }
}

/** Ask one question the way the app does: the model may call memory_search, then answers. */
export async function answerWithMemoryTool(
  system: string,
  question: string,
  deps: ToolLoopDeps,
  maxTurns = QA_TOOL_MAX_TURNS,
): Promise<ToolLoopResult> {
  const convo: ChatMessage[] = [
    { role: "system", content: system },
    { role: "user", content: question },
  ];
  const calls: ToolLoopResult["calls"] = [];
  const retrieved: RetrievedNode[] = [];
  const tools = [MEMORY_SEARCH_TOOL];
  for (let turn = 0; turn < maxTurns; turn++) {
    const msg = await deps.complete(convo, tools);
    const toolCalls = Array.isArray(msg.tool_calls) ? msg.tool_calls : [];
    if (toolCalls.length === 0) {
      return { text: typeof msg.content === "string" ? msg.content : "", calls, retrieved, turnLimit: false };
    }
    convo.push({ role: "assistant", content: msg.content ?? null, tool_calls: toolCalls });
    for (const [i, tc] of toolCalls.entries()) {
      const id = tc.id || `call_${turn}_${i}`;
      const name = tc.function?.name ?? "";
      let content: string;
      if (name !== MEMORY_SEARCH_TOOL.function.name) {
        content = `tool error: ${name || "(unnamed)"} is not available in this evaluation; only memory_search is offered`;
      } else {
        const args = parseArgs(tc.function?.arguments);
        const query = typeof args.query === "string" ? args.query : "";
        const { tenant, passages } = memorySearchTarget(args.tenant);
        calls.push({ tenant, query });
        const res = memoryResultFromSearchText(tenant, await deps.search(tenant, query, memorySearchBudget(passages), passages));
        for (const h of res.hits) retrieved.push(h.cite ? { id: h.id, cite: h.cite } : { id: h.id });
        content = formatMemoryHits(res);
      }
      convo.push({ role: "tool", tool_call_id: id, content });
    }
  }
  return { text: "", calls, retrieved, turnLimit: true };
}
