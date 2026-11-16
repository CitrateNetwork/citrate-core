// =====================================================================
// citrate-core 0.5.4 (WP-H6): the local dataset store's types, as core returns them.
//
// Data source (Rule 7): src-tauri/src/fl_datasets.rs, the sealed store under
// `<app data>/datasets/` (each version's manifest is the exact JSON DatasetRegistry's
// `manifestHash` commits to, keccak256 of those bytes). These types mirror the Rust structs
// field for field (camelCase); the store is the only writer.
// =====================================================================

export type FlSourceKind = "folder" | "conversations" | "web" | "licensed" | "connected-agent";
export type FlRobotsVerdict = "allowed" | "not-applicable";
export type FlTosClass = "allowlisted" | "member-approved" | "not-applicable";
export type FlLicenseBasis = "found" | "member-attested" | "unknown";
export type FlPrivacyLabel = "private" | "public" | "members" | "group" | "index-only";

export interface FlLicenseRecord {
  /** SPDX id or LicenseRef-…; null when unknown (publishing is refused while any source is unknown). */
  id: string | null;
  basis: FlLicenseBasis;
  evidence: string | null;
}

/** One source's provenance record (gap §3.1 rule 5). */
export interface FlSourceRecord {
  id: string;
  kind: FlSourceKind;
  /** A URL, `<granted folder>/<path>`, or `conversations`. Never an absolute local path. */
  locator: string;
  fetchedAtMs: number;
  contentSha256: string;
  license: FlLicenseRecord;
  robots: FlRobotsVerdict;
  tosClass: FlTosClass;
  consentRef: string;
  collectorVersion: string;
  untrusted: boolean;
  rows: number;
}

export interface FlPiiScanRecord {
  scanner: string;
  scannedAtMs: number;
  contentHash: string;
  pass: boolean;
  counts: Record<string, number>;
}

export interface FlDerivedOp {
  op: string;
  detail: string;
  removed: number;
  changed: number;
  added: number;
}

/** A version's manifest (the fields of `citrate.dataset.manifest.v1`, in order). */
export interface FlDatasetManifest {
  schema: string;
  dataset: string;
  name: string;
  dataClass: string;
  version: number;
  prev: string;
  createdAtMs: number;
  privacyLabel: FlPrivacyLabel;
  containsPersonalData: boolean;
  license: string | null;
  sources: FlSourceRecord[];
  rows: number;
  contentHash: string;
  piiScan: FlPiiScanRecord | null;
  derivedBy: FlDerivedOp[];
  consent: string[];
  untrusted: boolean;
  publishedPrev: string | null;
  writtenBy: string;
}

export interface FlDatasetVersion {
  dataset: string;
  version: number;
  manifestHash: string;
  prev: string;
  createdAtMs: number;
  rows: number;
  sources: number;
  contentHash: string;
  privacyLabel: FlPrivacyLabel;
  containsPersonalData: boolean;
  license: string | null;
  unknownLicenseSources: number;
  piiScan: FlPiiScanRecord | null;
  derivedBy: FlDerivedOp[];
  untrusted: boolean;
}

export interface FlDataset {
  id: string;
  name: string;
  dataClass: string;
  createdAtMs: number;
  /** Oldest first. */
  versions: FlDatasetVersion[];
  chainOk: boolean;
  chainProblem: string | null;
}

export interface FlDatasetList {
  datasets: FlDataset[];
  unreadable: string[];
}

export interface FlManifestView {
  manifestHash: string;
  /** The exact bytes manifestHash covers. */
  manifestJson: string;
  manifest: FlDatasetManifest;
}

export interface FlSampleRow {
  index: number;
  source: string;
  text: string;
  truncated: boolean;
}

export interface FlDatasetSample {
  dataset: string;
  version: number;
  totalRows: number;
  rows: FlSampleRow[];
  untrusted: boolean;
}

export type FlCleanOp =
  | { op: "dedupe" }
  | { op: "drop-rows"; rows: number[] }
  | { op: "drop-source"; source: string }
  | { op: "redact"; find: string; replace: string }
  | { op: "redact-secrets" }
  | { op: "normalize" };

export interface FlDiffRow {
  index: number;
  kind: "removed" | "changed";
  before: string;
  after: string | null;
}

export interface FlCleanPreview {
  dataset: string;
  baseVersion: number;
  baseManifestHash: string;
  ops: FlCleanOp[];
  removed: number;
  changed: number;
  kept: number;
  resultContentHash: string;
  diff: FlDiffRow[];
  untrusted: boolean;
}

export type FlCreateSource = { kind: "folder"; path: string } | { kind: "conversations"; max: number };

export interface FlDatasetCreateRequest {
  name: string;
  dataClass: string;
  source: FlCreateSource;
  /** The consent words the member agreed to on the card. */
  consentStatement: string;
}

export interface FlDatasetCreateOutcome {
  version: FlDatasetVersion;
  skipped: string[];
}

export interface FlDatasetConsent {
  id: string;
  sourceKind: FlSourceKind;
  scope: string;
  statement: string;
  grantedAtMs: number;
  withdrawnAtMs: number | null;
}

export interface FlConsentWithdrawn {
  consent: FlDatasetConsent;
  newVersion: FlDatasetVersion | null;
  earlierVersionsKept: boolean;
}

/** The bridge to core's `fl_dataset_*` commands. */
export interface FlDatasetsDomain {
  list(): Promise<FlDatasetList>;
  manifest(dataset: string, version: number): Promise<FlManifestView>;
  /** Only after the member approved the create card. */
  create(request: FlDatasetCreateRequest): Promise<FlDatasetCreateOutcome>;
  sample(dataset: string, version: number, n?: number, offset?: number): Promise<FlDatasetSample>;
  /** Writes nothing. */
  cleanPreview(dataset: string, version: number, ops: FlCleanOp[]): Promise<FlCleanPreview>;
  /** Only after the member approved the diff card; core recomputes and refuses a different result. */
  cleanApply(dataset: string, version: number, ops: FlCleanOp[], resultContentHash: string): Promise<FlDatasetVersion>;
  consentList(dataset: string): Promise<FlDatasetConsent[]>;
  /** Lowers authority: no card. */
  consentWithdraw(dataset: string, consent: string): Promise<FlConsentWithdrawn>;
  delete(dataset: string): Promise<void>;
}
