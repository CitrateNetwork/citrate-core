//! K5a: the signed Store catalog.
//!
//! The catalog is a JSON document plus a detached minisign signature over its exact bytes, made
//! with the catalog key from the genesis key ceremony. It uses the same verification core as the
//! component manifest (prehashed Ed25519 only, a trusted-comment domain, size cap first, nothing
//! in the JSON read before the signature verifies, anti-rollback by sequence, bounded lifetime),
//! but its own key slot and its own domain: a component signature never passes as a catalog
//! signature, and the reverse.
//!
//! **Key slot.** [`PRODUCTION_CATALOG_PUBKEY`] is EMPTY until the ceremony, so
//! [`catalog_trust_root`] refuses with [`CatalogError::KeyNotIssued`] and the Store lists
//! nothing. The key must differ from the app-updater key and from the component key.
//!
//! **Schema v1.** Listing kinds are Apps, Daemons, Widgets, Skills, Models and MCP servers. Each
//! listing names one artifact (https URL, SHA-256, size), the permissions it declares, a budget
//! (required for daemons, with spend fixed at "0"), the membership tier it needs, and whether it
//! is first-party. Only first-party listings are accepted in this version; third-party
//! submissions come after mainnet with a review step.
//!
//! Rule 3: nothing here holds a private key or signs.
use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use sha2::Digest as _;

use crate::error::ComponentError;
use crate::key::{
    app_updater_key_line, is_same_key_as_any, TrustRoot, PRODUCTION_COMPONENT_PUBKEY,
};
use crate::manifest::{is_https_url, verify_detached};

pub const CATALOG_SCHEMA: u32 = 1;
/// Trusted-comment domain of a catalog signature.
pub const CATALOG_DOMAIN: &str = "citrate-store-catalog";
pub const MAX_CATALOG_BYTES: usize = 1 << 20;
/// The longest a catalog may be valid for. Pending owner sign-off (placeholder value).
pub const MAX_CATALOG_LIFETIME_SECS: u64 = 31 * 86_400;
/// How far `issued_at` may be ahead of the local clock.
pub const CLOCK_SKEW_SECS: u64 = 600;
/// The largest artifact a listing may name (a model file is a few GB).
pub const MAX_LISTING_ARTIFACT_BYTES: u64 = 64 << 30;
const MAX_LISTINGS: usize = 256;
const MAX_PERMISSIONS: usize = 24;
const MAX_TEXT: usize = 600;

/// The production Store catalog public key: the base64 key line of a minisign `.pub` file.
/// EMPTY until the genesis key ceremony.
pub const PRODUCTION_CATALOG_PUBKEY: Option<&str> = None;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CatalogError {
    /// The production key slot is empty (the key ceremony has not happened).
    KeyNotIssued,
    /// The configured key does not parse, or is a key the catalog must not trust.
    BadKey(String),
    /// The signature does not decode, is the wrong algorithm, or does not verify.
    SignatureInvalid(String),
    /// A valid signature made for another purpose.
    WrongPurpose,
    /// Signed, but a field is out of bounds.
    Malformed(String),
    Expired,
    NotYetValid,
    /// Older than the catalog already seen.
    Rollback {
        seen: u64,
        got: u64,
    },
}

impl std::fmt::Display for CatalogError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use CatalogError::*;
        match self {
            KeyNotIssued => f.write_str("Store catalog not available yet: signing key not issued"),
            BadKey(m) => write!(f, "the Store catalog key is not usable: {m}"),
            SignatureInvalid(m) => write!(f, "the Store catalog signature did not verify: {m}"),
            WrongPurpose => f.write_str("the Store catalog was signed for a different purpose"),
            Malformed(m) => write!(f, "the Store catalog is not valid: {m}"),
            Expired => f.write_str("the Store catalog has expired"),
            NotYetValid => f.write_str("the Store catalog is not valid yet (check the clock)"),
            Rollback { seen, got } => write!(
                f,
                "the Store catalog is older than the one already seen (sequence {got} < {seen})"
            ),
        }
    }
}

impl std::error::Error for CatalogError {}

fn from_component(e: ComponentError) -> CatalogError {
    match e {
        ComponentError::KeyNotConfigured => CatalogError::KeyNotIssued,
        ComponentError::BadKey(m) => CatalogError::BadKey(m),
        ComponentError::SignatureInvalid(m) => CatalogError::SignatureInvalid(m),
        ComponentError::WrongTrustedComment => CatalogError::WrongPurpose,
        other => CatalogError::Malformed(other.to_string()),
    }
}

/// The production catalog trust root, or [`CatalogError::KeyNotIssued`] while the slot is empty.
pub fn catalog_trust_root() -> Result<TrustRoot, CatalogError> {
    catalog_trust_root_from(PRODUCTION_CATALOG_PUBKEY)
}

/// [`catalog_trust_root`] over a given slot, refusing the app-updater key and the production
/// component key (when that slot is set).
pub fn catalog_trust_root_from(slot: Option<&str>) -> Result<TrustRoot, CatalogError> {
    // An empty slot is refused before anything else is read.
    if slot.is_none() {
        return Err(CatalogError::KeyNotIssued);
    }
    let app_key = app_updater_key_line().map_err(from_component)?;
    let mut forbidden = vec![app_key.as_str()];
    if let Some(c) = PRODUCTION_COMPONENT_PUBKEY {
        forbidden.push(c);
    }
    catalog_trust_root_with(slot, &forbidden)
}

/// The key-separation rule over an explicit forbidden list (the seam that lets the component-key
/// branch be tested before the ceremony fills that slot). Keys are compared DECODED: the same
/// Ed25519 public key under another key id is still the same key.
pub fn catalog_trust_root_with(
    slot: Option<&str>,
    forbidden: &[&str],
) -> Result<TrustRoot, CatalogError> {
    let key = slot.ok_or(CatalogError::KeyNotIssued)?;
    if is_same_key_as_any(key, forbidden) {
        return Err(CatalogError::BadKey(
            "the catalog key must not be the app-updater key or the component key".into(),
        ));
    }
    TrustRoot::from_base64(key).map_err(from_component)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ListingKind {
    App,
    Daemon,
    Widget,
    Skill,
    Model,
    McpServer,
}

impl ListingKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ListingKind::App => "app",
            ListingKind::Daemon => "daemon",
            ListingKind::Widget => "widget",
            ListingKind::Skill => "skill",
            ListingKind::Model => "model",
            ListingKind::McpServer => "mcp_server",
        }
    }
}

/// The membership tier a listing needs. A closed set: an unknown tier fails the whole catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ListingTier {
    Public,
    Member,
    Enterprise,
}

impl ListingTier {
    pub fn as_str(self) -> &'static str {
        match self {
            ListingTier::Public => "public",
            ListingTier::Member => "member",
            ListingTier::Enterprise => "enterprise",
        }
    }

    /// The rank a member needs to reach this tier (public 0, member 1, enterprise 2).
    pub fn rank(self) -> i32 {
        match self {
            ListingTier::Public => 0,
            ListingTier::Member => 1,
            ListingTier::Enterprise => 2,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListingArtifact {
    pub url: String,
    /// Lowercase hex SHA-256 of the artifact bytes.
    pub sha256: String,
    pub size: u64,
}

/// One declared permission: a plain token (`local_model`, `web_search`, ...) and why the listing
/// needs it, in words shown to the member before they approve.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeclaredPermission {
    pub id: String,
    pub reason: String,
}

/// A declared budget (the daemon budget shape). Spend is "0" in this version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeclaredBudget {
    pub max_runs_per_day: u32,
    pub max_tokens_per_day: u32,
    pub max_tokens_per_run: u32,
    pub max_spend_salt: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Listing {
    pub id: String,
    pub kind: ListingKind,
    pub name: String,
    pub summary: String,
    pub version: String,
    pub publisher: String,
    pub first_party: bool,
    pub tier: ListingTier,
    pub artifact: ListingArtifact,
    #[serde(default)]
    pub permissions: Vec<DeclaredPermission>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget: Option<DeclaredBudget>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    pub schema: u32,
    pub sequence: u64,
    /// Unix seconds.
    pub issued_at: u64,
    /// Unix seconds; refused from this instant on.
    pub expires_at: u64,
    pub listings: Vec<Listing>,
}

impl Catalog {
    pub fn listing(&self, id: &str) -> Option<&Listing> {
        self.listings.iter().find(|l| l.id == id)
    }
}

/// The last catalog this machine accepted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeenCatalog {
    pub sequence: u64,
    /// SHA-256 of the catalog bytes, hex.
    pub digest_hex: String,
}

/// A catalog whose signature, freshness, sequence and fields all checked out. Only
/// [`verify_catalog`] makes one.
///
/// ```compile_fail
/// use citrate_components::catalog::{Catalog, VerifiedCatalog};
/// fn forge(c: Catalog) -> VerifiedCatalog {
///     VerifiedCatalog { catalog: c, digest_hex: String::new() }
/// }
/// ```
#[derive(Debug, Clone)]
pub struct VerifiedCatalog {
    catalog: Catalog,
    digest_hex: String,
}

impl VerifiedCatalog {
    pub fn catalog(&self) -> &Catalog {
        &self.catalog
    }

    pub fn digest_hex(&self) -> &str {
        &self.digest_hex
    }

    pub fn seen(&self) -> SeenCatalog {
        SeenCatalog {
            sequence: self.catalog.sequence,
            digest_hex: self.digest_hex.clone(),
        }
    }
}

fn malformed(m: impl Into<String>) -> CatalogError {
    CatalogError::Malformed(m.into())
}

/// Verifies `bytes` against the detached minisign signature `sig_text`.
///
/// Order: size cap, signature (key, prehash, domain), then JSON, fields, freshness and sequence.
pub fn verify_catalog(
    bytes: &[u8],
    sig_text: &str,
    root: &TrustRoot,
    last_seen: Option<&SeenCatalog>,
    now: u64,
) -> Result<VerifiedCatalog, CatalogError> {
    if bytes.len() > MAX_CATALOG_BYTES {
        return Err(malformed(format!("larger than {MAX_CATALOG_BYTES} bytes")));
    }
    verify_detached(root, sig_text, bytes, CATALOG_DOMAIN).map_err(from_component)?;
    let catalog: Catalog =
        serde_json::from_slice(bytes).map_err(|e| malformed(format!("json: {e}")))?;
    validate(&catalog)?;
    if catalog.issued_at > now.saturating_add(CLOCK_SKEW_SECS) {
        return Err(CatalogError::NotYetValid);
    }
    if now >= catalog.expires_at {
        return Err(CatalogError::Expired);
    }
    let digest_hex = hex::encode(sha2::Sha256::digest(bytes));
    if let Some(seen) = last_seen {
        if catalog.sequence < seen.sequence
            || (catalog.sequence == seen.sequence && digest_hex != seen.digest_hex)
        {
            return Err(CatalogError::Rollback {
                seen: seen.sequence,
                got: catalog.sequence,
            });
        }
    }
    Ok(VerifiedCatalog {
        catalog,
        digest_hex,
    })
}

/// A listing id: lowercase letters, digits and `-`, 1 to 64 characters, not starting with `-`.
pub fn is_listing_id(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty()
        && b.len() <= 64
        && b[0] != b'-'
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
}

fn is_permission_id(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty()
        && b.len() <= 48
        && b[0].is_ascii_lowercase()
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'_' | b'.'))
}

fn is_version(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && !s.starts_with('.')
        && s.bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'+' | b'-' | b'_'))
}

fn is_sha256_hex(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

fn text_ok(s: &str, max: usize) -> bool {
    let t = s.trim();
    !t.is_empty() && s.chars().count() <= max && !s.chars().any(|c| c.is_control() && c != '\n')
}

fn check_budget(b: &DeclaredBudget, what: &str) -> Result<(), CatalogError> {
    if b.max_spend_salt != "0" {
        return Err(malformed(format!(
            "{what}: the spend budget must be \"0\" in this version"
        )));
    }
    if b.max_runs_per_day == 0 || b.max_tokens_per_day == 0 || b.max_tokens_per_run == 0 {
        return Err(malformed(format!(
            "{what}: budget limits must be at least 1"
        )));
    }
    if b.max_tokens_per_run > b.max_tokens_per_day {
        return Err(malformed(format!(
            "{what}: budget tokens per run is more than tokens per day"
        )));
    }
    Ok(())
}

/// Field rules. Every rule has a test in `tests/catalog.rs`.
pub fn validate(c: &Catalog) -> Result<(), CatalogError> {
    if c.schema != CATALOG_SCHEMA {
        return Err(malformed(format!(
            "schema {} (expected {CATALOG_SCHEMA})",
            c.schema
        )));
    }
    if c.expires_at <= c.issued_at {
        return Err(malformed("expires_at is not after issued_at"));
    }
    if c.expires_at - c.issued_at > MAX_CATALOG_LIFETIME_SECS {
        return Err(malformed("valid for longer than the maximum lifetime"));
    }
    if c.listings.len() > MAX_LISTINGS {
        return Err(malformed("too many listings"));
    }
    let mut ids = BTreeSet::new();
    for l in &c.listings {
        if !is_listing_id(&l.id) {
            return Err(malformed(format!("listing id {:?}", l.id)));
        }
        if !ids.insert(l.id.as_str()) {
            return Err(malformed(format!("duplicate listing {}", l.id)));
        }
        let what = l.id.as_str();
        if !l.first_party {
            return Err(malformed(format!(
                "{what}: only first-party listings are accepted in this version"
            )));
        }
        if !text_ok(&l.name, 80) || l.name.contains('\n') {
            return Err(malformed(format!("{what}: name")));
        }
        if !text_ok(&l.summary, MAX_TEXT) {
            return Err(malformed(format!("{what}: summary")));
        }
        if !text_ok(&l.publisher, 80) || l.publisher.contains('\n') {
            return Err(malformed(format!("{what}: publisher")));
        }
        if !is_version(&l.version) {
            return Err(malformed(format!("{what}: version {:?}", l.version)));
        }
        if !is_https_url(&l.artifact.url) {
            return Err(malformed(format!(
                "{what}: artifact url must be https with a host"
            )));
        }
        if !is_sha256_hex(&l.artifact.sha256) {
            return Err(malformed(format!(
                "{what}: artifact sha256 must be 64 lowercase hex"
            )));
        }
        if l.artifact.size == 0 || l.artifact.size > MAX_LISTING_ARTIFACT_BYTES {
            return Err(malformed(format!("{what}: artifact size")));
        }
        if l.permissions.len() > MAX_PERMISSIONS {
            return Err(malformed(format!("{what}: too many permissions")));
        }
        let mut perms = BTreeSet::new();
        for p in &l.permissions {
            if !is_permission_id(&p.id) {
                return Err(malformed(format!("{what}: permission id {:?}", p.id)));
            }
            if !perms.insert(p.id.as_str()) {
                return Err(malformed(format!("{what}: duplicate permission {}", p.id)));
            }
            if !text_ok(&p.reason, 240) {
                return Err(malformed(format!(
                    "{what}: permission {} has no reason",
                    p.id
                )));
            }
        }
        match (&l.budget, l.kind) {
            (Some(b), _) => check_budget(b, what)?,
            (None, ListingKind::Daemon) => {
                return Err(malformed(format!("{what}: a daemon must declare a budget")))
            }
            (None, _) => {}
        }
    }
    Ok(())
}
