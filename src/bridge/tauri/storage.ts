// CX bridge impl — storage (C-17), TAURI. Owned by lane s2 (CX-S2).
//
// S2.3: real invokes of the S2.1 kubo seam commands. `pin`'s `bondSalt` is recorded locally; the
// real ceremony-gated on-chain bond is S2.2 (blocked on the chain-side CommD fix — see
// docs/FINDING_PIN_COMMD_BOND_2026-08-26.md), so the surface pins LOCALLY and says so honestly.
import { invoke } from "./invoke";
import type { PinRow, StorageDomain } from "../domains";

export const tauriStorage: StorageDomain = {
  add(path) {
    return invoke<{ cid: string; sizeBytes: number }>("storage_add", { path });
  },
  async pin(cid, bondSalt) {
    await invoke("storage_pin", { cid, bondSalt });
  },
  list() {
    return invoke<PinRow[]>("storage_list");
  },
  async retrieve(cid) {
    // The Rust command returns the on-disk path string; the domain shape wraps it.
    const path = await invoke<string>("storage_retrieve", { cid });
    return { path };
  },
  async unpin(cid) {
    await invoke("storage_unpin", { cid });
  },
};
