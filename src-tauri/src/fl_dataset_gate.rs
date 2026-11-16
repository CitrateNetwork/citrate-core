//! citrate-core 0.5.4 (WP-H7): **the dataset PII scan, the license recorder and the publish
//! gate**.
//!
//! These follow the owner's collection policy of 2026-11-16 (Hermes x FL gap analysis §3.1, rules
//! 5 and 6). They are enforced here, in core:
//!
//! - **The scanner decides.** [`scan_version`] runs [`crate::pii_scan`] over every string of a
//!   version's rows and records the result in the dataset store
//!   ([`DatasetStore::record_pii_scan`]). That record is bound to the version's `contentHash`.
//!   The model may propose redactions, but it never sets the verdict.
//! - **Licenses per source.** [`license_check`] reads every source record of a version. A source
//!   is publishable only with a known license that passes DatasetRegistry's rule
//!   ([`registry_license_ok`]) and is on [`PUBLISHABLE_LICENSES`]. The member's own content (a
//!   granted folder with no license file, or their Hermes conversations) is theirs to license:
//!   the dataset license chosen at publish covers it. Every other unknown source blocks publish.
//! - **The publish gate.** [`publish_gate`] refuses a publish when:
//!   - there is no scan of exactly this version's rows;
//!   - the scan found private information and the dataset is not labelled "Contains personal
//!     data";
//!   - a "Contains personal data" dataset would be Public. That is never allowed, whatever the
//!     scan says;
//!   - the visibility is Private (that is local only);
//!   - the license check fails, or the dataset license is not publishable;
//!   - a ShareAlike source would sit under a license that is not the same ShareAlike license.
//!
//! The publish itself (the IPFS pin and the DatasetRegistry record) calls [`publish_gate`] first,
//! and is refused unless it passes. Publishing is always HIC-1 and never runs in an envelope.
//!
//! Rule 3: nothing here signs. Rule 7: the data source is the dataset store (`rows_of`,
//! `manifest`).

use std::collections::BTreeMap;

use serde::Serialize;

use crate::fl_datasets::{
    registry_license_ok, DatasetStore, LicenseBasis, Manifest, PiiScanRecord, PrivacyLabel,
    SourceKind,
};
use crate::pii_scan::{scan_rows, ScanReport, SCANNER};

/// The licenses a dataset or a source may be published under (SPDX ids). Open licenses that
/// permit redistribution and derived works. NonCommercial and NoDerivatives licenses are not on
/// it: a published dataset trains models other members run.
pub const PUBLISHABLE_LICENSES: &[&str] = &[
    "CC0-1.0",
    "CC-BY-4.0",
    "CC-BY-SA-4.0",
    "CC-BY-3.0",
    "CC-BY-SA-3.0",
    "ODC-By-1.0",
    "ODbL-1.0",
    "PDDL-1.0",
    "CDLA-Permissive-2.0",
    "CDLA-Sharing-1.0",
    "MIT",
    "Apache-2.0",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "Unlicense",
];

fn share_alike(license: &str) -> bool {
    license.starts_with("CC-BY-SA-") || license == "ODbL-1.0" || license == "CDLA-Sharing-1.0"
}

/// Every string inside a JSON value, in order.
fn strings<'a>(v: &'a serde_json::Value, out: &mut Vec<&'a str>) {
    match v {
        serde_json::Value::String(s) => out.push(s),
        serde_json::Value::Array(a) => a.iter().for_each(|x| strings(x, out)),
        serde_json::Value::Object(o) => o.values().for_each(|x| strings(x, out)),
        _ => {}
    }
}

/// The scan of one version, as rows: each row's strings joined, so a finding names its row.
pub fn scan_manifest_rows(rows: &[crate::fl_datasets::Row]) -> ScanReport {
    let texts: Vec<String> = rows
        .iter()
        .map(|r| {
            let mut s = Vec::new();
            strings(&r.data, &mut s);
            s.join("\n")
        })
        .collect();
    scan_rows(texts.iter().map(String::as_str))
}

/// The store's record of a report: counts by class key, bound to `content_hash`.
pub fn scan_record(report: &ScanReport, content_hash: &str, now_ms: u64) -> PiiScanRecord {
    let counts: BTreeMap<String, u32> = report
        .counts
        .iter()
        .map(|(c, n)| (c.key().to_string(), u32::try_from(*n).unwrap_or(u32::MAX)))
        .collect();
    PiiScanRecord {
        scanner: SCANNER.to_string(),
        scanned_at_ms: now_ms,
        content_hash: content_hash.to_string(),
        pass: report.passed,
        counts,
    }
}

/// Scan one version and record the result in the store. Returns the report (row references and
/// counts, never the matched text).
pub fn scan_version(
    store: &DatasetStore<'_>,
    id: &str,
    version: u32,
    now_ms: u64,
) -> Result<ScanReport, String> {
    let (manifest, rows) = store.rows_of(id, version)?;
    let report = scan_manifest_rows(&rows);
    store.record_pii_scan(
        id,
        version,
        &scan_record(&report, &manifest.content_hash, now_ms),
    )?;
    Ok(report)
}

/// The verdict on one source's license.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum LicenseVerdict {
    /// A known, publishable license.
    Ok,
    /// The member's own content with no license file: the dataset license covers it.
    MemberOwned,
    /// Not known: publishing is refused until the source is licensed or dropped.
    Unknown,
    /// Known but not publishable (not on the list, or DatasetRegistry refuses it).
    NotPublishable,
}

/// One source's license record and verdict.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceLicense {
    pub source: String,
    pub kind: SourceKind,
    pub locator: String,
    pub license: Option<String>,
    pub basis: LicenseBasis,
    pub evidence: Option<String>,
    pub verdict: LicenseVerdict,
    pub reason: Option<String>,
}

/// A version's license report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LicenseReport {
    pub dataset: String,
    pub version: u32,
    pub sources: Vec<SourceLicense>,
    /// True when every source is `ok` or `member-owned`.
    pub ok: bool,
    /// The ShareAlike licenses among the sources (the dataset license must then be that one).
    pub share_alike: Vec<String>,
}

fn license_fit(id: &str) -> Result<(), String> {
    registry_license_ok(id)?;
    if !PUBLISHABLE_LICENSES.contains(&id) {
        return Err(format!(
            "{id} is not on the list of licenses a dataset can be published under"
        ));
    }
    Ok(())
}

/// Check every source's license in `m`.
pub fn license_check(m: &Manifest) -> LicenseReport {
    let mut sa: Vec<String> = Vec::new();
    let sources: Vec<SourceLicense> = m
        .sources
        .iter()
        .map(|s| {
            let own = matches!(s.kind, SourceKind::Folder | SourceKind::Conversations);
            let (verdict, reason) = match (&s.license.id, s.license.basis) {
                (Some(id), basis) if basis != LicenseBasis::Unknown => match license_fit(id) {
                    Ok(()) => {
                        if share_alike(id) && !sa.contains(id) {
                            sa.push(id.clone());
                        }
                        (LicenseVerdict::Ok, None)
                    }
                    Err(e) => (LicenseVerdict::NotPublishable, Some(e)),
                },
                _ if own => (LicenseVerdict::MemberOwned, None),
                _ => (
                    LicenseVerdict::Unknown,
                    Some("no license is known for this source".to_string()),
                ),
            };
            SourceLicense {
                source: s.id.clone(),
                kind: s.kind,
                locator: s.locator.clone(),
                license: s.license.id.clone(),
                basis: s.license.basis,
                evidence: s.license.evidence.clone(),
                verdict,
                reason,
            }
        })
        .collect();
    LicenseReport {
        dataset: m.dataset.clone(),
        version: m.version,
        ok: sources
            .iter()
            .all(|s| matches!(s.verdict, LicenseVerdict::Ok | LicenseVerdict::MemberOwned)),
        sources,
        share_alike: sa,
    }
}

/// What a publish asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishRequest<'a> {
    pub license: &'a str,
    pub privacy: PrivacyLabel,
}

/// The publish gate's verdict.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishCheck {
    pub allowed: bool,
    /// Every reason it is refused, in plain words.
    pub refusals: Vec<String>,
    pub licenses: LicenseReport,
}

/// The publish gate. `scan` is the recorded scan of this version, if any.
pub fn publish_gate(
    m: &Manifest,
    scan: Option<&PiiScanRecord>,
    req: &PublishRequest<'_>,
) -> PublishCheck {
    let mut refusals = Vec::new();
    if req.privacy == PrivacyLabel::Private {
        refusals.push(
            "a Private dataset stays on this device; choose who may see it to publish".into(),
        );
    }
    if m.contains_personal_data && req.privacy == PrivacyLabel::Public {
        refusals.push("a dataset labelled Contains personal data can never be Public".into());
    }
    match scan {
        None => refusals.push("this version has not been scanned for private information".into()),
        Some(s) if s.content_hash != m.content_hash => {
            refusals.push("the scan is of other rows than this version's; scan it again".into())
        }
        Some(s) if !s.pass && !m.contains_personal_data => {
            let found: Vec<String> = s
                .counts
                .iter()
                .filter(|(_, n)| **n > 0)
                .map(|(k, n)| format!("{n} {k}"))
                .collect();
            refusals.push(format!(
                "the private-information scan found {}; redact them, or label the dataset Contains personal data (never Public)",
                found.join(", ")
            ));
        }
        Some(_) => {}
    }
    if let Err(e) = license_fit(req.license) {
        refusals.push(format!("the dataset license is refused: {e}"));
    }
    let licenses = license_check(m);
    for s in licenses
        .sources
        .iter()
        .filter(|s| !matches!(s.verdict, LicenseVerdict::Ok | LicenseVerdict::MemberOwned))
    {
        refusals.push(format!(
            "source {} ({}): {}",
            s.source,
            s.locator,
            s.reason.clone().unwrap_or_else(|| "not publishable".into())
        ));
    }
    for sa in &licenses.share_alike {
        if sa != req.license {
            refusals.push(format!(
                "a source is under {sa}, so the dataset must be published under {sa} too"
            ));
        }
    }
    PublishCheck {
        allowed: refusals.is_empty(),
        refusals,
        licenses,
    }
}

/// The scan recorded for a version (the store returns it only when it is of that version's rows).
pub fn recorded_scan(
    store: &DatasetStore<'_>,
    id: &str,
    version: u32,
) -> Result<Option<PiiScanRecord>, String> {
    let list = store.list()?;
    let d = list
        .datasets
        .into_iter()
        .find(|d| d.id == id)
        .ok_or_else(|| format!("no dataset {id}"))?;
    let v = d
        .versions
        .into_iter()
        .find(|v| v.version == version)
        .ok_or_else(|| format!("{id} has no version {version}"))?;
    Ok(v.pii_scan)
}

// ------------------------------------------------------------------------------ commands

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn with_store<T: Send + 'static>(
    app: tauri::AppHandle,
    f: impl FnOnce(&DatasetStore<'_>) -> Result<T, String> + Send + 'static,
) -> impl std::future::Future<Output = Result<T, String>> {
    crate::blocking::off_main(move || {
        let keyring = crate::custody::OsKeyring::legacy();
        let store = DatasetStore::new(crate::fl_datasets::datasets_dir(&app)?, &keyring);
        f(&store)
    })
}

/// **fl_dataset_pii_scan**: scan a version on this device and record the result (the publish
/// gate reads it). Data source: the dataset store.
#[tauri::command]
pub async fn fl_dataset_pii_scan(
    app: tauri::AppHandle,
    dataset: String,
    version: u32,
) -> Result<ScanReport, String> {
    with_store(app, move |s| scan_version(s, &dataset, version, now_ms())).await
}

/// **fl_dataset_license_check**: every source's license verdict for a version.
#[tauri::command]
pub async fn fl_dataset_license_check(
    app: tauri::AppHandle,
    dataset: String,
    version: u32,
) -> Result<LicenseReport, String> {
    with_store(app, move |s| {
        let (m, _) = s.rows_of(&dataset, version)?;
        Ok(license_check(&m))
    })
    .await
}

/// **fl_dataset_publish_check**: would a publish of this version, under `license` and `privacy`,
/// pass the gate? Uses the version's recorded scan (run `fl_dataset_pii_scan` first).
#[tauri::command]
pub async fn fl_dataset_publish_check(
    app: tauri::AppHandle,
    dataset: String,
    version: u32,
    license: String,
    privacy: PrivacyLabel,
) -> Result<PublishCheck, String> {
    with_store(app, move |s| {
        let (m, _) = s.rows_of(&dataset, version)?;
        let scan = recorded_scan(s, &dataset, version)?;
        Ok(publish_gate(
            &m,
            scan.as_ref(),
            &PublishRequest {
                license: &license,
                privacy,
            },
        ))
    })
    .await
}

#[cfg(test)]
mod tests {
    include!("fl_dataset_gate_tests.rs");
}
