// CX bridge impl — storage (C-17), SIM. Owned by lane s2 after S0. Honest-empty (Rule 1).
// K4: the web preview has no IPFS store, no memory store and no chain reads, so the parameter read
// and backup/restore reject with a plain reason; the surfaces show it instead of any number.
import type { StorageDomain } from "../domains";
import type { SimHost } from "./index";

const DESKTOP_ONLY = "Needs the desktop app: the web preview has no local file or memory store.";

export function simStorage(_host: SimHost): StorageDomain {
  return {
    async add() {
      return { cid: "sim-cid-unavailable", sizeBytes: 0 };
    },
    async pinLocal() {
      /* sim: no local store */
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
    async pinParams() {
      throw new Error("Pin parameters are read from chain by the desktop app.");
    },
    async backupExport() {
      throw new Error(DESKTOP_ONLY);
    },
    async backupVerify() {
      throw new Error(DESKTOP_ONLY);
    },
    async backupRestore() {
      throw new Error(DESKTOP_ONLY);
    },
  };
}
