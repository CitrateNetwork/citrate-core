// =====================================================================
// HUP-S3.4 — verified self-learning, TypeScript side (US-3.4).
//
// The sidecar (agent-learn) owns proposals and the HIC decision log; core (hermes_learn.rs) owns
// the member id, the learned-memory ledger and the ceremony. These are the wire types (snake_case
// from the sidecar, camelCase from core's ledger) and the pure models the proposal card renders.
//
// The card model fails closed: Accept is enabled only while the proposal awaits a decision, has
// verifier evidence that all passed, no blocking conflict, and every other conflict acknowledged.
// Publishing is shown only for a saved skill, and only enabled when core says so.
// =====================================================================

export interface LearnVerdict {
  step: string;
  name: string;
  passed: boolean;
  detail: string;
}

export interface LearnEvidence {
  workflow_id: string;
  steps: string[];
  verdicts: LearnVerdict[];
  attempts: number;
  trajectory: { session_id: string; workflow_id: string; messages: number; sha256: string };
}

export type LearnContent = { kind: "skill"; skill_md: string } | { kind: "memory"; key: string; value: string };

export interface LearnConflict {
  kind: "same_name_skill" | "shadows_skill" | "pending_proposal" | "contradiction";
  existing_id: string;
  detail: string;
  blocking: boolean;
}

export type LearnState =
  | { state: "proposed" }
  | { state: "rejected"; by: string; reason: string }
  | { state: "persist_failed"; reason: string }
  | { state: "persisted" }
  | { state: "publish_prepared" }
  /** A memory the member set aside when resolving a contradiction; `kept` is the one kept. */
  | { state: "retracted"; by: string; kept: string };

export interface LearnProposal {
  id: string;
  kind: "skill" | "memory";
  content: LearnContent;
  content_sha256: string;
  evidence: LearnEvidence;
  provenance: { session_id: string; agent: string; model: string };
  created_at_ms: number;
  conflicts: LearnConflict[];
  state: LearnState;
}

export interface PublishAvailability {
  enabled: boolean;
  note: string;
}

export interface LearnStatus {
  sidecar: { enabled: boolean; pending?: number; store_error?: string | null; error?: string };
  publish: PublishAvailability;
}

/** A learned memory in core's ledger (camelCase). */
export interface LearnedMemory {
  proposalId: string;
  key: string;
  value: string;
  /** "true"; "both": contradicted and unresolved (both memories are kept, and recall leaves it
   *  out until it is resolved); "false": set aside by the member when resolving a contradiction
   *  (kept for the record). */
  belnap: "true" | "both" | "false";
  /** Learned memories (proposal ids) and memories the app already held (`memory:<id>`) this one
   *  contradicts and is unresolved against. */
  contradicts: string[];
  contentSha256: string;
  workflowId: string;
  acceptedBy: string;
  acceptedAtMs: number;
  decisionSeq: number;
  graph: { state: "stored" | "pending" | "failed" | "retracted"; nodeId?: string; detail?: string };
  /** For a retracted memory: the proposal id kept instead, or `memory:<id>` for a memory the
   *  app already held. */
  retractedFor?: string;
  resolvedSeq?: number;
  /** Graph nodes still to be marked superseded by this memory's node. */
  supersedeNodes?: string[];
}

export interface LearnAcceptResult {
  persisted: { kind: "skill"; name: string; content_sha256: string } | ({ kind: "memory" } & Record<string, unknown>);
  memory?: LearnedMemory;
  /** For a skill: whether the sidecar offers it to the next session already (no restart). */
  skillsReloaded?: boolean;
}

/** A declarative workflow for `hermes_workflow_run` (the sidecar's closed verifier set). */
export type WorkflowVerifier =
  | { kind: "tool_succeeded"; tool: string }
  | { kind: "tool_not_called"; tool: string }
  | { kind: "answer_contains"; text: string }
  | { kind: "json_field_equals"; tool: string; pointer: string; value: unknown }
  | { kind: "forge_tests_pass"; tool?: string }
  | { kind: "sarif_below"; tool: string; threshold: string }
  | { kind: "medusa_no_failures"; tool?: string };

export interface WorkflowSpec {
  id: string;
  steps: { id: string; instruction: string; max_attempts?: number; verifiers: WorkflowVerifier[] }[];
}

export interface WorkflowRunView {
  run_id: string;
  workflow_id: string;
  state: "running" | "verified" | "unverified";
  evidence?: LearnEvidence;
  answers?: string[];
  reason?: string;
}

// ---------------------------------------------------------------------------
// The proposal card model
// ---------------------------------------------------------------------------

const CONFLICT_LABEL: Record<LearnConflict["kind"], string> = {
  same_name_skill: "Same name in your skills",
  shadows_skill: "Shadows another skill",
  pending_proposal: "Another proposal is about this",
  contradiction: "Contradicts a memory",
};

export interface CardConflict {
  id: string;
  label: string;
  detail: string;
  blocking: boolean;
  acknowledged: boolean;
}

export interface ProposalCardModel {
  id: string;
  kindLabel: "Skill" | "Memory";
  title: string;
  /** Skill description, or the memory value. */
  subtitle: string;
  /** The SKILL.md body (skills only), for the expandable preview. */
  body: string | null;
  stateLabel: string;
  awaiting: boolean;
  evidence: {
    workflow: string;
    verdicts: { label: string; passed: boolean }[];
    attempts: string;
    trajectory: string;
    model: string;
  };
  allPassed: boolean;
  conflicts: CardConflict[];
  canAccept: boolean;
  /** Why Accept is disabled (null when it is enabled or nothing awaits). */
  acceptBlocked: string | null;
  /** Shown for a saved skill only. */
  publish: PublishAvailability | null;
}

function skillMeta(md: string): { name: string; description: string; body: string } {
  const m = /^---\r?\n([\s\S]*?)\r?\n---\r?\n?([\s\S]*)$/.exec(md);
  if (!m) return { name: "", description: "", body: md };
  let name = "";
  let description = "";
  for (const line of m[1].split(/\r?\n/)) {
    const kv = /^(name|description):\s*(.*)$/.exec(line);
    if (kv?.[1] === "name") name = kv[2].trim();
    if (kv?.[1] === "description") description = kv[2].trim();
  }
  return { name, description, body: m[2].trim() };
}

function stateLabel(s: LearnState): string {
  switch (s.state) {
    case "proposed":
      return "Waiting for you";
    case "persist_failed":
      return "Not saved yet: " + s.reason;
    case "rejected":
      return "Rejected";
    case "persisted":
      return "Saved";
    case "publish_prepared":
      return "Saved, publish prepared";
    case "retracted":
      return "Set aside";
  }
}

/** The card for one proposal. `acked` holds the conflict ids the member ticked. */
export function proposalCardModel(p: LearnProposal, acked: ReadonlySet<string>, publish: PublishAvailability | null): ProposalCardModel {
  const awaiting = p.state.state === "proposed" || p.state.state === "persist_failed";
  const verdicts = (p.evidence?.verdicts ?? []).map((v) => ({ label: `${v.step}: ${v.name}`, passed: v.passed === true }));
  const allPassed = verdicts.length > 0 && verdicts.every((v) => v.passed);
  const conflicts: CardConflict[] = (p.conflicts ?? []).map((c) => ({
    id: c.existing_id,
    label: CONFLICT_LABEL[c.kind] ?? c.kind,
    detail: c.detail,
    blocking: c.blocking,
    acknowledged: !c.blocking && acked.has(c.existing_id),
  }));
  let acceptBlocked: string | null = null;
  if (!awaiting) acceptBlocked = null;
  else if (!allPassed) acceptBlocked = "The verifier evidence does not show every check passing.";
  else if (conflicts.some((c) => c.blocking)) acceptBlocked = "A skill with this name is already in your skills. It is never overwritten.";
  else if (conflicts.some((c) => !c.acknowledged)) acceptBlocked = "Acknowledge each conflict above to accept anyway.";
  const canAccept = awaiting && acceptBlocked === null;

  let title: string;
  let subtitle: string;
  let body: string | null = null;
  if (p.content.kind === "skill") {
    const meta = skillMeta(p.content.skill_md);
    title = meta.name || "Unnamed skill";
    subtitle = meta.description;
    body = meta.body;
  } else {
    title = p.content.key;
    subtitle = p.content.value;
  }
  const t = p.evidence?.trajectory;
  const savedSkill = p.kind === "skill" && (p.state.state === "persisted" || p.state.state === "publish_prepared");
  return {
    id: p.id,
    kindLabel: p.kind === "skill" ? "Skill" : "Memory",
    title,
    subtitle,
    body,
    stateLabel: stateLabel(p.state),
    awaiting,
    evidence: {
      workflow: p.evidence?.workflow_id ?? "",
      verdicts,
      attempts: `${p.evidence?.attempts ?? 0} judged attempt${(p.evidence?.attempts ?? 0) === 1 ? "" : "s"}`,
      trajectory: t ? `session ${t.session_id} · ${t.messages} messages · sha256 ${t.sha256.slice(0, 12)}` : "",
      model: p.provenance?.model ?? "",
    },
    allPassed,
    conflicts,
    canAccept,
    acceptBlocked,
    publish: savedSkill ? (publish ?? { enabled: false, note: "Publishing is not available." }) : null,
  };
}

/** The acknowledged ids to send with an accept: only non-blocking conflicts the member ticked. */
export function acknowledgedFor(p: LearnProposal, acked: ReadonlySet<string>): string[] {
  return (p.conflicts ?? []).filter((c) => !c.blocking && acked.has(c.existing_id)).map((c) => c.existing_id);
}

/** One row of the learned-memories list. `all` (the whole ledger) names what a retracted memory
 *  was set aside for. */
export function memoryRowModel(
  m: LearnedMemory,
  all: readonly LearnedMemory[] = [],
): { title: string; value: string; belnapLabel: string | null; graphLabel: string; tone: "ok" | "warn" | "danger" | "muted" } {
  const graphLabel =
    m.graph.state === "stored"
      ? "In your memory graph"
      : m.graph.state === "pending"
        ? "Waiting for the memory store to start"
        : m.graph.state === "retracted"
          ? "Not stored: set aside before it reached your memory graph"
          : "Not stored: " + (m.graph.detail ?? "unknown error");
  if (m.belnap === "false") {
    const kept = all.find((x) => x.proposalId === m.retractedFor);
    return {
      title: m.key,
      value: m.value,
      belnapLabel: kept
        ? `Set aside: you kept "${kept.value}" instead. Kept for the record.`
        : isKnownRef(m.retractedFor)
          ? "Set aside: you kept the memory you already had instead. Kept for the record."
          : "Set aside when you resolved a contradiction. Kept for the record.",
      graphLabel,
      tone: "muted",
    };
  }
  const tone = m.graph.state === "failed" ? "danger" : m.belnap === "both" || m.graph.state === "pending" ? "warn" : "ok";
  return {
    title: m.key,
    value: m.value,
    belnapLabel:
      m.belnap === "both"
        ? "Contradiction, unresolved: both are kept and linked as contradicting; nothing was merged or overwritten. Hermes does not recall it until you resolve it."
        : null,
    graphLabel,
    tone,
  };
}

/** The prefix core gives a memory the app held before Hermes learned anything (not learned here). */
export const KNOWN_MEMORY_PREFIX = "memory:";

/** `memory:<id>`: a memory the app already held, which a learned memory can contradict. */
export function isKnownRef(id: string | undefined): id is string {
  if (!id || !id.startsWith(KNOWN_MEMORY_PREFIX)) return false;
  const rest = id.slice(KNOWN_MEMORY_PREFIX.length);
  return rest.length > 0 && rest.length <= 128 && /^[\x21-\x7e]+$/.test(rest);
}

/** The member's way out of `both`: keep this memory and set aside every learned memory, and every
 *  memory the app already held, it still contradicts. `null` when there is nothing this ledger can
 *  resolve. */
export function resolveChoice(all: readonly LearnedMemory[], m: LearnedMemory): { keep: string; retract: string[]; confirm: string } | null {
  if (m.belnap !== "both") return null;
  const others = m.contradicts
    .map((id) => all.find((x) => x.proposalId === id))
    .filter((x): x is LearnedMemory => x !== undefined && x.belnap !== "false" && x.proposalId !== m.proposalId);
  const known = m.contradicts.filter(isKnownRef);
  if (others.length === 0 && known.length === 0) return null;
  const named = [...others.map((o) => `"${o.value}"`), ...(known.length === 1 ? ["the memory you already had"] : known.length > 1 ? [`${known.length} memories you already had`] : [])].join(", ");
  return {
    keep: m.proposalId,
    retract: [...others.map((o) => o.proposalId), ...known],
    confirm: `Keep "${m.value}" for ${m.key} and set aside ${named}? What you set aside is kept for the record, not deleted. Each choice is recorded as your decision.`,
  };
}

/** For a learned memory that contradicts a memory the app already held: set the learned one aside
 *  and keep the one the app had. `null` when there is no such contradiction. */
export function setAsideChoice(m: LearnedMemory): { keep: string; retract: string[]; confirm: string } | null {
  if (m.belnap !== "both") return null;
  const known = m.contradicts.find(isKnownRef);
  if (!known) return null;
  return {
    keep: known,
    retract: [m.proposalId],
    confirm: `Set aside "${m.value}" for ${m.key} and keep the memory you already had? What you set aside is kept for the record, not deleted. Your choice is recorded as your decision.`,
  };
}
