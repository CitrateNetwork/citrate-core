// Test fixture (WP-H8): a collection plan in the exact shape core's fl_collect_plan returns.
// Imported only by tests; production plans come from core.
import type { FlCollectPlan, FlRunReport, FlSourceCheck } from "../collect";

export const DATASET = "ds-0123456789abcdef";

export function allowedSource(over: Partial<FlSourceCheck> = {}): FlSourceCheck {
  return {
    url: "https://en.wikipedia.org/wiki/Apple",
    host: "en.wikipedia.org",
    allowed: true,
    refusal: null,
    policy: "allowlisted",
    robots: "allowed",
    license: { id: "CC-BY-SA-4.0", basis: "found", evidence: "https://en.wikipedia.org/wiki/Wikipedia:Copyrights" },
    intervalSecs: 2,
    ...over,
  };
}

export function collectPlan(over: Partial<FlCollectPlan> = {}): FlCollectPlan {
  return {
    id: "cp-0011223344556677",
    dataset: DATASET,
    baseVersion: 1,
    sources: [
      allowedSource(),
      allowedSource({
        url: "https://www.linkedin.com/in/someone",
        host: "www.linkedin.com",
        allowed: false,
        refusal: "www.linkedin.com is on Citrate's do-not-collect list: its terms forbid automated collection, and its pages are personal profiles",
        policy: "denied",
        robots: "not-checked",
        license: { id: null, basis: "unknown", evidence: null },
      }),
    ],
    pages: 1,
    memberApprovedHosts: [],
    statement: "Collect 1 page from en.wikipedia.org into dataset ds-0123456789abcdef, honouring robots.txt and at most one request every 2 s per host. Each item records its URL, time, hash and license.",
    status: "proposed",
    createdAtMs: 1,
    policyVersion: 1,
    ...over,
  };
}

export function runReport(over: Partial<FlRunReport> = {}): FlRunReport {
  return {
    plan: "cp-0011223344556677",
    pagesFetched: 1,
    outcomes: [{ url: "https://en.wikipedia.org/wiki/Apple", collected: true, rows: 2, droppedSecretRows: 0, note: null }],
    stopped: null,
    version: { dataset: DATASET, version: 2, rows: 3, manifestHash: "0x" + "ab".repeat(32) },
    piiScan: { passed: true, counts: {} },
    untrusted: true,
    ...over,
  };
}
