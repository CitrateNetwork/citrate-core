// Bridge impl: federated rounds (HUP-S9.4), SIM (web/dev). A browser preview has no coordinator
// read, no plan and no llama-server, so every call says so instead of inventing a round (Rule 1).
import type { FlRoundsDomain } from "../domains";
import type { SimHost } from "./index";

const NEEDS_DESKTOP = "federated rounds need the desktop app";

export function simFlRounds(_host: SimHost): FlRoundsDomain {
  const refuse = async (): Promise<never> => {
    throw new Error(NEEDS_DESKTOP);
  };
  return {
    overview: refuse,
    setCoordinator: refuse,
    plan: refuse,
    lookupPlan: refuse,
    start: refuse,
    gateAdapter: refuse,
    loadAdapter: refuse,
    unloadAdapter: refuse,
    importRound: refuse,
    fetchRound: refuse,
    fetchAdapter: refuse,
    evalBegin: refuse,
    evalComplete: refuse,
    evalFinish: refuse,
    evalEnd: refuse,
  };
}
