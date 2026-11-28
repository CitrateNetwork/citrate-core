// citrate-core: bridge domain `modelsCatalog`, moved verbatim from src/bridge/domains.ts by L0-WP05
// (planset v060). src/bridge/domains.ts re-exports it, so imports from "../bridge/domains" keep
// working. The owning lane is the row for this path in .agentile/v60-ownership.map.

/** A downloadable/local model, from Hugging Face, GitHub Releases, or the bundle (C-16). */
export interface ModelDescriptor {
  id: string;
  source: "hf" | "github" | "bundled";
  repo: string;
  file: string;
  revision?: string;
  sizeBytes: number;
  sha256: string;
  kind: "gguf" | "safetensors";
}

/** C-16 — model catalog & switcher (local + HF + GitHub). Wired in CX-S1. */
/** HUP-S0.3 — an interrupted catalog download (survives an app restart; resumable). */
export interface PartialDownload {
  /** The catalog id; `download(id)` resumes its `.part`. */
  id: string;
  file: string;
  downloadedBytes: number;
  totalBytes: number;
  /** Whole percent, 0–100. */
  pct: number;
}

/** Whether a downloaded model can be deleted now (Rust `model_delete::DeleteState`). */
export interface ModelDeleteState {
  file: string;
  sizeBytes: number;
  deletable: boolean;
  /** Why not (in use, shipped with the app, downloading), when not deletable. */
  reason: string | null;
}

/** What a delete removed (Rust `model_delete::Deleted`). */
export interface ModelDeleted {
  file: string;
  /** Bytes removed: the GGUF plus its side files. */
  freedBytes: number;
  /** The chat server had it selected; the selection fell back to the default. */
  selectionCleared: boolean;
}

export interface ModelsCatalogDomain {
  /** Locally-present, verified models. */
  local(): Promise<ModelDescriptor[]>;
  /** HUP-S0.3 — interrupted catalog downloads that can be resumed. */
  partials(): Promise<PartialDownload[]>;
  /** Search downloadable models from a connected source (HF Hub / GitHub Releases). */
  search(source: "hf" | "github", query: string): Promise<ModelDescriptor[]>;
  /** Download + verify a descriptor; resolves on Ready. `onProgress` (0–100) fires as the
   *  bytes stream, so a multi-GB GGUF shows a real progress bar instead of a silent block. */
  download(id: string, onProgress?: (pct: number) => void): Promise<void>;
  /** Switch the active local model (restarts llama-server -m). */
  select(id: string): Promise<void>;
  /** Which downloaded models can be deleted now, and why not. Member-only (Models screen). */
  deleteStates(): Promise<ModelDeleteState[]>;
  /** Delete one downloaded model by file name, after the member confirmed it. Refused while the
   *  model is in use, downloading, or shipped with the app. Never an agent tool. */
  deleteLocal(file: string): Promise<ModelDeleted>;
  /** Hermes WP0.2b — the models registered on-chain in the ModelRegistry (40204). A PURE
   *  read (getAllModelHashes + getModel); NOT-yet-local, so the router marks them
   *  "download to use". Honest empty list on an unwired/failed read (never fabricated). */
  registry(): Promise<RegistryModel[]>;
  /** Hermes P3 / WP3.1 — propose registering a pulled+verified+pinned model on-chain
   *  (ModelRegistry.registerModel). Builds the calldata and submits a PENDING
   *  SignatureCeremony (Rule 3 — the human approves + broadcasts); nothing signs here.
   *  `ipfsCid` MUST be a real pinned CID (the contract requires it) — an empty CID
   *  rejects up-front. On confirm, the model appears in `registry()`. */
  register(input: RegisterModelInput): Promise<void>;
}

/** The fields ModelRegistry.registerModel needs (Hermes P3 / WP3.1). */
export interface RegisterModelInput {
  name: string;
  framework: string;
  version: string;
  /** A real pinned IPFS CID for the weights — required by the contract. */
  ipfsCid: string;
  sizeBytes: number;
  /** Per-inference price in wei (0 for free). */
  inferencePrice: number;
  description: string;
  license: string;
  tags: string[];
}

/** A model from the on-chain ModelRegistry (Hermes WP0.2b). Mirrors the Rust RegistryModel. */
export interface RegistryModel {
  /** The `0x`-hex modelHash — the stable id. */
  id: string;
  /** Human name. */
  name: string;
  /** `0x`-hex owner address. */
  owner: string;
  /** IPFS CID of the model weights. */
  ipfsCid: string;
}
