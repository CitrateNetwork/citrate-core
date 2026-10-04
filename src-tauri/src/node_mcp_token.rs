//! HUP-S4.2 — connect tokens for the citrate-node MCP server.
//!
//! A connect token is what an external agent (Claude Code, Cursor, another Hermes install) must
//! present on EVERY request to the node's MCP server. The member generates one in Settings and
//! gives it a label; the plaintext is shown ONCE and never stored. What persists is only
//! `SHA-256(token)` plus the label and timestamps, in a 0600 file under the app data dir. A token
//! is revoked by deleting its record, which takes effect on the very next request.
//!
//! A token is 32 bytes from the OS CSPRNG, hex-encoded behind the `cnmcp_` prefix, so a plain
//! SHA-256 (no salt, no stretching) is the right store: there is nothing to brute-force.
//! Verification compares against every live record in constant time per record.

use rand::rngs::OsRng;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Every connect token starts with this prefix (so a leaked one is recognisable in a scan).
pub const TOKEN_PREFIX: &str = "cnmcp_";
/// Random bytes in a token.
const TOKEN_BYTES: usize = 32;
/// The most live tokens a member can hold at once (one per client is the expected shape).
pub const MAX_TOKENS: usize = 16;
/// The most in-memory tokens live at once (HUP-S4.1: one per Hermes start; earlier ones are
/// revoked when a new one is minted).
pub const MAX_EPHEMERAL_TOKENS: usize = 4;
/// The longest label accepted (labels are shown in the approval UI and in ceremony origins).
pub const MAX_LABEL_CHARS: usize = 48;

/// One persisted token record. Holds the HASH only; the plaintext never touches disk.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TokenRecord {
    /// Public id (8 hex chars, random, unrelated to the token bytes). Shown in the UI and in logs.
    pub id: String,
    /// The member's label for the client ("Claude Code on this laptop").
    pub label: String,
    /// Lowercase hex SHA-256 of the full token string.
    pub sha256: String,
    /// Creation time (Unix ms).
    pub created_ms: u64,
}

/// The webview view of a token: no hash, no plaintext.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TokenView {
    pub id: String,
    pub label: String,
    pub created_ms: u64,
    /// Last successful use this app session (Unix ms), if any. Not persisted.
    pub last_used_ms: Option<u64>,
}

/// The result of issuing a token: the ONLY time the plaintext exists outside the client.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TokenIssued {
    pub id: String,
    pub label: String,
    pub created_ms: u64,
    /// The plaintext connect token. Shown once in Settings; never persisted.
    pub connect_token: String,
}

/// A token the server accepted: its id and label (used for request ownership and origins).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Authorized {
    pub id: String,
    pub label: String,
}

struct Inner {
    records: Vec<TokenRecord>,
    /// HUP-S4.1: tokens that live only in memory (the built-in Hermes entry's, minted at each
    /// Hermes start). Only their hash is held, and never on disk; an app restart ends them.
    ephemeral: Vec<TokenRecord>,
    /// id → last use (Unix ms), this session only.
    last_used: std::collections::BTreeMap<String, u64>,
}

/// The token store: the hashed records, optionally backed by a file.
pub struct TokenStore {
    path: Option<PathBuf>,
    inner: Mutex<Inner>,
}

fn sha256_hex(s: &str) -> String {
    hex::encode(Sha256::digest(s.as_bytes()))
}

/// Constant-time equality for two equal-length byte strings (length is public: always 64 hex).
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Validate and normalise a label: trimmed, 1..=48 chars, no control or bidi characters.
pub fn normalize_label(label: &str) -> Result<String, String> {
    let t = label.trim();
    if t.is_empty() {
        return Err("Give the token a label, such as the client it is for.".to_string());
    }
    if t.chars().count() > MAX_LABEL_CHARS {
        return Err(format!("Labels are at most {MAX_LABEL_CHARS} characters."));
    }
    if t.chars().any(crate::node_mcp_tools::is_unsafe_display_char) {
        return Err("Labels cannot contain control or text-direction characters.".to_string());
    }
    Ok(t.to_string())
}

impl TokenStore {
    /// An in-memory store (tests, and the fallback when the app data dir is unavailable).
    pub fn in_memory() -> Self {
        TokenStore {
            path: None,
            inner: Mutex::new(Inner {
                records: Vec::new(),
                ephemeral: Vec::new(),
                last_used: Default::default(),
            }),
        }
    }

    /// Load the store from `path`. A missing file is an empty store. An unreadable or malformed
    /// file also yields an empty store, which fails CLOSED: no client is accepted until the member
    /// issues a new token (which rewrites the file).
    pub fn load(path: PathBuf) -> Self {
        let records = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str::<Vec<TokenRecord>>(&s).ok())
            .unwrap_or_default();
        TokenStore {
            path: Some(path),
            inner: Mutex::new(Inner {
                records,
                ephemeral: Vec::new(),
                last_used: Default::default(),
            }),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn persist(&self, records: &[TokenRecord]) -> Result<(), String> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let body = serde_json::to_string_pretty(records).map_err(|e| e.to_string())?;
        write_private(path, body.as_bytes())
    }

    /// A fresh token and an id unused by any live record.
    fn mint(inner: &Inner, label: &str, now_ms: u64) -> (TokenRecord, String) {
        let mut secret = [0u8; TOKEN_BYTES];
        OsRng.fill_bytes(&mut secret);
        let token = format!("{TOKEN_PREFIX}{}", hex::encode(secret));
        secret.iter_mut().for_each(|b| *b = 0);
        let mut id_bytes = [0u8; 4];
        let id = loop {
            OsRng.fill_bytes(&mut id_bytes);
            let candidate = hex::encode(id_bytes);
            if !inner
                .records
                .iter()
                .chain(inner.ephemeral.iter())
                .any(|r| r.id == candidate)
            {
                break candidate;
            }
        };
        let record = TokenRecord {
            id,
            label: label.to_string(),
            sha256: sha256_hex(&token),
            created_ms: now_ms,
        };
        (record, token)
    }

    /// Issue a new token for `label`. Returns the plaintext once.
    pub fn issue(&self, label: &str, now_ms: u64) -> Result<TokenIssued, String> {
        let label = normalize_label(label)?;
        let mut inner = self.lock();
        if inner.records.len() >= MAX_TOKENS {
            return Err(format!(
                "You already have {MAX_TOKENS} connect tokens. Revoke one you no longer use first."
            ));
        }
        let (record, token) = Self::mint(&inner, &label, now_ms);
        let id = record.id.clone();
        let mut next = inner.records.clone();
        next.push(record);
        self.persist(&next)?;
        inner.records = next;
        Ok(TokenIssued {
            id,
            label,
            created_ms: now_ms,
            connect_token: token,
        })
    }

    /// HUP-S4.1: issue a token held in memory only (its hash; nothing is written to disk), for a
    /// client inside this app. It ends when it is revoked or the app exits.
    pub fn issue_ephemeral(&self, label: &str, now_ms: u64) -> Result<TokenIssued, String> {
        let label = normalize_label(label)?;
        let mut inner = self.lock();
        if inner.ephemeral.len() >= MAX_EPHEMERAL_TOKENS {
            return Err("too many in-app connect tokens are live".to_string());
        }
        let (record, token) = Self::mint(&inner, &label, now_ms);
        let id = record.id.clone();
        inner.ephemeral.push(record);
        Ok(TokenIssued {
            id,
            label,
            created_ms: now_ms,
            connect_token: token,
        })
    }

    /// HUP-S4.1: the ids of the in-memory tokens.
    pub fn ephemeral_ids(&self) -> Vec<String> {
        self.lock().ephemeral.iter().map(|r| r.id.clone()).collect()
    }

    /// HUP-S4.1: whether `presented` is a live in-memory token (does not mark it used).
    pub fn is_live_ephemeral(&self, presented: &str) -> bool {
        let digest = sha256_hex(presented);
        let inner = self.lock();
        let mut hit = false;
        for r in &inner.ephemeral {
            hit |= ct_eq(r.sha256.as_bytes(), digest.as_bytes());
        }
        hit
    }

    /// Check a presented token. Returns the matching record's id + label, or `None`. Every live
    /// record is compared (no early exit), each in constant time.
    pub fn verify(&self, presented: &str, now_ms: u64) -> Option<Authorized> {
        let expected_len = TOKEN_PREFIX.len() + TOKEN_BYTES * 2;
        if presented.len() != expected_len || !presented.starts_with(TOKEN_PREFIX) {
            return None;
        }
        let digest = sha256_hex(presented);
        let mut inner = self.lock();
        let mut hit: Option<Authorized> = None;
        for r in inner.records.iter().chain(inner.ephemeral.iter()) {
            if ct_eq(r.sha256.as_bytes(), digest.as_bytes()) && hit.is_none() {
                hit = Some(Authorized {
                    id: r.id.clone(),
                    label: r.label.clone(),
                });
            }
        }
        if let Some(a) = &hit {
            inner.last_used.insert(a.id.clone(), now_ms);
        }
        hit
    }

    /// Revoke (delete) the token `id`. Returns whether a token was removed.
    pub fn revoke(&self, id: &str) -> Result<bool, String> {
        let mut inner = self.lock();
        if let Some(i) = inner.ephemeral.iter().position(|r| r.id == id) {
            inner.ephemeral.remove(i);
            inner.last_used.remove(id);
            return Ok(true);
        }
        let before = inner.records.len();
        let next: Vec<TokenRecord> = inner
            .records
            .iter()
            .filter(|r| r.id != id)
            .cloned()
            .collect();
        if next.len() == before {
            return Ok(false);
        }
        self.persist(&next)?;
        inner.records = next;
        inner.last_used.remove(id);
        Ok(true)
    }

    /// The live tokens, oldest first, as webview-safe views.
    pub fn list(&self) -> Vec<TokenView> {
        let inner = self.lock();
        inner
            .records
            .iter()
            .chain(inner.ephemeral.iter())
            .map(|r| TokenView {
                id: r.id.clone(),
                label: r.label.clone(),
                created_ms: r.created_ms,
                last_used_ms: inner.last_used.get(&r.id).copied(),
            })
            .collect()
    }
}

/// Write `bytes` to `path` with owner-only permissions (0600 on Unix), via a temp file + rename so
/// a crash never leaves a half-written store.
pub fn write_private(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension("tmp");
    {
        use std::io::Write;
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f = opts.open(&tmp).map_err(|e| e.to_string())?;
        f.write_all(bytes).map_err(|e| e.to_string())?;
        f.sync_all().map_err(|e| e.to_string())?;
    }
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}
