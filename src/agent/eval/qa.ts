// =====================================================================
// citrate-core — Citrate QA eval set v1: dataset schema, anchor index, deterministic scorer
// (HUP-S3.5, US-3.1 "it knows Citrate out of the box")
//
// qa-v1.json holds 150 hand-authored questions over real, public Citrate material, each with
// answer key points and the citation(s) that support it (repo + path + heading anchor). The
// anchor index (qa-v1.anchors.json) records, for every cited file at the pinned commit of its
// source repo, the git blob id and every heading anchor, so a citation can be checked offline.
// scripts/qa-anchors.mjs regenerates and re-verifies that index from the local source repos.
//
// Scoring is deterministic (planset red-team correction #4: no model-as-judge):
//   - key-point coverage: each key point is a list of acceptable phrasings, matched as a
//     normalized, case-insensitive substring of the answer;
//   - citation validity: every `<source>:<path>#<anchor>` the answer cites must exist in the
//     anchor index; a required citation is hit when its source+path is cited (anchor optional);
//   - abstention: an unanswerable item passes only when the answer admits it is not documented
//     (the same refusal markers as the W3.4 harness, src/agent/eval.ts) and cites nothing invalid.
// Pure: no I/O. The CLI (scripts/eval-qa.mjs) does the file and network work.
// =====================================================================

import { admitsUncertainty } from "../eval.ts";

export const QA_CATEGORIES = [
  "chain-basics",
  "staking-validators",
  "wallet",
  "node-ops",
  "ai-inference",
  "memory",
  "agentile-hic",
  "devtools-contracts",
  "paraconsensus-fl",
  "governance",
] as const;
export type QaCategory = (typeof QA_CATEGORIES)[number];

export const QA_DIFFICULTIES = ["easy", "medium", "hard"] as const;
export type QaDifficulty = (typeof QA_DIFFICULTIES)[number];

export interface QaCitation {
  /** Key into `sources` (e.g. "citrate-docs"). */
  source: string;
  /** Repo-relative file path at the pinned commit. */
  path: string;
  /** Heading anchor (GitHub-style slug) inside that file. */
  anchor: string;
}

export interface QaItem {
  id: string;
  question: string;
  category: QaCategory;
  difficulty: QaDifficulty;
  /** false = the right answer is "not documented"; the item tests abstention. */
  answerable: boolean;
  /** Each key point is a non-empty list of acceptable phrasings (any one matches). */
  keyPoints: string[][];
  /** The citation(s) that support the answer. Empty for unanswerable items. */
  citations: QaCitation[];
  note?: string;
}

export interface QaSource {
  repo: string;
  commit: string;
  visibility: "public";
}

export interface QaDataset {
  version: string;
  provenance: {
    author: string;
    created: string;
    purpose: string;
    disjointFromTraining: true;
    note?: string;
  };
  sources: Record<string, QaSource>;
  items: QaItem[];
}

export interface AnchorIndexFile {
  /** `git rev-parse <commit>:<path>` — pins the exact file content the anchors came from. */
  blob: string;
  anchors: string[];
}

export interface AnchorIndex {
  version: string;
  sources: Record<string, { repo: string; commit: string; files: Record<string, AnchorIndexFile> }>;
}

// ------------------------------------------------------------------ markdown anchors

/** GitHub-style heading slug: lowercase, drop punctuation except `-` and `_`, spaces → `-`. */
export function slugify(heading: string): string {
  return heading
    .trim()
    .toLowerCase()
    .replace(/[^\p{L}\p{N}\s_-]/gu, "")
    .replace(/\s/g, "-");
}

interface Heading {
  level: number;
  text: string;
  anchor: string;
  line: number;
}

/** ATX headings outside fenced code and YAML frontmatter, with de-duplicated anchors. */
export function extractHeadings(markdown: string): Heading[] {
  const lines = markdown.split(/\r?\n/);
  const out: Heading[] = [];
  const seen = new Map<string, number>();
  let inFence = false;
  let start = 0;
  if (lines[0] === "---") {
    const end = lines.indexOf("---", 1);
    if (end > 0) start = end + 1;
  }
  for (let i = start; i < lines.length; i++) {
    const line = lines[i];
    if (/^\s{0,3}(```|~~~)/.test(line)) {
      inFence = !inFence;
      continue;
    }
    if (inFence) continue;
    const m = /^\s{0,3}(#{1,6})\s+(.+?)\s*#*\s*$/.exec(line);
    if (!m) continue;
    const base = slugify(m[2]);
    const n = seen.get(base) ?? 0;
    seen.set(base, n + 1);
    out.push({ level: m[1].length, text: m[2], anchor: n === 0 ? base : `${base}-${n}`, line: i });
  }
  return out;
}

export function extractAnchors(markdown: string): string[] {
  return extractHeadings(markdown).map((h) => h.anchor);
}

/** The text under one heading, up to the next heading of the same or a higher level. */
export function extractSection(markdown: string, anchor: string): string | null {
  const lines = markdown.split(/\r?\n/);
  const hs = extractHeadings(markdown);
  const idx = hs.findIndex((h) => h.anchor === anchor);
  if (idx < 0) return null;
  const h = hs[idx];
  const next = hs.slice(idx + 1).find((x) => x.level <= h.level);
  return lines.slice(h.line, next ? next.line : lines.length).join("\n");
}

// ------------------------------------------------------------------ dataset validation

const ID_RE = /^qa-[a-z0-9]+(-[a-z0-9]+)*$/;
const SHA_RE = /^[0-9a-f]{40}$/;

function fail(msg: string): never {
  throw new Error(`qa dataset: ${msg}`);
}

function isObj(x: unknown): x is Record<string, unknown> {
  return typeof x === "object" && x !== null && !Array.isArray(x);
}

/** Validate the dataset shape. Throws with a readable reason on the first defect. */
export function parseQaDataset(raw: unknown): QaDataset {
  if (!isObj(raw)) fail("not an object");
  if (typeof raw.version !== "string" || !/^qa-v\d+$/.test(raw.version)) fail("version must look like qa-vN");
  const p = raw.provenance;
  if (!isObj(p)) fail("missing provenance");
  for (const k of ["author", "created", "purpose"]) {
    if (typeof p[k] !== "string" || (p[k] as string).length === 0) fail(`provenance.${k} required`);
  }
  if (p.disjointFromTraining !== true) fail("provenance.disjointFromTraining must be true (eval/training disjointness)");
  if (!isObj(raw.sources)) fail("missing sources");
  const sources = raw.sources as Record<string, unknown>;
  for (const [name, s] of Object.entries(sources)) {
    if (!isObj(s)) fail(`source ${name} malformed`);
    if (typeof s.repo !== "string" || !/^[A-Za-z0-9-]+\/[A-Za-z0-9._-]+$/.test(s.repo)) fail(`source ${name}.repo`);
    if (typeof s.commit !== "string" || !SHA_RE.test(s.commit)) fail(`source ${name}.commit must be a full 40-hex sha`);
    if (s.visibility !== "public") fail(`source ${name} must be public-tier`);
  }
  if (!Array.isArray(raw.items)) fail("items must be an array");
  const ids = new Set<string>();
  for (const it of raw.items as unknown[]) {
    if (!isObj(it)) fail("item not an object");
    const id = it.id;
    if (typeof id !== "string" || !ID_RE.test(id)) fail(`bad id ${JSON.stringify(id)}`);
    if (ids.has(id)) fail(`duplicate id ${id}`);
    ids.add(id);
    if (typeof it.question !== "string" || it.question.trim().length < 10) fail(`${id}: question too short`);
    if (!QA_CATEGORIES.includes(it.category as QaCategory)) fail(`${id}: unknown category ${String(it.category)}`);
    if (!QA_DIFFICULTIES.includes(it.difficulty as QaDifficulty)) fail(`${id}: unknown difficulty`);
    if (typeof it.answerable !== "boolean") fail(`${id}: answerable must be boolean`);
    if (!Array.isArray(it.keyPoints) || !Array.isArray(it.citations)) fail(`${id}: keyPoints/citations must be arrays`);
    for (const kp of it.keyPoints as unknown[]) {
      if (!Array.isArray(kp) || kp.length === 0 || !kp.every((s) => typeof s === "string" && s.trim().length > 0)) {
        fail(`${id}: each key point is a non-empty list of non-empty strings`);
      }
    }
    for (const c of it.citations as unknown[]) {
      if (!isObj(c)) fail(`${id}: citation not an object`);
      if (typeof c.source !== "string" || !(c.source in sources)) fail(`${id}: citation source ${String(c.source)} not in sources`);
      if (typeof c.path !== "string" || c.path.startsWith("/") || c.path.includes("..")) fail(`${id}: citation path must be repo-relative`);
      if (typeof c.anchor !== "string" || c.anchor !== slugify(c.anchor) || c.anchor.length === 0) fail(`${id}: citation anchor must be a slug`);
    }
    if (it.answerable) {
      if ((it.keyPoints as unknown[]).length === 0) fail(`${id}: an answerable item needs key points`);
      if ((it.citations as unknown[]).length === 0) fail(`${id}: an answerable item needs a citation`);
    } else if ((it.keyPoints as unknown[]).length > 0 || (it.citations as unknown[]).length > 0) {
      fail(`${id}: an unanswerable item carries no key points or citations`);
    }
  }
  return raw as unknown as QaDataset;
}

/** Every citation in the dataset must resolve in the anchor index at the same pinned commit. */
export function findMissingCitations(ds: QaDataset, index: AnchorIndex): string[] {
  const missing: string[] = [];
  for (const [name, s] of Object.entries(ds.sources)) {
    const idx = index.sources[name];
    if (!idx) missing.push(`source ${name}: not in anchor index`);
    else if (idx.commit !== s.commit) missing.push(`source ${name}: index commit ${idx.commit} != dataset ${s.commit}`);
  }
  for (const it of ds.items) {
    for (const c of it.citations) {
      const f = index.sources[c.source]?.files[c.path];
      if (!f) missing.push(`${it.id}: ${c.source}:${c.path} not indexed`);
      else if (!f.anchors.includes(c.anchor)) missing.push(`${it.id}: ${c.source}:${c.path}#${c.anchor} has no such heading`);
    }
  }
  return missing;
}

// ------------------------------------------------------------------ scoring

/**
 * Lowercase, drop markdown code/bold marks (backticks, asterisks; underscores stay because they are
 * part of identifiers like `eth_` and `CITRATE_RPC_URL`), unify dashes/quotes, strip digit-group commas.
 */
export function normalizeText(s: string): string {
  return s
    .toLowerCase()
    .replace(/[`*]/g, "")
    .replace(/[‐-―−]/g, "-")
    .replace(/[‘’]/g, "'")
    .replace(/[“”]/g, '"')
    .replace(/(\d),(?=\d{3}\b)/g, "$1")
    .replace(/\s+/g, " ")
    .trim();
}

export function keyPointMatched(answer: string, alternatives: string[]): boolean {
  const a = normalizeText(answer);
  return alternatives.some((alt) => a.includes(normalizeText(alt)));
}

/** Fraction of key points the answer covers. No key points → 1. */
export function keyPointCoverage(answer: string, keyPoints: string[][]): number {
  if (keyPoints.length === 0) return 1;
  return keyPoints.filter((kp) => keyPointMatched(answer, kp)).length / keyPoints.length;
}

export interface CitedRef {
  source: string;
  path: string;
  anchor?: string;
}

const CITE_RE = /\b([a-z0-9][a-z0-9-]*):([A-Za-z0-9_./-]+\.(?:md|mdx|tex|txt))(?:#([A-Za-z0-9_-]+))?/g;

/** Extract `<source>:<path>[#anchor]` citations from free text, de-duplicated, in order. */
export function extractCitations(answer: string): CitedRef[] {
  const out: CitedRef[] = [];
  const seen = new Set<string>();
  for (const m of answer.matchAll(CITE_RE)) {
    const ref: CitedRef = { source: m[1], path: m[2] };
    if (m[3]) ref.anchor = m[3].toLowerCase();
    const key = `${ref.source}:${ref.path}#${ref.anchor ?? ""}`;
    if (!seen.has(key)) {
      seen.add(key);
      out.push(ref);
    }
  }
  return out;
}

/** A cited ref is valid when its file is in the index and its anchor (if any) is a real heading. */
export function citationValid(ref: CitedRef, index: AnchorIndex): boolean {
  const f = index.sources[ref.source]?.files[ref.path];
  if (!f) return false;
  return ref.anchor === undefined || f.anchors.includes(ref.anchor);
}

export interface QaItemScore {
  id: string;
  category: QaCategory;
  answerable: boolean;
  coverage: number;
  /** Answerable: at least one required citation's source+path was cited. Unanswerable: n/a (true). */
  citationHit: boolean;
  /** Count of cited refs that resolve / do not resolve in the anchor index. */
  citedValid: number;
  citedInvalid: number;
  abstained: boolean;
  pass: boolean;
  reasons: string[];
}

export interface QaScoreOptions {
  /** Minimum key-point coverage for an answerable item to pass (default 0.6). */
  coverageThreshold?: number;
}

/** Score one answer. `extraCitations` are structured citations a provider returned out-of-band. */
export function scoreQaItem(
  item: QaItem,
  answer: string,
  index: AnchorIndex,
  extraCitations: CitedRef[] = [],
  opts: QaScoreOptions = {},
): QaItemScore {
  const threshold = opts.coverageThreshold ?? 0.6;
  const cited = [...extractCitations(answer), ...extraCitations];
  const citedValid = cited.filter((r) => citationValid(r, index)).length;
  const citedInvalid = cited.length - citedValid;
  const abstained = admitsUncertainty(answer);
  const reasons: string[] = [];
  if (citedInvalid > 0) reasons.push(`${citedInvalid} citation(s) do not resolve`);

  if (!item.answerable) {
    if (!abstained) reasons.push("unanswerable item answered without admitting it is not documented");
    return {
      id: item.id,
      category: item.category,
      answerable: false,
      coverage: 1,
      citationHit: true,
      citedValid,
      citedInvalid,
      abstained,
      pass: abstained && citedInvalid === 0,
      reasons,
    };
  }

  const coverage = keyPointCoverage(answer, item.keyPoints);
  const citationHit = item.citations.some((req) =>
    cited.some((r) => r.source === req.source && r.path === req.path && citationValid(r, index)),
  );
  if (coverage < threshold) reasons.push(`key-point coverage ${coverage.toFixed(2)} < ${threshold}`);
  if (!citationHit) reasons.push("no required citation cited");
  return {
    id: item.id,
    category: item.category,
    answerable: true,
    coverage,
    citationHit,
    citedValid,
    citedInvalid,
    abstained,
    pass: coverage >= threshold && citationHit && citedInvalid === 0,
    reasons,
  };
}

export interface QaScorecard {
  datasetVersion: string;
  model: string;
  tier?: string;
  startedAt: string;
  n: number;
  passRate: number;
  /** Mean key-point coverage over answerable items. */
  keyPointCoverage: number;
  /** Answerable items that cited a required source. */
  citationHitRate: number;
  /** Valid cited refs / all cited refs (null when nothing was cited). */
  citationValidity: number | null;
  /** Unanswerable items where the model abstained. */
  abstentionRate: number | null;
  /** Answerable items where the model abstained anyway. */
  falseAbstentionRate: number | null;
  byCategory: Record<string, { n: number; passRate: number }>;
  failures: string[];
  failureReasons: Record<string, string[]>;
}

const mean = (xs: number[]): number | null => (xs.length ? xs.reduce((a, b) => a + b, 0) / xs.length : null);

export function aggregateQa(
  scores: QaItemScore[],
  meta: { datasetVersion: string; model: string; tier?: string; startedAt: string },
): QaScorecard {
  const ans = scores.filter((s) => s.answerable);
  const un = scores.filter((s) => !s.answerable);
  const citedAll = scores.reduce((a, s) => a + s.citedValid + s.citedInvalid, 0);
  const citedOk = scores.reduce((a, s) => a + s.citedValid, 0);
  const byCategory: Record<string, { n: number; passRate: number }> = {};
  for (const c of QA_CATEGORIES) {
    const xs = scores.filter((s) => s.category === c);
    if (xs.length) byCategory[c] = { n: xs.length, passRate: xs.filter((s) => s.pass).length / xs.length };
  }
  const failures = scores.filter((s) => !s.pass);
  const card: QaScorecard = {
    datasetVersion: meta.datasetVersion,
    model: meta.model,
    startedAt: meta.startedAt,
    n: scores.length,
    passRate: scores.length ? scores.filter((s) => s.pass).length / scores.length : 0,
    keyPointCoverage: mean(ans.map((s) => s.coverage)) ?? 0,
    citationHitRate: mean(ans.map((s) => (s.citationHit ? 1 : 0))) ?? 0,
    citationValidity: citedAll ? citedOk / citedAll : null,
    abstentionRate: mean(un.map((s) => (s.abstained ? 1 : 0))),
    falseAbstentionRate: mean(ans.map((s) => (s.abstained ? 1 : 0))),
    byCategory,
    failures: failures.map((s) => s.id),
    failureReasons: Object.fromEntries(failures.map((s) => [s.id, s.reasons])),
  };
  if (meta.tier !== undefined) card.tier = meta.tier;
  return card;
}

// ------------------------------------------------------------------ run loop

/** The instruction every QA run sends. Answers must cite `<source>:<path>#<anchor>`. */
export const QA_SYSTEM_PROMPT =
  "You are Hermes, the Citrate assistant. Answer the question about Citrate using only the bundled " +
  "Citrate documentation. Cite every source you rely on in the form <source>:<path>#<anchor>, for " +
  "example citrate-docs:content/chain/genesis.md#reference. If the documentation does not cover the " +
  "question, say plainly that it is not documented and do not guess.";

export interface QaRunDeps {
  /** Ask the model one question; return the final answer text (and optional structured citations). */
  ask: (question: string) => Promise<{ text: string; citations?: CitedRef[] }>;
  onProgress?: (id: string, pass: boolean) => void;
}

/**
 * Run every item sequentially. A transport error aborts the run (Rule 1: no partial or invented
 * scorecard); the caller writes nothing in that case.
 */
export async function runQaEval(
  ds: QaDataset,
  index: AnchorIndex,
  deps: QaRunDeps,
  meta: { model: string; tier?: string; startedAt?: string },
  opts: QaScoreOptions = {},
): Promise<{ scorecard: QaScorecard; items: (QaItemScore & { answer: string })[] }> {
  const startedAt = meta.startedAt ?? new Date().toISOString();
  const items: (QaItemScore & { answer: string })[] = [];
  for (const it of ds.items) {
    const res = await deps.ask(it.question);
    const s = scoreQaItem(it, res.text, index, res.citations ?? [], opts);
    items.push({ ...s, answer: res.text });
    deps.onProgress?.(it.id, s.pass);
  }
  const card = aggregateQa(items, { datasetVersion: ds.version, model: meta.model, tier: meta.tier, startedAt });
  return { scorecard: card, items };
}

// ------------------------------------------------------------------ answer-key grounding

/**
 * Key points of an answerable item that do NOT appear (normalized) in the text of its cited
 * sections. An empty result means the answer key is traceable to the cited source text.
 * Used by scripts/qa-anchors.mjs and the live test when the source repos are present locally.
 */
export function ungroundedKeyPoints(item: QaItem, citedSectionText: string): string[][] {
  return item.keyPoints.filter((kp) => !keyPointMatched(citedSectionText, kp));
}
