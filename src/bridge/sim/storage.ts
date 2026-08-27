// CX bridge impl — storage (C-17), SIM. Owned by lane s2 after S0. Honest-empty (Rule 1).
import type { StorageDomain } from "../domains";
import type { SimHost } from "./index";

export function simStorage(_host: SimHost): StorageDomain {
  return {
    async add() {
      return { cid: "sim-cid-unavailable", sizeBytes: 0 };
    },
    async pin() {
      /* sim: no bond tx */
    },
    async list() {
      return [];
    },
    async retrieve() {
      return { path: "" };
    },
    async unpin() {
      /* sim: no-op */
    },
  };
}
