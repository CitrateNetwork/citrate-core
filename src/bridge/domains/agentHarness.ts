// citrate-core: bridge domain `agentHarness`, moved verbatim from src/bridge/domains.ts by L0-WP05
// (planset v060). src/bridge/domains.ts re-exports it, so imports from "../bridge/domains" keep
// working. The owning lane is the row for this path in .agentile/v60-ownership.map.

import type { CheckpointList, UndoOutcome } from "../../agent/fileChanges";
import type { LearnAcceptResult, LearnContent, LearnProposal, LearnStatus, LearnedMemory, WorkflowRunView, WorkflowSpec } from "../../agent/learn";
import type { StepDiff } from "../../popout/diffModel";
import type { CeremonyView } from "../types";

// ── C-22 agent/Hermes harness: skills, code, comms (lane s6) ──
export interface AgentSkill {
  name: string;
  description: string;
}
export interface AgentApproval {
  id: string;
  kind: "code" | "chain" | "shell";
  summary: string;
  /** A chain effect's real target + calldata (what would actually be signed). Absent for code/shell. */
  to?: string;
  data?: string;
}
export interface AgentHarnessStatus {
  running: boolean;
  skills: number;
  pendingApprovals: number;
}
/** A skill registered on-chain in the SkillRegistry (Hermes P2). Mirrors the Rust RegistrySkill. */
export interface RegistrySkill {
  /** The `0x`-hex skillHash — the stable id. */
  id: string;
  /** Human name (e.g. "hf-model-register"). */
  name: string;
  /** Semver. */
  version: string;
  /** IPFS CID of the WASM capsule manifest (empty for a not-yet-published skill). */
  manifestCid: string;
  /** Short description. */
  description: string;
  /** `0x`-hex owner address. */
  owner: string;
}

/** HUP-S4.3 — which MCP servers Hermes may use (core writes the sidecar's allowlist). Mirrors the
 *  Rust `McpSettings`. All default off (default pending owner sign-off). HUP-S4.1 adds `node`: this
 *  node's own MCP server, through the stdio bridge with a connect token minted for Hermes. */
export interface HermesMcpSettings {
  mem: boolean;
  scan: boolean;
  node: boolean;
}

/** HUP-S4.1: one MCP server as the running sidecar reports it (`GET /mcp/servers`). Never carries
 *  a URL, an environment value or the server's instructions text. */
export interface McpRuntimeServer {
  name: string;
  transport: string;
  /** "ready" | "failed" | "exited". */
  state: string;
  protocolVersion?: string;
  /** "modern" (2026-07-28) | "legacy". */
  era?: string;
  serverName?: string;
  tasks?: boolean;
  /** Tools offered to Hermes. */
  tools: number;
  skipped: string[];
  error?: string;
  reconnects?: number;
  relists?: number;
  nextRetryMs?: number;
}

/** HUP-S4.1: the running sidecar's MCP servers, or `{running: false}`. */
export interface McpRuntimeView {
  running: boolean;
  configured?: boolean;
  servers?: McpRuntimeServer[];
}

/** HUP-S4.1: one MCP request the sidecar holds for the member (`GET /sessions/:id/mcp/pending`):
 *  an effectful MCP call after the session read untrusted content (`tool_call`), or a server asking
 *  the member to open a page (`open_url`, URL-mode elicitation; core opens it only on Allow). */
export interface McpPendingView {
  id: string;
  kind: "tool_call" | "open_url";
  callId: string;
  server: string;
  remoteTool: string;
  tool: string;
  hic: string;
  /** What the decision must carry back: the exact arguments, or the URL. */
  subject: string;
  arguments?: string;
  hints?: { readOnly: boolean; destructive: boolean; idempotent: boolean; openWorld: boolean };
  /** Why the member is asked (tool_call), or the server's own message (open_url, untrusted). */
  reason: string;
  url?: string;
  urlHost?: string;
  warnings: string[];
  expiresInSecs: number;
}

/** HUP-S4.3 — one server row (Rust `McpServerView`). */
export interface HermesMcpServer {
  name: string;
  label: string;
  transport: string;
  enabled: boolean;
  available: boolean;
  detail: string;
}

/** HUP-S4.3 — the settings view (Rust `McpView`). */
export interface HermesMcpView {
  settings: HermesMcpSettings;
  servers: HermesMcpServer[];
  /** An allowlist file is in place for the next Hermes start. */
  configWritten: boolean;
  /** Hermes is running and reads the allowlist only at start. */
  restartRequired: boolean;
}

export interface AgentHarnessDomain {
  /** Sidecar a Hermes agent (keyless; every chain effect stays ceremony-gated). */
  start(): Promise<void>;
  status(): Promise<AgentHarnessStatus>;
  skills(): Promise<AgentSkill[]>;
  /** Hermes P2 — the skills registered on-chain in the SkillRegistry (40204). A PURE read
   *  (getAllSkillHashes + getSkill); this is what Hermes "ships with" instead of an empty set.
   *  Honest empty list on an unwired/failed read (never fabricated). */
  registrySkills(): Promise<RegistrySkill[]>;
  /** Run a skill/code task behind the mandatory HITL approval flow. */
  runSkill(name: string, argsJson: string): Promise<{ ok: boolean }>;
  pendingApprovals(): Promise<AgentApproval[]>;
  stop(): Promise<void>;
  /**
   * CX-S6.3 — bridge the sidecar's head CHAIN effect into a PENDING ceremony (signs nothing) and
   * return the decoded CeremonyView to show at the Signature Ceremony, or null when the head is a
   * code/shell effect or nothing is pending. The human then approves via signing.broadcast and the
   * caller must call `resolve(true, id)` to let the capsule proceed. PBA-L7b-003: `id` is the call the
   * member is reviewing; if the head is a different call this rejects with `STALE_APPROVAL`.
   */
  bridgePending(id: string): Promise<CeremonyView | null>;
  /** CX-S6.3 — resolve the sidecar's head effect after the human decided: true proceeds, false aborts.
   *  PBA-L7b-003: BOUND to the reviewed call `id` — if the sidecar's head is no longer that call the
   *  promise rejects with a message starting `STALE_APPROVAL` and NOTHING is resolved (re-review). */
  resolve(approve: boolean, id: string): Promise<void>;
  /** C1-V3 — release the reviewed chain effect `id` after the member approved it and the
   *  transaction was submitted with an unknown outcome. Core tells the sidecar "submitted, outcome
   *  unknown, tx <hash>, check before retrying", never "rejected". Bound to `id` like `resolve`
   *  (STALE_APPROVAL when the head moved). `txHash` null = the failure carried no hash. */
  resolveOutcomeUnknown(id: string, txHash: string | null): Promise<void>;
  /** HUP-S1.1c — the sidecar-owned agent loop (ADR loop-in-sidecar). Open a session on the LOCAL
   *  model (endpoint + key are Rust-owned; the webview supplies only the prompt + tool specs, every
   *  tool runs in core through its approval gates). Returns the session id. */
  sessionOpen(systemPrompt: string, toolsJson: string, persona?: SessionPersonaChoice | null): Promise<string>;
  /** D-11 (ADR P-5/P-6) — the same session on the member's preferred cloud model (core reads it
   *  from its own setting, `providers.preferredGet`). The sidecar gets a loopback route to the core
   *  model proxy, never a key; refused under Local model only, with no preferred model, and when the
   *  provider is not connected in Settings › Models. Returns the session id. */
  sessionOpenCloud(systemPrompt: string, toolsJson: string, persona?: SessionPersonaChoice | null): Promise<string>;
  sessionSend(id: string, text: string): Promise<void>;
  /** Long-poll the session's events after sequence `after` (waits up to `waitMs` for new ones). */
  sessionEvents(id: string, after: number, waitMs: number): Promise<SessionEventsPage>;
  /** Hand back a core-hosted tool's result after core's own gates ran it. */
  sessionToolResult(id: string, callId: string, status: "ok" | "denied" | "error", content: string): Promise<void>;
  sessionStop(id: string): Promise<void>;
  /** HUP-S10.3 — open a session for a scheduled daemon run: the chat session body marked
   *  `unattended`, so every effectful call needs the member's explicit decision. Local model only. */
  /** K5a: `daemonId` lets core re-check a Store daemon against Hermes's current tools first;
   *  `runId` is the claimed run, which core checks is in flight for that daemon. */
  sessionOpenUnattended(systemPrompt: string, toolsJson: string, daemonId?: string, runId?: string): Promise<string>;
  /** HUP-S10.3 — close a session (a finished daemon run). */
  sessionClose(id: string): Promise<void>;
  /** HUP-S1.4 — the interview tracks the sidecar serves (question sets, persona, skills, workflow,
   *  gates). Rejects when the sidecar isn't running. */
  tracks(): Promise<InterviewTrack[]>;
  /** HUP-S1.4 — answers → brief. Unanswered questions take their defaults (empty answers = "just use
   *  defaults"); no `track` lets the sidecar suggest one from the goal. A refusal (no track fits, an
   *  answer outside its choices) rejects with a message starting `BRIEF_REFUSED: `. Builds nothing. */
  briefCreate(track: string | null, goal: string, answers: Record<string, string>): Promise<BriefDraft>;
  /** HUP-S1.4 — validate a member-edited brief against its track (required gates and the workflow
   *  can't be edited away). A refusal rejects with `BRIEF_REFUSED: <reason>`. */
  briefCheck(brief: Brief): Promise<{ ok: boolean; markdown: string }>;
  /** HUP-S4.3 — the MCP servers Hermes may use (mem-mcp, CitrateScan). Rejects outside the app. */
  mcpSettings(): Promise<HermesMcpView>;
  /** #681: whether the MCP server the running Hermes knows as `node` is core's own node entry
   *  (the bundled `--mcp-stdio` server core put in the allowlist it started Hermes with). Only
   *  then do that server's results count as deploy evidence. */
  nodeMcpTrusted(): Promise<boolean>;
  /** HUP-S4.3 — save the member's choice; takes effect at the next Hermes start. */
  mcpSet(settings: HermesMcpSettings): Promise<HermesMcpView>;
  /** HUP-S2.9 — the agent session's checkpointed file changes (newest first). `enabled: false`
   *  with a note when the sidecar cannot undo. Rejects when the sidecar isn't running. */
  checkpoints(id: string): Promise<CheckpointList>;
  /** HUP-S2.9 — undo one agent file change. A refusal (a file changed since, a pruned step) resolves
   *  with `ok: false` and the reason; nothing was restored. */
  undoStep(id: string, seq: number): Promise<UndoOutcome>;
  /** HUP-S2.9 — undo every change of the session not undone yet (all or nothing). */
  undoSession(id: string): Promise<UndoOutcome>;
  /** HUP-S5.4 — what one agent file change did, path by path, for the Code and diff pop-out
   *  (read-only). A refusal (pruned, not found, undo not enabled) resolves with `ok: false`. */
  checkpointDiff(id: string, seq: number): Promise<StepDiff>;
  /** HUP-S2.6 — record the member's answer on an approval card or a wallet review in core's HIC
   *  outbox (then in the decision records the nightly anchor covers). Resolves with the record id,
   *  or null in a build that keeps no decision records (web/dev); rejects when it could not be
   *  written. */
  recordDecision(kind: "ceremony.approval" | "agent.tool_approval", decision: "approved" | "denied", subject: string, reason: string): Promise<number | null>;
  /** HUP-S3.4 — run a declarative, verifier-judged workflow in a session; returns the run id. */
  workflowRun(sessionId: string, workflow: WorkflowSpec): Promise<string>;
  /** HUP-S3.4 — a workflow run's state and (when verified) its evidence. */
  workflowStatus(sessionId: string, runId: string): Promise<WorkflowRunView>;
  /** HUP-S3.4 — whether learning is on in the sidecar, and whether publishing is. */
  learnStatus(): Promise<LearnStatus>;
  /** HUP-S3.4 — proposals waiting for the member (`all`: every kept one). */
  learnProposals(all?: boolean): Promise<LearnProposal[]>;
  /** HUP-S3.4 — propose a skill or memory from a VERIFIED run of that session. */
  learnPropose(sessionId: string, runId: string, content: LearnContent): Promise<LearnProposal>;
  /** HUP-S3.4 — accept (HIC-1, recorded first). `acknowledged` names the conflicts accepted anyway.
   *  A refusal rejects with a message starting `LEARN_REFUSED: `. */
  learnAccept(id: string, acknowledged: string[]): Promise<LearnAcceptResult>;
  /** HUP-S3.4 — reject (recorded, final). */
  learnReject(id: string, reason: string): Promise<void>;
  /** HUP-S3.4 — the learned-memory ledger, with each memory's place in the memory graph. */
  learnMemories(): Promise<LearnedMemory[]>;
  /** HUP-S3.4 — store learned memories that are still waiting for the memory store. */
  learnStorePending(): Promise<LearnedMemory[]>;
  /** HUP-S3.4 — resolve a contradiction: keep one learned memory, set aside another (HIC-1; the
   *  sidecar records the decision before anything changes). Returns the updated ledger. A refusal
   *  rejects with a message starting `LEARN_REFUSED: `. */
  learnResolve(keep: string, retract: string): Promise<LearnedMemory[]>;
  /** HUP-S3.4 — publish a saved skill to the SkillRegistry (HIC-1 ceremony). Rejects with
   *  `PUBLISH_DISABLED: ` while publishing is off. */
  learnPublish(id: string, version: string): Promise<void>;
  /** HUP-S3.3 + S3.7 — the shipped personas with their prompt fragments (names owner-approved
   *  2026-10-01; `name_pending_sign_off` marks any that are not). Rejects when the sidecar isn't running. */
  personas(): Promise<HermesPersona[]>;
  /** HUP-S3.3 — the sidecar validates a member-defined persona and renders its fragment. A refusal
   *  rejects with a message starting `PERSONA_REFUSED: `. */
  personaCheck(persona: CustomPersonaInput): Promise<HermesPersona>;
  /** HUP-S3.3 — every track's workflow family, with `needs_tools` and, when this sidecar cannot
   *  run one, `unavailable` (why). */
  workflows(): Promise<TrackWorkflow[]>;
  /** HUP-S3.3 (US-3.3 AC2) — run a track's catalog workflow in a session by id. Rejects with a
   *  message starting `WORKFLOW_REFUSED: ` (unknown workflow, or a tool the session lacks). Read
   *  the run with `workflowStatus`. */
  trackWorkflowRun(sessionId: string, workflowId: string): Promise<TrackWorkflowStart>;
  /** HUP-S2.2 — the `shell_run` commands the sidecar holds for the member's decision in a session. */
  shellPending(sessionId: string): Promise<ShellPendingView[]>;
  /** HUP-S2.2 — allow or decline one held command. The decision carries the exact argv and folder
   *  the member was shown; the sidecar refuses it (rejects with `SHELL_DECISION_REFUSED: `) when
   *  they differ from what is waiting or nothing with that id waits any more. */
  shellDecide(sessionId: string, id: string, allow: boolean, argv: string[], cwd: string): Promise<void>;
  /** HUP-S4.1: the MCP requests the sidecar holds for the member's decision in a session. */
  mcpPending(sessionId: string): Promise<McpPendingView[]>;
  /** HUP-S4.1: allow or decline one held MCP request, bound to the subject shown. Rejects with
   *  `MCP_DECISION_REFUSED: ` when it no longer matches what is waiting. On Allow of an `open_url`
   *  request, core opens the address the sidecar holds in the system browser. */
  mcpDecide(sessionId: string, id: string, allow: boolean, subject: string): Promise<void>;
  /** HUP-S4.1: the running sidecar's MCP servers (connected / failed / tools). */
  mcpRuntime(): Promise<McpRuntimeView>;
}

/** HUP-S2.2 — the OS sandbox a held command would run in, as the sidecar describes it. */
export interface ShellSandboxSummary {
  /** "seatbelt" | "bwrap" | "none". */
  backend: string;
  enforced: boolean;
  /** "denied" | "allowed". */
  network: string;
  writable: string[];
  readable_extra: string[];
  /** One line for people. */
  summary: string;
}

/** HUP-S2.2 — one `shell_run` command waiting for the member (`GET /sessions/:id/shell/pending`). */
export interface ShellPendingView {
  id: string;
  callId: string;
  tool: string;
  hic: string;
  /** Exactly as proposed: the program name first, one entry per argument. */
  argv: string[];
  resolvedProgram: string;
  /** The canonical folder it runs in. */
  cwd: string;
  timeoutSecs: number;
  sandbox: ShellSandboxSummary;
  expiresInSecs: number;
}

/** HUP-S3.3 — the persona a sidecar session applies (skill allowlist + tool emphasis): a shipped
 *  persona by id, or a member-defined one. */
export type SessionPersonaChoice = { persona: string } | { customPersona: CustomPersonaInput };

/** HUP-S3.3 — what `POST /sessions/:id/track_workflows` answers. */
export interface TrackWorkflowStart {
  run_id: string;
  workflow_id: string;
  track: string;
  evidence: string;
}

// HUP-S3.3 + S3.7 — persona and track-workflow wire shapes. These mirror the sidecar's
// `agent-loop::personas` / `agent-loop::workflows` views verbatim (snake_case).
export interface HermesPersona {
  id: string;
  role: string;
  name: string;
  name_status: string;
  summary: string;
  voice: string;
  tone: string;
  style_rules: string[];
  default_track: string;
  default_workflow: string;
  tool_emphasis: string[];
  skills: string[];
  /** Optional voice id for the existing speech engine; null = the system voice. */
  tts_voice?: string | null;
  /** What the chat appends to its system prompt while this persona is active. */
  prompt_fragment: string;
  /** True while the shipped name is a placeholder (the UI says so). */
  name_pending_sign_off: boolean;
  custom: boolean;
  /** Allowlisted skills this sidecar has installed (absent for a custom persona's check). */
  skills_installed?: string[];
}
export interface CustomPersonaInput {
  id: string;
  name: string;
  summary: string;
  voice: string;
  tone: string;
  style_rules: string[];
  default_track: string;
  tool_emphasis: string[];
  skills: string[];
  tts_voice: string | null;
}
export interface TrackWorkflowStep {
  id: string;
  instruction: string;
  max_attempts: number;
  verifier_names: string[];
}
export interface TrackWorkflow {
  id: string;
  track: string;
  title: string;
  summary: string;
  is_default: boolean;
  /** "tool-report" (a tool's own report decides) or "answer-shape" (the answer's structure). */
  evidence: "tool-report" | "answer-shape" | string;
  tools: string[];
  /** Tools a passing run must call. */
  needs_tools?: string[];
  /** Why this sidecar cannot run the workflow (e.g. the toolchain is off); null = it can. */
  unavailable?: string | null;
  verifier_names: string[];
  steps: TrackWorkflowStep[];
}

// HUP-S1.4 — interviewer wire shapes. These mirror the sidecar's `agent-loop::interview` types
// verbatim (snake_case), so a brief round-trips webview → core → sidecar unchanged.
export interface InterviewQuestion {
  id: string;
  ask: string;
  /** Empty = free text; otherwise the answer must be one of these. */
  choices: string[];
  default: string;
}
export interface InterviewTrack {
  id: string;
  title: string;
  summary: string;
  persona: string;
  skills: string[];
  workflow: string;
  /** False until the workflow ships; the UI says so (Rule 1). */
  workflow_available: boolean;
  ships_in?: string | null;
  gates: string[];
  questions: InterviewQuestion[];
}
export interface BriefConstraint {
  id: string;
  ask: string;
  answer: string;
  from_default: boolean;
}
export interface Brief {
  track: string;
  goal: string;
  constraints: BriefConstraint[];
  persona: string;
  skills: string[];
  workflow: string;
  workflow_available: boolean;
  ships_in?: string | null;
  gates: string[];
}
export interface BriefDraft {
  brief: Brief;
  markdown: string;
}

/** HUP-S1.1c — one page of a sidecar session's event log. */
export interface SessionEventsPage {
  events: { seq: number; event: Record<string, unknown> }[];
  lastSeq: number;
  busy: boolean;
  /** HUP-S1.1: core-hosted calls the session is still waiting on (newer sidecars only). */
  pendingCoreCalls?: string[];
}
