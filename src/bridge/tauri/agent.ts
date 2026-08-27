// CX bridge impl — agentHarness (C-22), TAURI. Owned by lane s6 (CX-S6) after S0.
// Distinct from the legacy `agent` domain (node-agent GPU market). This is the Hermes
// skills/code/comms harness. S0.2 stub: honest Unavailable. CX-S6 wires the keyless sidecar.
import type { AgentHarnessDomain } from "../domains";
import { Unavailable } from "../types";

export const tauriAgentHarness: AgentHarnessDomain = {
  async start() {
    throw new Unavailable("agentHarness", "start");
  },
  async status() {
    throw new Unavailable("agentHarness", "status");
  },
  async skills() {
    throw new Unavailable("agentHarness", "skills");
  },
  async runSkill() {
    throw new Unavailable("agentHarness", "runSkill");
  },
  async pendingApprovals() {
    throw new Unavailable("agentHarness", "pendingApprovals");
  },
  async stop() {
    throw new Unavailable("agentHarness", "stop");
  },
};
