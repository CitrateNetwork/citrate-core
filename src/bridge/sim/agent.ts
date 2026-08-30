// CX bridge impl — agentHarness (C-22), SIM. Owned by lane s6 after S0. Honest-empty (Rule 1).
import type { AgentHarnessDomain, AgentHarnessStatus } from "../domains";
import type { SimHost } from "./index";

export function simAgentHarness(_host: SimHost): AgentHarnessDomain {
  return {
    async start() {
      /* sim: no sidecar */
    },
    async status(): Promise<AgentHarnessStatus> {
      return { running: false, skills: 0, pendingApprovals: 0 };
    },
    async skills() {
      return [];
    },
    async runSkill() {
      return { ok: false };
    },
    async pendingApprovals() {
      return [];
    },
    async stop() {
      /* sim: no-op */
    },
    async bridgePending() {
      // sim: no sidecar, so no chain effect to bridge — honest null (Rule 1).
      return null;
    },
    async resolve() {
      /* sim: no sidecar effect to release */
    },
  };
}
