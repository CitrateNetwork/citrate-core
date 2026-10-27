//! AMC-H1 (0.5.3; ADR-2026-10-26-byok-provider-governance P-11): the local, encrypted
//! conversation history and artifacts store.
//!
//! Conversations, their turns, tool calls and tool results, and the artifacts they produced stay on
//! this device, encrypted at rest under a device-sealed key. The member sets how long they are kept,
//! can export one in a readable form, and can delete one so that it cannot be read back. Nothing
//! here sends anything anywhere; sharing to citrate-memories is a separate, explicit member action
//! that this module does not offer.
//!
//! ## Engine: encrypted files, not a database
//!
//! The tree has no embedded database, and SQLite (rusqlite with its bundled C library) would be the
//! largest new audit surface in core for what is a small, append-mostly data set. More to the
//! point, a database does not make secure delete easier: freed pages, the WAL and the journal keep
//! old bytes, and on APFS or an SSD an overwrite is not an erase. So the store is three kinds of
//! plain files, each sealed with AES-256-GCM (already in the tree), and **delete is a key erase**:
//!
//! ```text
//! <app data>/history/
//!   catalog.bin            sealed under the KEK: settings, and per conversation its DEK, title,
//!                          provider/model, times, committed record count and committed log length
//!   conv/<id>.log          append-only frames, each sealed under that conversation's DEK
//!   blobs/<id>/<addr>      content-addressed artifact blobs, sealed under the same DEK
//! ```
//!
//! ## Keys
//!
//! - **KEK** (key-encryption key): 32 random bytes in the OS keyring (`history-kek-v1`, service
//!   `ai.citrate.core`), with a generation number. Device-sealed like the comms Option A key: it is
//!   random, not derived from the wallet, so history never follows a recovery phrase to another
//!   device. It seals only the catalog.
//! - **DEK** (one per conversation): 32 random bytes, kept only inside the sealed catalog. It seals
//!   that conversation's log frames and blobs.
//!
//! **Delete.** Removing a conversation drops its DEK from the catalog and then **rotates the KEK**:
//! the catalog is resealed under a new KEK and the old KEK is removed from the keyring. Every older
//! copy of the catalog (a snapshot, a backup, blocks the filesystem kept) is sealed under a KEK that
//! no longer exists, so the DEK, and with it every frame and blob of the conversation, cannot be
//! recovered even if their bytes survive. The files are then removed as well. Retention purges go
//! through the same path (one rotation per purge).
//!
//! **Rotation** (on delete, and on the member's request) is crash-safe in three steps: (1) the
//! keyring gets `{gen+1, new key, prev: old}`; (2) the catalog is resealed under the new key; (3) the
//! keyring drops `prev`. Opening after a crash between (1) and (2) reads the catalog with `prev` and
//! finishes the rotation; after (2), it only drops `prev`. A KEK that is neither current nor `prev`
//! cannot open the catalog. A member-requested rotation reseals only the catalog: DEKs are random per
//! conversation and never leave this process, so the frames and blobs need no re-encryption.
//!
//! ## Integrity and crash safety
//!
//! - Each frame's associated data binds it to its conversation id and its sequence number, so a
//!   frame cannot be moved, reordered, replayed into another conversation or dropped from the middle.
//! - The catalog records each log's committed length and record count. A log shorter than that, or
//!   a frame inside the committed region that does not open, is **tampered**: the read fails closed.
//! - Bytes after the committed length are a crash between the frame write and the catalog write.
//!   Frames there that open (only the DEK holder can make one) are accepted; the first one that does
//!   not ends the log, and the next append cuts the torn tail off before writing.
//! - The keyring keeps an **anchor** (`history-anchor-v1`): the catalog's sequence number. A
//!   catalog older than its anchor was rolled back (fail closed); a missing catalog with a non-zero
//!   anchor was removed (fail closed). One ahead of it is a crash between the two writes, accepted.
//! - Every file write is a unique temp file, created 0600, fsynced and renamed over the target
//!   (`atomic_file::replace_private`); appends fsync the log.
//! - Files left behind by a delete that crashed after its catalog write are swept at open.
//!
//! ## What a record holds
//!
//! Turns, tool calls, tool results, artifact references, the conversation's provider/model route
//! and taint notices. **No cost data**: a turn that went through the core model proxy carries only
//! the proxy's call id (`pc-<hex>`) so the usage log record (tokens, cost) is joined by id, never
//! copied. The record field set is pinned by `record_fields_are_a_closed_allowlist`.
//!
//! **Untrusted content.** Tool results and artifacts from a tool or a delegate are marked untrusted
//! when read, searched or exported, and so is an assistant turn written after the session was
//! tainted. Recorded content never changes a conversation's provider or model: [`HistoryStore::append`]
//! refuses route and artifact records, which only [`HistoryStore::set_route`] and
//! [`HistoryStore::put_artifact`] write.
//!
//! **Search** decrypts in memory and scans; there is no plaintext index on disk.

use std::collections::BTreeMap;
use std::io::{Seek as _, SeekFrom, Write as _};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use aes_gcm::aead::generic_array::GenericArray;
use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::Aes256Gcm;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

use crate::ai::AiKeyring;

/// The store's directory under the app data directory.
pub const HISTORY_DIR: &str = "history";
/// Keyring account holding the KEK document (current key, generation, and `prev` mid-rotation).
pub const KEY_ACCOUNT: &str = "history-kek-v1";
/// Keyring account holding the anchor (the catalog's sequence number; not secret).
pub const ANCHOR_ACCOUNT: &str = "history-anchor-v1";
pub const CATALOG_FILE: &str = "catalog.bin";
pub const LOG_DIR: &str = "conv";
pub const BLOB_DIR: &str = "blobs";

const CATALOG_MAGIC: &[u8; 4] = b"CHC1";
const LOG_MAGIC: &[u8; 4] = b"CHL1";
const BLOB_MAGIC: &[u8; 4] = b"CHB1";
const BLOB_ADDR_DOMAIN: &[u8] = b"citrate-history-blob-v1";
const NONCE_LEN: usize = 12;
const TAG_LEN: usize = 16;
const CATALOG_VERSION: u32 = 1;
const KEY_DOC_VERSION: u32 = 1;

/// Largest text in one record (a turn, tool arguments or a tool result), in bytes.
pub const MAX_TEXT_BYTES: usize = 1 << 20;
/// Largest artifact, in bytes (the same cap as a media gallery image).
pub const MAX_ARTIFACT_BYTES: usize = 25 * 1024 * 1024;
/// Longest conversation title, in characters.
pub const MAX_TITLE_CHARS: usize = 200;
/// Longest provider, model, tool name, status, artifact name or MIME type, in characters.
pub const MAX_LABEL_CHARS: usize = 200;
/// The longest retention the setting accepts (ten years); `None` keeps until the member deletes.
pub const MAX_RETENTION_DAYS: u32 = 3650;
/// Most search hits returned.
pub const MAX_SEARCH_HITS: usize = 200;
pub const DAY_MS: u64 = 86_400_000;
/// Largest sealed frame accepted when reading (two full texts plus framing).
const MAX_FRAME_BYTES: usize = 2 * MAX_TEXT_BYTES + 64 * 1024;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HistoryError {
    /// The OS keyring could not be read or written.
    Keyring,
    /// The catalog cannot be opened with the keys this device holds (wrong key, missing key, or a
    /// changed file: AEAD cannot tell these apart).
    Unreadable(&'static str),
    /// The store was changed outside the app (shortened, rolled back, removed or edited).
    Tampered(String),
    NotFound,
    Invalid(String),
    Io(String),
}

impl std::fmt::Display for HistoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HistoryError::Keyring => write!(f, "the OS keyring is unavailable"),
            HistoryError::Unreadable(m) => write!(f, "conversation history cannot be read: {m}"),
            HistoryError::Tampered(m) => {
                write!(f, "conversation history was changed outside the app: {m}")
            }
            HistoryError::NotFound => write!(f, "that conversation is not in the history"),
            HistoryError::Invalid(m) => write!(f, "{m}"),
            HistoryError::Io(m) => write!(f, "conversation history storage error: {m}"),
        }
    }
}

pub type Result<T> = std::result::Result<T, HistoryError>;

fn io(e: std::io::Error) -> HistoryError {
    HistoryError::Io(e.kind().to_string())
}

fn invalid(m: impl Into<String>) -> HistoryError {
    HistoryError::Invalid(m.into())
}

// ---------------------------------------------------------------------------
// Records
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// The in-app chat (`ai_chat` / `ai_chat_local`).
    Chat,
    /// A sidecar agent session (`hermes_session_*`).
    Agent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    User,
    Assistant,
    System,
}

/// Who produced an artifact. Tool and delegate output is untrusted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    Member,
    Model,
    Tool,
    Delegate,
}

impl Origin {
    pub fn untrusted(self) -> bool {
        matches!(self, Origin::Tool | Origin::Delegate)
    }
}

/// One history record. The field set is closed and pinned by a test: no cost, token or key field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum Record {
    Turn {
        role: Role,
        text: String,
        /// The core model proxy's call id for the model call that produced this turn; joins the
        /// usage log record (tokens, cost) instead of copying it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        usage_call: Option<String>,
        /// Written after the session read untrusted content.
        #[serde(default)]
        tainted: bool,
    },
    ToolCall {
        call_id: String,
        name: String,
        arguments: String,
    },
    ToolResult {
        call_id: String,
        status: String,
        content: String,
    },
    Artifact {
        id: String,
        name: String,
        mime: String,
        size: u64,
        sha256: String,
        blob: String,
        origin: Origin,
    },
    Route {
        provider: String,
        model: String,
    },
    Taint {
        source: String,
    },
}

impl Record {
    /// Whether this record's content came from outside the member and their model's own reply.
    pub fn untrusted(&self) -> bool {
        match self {
            Record::ToolResult { .. } => true,
            Record::Artifact { origin, .. } => origin.untrusted(),
            Record::Turn { tainted, .. } => *tainted,
            _ => false,
        }
    }

    /// The searchable text of this record.
    fn texts(&self) -> Vec<&str> {
        match self {
            Record::Turn { text, .. } => vec![text],
            Record::ToolCall {
                name, arguments, ..
            } => vec![name, arguments],
            Record::ToolResult { content, .. } => vec![content],
            Record::Artifact { name, .. } => vec![name],
            Record::Route { provider, model } => vec![provider, model],
            Record::Taint { source } => vec![source],
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            Record::Turn { .. } => "turn",
            Record::ToolCall { .. } => "tool_call",
            Record::ToolResult { .. } => "tool_result",
            Record::Artifact { .. } => "artifact",
            Record::Route { .. } => "route",
            Record::Taint { .. } => "taint",
        }
    }
}

/// What a frame holds: the record and when it was written.
#[derive(Serialize, Deserialize)]
struct Stored {
    ts: u64,
    #[serde(flatten)]
    record: Record,
}

/// One record as read back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub seq: u64,
    pub ts: u64,
    pub untrusted: bool,
    pub record: Record,
}

// ---------------------------------------------------------------------------
// Catalog and keys
// ---------------------------------------------------------------------------

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CatEntry {
    dek: String,
    title: String,
    source: Source,
    provider: String,
    model: String,
    created_ms: u64,
    updated_ms: u64,
    records: u64,
    log_bytes: u64,
    artifacts: u64,
    tainted: bool,
}

impl Drop for CatEntry {
    fn drop(&mut self) {
        self.dek.zeroize();
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Catalog {
    v: u32,
    seq: u64,
    #[serde(default)]
    retention_days: Option<u32>,
    conversations: BTreeMap<String, CatEntry>,
}

impl Catalog {
    fn new() -> Self {
        Catalog {
            v: CATALOG_VERSION,
            seq: 0,
            retention_days: None,
            conversations: BTreeMap::new(),
        }
    }
}

#[derive(Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
struct PrevKey {
    gen: u64,
    key: String,
}

#[derive(Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
struct KeyDoc {
    v: u32,
    gen: u64,
    key: String,
    #[serde(default)]
    prev: Option<PrevKey>,
}

#[derive(Serialize, Deserialize)]
struct AnchorDoc {
    v: u32,
    seq: u64,
}

struct Kek {
    gen: u64,
    key: Zeroizing<[u8; 32]>,
}

fn parse_key(hex_key: &str) -> Option<Zeroizing<[u8; 32]>> {
    let raw = Zeroizing::new(hex::decode(hex_key).ok()?);
    if raw.len() != 32 {
        return None;
    }
    let mut k = Zeroizing::new([0u8; 32]);
    k.copy_from_slice(&raw);
    Some(k)
}

fn random_key() -> Zeroizing<[u8; 32]> {
    let mut k = Zeroizing::new([0u8; 32]);
    rand::rngs::OsRng.fill_bytes(k.as_mut());
    k
}

fn random_hex(n: usize) -> String {
    let mut b = vec![0u8; n];
    rand::rngs::OsRng.fill_bytes(&mut b);
    hex::encode(b)
}

fn seal(key: &[u8; 32], aad: &[u8], plaintext: &[u8]) -> Result<Vec<u8>> {
    let cipher = Aes256Gcm::new(GenericArray::from_slice(key));
    let mut nonce = [0u8; NONCE_LEN];
    rand::rngs::OsRng.fill_bytes(&mut nonce);
    let ct = cipher
        .encrypt(
            GenericArray::from_slice(&nonce),
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .map_err(|_| HistoryError::Io("seal failed".into()))?;
    let mut out = Vec::with_capacity(NONCE_LEN + ct.len());
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ct);
    Ok(out)
}

fn unseal(key: &[u8; 32], aad: &[u8], sealed: &[u8]) -> Option<Zeroizing<Vec<u8>>> {
    if sealed.len() < NONCE_LEN + TAG_LEN {
        return None;
    }
    let cipher = Aes256Gcm::new(GenericArray::from_slice(key));
    cipher
        .decrypt(
            GenericArray::from_slice(&sealed[..NONCE_LEN]),
            Payload {
                msg: &sealed[NONCE_LEN..],
                aad,
            },
        )
        .ok()
        .map(Zeroizing::new)
}

fn catalog_aad(gen: u64) -> Vec<u8> {
    let mut a = CATALOG_MAGIC.to_vec();
    a.extend_from_slice(&gen.to_be_bytes());
    a
}

fn frame_aad(id: &str, seq: u64) -> Vec<u8> {
    let mut a = LOG_MAGIC.to_vec();
    a.extend_from_slice(id.as_bytes());
    a.push(0);
    a.extend_from_slice(&seq.to_be_bytes());
    a
}

fn blob_aad(id: &str, addr: &str) -> Vec<u8> {
    let mut a = BLOB_MAGIC.to_vec();
    a.extend_from_slice(id.as_bytes());
    a.push(0);
    a.extend_from_slice(addr.as_bytes());
    a
}

/// The blob address: a keyed hash of the content under the conversation's DEK, so equal content
/// is stored once per conversation while the file name reveals nothing about the content.
fn blob_addr(dek: &[u8; 32], bytes: &[u8]) -> String {
    use hmac::Mac as _;
    // HMAC accepts any key length, so this cannot fail; a failure would only mean an empty addr,
    // which `valid_hex64` then refuses.
    match <hmac::Hmac<Sha256> as hmac::Mac>::new_from_slice(dek) {
        Ok(mut m) => {
            m.update(BLOB_ADDR_DOMAIN);
            m.update(bytes);
            hex::encode(m.finalize().into_bytes())
        }
        Err(_) => String::new(),
    }
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

fn valid_conversation_id(id: &str) -> bool {
    id.len() == 35 && id.starts_with("cv-") && valid_lower_hex(&id[3..])
}

fn valid_lower_hex(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn valid_hex64(s: &str) -> bool {
    s.len() == 64 && valid_lower_hex(s)
}

/// The core model proxy's call id shape (`pc-<hex>`).
pub fn valid_usage_call(s: &str) -> bool {
    s.len() >= 11 && s.len() <= 67 && s.starts_with("pc-") && valid_lower_hex(&s[3..])
}

fn check_id(id: &str) -> Result<()> {
    if valid_conversation_id(id) {
        Ok(())
    } else {
        Err(invalid("that is not a conversation id"))
    }
}

fn check_label(what: &str, s: &str) -> Result<()> {
    if s.trim().is_empty() || s.chars().count() > MAX_LABEL_CHARS || s.chars().any(char::is_control)
    {
        return Err(invalid(format!(
            "{what} must be 1 to {MAX_LABEL_CHARS} printable characters"
        )));
    }
    Ok(())
}

fn check_text(what: &str, s: &str) -> Result<()> {
    if s.len() > MAX_TEXT_BYTES {
        return Err(invalid(format!(
            "{what} is larger than {MAX_TEXT_BYTES} bytes"
        )));
    }
    Ok(())
}

/// A display title: control characters become spaces, whitespace is collapsed, and it is cut to
/// [`MAX_TITLE_CHARS`]. An empty result is refused.
pub fn clean_title(raw: &str) -> Result<String> {
    let spaced: String = raw
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let t: String = spaced
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(MAX_TITLE_CHARS)
        .collect();
    if t.is_empty() {
        return Err(invalid("a conversation title cannot be empty"));
    }
    Ok(t)
}

fn check_record(r: &Record) -> Result<()> {
    match r {
        Record::Turn {
            text, usage_call, ..
        } => {
            check_text("a turn", text)?;
            if let Some(c) = usage_call {
                if !valid_usage_call(c) {
                    return Err(invalid("the usage call id is not a model proxy call id"));
                }
            }
        }
        Record::ToolCall {
            call_id,
            name,
            arguments,
        } => {
            check_label("a tool call id", call_id)?;
            check_label("a tool name", name)?;
            check_text("tool arguments", arguments)?;
        }
        Record::ToolResult {
            call_id,
            status,
            content,
        } => {
            check_label("a tool call id", call_id)?;
            check_label("a tool status", status)?;
            check_text("a tool result", content)?;
        }
        Record::Artifact {
            id,
            name,
            mime,
            sha256,
            blob,
            ..
        } => {
            check_label("an artifact id", id)?;
            check_label("an artifact name", name)?;
            check_label("a MIME type", mime)?;
            if !valid_hex64(sha256) || !valid_hex64(blob) {
                return Err(invalid("an artifact reference is malformed"));
            }
        }
        Record::Route { provider, model } => {
            check_label("a provider", provider)?;
            check_label("a model", model)?;
        }
        Record::Taint { source } => check_label("a taint source", source)?,
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Public views
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub id: String,
    pub title: String,
    pub source: Source,
    pub provider: String,
    pub model: String,
    pub created_ms: u64,
    pub updated_ms: u64,
    pub records: u64,
    pub artifacts: u64,
    pub tainted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Conversation {
    pub summary: Summary,
    pub entries: Vec<Entry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Hit {
    pub conversation: String,
    pub title: String,
    /// `None` for a title match.
    pub seq: Option<u64>,
    pub kind: &'static str,
    pub snippet: String,
    pub untrusted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub conversations: usize,
    pub retention_days: Option<u32>,
    pub key_generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactRef {
    pub id: String,
    pub sha256: String,
    pub size: u64,
}

pub struct NewConversation<'a> {
    pub title: &'a str,
    pub source: Source,
    pub provider: &'a str,
    pub model: &'a str,
}

pub struct NewArtifact<'a> {
    pub name: &'a str,
    pub mime: &'a str,
    pub origin: Origin,
    pub bytes: &'a [u8],
}

/// A conversation exported for the member: a Markdown transcript and a JSON document (with the
/// artifacts inline as base64). Plaintext by design; written only where the member chose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Export {
    pub stem: String,
    pub markdown: String,
    pub json: String,
}

fn summary_of(id: &str, e: &CatEntry) -> Summary {
    Summary {
        id: id.to_string(),
        title: e.title.clone(),
        source: e.source,
        provider: e.provider.clone(),
        model: e.model.clone(),
        created_ms: e.created_ms,
        updated_ms: e.updated_ms,
        records: e.records,
        artifacts: e.artifacts,
        tainted: e.tainted,
    }
}

// ---------------------------------------------------------------------------
// Log frames
// ---------------------------------------------------------------------------

/// The next frame at `off`: its sealed bytes and the offset after it, or `None` when what is there
/// is not a whole frame.
fn next_frame(buf: &[u8], off: usize) -> Option<(&[u8], usize)> {
    let head = buf.get(off..off.checked_add(4)?)?;
    let len = u32::from_be_bytes([head[0], head[1], head[2], head[3]]) as usize;
    if !(NONCE_LEN + TAG_LEN..=MAX_FRAME_BYTES).contains(&len) {
        return None;
    }
    let end = off.checked_add(4)?.checked_add(len)?;
    Some((buf.get(off + 4..end)?, end))
}

fn open_frame(dek: &[u8; 32], id: &str, seq: u64, sealed: &[u8]) -> Option<Stored> {
    let pt = unseal(dek, &frame_aad(id, seq), sealed)?;
    serde_json::from_slice::<Stored>(&pt).ok()
}

/// The frames after the committed region that still open: a crash between a frame write and its
/// catalog write. Returns the decoded frames and the offset where the valid log ends.
fn crash_tail(
    buf: &[u8],
    dek: &[u8; 32],
    id: &str,
    mut seq: u64,
    mut off: usize,
) -> (Vec<(u64, Stored)>, usize) {
    let mut out = Vec::new();
    while let Some((sealed, end)) = next_frame(buf, off) {
        match open_frame(dek, id, seq, sealed) {
            Some(s) => {
                out.push((seq, s));
                seq += 1;
                off = end;
            }
            None => break,
        }
    }
    (out, off)
}

// ---------------------------------------------------------------------------
// The store
// ---------------------------------------------------------------------------

/// What `load` found.
struct Loaded {
    /// The current KEK, `None` on a fresh install (no key yet).
    kek: Option<Kek>,
    cat: Catalog,
    /// The catalog was sealed under the previous KEK (a rotation was interrupted before step 2).
    sealed_with_prev: bool,
    /// The keyring still holds a previous KEK (a rotation was interrupted before step 3).
    has_prev: bool,
}

pub struct HistoryStore {
    dir: PathBuf,
    keyring: Arc<dyn AiKeyring>,
    lock: Mutex<()>,
}

impl HistoryStore {
    /// Open the store in `dir`, finishing an interrupted rotation and sweeping files a crashed
    /// delete left behind. Fails closed on an unreadable, rolled-back or removed catalog.
    pub fn open(dir: impl Into<PathBuf>, keyring: Arc<dyn AiKeyring>) -> Result<Self> {
        let s = HistoryStore {
            dir: dir.into(),
            keyring,
            lock: Mutex::new(()),
        };
        citrate_core_kit::fsutil::ensure_private_dir(&s.dir).map_err(io)?;
        {
            let _g = s.guard()?;
            s.recover()?;
        }
        Ok(s)
    }

    fn guard(&self) -> Result<MutexGuard<'_, ()>> {
        self.lock
            .lock()
            .map_err(|_| HistoryError::Io("history lock poisoned".into()))
    }

    fn catalog_path(&self) -> PathBuf {
        self.dir.join(CATALOG_FILE)
    }

    fn log_path(&self, id: &str) -> PathBuf {
        self.dir.join(LOG_DIR).join(format!("{id}.log"))
    }

    fn blob_dir(&self, id: &str) -> PathBuf {
        self.dir.join(BLOB_DIR).join(id)
    }

    fn read_key_doc(&self) -> Result<Option<KeyDoc>> {
        match self.keyring.get(KEY_ACCOUNT) {
            Err(_) => Err(HistoryError::Keyring),
            Ok(None) => Ok(None),
            Ok(Some(s)) => {
                let s = Zeroizing::new(s);
                let d: KeyDoc = serde_json::from_str(&s)
                    .map_err(|_| HistoryError::Unreadable("the history key is malformed"))?;
                if d.v != KEY_DOC_VERSION {
                    return Err(HistoryError::Unreadable("the history key is malformed"));
                }
                Ok(Some(d))
            }
        }
    }

    fn write_key_doc(&self, d: &KeyDoc) -> Result<()> {
        let s = Zeroizing::new(serde_json::to_string(d).map_err(|_| HistoryError::Keyring)?);
        self.keyring
            .set(KEY_ACCOUNT, &s)
            .map_err(|_| HistoryError::Keyring)
    }

    fn read_anchor(&self) -> Result<u64> {
        match self.keyring.get(ANCHOR_ACCOUNT) {
            Err(_) => Err(HistoryError::Keyring),
            Ok(None) => Ok(0),
            Ok(Some(s)) => serde_json::from_str::<AnchorDoc>(&s)
                .map(|a| a.seq)
                .map_err(|_| HistoryError::Tampered("the history anchor is malformed".into())),
        }
    }

    fn write_anchor(&self, seq: u64) -> Result<()> {
        let s =
            serde_json::to_string(&AnchorDoc { v: 1, seq }).map_err(|_| HistoryError::Keyring)?;
        self.keyring
            .set(ANCHOR_ACCOUNT, &s)
            .map_err(|_| HistoryError::Keyring)
    }

    fn load(&self) -> Result<Loaded> {
        let doc = self.read_key_doc()?;
        let anchor = self.read_anchor()?;
        let file = match std::fs::read(self.catalog_path()) {
            Ok(b) => Some(b),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(io(e)),
        };
        let fresh = |kek| Loaded {
            kek,
            cat: Catalog::new(),
            sealed_with_prev: false,
            has_prev: false,
        };
        match (doc, file) {
            (None, None) if anchor > 0 => Err(HistoryError::Tampered(
                "the history catalog and its key are missing".into(),
            )),
            (None, None) => Ok(fresh(None)),
            (None, Some(_)) => Err(HistoryError::Unreadable(
                "the history key is missing from the OS keyring",
            )),
            (Some(_), None) if anchor > 0 => Err(HistoryError::Tampered(
                "the history catalog is missing".into(),
            )),
            (Some(d), None) => {
                let key = parse_key(&d.key)
                    .ok_or(HistoryError::Unreadable("the history key is malformed"))?;
                Ok(fresh(Some(Kek { gen: d.gen, key })))
            }
            (Some(d), Some(bytes)) => {
                if bytes.len() < 12 || &bytes[..4] != CATALOG_MAGIC {
                    return Err(HistoryError::Unreadable(
                        "the history catalog is not a catalog",
                    ));
                }
                let mut g = [0u8; 8];
                g.copy_from_slice(&bytes[4..12]);
                let gen = u64::from_be_bytes(g);
                let current = Kek {
                    gen: d.gen,
                    key: parse_key(&d.key)
                        .ok_or(HistoryError::Unreadable("the history key is malformed"))?,
                };
                let (open_key, sealed_with_prev) = if gen == d.gen {
                    (Zeroizing::new(*current.key), false)
                } else {
                    match d.prev.as_ref().filter(|p| p.gen == gen) {
                        Some(p) => (
                            parse_key(&p.key)
                                .ok_or(HistoryError::Unreadable("the history key is malformed"))?,
                            true,
                        ),
                        None => {
                            return Err(HistoryError::Unreadable(
                                "the catalog is sealed under a key this device no longer holds",
                            ))
                        }
                    }
                };
                let pt = unseal(&open_key, &catalog_aad(gen), &bytes[12..]).ok_or(
                    HistoryError::Unreadable(
                        "the catalog does not open: a wrong key, or the file was changed",
                    ),
                )?;
                let cat: Catalog = serde_json::from_slice(&pt)
                    .map_err(|_| HistoryError::Tampered("the catalog is malformed".into()))?;
                if cat.v != CATALOG_VERSION {
                    return Err(HistoryError::Tampered(
                        "the catalog has an unknown version".into(),
                    ));
                }
                if cat.seq < anchor {
                    return Err(HistoryError::Tampered(
                        "the catalog is older than its anchor (rolled back)".into(),
                    ));
                }
                Ok(Loaded {
                    kek: Some(current),
                    cat,
                    sealed_with_prev,
                    has_prev: d.prev.is_some(),
                })
            }
        }
    }

    /// The current KEK, minting the first one on a fresh install.
    fn kek_for_write(&self, l: &mut Loaded) -> Result<Kek> {
        if let Some(k) = l.kek.take() {
            return Ok(k);
        }
        let key = random_key();
        self.write_key_doc(&KeyDoc {
            v: KEY_DOC_VERSION,
            gen: 1,
            key: hex::encode(key.as_ref()),
            prev: None,
        })?;
        Ok(Kek { gen: 1, key })
    }

    /// Seal and publish the catalog, then advance the anchor.
    fn commit(&self, kek: &Kek, cat: &mut Catalog) -> Result<()> {
        cat.seq += 1;
        let pt = Zeroizing::new(serde_json::to_vec(cat).map_err(|_| invalid("encode catalog"))?);
        let mut bytes = catalog_aad(kek.gen);
        bytes.extend_from_slice(&seal(&kek.key, &catalog_aad(kek.gen), &pt)?);
        crate::atomic_file::replace_private(&self.catalog_path(), &bytes).map_err(io)?;
        self.write_anchor(cat.seq)
    }

    /// Reseal the catalog under a new KEK and forget the old one (crash-safe, see module docs).
    fn rotate(&self, cur: &Kek, cat: &mut Catalog) -> Result<Kek> {
        let next = Kek {
            gen: cur.gen + 1,
            key: random_key(),
        };
        self.write_key_doc(&KeyDoc {
            v: KEY_DOC_VERSION,
            gen: next.gen,
            key: hex::encode(next.key.as_ref()),
            prev: Some(PrevKey {
                gen: cur.gen,
                key: hex::encode(cur.key.as_ref()),
            }),
        })?;
        self.commit(&next, cat)?;
        self.write_key_doc(&KeyDoc {
            v: KEY_DOC_VERSION,
            gen: next.gen,
            key: hex::encode(next.key.as_ref()),
            prev: None,
        })?;
        Ok(next)
    }

    fn recover(&self) -> Result<()> {
        let mut l = self.load()?;
        if let Some(kek) = l.kek.take() {
            if l.sealed_with_prev {
                // Step 1 of a rotation ran, step 2 did not: finish it.
                self.commit(&kek, &mut l.cat)?;
            }
            if l.has_prev || l.sealed_with_prev {
                self.write_key_doc(&KeyDoc {
                    v: KEY_DOC_VERSION,
                    gen: kek.gen,
                    key: hex::encode(kek.key.as_ref()),
                    prev: None,
                })?;
            }
        }
        self.sweep(&l.cat)
    }

    /// Remove logs, blob folders and temp files that no catalog entry owns (a delete or a write
    /// that crashed after its catalog step). Exact paths only.
    fn sweep(&self, cat: &Catalog) -> Result<()> {
        if let Ok(rd) = std::fs::read_dir(self.dir.join(LOG_DIR)) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                let owned = name
                    .strip_suffix(".log")
                    .is_some_and(|id| cat.conversations.contains_key(id));
                if !owned {
                    std::fs::remove_file(e.path()).map_err(io)?;
                }
            }
        }
        if let Ok(rd) = std::fs::read_dir(self.dir.join(BLOB_DIR)) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if !cat.conversations.contains_key(&name) {
                    let p = e.path();
                    if p.is_dir() {
                        std::fs::remove_dir_all(&p).map_err(io)?;
                    } else {
                        std::fs::remove_file(&p).map_err(io)?;
                    }
                }
            }
        }
        if let Ok(rd) = std::fs::read_dir(&self.dir) {
            let prefix = format!("{CATALOG_FILE}.tmp-");
            for e in rd.flatten() {
                if e.file_name().to_string_lossy().starts_with(&prefix) {
                    std::fs::remove_file(e.path()).map_err(io)?;
                }
            }
        }
        Ok(())
    }

    // -- conversations ------------------------------------------------------------------------

    /// Start a conversation. Returns its id (`cv-<32 hex>`).
    pub fn create(&self, c: NewConversation<'_>, now_ms: u64) -> Result<String> {
        let title = clean_title(c.title)?;
        check_label("a provider", c.provider)?;
        check_label("a model", c.model)?;
        let _g = self.guard()?;
        let mut l = self.load()?;
        let kek = self.kek_for_write(&mut l)?;
        let id = format!("cv-{}", random_hex(16));
        let dek = random_key();
        l.cat.conversations.insert(
            id.clone(),
            CatEntry {
                dek: hex::encode(dek.as_ref()),
                title,
                source: c.source,
                provider: c.provider.to_string(),
                model: c.model.to_string(),
                created_ms: now_ms,
                updated_ms: now_ms,
                records: 0,
                log_bytes: 0,
                artifacts: 0,
                tainted: false,
            },
        );
        self.commit(&kek, &mut l.cat)?;
        Ok(id)
    }

    /// Append a turn, tool call, tool result or taint notice. Route and artifact records are
    /// refused here: only [`Self::set_route`] and [`Self::put_artifact`] write them.
    pub fn append(&self, id: &str, record: Record, now_ms: u64) -> Result<u64> {
        check_id(id)?;
        if matches!(record, Record::Route { .. } | Record::Artifact { .. }) {
            return Err(invalid(
                "route and artifact records are written by their own operations",
            ));
        }
        check_record(&record)?;
        let _g = self.guard()?;
        let mut l = self.load()?;
        let kek = self.kek_for_write(&mut l)?;
        let seq = self.append_frame(&mut l.cat, id, record, now_ms)?;
        self.commit(&kek, &mut l.cat)?;
        Ok(seq)
    }

    /// Write one frame to the log of `id` and update its catalog entry (not committed here).
    fn append_frame(
        &self,
        cat: &mut Catalog,
        id: &str,
        record: Record,
        now_ms: u64,
    ) -> Result<u64> {
        let path = self.log_path(id);
        let e = cat
            .conversations
            .get_mut(id)
            .ok_or(HistoryError::NotFound)?;
        let dek =
            parse_key(&e.dek).ok_or(HistoryError::Unreadable("a conversation key is malformed"))?;
        let existing = match std::fs::read(&path) {
            Ok(b) => b,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(err) => return Err(io(err)),
        };
        let committed = usize::try_from(e.log_bytes)
            .map_err(|_| HistoryError::Tampered("a log length is out of range".into()))?;
        if existing.len() < committed {
            return Err(HistoryError::Tampered(
                "a conversation log is shorter than its catalog entry".into(),
            ));
        }
        let (tail, valid_end) = crash_tail(&existing, &dek, id, e.records, committed);
        let seq = e.records + tail.len() as u64;
        let is_taint = matches!(record, Record::Taint { .. });
        let is_artifact = matches!(record, Record::Artifact { .. });
        let pt = Zeroizing::new(
            serde_json::to_vec(&Stored { ts: now_ms, record })
                .map_err(|_| invalid("encode record"))?,
        );
        let sealed = seal(&dek, &frame_aad(id, seq), &pt)?;
        let frame_len = u32::try_from(sealed.len()).map_err(|_| invalid("record too large"))?;
        let log_dir = self.dir.join(LOG_DIR);
        citrate_core_kit::fsutil::ensure_private_dir(&log_dir).map_err(io)?;
        let new_file = existing.is_empty();
        let mut f = open_private_rw(&path)?;
        // Cut a torn tail off before writing (the bytes past the last frame that opens).
        f.set_len(valid_end as u64).map_err(io)?;
        f.seek(SeekFrom::End(0)).map_err(io)?;
        let mut buf = Vec::with_capacity(4 + sealed.len());
        buf.extend_from_slice(&frame_len.to_be_bytes());
        buf.extend_from_slice(&sealed);
        f.write_all(&buf).map_err(io)?;
        f.sync_all().map_err(io)?;
        if new_file {
            sync_dir(&log_dir)?;
        }
        // Frames from a crash tail are real records; count them, and any artifacts among them.
        let tail_artifacts = tail
            .iter()
            .filter(|(_, s)| matches!(s.record, Record::Artifact { .. }))
            .count() as u64;
        let tail_taint = tail
            .iter()
            .any(|(_, s)| matches!(s.record, Record::Taint { .. }));
        e.records = seq + 1;
        e.log_bytes = (valid_end + buf.len()) as u64;
        e.updated_ms = e.updated_ms.max(now_ms);
        e.artifacts += tail_artifacts + u64::from(is_artifact);
        e.tainted |= is_taint || tail_taint;
        Ok(seq)
    }

    /// Store an artifact (content-addressed blob, sealed under the conversation's key) and append
    /// its reference record.
    pub fn put_artifact(&self, id: &str, a: NewArtifact<'_>, now_ms: u64) -> Result<ArtifactRef> {
        check_id(id)?;
        check_label("an artifact name", a.name)?;
        check_label("a MIME type", a.mime)?;
        if a.bytes.len() > MAX_ARTIFACT_BYTES {
            return Err(invalid(format!(
                "an artifact is larger than {MAX_ARTIFACT_BYTES} bytes"
            )));
        }
        let _g = self.guard()?;
        let mut l = self.load()?;
        let kek = self.kek_for_write(&mut l)?;
        let dek = {
            let e = l.cat.conversations.get(id).ok_or(HistoryError::NotFound)?;
            parse_key(&e.dek).ok_or(HistoryError::Unreadable("a conversation key is malformed"))?
        };
        let addr = blob_addr(&dek, a.bytes);
        let bdir = self.blob_dir(id);
        let bpath = bdir.join(&addr);
        if !bpath.is_file() {
            citrate_core_kit::fsutil::ensure_private_dir(&bdir).map_err(io)?;
            let mut bytes = BLOB_MAGIC.to_vec();
            bytes.extend_from_slice(&seal(&dek, &blob_aad(id, &addr), a.bytes)?);
            crate::atomic_file::replace_private(&bpath, &bytes).map_err(io)?;
        }
        let r = ArtifactRef {
            id: format!("ar-{}", random_hex(8)),
            sha256: hex::encode(Sha256::digest(a.bytes)),
            size: a.bytes.len() as u64,
        };
        let record = Record::Artifact {
            id: r.id.clone(),
            name: a.name.to_string(),
            mime: a.mime.to_string(),
            size: r.size,
            sha256: r.sha256.clone(),
            blob: addr,
            origin: a.origin,
        };
        check_record(&record)?;
        self.append_frame(&mut l.cat, id, record, now_ms)?;
        self.commit(&kek, &mut l.cat)?;
        Ok(r)
    }

    /// Set the conversation's provider and model (a member or core action, never recorded content)
    /// and record the change.
    pub fn set_route(&self, id: &str, provider: &str, model: &str, now_ms: u64) -> Result<()> {
        check_id(id)?;
        let record = Record::Route {
            provider: provider.to_string(),
            model: model.to_string(),
        };
        check_record(&record)?;
        let _g = self.guard()?;
        let mut l = self.load()?;
        let kek = self.kek_for_write(&mut l)?;
        self.append_frame(&mut l.cat, id, record, now_ms)?;
        if let Some(e) = l.cat.conversations.get_mut(id) {
            e.provider = provider.to_string();
            e.model = model.to_string();
        }
        self.commit(&kek, &mut l.cat)
    }

    pub fn rename(&self, id: &str, title: &str) -> Result<()> {
        check_id(id)?;
        let title = clean_title(title)?;
        let _g = self.guard()?;
        let mut l = self.load()?;
        let kek = self.kek_for_write(&mut l)?;
        l.cat
            .conversations
            .get_mut(id)
            .ok_or(HistoryError::NotFound)?
            .title = title;
        self.commit(&kek, &mut l.cat)
    }

    // -- reads ---------------------------------------------------------------------------------

    pub fn status(&self) -> Result<Status> {
        let _g = self.guard()?;
        let l = self.load()?;
        Ok(Status {
            conversations: l.cat.conversations.len(),
            retention_days: l.cat.retention_days,
            key_generation: l.kek.as_ref().map(|k| k.gen).unwrap_or(0),
        })
    }

    /// Every conversation, most recently updated first.
    pub fn list(&self) -> Result<Vec<Summary>> {
        let _g = self.guard()?;
        let l = self.load()?;
        let mut v: Vec<Summary> = l
            .cat
            .conversations
            .iter()
            .map(|(id, e)| summary_of(id, e))
            .collect();
        v.sort_by(|a, b| b.updated_ms.cmp(&a.updated_ms).then(a.id.cmp(&b.id)));
        Ok(v)
    }

    /// One conversation's summary (catalog only; its log is not read).
    pub fn summary(&self, id: &str) -> Result<Summary> {
        check_id(id)?;
        let _g = self.guard()?;
        let l = self.load()?;
        let e = l.cat.conversations.get(id).ok_or(HistoryError::NotFound)?;
        Ok(summary_of(id, e))
    }

    fn entries_of(&self, id: &str, e: &CatEntry) -> Result<Vec<Entry>> {
        let dek =
            parse_key(&e.dek).ok_or(HistoryError::Unreadable("a conversation key is malformed"))?;
        let buf = match std::fs::read(self.log_path(id)) {
            Ok(b) => b,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                if e.log_bytes == 0 {
                    return Ok(Vec::new());
                }
                return Err(HistoryError::Tampered(
                    "a conversation log is missing".into(),
                ));
            }
            Err(err) => return Err(io(err)),
        };
        let committed = usize::try_from(e.log_bytes)
            .map_err(|_| HistoryError::Tampered("a log length is out of range".into()))?;
        if buf.len() < committed {
            return Err(HistoryError::Tampered(
                "a conversation log is shorter than its catalog entry".into(),
            ));
        }
        let mut out = Vec::new();
        let mut off = 0usize;
        let mut seq = 0u64;
        while off < committed {
            let (sealed, end) = next_frame(&buf[..committed], off).ok_or_else(|| {
                HistoryError::Tampered("a conversation log has a damaged record".into())
            })?;
            let s = open_frame(&dek, id, seq, sealed).ok_or_else(|| {
                HistoryError::Tampered("a conversation log record does not open".into())
            })?;
            out.push(Entry {
                seq,
                ts: s.ts,
                untrusted: s.record.untrusted(),
                record: s.record,
            });
            seq += 1;
            off = end;
        }
        if seq != e.records {
            return Err(HistoryError::Tampered(
                "a conversation log has a different number of records than its catalog entry"
                    .into(),
            ));
        }
        let (tail, _) = crash_tail(&buf, &dek, id, seq, committed);
        out.extend(tail.into_iter().map(|(seq, s)| Entry {
            seq,
            ts: s.ts,
            untrusted: s.record.untrusted(),
            record: s.record,
        }));
        Ok(out)
    }

    pub fn read(&self, id: &str) -> Result<Conversation> {
        check_id(id)?;
        let _g = self.guard()?;
        let l = self.load()?;
        let e = l.cat.conversations.get(id).ok_or(HistoryError::NotFound)?;
        let entries = self.entries_of(id, e)?;
        Ok(Conversation {
            summary: summary_of(id, e),
            entries,
        })
    }

    /// An artifact's metadata record and its bytes, checked against the recorded SHA-256.
    pub fn read_artifact(&self, id: &str, artifact_id: &str) -> Result<(Entry, Vec<u8>)> {
        check_id(id)?;
        let _g = self.guard()?;
        let l = self.load()?;
        let e = l.cat.conversations.get(id).ok_or(HistoryError::NotFound)?;
        let entry = self
            .entries_of(id, e)?
            .into_iter()
            .find(|en| matches!(&en.record, Record::Artifact { id: a, .. } if a == artifact_id))
            .ok_or(HistoryError::NotFound)?;
        let bytes = self.blob_bytes(id, e, &entry.record)?;
        Ok((entry, bytes))
    }

    fn blob_bytes(&self, id: &str, e: &CatEntry, r: &Record) -> Result<Vec<u8>> {
        let Record::Artifact { blob, sha256, .. } = r else {
            return Err(HistoryError::NotFound);
        };
        if !valid_hex64(blob) {
            return Err(HistoryError::Tampered(
                "an artifact reference is malformed".into(),
            ));
        }
        let dek =
            parse_key(&e.dek).ok_or(HistoryError::Unreadable("a conversation key is malformed"))?;
        let raw = std::fs::read(self.blob_dir(id).join(blob)).map_err(|err| {
            if err.kind() == std::io::ErrorKind::NotFound {
                HistoryError::Tampered("an artifact's content is missing".into())
            } else {
                io(err)
            }
        })?;
        if raw.len() < 4 || &raw[..4] != BLOB_MAGIC {
            return Err(HistoryError::Tampered(
                "an artifact's content is damaged".into(),
            ));
        }
        let pt = unseal(&dek, &blob_aad(id, blob), &raw[4..])
            .ok_or_else(|| HistoryError::Tampered("an artifact's content does not open".into()))?;
        if hex::encode(Sha256::digest(pt.as_slice())) != *sha256 {
            return Err(HistoryError::Tampered(
                "an artifact's content does not match its hash".into(),
            ));
        }
        Ok(pt.to_vec())
    }

    /// Case-insensitive search over titles and record text, newest conversations first. Decrypts in
    /// memory; nothing is indexed on disk.
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<Hit>> {
        let q = query.trim().to_lowercase();
        if q.chars().count() < 2 || q.chars().count() > MAX_LABEL_CHARS {
            return Err(invalid(format!(
                "a search needs 2 to {MAX_LABEL_CHARS} characters"
            )));
        }
        let limit = limit.clamp(1, MAX_SEARCH_HITS);
        let _g = self.guard()?;
        let l = self.load()?;
        let mut convs: Vec<(&String, &CatEntry)> = l.cat.conversations.iter().collect();
        convs.sort_by(|a, b| b.1.updated_ms.cmp(&a.1.updated_ms).then(a.0.cmp(b.0)));
        let mut hits = Vec::new();
        for (id, e) in convs {
            if let Some(snippet) = snippet_for(&e.title, &q) {
                hits.push(Hit {
                    conversation: id.clone(),
                    title: e.title.clone(),
                    seq: None,
                    kind: "title",
                    snippet,
                    untrusted: false,
                });
                if hits.len() >= limit {
                    return Ok(hits);
                }
            }
            for en in self.entries_of(id, e)? {
                if let Some(snippet) = en.record.texts().iter().find_map(|t| snippet_for(t, &q)) {
                    hits.push(Hit {
                        conversation: id.clone(),
                        title: e.title.clone(),
                        seq: Some(en.seq),
                        kind: en.record.kind(),
                        snippet,
                        untrusted: en.untrusted,
                    });
                    if hits.len() >= limit {
                        return Ok(hits);
                    }
                }
            }
        }
        Ok(hits)
    }

    // -- delete, retention, rotation -----------------------------------------------------------

    /// Delete a conversation so that it cannot be read back: its key leaves the catalog, the
    /// catalog is resealed under a new KEK (the old one is erased from the keyring), and its files
    /// are removed.
    pub fn delete(&self, id: &str) -> Result<()> {
        check_id(id)?;
        self.delete_many(&[id.to_string()]).map(|_| ())
    }

    fn delete_many(&self, ids: &[String]) -> Result<Vec<String>> {
        let _g = self.guard()?;
        let mut l = self.load()?;
        let kek = self.kek_for_write(&mut l)?;
        let mut gone = Vec::new();
        for id in ids {
            if l.cat.conversations.remove(id).is_some() {
                gone.push(id.clone());
            }
        }
        if gone.is_empty() {
            return Err(HistoryError::NotFound);
        }
        self.rotate(&kek, &mut l.cat)?;
        for id in &gone {
            match std::fs::remove_file(self.log_path(id)) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(io(e)),
            }
            match std::fs::remove_dir_all(self.blob_dir(id)) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(io(e)),
            }
        }
        Ok(gone)
    }

    /// Set how long conversations are kept after their last activity. `None` keeps them until the
    /// member deletes them.
    pub fn set_retention(&self, days: Option<u32>) -> Result<()> {
        if let Some(d) = days {
            if d == 0 || d > MAX_RETENTION_DAYS {
                return Err(invalid(format!(
                    "retention must be 1 to {MAX_RETENTION_DAYS} days, or keep until deleted"
                )));
            }
        }
        let _g = self.guard()?;
        let mut l = self.load()?;
        let kek = self.kek_for_write(&mut l)?;
        l.cat.retention_days = days;
        self.commit(&kek, &mut l.cat)
    }

    /// Delete every conversation whose last activity is older than the retention setting. Returns
    /// the deleted ids (empty when nothing is due or retention is off).
    pub fn enforce_retention(&self, now_ms: u64) -> Result<Vec<String>> {
        let due: Vec<String> = {
            let _g = self.guard()?;
            let l = self.load()?;
            let Some(days) = l.cat.retention_days else {
                return Ok(Vec::new());
            };
            let cutoff = now_ms.saturating_sub(u64::from(days) * DAY_MS);
            l.cat
                .conversations
                .iter()
                .filter(|(_, e)| e.updated_ms < cutoff)
                .map(|(id, _)| id.clone())
                .collect()
        };
        if due.is_empty() {
            return Ok(Vec::new());
        }
        self.delete_many(&due)
    }

    /// Reseal the catalog under a new KEK on the member's request. Returns the new generation.
    pub fn rotate_key(&self) -> Result<u64> {
        let _g = self.guard()?;
        let mut l = self.load()?;
        let kek = self.kek_for_write(&mut l)?;
        Ok(self.rotate(&kek, &mut l.cat)?.gen)
    }

    // -- export --------------------------------------------------------------------------------

    /// A readable copy of one conversation (Markdown and JSON). Untrusted content is labelled.
    pub fn export(&self, id: &str) -> Result<Export> {
        use base64::Engine as _;
        let conv = self.read(id)?;
        let mut artifacts = Vec::new();
        {
            let _g = self.guard()?;
            let l = self.load()?;
            let e = l.cat.conversations.get(id).ok_or(HistoryError::NotFound)?;
            for en in &conv.entries {
                if let Record::Artifact {
                    id: aid,
                    name,
                    mime,
                    size,
                    sha256,
                    origin,
                    ..
                } = &en.record
                {
                    let bytes = self.blob_bytes(id, e, &en.record)?;
                    artifacts.push(serde_json::json!({
                        "id": aid,
                        "name": name,
                        "mime": mime,
                        "size": size,
                        "sha256": sha256,
                        "origin": origin,
                        "untrusted": origin.untrusted(),
                        "base64": base64::engine::general_purpose::STANDARD.encode(&bytes),
                    }));
                }
            }
        }
        let json = serde_json::to_string_pretty(&serde_json::json!({
            "format": "citrate-conversation-export",
            "version": 1,
            "conversation": conv.summary,
            "entries": conv.entries,
            "artifacts": artifacts,
        }))
        .map_err(|_| invalid("encode export"))?;
        Ok(Export {
            stem: format!("citrate-conversation-{}", &id[3..15]),
            markdown: markdown_of(&conv),
            json,
        })
    }
}

/// Open (creating 0600) a log for reading and writing without truncating it.
fn open_private_rw(path: &Path) -> Result<std::fs::File> {
    let mut o = std::fs::OpenOptions::new();
    o.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    o.open(path).map_err(io)
}

/// Make a new directory entry durable (unix); a no-op elsewhere.
fn sync_dir(dir: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        std::fs::File::open(dir)
            .and_then(|d| d.sync_all())
            .map_err(io)?;
    }
    #[cfg(not(unix))]
    let _ = dir;
    Ok(())
}

/// About 60 characters either side of the first match of `q` (already lowercase) in `text`.
fn snippet_for(text: &str, q: &str) -> Option<String> {
    let lower: Vec<char> = text.chars().flat_map(char::to_lowercase).collect();
    let orig: Vec<char> = text.chars().collect();
    // Lowercasing can change the length of some characters; fall back to the lowered text then.
    let src = if lower.len() == orig.len() {
        &orig
    } else {
        &lower
    };
    let qv: Vec<char> = q.chars().collect();
    let pos = lower.windows(qv.len()).position(|w| w == qv.as_slice())?;
    let start = pos.saturating_sub(60);
    let end = (pos + qv.len() + 60).min(src.len());
    let mut s: String = src[start..end].iter().collect();
    if start > 0 {
        s.insert(0, '…');
    }
    if end < src.len() {
        s.push('…');
    }
    Some(s.replace(['\n', '\r'], " "))
}

/// `YYYY-MM-DD HH:MM UTC` for a millisecond timestamp.
pub fn utc_label(ms: u64) -> String {
    let secs = ms / 1000;
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    // Days since 1970-01-01 to a civil date (H. Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02} UTC",
        rem / 3600,
        (rem % 3600) / 60
    )
}

/// A code fence longer than any backtick run in `s`, so content cannot close it.
fn fence_for(s: &str) -> String {
    let mut longest = 0;
    let mut run = 0;
    for c in s.chars() {
        if c == '`' {
            run += 1;
            longest = longest.max(run);
        } else {
            run = 0;
        }
    }
    "`".repeat(longest.max(2) + 1)
}

fn fenced(label: &str, s: &str) -> String {
    let f = fence_for(s);
    format!("{f}{label}\n{s}\n{f}\n")
}

fn markdown_of(c: &Conversation) -> String {
    let s = &c.summary;
    let mut md = format!(
        "# {}\n\n- Source: {}\n- Provider: {}\n- Model: {}\n- Started: {}\n- Last activity: {}\n- Exported from Citrate Core. Sections marked *untrusted* came from tools or other outside sources.\n\n",
        s.title,
        match s.source {
            Source::Chat => "chat",
            Source::Agent => "agent session",
        },
        s.provider,
        s.model,
        utc_label(s.created_ms),
        utc_label(s.updated_ms),
    );
    for en in &c.entries {
        let when = utc_label(en.ts);
        match &en.record {
            Record::Turn { role, text, .. } => {
                let who = match role {
                    Role::User => "Member",
                    Role::Assistant => "Assistant",
                    Role::System => "System",
                };
                if en.untrusted {
                    md.push_str(&format!(
                        "## {who} ({when}), *untrusted: written after the session read outside content*\n\n"
                    ));
                } else {
                    md.push_str(&format!("## {who} ({when})\n\n"));
                }
                md.push_str(&fenced("text", text));
                md.push('\n');
            }
            Record::ToolCall {
                call_id,
                name,
                arguments,
            } => {
                md.push_str(&format!("### Tool call `{name}` ({call_id}, {when})\n\n"));
                md.push_str(&fenced("json", arguments));
                md.push('\n');
            }
            Record::ToolResult {
                call_id,
                status,
                content,
            } => {
                md.push_str(&format!(
                    "### Tool result ({call_id}, {status}, {when}), *untrusted*\n\n"
                ));
                md.push_str(&fenced("text", content));
                md.push('\n');
            }
            Record::Artifact {
                name,
                mime,
                size,
                sha256,
                origin,
                ..
            } => {
                let trust = if origin.untrusted() {
                    ", *untrusted*"
                } else {
                    ""
                };
                md.push_str(&format!(
                    "### Artifact ({when}{trust})\n\n- Name: {}\n- Type: {mime}\n- Size: {size} bytes\n- SHA-256: {sha256}\n- Content: in the JSON export\n\n",
                    name.replace(['\n', '\r'], " ")
                ));
            }
            Record::Route { provider, model } => {
                md.push_str(&format!(
                    "*Model changed to {model} ({provider}) at {when}.*\n\n"
                ));
            }
            Record::Taint { source } => {
                md.push_str(&format!(
                    "*The session read untrusted content from {source} at {when}.*\n\n"
                ));
            }
        }
    }
    md
}

#[cfg(test)]
pub(crate) mod tests {
    include!("history_tests.rs");
}
