// Bridge impl — escalation router (HUP-S1.5), SIM (web/dev). A browser preview has no OS keyring
// and no agent sidecar, so it lists no endpoints, reports a zero budget, refuses to add a key or
// run an escalation (Rule 1: nothing is pretended), and reports the registry route as disabled.
import type { EscalationDomain } from "../domains";
import type { SimHost } from "./index";

const NEEDS_APP = "escalation endpoints need the desktop app (keys are sealed in the OS keyring)";

export function simEscalation(_host: SimHost): EscalationDomain {
  return {
    async endpoints() {
      return [];
    },
    async addEndpoint() {
      throw new Error(NEEDS_APP);
    },
    async removeEndpoint() {
      throw new Error(NEEDS_APP);
    },
    async budget() {
      return {
        capMicros: 0,
        usedMicros: 0,
        remainingMicros: 0,
        confirmedMicros: 0,
        periodStartMs: 0,
        periodEndMs: 0,
        maxCapMicros: 0,
        unreadable: false,
        history: [],
      };
    },
    async setBudget() {
      throw new Error(NEEDS_APP);
    },
    async quote() {
      throw new Error(NEEDS_APP);
    },
    async confirmPrepare() {
      throw new Error(NEEDS_APP);
    },
    async run() {
      throw new Error(NEEDS_APP);
    },
    async registryStatus() {
      return {
        enabled: false,
        reason: "Registry escalation is not deployed yet. Escalations use your own endpoints.",
        missing: ["the desktop app"],
      };
    },
  };
}
