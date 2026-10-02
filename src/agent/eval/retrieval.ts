// =====================================================================
// citrate-core — retrieval for the Citrate QA eval (HUP-S3.1 first-run corpus, gate g2-knowledge)
//
// US-3.1 asks Hermes to answer from the BUNDLED knowledge with citations. With --memory-socket,
// scripts/eval-qa.mjs retrieves passages for each question from a memory daemon (mem-mcp) whose
// store holds the imported corpus, using the daemon's own search (`memory.search` with
// `passages: true`, the call the in-app memory_search tool makes for knowledge tenants), and puts
// the passages, each with the `<repo>:<path>#<anchor>` citation to quote, before the question.
// The scorer is unchanged (src/agent/eval/qa.ts). This is retrieve-then-answer: one search per
// tenant per question, chosen by the harness rather than by the model's own tool call.
// Pure: no I/O. The CLI does the socket and network work.
// =====================================================================

export interface Passage {
  /** Node id prefix the daemon printed. */
  id: string;
  /** Cosine score of the search hit. */
  score: number;
  /** `<repo>:<path>[#<anchor>]`, when the daemon printed one. */
  cite?: string;
  text: string;
}

const HIT_RE = /^ {2}([0-9a-f]{6,}) (-?\d+(?:\.\d+)?) \[[^\]]*\]/;

/**
 * Parse a `memory.search {passages: true}` rendering (citrate-memories `render_passages`). Hits
 * without a quoted passage (an older daemon's title-only rendering) are dropped.
 */
export function parsePassages(text: string): Passage[] {
  const out: (Passage & { lines?: string[] })[] = [];
  let cur: (Passage & { lines: string[] }) | null = null;
  const flush = () => {
    if (cur && cur.lines.length) {
      const p: Passage = { id: cur.id, score: cur.score, text: cur.lines.join("\n") };
      if (cur.cite) p.cite = cur.cite;
      out.push(p);
    }
  };
  for (const line of text.split("\n")) {
    const m = HIT_RE.exec(line);
    if (m) {
      flush();
      cur = { id: m[1], score: Number(m[2]), text: "", lines: [] };
      continue;
    }
    if (!cur) continue;
    if (line.startsWith("    cite: ")) cur.cite = line.slice("    cite: ".length).trim();
    else if (line.startsWith("    > ")) cur.lines.push(line.slice("    > ".length));
    else if (line === "    >") cur.lines.push("");
  }
  flush();
  return out;
}

/**
 * Merge the per-tenant passages by score (highest first), drop duplicates (same citation and
 * text), and number them `[n] cite as <cite>` until `maxChars` of passage text is used.
 */
export function buildRetrievalContext(perTenant: Passage[][], maxChars: number): string {
  const all = perTenant.flat().sort((a, b) => b.score - a.score);
  const seen = new Set<string>();
  const blocks: string[] = [];
  let used = 0;
  for (const p of all) {
    const key = `${p.cite ?? p.id}\u0000${p.text}`;
    if (seen.has(key)) continue;
    seen.add(key);
    if (used + p.text.length > maxChars) break;
    used += p.text.length;
    blocks.push(`[${blocks.length + 1}] cite as ${p.cite ?? `memory node ${p.id}`}\n${p.text}`);
  }
  return blocks.join("\n\n");
}

/** The user turn of a retrieval run: the excerpts, then the question. */
export function retrievalUserMessage(question: string, context: string): string {
  const head = context ? `Bundled documentation excerpts:\n\n${context}` : "Bundled documentation excerpts: none found.";
  return `${head}\n\nQuestion: ${question}`;
}

/** One newline-terminated JSON-RPC `tools/call` line for `memory.search` with passages. */
export function searchRequestLine(id: number, tenant: string, query: string, k: number): string {
  return (
    JSON.stringify({
      jsonrpc: "2.0",
      id,
      method: "tools/call",
      params: { name: "memory.search", arguments: { repo: tenant, query, budget: k, passages: true } },
    }) + "\n"
  );
}

/** The tool text of a `tools/call` response line; throws on a JSON-RPC error or `isError`. */
export function parseSearchResponse(line: string): string {
  const v = JSON.parse(line) as { error?: { message?: string }; result?: { content?: { text?: string }[]; isError?: boolean } };
  if (v.error) throw new Error(`memory daemon error: ${v.error.message ?? JSON.stringify(v.error)}`);
  const text = v.result?.content?.[0]?.text;
  if (typeof text !== "string") throw new Error("memory daemon response has no content[0].text");
  if (v.result?.isError) throw new Error(`memory.search refused: ${text}`);
  return text;
}
