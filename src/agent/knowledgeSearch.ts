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
