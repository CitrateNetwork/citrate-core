// Bridge impl — the Cognition (Devin) delegate (D-11), SIM (web/dev). A browser preview has no
// model proxy and no keychain, so every operation says so (Rule 1) instead of inventing a session.
import type { DelegationDomain } from "../domains";
import type { SimHost } from "./index";
import { Unavailable } from "../types";

const WHY = "delegating to Devin runs in the desktop app (the key stays in its OS keychain)";

export function simDelegation(_host: SimHost): DelegationDomain {
  const no = (op: string) => async (): Promise<never> => {
    throw new Unavailable("delegation", op, WHY);
  };
  return {
    prepare: no("prepare"),
    decline: no("decline"),
    approve: no("approve"),
    status: no("status"),
  };
}
