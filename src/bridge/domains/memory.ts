// citrate-core: bridge domain `memory`, moved verbatim from src/bridge/domains.ts by L0-WP05
// (planset v060). src/bridge/domains.ts re-exports it, so imports from "../bridge/domains" keep
// working. The owning lane is the row for this path in .agentile/v60-ownership.map.

// The citrate-memories mcp_serve daemon under the SidecarSupervisor (CORE-C3).
// recall/search/neighbors speak the daemon's JSON-RPC over its Unix socket and
// return REAL parsed nodes/edges from the per-user encrypted store — never a
// fabricated graph (Rule 1). `status` carries the supervisor state + socket path
// (a local path, not a secret); `constellation` recalls the personal +
// chain-state tenants for the Storage graph. `assert` writes one member-approved
// fact into the personal tenant (Hermes's memory_assert, after its HIC-1 card).
export interface MemoryHit {
  id: string;
  kind: string;
  title: string;
  status?: string;
  /** HUP-S3.1: `<repo>:<path>[#<anchor>]` of a document passage (search with `passages`). */
  cite?: string;
  /** HUP-S3.1: the hit's text (search with `passages`). */
  passage?: string;
}
export interface MemoryResult {
  tenant: string;
  totalInTenant: number;
  hits: MemoryHit[];
}
export interface MemoryNeighbor {
  direction: "out" | "in";
  kind: string;
  title: string;
  proposed: boolean;
}
export interface MemoryStatus {
  state: string;
  socketPath: string;
  semantic: boolean;
}
/** W3.2 — result of the first-run docs preload (or why it was skipped). */
export interface DocsIngestReport {
  docs: number;
  chunks: number;
  /** "not-semantic" | "not-running" | "already-seeded" | "empty-corpus" when nothing ran. */
  skipped?: string;
}

/** Live values the seed composes real facts from (structured, never free-form text). */
export interface SeedFacts {
  chainId: number;
  nodeState: string;
  height: number;
  peers: number;
  walletAddr: string;
  hasGrant: boolean;
  grantStakedSalt: number;
  bondStatus: string;
  hasSbt: boolean;
}
export interface SeedReport {
  authored: number;
  /** "not-semantic" | "not-running" | "already-seeded" when nothing was authored. */
  skipped?: string;
}
/** HUP-S3.1 — one progress line from the first-run knowledge-corpus import (mirrors the Rust
 *  `ImportLine`; the importer's JSON-lines contract lives in citrate-memories `mem_corpus::progress`). */
export type KnowledgeImportLine =
  | { event: "verified"; bundle_digest: string; tenants: number; nodes: number; edges: number }
  | { event: "tenant_start"; tenant: string; nodes: number; edges: number }
  | { event: "progress"; tenant: string; done: number; total: number }
  | { event: "tenant_skipped"; tenant: string; reason: string }
  | { event: "tenant_done"; tenant: string }
  | {
      event: "done";
      bundle_digest: string;
      embed_model: string;
      nodes_added: number;
      nodes_merged: number;
      edges_added: number;
      tenants_imported: string[];
      tenants_skipped: string[];
    }
  | { event: "error"; stage: string; message: string };

/** HUP-S3.1 — what the first-run knowledge import did. Counts come only from the importer. */
export interface KnowledgeImportReport {
  state: "imported" | "skipped" | "failed";
  /** "no-bundle" | "not-semantic" | "already-imported" | "in-progress" when nothing ran. */
  skipped?: string | null;
  error?: string | null;
  bundleDigest?: string | null;
  embedModel?: string | null;
  nodesAdded: number;
  edgesAdded: number;
  tenantsImported: string[];
  tenantsSkipped: string[];
  /** HUP-S3.1: nodes embedded on this machine, and nodes that took the release's precomputed vectors. */
  nodesEmbedded?: number;
  vectorsReused?: number;
}

/** What a durable `memory.assert` wrote: the tenant and the node id the daemon printed. */
export interface MemoryAssertReceipt {
  tenant: string;
  nodeId: string | null;
}

export interface MemoryDomain {
  status(): Promise<MemoryStatus>;
  start(): Promise<void>;
  stop(): Promise<void>;
  /** Durable write of one approved fact into the personal tenant. Rejects with the
   *  reason (daemon not running, empty or too long) when nothing was saved. */
  assert(fact: string): Promise<MemoryAssertReceipt>;
  recall(tenant: string, budget?: number): Promise<MemoryResult>;
  /** `opts.passages` (HUP-S3.1): each hit also carries its text and citation. */
  search(tenant: string, query: string, budget?: number, opts?: { passages?: boolean }): Promise<MemoryResult>;
  neighbors(tenant: string, idPrefix: string, budget?: number): Promise<MemoryNeighbor[]>;
  /** Recall the personal + chain-state tenants for the Storage constellation. */
  constellation(budget?: number): Promise<MemoryResult[]>;
  /** W3.2 — idempotent first-run preload of the bundled Citrate docs into the
   *  citrate-docs tenant. Gated in Rust (semantic + running + empty tenant). */
  ingestDocs(): Promise<DocsIngestReport>;
  /** Seed the constellation tenants with real network/node/stake facts on daemon-connect. */
  seedContext(facts: SeedFacts): Promise<SeedReport>;
  /** HUP-S3.1 — import the bundled knowledge corpus into the local store (first run; idempotent,
   *  verified against its manifest in Rust). Must run while the daemon is stopped; Rust stops and
   *  restarts a running daemon itself. `onProgress` receives each importer line. */
  importKnowledge(onProgress?: (line: KnowledgeImportLine) => void): Promise<KnowledgeImportReport>;
}
