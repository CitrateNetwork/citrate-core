// citrate-core: bridge domain `model`, moved verbatim from src/bridge/domains.ts by L0-WP05
// (planset v060). src/bridge/domains.ts re-exports it, so imports from "../bridge/domains" keep
// working. The owning lane is the row for this path in .agentile/v60-ownership.map.

/**
 * CORE-BC-3 — the local Gemma model status. `state` is the honest, file-derived
 * lifecycle; `Ready` is EARNED only by a real SHA-256 verify (never mere presence,
 * Rule 1). The download/verify facts (bytes/pct) trace to real bytes on disk — no
 * fabricated progress. Mirrors the Rust `ModelStatus` (serde camelCase).
 */
export type ModelStatus =
  | { state: "notPresent" }
  | {
      state: "downloading";
      downloadedBytes: number;
      totalBytes: number;
      pct: number;
      /** Live, while a download runs: smoothed speed and time left. */
      bytesPerSec?: number;
      etaSecs?: number;
      /** Live, while it waits to retry: seconds until the next try and the plain-words reason. */
      retryInSecs?: number;
      lastError?: string;
    }
  | { state: "paused"; downloadedBytes: number; totalBytes: number; pct: number }
  | { state: "verifying" }
  | { state: "ready" }
  | { state: "error"; msg: string };

/** Mirrors the Rust `LocalModelOptions` (serde camelCase). */
export interface LocalModelOptions {
  chosen: "default" | "small";
  options: { key: "default" | "small"; label: string; file: string; sizeBytes: number; sha256: string; hfRepo: string }[];
  freeBytes: number | null;
  marginBytes: number;
}

/**
 * CORE-BC-3 (BC-3.1/3.2) — the local model domain. `status/download/verify` drive
 * the BC-3.1 download+verify; `serveStart` spawns the BC-3.2 llama-server sidecar
 * (fails closed unless the model is verified-Ready + the binary is bundled). In
 * Tauri these invoke the real Rust commands; the sim impl is honest in web-dev
 * (guarded by assertSimAllowed — a fake progress animation is web-dev ONLY, never
 * presented as real in the packaged app).
 */
export interface ModelDomain {
  /** The honest, file-derived status (Ready only after a real verify). */
  status(): Promise<ModelStatus>;
  /** STREAMED, resumable download + verify of the onboarding model. `model` ("default" | "small")
   *  also becomes the member's choice. Resolves once verified-Ready; rejects with plain words. A
   *  second call while one runs waits for it. A pause rejects; `status()` then reads `paused`. */
  download(model?: "default" | "small"): Promise<void>;
  /** Pause the running download (its progress is kept). */
  pause(): Promise<void>;
  /** The default and the smaller model, the member's choice, and the free disk space. */
  options(): Promise<LocalModelOptions>;
  /** Stream the file through SHA-256; on a match the model becomes Ready. */
  verify(): Promise<void>;
  /** Spawn the llama-server sidecar on the verified-Ready model (fails closed). */
  serveStart(): Promise<void>;
  /** SCL-S7.5a: release the llama-server supervisor (graceful stop, idempotent). Used by
   *  "Restart the local model" so a supervisor that is not running is started fresh. */
  serveStop(): Promise<void>;
}
