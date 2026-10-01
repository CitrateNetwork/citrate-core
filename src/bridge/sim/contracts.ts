// CX bridge impl — contracts (Hermes P3 / WP3.2), SIM (web/dev).
//
// Honest: web/dev has no key + no chain + no ceremony to submit into, so a deploy cannot
// happen. Throw a plain reason rather than pretend it queued (Rule 1).
import type { ContractsDomain } from "../domains";
import type { SimHost } from "./index";

export function simContracts(_host: SimHost): ContractsDomain {
  return {
    async deploy() {
      throw new Error("deploying a contract needs the desktop node (no chain in web/dev)");
    },
    async gateLookup() {
      throw new Error("the deploy gate runs in the desktop node (no gate records in web/dev)");
    },
    async gateSubmit() {
      throw new Error("the deploy gate runs in the desktop node (no gate records in web/dev)");
    },
    async verifiedSource() {
      throw new Error("the verified-source lookup runs in the desktop node (not in web/dev)");
    },
  };
}
