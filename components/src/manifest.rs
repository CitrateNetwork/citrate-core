//! The signed component manifest.
//!
//! A manifest is a JSON document plus a detached minisign signature over its exact bytes. The
//! signature must be prehashed (`ED`, BLAKE2b-512), made by the pinned key, and carry a trusted
//! comment that starts with [`MANIFEST_DOMAIN`]; each artifact has its own signature whose
//! trusted comment starts with [`ARTIFACT_DOMAIN`]. The domains keep a signature made for one
//! purpose from passing as the other.
//!
//! Replay and freeze protection: `sequence` must never go down (a lower sequence is a rollback;
//! the same sequence is accepted only for the same bytes), `expires_at` bounds how long a
//! manifest is trusted, and the lifetime itself is capped.
use std::collections::BTreeMap;
use std::io::Read;

use serde::{Deserialize, Serialize};
use sha2::Digest as _;

use crate::error::ComponentError;
use crate::extract::safe_relative_path;
use crate::key::TrustRoot;
use crate::platform::Platform;

pub const MANIFEST_SCHEMA: u32 = 1;
/// Trusted-comment domain of a manifest signature.
pub const MANIFEST_DOMAIN: &str = "citrate-components-manifest";
/// Trusted-comment domain of an artifact signature.
pub const ARTIFACT_DOMAIN: &str = "citrate-components-artifact";
pub const MAX_MANIFEST_BYTES: usize = 1 << 20;
/// The largest artifact the updater downloads (a browser build is a few hundred MB).
pub const MAX_ARTIFACT_BYTES: u64 = 2 << 30;
/// The longest a manifest may be valid for. Pending owner sign-off (placeholder value).
pub const MAX_MANIFEST_LIFETIME_SECS: u64 = 31 * 86_400;
/// How far `issued_at` may be ahead of the local clock.
pub const CLOCK_SKEW_SECS: u64 = 600;
const MAX_COMPONENTS: usize = 64;
const MAX_ENTRYPOINTS: usize = 32;
const MAX_SIGNATURE_CHARS: usize = 1024;
const MAX_URL_CHARS: usize = 2048;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArchiveFormat {
    /// A single file, installed under the component's name.
    #[serde(rename = "raw")]
    Raw,
    #[serde(rename = "tar.gz")]
    TarGz,
    #[serde(rename = "tar.xz")]
    TarXz,
    /// Recognised so the bundle can name it; the installer refuses it in this version.
    #[serde(rename = "zip")]
    Zip,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ComponentKind {
    Browser,
    Toolchain,
    Search,
    Skills,
    DocsGraph,
    Library,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Artifact {
    pub url: String,
    pub sha256: String,
    pub size: u64,
    pub format: ArchiveFormat,
    /// A minisign signature (the whole `.minisig` text) over the artifact bytes.
    pub signature: String,
    /// Overrides the component's entrypoints for this platform (archives whose top-level
    /// directory names the platform).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entrypoints: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Component {
    pub name: String,
    pub version: String,
    pub kind: ComponentKind,
    /// SPDX expression of the upstream licence.
    pub license: String,
    /// Paths (relative to the unpacked tree) that must exist after unpacking.
    #[serde(default)]
    pub entrypoints: Vec<String>,
    /// Platform key -> artifact.
    pub artifacts: BTreeMap<String, Artifact>,
}

impl Component {
    /// The artifact for `platform`, falling back to a platform-independent one.
    pub fn artifact_for(&self, platform: Platform) -> Option<(Platform, &Artifact)> {
        self.artifacts
            .get(platform.as_str())
            .map(|a| (platform, a))
            .or_else(|| {
                self.artifacts
                    .get(Platform::Any.as_str())
                    .map(|a| (Platform::Any, a))
            })
    }

    /// The entrypoints that apply to `artifact`.
    pub fn entrypoints_for<'a>(&'a self, artifact: &'a Artifact) -> &'a [String] {
        artifact.entrypoints.as_deref().unwrap_or(&self.entrypoints)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema: u32,
    pub channel: String,
    pub sequence: u64,
    /// Unix seconds.
    pub issued_at: u64,
    /// Unix seconds; the manifest is refused from this instant on.
    pub expires_at: u64,
    pub components: Vec<Component>,
}

impl Manifest {
    pub fn component(&self, name: &str) -> Option<&Component> {
        self.components.iter().find(|c| c.name == name)
    }
}

/// The last manifest this machine accepted (kept in the store's state file).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeenManifest {
    pub sequence: u64,
    /// SHA-256 of the manifest bytes, hex.
    pub digest_hex: String,
    /// When this machine verified it (unix seconds).
    pub verified_at: u64,
    pub expires_at: u64,
}

/// A manifest whose signature, freshness, sequence and fields all checked out.
///
/// Only [`verify_manifest`] makes one: the fields are private, so a value built by hand cannot
/// skip verification on its way to the installer.
///
/// ```compile_fail
/// use citrate_components::manifest::{Manifest, VerifiedManifest};
/// fn forge(m: Manifest) -> VerifiedManifest {
///     VerifiedManifest { manifest: m, digest_hex: String::new(), verified_at: 0 }
/// }
/// ```
#[derive(Debug, Clone)]
pub struct VerifiedManifest {
    manifest: Manifest,
    digest_hex: String,
    verified_at: u64,
}

impl VerifiedManifest {
    /// The verified manifest.
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// SHA-256 of the manifest bytes that verified, hex.
    pub fn digest_hex(&self) -> &str {
        &self.digest_hex
    }

    /// When this machine verified it (unix seconds).
    pub fn verified_at(&self) -> u64 {
        self.verified_at
    }

    pub fn seen(&self) -> SeenManifest {
        SeenManifest {
            sequence: self.manifest.sequence,
            digest_hex: self.digest_hex.clone(),
            verified_at: self.verified_at,
            expires_at: self.manifest.expires_at,
        }
    }
}

/// Verifies `bytes` against the detached minisign signature `sig_text`.
///
/// Order: size cap, signature (key, prehash, trusted-comment domain), then JSON, fields,
/// freshness and sequence. Nothing in the JSON is looked at before the signature verifies.
pub fn verify_manifest(
    bytes: &[u8],
    sig_text: &str,
    root: &TrustRoot,
    last_seen: Option<&SeenManifest>,
    now: u64,
) -> Result<VerifiedManifest, ComponentError> {
    if bytes.len() > MAX_MANIFEST_BYTES {
        return Err(malformed(format!("larger than {MAX_MANIFEST_BYTES} bytes")));
    }
    verify_detached(root, sig_text, bytes, MANIFEST_DOMAIN)?;
    let manifest: Manifest =
        serde_json::from_slice(bytes).map_err(|e| malformed(format!("json: {e}")))?;
    validate(&manifest)?;
    if manifest.issued_at > now.saturating_add(CLOCK_SKEW_SECS) {
        return Err(ComponentError::ManifestNotYetValid);
    }
    if now >= manifest.expires_at {
        return Err(ComponentError::ManifestExpired);
    }
    let digest_hex = hex::encode(sha2::Sha256::digest(bytes));
    check_sequence(last_seen, manifest.sequence, &digest_hex)?;
    Ok(VerifiedManifest {
        manifest,
        digest_hex,
        verified_at: now,
    })
}

/// A lower sequence is a rollback; the same sequence must be the same bytes.
pub(crate) fn check_sequence(
    last_seen: Option<&SeenManifest>,
    sequence: u64,
    digest_hex: &str,
) -> Result<(), ComponentError> {
    if let Some(seen) = last_seen {
        if sequence < seen.sequence || (sequence == seen.sequence && digest_hex != seen.digest_hex)
        {
            return Err(ComponentError::ManifestRollback {
                seen: seen.sequence,
                got: sequence,
            });
        }
    }
    Ok(())
}

fn malformed(m: impl Into<String>) -> ComponentError {
    ComponentError::ManifestMalformed(m.into())
}

/// Decodes a minisign signature and checks that it is prehashed (the only kind accepted).
pub(crate) fn decode_signature(
    sig_text: &str,
) -> Result<minisign_verify::Signature, ComponentError> {
    if sig_text.len() > MAX_SIGNATURE_CHARS {
        return Err(ComponentError::SignatureInvalid(
            "signature text too long".into(),
        ));
    }
    minisign_verify::Signature::decode(sig_text)
        .map_err(|e| ComponentError::SignatureInvalid(e.to_string()))
}

fn domain_ok(sig: &minisign_verify::Signature, domain: &str) -> bool {
    let tc = sig.trusted_comment();
    match tc.strip_prefix(domain) {
        Some(rest) => rest.is_empty() || rest.starts_with(' ') || rest.starts_with('\t'),
        None => false,
    }
}

/// Verifies a detached signature over in-memory bytes (prehashed only), then its domain.
pub(crate) fn verify_detached(
    root: &TrustRoot,
    sig_text: &str,
    data: &[u8],
    domain: &str,
) -> Result<(), ComponentError> {
    let sig = decode_signature(sig_text)?;
    root.key
        .verify(data, &sig, false)
        .map_err(|e| ComponentError::SignatureInvalid(e.to_string()))?;
    if !domain_ok(&sig, domain) {
        return Err(ComponentError::WrongTrustedComment);
    }
    Ok(())
}

/// Streams `reader` once, returning (bytes read, SHA-256 hex) and verifying the prehashed
/// signature over the same bytes, then its domain.
pub(crate) fn verify_stream(
    root: &TrustRoot,
    sig_text: &str,
    mut reader: impl Read,
    domain: &str,
) -> Result<(u64, String), ComponentError> {
    let sig = decode_signature(sig_text)?;
    let mut v = root
        .key
        .verify_stream(&sig)
        .map_err(|e| ComponentError::SignatureInvalid(e.to_string()))?;
    let mut sha = sha2::Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    let mut n: u64 = 0;
    loop {
        let k = reader.read(&mut buf).map_err(crate::error::io)?;
        if k == 0 {
            break;
        }
        let chunk = buf.get(..k).unwrap_or(&[]);
        v.update(chunk);
        sha.update(chunk);
        n = n.saturating_add(k as u64);
    }
    let digest = hex::encode(sha.finalize());
    v.finalize()
        .map_err(|e| ComponentError::SignatureInvalid(e.to_string()))?;
    if !domain_ok(&sig, domain) {
        return Err(ComponentError::WrongTrustedComment);
    }
    Ok((n, digest))
}

fn is_name(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty()
        && b.len() <= 48
        && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit())
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
}

fn is_version(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && !s.starts_with('.')
        && s.bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'+' | b'-' | b'_'))
}

fn is_channel(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 32
        && s.bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
}

fn is_sha256_hex(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

/// `https://host/...` with a non-empty host, printable ASCII, no whitespace.
pub(crate) fn is_https_url(s: &str) -> bool {
    if s.len() > MAX_URL_CHARS || !s.bytes().all(|c| c.is_ascii_graphic()) {
        return false;
    }
    match s.strip_prefix("https://") {
        Some(rest) => {
            let host = rest.split(['/', '?', '#']).next().unwrap_or("");
            !host.is_empty() && !host.contains('@')
        }
        None => false,
    }
}

fn check_entrypoints(eps: &[String], what: &str) -> Result<(), ComponentError> {
    if eps.len() > MAX_ENTRYPOINTS {
        return Err(malformed(format!("{what}: too many entrypoints")));
    }
    for e in eps {
        safe_relative_path(e).map_err(|_| {
            malformed(format!(
                "{what}: entrypoint {e:?} is not a safe relative path"
            ))
        })?;
    }
    Ok(())
}

/// Field rules. Every rule has a test in `tests/manifest.rs`.
pub fn validate(m: &Manifest) -> Result<(), ComponentError> {
    if m.schema != MANIFEST_SCHEMA {
        return Err(malformed(format!(
            "schema {} (expected {MANIFEST_SCHEMA})",
            m.schema
        )));
    }
    if !is_channel(&m.channel) {
        return Err(malformed("channel"));
    }
    if m.expires_at <= m.issued_at {
        return Err(malformed("expires_at is not after issued_at"));
    }
    if m.expires_at - m.issued_at > MAX_MANIFEST_LIFETIME_SECS {
        return Err(malformed("valid for longer than the maximum lifetime"));
    }
    if m.components.is_empty() || m.components.len() > MAX_COMPONENTS {
        return Err(malformed("component count"));
    }
    let mut names = std::collections::BTreeSet::new();
    for c in &m.components {
        if !is_name(&c.name) {
            return Err(malformed(format!("component name {:?}", c.name)));
        }
        if !names.insert(c.name.as_str()) {
            return Err(malformed(format!("duplicate component {}", c.name)));
        }
        if !is_version(&c.version) {
            return Err(malformed(format!("{}: version {:?}", c.name, c.version)));
        }
        if c.license.trim().is_empty()
            || c.license.len() > 128
            || !c.license.bytes().all(|b| b.is_ascii_graphic() || b == b' ')
        {
            return Err(malformed(format!("{}: licence", c.name)));
        }
        check_entrypoints(&c.entrypoints, &c.name)?;
        if c.artifacts.is_empty() {
            return Err(malformed(format!("{}: no artifacts", c.name)));
        }
        for (plat, a) in &c.artifacts {
            let what = format!("{} {plat}", c.name);
            if Platform::parse(plat).is_none() {
                return Err(malformed(format!("{what}: unknown platform")));
            }
            if !is_https_url(&a.url) {
                return Err(malformed(format!("{what}: url must be https with a host")));
            }
            if !is_sha256_hex(&a.sha256) {
                return Err(malformed(format!(
                    "{what}: sha256 must be 64 lowercase hex"
                )));
            }
            if a.size == 0 || a.size > MAX_ARTIFACT_BYTES {
                return Err(malformed(format!("{what}: size")));
            }
            decode_signature(&a.signature)
                .map_err(|_| malformed(format!("{what}: signature does not decode")))?;
            if let Some(eps) = &a.entrypoints {
                check_entrypoints(eps, &what)?;
            }
        }
    }
    Ok(())
}
