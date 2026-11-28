// L1 bridge namespace `bridge.acp` (planset v060, lanes/L0_INTEGRATION_SEAMS.md section 3.3):
// ACP agent sessions, the harness list and worktrees. L1 owns this file and its two impls, src/bridge/tauri/l1.ts and
// src/bridge/sim/l1.ts (.agentile/v60-ownership.map); no other lane edits or imports them.
//
// It starts with no members. Each member L1 adds names its data source (Rule 7) and lands with
// both impls: the tauri one invokes a registered L1 command, the sim one answers honestly or throws
// `Unavailable` (src/bridge/types.ts), never an invented value (Rule 1). Types only in this file.
/** One ACP agent's private home (`<app data>/agents/<id>/home`), as core reports it. Metadata
 *  only: core never reads an agent's login files. */
export interface AgentHomeStatus {
  id: string;
  homeExists: boolean;
  /** Whether the agent's own sign-in file is present; `null` when core does not offer the
   *  agent's own sign-in (it signs in through the Citrate proxy, or needs none). */
  ownLoginPresent: boolean | null;
  ownLoginOffered: boolean;
}

export interface AcpDomain {
  /** `agent_runtime_auth_status`. Data source: core's app data folder (metadata only). Only an
   *  agent `release/agents.lock.json` names is accepted. */
  authStatus(id: string): Promise<AgentHomeStatus>;
  /** `agent_runtime_sign_out`: revokes the agent's proxy routes and empties its home. Data source:
   *  core's app data folder and the in-process model proxy. */
  signOut(id: string): Promise<AgentHomeStatus>;
  /** `agent_runtime_remove`: the uninstall cleanup (routes revoked, the agent's folder deleted).
   *  Resolves whether anything was removed. */
  removeAgent(id: string): Promise<boolean>;
}
