// =====================================================================
// citrate-core — versioned eval datasets built from fragment files (HUP-S1.7 / HUP-S1.10, A50)
//
// v1 datasets are frozen (src/agent/eval/datasetHashes.test.ts pins their bytes). A v2 dataset is
// a small header file plus a fragment directory:
//
//   toolcall-v2.json      {version, provenance, includes: ["toolcall-v1.json"], fragments: "toolcall-v2.d"}
//   toolcall-v2.d/*.json  {added, by, note?, tasks: [...]}     (injection: cases: [...])
//
// A lane that adds a tool adds its eval items as a NEW fragment file; nobody edits a shared list,
// and nobody edits v1. The merge is pure (this file); reading the files is in datasetFiles.ts.
// The merged object is then validated by the same parsers as v1 (runner.ts), so every item is
// still checked against the real AGENT_TOOLS.
//
// Imports carry explicit `.ts` extensions so the CLIs can load this with Node's type stripping.
// =====================================================================

export type DatasetKind = "toolcall" | "injection";

/** One fragment file, as read from disk. */
export interface FragmentFile {
  /** The file name inside the fragment directory (used in error messages and for ordering). */
  file: string;
  data: unknown;
}

/** The header of a v2 dataset. */
export interface DatasetHeader {
  version: string;
  provenance: unknown;
  /** Earlier dataset files (relative to the header) whose items are included unchanged. */
  includes: string[];
  /** The fragment directory, relative to the header. */
  fragments: string;
}

const FRAGMENT_NAME = /^[0-9a-z][0-9a-z._-]*\.json$/;
const HEADER_KEYS = new Set(["version", "provenance", "includes", "fragments"]);
const FRAGMENT_META = new Set(["added", "by", "note"]);

function isObj(v: unknown): v is Record<string, unknown> {
  return typeof v === "object" && v !== null && !Array.isArray(v);
}

export function itemsKey(kind: DatasetKind): "tasks" | "cases" {
  return kind === "toolcall" ? "tasks" : "cases";
}

/** Check a v2 header's shape. Unknown keys are refused so a typo never changes what is scored. */
export function parseHeader(raw: unknown, where: string): DatasetHeader {
  if (!isObj(raw)) throw new Error(`${where}: header must be an object`);
  for (const k of Object.keys(raw)) {
    if (!HEADER_KEYS.has(k)) throw new Error(`${where}: unknown header key ${k}`);
  }
  if (typeof raw.version !== "string" || !raw.version.trim()) throw new Error(`${where}: version required`);
  if (!Array.isArray(raw.includes) || !raw.includes.every((s) => typeof s === "string" && FRAGMENT_NAME.test(s))) {
    throw new Error(`${where}: includes must be a list of sibling dataset file names`);
  }
  if (typeof raw.fragments !== "string" || !/^[0-9a-z][0-9a-z._-]*\.d$/.test(raw.fragments)) {
    throw new Error(`${where}: fragments must name a sibling directory ending in .d`);
  }
  return {
    version: raw.version,
    provenance: raw.provenance,
    includes: raw.includes as string[],
    fragments: raw.fragments,
  };
}

/** Check one fragment file and return its items. */
export function parseFragment(kind: DatasetKind, f: FragmentFile): unknown[] {
  const key = itemsKey(kind);
  const where = `fragment ${f.file}`;
  if (!FRAGMENT_NAME.test(f.file)) throw new Error(`${where}: file names are lowercase [0-9a-z._-] and end in .json`);
  if (!isObj(f.data)) throw new Error(`${where}: must be a JSON object`);
  for (const k of Object.keys(f.data)) {
    if (k !== key && !FRAGMENT_META.has(k)) throw new Error(`${where}: unknown key ${k} (a ${kind} fragment holds ${key})`);
  }
  if (typeof f.data.added !== "string" || !/^\d{4}-\d{2}-\d{2}$/.test(f.data.added)) {
    throw new Error(`${where}: added must be a YYYY-MM-DD date`);
  }
  if (typeof f.data.by !== "string" || !f.data.by.trim()) throw new Error(`${where}: by (the WP or lane that added it) required`);
  const items = f.data[key];
  if (!Array.isArray(items) || items.length === 0) throw new Error(`${where}: ${key} must be a non-empty list`);
  return items;
}

/** Merge included datasets and fragments (in file-name order) into one raw dataset object for the
 *  v1 parsers. A duplicate id anywhere is refused, naming both places. */
export function mergeDataset(
  kind: DatasetKind,
  header: DatasetHeader,
  included: { file: string; data: unknown }[],
  fragments: FragmentFile[],
): Record<string, unknown> {
  const key = itemsKey(kind);
  const items: unknown[] = [];
  const seen = new Map<string, string>();
  const add = (list: unknown[], from: string) => {
    for (const it of list) {
      const id = isObj(it) ? it.id : undefined;
      if (typeof id !== "string" || !id) throw new Error(`${from}: every item needs a string id`);
      const prev = seen.get(id);
      if (prev) throw new Error(`duplicate id ${id} in ${from} (already in ${prev})`);
      seen.set(id, from);
      items.push(it);
    }
  };
  for (const inc of included) {
    if (!isObj(inc.data) || !Array.isArray(inc.data[key])) throw new Error(`include ${inc.file}: not a ${kind} dataset`);
    add(inc.data[key] as unknown[], `include ${inc.file}`);
  }
  const ordered = [...fragments].sort((a, b) => (a.file < b.file ? -1 : a.file > b.file ? 1 : 0));
  for (const f of ordered) add(parseFragment(kind, f), `fragment ${f.file}`);
  return { version: header.version, provenance: header.provenance, [key]: items };
}
