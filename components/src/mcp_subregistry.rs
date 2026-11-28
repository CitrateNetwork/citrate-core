//! L6-WP08a: the signed MCP subregistry, the Citrate curated mirror of the official MCP Registry
//! (`registry.modelcontextprotocol.io`).
//!
//! The document is JSON plus a detached minisign signature over its exact bytes, made offline with
//! the **catalog key** (the same custody as the Store catalog) but under its own trusted-comment
//! domain, [`SUBREGISTRY_DOMAIN`]: a Store catalog signature never passes as a subregistry
//! signature, and the reverse. It reuses the catalog verification core: prehashed Ed25519 only,
//! the size cap before anything else, nothing in the JSON read before the signature verifies,
//! anti-rollback by sequence (persisted on disk by [`verify_and_record`]), and a bounded lifetime.
//!
//! **Key slot.** The trust root is the catalog key slot, [`PRODUCTION_CATALOG_PUBKEY`], which is
//! EMPTY until the owner's key ceremony. Until then [`subregistry_trust_root`] refuses with
//! [`SubregistryError::KeyNotIssued`] ("Signed catalog not available yet: signing key not
//! issued"), and the app lists only the verified brand servers compiled into it. No placeholder
//! key exists here (`tests/mcp_subregistry.rs` scans this file for one).
//!
//! **Schema v1.** The entry shape follows the official registry `server.json` (`name`, `title`,
//! `description`, `version`, `remotes`, `packages`), with the Citrate review record under
//! `_meta["ai.citrate/curation"] = { trust, reviewedAt, toolSnapshotSha256 }`. Unlike the Store
//! catalog (first-party only), entries may be third-party servers, but only with that record. An
//! entry names at least one https remote or one package pinned to an exact version. Any entry
//! that breaks a rule refuses the whole document.
//!
//! Rule 3: nothing here holds a private key or signs. The mirror builder (`tools/mcp-mirror/`)
//! writes the unsigned document; the owner signs it offline.
use std::collections::BTreeSet;
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::Digest as _;

use crate::catalog::{catalog_trust_root_from, CatalogError, PRODUCTION_CATALOG_PUBKEY};
use crate::error::ComponentError;
use crate::key::TrustRoot;
use crate::manifest::{is_https_url, verify_detached};

pub const SUBREGISTRY_SCHEMA: u32 = 1;
/// Trusted-comment domain of a subregistry signature.
pub const SUBREGISTRY_DOMAIN: &str = "citrate-mcp-subregistry-v1";
/// Checked before the signature and before any parsing.
pub const MAX_SUBREGISTRY_BYTES: usize = 2 << 20;
/// The longest a subregistry may be valid for (the builder runs weekly; same bound as the Store).
pub const MAX_SUBREGISTRY_LIFETIME_SECS: u64 = 31 * 86_400;
/// How far `issued_at` may be ahead of the local clock.
pub const CLOCK_SKEW_SECS: u64 = 600;
const MAX_SERVERS: usize = 2048;
const MAX_REMOTES: usize = 8;
const MAX_PACKAGES: usize = 8;
const MAX_NAME: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubregistryError {
    /// The catalog key slot is empty (the key ceremony has not happened).
    KeyNotIssued,
    /// The configured key does not parse, or is a key the subregistry must not trust.
    BadKey(String),
    /// The signature does not decode, is the wrong algorithm, or does not verify.
    SignatureInvalid(String),
    /// A valid signature made for another purpose (a Store catalog or component manifest).
    WrongPurpose,
    /// Larger than the cap; refused before the signature or the parser saw it.
    TooLarge { limit: usize },
    /// Signed, but not JSON of the schema, or a field is out of bounds.
    Malformed(String),
    Expired,
    NotYetValid,
    /// Older than the subregistry already seen.
    Rollback { seen: u64, got: u64 },
    /// The persisted last-seen record cannot be read or written. Never silently reset: the
    /// sequence in it is what stops a replayed older document.
    State(String),
}

impl std::fmt::Display for SubregistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use SubregistryError::*;
        match self {
            KeyNotIssued => f.write_str("Signed catalog not available yet: signing key not issued"),
            BadKey(m) => write!(f, "the catalog key is not usable: {m}"),
            SignatureInvalid(m) => write!(f, "the signed catalog signature did not verify: {m}"),
            WrongPurpose => f.write_str("the signed catalog was signed for a different purpose"),
            TooLarge { limit } => write!(f, "the signed catalog is larger than {limit} bytes"),
            Malformed(m) => write!(f, "the signed catalog is not valid: {m}"),
            Expired => f.write_str("the signed catalog has expired"),
            NotYetValid => f.write_str("the signed catalog is not valid yet (check the clock)"),
            Rollback { seen, got } => write!(
                f,
                "the signed catalog is older than the one already seen (sequence {got} < {seen})"
            ),
            State(m) => write!(f, "the signed catalog record on this machine is unreadable: {m}"),
        }
    }
}

impl std::error::Error for SubregistryError {}

fn from_catalog(e: CatalogError) -> SubregistryError {
    match e {
        CatalogError::KeyNotIssued => SubregistryError::KeyNotIssued,
        CatalogError::BadKey(m) => SubregistryError::BadKey(m),
        CatalogError::SignatureInvalid(m) => SubregistryError::SignatureInvalid(m),
        CatalogError::WrongPurpose => SubregistryError::WrongPurpose,
        other => SubregistryError::Malformed(other.to_string()),
    }
}

fn from_component(e: ComponentError) -> SubregistryError {
    match e {
        ComponentError::KeyNotConfigured => SubregistryError::KeyNotIssued,
        ComponentError::BadKey(m) => SubregistryError::BadKey(m),
        ComponentError::SignatureInvalid(m) => SubregistryError::SignatureInvalid(m),
        ComponentError::WrongTrustedComment => SubregistryError::WrongPurpose,
        other => SubregistryError::Malformed(other.to_string()),
    }
}

/// The production trust root (the catalog key slot), or [`SubregistryError::KeyNotIssued`]
/// while the slot is empty.
pub fn subregistry_trust_root() -> Result<TrustRoot, SubregistryError> {
    subregistry_trust_root_from(PRODUCTION_CATALOG_PUBKEY)
}

/// [`subregistry_trust_root`] over a given slot, with the catalog key-separation rule (never the
/// app-updater key or the component key).
pub fn subregistry_trust_root_from(slot: Option<&str>) -> Result<TrustRoot, SubregistryError> {
    catalog_trust_root_from(slot).map_err(from_catalog)
}

/// The Citrate review level of a mirrored entry. A closed set: an unknown value refuses the
/// document. "Registry, unreviewed" and "member added" never appear in a signed subregistry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Trust {
    VerifiedBrand,
    Curated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Transport {
    #[serde(rename = "streamable-http")]
    StreamableHttp,
    #[serde(rename = "sse")]
    Sse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RegistryType {
    Npm,
    Pypi,
    Oci,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Remote {
    #[serde(rename = "type")]
    pub transport: Transport,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Package {
    #[serde(rename = "registryType")]
    pub registry_type: RegistryType,
    pub identifier: String,
    /// An exact version: never `latest` or a range.
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Curation {
    pub trust: Trust,
    /// `YYYY-MM-DD` of the human review.
    #[serde(rename = "reviewedAt")]
    pub reviewed_at: String,
    /// Lowercase hex SHA-256 of the reviewed `tools/list` snapshot, when one was taken.
    #[serde(
        rename = "toolSnapshotSha256",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub tool_snapshot_sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntryMeta {
    #[serde(rename = "ai.citrate/curation")]
    pub curation: Curation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerEntry {
    /// The official registry name, `<reverse-dns namespace>/<server>`.
    pub name: String,
    pub title: String,
    pub description: String,
    pub version: String,
    #[serde(default)]
    pub remotes: Vec<Remote>,
    #[serde(default)]
    pub packages: Vec<Package>,
    #[serde(rename = "_meta")]
    pub meta: EntryMeta,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Subregistry {
    pub schema: u32,
    pub sequence: u64,
    /// Unix seconds.
    pub issued_at: u64,
    /// Unix seconds; refused from this instant on.
    pub expires_at: u64,
    pub servers: Vec<ServerEntry>,
}

/// The last subregistry this machine accepted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeenSubregistry {
    pub sequence: u64,
    /// SHA-256 of the document bytes, hex.
    pub digest_hex: String,
}

/// A subregistry whose size, signature, fields, freshness and sequence all checked out. Only
/// [`verify_subregistry`] makes one.
///
/// ```compile_fail
/// use citrate_components::mcp_subregistry::{Subregistry, VerifiedSubregistry};
/// fn forge(d: Subregistry) -> VerifiedSubregistry {
///     VerifiedSubregistry { document: d, digest_hex: String::new() }
/// }
/// ```
#[derive(Debug, Clone)]
pub struct VerifiedSubregistry {
    document: Subregistry,
    digest_hex: String,
}

impl VerifiedSubregistry {
    pub fn document(&self) -> &Subregistry {
        &self.document
    }

    pub fn digest_hex(&self) -> &str {
        &self.digest_hex
    }

    pub fn seen(&self) -> SeenSubregistry {
        SeenSubregistry {
            sequence: self.document.sequence,
            digest_hex: self.digest_hex.clone(),
        }
    }
}

fn malformed(m: impl Into<String>) -> SubregistryError {
    SubregistryError::Malformed(m.into())
}

fn rollback_check(seen: &SeenSubregistry, got: &SeenSubregistry) -> Result<(), SubregistryError> {
    if got.sequence < seen.sequence
        || (got.sequence == seen.sequence && got.digest_hex != seen.digest_hex)
    {
        return Err(SubregistryError::Rollback {
            seen: seen.sequence,
            got: got.sequence,
        });
    }
    Ok(())
}

/// Verifies `bytes` against the detached minisign signature `sig_text`.
///
/// Order: size cap, signature (key, prehash, domain), then JSON, fields, freshness and sequence.
pub fn verify_subregistry(
    bytes: &[u8],
    sig_text: &str,
    root: &TrustRoot,
    last_seen: Option<&SeenSubregistry>,
    now: u64,
) -> Result<VerifiedSubregistry, SubregistryError> {
    if bytes.len() > MAX_SUBREGISTRY_BYTES {
        return Err(SubregistryError::TooLarge {
            limit: MAX_SUBREGISTRY_BYTES,
        });
    }
    verify_detached(root, sig_text, bytes, SUBREGISTRY_DOMAIN).map_err(from_component)?;
    let document: Subregistry =
        serde_json::from_slice(bytes).map_err(|e| malformed(format!("json: {e}")))?;
    validate(&document)?;
    if document.issued_at > now.saturating_add(CLOCK_SKEW_SECS) {
        return Err(SubregistryError::NotYetValid);
    }
    if now >= document.expires_at {
        return Err(SubregistryError::Expired);
    }
    let verified = VerifiedSubregistry {
        digest_hex: hex::encode(sha2::Sha256::digest(bytes)),
        document,
    };
    if let Some(seen) = last_seen {
        rollback_check(seen, &verified.seen())?;
    }
    Ok(verified)
}

/// Reads the persisted last-seen record. A missing file is `None` (first run); a file that does
/// not parse is [`SubregistryError::State`], never treated as missing.
pub fn load_seen(path: &Path) -> Result<Option<SeenSubregistry>, SubregistryError> {
    match std::fs::read(path) {
        Ok(b) => serde_json::from_slice(&b)
            .map(Some)
            .map_err(|e| SubregistryError::State(format!("{}: {e}", path.display()))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(SubregistryError::State(format!("{}: {e}", path.display()))),
    }
}

/// Persists `seen` (write to a sibling temp file, then rename), refusing to lower the record.
pub fn record_seen(path: &Path, seen: &SeenSubregistry) -> Result<(), SubregistryError> {
    if let Some(prev) = load_seen(path)? {
        rollback_check(&prev, seen)?;
    }
    let state = |e: std::io::Error| SubregistryError::State(format!("{}: {e}", path.display()));
    let json = serde_json::to_vec(seen).map_err(|e| SubregistryError::State(e.to_string()))?;
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = std::path::PathBuf::from(tmp);
    std::fs::write(&tmp, json).map_err(state)?;
    std::fs::rename(&tmp, path).map_err(state)
}

/// [`verify_subregistry`] against the record at `seen_path`, recording the accepted document
/// there. A refused document is never recorded, and a refusal never lowers the record.
pub fn verify_and_record(
    bytes: &[u8],
    sig_text: &str,
    root: &TrustRoot,
    seen_path: &Path,
    now: u64,
) -> Result<VerifiedSubregistry, SubregistryError> {
    let last = load_seen(seen_path)?;
    let verified = verify_subregistry(bytes, sig_text, root, last.as_ref(), now)?;
    record_seen(seen_path, &verified.seen())?;
    Ok(verified)
}

/// `<namespace>/<server>`: a reverse-DNS namespace (letters, digits, `.` and `-`, starting with a
/// letter or digit), one `/`, then letters, digits, `.`, `_` and `-`.
fn is_registry_name(s: &str) -> bool {
    let Some((ns, server)) = s.split_once('/') else {
        return false;
    };
    let ns_ok = ns
        .bytes()
        .next()
        .is_some_and(|c| c.is_ascii_alphanumeric())
        && ns
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'-'));
    let server_ok = !server.is_empty()
        && server
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-'));
    s.len() <= MAX_NAME && ns_ok && server_ok
}

/// An exact version: letters, digits, `.`, `+`, `-`, `_`; not `latest`, not starting with `.`.
fn is_pinned_version(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && !s.eq_ignore_ascii_case("latest")
        && s.bytes().next().is_some_and(|c| c.is_ascii_alphanumeric())
        && s
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'+' | b'-' | b'_'))
}

/// An npm, PyPI or OCI identifier: no spaces, quotes, shell metacharacters or `..`.
fn is_package_identifier(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 214
        && !s.contains("..")
        && s.bytes().next().is_some_and(|c| c.is_ascii_alphanumeric() || c == b'@')
        && s.bytes().all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, b'@' | b'/' | b'.' | b'_' | b'-' | b':')
        })
}

fn is_date(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b.iter()
            .enumerate()
            .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
}

fn is_sha256_hex(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

fn text_ok(s: &str, max: usize, multiline: bool) -> bool {
    !s.trim().is_empty()
        && s.chars().count() <= max
        && !s
            .chars()
            .any(|c| c.is_control() && !(multiline && c == '\n'))
}

fn validate_entry(e: &ServerEntry) -> Result<(), SubregistryError> {
    let what = e.name.as_str();
    if !text_ok(&e.title, 80, false) {
        return Err(malformed(format!("{what}: title")));
    }
    if !text_ok(&e.description, 600, true) {
        return Err(malformed(format!("{what}: description")));
    }
    if !is_pinned_version(&e.version) {
        return Err(malformed(format!("{what}: version {:?}", e.version)));
    }
    if e.remotes.is_empty() && e.packages.is_empty() {
        return Err(malformed(format!(
            "{what}: an entry needs a remote or a package"
        )));
    }
    if e.remotes.len() > MAX_REMOTES || e.packages.len() > MAX_PACKAGES {
        return Err(malformed(format!("{what}: too many remotes or packages")));
    }
    for r in &e.remotes {
        if !is_https_url(&r.url) {
            return Err(malformed(format!(
                "{what}: remote url must be https with a host"
            )));
        }
    }
    for p in &e.packages {
        if !is_package_identifier(&p.identifier) {
            return Err(malformed(format!("{what}: package identifier")));
        }
        if !is_pinned_version(&p.version) {
            return Err(malformed(format!(
                "{what}: package {} must be pinned to an exact version",
                p.identifier
            )));
        }
    }
    let c = &e.meta.curation;
    if !is_date(&c.reviewed_at) {
        return Err(malformed(format!("{what}: reviewedAt must be YYYY-MM-DD")));
    }
    if let Some(h) = &c.tool_snapshot_sha256 {
        if !is_sha256_hex(h) {
            return Err(malformed(format!(
                "{what}: toolSnapshotSha256 must be 64 lowercase hex"
            )));
        }
    }
    Ok(())
}

/// Field rules. Every rule has a test in `tests/mcp_subregistry.rs`.
pub fn validate(d: &Subregistry) -> Result<(), SubregistryError> {
    if d.schema != SUBREGISTRY_SCHEMA {
        return Err(malformed(format!(
            "schema {} (expected {SUBREGISTRY_SCHEMA})",
            d.schema
        )));
    }
    if d.expires_at <= d.issued_at {
        return Err(malformed("expires_at is not after issued_at"));
    }
    if d.expires_at - d.issued_at > MAX_SUBREGISTRY_LIFETIME_SECS {
        return Err(malformed("valid for longer than the maximum lifetime"));
    }
    if d.servers.len() > MAX_SERVERS {
        return Err(malformed("too many servers"));
    }
    let mut names = BTreeSet::new();
    for e in &d.servers {
        if !is_registry_name(&e.name) {
            return Err(malformed(format!("server name {:?}", e.name)));
        }
        if !names.insert(e.name.as_str()) {
            return Err(malformed(format!("duplicate server {}", e.name)));
        }
        validate_entry(e)?;
    }
    Ok(())
}
