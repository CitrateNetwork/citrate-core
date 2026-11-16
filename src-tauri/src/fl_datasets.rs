//! citrate-core 0.5.4 (WP-H6): **the member's local dataset store** for federated learning.
//!
//! Spec: Hermes × FL gap analysis §5.4 and §3.1 (owner decisions 2026-11-16); v0.6.0 planset
//! `02_ARCHITECTURE.md` §4 (data pipeline) and `06_CONTRACTS.md` §5 (DatasetRegistry). This store
//! is the one place a dataset is written. Hermes reaches it only through core's `fl_dataset_*`
//! tools, each write behind a core-rendered HIC-1 card, and the compliant collector (WP-H8) adds
//! collected items only through [`DatasetStore::append`]. The agent's file tools cannot reach the
//! store: it lives in core's app-data folder, which the agent's default-deny list covers
//! (`agent-guard` `PREFIX_RULES`: `ai.citrate.core`) and which no folder grant may include
//! ([`crate::agent_grants`]). [`dataset_root_is_agent_denied`] is the tripwire.
//!
//! # Layout (owner-only, sealed at rest)
//!
//! ```text
//! <app data>/datasets/<id>/
//!   dataset.sealed          the dataset record (name, data class, created)
//!   v<N>.manifest.sealed    version N's manifest: the exact JSON bytes manifestHash covers
//!   v<N>.rows.sealed        version N's rows, one JSON object per line
//!   v<N>.scan.sealed        the PII scan of version N's rows, once one has run (WP-H7)
//!   consent.sealed          the member's consent records for this dataset
//! ```
//!
//! Every file is sealed with the Hermes history envelope (AES-256-GCM DEK per file, wrapped under
//! an ML-KEM-768 KEK; [`crate::hermes_history::seal_in`]) under its own magic, HKDF info and
//! keyring account ([`KEY_ACCOUNT`]). The AAD binds each file to `<id>/<name>`, so a file copied
//! to another dataset or version does not open. The key is random, kept in the OS keyring, never
//! in the recovery kit, and never reaches the webview.
//!
//! # Manifest and hash chain
//!
//! A version's manifest ([`Manifest`]) is serialized once, compactly, with a fixed field order,
//! and those exact bytes are stored. `manifestHash = keccak256(manifest bytes)`: the value
//! DatasetRegistry's `Version.manifestHash` commits to (planset 06 §5: "sources, robots, license,
//! PII scan, rows, content hash"). On read the bytes must re-serialize to themselves, so a
//! manifest has one form only. Each version names the previous version's manifestHash in `prev`
//! (zero for v1): a linear local chain. `contentHash` is the SHA-256 of the rows file.
//!
//! Versions are never edited. Cleaning, collecting, a consent withdrawal and a publish each write
//! version N+1 and keep N.
//!
//! The registry's own `prev` is the manifestHash of the last *published* version, which is the
//! local `prev` only when every version was published. A publish version records the registry
//! value in `publishedPrev`, so the manifest states both.
//!
//! # Consent (gap §3.1 rule 7)
//!
//! Consent is per dataset and per source class: every source record names the consent record it
//! was added under (`consentRef`). A withdrawal is final for that record; core writes a new
//! version without that consent's sources at once (no card: it lowers authority). Earlier local
//! versions stay until the member deletes the dataset; a published CID cannot be recalled.
//!
//! Rule 3: nothing here signs. Rule 7: every command's data source is this folder, opened with
//! the dataset key in the OS keyring, plus the member-granted folder or the verified Hermes
//! conversations a create reads. No network.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use sha3::Keccak256;
use zeroize::Zeroizing;

use crate::custody::Keyring;
use crate::hermes_history::{file_gen_in, open_in, seal_in, KeyGen, KeySet, SealDomain};

// ------------------------------------------------------------------------------ constants

/// The keyring account (legacy `ai.citrate.core` service) holding the dataset key.
pub const KEY_ACCOUNT: &str = "fl-datasets-key-v1";
/// The app-data folder holding every dataset.
pub const DATASETS_DIR: &str = "datasets";
/// The manifest schema id; a manifest with any other value is refused.
pub const MANIFEST_SCHEMA: &str = "citrate.dataset.manifest.v1";
/// The collector version core's own ingest writes into a source record.
pub const CORE_INGEST_VERSION: &str = "citrate-core-dataset-store/1";
/// `prev` of version 1.
pub const ZERO_HASH: &str = "0x0000000000000000000000000000000000000000000000000000000000000000";

/// Most datasets on one device.
pub const MAX_DATASETS: usize = 200;
/// Most versions of one dataset.
pub const MAX_VERSIONS: u32 = 1000;
/// Most rows in one version.
pub const MAX_ROWS: usize = 200_000;
/// The largest row (its JSON line), bytes.
pub const MAX_ROW_BYTES: usize = 256 * 1024;
/// The largest rows file of one version, bytes.
pub const MAX_ROWS_BYTES: usize = 64 * 1024 * 1024;
/// Most source records in one version.
pub const MAX_SOURCES: usize = 10_000;
/// Most files one folder ingest reads.
pub const MAX_FOLDER_FILES: usize = 2000;
/// The largest file a folder ingest reads, bytes.
pub const MAX_FOLDER_FILE_BYTES: u64 = 8 * 1024 * 1024;
/// How deep a folder ingest walks below the granted folder.
pub const MAX_FOLDER_DEPTH: usize = 8;
/// Most rows one sample returns.
pub const MAX_SAMPLE_ROWS: u32 = 20;
/// Longest text of one sampled row or diff side, characters.
pub const MAX_SAMPLE_CHARS: usize = 1000;
/// Most clean operations in one request.
pub const MAX_CLEAN_OPS: usize = 16;
/// Most changed rows a clean preview lists.
pub const MAX_DIFF_ROWS: usize = 20;
/// Longest free text (name, consent statement, license evidence), characters.
pub const MAX_TEXT: usize = 512;

/// The extensions a folder ingest reads. Everything else is listed as skipped.
pub const FOLDER_EXTENSIONS: &[&str] = &["txt", "md", "markdown", "rst", "jsonl", "json"];

const DOMAIN: SealDomain = SealDomain {
    magic: b"CITRDSS\0",
    hkdf_info: b"citrate.core.fl-datasets.kek.v1",
};
const EXT: &str = "sealed";
const DATASET_STEM: &str = "dataset";
const CONSENT_STEM: &str = "consent";
const PUBLISHED_STEM: &str = "published";

// ------------------------------------------------------------------------------ the schema

/// Where a source's content came from (gap §3.1 rule 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SourceKind {
    /// A file in a folder the member granted Hermes, read in place.
    Folder,
    /// The member's verified Hermes conversations (the training switch must be on).
    Conversations,
    /// A public web page, fetched by the compliant collector only.
    Web,
    /// A source with an explicit license record (a dataset card, an SPDX tag), via the collector.
    Licensed,
    /// Output of an agent or MCP server the member connected; untrusted by definition.
    ConnectedAgent,
}

/// What robots.txt said about this source (gap §3.1 rule 2). A disallowed source is never stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RobotsVerdict {
    /// robots.txt allows the declared user agent this path.
    Allowed,
    /// The source is not a web fetch (local files, conversations).
    NotApplicable,
}

/// The host's terms-of-service class (gap §3.1 rule 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TosClass {
    /// On Citrate's curated allowlist.
    Allowlisted,
    /// Not on a list; the member approved this host on a card.
    MemberApproved,
    /// Not a web fetch.
    NotApplicable,
}

/// How a source's license is known.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LicenseBasis {
    /// Read from the source (an SPDX tag, a LICENSE file, a dataset card).
    Found,
    /// Typed by the member, with the attestation text in `evidence`.
    MemberAttested,
    /// Not known. Publishing is refused while any source is unknown.
    Unknown,
}

/// One source's license.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LicenseRecord {
    /// SPDX id or `LicenseRef-…`; `None` when unknown.
    pub id: Option<String>,
    pub basis: LicenseBasis,
    /// Where it was found (file name, URL) or the member's attestation text.
    pub evidence: Option<String>,
}

impl LicenseRecord {
    pub fn unknown() -> Self {
        LicenseRecord {
            id: None,
            basis: LicenseBasis::Unknown,
            evidence: None,
        }
    }
}

/// The provenance record of one source (gap §3.1 rule 5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceRecord {
    /// `s-<n>`, unique within the dataset across all versions.
    pub id: String,
    pub kind: SourceKind,
    /// The URL, or `<granted folder name>/<path inside it>`, or `conversations`. Never an
    /// absolute local path (it would carry the member's user name into a published manifest).
    pub locator: String,
    pub fetched_at_ms: u64,
    /// `0x` + SHA-256 of the source bytes as read.
    pub content_sha256: String,
    pub license: LicenseRecord,
    pub robots: RobotsVerdict,
    pub tos_class: TosClass,
    /// The consent record (`c-<n>`) this source was added under.
    pub consent_ref: String,
    /// What added it: [`CORE_INGEST_VERSION`] or the collector's version.
    pub collector_version: String,
    /// Content other people wrote (web pages, connected agents). Taints the session that reads it.
    pub untrusted: bool,
    /// Rows this source contributed to the version.
    pub rows: u32,
}

/// The privacy label (planset 02 §4). Local datasets are `Private`; the others are chosen at
/// publish and map onto DatasetRegistry's `Privacy`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PrivacyLabel {
    Private,
    Public,
    Members,
    Group,
    IndexOnly,
}

/// A PII scan of one version's rows (WP-H7 runs the scanner; this store keeps its record). The
/// scanner decides, never the model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PiiScanRecord {
    /// Scanner name and version.
    pub scanner: String,
    pub scanned_at_ms: u64,
    /// The `contentHash` of the rows scanned; must equal the version's.
    pub content_hash: String,
    /// `true` when no PII was found.
    pub pass: bool,
    /// Findings per class, keyed as the WP-H7 scanner names them (email, phone, name,
    /// street-address, government-id, card, secret, ...).
    pub counts: BTreeMap<String, u32>,
}

/// What produced a version from its `prev`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DerivedOp {
    /// `create`, `append`, `dedupe`, `drop-rows`, `drop-source`, `redact`, `redact-secrets`,
    /// `normalize`, `consent-withdrawn` or `publish`.
    pub op: String,
    pub detail: String,
    pub removed: u32,
    pub changed: u32,
    pub added: u32,
}

/// One version's manifest: the exact bytes `manifestHash` commits to. Field order is fixed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Manifest {
    pub schema: String,
    /// The local dataset id (`ds-` + 16 hex).
    pub dataset: String,
    pub name: String,
    pub data_class: String,
    pub version: u32,
    /// `0x` + keccak256 of version N-1's manifest bytes; [`ZERO_HASH`] for v1.
    pub prev: String,
    pub created_at_ms: u64,
    pub privacy_label: PrivacyLabel,
    /// The member says the rows contain personal data. Such a dataset can never be Public.
    pub contains_personal_data: bool,
    /// The dataset's license, set by a publish version; `None` before.
    pub license: Option<String>,
    pub sources: Vec<SourceRecord>,
    pub rows: u32,
    /// `0x` + SHA-256 of the rows file.
    pub content_hash: String,
    /// The PII scan a publish version carries; `None` before.
    pub pii_scan: Option<PiiScanRecord>,
    pub derived_by: Vec<DerivedOp>,
    /// Consent records in force for this version's sources.
    pub consent: Vec<String>,
    /// Any source is untrusted.
    pub untrusted: bool,
    /// On a publish version: the registry `prev` (last published manifestHash, or zero).
    pub published_prev: Option<String>,
    /// The app version that wrote this manifest.
    pub written_by: String,
}

/// One row: the source it came from and its data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Row {
    pub source: String,
    pub data: serde_json::Value,
}

/// The dataset record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DatasetRecord {
    pub id: String,
    pub name: String,
    pub data_class: String,
    pub created_at_ms: u64,
}

/// One consent record (gap §3.1 rule 7).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConsentRecord {
    /// `c-<n>`.
    pub id: String,
    pub source_kind: SourceKind,
    /// What it covers: the granted folder's name, a host, or `conversations`.
    pub scope: String,
    /// The exact words the member agreed to on the card.
    pub statement: String,
    pub granted_at_ms: u64,
    pub withdrawn_at_ms: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ConsentDoc {
    next: u64,
    records: Vec<ConsentRecord>,
}

// ------------------------------------------------------------------------------ views

/// One version as listed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionView {
    pub dataset: String,
    pub version: u32,
    pub manifest_hash: String,
    pub prev: String,
    pub created_at_ms: u64,
    pub rows: u32,
    pub sources: u32,
    pub content_hash: String,
    pub privacy_label: PrivacyLabel,
    pub contains_personal_data: bool,
    pub license: Option<String>,
    /// Sources whose license is unknown (publishing is refused while this is not zero).
    pub unknown_license_sources: u32,
    /// The PII scan recorded for this version's rows: `None` = not scanned.
    pub pii_scan: Option<PiiScanRecord>,
    pub derived_by: Vec<DerivedOp>,
    pub untrusted: bool,
}

/// One dataset as listed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DatasetView {
    pub id: String,
    pub name: String,
    pub data_class: String,
    pub created_at_ms: u64,
    /// Oldest first.
    pub versions: Vec<VersionView>,
    /// `true` when every version opened and the hash chain links (v1 prev zero, each prev the
    /// previous manifestHash, numbers contiguous).
    pub chain_ok: bool,
    /// Why the chain is not ok.
    pub chain_problem: Option<String>,
}

/// Every dataset on this device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DatasetList {
    pub datasets: Vec<DatasetView>,
    /// Dataset folders that did not open (never shown, never overwritten).
    pub unreadable: Vec<String>,
}

/// A version's exact manifest and its hash.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestView {
    pub manifest_hash: String,
    /// The exact bytes the hash covers, as UTF-8 text.
    pub manifest_json: String,
    pub manifest: Manifest,
}

/// A sampled row (redacted, truncated).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SampleRow {
    pub index: u32,
    pub source: String,
    pub text: String,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Sample {
    pub dataset: String,
    pub version: u32,
    pub total_rows: u32,
    pub rows: Vec<SampleRow>,
    /// Any source of this version is untrusted: the caller fences the text.
    pub untrusted: bool,
}

/// One cleaning step (gap §5.4 `fl_dataset_clean_propose`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "kebab-case", deny_unknown_fields)]
pub enum CleanOp {
    /// Drop rows whose data repeats an earlier row exactly.
    Dedupe,
    /// Drop rows by index in the base version.
    DropRows { rows: Vec<u32> },
    /// Drop every row of one source.
    DropSource { source: String },
    /// Replace every occurrence of `find` in text values with `replace`.
    Redact { find: String, replace: String },
    /// Replace secret shapes (keys, tokens, seed phrases) in text values.
    RedactSecrets,
    /// Line endings to `\n`, trailing spaces removed, text trimmed.
    Normalize,
}

/// One row a clean changes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffRow {
    pub index: u32,
    /// `removed` or `changed`.
    pub kind: String,
    pub before: String,
    pub after: Option<String>,
}

/// What a clean would do. Nothing is written until [`DatasetStore::clean_apply`] gets the same
/// ops and `resultContentHash`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanPreview {
    pub dataset: String,
    pub base_version: u32,
    pub base_manifest_hash: String,
    pub ops: Vec<CleanOp>,
    pub removed: u32,
    pub changed: u32,
    pub kept: u32,
    pub result_content_hash: String,
    pub diff: Vec<DiffRow>,
    pub untrusted: bool,
}

/// Where a new dataset's first version comes from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum CreateSource {
    /// A folder the member granted Hermes read access to (absolute path).
    Folder { path: String },
    /// The member's verified Hermes conversations, newest first, at most `max`.
    Conversations { max: u32 },
}

/// A new dataset.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateRequest {
    pub name: String,
    pub data_class: String,
    pub source: CreateSource,
    /// The consent words the member agreed to on the card.
    pub consent_statement: String,
}

/// What a create read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateOutcome {
    pub version: VersionView,
    /// Files or lines left out, with the reason.
    pub skipped: Vec<String>,
}

/// One item the compliant collector (WP-H8) adds: its provenance and its rows. The store assigns
/// the source id, the consent reference and the row count.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CollectedItem {
    pub kind: SourceKind,
    pub locator: String,
    pub fetched_at_ms: u64,
    /// `0x` + SHA-256 of the fetched bytes.
    pub content_sha256: String,
    pub license: LicenseRecord,
    pub robots: RobotsVerdict,
    pub tos_class: TosClass,
    pub collector_version: String,
    pub rows: Vec<serde_json::Value>,
}

/// The consent an append runs under: an existing record of this dataset, or a new one the member
/// agreed to on the collect plan card.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ConsentInput {
    Existing {
        id: String,
    },
    New {
        source_kind: SourceKind,
        scope: String,
        statement: String,
    },
}

/// A withdrawal's effect.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConsentWithdrawn {
    pub consent: ConsentRecord,
    /// The version core wrote without that consent's sources, if the latest version had any.
    pub new_version: Option<VersionView>,
    /// Always true: earlier local versions keep the rows until the dataset is deleted, and a
    /// published CID cannot be recalled.
    pub earlier_versions_kept: bool,
}

/// What publishing would do, and every reason core refuses it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishCheck {
    pub dataset: String,
    pub name: String,
    pub version: u32,
    pub content_hash: String,
    pub rows: u32,
    pub sources: u32,
    pub license: String,
    pub privacy: PrivacyLabel,
    /// The DatasetRegistry `prev` this publication would carry (last published manifestHash).
    pub registry_prev: String,
    pub bundle_bytes: u64,
    pub untrusted: bool,
    /// Empty = core may publish.
    pub refusals: Vec<String>,
}

/// One publication of a dataset version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Publication {
    pub version: u32,
    pub manifest_hash: String,
    pub content_hash: String,
    /// The IPFS CID of the bundle (plaintext: Public only in this build).
    pub cid: String,
    pub license: String,
    pub privacy: PrivacyLabel,
    pub published_at_ms: u64,
    /// The DatasetRegistry `prev` for this version's record.
    pub registry_prev: String,
}

/// A finished publish.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Published {
    pub version: VersionView,
    pub name: String,
    pub publication: Publication,
    pub bundle_bytes: u64,
}

/// Where a publish bundle goes: IPFS, added and pinned on this node.
pub trait PublishSink {
    /// Add `bytes` as the file `name` and pin it on this node. Returns the CID.
    fn add_and_pin(&self, name: &str, bytes: &[u8]) -> Result<String, String>;
}

/// The bundled kubo daemon, through core's storage seam (`storage.rs`).
pub struct IpfsSink(pub crate::storage::StorageManager);

impl PublishSink for IpfsSink {
    fn add_and_pin(&self, name: &str, bytes: &[u8]) -> Result<String, String> {
        let out = self.0.add_bytes(name, bytes).map_err(|e| e.to_string())?;
        self.0.pin_local(&out.cid).map_err(|e| e.to_string())?;
        Ok(out.cid)
    }
}

fn privacy_word(p: PrivacyLabel) -> &'static str {
    match p {
        PrivacyLabel::Private => "private",
        PrivacyLabel::Public => "public",
        PrivacyLabel::Members => "members",
        PrivacyLabel::Group => "group",
        PrivacyLabel::IndexOnly => "index-only",
    }
}

// ------------------------------------------------------------------------------ helpers

fn keccak_hex(bytes: &[u8]) -> String {
    let mut h = Keccak256::new();
    h.update(bytes);
    format!("0x{}", hex::encode(h.finalize()))
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("0x{}", hex::encode(Sha256::digest(bytes)))
}

/// `0x` + 64 lowercase hex.
pub fn is_hash(s: &str) -> bool {
    s.len() == 66
        && s.starts_with("0x")
        && s[2..]
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// A dataset id this store mints: `ds-` + 16 lowercase hex.
pub fn valid_dataset_id(id: &str) -> bool {
    id.len() == 19
        && id.starts_with("ds-")
        && id[3..]
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// A data class: 1 to 48 of `[a-z0-9-]`, not starting or ending with `-`.
pub fn valid_data_class(c: &str) -> bool {
    !c.is_empty()
        && c.len() <= 48
        && !c.starts_with('-')
        && !c.ends_with('-')
        && c.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn clean_text(what: &str, s: &str) -> Result<String, String> {
    let t = s.trim();
    if t.is_empty() {
        return Err(format!("the {what} is empty"));
    }
    if t.chars().count() > MAX_TEXT {
        return Err(format!("the {what} is longer than {MAX_TEXT} characters"));
    }
    if t.chars().any(|c| c.is_control() && c != '\n') {
        return Err(format!("the {what} has control characters"));
    }
    Ok(t.to_string())
}

/// DatasetRegistry's license rule (`_checkLicense`): non-empty, at most 256 bytes, no leading or
/// trailing ASCII whitespace or control byte, and neither "unknown" nor "NOASSERTION" in any
/// letter case. Core refuses these before the contract does.
pub fn registry_license_ok(license: &str) -> Result<(), String> {
    let b = license.as_bytes();
    if b.is_empty() {
        return Err("the license is empty".into());
    }
    if b.len() > 256 {
        return Err("the license is longer than 256 bytes".into());
    }
    let edge = |c: u8| c <= 0x20 || c == 0x7f;
    if edge(b[0]) || edge(b[b.len() - 1]) {
        return Err("the license starts or ends with a space or control character".into());
    }
    let lower = license.to_ascii_lowercase();
    if lower == "unknown" || lower == "noassertion" {
        return Err(format!("\"{license}\" is not a license"));
    }
    Ok(())
}

/// Every string inside `v`, changed by `f`. Returns whether anything changed.
fn map_strings(v: &mut serde_json::Value, f: &dyn Fn(&str) -> String) -> bool {
    match v {
        serde_json::Value::String(s) => {
            let n = f(s);
            if n != *s {
                *s = n;
                true
            } else {
                false
            }
        }
        serde_json::Value::Array(a) => {
            let mut any = false;
            for x in a {
                any |= map_strings(x, f);
            }
            any
        }
        serde_json::Value::Object(o) => {
            let mut any = false;
            for (_, x) in o.iter_mut() {
                any |= map_strings(x, f);
            }
            any
        }
        _ => false,
    }
}

/// Email addresses masked (a display courtesy for samples; the PII scanner of WP-H7 decides).
fn mask_emails(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for (i, word) in s.split(' ').enumerate() {
        if i > 0 {
            out.push(' ');
        }
        let at = word.find('@');
        let looks = at.is_some_and(|a| {
            a > 0 && word[a + 1..].contains('.') && !word[a + 1..].starts_with('.')
        });
        if looks {
            out.push_str("<email>");
        } else {
            out.push_str(word);
        }
    }
    out
}

/// A row's text for display: secrets redacted, emails masked, truncated.
fn display_text(data: &serde_json::Value) -> (String, bool) {
    let raw = match data {
        serde_json::Value::String(s) => s.clone(),
        // A text file's row is `{"text": …}`: show the text itself.
        serde_json::Value::Object(o)
            if o.len() == 1 && o.get("text").is_some_and(|t| t.is_string()) =>
        {
            o.get("text")
                .and_then(|t| t.as_str())
                .unwrap_or_default()
                .to_string()
        }
        other => other.to_string(),
    };
    let red = mask_emails(&crate::secret_shapes::redact(&raw));
    if red.chars().count() > MAX_SAMPLE_CHARS {
        (red.chars().take(MAX_SAMPLE_CHARS).collect(), true)
    } else {
        (red, false)
    }
}

fn normalize_text(s: &str) -> String {
    let unified = s.replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<&str> = unified.split('\n').map(|l| l.trim_end()).collect();
    lines.join("\n").trim().to_string()
}

fn rows_bytes(rows: &[Row]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    for r in rows {
        let line = serde_json::to_string(r).map_err(|e| e.to_string())?;
        if line.len() > MAX_ROW_BYTES {
            return Err(format!("a row is larger than {MAX_ROW_BYTES} bytes"));
        }
        out.extend_from_slice(line.as_bytes());
        out.push(b'\n');
        if out.len() > MAX_ROWS_BYTES {
            return Err(format!("the rows are larger than {MAX_ROWS_BYTES} bytes"));
        }
    }
    Ok(out)
}

fn parse_rows(bytes: &[u8]) -> Option<Vec<Row>> {
    let text = std::str::from_utf8(bytes).ok()?;
    let mut rows = Vec::new();
    for line in text.split_terminator('\n') {
        rows.push(serde_json::from_str::<Row>(line).ok()?);
    }
    Some(rows)
}

fn new_dataset_id() -> String {
    let mut b = [0u8; 8];
    rand::rngs::OsRng.fill_bytes(&mut b);
    format!("ds-{}", hex::encode(b))
}

fn app_version() -> String {
    format!("citrate-core/{}", env!("CARGO_PKG_VERSION"))
}

/// The license a folder states at its root (`LICENSE`, `LICENSE.md`, `LICENSE.txt`, `COPYING`):
/// an `SPDX-License-Identifier:` line, or a few well-known license headings. `None` = unknown.
pub fn folder_license(root: &Path) -> Option<(String, String)> {
    for name in ["LICENSE", "LICENSE.md", "LICENSE.txt", "COPYING"] {
        let p = root.join(name);
        let Ok(meta) = std::fs::symlink_metadata(&p) else {
            continue;
        };
        if !meta.is_file() || meta.len() > 256 * 1024 {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&p) else {
            continue;
        };
        if let Some(id) = spdx_tag(&text) {
            return Some((id, name.to_string()));
        }
        let head: String = text
            .chars()
            .take(400)
            .collect::<String>()
            .to_ascii_lowercase();
        let known: &[(&str, &str)] = &[
            ("mit license", "MIT"),
            ("apache license", "Apache-2.0"),
            ("creative commons attribution 4.0", "CC-BY-4.0"),
            (
                "creative commons attribution-sharealike 4.0",
                "CC-BY-SA-4.0",
            ),
            ("cc0 1.0 universal", "CC0-1.0"),
            ("the unlicense", "Unlicense"),
            ("bsd 3-clause", "BSD-3-Clause"),
            ("bsd 2-clause", "BSD-2-Clause"),
        ];
        let words = head.split_whitespace().collect::<Vec<_>>().join(" ");
        for (needle, id) in known {
            if *id == "Apache-2.0" {
                if words.contains(needle) && words.contains("version 2.0") {
                    return Some((id.to_string(), name.to_string()));
                }
                continue;
            }
            if words.contains(needle) {
                return Some((id.to_string(), name.to_string()));
            }
        }
    }
    None
}

/// The value of the first `SPDX-License-Identifier:` line in `text` (first 50 lines).
pub fn spdx_tag(text: &str) -> Option<String> {
    for line in text.lines().take(50) {
        if let Some(i) = line.find("SPDX-License-Identifier:") {
            let v = line[i + "SPDX-License-Identifier:".len()..]
                .trim()
                .trim_end_matches("*/")
                .trim_end_matches("-->")
                .trim();
            if registry_license_ok(v).is_ok() && v.len() <= 128 {
                return Some(v.to_string());
            }
        }
    }
    None
}

/// Is `path` inside an active folder grant for reading? (Folder grants only: the 24 h read-only
/// full-access window does not count as granting a folder for a dataset.)
pub fn folder_read_granted(
    st: &crate::agent_grants::GrantState,
    path: &Path,
    now_secs: u64,
) -> bool {
    use crate::agent_grants::{Access, GrantKind};
    st.grants.iter().any(|g| {
        g.kind == GrantKind::Folder
            && g.access == Access::Read
            && g.revoked_at.is_none()
            && now_secs >= g.granted_at
            && !matches!(g.expires_at, Some(t) if now_secs >= t)
            && path.starts_with(Path::new(&g.root))
    })
}

/// The dataset root is inside core's app-data folder, which the agent's default-deny list covers
/// (`agent-guard` denies any path component starting `ai.citrate.core`). The production root is
/// `<app data>/datasets` with the Tauri identifier `ai.citrate.core`.
pub fn dataset_root_is_agent_denied(root: &Path) -> bool {
    root.components().any(|c| match c {
        std::path::Component::Normal(n) => n
            .to_string_lossy()
            .to_ascii_lowercase()
            .starts_with("ai.citrate.core"),
        _ => false,
    })
}

// ------------------------------------------------------------------------------ the store

/// Serializes every write in this process.
static STORE_LOCK: Mutex<()> = Mutex::new(());

/// The member's datasets under one folder, sealed under the key in `keyring`.
pub struct DatasetStore<'a> {
    dir: PathBuf,
    keyring: &'a dyn Keyring,
}

struct Version {
    manifest: Manifest,
    manifest_bytes: Vec<u8>,
    hash: String,
}

impl<'a> DatasetStore<'a> {
    pub fn new(dir: impl Into<PathBuf>, keyring: &'a dyn Keyring) -> Self {
        Self {
            dir: dir.into(),
            keyring,
        }
    }

    fn unreachable() -> String {
        "the keychain is not reachable; your datasets stay closed".to_string()
    }

    fn ds_dir(&self, id: &str) -> Result<PathBuf, String> {
        if !valid_dataset_id(id) {
            return Err(format!("{id:?} is not a dataset id"));
        }
        Ok(self.dir.join(id))
    }

    fn dataset_ids(&self) -> Result<Vec<String>, String> {
        let rd = match std::fs::read_dir(&self.dir) {
            Ok(r) => r,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(format!("the datasets folder cannot be read: {}", e.kind())),
        };
        let mut out: Vec<String> = rd
            .filter_map(Result::ok)
            .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
            .filter_map(|e| e.file_name().to_str().map(str::to_string))
            .filter(|n| valid_dataset_id(n))
            .collect();
        out.sort();
        Ok(out)
    }

    /// The dataset key. Minted only when `mint` and no dataset exists.
    fn keys(&self, mint: bool) -> Result<Option<KeySet>, String> {
        let stored = self
            .keyring
            .get(KEY_ACCOUNT)
            .map_err(|_| Self::unreachable())?;
        match stored.map(Zeroizing::new) {
            Some(raw) => KeySet::decode(&raw).map(Some).map_err(|_| {
                "the dataset key in the keychain is malformed; your datasets stay closed"
                    .to_string()
            }),
            None => {
                if !self.dataset_ids()?.is_empty() {
                    return Err("datasets exist but their key is not in the keychain; they cannot be opened".into());
                }
                if !mint {
                    return Ok(None);
                }
                let keys = KeySet {
                    cur: KeyGen::mint(1),
                    prev: None,
                };
                self.keyring
                    .set(KEY_ACCOUNT, &keys.encode())
                    .map_err(|_| Self::unreachable())?;
                Ok(Some(keys))
            }
        }
    }

    fn need_keys(&self) -> Result<KeySet, String> {
        self.keys(false)?
            .ok_or_else(|| "there are no datasets on this device".to_string())
    }

    fn read_file(
        &self,
        keys: &KeySet,
        id: &str,
        name: &str,
    ) -> Result<Option<Zeroizing<Vec<u8>>>, String> {
        let path = self.ds_dir(id)?.join(format!("{name}.{EXT}"));
        let meta = match std::fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(format!("a dataset file cannot be read: {}", e.kind())),
        };
        let damaged =
            || format!("{id}/{name} does not open (changed, moved, or sealed under another key)");
        if !meta.is_file() || meta.len() as usize > MAX_ROWS_BYTES * 2 {
            return Err(damaged());
        }
        let bytes = std::fs::read(&path)
            .map_err(|e| format!("a dataset file cannot be read: {}", e.kind()))?;
        if file_gen_in(&DOMAIN, &bytes).is_none() {
            return Err(damaged());
        }
        open_in(&DOMAIN, keys, &format!("{id}/{name}"), &bytes)
            .map(Some)
            .ok_or_else(damaged)
    }

    fn write_file(
        &self,
        keys: &KeySet,
        id: &str,
        name: &str,
        plain: &[u8],
        new_only: bool,
    ) -> Result<(), String> {
        let dir = self.ds_dir(id)?;
        create_private_dir(&self.dir)?;
        create_private_dir(&dir)?;
        let path = dir.join(format!("{name}.{EXT}"));
        if new_only && std::fs::symlink_metadata(&path).is_ok() {
            return Err(format!(
                "{id}/{name} already exists; versions are never overwritten"
            ));
        }
        let sealed = seal_in(&DOMAIN, &keys.cur, &format!("{id}/{name}"), plain)
            .map_err(|_| "the dataset could not be sealed".to_string())?;
        let staged = crate::atomic_file::stage_secret(&path, &sealed)
            .map_err(|e| format!("the dataset could not be written: {}", e.kind()))?;
        crate::atomic_file::commit_staged(&staged, &path)
            .map_err(|e| format!("the dataset could not be written: {}", e.kind()))
    }

    fn record(&self, keys: &KeySet, id: &str) -> Result<DatasetRecord, String> {
        let plain = self
            .read_file(keys, id, DATASET_STEM)?
            .ok_or_else(|| format!("no dataset {id}"))?;
        let rec: DatasetRecord =
            serde_json::from_slice(&plain).map_err(|_| format!("the record of {id} is damaged"))?;
        if rec.id != id {
            return Err(format!("the record of {id} names another dataset"));
        }
        Ok(rec)
    }

    fn version_numbers(&self, id: &str) -> Result<Vec<u32>, String> {
        let rd = std::fs::read_dir(self.ds_dir(id)?)
            .map_err(|e| format!("a dataset folder cannot be read: {}", e.kind()))?;
        let mut out: Vec<u32> = rd
            .filter_map(Result::ok)
            .filter_map(|e| e.file_name().to_str().map(str::to_string))
            .filter_map(|n| {
                n.strip_prefix('v')
                    .and_then(|r| r.strip_suffix(".manifest.sealed"))
                    .and_then(|v| v.parse::<u32>().ok())
                    .filter(|v| *v >= 1)
            })
            .collect();
        out.sort_unstable();
        Ok(out)
    }

    fn version(&self, keys: &KeySet, id: &str, v: u32) -> Result<Version, String> {
        let plain = self
            .read_file(keys, id, &format!("v{v}.manifest"))?
            .ok_or_else(|| format!("{id} has no version {v}"))?;
        let manifest: Manifest = serde_json::from_slice(&plain)
            .map_err(|_| format!("{id} v{v}: the manifest is damaged"))?;
        let again = serde_json::to_vec(&manifest).map_err(|e| e.to_string())?;
        if again != plain.as_slice() {
            return Err(format!(
                "{id} v{v}: the manifest is not in its one canonical form"
            ));
        }
        if manifest.schema != MANIFEST_SCHEMA || manifest.dataset != id || manifest.version != v {
            return Err(format!(
                "{id} v{v}: the manifest names another dataset, version or schema"
            ));
        }
        Ok(Version {
            hash: keccak_hex(&plain),
            manifest_bytes: plain.to_vec(),
            manifest,
        })
    }

    fn rows(&self, keys: &KeySet, id: &str, v: &Version) -> Result<Vec<Row>, String> {
        let plain = self
            .read_file(keys, id, &format!("v{}.rows", v.manifest.version))?
            .ok_or_else(|| format!("{id} v{}: the rows are missing", v.manifest.version))?;
        if sha256_hex(&plain) != v.manifest.content_hash {
            return Err(format!(
                "{id} v{}: the rows do not match the manifest's contentHash",
                v.manifest.version
            ));
        }
        parse_rows(&plain)
            .ok_or_else(|| format!("{id} v{}: the rows are damaged", v.manifest.version))
    }

    fn scan(&self, keys: &KeySet, id: &str, v: &Version) -> Result<Option<PiiScanRecord>, String> {
        let Some(plain) = self.read_file(keys, id, &format!("v{}.scan", v.manifest.version))?
        else {
            return Ok(None);
        };
        let s: PiiScanRecord = serde_json::from_slice(&plain).map_err(|_| {
            format!(
                "{id} v{}: the PII scan record is damaged",
                v.manifest.version
            )
        })?;
        Ok((s.content_hash == v.manifest.content_hash).then_some(s))
    }

    fn consent_doc(&self, keys: &KeySet, id: &str) -> Result<ConsentDoc, String> {
        match self.read_file(keys, id, CONSENT_STEM)? {
            None => Ok(ConsentDoc {
                next: 1,
                records: Vec::new(),
            }),
            Some(p) => serde_json::from_slice(&p)
                .map_err(|_| format!("the consent records of {id} are damaged")),
        }
    }

    fn save_consent(&self, keys: &KeySet, id: &str, doc: &ConsentDoc) -> Result<(), String> {
        let bytes = serde_json::to_vec(doc).map_err(|e| e.to_string())?;
        self.write_file(keys, id, CONSENT_STEM, &bytes, false)
    }

    fn view(&self, keys: &KeySet, id: &str, v: &Version) -> Result<VersionView, String> {
        let m = &v.manifest;
        Ok(VersionView {
            dataset: id.to_string(),
            version: m.version,
            manifest_hash: v.hash.clone(),
            prev: m.prev.clone(),
            created_at_ms: m.created_at_ms,
            rows: m.rows,
            sources: u32::try_from(m.sources.len()).unwrap_or(u32::MAX),
            content_hash: m.content_hash.clone(),
            privacy_label: m.privacy_label,
            contains_personal_data: m.contains_personal_data,
            license: m.license.clone(),
            unknown_license_sources: u32::try_from(
                m.sources.iter().filter(|s| s.license.id.is_none()).count(),
            )
            .unwrap_or(u32::MAX),
            pii_scan: match &m.pii_scan {
                Some(s) => Some(s.clone()),
                None => self.scan(keys, id, v)?,
            },
            derived_by: m.derived_by.clone(),
            untrusted: m.untrusted,
        })
    }

    fn latest(&self, keys: &KeySet, id: &str) -> Result<Version, String> {
        let n = self.version_numbers(id)?;
        let last = *n.last().ok_or_else(|| format!("{id} has no versions"))?;
        self.version(keys, id, last)
    }

    /// Write version `prev.version + 1` (or v1 when `prev` is `None`).
    #[allow(clippy::too_many_arguments)]
    fn write_version(
        &self,
        keys: &KeySet,
        rec: &DatasetRecord,
        prev: Option<&Version>,
        sources: Vec<SourceRecord>,
        rows: Vec<Row>,
        op: DerivedOp,
        consent: Vec<String>,
        now_ms: u64,
        publish: Option<(String, PrivacyLabel, PiiScanRecord, String)>,
    ) -> Result<Version, String> {
        let version = prev.map_or(1, |p| p.manifest.version + 1);
        if version > MAX_VERSIONS {
            return Err(format!("a dataset keeps at most {MAX_VERSIONS} versions"));
        }
        if rows.len() > MAX_ROWS {
            return Err(format!("a version holds at most {MAX_ROWS} rows"));
        }
        if sources.len() > MAX_SOURCES {
            return Err(format!("a version holds at most {MAX_SOURCES} sources"));
        }
        // Every row names a source of this version; every source's count is the rows it has.
        let mut counts: BTreeMap<&str, u32> = BTreeMap::new();
        for r in &rows {
            *counts.entry(r.source.as_str()).or_default() += 1;
        }
        let ids: BTreeSet<&str> = sources.iter().map(|s| s.id.as_str()).collect();
        if let Some(stray) = counts.keys().find(|k| !ids.contains(*k)) {
            return Err(format!(
                "a row names source {stray}, which this version does not list"
            ));
        }
        let sources: Vec<SourceRecord> = sources
            .into_iter()
            .map(|mut s| {
                s.rows = counts.get(s.id.as_str()).copied().unwrap_or(0);
                s
            })
            .filter(|s| s.rows > 0)
            .collect();
        let body = rows_bytes(&rows)?;
        let untrusted = sources.iter().any(|s| s.untrusted);
        let (license, privacy, pii, published_prev) = match publish {
            Some((l, p, s, pp)) => (Some(l), p, Some(s), Some(pp)),
            None => (
                None,
                prev.map_or(PrivacyLabel::Private, |p| p.manifest.privacy_label),
                None,
                None,
            ),
        };
        let manifest = Manifest {
            schema: MANIFEST_SCHEMA.into(),
            dataset: rec.id.clone(),
            name: rec.name.clone(),
            data_class: rec.data_class.clone(),
            version,
            prev: prev.map_or_else(|| ZERO_HASH.to_string(), |p| p.hash.clone()),
            created_at_ms: now_ms,
            privacy_label: privacy,
            contains_personal_data: prev.is_some_and(|p| p.manifest.contains_personal_data),
            license: license.or_else(|| prev.and_then(|p| p.manifest.license.clone())),
            consent: {
                let in_use: BTreeSet<&str> =
                    sources.iter().map(|s| s.consent_ref.as_str()).collect();
                consent
                    .into_iter()
                    .filter(|c| in_use.contains(c.as_str()))
                    .collect()
            },
            sources,
            rows: u32::try_from(rows.len()).unwrap_or(u32::MAX),
            content_hash: sha256_hex(&body),
            pii_scan: pii,
            derived_by: vec![op],
            untrusted,
            published_prev,
            written_by: app_version(),
        };
        let mbytes = serde_json::to_vec(&manifest).map_err(|e| e.to_string())?;
        // Rows first: a manifest is never on disk without its rows.
        self.write_file(keys, &rec.id, &format!("v{version}.rows"), &body, true)?;
        self.write_file(
            keys,
            &rec.id,
            &format!("v{version}.manifest"),
            &mbytes,
            true,
        )?;
        Ok(Version {
            hash: keccak_hex(&mbytes),
            manifest_bytes: mbytes,
            manifest,
        })
    }

    fn next_source_id(&self, keys: &KeySet, id: &str) -> Result<u64, String> {
        let mut max = 0u64;
        for v in self.version_numbers(id)? {
            for s in self.version(keys, id, v)?.manifest.sources {
                if let Some(n) = s.id.strip_prefix("s-").and_then(|n| n.parse::<u64>().ok()) {
                    max = max.max(n);
                }
            }
        }
        Ok(max + 1)
    }

    // ------------------------------------------------------------------ public API

    /// Every dataset on this device, each version's view, and whether its hash chain links.
    pub fn list(&self) -> Result<DatasetList, String> {
        let ids = self.dataset_ids()?;
        let Some(keys) = self.keys(false)? else {
            return Ok(DatasetList {
                datasets: Vec::new(),
                unreadable: Vec::new(),
            });
        };
        let mut out = DatasetList {
            datasets: Vec::new(),
            unreadable: Vec::new(),
        };
        for id in ids {
            let rec = match self.record(&keys, &id) {
                Ok(r) => r,
                Err(e) => {
                    out.unreadable.push(e);
                    continue;
                }
            };
            let mut versions = Vec::new();
            let mut problem: Option<String> = None;
            let mut prev_hash = ZERO_HASH.to_string();
            for (i, n) in self.version_numbers(&id)?.into_iter().enumerate() {
                if problem.is_none() && n != i as u32 + 1 {
                    problem = Some(format!("version {} is missing", i + 1));
                }
                match self
                    .version(&keys, &id, n)
                    .and_then(|v| self.view(&keys, &id, &v).map(|vw| (v, vw)))
                {
                    Ok((v, vw)) => {
                        if problem.is_none() && v.manifest.prev != prev_hash {
                            problem =
                                Some(format!("version {n} does not link to version {}", n - 1));
                        }
                        prev_hash = v.hash.clone();
                        versions.push(vw);
                    }
                    Err(e) => {
                        problem.get_or_insert(e);
                    }
                }
            }
            if versions.is_empty() && problem.is_none() {
                problem = Some("no versions".into());
            }
            out.datasets.push(DatasetView {
                id: rec.id,
                name: rec.name,
                data_class: rec.data_class,
                created_at_ms: rec.created_at_ms,
                versions,
                chain_ok: problem.is_none(),
                chain_problem: problem,
            });
        }
        out.datasets
            .sort_by(|a, b| b.created_at_ms.cmp(&a.created_at_ms).then(a.id.cmp(&b.id)));
        Ok(out)
    }

    /// A version's exact manifest bytes and hash.
    pub fn manifest(&self, id: &str, version: u32) -> Result<ManifestView, String> {
        let keys = self.need_keys()?;
        let v = self.version(&keys, id, version)?;
        Ok(ManifestView {
            manifest_hash: v.hash,
            manifest_json: String::from_utf8(v.manifest_bytes).map_err(|e| e.to_string())?,
            manifest: v.manifest,
        })
    }

    /// Create a dataset and its version 1 from already-read sources and rows. The ingest
    /// functions ([`ingest_folder`], [`ingest_conversations`]) produce the input.
    pub fn create(
        &self,
        req: &CreateRequest,
        ingest: Ingest,
        now_ms: u64,
    ) -> Result<CreateOutcome, String> {
        let name = clean_text("dataset name", &req.name)?;
        if !valid_data_class(&req.data_class) {
            return Err("a data class is 1 to 48 lowercase letters, digits or dashes".into());
        }
        let statement = clean_text("consent statement", &req.consent_statement)?;
        if ingest.rows.is_empty() {
            return Err("nothing to add: the source has no readable rows".into());
        }
        let _one = STORE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        if self.dataset_ids()?.len() >= MAX_DATASETS {
            return Err(format!("this device keeps at most {MAX_DATASETS} datasets"));
        }
        let keys = self.keys(true)?.ok_or_else(Self::unreachable)?;
        let id = new_dataset_id();
        let rec = DatasetRecord {
            id: id.clone(),
            name,
            data_class: req.data_class.clone(),
            created_at_ms: now_ms,
        };
        let consent = ConsentRecord {
            id: "c-1".into(),
            source_kind: ingest.kind,
            scope: ingest.scope.clone(),
            statement,
            granted_at_ms: now_ms,
            withdrawn_at_ms: None,
        };
        let sources: Vec<SourceRecord> = ingest
            .sources
            .into_iter()
            .map(|mut s| {
                s.consent_ref = consent.id.clone();
                s
            })
            .collect();
        let added = u32::try_from(ingest.rows.len()).unwrap_or(u32::MAX);
        let result = (|| {
            self.write_file(
                &keys,
                &id,
                DATASET_STEM,
                &serde_json::to_vec(&rec).map_err(|e| e.to_string())?,
                true,
            )?;
            self.save_consent(
                &keys,
                &id,
                &ConsentDoc {
                    next: 2,
                    records: vec![consent.clone()],
                },
            )?;
            let op = DerivedOp {
                op: "create".into(),
                detail: format!("from {}", ingest.scope),
                removed: 0,
                changed: 0,
                added,
            };
            let v = self.write_version(
                &keys,
                &rec,
                None,
                sources,
                ingest.rows,
                op,
                vec![consent.id.clone()],
                now_ms,
                None,
            )?;
            self.view(&keys, &id, &v)
        })();
        match result {
            Ok(version) => Ok(CreateOutcome {
                version,
                skipped: ingest.skipped,
            }),
            Err(e) => {
                let _ = std::fs::remove_dir_all(self.dir.join(&id));
                Err(e)
            }
        }
    }

    /// The collector's entry point (WP-H8): add collected items on top of `base_version` (which
    /// must be the latest) as a new version. Core checks the provenance here, whatever the caller
    /// checked: a web source needs an `allowed` robots verdict and a terms class; every source
    /// needs a fetch time, a content hash and a collector version. Web and connected-agent items
    /// are marked untrusted.
    // WP-H7 (the PII scanner) and WP-H8 (the collector) call this in-process; it has no command.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn append(
        &self,
        id: &str,
        base_version: u32,
        consent: ConsentInput,
        items: Vec<CollectedItem>,
        now_ms: u64,
    ) -> Result<VersionView, String> {
        if items.is_empty() {
            return Err("nothing to add".into());
        }
        let _one = STORE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let keys = self.need_keys()?;
        let rec = self.record(&keys, id)?;
        let base = self.latest(&keys, id)?;
        if base.manifest.version != base_version {
            return Err(format!(
                "version {base_version} is not the latest of {id} (v{} is); nothing was added",
                base.manifest.version
            ));
        }
        let mut doc = self.consent_doc(&keys, id)?;
        let consent_id = match consent {
            ConsentInput::Existing { id: cid } => {
                let c = doc
                    .records
                    .iter()
                    .find(|c| c.id == cid)
                    .ok_or_else(|| format!("no consent record {cid}"))?;
                if c.withdrawn_at_ms.is_some() {
                    return Err(format!("consent {cid} was withdrawn; nothing was added"));
                }
                cid
            }
            ConsentInput::New {
                source_kind,
                scope,
                statement,
            } => {
                let cid = format!("c-{}", doc.next);
                doc.records.push(ConsentRecord {
                    id: cid.clone(),
                    source_kind,
                    scope: clean_text("consent scope", &scope)?,
                    statement: clean_text("consent statement", &statement)?,
                    granted_at_ms: now_ms,
                    withdrawn_at_ms: None,
                });
                doc.next += 1;
                cid
            }
        };
        let kind_ok = doc
            .records
            .iter()
            .find(|c| c.id == consent_id)
            .map(|c| c.source_kind);
        let first = self.next_source_id(&keys, id)?;
        let mut sources = base.manifest.sources.clone();
        let mut rows = self.rows(&keys, id, &base)?;
        let mut added = 0u32;
        for (next, it) in (first..).zip(items) {
            check_item(&it)?;
            if kind_ok != Some(it.kind) {
                return Err(format!(
                    "consent {consent_id} does not cover {:?} sources",
                    it.kind
                ));
            }
            let sid = format!("s-{next}");
            for data in it.rows {
                rows.push(Row {
                    source: sid.clone(),
                    data,
                });
                added += 1;
            }
            sources.push(SourceRecord {
                id: sid,
                kind: it.kind,
                locator: it.locator,
                fetched_at_ms: it.fetched_at_ms,
                content_sha256: it.content_sha256,
                license: it.license,
                robots: it.robots,
                tos_class: it.tos_class,
                consent_ref: consent_id.clone(),
                collector_version: it.collector_version,
                untrusted: matches!(it.kind, SourceKind::Web | SourceKind::ConnectedAgent),
                rows: 0,
            });
        }
        let in_force: Vec<String> = doc
            .records
            .iter()
            .filter(|c| c.withdrawn_at_ms.is_none())
            .map(|c| c.id.clone())
            .collect();
        self.save_consent(&keys, id, &doc)?;
        let op = DerivedOp {
            op: "append".into(),
            detail: format!("collected under {consent_id}"),
            removed: 0,
            changed: 0,
            added,
        };
        let v = self.write_version(
            &keys,
            &rec,
            Some(&base),
            sources,
            rows,
            op,
            in_force,
            now_ms,
            None,
        )?;
        self.view(&keys, id, &v)
    }

    /// A few rows of a version, secrets redacted and emails masked, each truncated.
    pub fn sample(&self, id: &str, version: u32, n: u32, offset: u32) -> Result<Sample, String> {
        let keys = self.need_keys()?;
        let v = self.version(&keys, id, version)?;
        let rows = self.rows(&keys, id, &v)?;
        let n = n.clamp(1, MAX_SAMPLE_ROWS) as usize;
        let rows_out = rows
            .iter()
            .enumerate()
            .skip(offset as usize)
            .take(n)
            .map(|(i, r)| {
                let (text, truncated) = display_text(&r.data);
                SampleRow {
                    index: i as u32,
                    source: r.source.clone(),
                    text,
                    truncated,
                }
            })
            .collect();
        Ok(Sample {
            dataset: id.to_string(),
            version,
            total_rows: v.manifest.rows,
            rows: rows_out,
            untrusted: v.manifest.untrusted,
        })
    }

    /// Apply `ops` to `rows` in order. Returns (result, removed, changed, diff).
    fn run_clean(
        rows: &[Row],
        ops: &[CleanOp],
    ) -> Result<(Vec<Row>, u32, u32, Vec<DiffRow>), String> {
        if ops.is_empty() {
            return Err("no cleaning step was given".into());
        }
        if ops.len() > MAX_CLEAN_OPS {
            return Err(format!("at most {MAX_CLEAN_OPS} cleaning steps at once"));
        }
        // (original index, row) so the diff names base-version indices.
        let mut cur: Vec<(u32, Row)> = rows
            .iter()
            .cloned()
            .enumerate()
            .map(|(i, r)| (i as u32, r))
            .collect();
        let mut removed_idx: BTreeSet<u32> = BTreeSet::new();
        let mut changed_idx: BTreeSet<u32> = BTreeSet::new();
        for op in ops {
            match op {
                CleanOp::Dedupe => {
                    let mut seen = BTreeSet::new();
                    cur.retain(|(i, r)| {
                        let key = r.data.to_string();
                        if seen.insert(key) {
                            true
                        } else {
                            removed_idx.insert(*i);
                            false
                        }
                    });
                }
                CleanOp::DropRows { rows: drop } => {
                    if drop.is_empty() {
                        return Err("drop-rows names no rows".into());
                    }
                    let set: BTreeSet<u32> = drop.iter().copied().collect();
                    if let Some(bad) = set.iter().find(|i| **i as usize >= rows.len()) {
                        return Err(format!(
                            "row {bad} does not exist (the version has {} rows)",
                            rows.len()
                        ));
                    }
                    cur.retain(|(i, _)| {
                        if set.contains(i) {
                            removed_idx.insert(*i);
                            false
                        } else {
                            true
                        }
                    });
                }
                CleanOp::DropSource { source } => {
                    if !rows.iter().any(|r| &r.source == source) {
                        return Err(format!("no row comes from source {source}"));
                    }
                    cur.retain(|(i, r)| {
                        if &r.source == source {
                            removed_idx.insert(*i);
                            false
                        } else {
                            true
                        }
                    });
                }
                CleanOp::Redact { find, replace } => {
                    if find.is_empty()
                        || find.chars().count() > MAX_TEXT
                        || replace.chars().count() > MAX_TEXT
                    {
                        return Err("redact needs a non-empty find text and short texts".into());
                    }
                    for (i, r) in cur.iter_mut() {
                        if map_strings(&mut r.data, &|s: &str| {
                            s.replace(find.as_str(), replace.as_str())
                        }) {
                            changed_idx.insert(*i);
                        }
                    }
                }
                CleanOp::RedactSecrets => {
                    for (i, r) in cur.iter_mut() {
                        if map_strings(&mut r.data, &crate::secret_shapes::redact) {
                            changed_idx.insert(*i);
                        }
                    }
                }
                CleanOp::Normalize => {
                    for (i, r) in cur.iter_mut() {
                        if map_strings(&mut r.data, &normalize_text) {
                            changed_idx.insert(*i);
                        }
                    }
                }
            }
        }
        changed_idx.retain(|i| !removed_idx.contains(i));
        let after: BTreeMap<u32, &Row> = cur.iter().map(|(i, r)| (*i, r)).collect();
        let mut diff = Vec::new();
        for i in removed_idx
            .iter()
            .chain(changed_idx.iter())
            .copied()
            .collect::<BTreeSet<u32>>()
        {
            if diff.len() >= MAX_DIFF_ROWS {
                break;
            }
            let before = display_text(&rows[i as usize].data).0;
            let (kind, after_text) = match after.get(&i) {
                Some(r) => ("changed", Some(display_text(&r.data).0)),
                None => ("removed", None),
            };
            diff.push(DiffRow {
                index: i,
                kind: kind.into(),
                before,
                after: after_text,
            });
        }
        let removed = u32::try_from(removed_idx.len()).unwrap_or(u32::MAX);
        let changed = u32::try_from(changed_idx.len()).unwrap_or(u32::MAX);
        Ok((
            cur.into_iter().map(|(_, r)| r).collect(),
            removed,
            changed,
            diff,
        ))
    }

    /// What cleaning `version` with `ops` would do. Writes nothing.
    pub fn clean_preview(
        &self,
        id: &str,
        version: u32,
        ops: &[CleanOp],
    ) -> Result<CleanPreview, String> {
        let keys = self.need_keys()?;
        let v = self.version(&keys, id, version)?;
        let rows = self.rows(&keys, id, &v)?;
        let (result, removed, changed, diff) = Self::run_clean(&rows, ops)?;
        if removed == 0 && changed == 0 {
            return Err("those steps change nothing in this version".into());
        }
        Ok(CleanPreview {
            dataset: id.to_string(),
            base_version: version,
            base_manifest_hash: v.hash,
            ops: ops.to_vec(),
            removed,
            changed,
            kept: u32::try_from(result.len()).unwrap_or(u32::MAX),
            result_content_hash: sha256_hex(&rows_bytes(&result)?),
            diff,
            untrusted: v.manifest.untrusted,
        })
    }

    /// Write the cleaned version N+1 the member approved. `expected` is the preview's
    /// `resultContentHash`: core recomputes the clean and refuses when the result differs from
    /// what the card showed. The base must be the latest version.
    pub fn clean_apply(
        &self,
        id: &str,
        version: u32,
        ops: &[CleanOp],
        expected: &str,
        now_ms: u64,
    ) -> Result<VersionView, String> {
        let _one = STORE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let keys = self.need_keys()?;
        let rec = self.record(&keys, id)?;
        let base = self.latest(&keys, id)?;
        if base.manifest.version != version {
            return Err(format!(
                "version {version} is no longer the latest of {id} (v{} is); nothing was written",
                base.manifest.version
            ));
        }
        let rows = self.rows(&keys, id, &base)?;
        let (result, removed, changed, _) = Self::run_clean(&rows, ops)?;
        let got = sha256_hex(&rows_bytes(&result)?);
        if got != expected {
            return Err(
                "the cleaned rows differ from the approved preview; nothing was written".into(),
            );
        }
        let names: Vec<&str> = ops
            .iter()
            .map(|o| match o {
                CleanOp::Dedupe => "dedupe",
                CleanOp::DropRows { .. } => "drop-rows",
                CleanOp::DropSource { .. } => "drop-source",
                CleanOp::Redact { .. } => "redact",
                CleanOp::RedactSecrets => "redact-secrets",
                CleanOp::Normalize => "normalize",
            })
            .collect();
        let op = DerivedOp {
            op: names.first().copied().unwrap_or("clean").to_string(),
            detail: names.join(", "),
            removed,
            changed,
            added: 0,
        };
        let doc = self.consent_doc(&keys, id)?;
        let in_force = doc
            .records
            .iter()
            .filter(|c| c.withdrawn_at_ms.is_none())
            .map(|c| c.id.clone())
            .collect();
        let v = self.write_version(
            &keys,
            &rec,
            Some(&base),
            base.manifest.sources.clone(),
            result,
            op,
            in_force,
            now_ms,
            None,
        )?;
        self.view(&keys, id, &v)
    }

    /// The rows of a version, for the PII scanner (WP-H7) and the publish bundle. In-process only.
    // WP-H7 (the PII scanner) and WP-H8 (the collector) call this in-process; it has no command.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn rows_of(&self, id: &str, version: u32) -> Result<(Manifest, Vec<Row>), String> {
        let keys = self.need_keys()?;
        let v = self.version(&keys, id, version)?;
        let rows = self.rows(&keys, id, &v)?;
        Ok((v.manifest, rows))
    }

    /// Record the PII scanner's result for a version (WP-H7). The record must name the version's
    /// `contentHash`; a later scan replaces an earlier one.
    // WP-H7 (the PII scanner) and WP-H8 (the collector) call this in-process; it has no command.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn record_pii_scan(
        &self,
        id: &str,
        version: u32,
        scan: &PiiScanRecord,
    ) -> Result<(), String> {
        let _one = STORE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let keys = self.need_keys()?;
        let v = self.version(&keys, id, version)?;
        if scan.content_hash != v.manifest.content_hash {
            return Err("the scan is of other rows than this version's".into());
        }
        if scan.scanner.trim().is_empty() {
            return Err("the scan names no scanner".into());
        }
        if scan.pass != scan.counts.values().all(|n| *n == 0) {
            return Err("the scan's verdict does not match its counts".into());
        }
        self.write_file(
            &keys,
            id,
            &format!("v{version}.scan"),
            &serde_json::to_vec(scan).map_err(|e| e.to_string())?,
            false,
        )
    }

    /// The member's consent records for a dataset.
    pub fn consent_list(&self, id: &str) -> Result<Vec<ConsentRecord>, String> {
        let keys = self.need_keys()?;
        self.record(&keys, id)?;
        Ok(self.consent_doc(&keys, id)?.records)
    }

    /// Withdraw one consent (final). When the latest version has sources under it, core writes a
    /// new version without them at once.
    pub fn consent_withdraw(
        &self,
        id: &str,
        consent_id: &str,
        now_ms: u64,
    ) -> Result<ConsentWithdrawn, String> {
        let _one = STORE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let keys = self.need_keys()?;
        let rec = self.record(&keys, id)?;
        let mut doc = self.consent_doc(&keys, id)?;
        let c = doc
            .records
            .iter_mut()
            .find(|c| c.id == consent_id)
            .ok_or_else(|| format!("no consent record {consent_id}"))?;
        if c.withdrawn_at_ms.is_some() {
            return Err(format!("consent {consent_id} was already withdrawn"));
        }
        c.withdrawn_at_ms = Some(now_ms);
        let consent = c.clone();
        self.save_consent(&keys, id, &doc)?;
        let base = self.latest(&keys, id)?;
        let gone: BTreeSet<String> = base
            .manifest
            .sources
            .iter()
            .filter(|s| s.consent_ref == consent_id)
            .map(|s| s.id.clone())
            .collect();
        let new_version = if gone.is_empty() {
            None
        } else {
            let rows = self.rows(&keys, id, &base)?;
            let before = rows.len();
            let kept: Vec<Row> = rows
                .into_iter()
                .filter(|r| !gone.contains(&r.source))
                .collect();
            let sources = base
                .manifest
                .sources
                .iter()
                .filter(|s| !gone.contains(&s.id))
                .cloned()
                .collect();
            let op = DerivedOp {
                op: "consent-withdrawn".into(),
                detail: format!("{consent_id} withdrawn"),
                removed: u32::try_from(before - kept.len()).unwrap_or(u32::MAX),
                changed: 0,
                added: 0,
            };
            let in_force = doc
                .records
                .iter()
                .filter(|c| c.withdrawn_at_ms.is_none())
                .map(|c| c.id.clone())
                .collect();
            let v = self.write_version(
                &keys,
                &rec,
                Some(&base),
                sources,
                kept,
                op,
                in_force,
                now_ms,
                None,
            )?;
            Some(self.view(&keys, id, &v)?)
        };
        Ok(ConsentWithdrawn {
            consent,
            new_version,
            earlier_versions_kept: true,
        })
    }

    fn publications(&self, keys: &KeySet, id: &str) -> Result<Vec<Publication>, String> {
        match self.read_file(keys, id, PUBLISHED_STEM)? {
            None => Ok(Vec::new()),
            Some(p) => serde_json::from_slice(&p)
                .map_err(|_| format!("the publication records of {id} are damaged")),
        }
    }

    /// The publications of one dataset (IPFS CIDs and the registry record each waits for).
    pub fn published(&self, id: &str) -> Result<Vec<Publication>, String> {
        let keys = self.need_keys()?;
        self.record(&keys, id)?;
        self.publications(&keys, id)
    }

    /// Every reason core refuses to publish `version` with `license` and `privacy`. Empty = it
    /// may publish. The gates (gap §3.1 rule 6, planset 06 §5): the license passes the registry
    /// rule; no source's license is unknown; a PII scan of exactly these rows passed (the scanner
    /// decides, never the model); a dataset with personal data is never Public; and this build
    /// publishes Public only (members and group need encrypted group files, index-only needs the
    /// registry record alone).
    fn publish_refusals(
        &self,
        keys: &KeySet,
        id: &str,
        v: &Version,
        license: &str,
        privacy: PrivacyLabel,
    ) -> Result<Vec<String>, String> {
        let mut out = Vec::new();
        if let Err(e) = registry_license_ok(license) {
            out.push(format!("License: {e}"));
        }
        let unknown = v
            .manifest
            .sources
            .iter()
            .filter(|s| s.license.id.is_none())
            .count();
        if unknown > 0 {
            out.push(format!(
                "License unknown for {unknown} source{}",
                if unknown == 1 { "" } else { "s" }
            ));
        }
        match self
            .scan(keys, id, v)?
            .or_else(|| v.manifest.pii_scan.clone())
        {
            None => {
                out.push("No PII scan of these rows yet; the on-device scan must pass first".into())
            }
            Some(s) if s.content_hash != v.manifest.content_hash => {
                out.push("The PII scan is of other rows; scan this version".into())
            }
            Some(s) if !s.pass => out.push(format!(
                "The PII scan found personal data ({}); clean it or keep the dataset local",
                s.counts
                    .iter()
                    .filter(|(_, n)| **n > 0)
                    .map(|(k, n)| format!("{n} {k}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
            Some(_) => {}
        }
        if v.manifest.contains_personal_data && privacy == PrivacyLabel::Public {
            out.push("A dataset that contains personal data can never be Public".into());
        }
        match privacy {
            PrivacyLabel::Public => {}
            PrivacyLabel::Members | PrivacyLabel::Group => out.push(
                "Publishing to members or a group needs encrypted group files, which are not in this build".into(),
            ),
            PrivacyLabel::IndexOnly => out.push(
                "Index-only publishes no file, only the DatasetRegistry record, which is not in this build".into(),
            ),
            PrivacyLabel::Private => out.push("Private datasets are not published".into()),
        }
        Ok(out)
    }

    /// What publishing `version` would do and every reason it is refused. Writes nothing.
    pub fn publish_check(
        &self,
        id: &str,
        version: u32,
        license: &str,
        privacy: PrivacyLabel,
    ) -> Result<PublishCheck, String> {
        let keys = self.need_keys()?;
        let rec = self.record(&keys, id)?;
        let latest = self.latest(&keys, id)?;
        let v = self.version(&keys, id, version)?;
        let mut refusals = self.publish_refusals(&keys, id, &v, license, privacy)?;
        if latest.manifest.version != version {
            refusals.push(format!(
                "Version {version} is not the latest (v{} is)",
                latest.manifest.version
            ));
        }
        let pubs = self.publications(&keys, id)?;
        let rows = self
            .read_file(&keys, id, &format!("v{version}.rows"))?
            .map(|r| r.len())
            .unwrap_or(0);
        Ok(PublishCheck {
            dataset: id.to_string(),
            name: rec.name,
            version,
            content_hash: v.manifest.content_hash.clone(),
            rows: v.manifest.rows,
            sources: u32::try_from(v.manifest.sources.len()).unwrap_or(u32::MAX),
            license: license.to_string(),
            privacy,
            registry_prev: pubs
                .last()
                .map_or_else(|| ZERO_HASH.to_string(), |p| p.manifest_hash.clone()),
            bundle_bytes: (v.manifest_bytes.len() + 1 + rows) as u64,
            untrusted: v.manifest.untrusted,
            refusals,
        })
    }

    /// Publish the latest version the member approved: write the publish version N+1 (rows
    /// unchanged; the license, the label, the PII scan and the registry `prev` in its manifest),
    /// add its bundle to IPFS through `sink` and pin it on this node, and record the publication.
    /// `expected_content_hash` is the rows hash the card showed. Refused on any gate.
    ///
    /// The bundle is one file: the publish version's manifest bytes, a newline, then its rows file.
    /// Anyone can check it: keccak256 of the first line is the manifestHash, SHA-256 of the rest is
    /// the manifest's contentHash. Public only in this build, so the CID holds plaintext.
    #[allow(clippy::too_many_arguments)]
    pub fn publish(
        &self,
        id: &str,
        version: u32,
        license: &str,
        privacy: PrivacyLabel,
        expected_content_hash: &str,
        sink: &dyn PublishSink,
        now_ms: u64,
    ) -> Result<Published, String> {
        let _one = STORE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let keys = self.need_keys()?;
        let rec = self.record(&keys, id)?;
        let base = self.latest(&keys, id)?;
        if base.manifest.version != version {
            return Err(format!(
                "version {version} is no longer the latest of {id} (v{} is); nothing was published",
                base.manifest.version
            ));
        }
        if base.manifest.content_hash != expected_content_hash {
            return Err("the rows differ from the ones you approved; nothing was published".into());
        }
        let refusals = self.publish_refusals(&keys, id, &base, license, privacy)?;
        if !refusals.is_empty() {
            return Err(format!("not published: {}", refusals.join("; ")));
        }
        let mut pubs = self.publications(&keys, id)?;
        let registry_prev = pubs
            .last()
            .map_or_else(|| ZERO_HASH.to_string(), |p| p.manifest_hash.clone());
        let rows = self.rows(&keys, id, &base)?;
        // A publish version left by an earlier try whose IPFS add failed is published as it is.
        let reuse = base.manifest.published_prev.as_deref() == Some(registry_prev.as_str())
            && base.manifest.license.as_deref() == Some(license)
            && base.manifest.privacy_label == privacy
            && !pubs.iter().any(|p| p.version == base.manifest.version);
        let pv = if reuse {
            base
        } else {
            let scan = self
                .scan(&keys, id, &base)?
                .or_else(|| base.manifest.pii_scan.clone())
                .ok_or_else(|| "no PII scan of these rows".to_string())?;
            let doc = self.consent_doc(&keys, id)?;
            let in_force = doc
                .records
                .iter()
                .filter(|c| c.withdrawn_at_ms.is_none())
                .map(|c| c.id.clone())
                .collect();
            let op = DerivedOp {
                op: "publish".into(),
                detail: format!("{license}, {}", privacy_word(privacy)),
                removed: 0,
                changed: 0,
                added: 0,
            };
            self.write_version(
                &keys,
                &rec,
                Some(&base),
                base.manifest.sources.clone(),
                rows.clone(),
                op,
                in_force,
                now_ms,
                Some((license.to_string(), privacy, scan, registry_prev.clone())),
            )?
        };
        let mut bundle = pv.manifest_bytes.clone();
        bundle.push(b'\n');
        bundle.extend_from_slice(&rows_bytes(&rows)?);
        let cid = sink.add_and_pin(&format!("{id}-v{}.citrate-dataset.jsonl", pv.manifest.version), &bundle).map_err(|e| {
            format!(
                "version {} is ready to publish, but IPFS did not take it ({e}); nothing left this device. Try again once IPFS is running.",
                pv.manifest.version
            )
        })?;
        let publication = Publication {
            version: pv.manifest.version,
            manifest_hash: pv.hash.clone(),
            content_hash: pv.manifest.content_hash.clone(),
            cid: cid.clone(),
            license: license.to_string(),
            privacy,
            published_at_ms: now_ms,
            registry_prev: registry_prev.clone(),
        };
        pubs.push(publication.clone());
        self.write_file(
            &keys,
            id,
            PUBLISHED_STEM,
            &serde_json::to_vec(&pubs).map_err(|e| e.to_string())?,
            false,
        )?;
        Ok(Published {
            version: self.view(&keys, id, &pv)?,
            name: rec.name,
            publication,
            bundle_bytes: bundle.len() as u64,
        })
    }

    /// Delete a dataset and every version of it on this device. When it was the last one, the
    /// dataset key is removed from the keychain too, so no leftover copy on this device opens.
    pub fn delete(&self, id: &str) -> Result<(), String> {
        let _one = STORE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = self.ds_dir(id)?;
        match std::fs::remove_dir_all(&dir) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(format!("no dataset {id}"))
            }
            Err(e) => return Err(format!("the dataset could not be deleted: {}", e.kind())),
        }
        if self.dataset_ids()?.is_empty() {
            self.keyring
                .delete(KEY_ACCOUNT)
                .map_err(|_| Self::unreachable())?;
        }
        Ok(())
    }
}

fn check_item(it: &CollectedItem) -> Result<(), String> {
    let bad = |why: &str| {
        Err(format!(
            "a collected item from {:?} was refused: {why}",
            it.locator
        ))
    };
    if it.locator.trim().is_empty() || it.locator.chars().count() > 2048 {
        return bad("no locator");
    }
    if it.fetched_at_ms == 0 {
        return bad("no fetch time");
    }
    if !is_hash(&it.content_sha256) {
        return bad("no content hash");
    }
    if it.collector_version.trim().is_empty() {
        return bad("no collector version");
    }
    if it.rows.is_empty() {
        return bad("no rows");
    }
    match it.kind {
        SourceKind::Web => {
            if it.robots != RobotsVerdict::Allowed {
                return bad("a web source needs robots.txt to allow it");
            }
            if it.tos_class == TosClass::NotApplicable {
                return bad("a web source needs a terms class");
            }
            let lower = it.locator.to_ascii_lowercase();
            if !(lower.starts_with("https://") || lower.starts_with("http://")) {
                return bad("a web source's locator is its URL");
            }
        }
        SourceKind::Licensed => {
            if it.license.id.is_none() {
                return bad("a licensed source needs its license");
            }
        }
        SourceKind::Folder | SourceKind::Conversations => {
            return bad(
                "folder and conversation sources are added by core's own ingest, not the collector",
            );
        }
        SourceKind::ConnectedAgent => {}
    }
    if let Some(l) = &it.license.id {
        registry_license_ok(l).or_else(|e| bad(&e))?;
    }
    Ok(())
}

fn create_private_dir(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir)
        .map_err(|e| format!("the datasets folder could not be created: {}", e.kind()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
    }
    Ok(())
}

// ------------------------------------------------------------------------------ ingest

/// Sources and rows read for a new dataset.
#[derive(Debug, Clone, PartialEq)]
pub struct Ingest {
    pub kind: SourceKind,
    /// What the consent covers (the folder's name, or `conversations`).
    pub scope: String,
    pub sources: Vec<SourceRecord>,
    pub rows: Vec<Row>,
    pub skipped: Vec<String>,
}

fn rows_from_file(ext: &str, text: &str) -> Result<Vec<serde_json::Value>, String> {
    match ext {
        "jsonl" => {
            let mut out = Vec::new();
            for (n, line) in text
                .lines()
                .enumerate()
                .filter(|(_, l)| !l.trim().is_empty())
            {
                out.push(
                    serde_json::from_str(line)
                        .map_err(|_| format!("line {} is not JSON", n + 1))?,
                );
            }
            Ok(out)
        }
        "json" => match serde_json::from_str::<serde_json::Value>(text)
            .map_err(|_| "not JSON".to_string())?
        {
            serde_json::Value::Array(a) => Ok(a),
            v => Ok(vec![v]),
        },
        _ => {
            let t = text.trim();
            if t.is_empty() {
                Ok(Vec::new())
            } else {
                Ok(vec![serde_json::json!({ "text": t })])
            }
        }
    }
}

/// Read a member-granted folder: text, Markdown, JSON and JSON Lines files, no hidden files or
/// folders, no links, nothing in a default-deny location. `root` must be inside an active read
/// grant (checked by the caller with [`folder_read_granted`]).
pub fn ingest_folder(root: &Path, now_ms: u64) -> Result<Ingest, String> {
    let root = std::fs::canonicalize(root)
        .map_err(|_| format!("{} is not an existing folder", root.display()))?;
    if !root.is_dir() {
        return Err(format!("{} is not a folder", root.display()));
    }
    if let Some(why) = crate::grant_deny::denied_location(&root) {
        return Err(format!(
            "{} is a protected location ({why})",
            root.display()
        ));
    }
    let label = root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "folder".into());
    let license = match folder_license(&root) {
        Some((id, file)) => LicenseRecord {
            id: Some(id),
            basis: LicenseBasis::Found,
            evidence: Some(file),
        },
        None => LicenseRecord::unknown(),
    };
    let mut files: Vec<PathBuf> = Vec::new();
    let mut skipped = Vec::new();
    let mut stack = vec![(root.clone(), 0usize)];
    while let Some((dir, depth)) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            skipped.push(format!("{}: cannot be read", rel(&root, &dir)));
            continue;
        };
        let mut entries: Vec<_> = rd.filter_map(Result::ok).collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let p = e.path();
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_symlink() {
                skipped.push(format!("{}: a link (not followed)", rel(&root, &p)));
            } else if ft.is_dir() {
                if depth + 1 > MAX_FOLDER_DEPTH {
                    skipped.push(format!(
                        "{}: deeper than {MAX_FOLDER_DEPTH} folders",
                        rel(&root, &p)
                    ));
                } else if crate::grant_deny::denied_location(&p).is_none() {
                    stack.push((p, depth + 1));
                } else {
                    skipped.push(format!("{}: a protected location", rel(&root, &p)));
                }
            } else if ft.is_file() {
                files.push(p);
            }
        }
    }
    files.sort();
    let mut sources = Vec::new();
    let mut rows = Vec::new();
    let mut total = 0u64;
    for (n, p) in files.iter().enumerate() {
        let r = rel(&root, p);
        let ext = p
            .extension()
            .and_then(|x| x.to_str())
            .map(str::to_ascii_lowercase)
            .unwrap_or_default();
        if !FOLDER_EXTENSIONS.contains(&ext.as_str()) {
            skipped.push(format!("{r}: not a text, Markdown or JSON file"));
            continue;
        }
        if sources.len() >= MAX_FOLDER_FILES {
            skipped.push(format!("{r}: past the {MAX_FOLDER_FILES}-file limit"));
            continue;
        }
        let size = std::fs::metadata(p).map(|m| m.len()).unwrap_or(u64::MAX);
        if size > MAX_FOLDER_FILE_BYTES || total + size > MAX_ROWS_BYTES as u64 {
            skipped.push(format!("{r}: too large"));
            continue;
        }
        let Ok(bytes) = std::fs::read(p) else {
            skipped.push(format!("{r}: cannot be read"));
            continue;
        };
        let Ok(text) = std::str::from_utf8(&bytes) else {
            skipped.push(format!("{r}: not UTF-8 text"));
            continue;
        };
        let vals = match rows_from_file(&ext, text) {
            Ok(v) if !v.is_empty() => v,
            Ok(_) => {
                skipped.push(format!("{r}: empty"));
                continue;
            }
            Err(why) => {
                skipped.push(format!("{r}: {why}"));
                continue;
            }
        };
        total += size;
        let sid = format!("s-{}", n + 1);
        let file_license = spdx_tag(text)
            .map(|id| LicenseRecord {
                id: Some(id),
                basis: LicenseBasis::Found,
                evidence: Some(r.clone()),
            })
            .unwrap_or_else(|| license.clone());
        for v in vals {
            rows.push(Row {
                source: sid.clone(),
                data: v,
            });
        }
        sources.push(SourceRecord {
            id: sid,
            kind: SourceKind::Folder,
            locator: format!("{label}/{r}"),
            fetched_at_ms: now_ms,
            content_sha256: sha256_hex(&bytes),
            license: file_license,
            robots: RobotsVerdict::NotApplicable,
            tos_class: TosClass::NotApplicable,
            consent_ref: String::new(),
            collector_version: CORE_INGEST_VERSION.into(),
            untrusted: false,
            rows: 0,
        });
    }
    // Renumber sources densely (s-1..), keeping row links.
    let mut map = BTreeMap::new();
    for (i, s) in sources.iter_mut().enumerate() {
        let new = format!("s-{}", i + 1);
        map.insert(s.id.clone(), new.clone());
        s.id = new;
    }
    for r in rows.iter_mut() {
        if let Some(n) = map.get(&r.source) {
            r.source = n.clone();
        }
    }
    Ok(Ingest {
        kind: SourceKind::Folder,
        scope: label,
        sources,
        rows,
        skipped,
    })
}

fn rel(root: &Path, p: &Path) -> String {
    p.strip_prefix(root)
        .unwrap_or(p)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Read the member's verified Hermes conversations (newest first, at most `max`). Refused when
/// the "train on my verified conversations" switch is off.
pub fn ingest_conversations(hermes_dir: &Path, max: u32, now_ms: u64) -> Result<Ingest, String> {
    let got = crate::fl_trajectories::collect_verified(hermes_dir, max)?;
    let body = got.lines.join("\n");
    let rows = got
        .lines
        .iter()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .map(|data| Row {
            source: "s-1".into(),
            data,
        })
        .collect::<Vec<_>>();
    let source = SourceRecord {
        id: "s-1".into(),
        kind: SourceKind::Conversations,
        locator: "conversations".into(),
        fetched_at_ms: now_ms,
        content_sha256: sha256_hex(body.as_bytes()),
        // The member's own conversations with their agent: theirs to license at publish.
        license: LicenseRecord::unknown(),
        robots: RobotsVerdict::NotApplicable,
        tos_class: TosClass::NotApplicable,
        consent_ref: String::new(),
        collector_version: CORE_INGEST_VERSION.into(),
        untrusted: false,
        rows: 0,
    };
    let mut skipped = Vec::new();
    if got.skipped_lines > 0 {
        skipped.push(format!(
            "{} lines were not verified conversation turns",
            got.skipped_lines
        ));
    }
    if got.over_cap > 0 {
        skipped.push(format!(
            "{} verified turns beyond the cap of {max}",
            got.over_cap
        ));
    }
    Ok(Ingest {
        kind: SourceKind::Conversations,
        scope: "conversations".into(),
        sources: vec![source],
        rows,
        skipped,
    })
}

// ------------------------------------------------------------------------------ commands

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

/// `<app data>/datasets`. Fails closed if that folder is not inside core's own data folder (the
/// one the agent's default-deny list covers): the store is never placed where file tools reach.
pub fn datasets_dir<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Result<PathBuf, String> {
    use tauri::Manager;
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join(DATASETS_DIR);
    if !dataset_root_is_agent_denied(&dir) {
        return Err("the datasets folder is outside Citrate Core's protected data folder; datasets stay closed".into());
    }
    Ok(dir)
}

fn with_store<T: Send + 'static>(
    app: tauri::AppHandle,
    f: impl FnOnce(&DatasetStore<'_>, &tauri::AppHandle) -> Result<T, String> + Send + 'static,
) -> impl std::future::Future<Output = Result<T, String>> {
    crate::blocking::off_main(move || {
        let keyring = crate::custody::OsKeyring::legacy();
        let dir = datasets_dir(&app)?;
        f(&DatasetStore::new(dir, &keyring), &app)
    })
}

/// **fl_dataset_list**: every dataset on this device with its versions and hash chain. Data
/// source: `<app data>/datasets/`, opened with the dataset key in the OS keyring.
#[tauri::command]
pub async fn fl_dataset_list(app: tauri::AppHandle) -> Result<DatasetList, String> {
    with_store(app, |s, _| s.list()).await
}

/// **fl_dataset_manifest**: one version's exact manifest JSON and its manifestHash.
#[tauri::command]
pub async fn fl_dataset_manifest(
    app: tauri::AppHandle,
    dataset: String,
    version: u32,
) -> Result<ManifestView, String> {
    with_store(app, move |s, _| s.manifest(&dataset, version)).await
}

/// **fl_dataset_create**: the member approved a create card; read the source and write v1.
/// A folder must be inside an active folder grant for reading; conversations need the training
/// switch on.
#[tauri::command]
pub async fn fl_dataset_create(
    app: tauri::AppHandle,
    request: CreateRequest,
) -> Result<CreateOutcome, String> {
    with_store(app, move |s, app| {
        let now = now_ms();
        let ingest = match &request.source {
            CreateSource::Folder { path } => {
                let p = Path::new(path);
                if !p.is_absolute() {
                    return Err("name the folder by its full path".into());
                }
                let canon = std::fs::canonicalize(p).map_err(|_| format!("{path} is not an existing folder"))?;
                let grants = match crate::agent_grants::GrantStore::for_app(app)?.load() {
                    crate::agent_grants::Loaded::Ok(st) => st,
                    crate::agent_grants::Loaded::Corrupted(_) => {
                        return Err("the folder grants are damaged, so no folder counts as granted".into())
                    }
                };
                if !folder_read_granted(&grants, &canon, now / 1000) {
                    return Err(format!(
                        "{} is not in a folder you granted Hermes to read. Grant it in Hermes > Controls > Details > Terminal & folders first; nothing was read.",
                        canon.display()
                    ));
                }
                ingest_folder(&canon, now)?
            }
            CreateSource::Conversations { max } => {
                use tauri::Manager;
                let hermes = app.path().app_local_data_dir().map_err(|e| e.to_string())?.join("hermes");
                ingest_conversations(&hermes, *max, now)?
            }
        };
        s.create(&request, ingest, now)
    })
    .await
}

/// **fl_dataset_sample**: a few rows, redacted and truncated.
#[tauri::command]
pub async fn fl_dataset_sample(
    app: tauri::AppHandle,
    dataset: String,
    version: u32,
    n: Option<u32>,
    offset: Option<u32>,
) -> Result<Sample, String> {
    with_store(app, move |s, _| {
        s.sample(&dataset, version, n.unwrap_or(5), offset.unwrap_or(0))
    })
    .await
}

/// **fl_dataset_clean_preview**: what a clean would change. Writes nothing.
#[tauri::command]
pub async fn fl_dataset_clean_preview(
    app: tauri::AppHandle,
    dataset: String,
    version: u32,
    ops: Vec<CleanOp>,
) -> Result<CleanPreview, String> {
    with_store(app, move |s, _| s.clean_preview(&dataset, version, &ops)).await
}

/// **fl_dataset_clean_apply**: the member approved the diff card; write version N+1.
#[tauri::command]
pub async fn fl_dataset_clean_apply(
    app: tauri::AppHandle,
    dataset: String,
    version: u32,
    ops: Vec<CleanOp>,
    result_content_hash: String,
) -> Result<VersionView, String> {
    with_store(app, move |s, _| {
        s.clean_apply(&dataset, version, &ops, &result_content_hash, now_ms())
    })
    .await
}

/// **fl_dataset_consent_list**: the consent records of one dataset.
#[tauri::command]
pub async fn fl_dataset_consent_list(
    app: tauri::AppHandle,
    dataset: String,
) -> Result<Vec<ConsentRecord>, String> {
    with_store(app, move |s, _| s.consent_list(&dataset)).await
}

/// **fl_dataset_consent_withdraw**: withdraw one consent (lowers authority; no card).
#[tauri::command]
pub async fn fl_dataset_consent_withdraw(
    app: tauri::AppHandle,
    dataset: String,
    consent: String,
) -> Result<ConsentWithdrawn, String> {
    with_store(app, move |s, _| {
        s.consent_withdraw(&dataset, &consent, now_ms())
    })
    .await
}

/// **fl_dataset_publish_check**: every reason core refuses publishing a version. Writes nothing.
#[tauri::command]
pub async fn fl_dataset_publish_check(
    app: tauri::AppHandle,
    dataset: String,
    version: u32,
    license: String,
    privacy: PrivacyLabel,
) -> Result<PublishCheck, String> {
    with_store(app, move |s, _| {
        s.publish_check(&dataset, version, &license, privacy)
    })
    .await
}

/// **fl_dataset_publish**: the member approved the publish card (HIC-1, never in an envelope):
/// write the publish version and add its bundle to IPFS on this node. Cannot be recalled.
#[tauri::command]
pub async fn fl_dataset_publish(
    app: tauri::AppHandle,
    dataset: String,
    version: u32,
    license: String,
    privacy: PrivacyLabel,
    content_hash: String,
) -> Result<Published, String> {
    with_store(app, move |s, app| {
        let sink = IpfsSink(crate::storage::build_manager(app)?);
        s.publish(
            &dataset,
            version,
            &license,
            privacy,
            &content_hash,
            &sink,
            now_ms(),
        )
    })
    .await
}

/// **fl_dataset_published**: a dataset's publications (CIDs, and the registry `prev` each carries).
#[tauri::command]
pub async fn fl_dataset_published(
    app: tauri::AppHandle,
    dataset: String,
) -> Result<Vec<Publication>, String> {
    with_store(app, move |s, _| s.published(&dataset)).await
}

/// **fl_dataset_delete**: delete a dataset and all its versions on this device.
#[tauri::command]
pub async fn fl_dataset_delete(app: tauri::AppHandle, dataset: String) -> Result<(), String> {
    with_store(app, move |s, _| s.delete(&dataset)).await
}

#[cfg(test)]
#[path = "fl_datasets_tests.rs"]
mod tests;
