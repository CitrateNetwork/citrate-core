// =====================================================================
// citrate-core: the member's worker terms (FL design board 11, Settings › Work › Terms)
//
// Terms are sealed on this device and never published; only the floor is enforced on chain. They
// are written to core storage (worker-policy.json, board 11) after the member confirms them: the
// Settings › Work confirm card, or a core-rendered Hermes policy proposal card. This module never
// shows a card itself and Hermes output never calls it.
//
// Until core's worker-policy storage ships, saving is refused with a plain sentence (nothing is
// written anywhere, Rule 1) and loading reads nothing.
// =====================================================================

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

export const TERMS_SAVE_UNAVAILABLE = "Saving terms is not available in this build";

const HHMM = /^([01]\d|2[0-3]):[0-5]\d$/;
const FLOOR = /^\d{1,6}(\.\d{1,18})?$/;

/** Why terms are malformed, or null when they are well formed. */
export function termsProblem(t: WorkerTerms): string | null {
  if (!FLOOR.test(t.floorSaltPerUnit)) return "The floor must be a SALT amount like 1.20";
  if (t.hours !== "any" && (!HHMM.test(t.hours.start) || !HHMM.test(t.hours.end))) return "Hours must be times like 23:00";
  if (t.dataClasses.some((c) => c.trim() === "")) return "A data class is empty";
  return null;
}

/** Write confirmed terms to core. Refused in this build (no storage yet); nothing changes. */
export async function saveWorkerTerms(terms: WorkerTerms): Promise<WorkerTermsReceipt> {
  const bad = termsProblem(terms);
  if (bad) throw new Error(bad);
  throw new Error(TERMS_SAVE_UNAVAILABLE);
}

/** The saved terms, or null when none are saved (always null until core storage ships). */
export async function loadWorkerTerms(): Promise<WorkerTerms | null> {
  return null;
}
