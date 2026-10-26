//! The record of signed transactions this app has handed to `eth_sendRawTransaction`
//! (UI truth contract v1.1 §1, chain#326).
//!
//! A node error proves a transaction was refused only on the FIRST submission of those signed
//! bytes: a re-send of bytes that were accepted and mined comes back with `NonceTooLow` or a
//! balance error, which look definitive. So the first-ness of a send must survive an app restart.
//! This keeps the keccak256 hashes of the last [`CAPACITY`] submissions in a small JSON file,
//! rewritten atomically and durably (temp file in the same directory, fsync, rename, fsync of the
//! directory) BEFORE each broadcast:
//!
//! ```text
//! {"genesis": G, "entries": [{hash, sender, nonce, genesis: G}], "evicted": {sender: nonce},
//!  "uncertain": bool, "sent_on_previous_networks": [{genesis, entries, evicted, uncertain}]}
//! ```
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
//!
//! # Keyed to the genesis (#511)
//!
//! The record is opened with the genesis of the network every send is gated to, and each entry is
//! written with the genesis it was sent on. A record written before this ([`LEGACY_GENESIS`]) is
//! r1005's. Entries, nonce marks and uncertainty of another genesis are moved, when the record
//! opens, to `sent_on_previous_networks` (written at once, in the same atomic write): history that
//! is never looked up and takes no part in this network's nonce marks or uncertainty. Opening on an
//! archived genesis again makes its section live again. One thing still crosses networks: the
//! chain id stays the same across a genesis switch, so bytes broadcast on the previous network are
//! valid bytes on this one, and a send of byte-identical bytes is never a first submission.

use std::collections::{BTreeMap, VecDeque};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

/// How many recent submissions are kept.
pub const CAPACITY: usize = 256;

/// The genesis of a record written before entries were keyed to a genesis: the r1005 testnet
/// (`alpha-r1005` in src-tauri/network/profiles.json), the only network those builds sent to.
pub const LEGACY_GENESIS: &str =
    "0x1dcfc4909dd9fdcb0f03c823d627e4b2032cc86bef5fc4561030d6c64b9775f1";

/// One submitted transaction: its hash, and its sender and nonce when known. Its genesis is the
/// one of the [`Section`] that holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    hash: String,
    sender: Option<String>,
    nonce: Option<u64>,
}

/// The submissions on one network (one genesis).
#[derive(Debug, Clone, Default)]
struct Section {
    entries: VecDeque<Entry>,
    /// Per sender (lower case): the highest nonce among evicted entries.
    evicted: BTreeMap<String, u64>,
    /// The history can't prove any absent hash was never sent (see the module docs).
    uncertain: bool,
}

impl Section {
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

#[derive(Debug, Clone, Default)]
struct Inner {
    /// The network the record was opened on.
    live: Section,
    /// Sent on previous networks, by genesis: history only (see the module docs).
    previous: BTreeMap<String, Section>,
}

impl Inner {
    /// Whether these bytes were recorded, on this network or a previous one.
    fn contains(&self, hash: &str) -> bool {
        self.live.contains(hash) || self.previous.values().any(|s| s.contains(hash))
    }
}

/// An entry as read. `genesis` is absent in records written before #511.
#[derive(serde::Deserialize)]
struct EntryIn {
    hash: String,
    #[serde(default)]
    sender: Option<String>,
    #[serde(default)]
    nonce: Option<u64>,
    #[serde(default)]
    genesis: Option<String>,
}

/// An archived section as read: its genesis is required.
#[derive(serde::Deserialize)]
struct SectionIn {
    genesis: String,
    entries: Vec<EntryIn>,
    #[serde(default)]
    evicted: BTreeMap<String, u64>,
    #[serde(default)]
    uncertain: bool,
}

/// The record as read. `genesis` is absent in records written before #511 ([`LEGACY_GENESIS`]).
#[derive(serde::Deserialize)]
struct OnDisk {
    #[serde(default)]
    genesis: Option<String>,
    entries: Vec<EntryIn>,
    #[serde(default)]
    evicted: BTreeMap<String, u64>,
    uncertain: bool,
    #[serde(default)]
    sent_on_previous_networks: Vec<SectionIn>,
}

/// The format before sender/nonce were kept: `{hashes, uncertain}`.
#[derive(serde::Deserialize)]
struct OnDiskV2 {
    hashes: Vec<String>,
    uncertain: bool,
}

/// An entry as written: it always names its genesis.
#[derive(serde::Serialize)]
struct EntryOut<'a> {
    hash: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    sender: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    nonce: Option<u64>,
    genesis: &'a str,
}

/// An archived section as written.
#[derive(serde::Serialize)]
struct SectionOut<'a> {
    genesis: &'a str,
    entries: Vec<EntryOut<'a>>,
    evicted: &'a BTreeMap<String, u64>,
    uncertain: bool,
}

/// The record as written.
#[derive(serde::Serialize)]
struct OnDiskOut<'a> {
    genesis: &'a str,
    entries: Vec<EntryOut<'a>>,
    evicted: &'a BTreeMap<String, u64>,
    uncertain: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    sent_on_previous_networks: Vec<SectionOut<'a>>,
}

fn entries_out<'a>(s: &'a Section, genesis: &'a str) -> Vec<EntryOut<'a>> {
    s.entries
        .iter()
        .map(|e| EntryOut {
            hash: &e.hash,
            sender: e.sender.as_deref(),
            nonce: e.nonce,
            genesis,
        })
        .collect()
}

impl Inner {
    /// The on-disk form of this record, opened on `genesis`.
    fn to_disk<'a>(&'a self, genesis: &'a str) -> OnDiskOut<'a> {
        OnDiskOut {
            genesis,
            entries: entries_out(&self.live, genesis),
            evicted: &self.live.evicted,
            uncertain: self.live.uncertain,
            sent_on_previous_networks: self
                .previous
                .iter()
                .map(|(g, s)| SectionOut {
                    genesis: g,
                    entries: entries_out(s, g),
                    evicted: &s.evicted,
                    uncertain: s.uncertain,
                })
                .collect(),
        }
    }
}

/// The sections a record file holds, by genesis, before they are sorted into live and previous.
#[derive(Default)]
struct Loaded {
    sections: BTreeMap<String, Section>,
    /// The genesis of the file's live section (`None`: no file, or not readable).
    record: Option<String>,
    /// The genesis hashes of the file's archived sections.
    archived: std::collections::BTreeSet<String>,
}

impl Loaded {
    fn section(&mut self, genesis: &str) -> &mut Section {
        self.sections
            .entry(genesis.to_ascii_lowercase())
            .or_default()
    }

    fn push(&mut self, genesis: &str, e: EntryIn) {
        self.section(genesis).entries.push_back(Entry {
            hash: e.hash.to_ascii_lowercase(),
            sender: e.sender.map(|s| s.to_ascii_lowercase()),
            nonce: e.nonce,
        });
    }

    fn marks(&mut self, genesis: &str, evicted: BTreeMap<String, u64>, uncertain: bool) {
        let s = self.section(genesis);
        for (sender, n) in evicted {
            let hw = s.evicted.entry(sender.to_ascii_lowercase()).or_insert(n);
            *hw = (*hw).max(n);
        }
        s.uncertain |= uncertain;
    }

    /// The sections of a record in the current format. An entry with no genesis is its record's;
    /// a record with no genesis is r1005's.
    fn from_disk(d: OnDisk) -> Self {
        let mut l = Loaded::default();
        let record = d
            .genesis
            .unwrap_or_else(|| LEGACY_GENESIS.to_string())
            .to_ascii_lowercase();
        l.section(&record);
        l.record = Some(record.clone());
        for e in d.entries {
            let g = e.genesis.clone().unwrap_or_else(|| record.clone());
            l.push(&g, e);
        }
        l.marks(&record, d.evicted, d.uncertain);
        for p in d.sent_on_previous_networks {
            l.section(&p.genesis);
            l.archived.insert(p.genesis.to_ascii_lowercase());
            for e in p.entries {
                let g = e.genesis.clone().unwrap_or_else(|| p.genesis.clone());
                l.push(&g, e);
            }
            l.marks(&p.genesis, p.evicted, p.uncertain);
        }
        l
    }

    /// A record in an earlier format (hashes only), which is r1005's.
    fn legacy(hashes: Vec<String>, uncertain: bool) -> Self {
        let mut l = Loaded {
            record: Some(LEGACY_GENESIS.to_string()),
            ..Loaded::default()
        };
        for hash in hashes {
            l.push(
                LEGACY_GENESIS,
                EntryIn {
                    hash,
                    sender: None,
                    nonce: None,
                    genesis: None,
                },
            );
        }
        l.marks(LEGACY_GENESIS, BTreeMap::new(), uncertain);
        l
    }

    /// An empty record on `genesis`.
    fn empty(genesis: &str, uncertain: bool) -> Self {
        let mut l = Loaded::default();
        l.marks(genesis, BTreeMap::new(), uncertain);
        l
    }
}

/// Makes a rename inside `dir` durable (see [`sync_dir`]). A parameter so tests can observe the
/// order of the write and make the sync fail.
type SyncDir = fn(&std::path::Path) -> std::io::Result<()>;

/// Where a persisted record lives, and the genesis it was opened on.
#[derive(Debug)]
struct Store {
    path: PathBuf,
    genesis: String,
}

/// The submitted-transaction record. `store` = None keeps it in memory only (tests, and any
/// ceremony built without an app data dir or a network genesis).
#[derive(Debug)]
pub struct SubmittedTxLog {
    store: Option<Store>,
    inner: Mutex<Inner>,
    sync_dir: SyncDir,
    /// How many attempts in this process recorded each hash (#347 Codex P1). Not persisted: it
    /// tells an attempt whether another one may have sent the same bytes while it was sending.
    attempts: Mutex<BTreeMap<String, u32>>,
}

impl SubmittedTxLog {
    /// A record that lives only as long as the process.
    ///
    /// Test-only: a certain record that does not survive a restart is never valid in the app,
    /// which uses [`Self::open`] or [`Self::unpersisted`].
    #[cfg(test)]
    pub(crate) fn in_memory() -> Self {
        SubmittedTxLog {
            store: None,
            inner: Mutex::new(Inner::default()),
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
            store: None,
            inner: Mutex::new(Inner {
                live: Section {
                    uncertain: true,
                    ..Section::default()
                },
                previous: BTreeMap::new(),
            }),
            sync_dir,
            attempts: Default::default(),
        }
    }

    /// Load the record at `path` for the network whose block-0 hash is `genesis`. A missing file
    /// is empty and certain (uncertain next to an existing wallet); an unreadable or corrupt one is
    /// empty and uncertain. Sections of other genesis hashes become history (see the module docs).
    pub fn open(path: PathBuf, genesis: &str) -> Self {
        Self::open_with(path, genesis, sync_dir)
    }

    /// [`Self::open`] with the directory sync given.
    fn open_with(path: PathBuf, genesis: &str, sync_dir: SyncDir) -> Self {
        let genesis = genesis.to_ascii_lowercase();
        let mut missing = false;
        let loaded = match std::fs::read(&path) {
            Ok(bytes) => {
                if let Ok(d) = serde_json::from_slice::<OnDisk>(&bytes) {
                    Loaded::from_disk(d)
                } else if let Ok(d) = serde_json::from_slice::<OnDiskV2>(&bytes) {
                    Loaded::legacy(d.hashes, d.uncertain)
                } else if let Ok(list) = serde_json::from_slice::<Vec<String>>(&bytes) {
                    // The bare-list format from before the flag: at capacity it may have evicted.
                    let full = list.len() >= CAPACITY;
                    Loaded::legacy(list, full)
                } else {
                    Loaded::empty(&genesis, true)
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
                Loaded::empty(&genesis, wallet_exists)
            }
            Err(_) => Loaded::empty(&genesis, true),
        };
        // Sections of another genesis than `genesis` were moved to history, or back from it, when
        // the file's layout differs from the one it is written in now.
        let Loaded {
            mut sections,
            record,
            archived,
        } = loaded;
        let mut live = sections.remove(&genesis).unwrap_or_default();
        live.evict_to_capacity();
        for s in sections.values_mut() {
            s.evict_to_capacity();
        }
        let moved = record.is_some_and(|r| r != genesis)
            || archived.contains(&genesis)
            || !sections.keys().eq(archived.iter());
        let mut inner = Inner {
            live,
            previous: sections,
        };
        let store = Store { path, genesis };
        // A missing record is written at once (certain for a new profile, uncertain for an
        // upgraded one), so a wallet created later in this run never makes the next launch look
        // like an upgrade. If it can't be written, nothing proves the history: uncertain.
        // Moved sections are written at once too, in the same atomic write. If that fails, the
        // next record writes them (every record write carries the whole record).
        if missing || moved {
            if let Err(e) = write_atomic(&store.path, &inner.to_disk(&store.genesis), sync_dir) {
                if missing {
                    inner.live.uncertain = true;
                } else {
                    eprintln!(
                        "[submitted-tx] could not write {} after a network switch: {e}",
                        store.path.display()
                    );
                }
            }
        }
        SubmittedTxLog {
            store: Some(store),
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
        let first = g
            .live
            .absent_is_first(origin.as_ref().map(|(s, n)| (s.as_str(), *n)));
        let hash_key = hash.clone();
        let mut next = g.clone();
        next.live.entries.push_back(Entry {
            hash,
            sender: origin.as_ref().map(|(s, _)| s.clone()),
            nonce: origin.as_ref().map(|(_, n)| *n),
        });
        next.live.evict_to_capacity();
        if let Some(store) = &self.store {
            write_atomic(&store.path, &next.to_disk(&store.genesis), self.sync_dir)
                .map_err(|e| format!("could not write {}: {e}", store.path.display()))?;
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

    /// How many hashes the record holds for the network it was opened on.
    pub fn len(&self) -> usize {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .live
            .entries
            .len()
    }

    /// True when the record holds no hashes for the network it was opened on.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// True when the history on the network the record was opened on can't prove an absent hash
    /// was never sent: every send is then a re-submission.
    pub fn is_uncertain(&self) -> bool {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .live
            .uncertain
    }
}

/// Write `disk` as JSON to `path` via a temp file in the same directory, fsync, rename, then fsync
/// the directory so the rename itself survives a power loss. Any failure is an error: the caller
/// must not broadcast on a record that isn't durable.
fn write_atomic(
    path: &std::path::Path,
    disk: &OnDiskOut,
    sync_dir: SyncDir,
) -> std::io::Result<()> {
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
