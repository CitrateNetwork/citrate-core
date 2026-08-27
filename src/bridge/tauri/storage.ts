// CX bridge impl — storage (C-17), TAURI. Owned by lane s2 (CX-S2) after S0.
// S0.2 stub: honest Unavailable. CX-S2 wires kubo add/pin/ls/rm/cat + the ceremony-gated
// IPFSIncentivesV3 bond, copying this shape.
import type { StorageDomain } from "../domains";
import { Unavailable } from "../types";

export const tauriStorage: StorageDomain = {
  async add() {
    throw new Unavailable("storage", "add");
  },
  async pin() {
    throw new Unavailable("storage", "pin");
  },
  async list() {
    throw new Unavailable("storage", "list");
  },
  async retrieve() {
    throw new Unavailable("storage", "retrieve");
  },
  async unpin() {
    throw new Unavailable("storage", "unpin");
  },
};
