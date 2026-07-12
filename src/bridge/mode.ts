// =====================================================================
// citrate-core — bridge runtime selection (CORE-A1 · A1.1)
//
// `bridge.mode` is `tauri` when running inside the packaged app, else `sim`.
// Detection is done ONCE, at the boundary, via `@tauri-apps/api` — Tauri v2
// injects `window.__TAURI_INTERNALS__` and ships `isTauri()`. We read the
// injected global directly (synchronous, no import cost on the web path) and
// cross-check the SDK helper when present.
// =====================================================================
import { isTauri } from "@tauri-apps/api/core";

function detectMode(): "sim" | "tauri" {
  try {
    // Primary: the SDK's own check (reads the injected internals global).
    if (isTauri()) return "tauri";
  } catch {
    /* isTauri may throw in a non-browser/test env — fall through */
  }
  // Fallback: the injected global itself. Belt-and-suspenders for early boot.
  if (typeof window !== "undefined" && "__TAURI_INTERNALS__" in window) {
    return "tauri";
  }
  return "sim";
}

export const BRIDGE_MODE: "sim" | "tauri" = detectMode();

/**
 * Build-time guard. The sim adapter asserts this before doing any work so a
 * packaged Tauri build can never execute the dev shim (Rule 1 / A1.2). If the
 * bundler ever selects `tauri` but sim code runs, this throws loudly rather
 * than silently serving sim data as live.
 */
export function assertSimAllowed(op: string): void {
  if (BRIDGE_MODE === "tauri") {
    throw new Error(
      `citrate-core: sim adapter reached in a Tauri build (op "${op}"). ` +
        `The dev shim must never execute in the packaged app.`,
    );
  }
}
