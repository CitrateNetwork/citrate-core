// The two pinned onboarding models, as the model step and Settings › Models show them. Data source:
// the pins in src-tauri/src/model.rs (DEFAULT_PIN / SMALL_PIN); localModels.test.ts reads that file
// and fails if these drift from it. The download itself always uses the Rust pins.
export type LocalModelKey = "default" | "small";

export interface LocalModelFacts {
  key: LocalModelKey;
  label: string;
  file: string;
  sizeBytes: number;
  sha256: string;
  repo: string;
  quant: string;
}

export const LOCAL_MODELS: Record<LocalModelKey, LocalModelFacts> = {
  default: {
    key: "default",
    label: "Gemma 4 E4B",
    file: "gemma-4-E4B-it-Q4_0.gguf",
    sizeBytes: 4_590_807_392,
    sha256: "a555b900214b477d8880e7832e0b8925e139b0159640036b09fe472b6f2097f2",
    repo: "ggml-org/gemma-4-E4B-it-GGUF",
    quant: "Q4_0",
  },
  small: {
    key: "small",
    label: "Gemma 4 E2B",
    file: "gemma-4-E2B-it-Q4_0.gguf",
    sizeBytes: 2_841_481_184,
    sha256: "8e30dff3ac4c8434c49a7036fa15564bdbb6044e42bf04550bf1a096ad7e6a52",
    repo: "ggml-org/gemma-4-E2B-it-GGUF",
    quant: "Q4_0",
  },
};

/** Free space kept beyond a model's remaining bytes before a download starts (DISK_MARGIN_BYTES). */
export const DISK_MARGIN_BYTES = 256 * 1024 * 1024;

/** Bytes as decimal GB with two decimals ("4.59"). */
export function gb(bytes: number): string {
  return (bytes / 1e9).toFixed(2);
}

/** "12.3 MB/s" from bytes per second. */
export function speedText(bytesPerSec: number): string {
  const mb = bytesPerSec / 1e6;
  return mb >= 10 ? `${mb.toFixed(0)} MB/s` : mb >= 0.1 ? `${mb.toFixed(1)} MB/s` : `${Math.max(1, Math.round(bytesPerSec / 1e3))} KB/s`;
}

/** "about 3 min left" from seconds. */
export function etaText(secs: number): string {
  if (secs < 60) return "less than a minute left";
  const m = Math.round(secs / 60);
  if (m < 60) return `about ${m} min left`;
  const h = Math.floor(m / 60);
  const rest = m % 60;
  return rest ? `about ${h} h ${rest} min left` : `about ${h} h left`;
}

/** Whether the disk is too small for `key` with what is already downloaded (`have`). */
export function diskTooSmall(freeBytes: number | null, key: LocalModelKey, have = 0): boolean {
  if (freeBytes === null) return false;
  return freeBytes < LOCAL_MODELS[key].sizeBytes - have + DISK_MARGIN_BYTES;
}
