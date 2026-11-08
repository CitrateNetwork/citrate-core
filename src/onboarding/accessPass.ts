// Download gate: the first-run access check, TAURI only. Mirrors src-tauri/src/access_pass.rs.
// In the web/sim preview there is no gate: the check reads as off and nothing is asked.
import { BRIDGE_MODE } from "../bridge/mode";
import { invoke } from "../bridge/tauri/invoke";
import { STORAGE_KEY } from "../shell/state";

export interface AccessPassView {
  /** "off": this build does not check. "on": it does. */
  gate: "off" | "on";
  /** Setup must ask for an access code before it continues. */
  required: boolean;
  /** "invited" (a code was accepted), "existing" (an install from before the check), or null. */
  recorded: string | null;
  platform: string | null;
}

export const ACCESS_OFF: AccessPassView = { gate: "off", required: false, recorded: null, platform: null };

/**
 * Whether an earlier run of the app FINISHED onboarding: the saved app state (`STORAGE_KEY`) holds
 * `stage: "done"`, which every build since 0.5.0 writes when onboarding ends and nothing else writes.
 * Other app data is no sign of an earlier install: config.json is saved on every quit and the custody
 * envelope is made on every launch, so a fresh install that quit at the access check has both.
 */
export function finishedOnboardingBefore(raw: string | null): boolean {
  if (!raw) return false;
  let saved: unknown;
  try {
    saved = JSON.parse(raw);
  } catch {
    return false;
  }
  if (saved === null || typeof saved !== "object" || Array.isArray(saved)) return false;
  return (saved as { stage?: unknown }).stage === "done";
}

function savedState(): string | null {
  try {
    return localStorage.getItem(STORAGE_KEY);
  } catch {
    return null;
  }
}

/** The check's status. Rust records `existing` (never asks) when the saved state shows a finished onboarding. */
export async function accessPassStatus(): Promise<AccessPassView> {
  if (BRIDGE_MODE !== "tauri") return ACCESS_OFF;
  return invoke<AccessPassView>("access_pass_status", { onboarded: finishedOnboardingBefore(savedState()) });
}

/** Check a pasted code or `citrate://access` link. Rejects with Rust's plain message. */
export async function accessPassVerify(input: string): Promise<AccessPassView> {
  if (BRIDGE_MODE !== "tauri") return ACCESS_OFF;
  return invoke<AccessPassView>("access_pass_verify", { input });
}
