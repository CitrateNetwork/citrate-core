// CX bridge impl — agentHarness (C-22), TAURI. Owned by lane s6 (CX-S6).
//
// Distinct from the legacy `agent` domain (node-agent GPU market). This is the keyless Hermes
// skills/code/comms harness. The Rust `hermes_*` commands (src-tauri/src/hermes.rs) were built to
// this exact domain shape: `RemoteStatus` carries `#[serde(rename_all = "camelCase")]` so it
// deserializes straight into AgentHarnessStatus ({running, skills, pendingApprovals}); `SkillMeta`
// is AgentSkill; `PendingApproval` is AgentApproval (+ optional to/data the ceremony bridge uses,
// harmlessly ignored here). Every chain effect a skill proposes stays ceremony-gated (Rule 3) —
// this bridge starts/stops the sidecar and reads its state; it never signs.
import { invoke } from "./invoke";
import type { AgentApproval, AgentHarnessDomain, AgentHarnessStatus, AgentSkill, AgentSkillsDomain, LocalSkill, RegistrySkill, SessionEventsPage, InterviewTrack, BriefDraft, HermesMcpView, HermesPersona, TrackWorkflow, TrackWorkflowStart } from "../domains";
import type { CeremonyView } from "../types";
import type { CheckpointList, UndoOutcome } from "../../agent/fileChanges";
import type { LearnAcceptResult, LearnedMemory, LearnProposal, LearnStatus, WorkflowRunView } from "../../agent/learn";

// The sidecar's run_skill takes a serde_json::Value. The domain hands us a string: JSON if it
// parses (an object/array/number), otherwise the raw text as a JSON string value; empty → {}.
function toArgs(argsJson: string): unknown {
  const t = argsJson.trim();
  if (!t) return {};
  try {
    return JSON.parse(t);
  } catch {
    return t;
  }
}

export const tauriAgentHarness: AgentHarnessDomain = {
  async start() {
    // Returns the sidecar's local lifecycle HermesStatus; the domain is void — callers read
    // running-state via status().
    await invoke("hermes_start");
  },
  status(): Promise<AgentHarnessStatus> {
    return invoke<AgentHarnessStatus>("hermes_status");
  },
  skills(): Promise<AgentSkill[]> {
    return invoke<AgentSkill[]>("hermes_skills");
  },
  registrySkills(): Promise<RegistrySkill[]> {
    return invoke<RegistrySkill[]>("skills_registry_list");
  },
  runSkill(name, argsJson): Promise<{ ok: boolean }> {
    return invoke<{ ok: boolean }>("hermes_run_skill", { name, args: toArgs(argsJson) });
  },
  pendingApprovals(): Promise<AgentApproval[]> {
    return invoke<AgentApproval[]>("hermes_pending_approvals");
  },
  async stop() {
    await invoke("hermes_stop");
  },
  bridgePending(id: string): Promise<CeremonyView | null> {
    // Returns the CeremonyView for the head chain effect (also enqueues the pending ceremony that
    // signing.broadcast will consume), or null for a code/shell head / nothing pending.
    return invoke<CeremonyView | null>("hermes_bridge_pending", { id });
  },
  async resolve(approve: boolean, id: string) {
    // PBA-L7b-003: the approval is bound to the reviewed call id (never the legacy head-resolve).
    await invoke("hermes_resolve", { approve, id });
  },
  sessionOpen(systemPrompt, toolsJson, persona) {
    // HUP-S3.3: the persona travels as `persona` (shipped id) or `customPersona`; none = unchanged.
    const extra = persona ? ("persona" in persona ? { persona: persona.persona } : { customPersona: persona.customPersona }) : {};
    return invoke<string>("hermes_session_open", { systemPrompt, toolsJson, ...extra });
  },
  async sessionSend(id, text) {
    await invoke("hermes_session_send", { id, text });
  },
  sessionEvents(id, after, waitMs) {
    return invoke<SessionEventsPage>("hermes_session_events", { id, after, waitMs });
  },
  async sessionToolResult(id, callId, status, content) {
    await invoke("hermes_session_tool_result", { id, callId, status, content });
  },
  async sessionStop(id) {
    await invoke("hermes_session_stop", { id });
  },
  sessionOpenUnattended(systemPrompt, toolsJson) {
    return invoke<string>("hermes_session_open_unattended", { systemPrompt, toolsJson });
  },
  async sessionClose(id) {
    await invoke("hermes_session_close", { id });
  },
  tracks() {
    return invoke<InterviewTrack[]>("hermes_tracks");
  },
  briefCreate(track, goal, answers) {
    return invoke<BriefDraft>("hermes_brief_create", { track, goal, answers });
  },
  briefCheck(brief) {
    return invoke<{ ok: boolean; markdown: string }>("hermes_brief_check", { brief });
  },
  // HUP-S4.3 — MCP servers Hermes may use; core writes the sidecar's allowlist file.
  mcpSettings() {
    return invoke<HermesMcpView>("hermes_mcp_settings");
  },
  mcpSet(settings) {
    return invoke<HermesMcpView>("hermes_mcp_set", { settings });
  },
  // HUP-S2.9 — undo for agent file changes (the sidecar's checkpoint routes, through Rust).
  checkpoints(id) {
    return invoke<CheckpointList>("hermes_checkpoints", { id });
  },
  undoStep(id, seq) {
    return invoke<UndoOutcome>("hermes_undo_step", { id, seq });
  },
  undoSession(id) {
    return invoke<UndoOutcome>("hermes_undo_session", { id });
  },
  // HUP-S3.4 — verified workflow runs + verified self-learning (src-tauri/src/hermes_learn.rs).
  workflowRun(sessionId, workflow) {
    return invoke<string>("hermes_workflow_run", { sessionId, workflowJson: JSON.stringify(workflow) });
  },
  workflowStatus(sessionId, runId) {
    return invoke<WorkflowRunView>("hermes_workflow_status", { sessionId, runId });
  },
  learnStatus() {
    return invoke<LearnStatus>("hermes_learn_status");
  },
  async learnProposals(all = false) {
    const v = await invoke<{ proposals?: LearnProposal[] }>("hermes_learn_proposals", { all });
    return Array.isArray(v?.proposals) ? v.proposals : [];
  },
  learnPropose(sessionId, runId, content) {
    return invoke<LearnProposal>("hermes_learn_propose", { sessionId, runId, contentJson: JSON.stringify(content) });
  },
  learnAccept(id, acknowledged) {
    return invoke<LearnAcceptResult>("hermes_learn_accept", { id, acknowledged });
  },
  async learnReject(id, reason) {
    await invoke("hermes_learn_reject", { id, reason });
  },
  learnMemories() {
    return invoke<LearnedMemory[]>("hermes_learn_memories");
  },
  learnStorePending() {
    return invoke<LearnedMemory[]>("hermes_learn_store_pending");
  },
  learnResolve(keep, retract) {
    return invoke<LearnedMemory[]>("hermes_learn_resolve", { keep, retract });
  },
  async learnPublish(id, version) {
    await invoke("hermes_learn_publish", { id, version });
  },
  personas() {
    return invoke<HermesPersona[]>("hermes_personas");
  },
  personaCheck(persona) {
    return invoke<HermesPersona>("hermes_persona_check", { persona });
  },
  workflows() {
    return invoke<TrackWorkflow[]>("hermes_workflows");
  },
  trackWorkflowRun(sessionId, workflowId) {
    return invoke<TrackWorkflowStart>("hermes_track_workflow_run", { sessionId, workflowId });
  },
};

export const tauriAgentSkills: AgentSkillsDomain = {
  list(): Promise<LocalSkill[]> {
    return invoke<LocalSkill[]>("skills_local_list");
  },
  write(name, description, instructions, overwrite = false): Promise<LocalSkill> {
    return invoke<LocalSkill>("skills_local_write", { name, description, instructions, overwrite });
  },
  read(name): Promise<string> {
    return invoke<string>("skills_local_read", { name });
  },
  async remove(name) {
    await invoke("skills_local_delete", { name });
  },
};
