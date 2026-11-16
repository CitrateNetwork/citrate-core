// =====================================================================
// citrate-core: the on-device private-information check, webview side (V6, WP-H7).
//
// The scanner itself is Rust (src-tauri/src/pii_scan.rs, `pii_scan_text`): deterministic, offline,
// no model. This is the typed seam to it. The web preview has no scanner, so a check there fails
// and the caller posts nothing (fail closed).
// =====================================================================
import type { DisclosureScan } from "../surfaces/groups/groupHermes";

export const SCAN_NEEDS_DESKTOP = "the private-information check runs only in the desktop app";

export async function scanForDisclosure(text: string): Promise<DisclosureScan> {
  const { BRIDGE_MODE } = await import("../bridge/mode");
  if (BRIDGE_MODE !== "tauri") throw new Error(SCAN_NEEDS_DESKTOP);
  const { invoke } = await import("../bridge/tauri/invoke");
  return invoke<DisclosureScan>("pii_scan_text", { text });
}
