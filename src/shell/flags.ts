// created: 2026-11-28 | branch: feat/v60-l0-wp09-flags
// author: Larry Klosowski (L0 lane, L0-WP09; Claude Opus 5.5)
// status: active
//
// L0-WP09 (planset v060, L0_INTEGRATION_SEAMS 3.7): the webview's read of the build-time feature
// flags in release/flags.json, through the generated ./flags.gen (scripts/gen-flags.mjs).
import { FLAGS, type FlagId } from "./flags.gen";

export type { FlagId };

/** The flag's value in this build. An id the table does not hold reads as off (fails closed). */
export function flag(id: FlagId): boolean {
  return Object.prototype.hasOwnProperty.call(FLAGS, id) && FLAGS[id] === true;
}
