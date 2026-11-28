// citrate-core: bridge domain `delegation`, moved verbatim from src/bridge/domains.ts by L0-WP05
// (planset v060). src/bridge/domains.ts re-exports it, so imports from "../bridge/domains" keep
// working. The owning lane is the row for this path in .agentile/v60-ownership.map.

// ---- D-11 (RC52, ADR P-8) — the Cognition (Devin) delegate. Mirrors Rust `cognition.rs`. ----
// Hermes proposes; core assembles the exact payload from the scope the member approves on a card.

export interface DelegationScope {
  task: string;
  /** `owner/name` on the member's Devin-connected git host. */
  repo: string;
  files: string[];
  context: string;
  /** The Devin spend ceiling for this task (ACUs); absent = core's default (5). */
  maxAcuLimit?: number;
}
export interface DelegationView {
  delegationId: string;
  repo: string;
  files: string[];
  task: string;
  context: string;
  /** The exact text core sends. */
  prompt: string;
  scopeHash: string;
  expiresAtMs: number;
  destination: string;
  /** The Devin spend ceiling sent with the session (ACUs), inside the approval hash. */
  maxAcuLimit: number;
}
export interface DelegationStarted {
  delegationId: string;
  repo: string;
  sessionUrl: string | null;
}
export interface DelegationPullRequest {
  url: string;
  state: string | null;
}
/** Every text field is third-party output: fence it before the model sees it. */
export interface DelegationSession {
  delegationId: string;
  repo: string;
  sessionUrl: string | null;
  status: string;
  statusDetail: string | null;
  pullRequests: DelegationPullRequest[];
  messages: string[];
  structuredOutput: string | null;
  acusConsumed: number | null;
  untrusted: true;
}
export interface DelegationDomain {
  /** Check a proposal and get the exact prompt to show. Nothing is sent. */
  prepare(scope: DelegationScope): Promise<DelegationView>;
  decline(delegationId: string): Promise<void>;
  /** The member approved the card: send exactly what was prepared (the hash binds it). */
  approve(delegationId: string, scopeHash: string): Promise<DelegationStarted>;
  status(delegationId: string): Promise<DelegationSession>;
}
