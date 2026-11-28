// citrate-core: bridge domain `flRounds`, moved verbatim from src/bridge/domains.ts by L0-WP05
// (planset v060). src/bridge/domains.ts re-exports it, so imports from "../bridge/domains" keep
// working. The owning lane is the row for this path in .agentile/v60-ownership.map.

// ── HUP-S9.4: Hermes plans, explains and starts federated rounds; LoRA eval gate ──
// Shapes mirror src-tauri/src/fl_rounds.rs (serde camelCase). Data source: the compute-pool
// training-coordinator `GET /v1/status` at the configured URL; there is no default coordinator.
export type FlCapability = "probe" | "federated" | "h01";
export interface FlRoundProposal {
  requires: FlCapability;
  loraRank: number;
  maxTrajectories: number;
  leaseHours: number;
  /** The round to join (0x + 64 hex, citrate-chain FL_ROUND_V1 round_id). With one, an approved
   *  start also writes this device's consent for that round (D-29). */
  roundId?: string;
}
export type FlSettlement = "shadow" | "live" | "unknown";
export type FlPoolPhase = "noWork" | "open" | "running" | "complete";
export interface FlCoordinatorStatus {
  pending: number;
  leased: number;
  done: number;
  quarantined: number;
  workers: number;
  settlement: FlSettlement;
  phase: FlPoolPhase;
}
export type FlCoordinatorView =
  | { state: "notConfigured" }
  | { state: "unreachable"; url: string; reason: string }
  | { state: "live"; url: string; status: FlCoordinatorStatus };
export interface FlRoundExplanation {
  data: string;
  compute: string;
  reward: string;
  privacy: string;
  status: string;
}
export interface FlRoundPlan {
  planHash: string;
  createdAtMs: number;
  coordinator: FlCoordinatorView;
  proposal: FlRoundProposal;
  baseModel: string;
  device: { tier: string | null; accelerator: boolean | null };
  explain: FlRoundExplanation;
  canStart: boolean;
  blockers: string[];
}
export interface FlStartReceipt {
  planHash: string;
  coordinatorUrl: string;
  authorizedAtMs: number;
  /** Always false in this build: the device training worker is not bundled (HUP-S9.1/S9.2). */
  trainingStarted: boolean;
  note: string;
  roundId: string | null;
  /** Where the round consent was written (the worker's CITRATE_FL_CONSENT_FILE). */
  consentFile: string | null;
}
export interface FlStartRecord {
  planHash: string;
  coordinatorUrl: string;
  requires: FlCapability;
  authorizedAtMs: number;
  roundId?: string;
  revokedAtMs?: number;
}
export interface FlCoordinatorConfig {
  url: string | null;
  source: "env" | "settings" | "invalid" | "none";
  settingsUrl: string | null;
  note: string | null;
}
export interface FlAdapterGateRequest {
  adapterPath: string;
  expectedSha256: string;
  baseToolsPath: string;
  candidateToolsPath: string;
  baseQaPath?: string;
  candidateQaPath?: string;
  /** The round result (receipt) of the round that produced this adapter. */
  roundResultPath?: string;
}
/** The round an adapter came from, as its round result file states it. Core checks that the file is
 *  consistent (Accepted, chain, bundle and replay digests agree); it does not read the ledger. */
export interface FlRoundBinding {
  roundId: string;
  recordDigest: string;
  adapterSha256: string;
  chainId: number;
  ledger: string;
  participants: number;
}
export interface FlMetricDelta {
  metric: string;
  base: number | null;
  candidate: number | null;
  improvement: number | null;
}
export interface FlAdapterGateRecord {
  adapterSha256: string;
  adapterPath: string;
  baseModel: string;
  decidedAtMs: number;
  round: FlRoundBinding | null;
  decision: {
    verdict: "ACCEPT" | "REJECT";
    reasons: string[];
    metrics: FlMetricDelta[];
    compositeBase: number;
    compositeCandidate: number;
  };
}
export interface FlOverview {
  config: FlCoordinatorConfig;
  starts: FlStartRecord[];
  gates: FlAdapterGateRecord[];
  activeAdapter: string | null;
  /** The adapter put back after a restart (on the base it was loaded on). */
  rememberedAdapter: { sha256: string; baseModel: string } | null;
  restoreError: string | null;
  /** The rounds this device consented to, as the device worker reads them, and that file. */
  consentedRounds: string[];
  consentFile: string | null;
  consentError: string | null;
  storeError: string | null;
}
export interface FlRoundsDomain {
  overview(): Promise<FlOverview>;
  /** Persist (or clear, with null) the coordinator base URL. Core validates it. */
  setCoordinator(url: string | null): Promise<FlCoordinatorConfig>;
  /** Read the coordinator and build the plan + plain-words explanation. Read-only. */
  plan(proposal?: FlRoundProposal): Promise<FlRoundPlan>;
  lookupPlan(planHash: string): Promise<FlRoundPlan>;
  /** Record the member's HIC-1 approval of exactly this plan. Call only after the approval card. */
  start(planHash: string): Promise<FlStartReceipt>;
  gateAdapter(request: FlAdapterGateRequest): Promise<FlAdapterGateRecord>;
  /** Load an ACCEPTED adapter into llama-server (`--lora`). Returns the served copy's path. */
  loadAdapter(sha256: string): Promise<string>;
  /** Serve the base model alone. False when no adapter was loaded or remembered (nothing changed). */
  unloadAdapter(): Promise<boolean>;
  /** Withdraw this device's consent for one round. Returns the rounds still consented. */
  revokeConsent(roundId: string): Promise<string[]>;
  /** HUP-S9.3: the "train on my verified conversations" switch (default off) and what is stored. */
  trajectoryStatus(): Promise<FlTrajectoryStatus>;
  /** Turn the switch on or off. Off deletes assembled training sets now (and, if asked, recordings). */
  setTrajectoryConsent(enabled: boolean, deleteRecorded: boolean): Promise<FlTrajectoryStatus>;
  /** Turn the switch off, and only off (core trajectories_consent_revoke: no switch argument).
   *  Deletes assembled training sets now and keeps recordings. Hermes's consent withdraw uses this. */
  revokeTrajectoryConsent(): Promise<FlTrajectoryStatus>;
  /** Assemble the newest verified conversations (at most `maxTrajectories`) into one training set.
   *  Core refuses when the switch is off. */
  buildTrainingSet(maxTrajectories: number): Promise<FlTrainingSet>;
  /** Board 01: the assembled training sets on this device, newest first (names, sizes, example
   *  counts, sha256; never their lines), for attaching to a Hermes chat. */
  datasetsList(): Promise<FlDatasetFile[]>;
}
/** Board 01 (core fl_trajectories.rs DatasetFile). */
export interface FlDatasetFile {
  name: string;
  builtMs: number;
  bytes: number;
  examples: number;
  sha256: string;
}
/** HUP-S9.3 (core fl_trajectories.rs TrajectoryStatus). */
export interface FlTrajectoryStatus {
  settings: { enabled: boolean; changedAtMs: number | null };
  recordedFiles: number;
  datasets: number;
  appliesOnRestart: boolean;
  loadError: string | null;
}
/** HUP-S9.3 (core fl_trajectories.rs DatasetSummary). */
export interface FlTrainingSet {
  path: string;
  sha256: string;
  examples: number;
  filesRead: number;
  skippedLines: number;
  overCap: number;
}
