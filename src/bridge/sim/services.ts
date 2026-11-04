// Bridge impl — the services registry snapshot (K1a-2) and sidecar health (CORE-D4), SIM
// (web/dev). A browser preview supervises no processes, so there is nothing to read: it says so
// (Rule 1) instead of drawing service states, and no alert ever arrives.
import type { ServicesDomain } from "../domains";
import type { SimHost } from "./index";
import { Unavailable } from "../types";

export function simServices(_host: SimHost): ServicesDomain {
  return {
    async snapshot() {
      throw new Unavailable("services", "snapshot", "the browser preview supervises no processes");
    },
    async health() {
      throw new Unavailable("services", "health", "the browser preview supervises no processes");
    },
    async onAlert() {
      return () => {};
    },
  };
}
