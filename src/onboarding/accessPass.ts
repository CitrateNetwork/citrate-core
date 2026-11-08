// Download gate: the first-run access check, TAURI only. Mirrors src-tauri/src/access_pass.rs.
// In the web/sim preview there is no gate: the check reads as off and nothing is asked.
import { BRIDGE_MODE } from "../bridge/mode";
import { invoke } from "../bridge/tauri/invoke";

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

export async function accessPassStatus(): Promise<AccessPassView> {
  if (BRIDGE_MODE !== "tauri") return ACCESS_OFF;
  return invoke<AccessPassView>("access_pass_status");
}

/** Check a pasted code or `citrate://access` link. Rejects with Rust's plain message. */
export async function accessPassVerify(input: string): Promise<AccessPassView> {
  if (BRIDGE_MODE !== "tauri") return ACCESS_OFF;
  return invoke<AccessPassView>("access_pass_verify", { input });
}
