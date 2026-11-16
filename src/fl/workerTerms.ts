// =====================================================================
// citrate-core: the member's worker terms (FL design board 11, Settings › Work › Terms)
//
// Terms are sealed on this device and never published; only the floor is enforced on chain
// (WorkMarket join's minPrice). Data source (Rule 7): core's fl_work.rs, `<app data>/fl-work/
// terms.sealed` under the work key in the OS keyring (ML-KEM-768 + AES-256-GCM, as the Hermes
// history). They are written only after the member confirms them: the Settings › Work confirm
// card, or a core-rendered Hermes policy proposal card. This module never shows a card itself
// and Hermes output never calls it.
// =====================================================================
import { BRIDGE_MODE } from "../bridge/mode";
import { invoke } from "../bridge/tauri/invoke";

export interface WorkerTerms {
  /** Decimal SALT per unit, e.g. "1.20". */
  floorSaltPerUnit: string;
  hours: { start: string; end: string } | "any";
  weekdaysOnly: boolean;
  mainsOnly: boolean;
  /** Local dataset classes rounds may read; empty means none. */
  dataClasses: string[];
}

export interface WorkerTermsReceipt {
  /** ms since the epoch. */
  savedAt: number;
  termsHash: string;
  sealed: boolean;
}

/** Outside the desktop app there is no sealed store to read or write. */
export const TERMS_NEED_DESKTOP = "Your terms are kept by the desktop app";

const HHMM = /^([01]\d|2[0-3]):[0-5]\d$/;
const FLOOR = /^\d{1,6}(\.\d{1,18})?$/;

/** Why terms are malformed, or null when they are well formed. */
export function termsProblem(t: WorkerTerms): string | null {
  if (!FLOOR.test(t.floorSaltPerUnit)) return "The floor must be a SALT amount like 1.20";
  if (t.hours !== "any" && (!HHMM.test(t.hours.start) || !HHMM.test(t.hours.end))) return "Hours must be times like 23:00";
  if (t.dataClasses.some((c) => c.trim() === "")) return "A data class is empty";
  return null;
}

/** Seal confirmed terms in core. Refuses malformed terms before asking core; core checks again. */
export async function saveWorkerTerms(terms: WorkerTerms): Promise<WorkerTermsReceipt> {
  const bad = termsProblem(terms);
  if (bad) throw new Error(bad);
  if (BRIDGE_MODE !== "tauri") throw new Error(TERMS_NEED_DESKTOP);
  return invoke<WorkerTermsReceipt>("fl_work_terms_save", { terms });
}

/** The saved terms, or null when none are saved. */
export async function loadWorkerTerms(): Promise<WorkerTerms | null> {
  if (BRIDGE_MODE !== "tauri") throw new Error(TERMS_NEED_DESKTOP);
  return invoke<WorkerTerms | null>("fl_work_terms_get");
}
