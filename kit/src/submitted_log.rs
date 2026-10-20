//! The record of signed transactions this app has handed to `eth_sendRawTransaction`
//! (UI truth contract v1.1 §1, chain#326).
//!
//! A node error proves a transaction was refused only on the FIRST submission of those signed
//! bytes: a re-send of bytes that were accepted and mined comes back with `NonceTooLow` or a
//! balance error, which look definitive. So the first-ness of a send must survive an app restart.
//! This keeps the keccak256 hashes of the last [`CAPACITY`] submissions in a small JSON file,
//! `{"hashes": [...], "uncertain": bool}`, rewritten atomically (temp file in the same directory,
//! fsync, rename) BEFORE each broadcast.
//!
//! The rule: when the history can't prove an absent hash was never sent, the send is a
//! re-submission ("outcome unknown"), never a first send ("not sent"). The history is *uncertain*,
//! and stays uncertain across rewrites and restarts, once any of these happened:
//! - a hash was evicted to keep the record at [`CAPACITY`];
//! - the file existed but could not be read or parsed;
//! - a pre-flag (bare list) file was already at capacity, so it may have evicted hashes.
//!
//! A missing file is an empty, certain record. A hash whose record can't be made durable is
//! refused ([`SubmittedTxLog::try_record`] returns `Err`) and not kept: the caller must not send.
//! Hashes are public transaction ids, not key material.

use std::collections::VecDeque;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

/// How many recent submissions are kept.
pub const CAPACITY: usize = 256;

#[derive(Debug)]
struct Inner {
    hashes: VecDeque<String>,
    /// The history can't prove an absent hash was never sent (see the module docs).
    uncertain: bool,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct OnDisk {
    hashes: Vec<String>,
    uncertain: bool,
}

/// The submitted-transaction record. `path` = None keeps it in memory only (tests, and any
/// ceremony built without an app data dir).
#[derive(Debug)]
pub struct SubmittedTxLog {
    path: Option<PathBuf>,
    inner: Mutex<Inner>,
}

impl SubmittedTxLog {
    /// A record that lives only as long as the process.
    pub fn in_memory() -> Self {
        SubmittedTxLog {
            path: None,
            inner: Mutex::new(Inner {
                hashes: VecDeque::new(),
                uncertain: false,
            }),
        }
    }

    /// Load the record at `path`. A missing file is empty and certain; an unreadable or corrupt
    /// one is empty and uncertain.
    pub fn open(path: PathBuf) -> Self {
        let (list, mut uncertain) = match std::fs::read(&path) {
            Ok(bytes) => match serde_json::from_slice::<OnDisk>(&bytes) {
                Ok(d) => (d.hashes, d.uncertain),
                // The bare-list format from before the flag: at capacity it may have evicted.
                Err(_) => match serde_json::from_slice::<Vec<String>>(&bytes) {
                    Ok(list) => {
                        let full = list.len() >= CAPACITY;
                        (list, full)
                    }
                    Err(_) => (Vec::new(), true),
                },
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Vec::new(), false),
            Err(_) => (Vec::new(), true),
        };
        let mut hashes: VecDeque<String> =
            list.into_iter().map(|h| h.to_ascii_lowercase()).collect();
        while hashes.len() > CAPACITY {
            hashes.pop_front();
            uncertain = true;
        }
        SubmittedTxLog {
            path: Some(path),
            inner: Mutex::new(Inner { hashes, uncertain }),
        }
    }

    /// Record `hash` BEFORE it is broadcast. `Ok(true)` only when this is provably its first
    /// submission (never seen, history certain) and the record is durable. `Ok(false)` for a
    /// re-submission or an uncertain history. `Err` when the record could not be written: the
    /// hash is not kept, and the caller must not send.
    pub fn try_record(&self, hash: &str) -> Result<bool, String> {
        let hash = hash.to_ascii_lowercase();
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if g.hashes.iter().any(|h| *h == hash) {
            return Ok(false);
        }
        let mut next = g.hashes.clone();
        next.push_back(hash);
        let mut uncertain = g.uncertain;
        while next.len() > CAPACITY {
            next.pop_front();
            uncertain = true;
        }
        if let Some(path) = &self.path {
            let disk = OnDisk {
                hashes: next.iter().cloned().collect(),
                uncertain,
            };
            write_atomic(path, &disk)
                .map_err(|e| format!("could not write {}: {e}", path.display()))?;
        }
        let first = !g.uncertain;
        g.hashes = next;
        g.uncertain = uncertain;
        Ok(first)
    }

    /// [`Self::try_record`] for a transaction whose sender and nonce are known.
    pub fn try_record_for(&self, hash: &str, sender: &str, nonce: u64) -> Result<bool, String> {
        let _ = (sender, nonce);
        self.try_record(hash)
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
            .hashes
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

/// Write `disk` as JSON to `path` via a temp file in the same directory, fsync, then rename.
fn write_atomic(path: &std::path::Path, disk: &OnDisk) -> std::io::Result<()> {
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
    })
}

#[cfg(test)]
mod tests {
    include!("submitted_log_tests.rs");
}
