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
import type { AgentApproval, AgentHarnessDomain, AgentHarnessStatus, AgentSkill, AgentSkillsDomain, LocalSkill, RegistrySkill } from "../domains";
import type { CeremonyView } from "../types";

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
  bridgePending(): Promise<CeremonyView | null> {
    // Returns the CeremonyView for the head chain effect (also enqueues the pending ceremony that
    // signing.broadcast will consume), or null for a code/shell head / nothing pending.
    return invoke<CeremonyView | null>("hermes_bridge_pending");
  },
  async resolve(approve: boolean) {
    await invoke("hermes_resolve", { approve });
  },
};

export const tauriAgentSkills: AgentSkillsDomain = {
  list(): Promise<LocalSkill[]> {
    return invoke<LocalSkill[]>("skills_local_list");
  },
  write(name, description, instructions): Promise<LocalSkill> {
    return invoke<LocalSkill>("skills_local_write", { name, description, instructions });
  },
  read(name): Promise<string> {
    return invoke<string>("skills_local_read", { name });
  },
  async remove(name) {
    await invoke("skills_local_delete", { name });
  },
};
