//! The record of signed transactions this app has handed to `eth_sendRawTransaction`
//! (UI truth contract v1.1 §1, chain#326).
//!
//! A node error proves a transaction was refused only on the FIRST submission of those signed
//! bytes: a re-send of bytes that were accepted and mined comes back with `NonceTooLow` or a
//! balance error, which look definitive. So the first-ness of a send must survive an app restart.
//! This keeps the keccak256 hashes of the last [`CAPACITY`] submissions in a small JSON file,
//! `{"entries": [{hash, sender, nonce}], "evicted": {sender: nonce}, "uncertain": bool}`, rewritten
//! atomically and durably (temp file in the same directory, fsync, rename, fsync of the directory)
//! BEFORE each broadcast.
//!
//! The rule: when the history can't prove an absent hash was never sent, the send is a
//! re-submission ("outcome unknown"), never a first send ("not sent").
//!
//! Each entry keeps the tx's sender and nonce when known. Evicting an entry to keep the record at
//! [`CAPACITY`] raises that sender's *evicted nonce high-water mark*. Signed bytes fix their nonce,
//! so an absent hash from a sender at a nonce ABOVE that mark can't be a resend of an evicted tx
//! and is still a first submission; at or below it, it is unknown.
//!
//! The whole history is *uncertain* (every send unknown), across rewrites and restarts, once:
//! - an entry with no sender/nonce was evicted (it can't be bounded);
//! - the file existed but could not be read or parsed;
//! - a pre-flag (bare list) file was already at capacity, so it may have evicted hashes.
//!
//! Uncertainty is never cleared: once set it is written back on every rewrite, so after one corrupt
//! or unreadable file no send is reported as "not sent" again (every error is "outcome unknown").
//! That fails closed by design.
//!
//! A missing file is an empty, certain record only for a profile with no wallet yet; next to an
//! existing custody envelope (an upgrade from a build that never wrote this record) it is
//! uncertain. Either way it is written at once. A hash whose record can't be made durable is
//! refused ([`SubmittedTxLog::try_record`] returns `Err`) and not kept: the caller must not send.
//! Hashes are public transaction ids, not key material.

use std::collections::VecDeque;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

/// How many recent submissions are kept.
pub const CAPACITY: usize = 256;

/// The genesis of a record written before entries were keyed to a genesis: the r1005 testnet
/// (`alpha-r1005` in src-tauri/network/profiles.json), the only network those builds sent to.
pub const LEGACY_GENESIS: &str =
    "0x1dcfc4909dd9fdcb0f03c823d627e4b2032cc86bef5fc4561030d6c64b9775f1";

/// One submitted transaction: its hash, and its sender and nonce when known.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct Entry {
    hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sender: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    nonce: Option<u64>,
}

#[derive(Debug, Clone)]
struct Inner {
    entries: VecDeque<Entry>,
    /// Per sender (lower case): the highest nonce among evicted entries.
    evicted: std::collections::BTreeMap<String, u64>,
    /// The history can't prove any absent hash was never sent (see the module docs).
    uncertain: bool,
}

impl Inner {
    fn contains(&self, hash: &str) -> bool {
        self.entries.iter().any(|e| e.hash == hash)
    }

    /// Whether an absent tx is provably a first submission.
    fn absent_is_first(&self, origin: Option<(&str, u64)>) -> bool {
        if self.uncertain {
            return false;
        }
        match origin {
            Some((sender, nonce)) => self.evicted.get(sender).is_none_or(|hw| nonce > *hw),
            None => self.evicted.is_empty(),
        }
    }

    /// Drop the oldest entries down to CAPACITY, raising the high-water marks.
    fn evict_to_capacity(&mut self) {
        while self.entries.len() > CAPACITY {
            if let Some(e) = self.entries.pop_front() {
                match (e.sender, e.nonce) {
                    (Some(s), Some(n)) => {
                        let hw = self.evicted.entry(s).or_insert(n);
                        *hw = (*hw).max(n);
                    }
                    _ => self.uncertain = true,
                }
            }
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct OnDisk {
    entries: Vec<Entry>,
    #[serde(default)]
    evicted: std::collections::BTreeMap<String, u64>,
    uncertain: bool,
}

/// The format before sender/nonce were kept: `{hashes, uncertain}`.
#[derive(serde::Deserialize)]
struct OnDiskV2 {
    hashes: Vec<String>,
    uncertain: bool,
}

/// Makes a rename inside `dir` durable (see [`sync_dir`]). A parameter so tests can observe the
/// order of the write and make the sync fail.
type SyncDir = fn(&std::path::Path) -> std::io::Result<()>;

/// The submitted-transaction record. `path` = None keeps it in memory only (tests, and any
/// ceremony built without an app data dir).
#[derive(Debug)]
pub struct SubmittedTxLog {
    path: Option<PathBuf>,
    inner: Mutex<Inner>,
    sync_dir: SyncDir,
    /// How many attempts in this process recorded each hash (#347 Codex P1). Not persisted: it
    /// tells an attempt whether another one may have sent the same bytes while it was sending.
    attempts: Mutex<std::collections::BTreeMap<String, u32>>,
}

impl SubmittedTxLog {
    /// A record that lives only as long as the process.
    ///
    /// Test-only: a certain record that does not survive a restart is never valid in the app,
    /// which uses [`Self::open`] or [`Self::unpersisted`].
    #[cfg(test)]
    pub(crate) fn in_memory() -> Self {
        SubmittedTxLog {
            path: None,
            inner: Mutex::new(Inner {
                entries: VecDeque::new(),
                evicted: Default::default(),
                uncertain: false,
            }),
            sync_dir,
            attempts: Default::default(),
        }
    }

    /// The record used when no persistent location exists (the app data dir could not be
    /// resolved). It lives only as long as the process, so it can't show that bytes were never
    /// sent in an earlier run: it is uncertain from the start, and every send is a re-submission
    /// ("outcome unknown" on any error, never "not sent").
    pub fn unpersisted() -> Self {
        SubmittedTxLog {
            path: None,
            inner: Mutex::new(Inner {
                entries: VecDeque::new(),
                evicted: Default::default(),
                uncertain: true,
            }),
            sync_dir,
            attempts: Default::default(),
        }
    }

    /// Load the record at `path`. A missing file is empty and certain; an unreadable or corrupt
    /// one is empty and uncertain.
    pub fn open(path: PathBuf, genesis: &str) -> Self {
        Self::open_with(path, genesis, sync_dir)
    }

    /// [`Self::open`] with the directory sync given.
    fn open_with(path: PathBuf, _genesis: &str, sync_dir: SyncDir) -> Self {
        let bare = |hashes: Vec<String>| -> Vec<Entry> {
            hashes
                .into_iter()
                .map(|hash| Entry {
                    hash,
                    sender: None,
                    nonce: None,
                })
                .collect()
        };
        let mut missing = false;
        let (entries, evicted, uncertain) = match std::fs::read(&path) {
            Ok(bytes) => {
                if let Ok(d) = serde_json::from_slice::<OnDisk>(&bytes) {
                    (d.entries, d.evicted, d.uncertain)
                } else if let Ok(d) = serde_json::from_slice::<OnDiskV2>(&bytes) {
                    (bare(d.hashes), Default::default(), d.uncertain)
                } else if let Ok(list) = serde_json::from_slice::<Vec<String>>(&bytes) {
                    // The bare-list format from before the flag: at capacity it may have evicted.
                    let full = list.len() >= CAPACITY;
                    (bare(list), Default::default(), full)
                } else {
                    (Vec::new(), Default::default(), true)
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // #347 Codex P1: builds before this record never wrote it. A profile that
                // already holds a wallet (the custody envelope in the same app data dir) may
                // have submitted txs with an unknown outcome, so its missing record is NOT a
                // certain empty history. Only a profile with no wallet yet starts certain.
                missing = true;
                let wallet_exists = path
                    .parent()
                    .map(|d| d.join(crate::custody::ENVELOPE_FILE))
                    .is_some_and(|p| p.exists());
                (Vec::new(), Default::default(), wallet_exists)
            }
            Err(_) => (Vec::new(), Default::default(), true),
        };
        let mut inner = Inner {
            entries: entries
                .into_iter()
                .map(|e| Entry {
                    hash: e.hash.to_ascii_lowercase(),
                    sender: e.sender.map(|s| s.to_ascii_lowercase()),
                    nonce: e.nonce,
                })
                .collect(),
            evicted: evicted
                .into_iter()
                .map(|(k, v)| (k.to_ascii_lowercase(), v))
                .collect(),
            uncertain,
        };
        inner.evict_to_capacity();
        // A missing record is written at once (certain for a new profile, uncertain for an
        // upgraded one), so a wallet created later in this run never makes the next launch look
        // like an upgrade. If it can't be written, nothing proves the history: uncertain.
        if missing {
            let disk = OnDisk {
                entries: Vec::new(),
                evicted: Default::default(),
                uncertain: inner.uncertain,
            };
            if write_atomic(&path, &disk, sync_dir).is_err() {
                inner.uncertain = true;
            }
        }
        SubmittedTxLog {
            path: Some(path),
            inner: Mutex::new(inner),
            sync_dir,
            attempts: Default::default(),
        }
    }

    /// Record `hash` BEFORE it is broadcast. `Ok(true)` only when this is provably its first
    /// submission (never seen, history certain) and the record is durable. `Ok(false)` for a
    /// re-submission or an uncertain history. `Err` when the record could not be written: the
    /// hash is not kept, and the caller must not send.
    pub fn try_record(&self, hash: &str) -> Result<bool, String> {
        self.record_entry(hash, None)
    }

    /// [`Self::try_record`] for a transaction whose sender and nonce are known, so an eviction
    /// can be bounded by nonce instead of making the whole history uncertain.
    pub fn try_record_for(&self, hash: &str, sender: &str, nonce: u64) -> Result<bool, String> {
        self.record_entry(hash, Some((sender, nonce)))
    }

    fn record_entry(&self, hash: &str, origin: Option<(&str, u64)>) -> Result<bool, String> {
        let hash = hash.to_ascii_lowercase();
        let origin = origin.map(|(s, n)| (s.to_ascii_lowercase(), n));
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if g.contains(&hash) {
            self.count_attempt(&hash);
            return Ok(false);
        }
        let first = g.absent_is_first(origin.as_ref().map(|(s, n)| (s.as_str(), *n)));
        let hash_key = hash.clone();
        let mut next = g.clone();
        next.entries.push_back(Entry {
            hash,
            sender: origin.as_ref().map(|(s, _)| s.clone()),
            nonce: origin.as_ref().map(|(_, n)| *n),
        });
        next.evict_to_capacity();
        if let Some(path) = &self.path {
            let disk = OnDisk {
                entries: next.entries.iter().cloned().collect(),
                evicted: next.evicted.clone(),
                uncertain: next.uncertain,
            };
            write_atomic(path, &disk, self.sync_dir)
                .map_err(|e| format!("could not write {}: {e}", path.display()))?;
        }
        *g = next;
        self.count_attempt(&hash_key);
        Ok(first)
    }

    fn count_attempt(&self, hash: &str) {
        let mut a = self.attempts.lock().unwrap_or_else(|e| e.into_inner());
        *a.entry(hash.to_string()).or_insert(0) += 1;
    }

    /// True when this process recorded `hash` exactly once: no other attempt may have sent the
    /// same signed bytes. A first submission whose send is refused may be reported as "not sent"
    /// only while this holds; once a second attempt recorded the hash, the refusal can be the node
    /// knowing (or having mined) bytes the other attempt sent (#347 Codex P1).
    pub fn sole_attempt(&self, hash: &str) -> bool {
        let a = self.attempts.lock().unwrap_or_else(|e| e.into_inner());
        a.get(&hash.to_ascii_lowercase()).copied() == Some(1)
    }

    /// [`Self::try_record`] for callers that only need first-ness. A failed write is never a
    /// first submission (the hash is not kept).
    pub fn record(&self, hash: &str) -> bool {
        self.try_record(hash).unwrap_or(false)
    }

    /// How many hashes the record holds.
    pub fn len(&self) -> usize {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entries
            .len()
    }

    /// True when the record holds no hashes.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// True when the history can't prove an absent hash was never sent: every send is then a
    /// re-submission.
    pub fn is_uncertain(&self) -> bool {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .uncertain
    }
}

/// Write `disk` as JSON to `path` via a temp file in the same directory, fsync, rename, then fsync
/// the directory so the rename itself survives a power loss. Any failure is an error: the caller
/// must not broadcast on a record that isn't durable.
fn write_atomic(path: &std::path::Path, disk: &OnDisk, sync_dir: SyncDir) -> std::io::Result<()> {
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(std::path::Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    crate::fsutil::ensure_private_dir(&dir)?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "submitted-tx.json".to_string());
    let tmp = dir.join(format!(".{name}.tmp-{}", std::process::id()));
    let body = serde_json::to_vec(disk).map_err(std::io::Error::other)?;
    {
        let mut f = crate::fsutil::create_secret_file(&tmp)?;
        f.write_all(&body)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })?;
    sync_dir(&dir)
}

/// Make a rename inside `dir` durable: fsync the directory (on macOS `sync_all` is
/// `F_FULLFSYNC`). A failure is returned, never ignored.
#[cfg(unix)]
fn sync_dir(dir: &std::path::Path) -> std::io::Result<()> {
    std::fs::File::open(dir)?.sync_all()
}

/// Non-unix: a directory can't be opened for fsync through std. This app ships on macOS and
/// Linux; on other platforms the rename is as durable as the filesystem makes it, and the
/// directory is still required to exist.
#[cfg(not(unix))]
fn sync_dir(dir: &std::path::Path) -> std::io::Result<()> {
    std::fs::metadata(dir).map(|_| ())
}

#[cfg(test)]
mod tests {
    include!("submitted_log_tests.rs");
}
