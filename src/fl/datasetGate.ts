// =====================================================================
// citrate-core 0.5.4 (WP-H7): the dataset PII scan, license check and publish gate, webview side.
//
// Everything is decided in Rust (src-tauri/src/fl_dataset_gate.rs over pii_scan.rs and the dataset
// store); this module is the typed seam to those commands and the plain-words text Hermes gets.
// The model never sets a scan verdict. The web preview has no dataset store, so every call there
// refuses (Rule 1).
// =====================================================================
import type { FlPrivacyLabel } from "./datasetTypes";

/** One finding's location (mirrors Rust `RowFinding`; never the matched text). */
export interface FlRowFinding {
  row: number;
  class: string;
  start: number;
  end: number;
}

/** A version's scan (mirrors Rust `pii_scan::ScanReport`). */
export interface FlScanReport {
  scanner: string;
  passed: boolean;
  rowsScanned: number;
  counts: Record<string, number>;
  findings: FlRowFinding[];
  truncated: boolean;
  coverage: string;
}

export type FlLicenseVerdict = "ok" | "member-owned" | "unknown" | "not-publishable";

export interface FlSourceLicense {
  source: string;
  kind: string;
  locator: string;
  license: string | null;
  basis: "found" | "member-attested" | "unknown";
  evidence: string | null;
  verdict: FlLicenseVerdict;
  reason: string | null;
}

/** Mirrors Rust `LicenseReport`. */
export interface FlLicenseReport {
  dataset: string;
  version: number;
  sources: FlSourceLicense[];
  ok: boolean;
  shareAlike: string[];
}

/** Mirrors Rust `PublishCheck`. */
export interface FlPublishCheck {
  allowed: boolean;
  refusals: string[];
  licenses: FlLicenseReport;
}

export const GATE_NEEDS_DESKTOP = "dataset checks need the desktop app; nothing was read";

async function call<T>(cmd: string, args: Record<string, unknown>): Promise<T> {
  const { BRIDGE_MODE } = await import("../bridge/mode");
  if (BRIDGE_MODE !== "tauri") throw new Error(GATE_NEEDS_DESKTOP);
  const { invoke } = await import("../bridge/tauri/invoke");
  return invoke<T>(cmd, args);
}

/** The seam (an object, so tests can spy on its methods). */
export const datasetGate = {
  /** Scan a version on this device and record the result in the store. */
  piiScan: (dataset: string, version: number) => call<FlScanReport>("fl_dataset_pii_scan", { dataset, version }),
  licenseCheck: (dataset: string, version: number) => call<FlLicenseReport>("fl_dataset_license_check", { dataset, version }),
  publishCheck: (dataset: string, version: number, license: string, privacy: FlPrivacyLabel) =>
    call<FlPublishCheck>("fl_dataset_publish_check", { dataset, version, license, privacy }),
};

const LABELS: Record<string, string> = {
  email: "email address",
  phone: "phone number",
  card: "card number",
  "government-id": "government id",
  iban: "bank account (IBAN)",
  "ip-address": "IP address",
  "street-address": "street address",
  name: "person's name",
  secret: "key, token or seed phrase",
};

/** "email address" -> "email addresses", for a count above one. */
export function pluralLabel(label: string): string {
  const special: Record<string, string> = {
    "person's name": "names",
    "key, token or seed phrase": "keys, tokens or seed phrases",
    "bank account (IBAN)": "bank accounts (IBAN)",
  };
  if (special[label]) return special[label];
  return label.endsWith("ss") ? label + "es" : label + "s";
}

/** The scan in plain words for Hermes: counts by class and which rows, never the matched text. */
export function scanForAgent(dataset: string, version: number, r: FlScanReport): string {
  if (r.passed) {
    return `The private-information scan of ${dataset} v${version} found nothing in ${r.rowsScanned} rows (${r.scanner}, on this device). ${r.coverage}`;
  }
  const found = Object.entries(r.counts)
    .filter(([, n]) => n > 0)
    .map(([k, n]) => `${n} ${n === 1 ? LABELS[k] ?? k : pluralLabel(LABELS[k] ?? k)}`)
    .join(", ");
  const rows = [...new Set(r.findings.map((f) => f.row + 1))].slice(0, 20);
  return (
    `The private-information scan of ${dataset} v${version} found ${found} in ${r.rowsScanned} rows (${r.scanner}, on this device). ` +
    `Rows: ${rows.join(", ")}${r.truncated || rows.length === 20 ? " and more" : ""}. ` +
    "Publishing is refused until they are redacted (a clean in Rounds › Datasets writes a new version) or the member labels the dataset Contains personal data, which can never be Public. " +
    r.coverage
  );
}

/** The license report in plain words for Hermes. */
export function licensesForAgent(r: FlLicenseReport): string {
  const lines = r.sources.map((s) => {
    const what = s.license ? `${s.license} (${s.basis === "member-attested" ? "attested by the member" : "found"})` : "no license";
    const v =
      s.verdict === "ok"
        ? "publishable"
        : s.verdict === "member-owned"
          ? "the member's own content; the dataset license covers it"
          : s.reason ?? "not publishable";
    return `- ${s.source} ${s.locator}: ${what}; ${v}`;
  });
  const sa = r.shareAlike.length ? ` A source is under ${r.shareAlike.join(" and ")}, so the dataset must be published under that license.` : "";
  return `${r.dataset} v${r.version}: ${r.ok ? "every source can be published" : "some sources block publishing"}.${sa}\n${lines.join("\n")}`;
}
