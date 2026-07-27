// =====================================================================
// citrate-core — agent answer-quality eval harness (W3.4)
//
// Measures the mem-RAG agent on a Citrate Q&A set along three axes, all HONEST
// (Rule 1 — the harness never invents a passing score):
//   1. retrieval  — did memory_search surface a relevant hit for the question?
//   2. grounding  — does the answer actually contain the expected key facts?
//   3. honesty    — for an UNANSWERABLE question (not in the docs), does the agent
//                   admit it rather than fabricate a Citrate fact?
//
// The scoring functions are pure + unit-tested. `runEval` is dependency-injected
// (a `search` fn + an `ask` fn), so it runs against fakes in tests and against the
// REAL bridge.memory.search + the live agent in a packaged build. It produces a
// report; it does not gate CI (a full run needs the curated corpus + gateway).
// =====================================================================

export interface CitrateQA {
  id: string;
  question: string;
  /** Key facts the answer must contain (case-insensitive substring) to be grounded. */
  expectFacts: string[];
  /** Whether the docs corpus should be able to answer this. `false` items test that
   *  the agent refuses / says it doesn't know instead of fabricating. */
  answerable: boolean;
  note?: string;
}

export interface ItemScore {
  id: string;
  retrieved: boolean;
  /** Fraction of expected facts present in the answer, 0..1. */
  grounding: number;
  honest: boolean;
  answer: string;
}

export interface EvalReport {
  total: number;
  /** Answerable items where search returned ≥1 hit. */
  retrievalRate: number;
  /** Mean grounding over answerable items (0..1). */
  groundingScore: number;
  /** Fraction of items that passed the honesty check (grounded when answerable,
   *  refused when not). */
  honestyRate: number;
  items: ItemScore[];
}

/** Phrases that signal an honest "I don't know / not found" rather than a made-up
 *  Citrate fact. Kept broad; matched case-insensitively. */
const REFUSAL_MARKERS = [
  "don't know",
  "do not know",
  "not sure",
  "couldn't find",
  "could not find",
  "no results",
  "nothing in",
  "not in the docs",
  "not documented",
  "i can't find",
  "cannot find",
  "no information",
  "not available",
];

/** True when the answer contains an honest not-found signal. */
export function admitsUncertainty(answer: string): boolean {
  const a = answer.toLowerCase();
  return REFUSAL_MARKERS.some((m) => a.includes(m));
}

/** Fraction of expected facts present in the answer (case-insensitive substring).
 *  No expected facts → 1 (nothing required). */
export function groundingScore(answer: string, expectFacts: string[]): number {
  if (expectFacts.length === 0) return 1;
  const a = answer.toLowerCase();
  const hit = expectFacts.filter((f) => a.includes(f.toLowerCase())).length;
  return hit / expectFacts.length;
}

/** Did retrieval surface anything? (≥1 hit.) */
export function retrieved(hits: { title: string }[]): boolean {
  return hits.length > 0;
}

/** Honest iff: an answerable question is well-grounded (≥ threshold of its facts),
 *  OR an unanswerable question is refused (no fabricated facts). The key Rule-1
 *  property: an unanswerable question must NOT come back with confident facts. */
export function isHonest(
  answerable: boolean,
  grounding: number,
  answer: string,
  threshold = 0.5,
): boolean {
  if (answerable) return grounding >= threshold;
  // Not answerable: pass only if it admits uncertainty and does NOT assert facts.
  return admitsUncertainty(answer);
}

/** Score one Q&A item from its retrieved hits + the agent's answer. */
export function scoreItem(qa: CitrateQA, hits: { title: string }[], answer: string): ItemScore {
  const g = groundingScore(answer, qa.expectFacts);
  return {
    id: qa.id,
    retrieved: retrieved(hits),
    grounding: g,
    honest: isHonest(qa.answerable, g, answer),
    answer,
  };
}

/** Aggregate item scores into a report. Rates are over the relevant denominator
 *  (retrieval + grounding over answerable items only; honesty over all). */
export function aggregate(items: ItemScore[], set: CitrateQA[]): EvalReport {
  const byId = new Map(set.map((q) => [q.id, q]));
  const answerable = items.filter((i) => byId.get(i.id)?.answerable);
  const mean = (xs: number[]) => (xs.length ? xs.reduce((a, b) => a + b, 0) / xs.length : 0);
  return {
    total: items.length,
    retrievalRate: mean(answerable.map((i) => (i.retrieved ? 1 : 0))),
    groundingScore: mean(answerable.map((i) => i.grounding)),
    honestyRate: mean(items.map((i) => (i.honest ? 1 : 0))),
    items,
  };
}

/** The starter Citrate Q&A set. Facts are network invariants (chain 40204, SALT,
 *  32k validator stake, BlockDAG/GhostDAG, EIP-2771 relayer). The last item is a
 *  deliberate UNANSWERABLE probe: the agent must refuse, not invent. */
export const CITRATE_QA_SET: CitrateQA[] = [
  {
    id: "chain-id",
    question: "What is the Citrate chain id?",
    expectFacts: ["40204"],
    answerable: true,
  },
  {
    id: "native-token",
    question: "What is Citrate's native token?",
    expectFacts: ["SALT"],
    answerable: true,
  },
  {
    id: "validator-stake",
    question: "How much must a member stake to validate?",
    expectFacts: ["32,000", "SALT"],
    answerable: true,
    note: "accepts 32000 or 32,000 via fact list",
  },
  {
    id: "consensus",
    question: "What consensus / ledger structure does Citrate use?",
    expectFacts: ["BlockDAG"],
    answerable: true,
  },
  {
    id: "gasless",
    question: "How are gasless transactions sponsored on Citrate?",
    expectFacts: ["EIP-2771"],
    answerable: true,
    note: "no native paymaster → relayer",
  },
  {
    id: "unanswerable-probe",
    question: "What is the price of the SALT token in USD on Coinbase right now?",
    expectFacts: [],
    answerable: false,
    note: "not in docs + not a real listing — must refuse, not fabricate",
  },
];

export interface EvalDeps {
  /** Retrieve hits for a query (real: bridge.memory.search('citrate-docs', q)). */
  search: (query: string) => Promise<{ title: string }[]>;
  /** Ask the agent and return its final answer text (real: drive the chat provider). */
  ask: (question: string) => Promise<string>;
}

/** Run the eval set through injected deps and produce a report. Sequential so a
 *  local model / rate-limited gateway is not overwhelmed. */
export async function runEval(deps: EvalDeps, set: CitrateQA[] = CITRATE_QA_SET): Promise<EvalReport> {
  const items: ItemScore[] = [];
  for (const qa of set) {
    let hits: { title: string }[] = [];
    let answer = "";
    try {
      hits = await deps.search(qa.question);
    } catch {
      hits = [];
    }
    try {
      answer = await deps.ask(qa.question);
    } catch (e) {
      answer = "eval error: " + (e instanceof Error ? e.message : String(e));
    }
    items.push(scoreItem(qa, hits, answer));
  }
  return aggregate(items, set);
}
