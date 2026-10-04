// =====================================================================
// citrate-core: Hermes knowledge search (HUP-S3.1, US-3.1 "it knows Citrate out of the box")
//
// The bundled knowledge corpus (citrate-memories mem-corpus, imported on first run by
// src-tauri/src/knowledge_import.rs) lands in four knowledge tenants. The memory_search tool picks
// the tenant the model asked for, asks the memory daemon for PASSAGES on knowledge tenants
// (`memory.search {passages: true}`: each hit's text plus a `<repo>:<path>#<anchor>` citation),
// and hands the model each passage with the citation it must quote. Personal notes keep the
// title-only rendering. Pure: no I/O.
// =====================================================================
import type { MemoryResult } from "../bridge/domains";

/** The tenants the knowledge corpus writes (mem_corpus::KNOWLEDGE_TENANTS), in import order. */
export const KNOWLEDGE_TENANTS = ["citrate-docs", "methodology", "refs", "skills"] as const;
export type KnowledgeTenant = (typeof KNOWLEDGE_TENANTS)[number];

/**
 * The memory_search tool the model is offered. The agent's tool list (harness AGENT_TOOLS) holds
 * this object, and the Citrate QA eval (src/agent/eval/toolLoop.ts) offers the same one, so the
 * eval measures the call the app makes.
 */
export const MEMORY_SEARCH_TOOL = {
  type: "function",
  function: {
    name: "memory_search",
    description:
      "Semantic search over the member's memory graph, including the bundled Citrate knowledge (docs, papers, Agentile, Solidity references, reviewed skills). Use for any Citrate protocol/how-to/docs question. Knowledge results come with passages and a citation (<repo>:<path>#<anchor>) to quote. Returns real hits or an empty result.",
    parameters: {
      type: "object",
      properties: {
        query: { type: "string", description: "what to search for" },
        tenant: {
          type: "string",
          description:
            "graph to search: 'citrate-docs' (Citrate docs and papers), 'methodology' (Agentile), 'refs' (OpenZeppelin, forge-std and other Solidity references), 'skills' (reviewed skills) or 'personal' (the member's own notes). Defaults to citrate-docs.",
        },
      },
      required: ["query"],
    },
  },
} as const;

/** Hits a memory_search call asks for: 5 passages on a knowledge tenant, 6 titles on personal notes. */
export function memorySearchBudget(passages: boolean): number {
  return passages ? 5 : 6;
}

/** Which tenant a memory_search call reads, and whether it asks for passages. */
export function memorySearchTarget(tenant: unknown): { tenant: string; passages: boolean } {
  if (tenant === "personal") return { tenant: "personal", passages: false };
  const t = (KNOWLEDGE_TENANTS as readonly unknown[]).includes(tenant) ? (tenant as KnowledgeTenant) : "citrate-docs";
  return { tenant: t, passages: true };
}

/**
 * Render search/recall hits for the model. A hit that carries a passage is shown with the citation
 * to quote and its text; any other hit is its title line. An empty result says so plainly.
 */
export function formatMemoryHits(res: MemoryResult): string {
  if (!res.hits.length) {
    return `No results in the ${res.tenant} memory (${res.totalInTenant} nodes total). Do not fabricate; tell the member nothing was found.`;
  }
  return res.hits
    .map((h, i) => {
      if (!h.passage) return `[${i + 1}] ${h.title}`;
      const head = h.cite ? `[${i + 1}] cite as ${h.cite}` : `[${i + 1}] ${h.title}`;
      return `${head}\n${h.passage}`;
    })
    .join("\n\n");
}
