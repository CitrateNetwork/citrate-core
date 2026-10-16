// =====================================================================
// citrate-core — "citations resolve to bundled nodes" (HUP-S3.1, US-3.1 AC2)
//
// The anchor index (qa-v1.anchors.json) covers only the files the QA set cites. In a retrieval
// run Hermes answers from the bundled corpus, and a correct answer may also cite another
// bundled page. This index lists every file passage in the corpus tenant files (format
// citrate-corpus/2: node `source_ref.Artifact {repo, path}`, content as a string) with the
// anchors of its sections, computed the way mem-corpus does (cite.rs: the GitHub slug of the
// text after the last ` › ` on the chunk's first line). Pure: the CLI reads the files.
// =====================================================================
import { slugify } from "./qa.ts";

export interface CorpusCitationIndex {
  /** `<repo>:<path>` → the section anchors the corpus holds for that file. */
  files: Map<string, Set<string>>;
}

interface CorpusNodeLike {
  source_ref?: { Artifact?: { repo?: unknown; path?: unknown } };
  content?: unknown;
}

const BREADCRUMB_SEP = " › ";

/** Build the index from the text of each tenant file (`tenants/<tenant>.corpus.json`). */
export function buildCorpusCitationIndex(tenantTexts: string[]): CorpusCitationIndex {
  const files = new Map<string, Set<string>>();
  for (const text of tenantTexts) {
    const bundle = JSON.parse(text) as { nodes?: CorpusNodeLike[] };
    for (const n of bundle.nodes ?? []) {
      const a = n.source_ref?.Artifact;
      if (!a || typeof a.repo !== "string" || typeof a.path !== "string") continue;
      const key = `${a.repo}:${a.path}`;
      const anchors = files.get(key) ?? new Set<string>();
      files.set(key, anchors);
      if (typeof n.content !== "string") continue;
      const first = n.content.split("\n", 1)[0];
      const at = first.lastIndexOf(BREADCRUMB_SEP);
      if (at < 0) continue;
      const anchor = slugify(first.slice(at + BREADCRUMB_SEP.length));
      if (anchor) anchors.add(anchor);
    }
  }
  return { files };
}
