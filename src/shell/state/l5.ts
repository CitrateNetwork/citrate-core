// =====================================================================
// citrate-core: an AppState part (L0-WP06)
//
// Lane L5 (Models, multimodal, LoRA: the local model download, read aloud).
//
// L0-WP06 (lanes/L0_INTEGRATION_SEAMS.md 3.4). Owned by L5 (.agentile/v60-ownership.map).
// These fields moved here from state.ts verbatim, each with its freshState default and its
// PERSIST_SCOPE entry. state.ts composes AppState, freshState and PERSIST_SCOPE from every part;
// a field lives in exactly one part (src/shell/seams/statePartition.test.ts).
//
// New 0.6.0 state does not go here: it goes into a slice registered with registerSlice
// (src/shell/slices/registry.ts). This part changes when one of its legacy fields moves or retires.
// The persona seeding in freshState (the P2 to P4 sim values) stays in state.ts.
// =====================================================================

import type { PersistScope } from "../netScope";

export interface L5State {
  modelPct: number;
  /**
   * CORE-BC-3 — the local Gemma model lifecycle for the onboarding S6.5 step.
   * `modelState` is the honest, bridge-derived status: it becomes "ready" ONLY
   * after a real SHA-256 verify (never fabricated). `modelDownloadedBytes`/
   * `modelTotalBytes` trace to REAL bytes on disk (Tauri) or the preview animation
   * (web-dev). `modelSkipped` marks that the member chose to run chat on the
   * gateway/demo instead — an honest opt-out, not a failure. NOT persisted config
   * (runtime status, refreshed from the real bridge).
   */
  modelState: "notPresent" | "downloading" | "paused" | "verifying" | "ready" | "error";
  modelDownloadedBytes: number;
  modelTotalBytes: number;
  modelError: string | null;
  modelSkipped: boolean;
  /** Which pinned onboarding model the member's download is for (model_local_options). */
  modelChoice: "default" | "small";
  /** Live download facts from model_status (null when unknown / not downloading). */
  modelBytesPerSec: number | null;
  modelEtaSecs: number | null;
  /** While a download waits to retry: seconds left and the plain-words reason. */
  modelRetryInSecs: number | null;
  modelRetryReason: string | null;
  /** Free bytes on the disk holding the models folder (model_local_options), null when unknown. */
  modelFreeBytes: number | null;
  /** HUP-S3.7 — read Hermes's replies aloud with the persona's voice (the system speech engine).
   *  Off by default. Persisted. */
  hermesReadAloud: boolean;
}

export function freshL5(_pid: string): L5State {
  return {
    modelPct: 0,
    modelState: "notPresent",
    modelDownloadedBytes: 0,
    modelTotalBytes: 4_590_807_392,
    modelError: null,
    modelSkipped: false,
    modelChoice: "default",
    modelBytesPerSec: null,
    modelEtaSecs: null,
    modelRetryInSecs: null,
    modelRetryReason: null,
    modelFreeBytes: null,
    hermesReadAloud: false,
  };
}

const D = "device" as const;
export const L5_PERSIST_SCOPE = {
  hermesReadAloud: D,
} satisfies Partial<Record<keyof L5State, PersistScope>>;
