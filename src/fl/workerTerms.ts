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

// ---------------------------------------------------------------- the data classes this device has

/** The data classes worker terms (and a Hermes worker-terms proposal) may name, with the words the
 *  app shows: the kinds of dataset this device can really hold. In this build there is one local
 *  dataset source, core's training-set builder for verified Hermes conversations
 *  (fl_trajectories.rs, Groups › Rounds › Datasets). A round reads only datasets the member adds
 *  to it, on this device. Owned here (lane 1); src/agent/workerProposal.ts imports it. */
export const ALLOWED_DATA_CLASSES: Readonly<Record<string, string>> = {
  "hermes-conversations": "Verified Hermes conversations on this device",
};

/** Why these data classes cannot be named, or null. */
export function dataClassProblem(classes: readonly string[]): string | null {
  if (classes.length > 8) return "At most 8 data classes";
  const unknown = classes.filter((c) => !Object.prototype.hasOwnProperty.call(ALLOWED_DATA_CLASSES, c));
  if (unknown.length > 0) {
    return `Data class ${unknown.map((c) => JSON.stringify(c.slice(0, 40))).join(", ")} is not one this device has (allowed: ${Object.keys(ALLOWED_DATA_CLASSES).join(", ")})`;
  }
  if (new Set(classes).size !== classes.length) return "A data class is listed twice";
  return null;
}

/** The words for a data class (an unknown one is shown as it is). */
export function dataClassWords(c: string): string {
  return ALLOWED_DATA_CLASSES[c] ?? c;
}

const HHMM = /^([01]\d|2[0-3]):[0-5]\d$/;
const FLOOR = /^\d{1,6}(\.\d{1,18})?$/;

/** Why terms are malformed, or null when they are well formed. */
export function termsProblem(t: WorkerTerms): string | null {
  if (!FLOOR.test(t.floorSaltPerUnit)) return "The floor must be a SALT amount like 1.20";
  if (t.hours !== "any" && (!HHMM.test(t.hours.start) || !HHMM.test(t.hours.end))) return "Hours must be times like 23:00";
  if (t.dataClasses.some((c) => c.trim() === "")) return "A data class is empty";
  return dataClassProblem(t.dataClasses);
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
