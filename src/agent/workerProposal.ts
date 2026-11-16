// =====================================================================
// citrate-core: what Hermes may put on a worker-terms proposal card (FL design board 01)
//
// Core checks a proposal before any card is shown. Beyond the well-formedness check of
// src/fl/workerTerms.ts (termsProblem), the data classes must come from the allowed list: the kinds
// of dataset this device can really hold. In this build there is one local dataset source, core's
// training-set builder for verified Hermes conversations (fl_trajectories.rs, Groups › Rounds ›
// Datasets), so that is the one class a proposal may name. A round reads only datasets the member
// adds to it, on this device.
// =====================================================================

/** The data classes a worker-terms proposal may name, with the words the card shows. */
export const ALLOWED_DATA_CLASSES: Readonly<Record<string, string>> = {
  "hermes-conversations": "Verified Hermes conversations on this device",
};

/** Why these data classes cannot be proposed, or null. */
export function dataClassProblem(classes: readonly string[]): string | null {
  if (classes.length > 8) return "At most 8 data classes";
  const unknown = classes.filter((c) => !Object.prototype.hasOwnProperty.call(ALLOWED_DATA_CLASSES, c));
  if (unknown.length > 0) {
    return `Data class ${unknown.map((c) => JSON.stringify(c.slice(0, 40))).join(", ")} is not one this device has (allowed: ${Object.keys(ALLOWED_DATA_CLASSES).join(", ")})`;
  }
  if (new Set(classes).size !== classes.length) return "A data class is listed twice";
  return null;
}

/** A proposal id unique across launches (restored history never collides with a new card). */
export function newProposalId(): string {
  const c = globalThis.crypto as Crypto | undefined;
  if (c && typeof c.randomUUID === "function") return "p-" + c.randomUUID();
  return "p-" + Date.now().toString(36) + "-" + Math.random().toString(36).slice(2, 10);
}

/** The card's words for a data class (an unknown one is shown as it is). */
export function dataClassWords(c: string): string {
  return ALLOWED_DATA_CLASSES[c] ?? c;
}
