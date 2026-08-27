// CX bridge impl — modelsCatalog (C-16), SIM (web/dev). Owned by lane s1 after S0.
//
// Honest-empty: the sim adapter never fabricates a catalog (Rule 1). A dev build shows no
// downloadable models rather than fake ones. CX-S1 may add a small honest sim fixture.
import type { ModelsCatalogDomain } from "../domains";
import type { SimHost } from "./index";

export function simModelsCatalog(_host: SimHost): ModelsCatalogDomain {
  return {
    async local() {
      return [];
    },
    async search() {
      return [];
    },
    async download() {
      /* sim: no-op (no real download) */
    },
    async select() {
      /* sim: no-op (no real llama-server) */
    },
  };
}
