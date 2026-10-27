// Bridge impl — the services registry snapshot (K1a-2), SIM (web/dev). A browser preview supervises
// no processes, so there is nothing to read: it says so (Rule 1) instead of drawing service states.
import type { ServicesDomain } from "../domains";
import type { SimHost } from "./index";
import { Unavailable } from "../types";

export function simServices(_host: SimHost): ServicesDomain {
  return {
    async snapshot() {
      throw new Unavailable("services", "snapshot", "the browser preview supervises no processes");
    },
  };
}
