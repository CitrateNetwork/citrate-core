// =====================================================================
// citrate-core — the held-out manifest for federated LoRA training (HUP-S9.3 / RA-15).
//
// The S9.4 eval delta is only honest if no eval prompt was trained on. This module turns an eval
// dataset into a manifest of hashed, normalized prompts. citrate-compute-pool's dataset converter
// (training-worker/src/fl/dataset.rs) drops any training example with a user message whose
// normalized hash is in the manifest. The manifest carries hashes, not prompts, so the prompts
// themselves never travel into a training repo (planset 05 "Eval provenance").
//
// The normalization is specified twice, here and in the Rust converter, and both sides test the
// same vectors (heldout.test.ts, compute-pool dataset_tests.rs):
//   lowercase; every character that is neither alphabetic nor numeric becomes a space; runs of
//   whitespace collapse to one space; leading and trailing space is trimmed.
// =====================================================================
import type { ToolcallDataset } from "./runner.ts";

export const HELDOUT_FORMAT = "citrate-heldout-v1";
export const HELDOUT_NORMALIZATION =
  "lowercase; every character that is neither alphabetic nor numeric becomes a space; whitespace runs collapse to one space; trimmed";

export interface HeldoutItem {
  id: string;
  sha256: string;
}

export interface HeldoutManifest {
  format: typeof HELDOUT_FORMAT;
  dataset: string;
  normalization: string;
  count: number;
  items: HeldoutItem[];
}

/** The shared normalization (see the header). */
export function normalizePrompt(s: string): string {
  return s
    .toLowerCase()
    .replace(/[^\p{Alphabetic}\p{N}]/gu, " ")
    .replace(/\s+/g, " ")
    .trim();
}

/**
 * Build the manifest for a tool-call dataset. `sha256Hex` is passed in (Node's crypto in the
 * tests and scripts) so this module stays free of Node imports. Items are sorted by id, so the
 * file is stable however the fragments are ordered.
 */
export function buildHeldoutManifest(ds: ToolcallDataset, sha256Hex: (text: string) => string): HeldoutManifest {
  const items = ds.tasks
    .map((t) => ({ id: t.id, sha256: sha256Hex(normalizePrompt(t.prompt)) }))
    .sort((a, b) => (a.id < b.id ? -1 : a.id > b.id ? 1 : 0));
  return {
    format: HELDOUT_FORMAT,
    dataset: ds.version,
    normalization: HELDOUT_NORMALIZATION,
    count: items.length,
    items,
  };
}

/** The manifest as committed: two-space JSON plus a trailing newline. */
export function serializeHeldoutManifest(m: HeldoutManifest): string {
  return `${JSON.stringify(m, null, 2)}\n`;
}
