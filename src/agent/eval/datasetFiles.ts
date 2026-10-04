// =====================================================================
// citrate-core — read eval datasets from disk (HUP-S1.7 / HUP-S1.10), for the CLIs and tests.
//
// `toolcall-v1` / `injection-v1` are single frozen files. `toolcall-v2` / `injection-v2` are a
// header plus a fragment directory (fragments.ts). Both come back validated by runner.ts.
// The callers pass in the two file operations (Node fs), so this module stays free of
// Node imports and type-checks with the app's DOM-only tsconfig.
// =====================================================================
import { mergeDataset, parseHeader, type DatasetKind, type FragmentFile } from "./fragments.ts";
import {
  parseInjectionDataset,
  parseToolcallDataset,
  type InjectionDataset,
  type ToolcallDataset,
} from "./runner.ts";

/** The dataset versions the CLIs accept. */
export const DATASET_GENERATIONS = ["v1", "v2"] as const;
export type DatasetGeneration = (typeof DATASET_GENERATIONS)[number];

/** The two file operations the loader needs (Node: readFileSync(p, "utf8") and readdirSync). */
export interface DatasetFs {
  readText(path: string): string;
  listDir(path: string): string[];
}

const join = (a: string, b: string) => `${a.replace(/[\\/]+$/, "")}/${b}`;

/** Read a dataset file; a header (it has `fragments`) is expanded with its includes + fragments. */
export function readRawDataset(fs: DatasetFs, kind: DatasetKind, dir: string, file: string): unknown {
  const readJson = (p: string): unknown => JSON.parse(fs.readText(p));
  const raw = readJson(join(dir, file));
  if (typeof raw !== "object" || raw === null || !("fragments" in raw)) return raw;
  const header = parseHeader(raw, file);
  const included = header.includes.map((inc) => ({ file: inc, data: readJson(join(dir, inc)) }));
  const fragDir = join(dir, header.fragments);
  const fragments: FragmentFile[] = fs
    .listDir(fragDir)
    .filter((f) => f.endsWith(".json"))
    .map((f) => ({ file: f, data: readJson(join(fragDir, f)) }));
  return mergeDataset(kind, header, included, fragments);
}

export function loadToolcallDataset(fs: DatasetFs, dir: string, gen: DatasetGeneration): ToolcallDataset {
  return parseToolcallDataset(readRawDataset(fs, "toolcall", dir, `toolcall-${gen}.json`));
}

export function loadInjectionDataset(fs: DatasetFs, dir: string, gen: DatasetGeneration): InjectionDataset {
  return parseInjectionDataset(readRawDataset(fs, "injection", dir, `injection-${gen}.json`));
}
