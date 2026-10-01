// =====================================================================
// HUP-S2.4 / A8 — the reviewed taint annotations for every chat-agent tool.
//
// Shape matches citrate-agent-runtime's `ToolAnnotations` (HUP-S2.7): `effect` says what running
// the tool can change (none | write | spend | sign) and `trust` says whether its OUTPUT can be
// taken as instruction-free context (trusted | untrusted). The sidecar uses them for its taint
// downgrade: once a session has read untrusted output, every effectful call needs the member's
// explicit decision. Core keeps exactly one annotation per tool (the Record type makes a missing
// one a compile error; src/agent/toolAnnotations.test.ts is the enumeration tripwire) and
// build_session_body in src-tauri refuses a tool without both values.
//
// Trust reasoning: output written by the member or produced by their own node is trusted; output
// that other people can author (on-chain registries, group names and rosters, the opt-in
// directory, a saved skill body that may have been written from untrusted content) is untrusted.
// =====================================================================
import { AGENT_TOOLS } from "./harness";

export type ToolEffect = "none" | "write" | "spend" | "sign";
export type ToolTrust = "trusted" | "untrusted";

export interface ToolAnnotation {
  effect: ToolEffect;
  trust: ToolTrust;
}

export type AgentToolName = (typeof AGENT_TOOLS)[number]["function"]["name"];

export const AGENT_TOOL_ANNOTATIONS: Readonly<Record<AgentToolName, ToolAnnotation>> = {
  memory_search: { effect: "none", trust: "trusted" }, // shipped Citrate docs + the member's own approved notes
  memory_recall: { effect: "none", trust: "trusted" },
  app_navigate: { effect: "none", trust: "trusted" }, // moves the UI only
  memory_assert: { effect: "write", trust: "trusted" },
  journal_append: { effect: "write", trust: "trusted" },
  journal_read: { effect: "none", trust: "trusted" }, // the member's own journal
  node_status: { effect: "none", trust: "trusted" },
  staking_status: { effect: "none", trust: "trusted" },
  groups_list: { effect: "none", trust: "untrusted" }, // group names are set by other members
  group_roster: { effect: "none", trust: "untrusted" },
  group_create: { effect: "write", trust: "trusted" },
  group_invite: { effect: "write", trust: "trusted" },
  directory_find: { effect: "none", trust: "untrusted" }, // third-party published bindings
  skills_list: { effect: "none", trust: "untrusted" }, // permissionless on-chain SkillRegistry
  skill_write: { effect: "write", trust: "trusted" },
  skill_run: { effect: "none", trust: "untrusted" }, // returns stored instructions into the loop
  models_list: { effect: "none", trust: "untrusted" }, // permissionless on-chain ModelRegistry
  contract_deploy: { effect: "sign", trust: "trusted" }, // opens a SignatureCeremony for a creation tx
  // HUP-S9.4: the plan text is composed by core; only typed counts and one of three fixed
  // settlement words come from the coordinator (fl_rounds.rs parse_status), so it is trusted.
  fl_round_plan: { effect: "none", trust: "trusted" },
  fl_round_start: { effect: "write", trust: "trusted" }, // records the member's approval of one plan
};

/** The annotation for a tool name, or null for a name core does not offer. */
export function annotationFor(name: string): ToolAnnotation | null {
  return Object.prototype.hasOwnProperty.call(AGENT_TOOL_ANNOTATIONS, name)
    ? AGENT_TOOL_ANNOTATIONS[name as AgentToolName]
    : null;
}

/**
 * The tool list for a sidecar session: each OpenAI wrapper plus its annotations (top level, which
 * build_session_body reads). The gateway and local tool loops keep sending plain AGENT_TOOLS.
 */
export function annotatedAgentTools() {
  return AGENT_TOOLS.map((t) => ({ ...t, annotations: AGENT_TOOL_ANNOTATIONS[t.function.name] }));
}
