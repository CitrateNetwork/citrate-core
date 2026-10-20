//! The record of signed transactions this app has handed to `eth_sendRawTransaction`
//! (UI truth contract v1.1 §1, chain#326).
//!
//! A node error proves a transaction was refused only on the FIRST submission of those signed
//! bytes: a re-send of bytes that were accepted and mined comes back with `NonceTooLow` or a
//! balance error, which look definitive. So the first-ness of a send must survive an app restart.
//! This keeps the keccak256 hashes of the last [`CAPACITY`] submissions in a small JSON file,
//! rewritten atomically (temp file in the same directory, fsync, rename) on each new hash.
//!
//! Fails safe: a file that exists but can't be read leaves the log *degraded*, and a degraded log
//! reports every send as a re-submission, so nothing is ever called "not sent" without proof. A
//! missing file is an empty log. A failed write keeps the in-memory record and is logged.
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
    degraded: bool,
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
                degraded: false,
            }),
        }
    }

    /// Load the record at `path` (a missing file is empty; an unreadable one is degraded).
    pub fn open(path: PathBuf) -> Self {
        let (hashes, degraded) = match std::fs::read(&path) {
            Ok(bytes) => match serde_json::from_slice::<Vec<String>>(&bytes) {
                Ok(list) => {
                    let mut q: VecDeque<String> =
                        list.into_iter().map(|h| h.to_ascii_lowercase()).collect();
                    while q.len() > CAPACITY {
                        q.pop_front();
                    }
                    (q, false)
                }
                Err(_) => (VecDeque::new(), true),
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (VecDeque::new(), false),
            Err(_) => (VecDeque::new(), true),
        };
        SubmittedTxLog {
            path: Some(path),
            inner: Mutex::new(Inner { hashes, degraded }),
        }
    }

    /// Record `hash` as submitted. Returns true only when this is provably its first submission:
    /// never seen before, and the record is not degraded.
    pub fn record(&self, hash: &str) -> bool {
        let hash = hash.to_ascii_lowercase();
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if g.hashes.iter().any(|h| *h == hash) {
            return false;
        }
        g.hashes.push_back(hash);
        while g.hashes.len() > CAPACITY {
            g.hashes.pop_front();
        }
        let first = !g.degraded;
        if let Some(path) = &self.path {
            let list: Vec<&String> = g.hashes.iter().collect();
            if let Err(e) = write_atomic(path, &list) {
                eprintln!("[ceremony] could not persist the submitted-transaction record: {e}");
            }
        }
        first
    }

    /// Record `hash` before it is broadcast. `Ok(true)` only when this is provably a first
    /// submission; `Err` when the record could not be made durable (the caller must not send).
    pub fn try_record(&self, hash: &str) -> Result<bool, String> {
        Ok(self.record(hash))
    }

    /// True when the history can't prove an absent hash was never sent (corrupt, unreadable or
    /// truncated history): every send is then a re-submission.
    pub fn is_uncertain(&self) -> bool {
        self.is_degraded()
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

    /// True when the file existed but could not be read: every send is then a re-submission.
    pub fn is_degraded(&self) -> bool {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .degraded
    }
}

/// Write `list` as JSON to `path` via a temp file in the same directory, fsync, then rename.
fn write_atomic(path: &std::path::Path, list: &[&String]) -> std::io::Result<()> {
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
    let body = serde_json::to_vec(list).map_err(std::io::Error::other)?;
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
