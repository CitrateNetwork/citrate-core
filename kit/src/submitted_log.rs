//! The record of signed transactions this app has handed to `eth_sendRawTransaction`
//! (UI truth contract v1.1 §1, chain#326).
//!
//! A node error proves a transaction was refused only on the FIRST submission of those signed
//! bytes: a re-send of bytes that were accepted and mined comes back with `NonceTooLow` or a
//! balance error, which look definitive. So the first-ness of a send must survive an app restart.
//! This keeps the keccak256 hashes of the last [`CAPACITY`] submissions per network in a small
//! JSON file, [`RECORD_FILE`], rewritten atomically and durably (temp file in the same directory,
//! fsync, rename, fsync of the directory) BEFORE each broadcast.
//!
//! The rule: when the history can't prove an absent hash was never sent, the send is a
//! re-submission ("outcome unknown"), never a first send ("not sent").
//!
//! # One section per network (#511)
//!
//! The record is partitioned by network: one *section* per genesis (block-0 hash). The record is
//! opened with the genesis of the network every send is gated to; that network's section is the
//! *current* one, and every other section is *archived*: kept read-only, as history of a previous
//! network. Everything in this module's docs below (entries, nonce marks, uncertainty, the upgrade
//! bound) is per section. When the network changes, the whole section moves to the archive
//! together, in one atomic write at open; opening on an archived genesis again makes its section
//! current again, as it was.
//!
//! Every decision about the current network (first-ness, [`SubmittedTxLog::latest_call`],
//! [`SubmittedTxLog::forget`], [`SubmittedTxLog::is_uncertain`], [`SubmittedTxLog::len`]) reads
//! only the current section. An archived section can only *withhold* certainty, never give it or
//! an outcome: the chain id is the same across a genesis switch, so bytes broadcast on a previous
//! network are valid bytes on this one (anyone may have replayed them here), and a send of
//! byte-identical bytes is never a provable first submission. That is the one read of the archive
//! ([`Inner::sent_on_a_previous_network`]); a tripwire test keeps it the only one.
//!
//! # Within a section
//!
//! Each entry keeps the tx's sender and nonce when known. Evicting an entry to keep the section at
//! [`CAPACITY`] raises that sender's *evicted nonce high-water mark*. Signed bytes fix their nonce,
//! so an absent hash from a sender at a nonce ABOVE that mark can't be a resend of an evicted tx
//! and is still a first submission; at or below it, it is unknown.
//!
//! The whole section is *uncertain* (every send unknown), across rewrites and restarts, once:
//! - an entry with no sender/nonce was evicted (it can't be bounded);
//! - the record existed but could not be read or parsed (then every network is: see below);
//! - a pre-flag (bare list) legacy file was already at capacity, so it may have evicted hashes.
//!
//! Uncertainty is never cleared: once set it is written back on every rewrite, so after one corrupt
//! or unreadable file no send is reported as "not sent" again (every error is "outcome unknown").
//! That fails closed by design. A damaged record also loses the other networks' sections, so it
//! sets `history_lost`: every section that record did not hold, created later, starts uncertain.
//!
//! A missing record is an empty, certain section only for a profile with no wallet yet. Next to an
//! existing custody envelope (an upgrade from a build that never wrote a record) the earlier runs'
//! sends are unknown, but only up to a bound (#443): each sender's first send after the upgrade at
//! a nonce the app read from the node ([`NonceSource::Fetched`]) sets that sender's *upgrade bound*
//! to that nonce. That send, and every later one at or below the bound or with a requester-supplied
//! nonce, stays unknown; a later fetched-nonce send above the bound signs bytes no earlier run can
//! have sent, so it is a first submission again. The bound belongs to the section it was set in
//! and moves to the archive with it. It never applies to an uncertainty from a damaged or
//! truncated history. Either way the missing record is written at once. A hash whose record can't
//! be made durable is refused ([`SubmittedTxLog::try_record`] returns `Err`) and not kept: the
//! caller must not send. Hashes are public transaction ids, not key material.
//!
//! # Files
//!
//! [`RECORD_FILE`] holds `{"format": 3, "current": S, "previous_networks": [S, ...],
//! "history_lost": bool}` with `S = {genesis, entries, evicted, uncertain, upgrade_bound?}`. It is
//! a new file name, and it has no top-level `entries`, so a build that knows only
//! [`LEGACY_FILE`] neither opens it nor, if handed it, parses it (its reader then fails closed).
//! A `format` this build does not know is unreadable here, never misread.
//!
//! [`LEGACY_FILE`] was written by builds before the record was partitioned; they could only send
//! to r1005 ([`LEGACY_GENESIS`]), so its entries, nonce marks, uncertainty and upgrade bound are
//! merged into r1005's section. Only after that merge is durable is the legacy file set aside
//! (renamed with a `.migrated` suffix, never deleted). A legacy file found again later (an older
//! build ran in between) is merged again; the merge is idempotent. A record in a format this build
//! can't read is set aside the same way (`.unreadable`) before it is replaced.

use std::collections::{BTreeMap, VecDeque};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// How many recent submissions are kept per network.
pub const CAPACITY: usize = 256;

/// The record's file name in the app data dir.
pub const RECORD_FILE: &str = "submitted-tx-networks.json";

/// The file builds before the per-network record wrote. Read once to migrate, then set aside.
pub const LEGACY_FILE: &str = "submitted-tx.json";

/// The genesis of [`LEGACY_FILE`]'s sends: the r1005 testnet (`alpha-r1005` in
/// src-tauri/network/profiles.json), the only network those builds sent to.
pub const LEGACY_GENESIS: &str =
    "0x1dcfc4909dd9fdcb0f03c823d627e4b2032cc86bef5fc4561030d6c64b9775f1";

/// The [`RECORD_FILE`] format this build reads and writes.
const FORMAT: u32 = 3;

/// What a recorded transaction calls: its `to` and the first four bytes of its calldata. Lets
/// a caller find its own submission again (Join-8: the Activate step finds the member's
/// `MemberBond.activate` send after a restart).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallMeta {
    /// Lower-case 0x-hex address.
    pub to: String,
    /// Lower-case 0x-hex selector (4 bytes).
    pub selector: String,
}

/// A recorded submission found by [`SubmittedTxLog::latest_call`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedCall {
    pub hash: String,
    pub nonce: Option<u64>,
    /// When it was recorded (ms since the Unix epoch), if known.
    pub at_ms: Option<u64>,
}

/// Where a recorded transaction's nonce came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NonceSource {
    /// The app read it from the node (`eth_getTransactionCount` at `pending`) for this send.
    Fetched,
    /// The requester (a dApp) supplied it.
    Supplied,
}

/// One submitted transaction: its hash, and its sender and nonce when known. Its network is the
/// one of the [`Section`] that holds it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct Entry {
    hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sender: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    nonce: Option<u64>,
    /// What the tx calls, when the sender recorded it (Join-8).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    to: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    selector: Option<String>,
    /// When it was recorded (ms since the Unix epoch).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    at_ms: Option<u64>,
    /// Forgotten by the member ([`SubmittedTxLog::forget`]): no longer found by
    /// [`SubmittedTxLog::latest_call`], but still a recorded submission.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    forgotten: bool,
}

impl Entry {
    fn bare(hash: String) -> Self {
        Entry {
            hash,
            sender: None,
            nonce: None,
            to: None,
            selector: None,
            at_ms: None,
            forgotten: false,
        }
    }

    fn lowercased(self) -> Self {
        Entry {
            hash: self.hash.to_ascii_lowercase(),
            sender: self.sender.map(|s| s.to_ascii_lowercase()),
            to: self.to.map(|s| s.to_ascii_lowercase()),
            selector: self.selector.map(|s| s.to_ascii_lowercase()),
            ..self
        }
    }
}

fn lower_keys(m: BTreeMap<String, u64>) -> BTreeMap<String, u64> {
    let mut out = BTreeMap::new();
    for (k, v) in m {
        let hw = out.entry(k.to_ascii_lowercase()).or_insert(v);
        *hw = (*hw).max(v);
    }
    out
}

fn now_ms() -> Option<u64> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_millis() as u64)
}

/// The submissions on one network (one genesis).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Section {
    entries: VecDeque<Entry>,
    /// Per sender (lower case): the highest nonce among evicted entries.
    evicted: BTreeMap<String, u64>,
    /// The history can't prove any absent hash was never sent (see the module docs).
    uncertain: bool,
    /// An upgraded profile (#443): per sender (lower case), the nonce of its first send after the
    /// upgrade at an app-read nonce. `None` for a section that was never upgraded.
    upgrade: Option<BTreeMap<String, u64>>,
}

impl Section {
    fn damaged() -> Self {
        Section {
            uncertain: true,
            ..Section::default()
        }
    }

    fn contains(&self, hash: &str) -> bool {
        self.entries.iter().any(|e| e.hash == hash)
    }

    /// Whether an absent tx is provably a first submission on this section's network.
    fn absent_is_first(&self, origin: Option<(&str, u64)>, source: NonceSource) -> bool {
        if self.uncertain {
            return false;
        }
        if let Some(bound) = &self.upgrade {
            let above = match (origin, source) {
                (Some((sender, nonce)), NonceSource::Fetched) => {
                    bound.get(sender).is_some_and(|b| nonce > *b)
                }
                _ => false,
            };
            if !above {
                return false;
            }
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

    /// Merge `other`, a history of the same network, into this one so that neither loses an
    /// unknown: entries are united by hash (the newer ones of `other` appended), nonce marks take
    /// the higher mark, uncertainty is kept, and upgrade bounds keep the wider unknown window
    /// (per sender the higher bound; a sender bounded on one side only is unbounded again, so its
    /// next fetched-nonce send sets a bound that covers both). Merging the same history twice
    /// never makes anything more certain.
    fn absorb(&mut self, other: Section) {
        for e in other.entries {
            if !self.contains(&e.hash) {
                self.entries.push_back(e);
            }
        }
        for (sender, n) in other.evicted {
            let hw = self.evicted.entry(sender).or_insert(n);
            *hw = (*hw).max(n);
        }
        self.uncertain |= other.uncertain;
        self.upgrade = match (self.upgrade.take(), other.upgrade) {
            (None, b) => b,
            (a, None) => a,
            (Some(a), Some(b)) => Some(
                a.into_iter()
                    .filter_map(|(s, n)| b.get(&s).map(|m| (s, n.max(*m))))
                    .collect(),
            ),
        };
        self.evict_to_capacity();
    }

    /// A [`LEGACY_FILE`] body, in any format a build before the per-network record wrote. One
    /// that can't be parsed is a damaged history.
    fn from_legacy(bytes: &[u8]) -> Self {
        let mut s = if let Ok(d) = serde_json::from_slice::<LegacyDisk>(bytes) {
            let (uncertain, upgrade) = match d.upgrade_bound {
                // #507 wrote `uncertain: true` as the fail-closed marker of an upgrade-bounded
                // record; with the bound present, the bound is the truth.
                Some(bound) if d.uncertain => (false, Some(lower_keys(bound))),
                _ => (d.uncertain, None),
            };
            Section {
                entries: d.entries.into_iter().map(Entry::lowercased).collect(),
                evicted: lower_keys(d.evicted),
                uncertain,
                upgrade,
            }
        } else if let Ok(d) = serde_json::from_slice::<LegacyDiskV2>(bytes) {
            Section {
                entries: d
                    .hashes
                    .into_iter()
                    .map(|h| Entry::bare(h).lowercased())
                    .collect(),
                uncertain: d.uncertain,
                ..Section::default()
            }
        } else if let Ok(list) = serde_json::from_slice::<Vec<String>>(bytes) {
            // The bare-list format from before the flag: at capacity it may have evicted.
            Section {
                uncertain: list.len() >= CAPACITY,
                entries: list
                    .into_iter()
                    .map(|h| Entry::bare(h).lowercased())
                    .collect(),
                ..Section::default()
            }
        } else {
            Section::damaged()
        };
        s.evict_to_capacity();
        s
    }

    fn from_disk(d: SectionDisk) -> (String, Self) {
        let mut s = Section {
            entries: d.entries.into_iter().map(Entry::lowercased).collect(),
            evicted: lower_keys(d.evicted),
            uncertain: d.uncertain,
            upgrade: d.upgrade_bound.map(lower_keys),
        };
        s.evict_to_capacity();
        (d.genesis.to_ascii_lowercase(), s)
    }

    fn to_disk(&self, genesis: &str) -> SectionDisk {
        SectionDisk {
            genesis: genesis.to_string(),
            entries: self.entries.iter().cloned().collect(),
            evicted: self.evicted.clone(),
            uncertain: self.uncertain,
            upgrade_bound: self.upgrade.clone(),
        }
    }
}

#[derive(Debug, Clone)]
struct Inner {
    /// The genesis the record was opened on: the network every send is gated to.
    genesis: String,
    /// That network's section.
    current: Section,
    /// The other networks' sections, by genesis: read-only history (see the module docs).
    previous: BTreeMap<String, Section>,
    /// A damaged record lost its sections: a network it did not hold starts uncertain.
    history_lost: bool,
}

impl Inner {
    fn new(current: Section) -> Self {
        Inner {
            genesis: String::new(),
            current,
            previous: BTreeMap::new(),
            history_lost: false,
        }
    }

    /// The one read of the archive: these exact bytes were recorded on a previous network. It can
    /// only take certainty away from a send (the same chain id makes the bytes valid here too);
    /// it never decides anything about the current network by itself.
    fn sent_on_a_previous_network(&self, hash: &str) -> bool {
        self.previous.values().any(|s| s.contains(hash))
    }

    fn to_disk(&self) -> RecordDisk {
        RecordDisk {
            format: FORMAT,
            current: self.current.to_disk(&self.genesis),
            previous_networks: self.previous.iter().map(|(g, s)| s.to_disk(g)).collect(),
            history_lost: self.history_lost,
        }
    }
}

/// A section of [`RECORD_FILE`].
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SectionDisk {
    genesis: String,
    entries: Vec<Entry>,
    #[serde(default)]
    evicted: BTreeMap<String, u64>,
    uncertain: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    upgrade_bound: Option<BTreeMap<String, u64>>,
}

/// [`RECORD_FILE`].
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RecordDisk {
    format: u32,
    current: SectionDisk,
    #[serde(default)]
    previous_networks: Vec<SectionDisk>,
    #[serde(default)]
    history_lost: bool,
}

/// [`LEGACY_FILE`] as the last builds before the per-network record wrote it (#507, Join-8).
#[derive(serde::Deserialize)]
struct LegacyDisk {
    entries: Vec<Entry>,
    #[serde(default)]
    evicted: BTreeMap<String, u64>,
    uncertain: bool,
    #[serde(default)]
    upgrade_bound: Option<BTreeMap<String, u64>>,
}

/// [`LEGACY_FILE`] before sender/nonce were kept: `{hashes, uncertain}`.
#[derive(serde::Deserialize)]
struct LegacyDiskV2 {
    hashes: Vec<String>,
    uncertain: bool,
}

/// What [`RECORD_FILE`] held when the record was opened.
enum Found {
    Missing,
    /// Read and parsed: the genesis its current section was on, and its sections.
    Record(Box<Inner>),
    /// Present but not parsable (corrupt, truncated, or a format this build doesn't know).
    Unparsable,
    /// Present but could not be read.
    Unreadable,
}

fn read_record(path: &Path) -> Found {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Found::Missing,
        Err(_) => return Found::Unreadable,
    };
    let Ok(d) = serde_json::from_slice::<RecordDisk>(&bytes) else {
        return Found::Unparsable;
    };
    if d.format != FORMAT {
        return Found::Unparsable;
    }
    let (genesis, current) = Section::from_disk(d.current);
    let mut inner = Inner::new(current);
    inner.genesis = genesis;
    inner.history_lost = d.history_lost;
    for p in d.previous_networks {
        let (g, s) = Section::from_disk(p);
        if g == inner.genesis {
            inner.current.absorb(s);
        } else {
            inner.previous.entry(g).or_default().absorb(s);
        }
    }
    Found::Record(Box::new(inner))
}

/// Rename `path` to a free `<name>.<tag>[-N]` next to it, then sync the directory. Never deletes.
fn set_aside(path: &Path, tag: &str, sync_dir: SyncDir) -> std::io::Result<PathBuf> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let free = (1..=1000u32)
        .map(|n| match n {
            1 => dir.join(format!("{name}.{tag}")),
            _ => dir.join(format!("{name}.{tag}-{n}")),
        })
        .find(|p| !p.exists())
        .ok_or_else(|| std::io::Error::other("no free name to set the file aside"))?;
    std::fs::rename(path, &free)?;
    sync_dir(dir)?;
    Ok(free)
}

/// Makes a rename inside `dir` durable (see [`sync_dir`]). A parameter so tests can observe the
/// order of the write and make the sync fail.
type SyncDir = fn(&std::path::Path) -> std::io::Result<()>;

/// The submitted-transaction record. `path` = None keeps it in memory only (tests, and any
/// ceremony built without an app data dir or a network genesis).
#[derive(Debug)]
pub struct SubmittedTxLog {
    path: Option<PathBuf>,
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
            path: None,
            inner: Mutex::new(Inner::new(Section::default())),
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
            inner: Mutex::new(Inner::new(Section::damaged())),
            sync_dir,
            attempts: Default::default(),
        }
    }

    /// Load the record at `path` (the app data dir's [`RECORD_FILE`]) for the network whose
    /// block-0 hash is `genesis`, migrating a [`LEGACY_FILE`] next to it. A missing record is
    /// empty and certain (upgrade-bounded next to an existing wallet); an unreadable or corrupt
    /// one is uncertain. Other networks' sections are archived (see the module docs).
    pub fn open(path: PathBuf, genesis: &str) -> Self {
        Self::open_with(path, genesis, sync_dir)
    }

    /// [`Self::open`] with the directory sync given.
    fn open_with(path: PathBuf, genesis: &str, sync_dir: SyncDir) -> Self {
        let genesis = genesis.to_ascii_lowercase();
        let found = read_record(&path);
        let (mut sections, history_lost, opened_on) = match &found {
            Found::Record(inner) => {
                let mut s = inner.previous.clone();
                s.insert(inner.genesis.clone(), inner.current.clone());
                (s, inner.history_lost, Some(inner.genesis.clone()))
            }
            Found::Missing => (BTreeMap::new(), false, None),
            Found::Unparsable | Found::Unreadable => (BTreeMap::new(), true, None),
        };
        // A legacy file next to the record holds r1005's sends from a build before this record.
        let legacy_path = path
            .parent()
            .map(|d| d.join(LEGACY_FILE))
            .filter(|p| *p != path);
        let legacy = legacy_path.as_ref().and_then(|p| match std::fs::read(p) {
            Ok(bytes) => Some(Section::from_legacy(&bytes)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(_) => Some(Section::damaged()),
        });
        let migrating = legacy.is_some();
        if let Some(l) = legacy {
            sections
                .entry(LEGACY_GENESIS.to_string())
                .or_default()
                .absorb(l);
        }
        // A damaged record may have held more of every network's history than survives here.
        let damaged = matches!(found, Found::Unparsable | Found::Unreadable);
        if damaged {
            for s in sections.values_mut() {
                s.uncertain = true;
            }
        }
        // #347 Codex P1 / #443: builds before the record never wrote one. A profile that already
        // holds a wallet (the custody envelope in the same app data dir) and has no record at all
        // may have submitted txs with an unknown outcome: upgrade-bounded. Those builds sent only
        // to r1005, so r1005's section is bounded as well as the current one (they are the same
        // section on r1005). Only a profile with no wallet yet starts certain.
        let upgraded = matches!(found, Found::Missing)
            && !migrating
            && path
                .parent()
                .map(|d| d.join(crate::custody::ENVELOPE_FILE))
                .is_some_and(|p| p.exists());
        if upgraded {
            sections.insert(
                LEGACY_GENESIS.to_string(),
                Section {
                    upgrade: Some(BTreeMap::new()),
                    ..Section::default()
                },
            );
        }
        // A network this record holds no section for starts empty: certain, unless the record
        // was damaged once (`history_lost`) or the profile was just upgraded.
        let current = sections.remove(&genesis).unwrap_or_else(|| Section {
            uncertain: history_lost,
            upgrade: upgraded.then(BTreeMap::new),
            ..Section::default()
        });
        // A record this build can't parse is kept, never overwritten: set aside first.
        if matches!(found, Found::Unparsable) && set_aside(&path, "unreadable", sync_dir).is_err() {
            eprintln!(
                "[submitted-tx] could not set aside the unreadable {}",
                path.display()
            );
        }
        let switched = opened_on.is_some_and(|g| g != genesis);
        let mut inner = Inner {
            genesis,
            current,
            previous: sections,
            history_lost,
        };
        // An unreadable record (an I/O error, not bad bytes) is not written over at open; the next
        // record write replaces it, carrying `history_lost`.
        let write = matches!(found, Found::Missing | Found::Unparsable) || migrating || switched;
        if write {
            match write_atomic(&path, &inner.to_disk(), sync_dir) {
                Ok(()) => {
                    // The legacy entries are durable in the record now: only then set it aside.
                    if let Some(lp) = legacy_path.filter(|_| migrating) {
                        if let Err(e) = set_aside(&lp, "migrated", sync_dir) {
                            eprintln!(
                                "[submitted-tx] could not set aside {} (merged again next launch): {e}",
                                lp.display()
                            );
                        }
                    }
                }
                // A missing record that can't be written proves nothing: uncertain. The legacy
                // file stays where it is and is merged again next launch; a switch's archive is
                // written by the next record write (every write carries the whole record).
                Err(e) => {
                    if matches!(found, Found::Missing) {
                        inner.current.uncertain = true;
                    }
                    eprintln!("[submitted-tx] could not write {}: {e}", path.display());
                }
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
        self.record_entry(hash, None, NonceSource::Supplied, None)
    }

    /// [`Self::try_record`] for a transaction whose sender and nonce are known, so an eviction
    /// can be bounded by nonce instead of making the whole history uncertain. The nonce's origin
    /// is not known here, so it is treated as [`NonceSource::Supplied`].
    pub fn try_record_for(&self, hash: &str, sender: &str, nonce: u64) -> Result<bool, String> {
        self.record_entry(hash, Some((sender, nonce)), NonceSource::Supplied, None)
    }

    fn record_entry(
        &self,
        hash: &str,
        origin: Option<(&str, u64)>,
        source: NonceSource,
        call: Option<&CallMeta>,
    ) -> Result<bool, String> {
        let hash = hash.to_ascii_lowercase();
        let origin = origin.map(|(s, n)| (s.to_ascii_lowercase(), n));
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if g.current.contains(&hash) {
            // A re-send of recorded bytes. If they were forgotten (Forget, or taken out of recovery
            // after a refusal), they are in flight again: bring the entry back into recovery as the
            // latest send, durably, BEFORE the send. Otherwise a restart finds nothing and offers a
            // second activation while this one is in flight. No durable write, no send.
            if g.current
                .entries
                .iter()
                .any(|e| e.hash == hash && e.forgotten)
            {
                let mut next = g.clone();
                if let Some(pos) = next.current.entries.iter().position(|e| e.hash == hash) {
                    if let Some(mut e) = next.current.entries.remove(pos) {
                        e.forgotten = false;
                        if let Some(c) = call {
                            e.to = Some(c.to.to_ascii_lowercase());
                            e.selector = Some(c.selector.to_ascii_lowercase());
                        }
                        e.at_ms = now_ms().or(e.at_ms);
                        next.current.entries.push_back(e);
                    }
                }
                self.persist(&next)?;
                *g = next;
            }
            self.count_attempt(&hash);
            return Ok(false);
        }
        let first = g
            .current
            .absent_is_first(origin.as_ref().map(|(s, n)| (s.as_str(), *n)), source)
            && !g.sent_on_a_previous_network(&hash);
        let hash_key = hash.clone();
        let mut next = g.clone();
        // #443: a sender's first app-read-nonce send after an upgrade sets its bound (durably,
        // with this entry).
        if let (Some(bound), Some((sender, nonce)), NonceSource::Fetched) =
            (next.current.upgrade.as_mut(), origin.as_ref(), source)
        {
            bound.entry(sender.clone()).or_insert(*nonce);
        }
        next.current.entries.push_back(Entry {
            hash,
            sender: origin.as_ref().map(|(s, _)| s.clone()),
            nonce: origin.as_ref().map(|(_, n)| *n),
            to: call.map(|c| c.to.to_ascii_lowercase()),
            selector: call.map(|c| c.selector.to_ascii_lowercase()),
            // Kept only for a recorded call (what latest_call returns); other entries are unchanged.
            at_ms: call.and_then(|_| now_ms()),
            forgotten: false,
        });
        next.current.evict_to_capacity();
        self.persist(&next)?;
        *g = next;
        self.count_attempt(&hash_key);
        Ok(first)
    }

    /// Write `next` durably (no-op without a path). The caller swaps it in only on success.
    fn persist(&self, next: &Inner) -> Result<(), String> {
        if let Some(path) = &self.path {
            write_atomic(path, &next.to_disk(), self.sync_dir)
                .map_err(|e| format!("could not write {}: {e}", path.display()))?;
        }
        Ok(())
    }

    /// [`Self::try_record_for`] that also keeps what the tx calls ([`CallMeta`]) and when it was
    /// recorded, so [`Self::latest_call`] can find it after a restart.
    pub fn try_record_call(
        &self,
        hash: &str,
        sender: &str,
        nonce: u64,
        source: NonceSource,
        call: Option<&CallMeta>,
    ) -> Result<bool, String> {
        self.record_entry(hash, Some((sender, nonce)), source, call)
    }

    /// The most recent submission from `sender` calling `call` on the current network, unless it
    /// was forgotten ([`Self::forget`]). A send on a previous network is never returned.
    pub fn latest_call(&self, sender: &str, call: &CallMeta) -> Option<RecordedCall> {
        let (sender, to, selector) = (
            sender.to_ascii_lowercase(),
            call.to.to_ascii_lowercase(),
            call.selector.to_ascii_lowercase(),
        );
        let g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        // Only the LATEST matching send counts: a forgotten newer send never brings back an older one.
        let e = g.current.entries.iter().rev().find(|e| {
            e.sender.as_deref() == Some(sender.as_str())
                && e.to.as_deref() == Some(to.as_str())
                && e.selector.as_deref() == Some(selector.as_str())
        })?;
        (!e.forgotten).then(|| RecordedCall {
            hash: e.hash.clone(),
            nonce: e.nonce,
            at_ms: e.at_ms,
        })
    }

    /// Mark a submission on the current network as forgotten, durably: [`Self::latest_call`] no
    /// longer returns it. The hash stays recorded, so the same bytes are never a first submission
    /// again. `Ok(true)` when it was found and marked; a send on a previous network is not found.
    pub fn forget(&self, hash: &str) -> Result<bool, String> {
        let hash = hash.to_ascii_lowercase();
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let mut next = g.clone();
        let Some(e) = next.current.entries.iter_mut().find(|e| e.hash == hash) else {
            return Ok(false);
        };
        e.forgotten = true;
        self.persist(&next)?;
        *g = next;
        Ok(true)
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

    /// How many hashes the record holds for the current network.
    pub fn len(&self) -> usize {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .current
            .entries
            .len()
    }

    /// True when the record holds no hashes for the current network.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// True when the current network's history can't prove every absent hash was never sent:
    /// every send is then a re-submission, except, on an upgrade-bounded section, a fetched-nonce
    /// send above its sender's bound.
    pub fn is_uncertain(&self) -> bool {
        let g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        g.current.uncertain || g.current.upgrade.is_some()
    }
}

/// Write `disk` as JSON to `path` via a temp file in the same directory, fsync, rename, then fsync
/// the directory so the rename itself survives a power loss. Any failure is an error: the caller
/// must not broadcast on a record that isn't durable.
fn write_atomic(
    path: &std::path::Path,
    disk: &RecordDisk,
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
        .unwrap_or_else(|| RECORD_FILE.to_string());
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
