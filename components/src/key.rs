//! The trust root: one pinned minisign public key.
//!
//! **@rule8.** The production component-signing key is created at a key ceremony and only its
//! public half is ever committed, into [`PRODUCTION_COMPONENT_PUBKEY`]. Until then the slot is
//! `None` and [`TrustRoot::production`] refuses, so the updater installs nothing. The key must
//! differ from the app-updater key in `src-tauri/tauri.conf.json` (key separation: a signature
//! that installs the app must never install a component); [`TrustRoot::production`] checks that.
use base64::Engine as _;
use sha2::Digest as _;

use crate::error::ComponentError;

/// The production component-signing public key: the base64 key line of a minisign `.pub` file
/// (the second line, starting `RW`). EMPTY until the @rule8 key ceremony.
pub const PRODUCTION_COMPONENT_PUBKEY: Option<&str> = None;

/// The app's own updater configuration, read for the key-separation check.
const TAURI_CONF: &str = include_str!("../../src-tauri/tauri.conf.json");

/// A parsed, pinned public key.
pub struct TrustRoot {
    pub(crate) key: minisign_verify::PublicKey,
    fingerprint: String,
}

impl std::fmt::Debug for TrustRoot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TrustRoot")
            .field("fingerprint", &self.fingerprint)
            .finish()
    }
}

impl TrustRoot {
    /// The production trust root, or [`ComponentError::KeyNotConfigured`] while the slot is
    /// empty.
    pub fn production() -> Result<Self, ComponentError> {
        let key = PRODUCTION_COMPONENT_PUBKEY.ok_or(ComponentError::KeyNotConfigured)?;
        let app_key = app_updater_key_line()?;
        Self::from_base64_checked(key, &[app_key.as_str()])
    }

    /// Parses a minisign public key line (base64 of `Ed` ‖ key id ‖ Ed25519 public key).
    pub fn from_base64(b64: &str) -> Result<Self, ComponentError> {
        let line = b64.trim();
        if line.is_empty() {
            return Err(ComponentError::BadKey("empty".into()));
        }
        let key = minisign_verify::PublicKey::from_base64(line)
            .map_err(|e| ComponentError::BadKey(e.to_string()))?;
        let fingerprint = hex::encode(&sha2::Sha256::digest(line.as_bytes())[..8]);
        Ok(Self { key, fingerprint })
    }

    /// [`Self::from_base64`], refusing any key in `forbidden` (compared as trimmed key lines).
    pub fn from_base64_checked(b64: &str, forbidden: &[&str]) -> Result<Self, ComponentError> {
        let line = b64.trim();
        if forbidden.iter().any(|f| f.trim() == line) {
            return Err(ComponentError::BadKey(
                "the component key must not be the app-updater key".into(),
            ));
        }
        Self::from_base64(line)
    }

    /// The first 8 bytes of SHA-256 over the key line, hex: a short, stable display id.
    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }
}

/// The app-updater public key line from `tauri.conf.json` (`plugins.updater.pubkey` is the
/// base64 of a whole minisign `.pub` file; its second line is the key).
pub fn app_updater_key_line() -> Result<String, ComponentError> {
    let v: serde_json::Value = serde_json::from_str(TAURI_CONF)
        .map_err(|e| ComponentError::BadKey(format!("tauri.conf.json: {e}")))?;
    let b64 = v
        .get("plugins")
        .and_then(|p| p.get("updater"))
        .and_then(|u| u.get("pubkey"))
        .and_then(|k| k.as_str())
        .ok_or_else(|| ComponentError::BadKey("tauri.conf.json has no updater pubkey".into()))?;
    let file = base64::engine::general_purpose::STANDARD
        .decode(b64.trim())
        .map_err(|_| ComponentError::BadKey("the app-updater pubkey is not base64".into()))?;
    let text = String::from_utf8(file)
        .map_err(|_| ComponentError::BadKey("the app-updater pubkey is not text".into()))?;
    text.lines()
        .nth(1)
        .map(|l| l.trim().to_string())
        .ok_or_else(|| ComponentError::BadKey("the app-updater pubkey has no key line".into()))
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_app_updater_key_is_readable() {
        let line = super::app_updater_key_line().expect("tauri.conf.json carries the updater key");
        assert!(line.starts_with("RW"), "{line}");
    }
}
