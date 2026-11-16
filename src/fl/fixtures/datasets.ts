// Test fixture (0.5.4 WP-H6): dataset store results in the exact shapes core's fl_dataset_*
// commands return. Imported only by tests; production reads datasets from core's sealed store.
import type { FlCleanPreview, FlDataset, FlDatasetList, FlDatasetVersion, FlPublication, FlPublishCheck } from "../datasetTypes";

export const DATASET_ID = "ds-0123456789abcdef";
export const HASH_A = "0x" + "a1".repeat(32);
export const HASH_B = "0x" + "b2".repeat(32);
export const ZERO = "0x" + "00".repeat(32);

export function datasetVersion(over: Partial<FlDatasetVersion> = {}): FlDatasetVersion {
  return {
    dataset: DATASET_ID,
    version: 1,
    manifestHash: HASH_A,
    prev: ZERO,
    createdAtMs: 1_700_000_000_000,
    rows: 12,
    sources: 3,
    contentHash: HASH_B,
    privacyLabel: "private",
    containsPersonalData: false,
    license: null,
    unknownLicenseSources: 1,
    piiScan: null,
    derivedBy: [{ op: "create", detail: "from orchard-notes", removed: 0, changed: 0, added: 12 }],
    untrusted: false,
    ...over,
  };
}

export function dataset(over: Partial<FlDataset> = {}): FlDataset {
  return {
    id: DATASET_ID,
    name: "Orchard notes",
    dataClass: "orchard-notes",
    createdAtMs: 1_700_000_000_000,
    versions: [datasetVersion()],
    chainOk: true,
    chainProblem: null,
    ...over,
  };
}

export function datasetList(datasets: FlDataset[] = [dataset()]): FlDatasetList {
  return { datasets, unreadable: [] };
}

export function cleanPreview(over: Partial<FlCleanPreview> = {}): FlCleanPreview {
  return {
    dataset: DATASET_ID,
    baseVersion: 1,
    baseManifestHash: HASH_A,
    ops: [{ op: "dedupe" }],
    removed: 2,
    changed: 0,
    kept: 10,
    resultContentHash: "0x" + "c3".repeat(32),
    diff: [{ index: 4, kind: "removed", before: "a repeated row", after: null }],
    untrusted: false,
    ...over,
  };
}

export function publishCheck(over: Partial<FlPublishCheck> = {}): FlPublishCheck {
  return {
    dataset: DATASET_ID,
    name: "Orchard notes",
    version: 1,
    contentHash: HASH_B,
    rows: 12,
    sources: 3,
    license: "MIT",
    privacy: "public",
    registryPrev: ZERO,
    bundleBytes: 4096,
    untrusted: false,
    refusals: [],
    ...over,
  };
}

export function publication(over: Partial<FlPublication> = {}): FlPublication {
  return {
    version: 2,
    manifestHash: "0x" + "d4".repeat(32),
    contentHash: HASH_B,
    cid: "bafkreiexamplecid",
    license: "MIT",
    privacy: "public",
    publishedAtMs: 1_700_000_100_000,
    registryPrev: ZERO,
    ...over,
  };
}
