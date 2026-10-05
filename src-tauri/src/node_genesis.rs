//! Node data reset on a genesis change (40204 reroll, 2026-10-05).
//!
//! A member upgrading to a build whose address book names a new genesis still
//! has the OLD chain in its node data dir. A binary carrying the new genesis
//! pins must never open that database (fed#289: "any binary with the genesis
//! pins must never touch the old genesis"). So before every node start, core
//! compares the genesis its node data belongs to against the book's
//! `genesisHash` (`crate::addresses::genesis_hash`) and, when they differ,
//! deletes the chain database and lets the node resync from the start.
//!
//! ## What the node data dir holds
//! `<app_data_dir>/node/` (see `node::build_node_state`) is the node's
//! `--data-dir`. The chain node opens its RocksDB directly in that directory,
//! so the database files sit next to the key material:
//!
//! | entry | written by | on a genesis change |
//! |---|---|---|
//! | `CURRENT`, `IDENTITY`, `LOCK`, `LOG`, `LOG.old.*`, `MANIFEST-*`, `OPTIONS-*`, `*.sst`, `*.log`, `*.blob`, `*.dbtmp` | RocksDB (blocks, DAG, state) | deleted |
//! | `proposer.key` | node (`--mine` path; validator registration signs with it) | kept |
//! | `noise.key` (and any `noise.key.*` copy) | node (P2P Noise static identity) | kept |
//! | `peer.id` | node (legacy peer id) | kept |
//! | `encryption.meta` | node (at-rest key commitment for the keyring storage key) | kept |
//! | `node.toml` | core (`ensure_node_config`) | kept |
//! | `crash-records.jsonl` | core (supervisor) | kept |
//! | [`GENESIS_MARKER_FILE`] | core (this module) | rewritten last |
//!
//! Deletion is by positive match on RocksDB's own file names
//! ([`is_chain_db_entry`]): anything not recognised as a database file is
//! kept, so key material the node adds later survives by default.
//!
//! ## Ordering (crash safety)
//! 1. Validate the book genesis; a zero or unparsable hash refuses the start.
//! 2. Marker equal to the book genesis: nothing to do.
//! 3. Otherwise delete every chain DB entry (`CURRENT` first, so a partly
//!    deleted directory never looks like a complete database), then write the
//!    marker atomically (temp file, fsync, rename).
//!
//! A crash anywhere before step 3's rename leaves the old marker or none, so
//! the next start runs the reset again; it is idempotent.

use std::path::{Path, PathBuf};

/// The marker file naming the genesis this node data dir belongs to.
pub const GENESIS_MARKER_FILE: &str = "chain-genesis";

/// The one-line notice shown to the member after a reset.
pub const CHAIN_RESET_NOTICE: &str =
    "Citrate Network was upgraded to a new chain; your node is resyncing from the start";

/// Why the node may not start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GenesisGateError {
    /// The address book genesis is not `0x` + 64 hex, or is the zero hash.
    BadBookGenesis(String),
    /// A node still answers on the local RPC, so its database may be open.
    NodeStillRunning,
    /// A filesystem step failed; the start is refused and retried next time.
    Io(String),
}

impl std::fmt::Display for GenesisGateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GenesisGateError::BadBookGenesis(g) => write!(
                f,
                "the node was not started: this build's address book has no valid genesis \
                 hash ({g:?}), so core cannot tell which chain the node data belongs to"
            ),
            GenesisGateError::NodeStillRunning => write!(
                f,
                "the node was not started: the chain was upgraded and the old chain data must \
                 be removed, but a Citrate node is still answering on the local RPC; quit it \
                 and start again"
            ),
            GenesisGateError::Io(m) => write!(
                f,
                "the node was not started: preparing the node data for the new chain failed: {m}"
            ),
        }
    }
}

impl std::error::Error for GenesisGateError {}

/// What [`reconcile_genesis`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GenesisOutcome {
    /// The marker already names the book genesis; the data is kept as is.
    Kept,
    /// No chain data and no marker for another genesis: a fresh data dir, marked.
    FreshMarked,
    /// The data belonged to another genesis (or to none recorded): the chain
    /// DB was deleted and the marker rewritten.
    Reset {
        /// The genesis the old marker named, if there was one.
        previous: Option<String>,
        /// The entries deleted, by file name, sorted.
        removed: Vec<String>,
        /// Their total size in bytes.
        bytes: u64,
    },
}

/// Parse and normalise a book genesis: `0x` + 64 hex digits, not all zero.
/// Returns the lowercase form.
pub fn validate_book_genesis(g: &str) -> Result<String, GenesisGateError> {
    let bad = || GenesisGateError::BadBookGenesis(g.to_string());
    let hex = g.strip_prefix("0x").ok_or_else(bad)?;
    if hex.len() != 64 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(bad());
    }
    if hex.bytes().all(|b| b == b'0') {
        return Err(bad());
    }
    Ok(format!("0x{}", hex.to_ascii_lowercase()))
}

fn all_digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

/// True for a file name RocksDB writes into its database directory. This is
/// the ONLY set the reset deletes; every other entry is kept.
pub fn is_chain_db_entry(name: &str) -> bool {
    if matches!(name, "CURRENT" | "IDENTITY" | "LOCK" | "LOG") {
        return true;
    }
    if let Some(rest) = name.strip_prefix("LOG.old.") {
        return all_digits(rest);
    }
    if let Some(rest) = name.strip_prefix("MANIFEST-") {
        return all_digits(rest);
    }
    if let Some(rest) = name.strip_prefix("OPTIONS-") {
        let rest = rest.strip_suffix(".dbtmp").unwrap_or(rest);
        return all_digits(rest);
    }
    if let Some((stem, ext)) = name.rsplit_once('.') {
        if matches!(ext, "sst" | "log" | "blob" | "dbtmp") {
            return all_digits(stem);
        }
    }
    false
}

/// The chain DB entries currently in `data_dir`, sorted, `CURRENT` first.
fn chain_db_entries(data_dir: &Path) -> Result<Vec<String>, GenesisGateError> {
    let rd = match std::fs::read_dir(data_dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => {
            return Err(GenesisGateError::Io(format!(
                "read {}: {e}",
                data_dir.display()
            )))
        }
    };
    let mut names = Vec::new();
    for entry in rd {
        let entry = entry.map_err(|e| GenesisGateError::Io(e.to_string()))?;
        let name = entry.file_name().to_string_lossy().to_string();
        let is_file = entry.file_type().map(|t| t.is_file()).unwrap_or(false);
        if is_file && is_chain_db_entry(&name) {
            names.push(name);
        }
    }
    names.sort_by_key(|n| (n != "CURRENT", n.clone()));
    Ok(names)
}

/// Delete the chain DB entries in `data_dir` and return their names. Exposed
/// to the tests so they can stop between the delete and the marker write.
pub(crate) fn wipe_chain_db(data_dir: &Path) -> Result<(Vec<String>, u64), GenesisGateError> {
    let names = chain_db_entries(data_dir)?;
    let mut bytes = 0u64;
    for name in &names {
        let path = data_dir.join(name);
        bytes = bytes.saturating_add(std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0));
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(GenesisGateError::Io(format!("remove {name}: {e}"))),
        }
    }
    Ok((names, bytes))
}

fn marker_path(data_dir: &Path) -> PathBuf {
    data_dir.join(GENESIS_MARKER_FILE)
}

/// The genesis the marker names, lowercase and trimmed; `None` when absent.
pub fn read_marker(data_dir: &Path) -> Result<Option<String>, GenesisGateError> {
    match std::fs::read_to_string(marker_path(data_dir)) {
        Ok(s) => Ok(Some(s.trim().to_ascii_lowercase())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(GenesisGateError::Io(format!("read marker: {e}"))),
    }
}

/// Write the marker atomically: temp file, fsync, rename.
fn write_marker(data_dir: &Path, genesis: &str) -> Result<(), GenesisGateError> {
    use std::io::Write;
    let io = |e: std::io::Error| GenesisGateError::Io(format!("write marker: {e}"));
    std::fs::create_dir_all(data_dir).map_err(io)?;
    let tmp = data_dir.join(format!("{GENESIS_MARKER_FILE}.tmp"));
    {
        let mut f = std::fs::File::create(&tmp).map_err(io)?;
        f.write_all(format!("{genesis}\n").as_bytes()).map_err(io)?;
        f.sync_all().map_err(io)?;
    }
    std::fs::rename(&tmp, marker_path(data_dir)).map_err(io)?;
    Ok(())
}

/// Make `data_dir` safe to start a node for `book_genesis`.
///
/// `node_running` is asked only when a reset is needed; when it reports a live
/// node the reset is refused rather than deleting a database in use.
pub fn reconcile_genesis(
    data_dir: &Path,
    book_genesis: &str,
    node_running: impl FnOnce() -> bool,
) -> Result<GenesisOutcome, GenesisGateError> {
    let book = validate_book_genesis(book_genesis)?;
    let marker = read_marker(data_dir)?;
    if marker.as_deref() == Some(book.as_str()) {
        return Ok(GenesisOutcome::Kept);
    }
    let has_chain_data = !chain_db_entries(data_dir)?.is_empty();
    if !has_chain_data && marker.is_none() {
        write_marker(data_dir, &book)?;
        return Ok(GenesisOutcome::FreshMarked);
    }
    if has_chain_data && node_running() {
        return Err(GenesisGateError::NodeStillRunning);
    }
    let (removed, bytes) = wipe_chain_db(data_dir)?;
    write_marker(data_dir, &book)?;
    Ok(GenesisOutcome::Reset {
        previous: marker,
        removed,
        bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const OLD: &str = "0x0f2b567f00000000000000000000000000000000000000000000000000000001";
    const NEW: &str = "0xabcdef0000000000000000000000000000000000000000000000000000000002";

    /// Key material and other files the reset must never delete.
    const KEPT: &[&str] = &[
        "proposer.key",
        "noise.key",
        "noise.key.fresh-20260930T073048Z",
        "peer.id",
        "encryption.meta",
        "node.toml",
        "crash-records.jsonl",
        "unknown-future-key.bin",
    ];
    /// A realistic RocksDB file set (names from a member node dir).
    const DB: &[&str] = &[
        "CURRENT",
        "IDENTITY",
        "LOCK",
        "LOG",
        "LOG.old.1790753303713942",
        "MANIFEST-000624",
        "OPTIONS-000622",
        "OPTIONS-000626.dbtmp",
        "000013.sst",
        "000623.log",
        "000700.blob",
        "000701.dbtmp",
    ];

    fn tmp(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let p = std::env::temp_dir().join(format!(
            "citrate-core-genesis-{tag}-{nanos}-{:?}",
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn populate(dir: &Path) {
        for f in KEPT.iter().chain(DB.iter()) {
            std::fs::write(dir.join(f), f.as_bytes()).unwrap();
        }
    }

    fn assert_keys_kept(dir: &Path) {
        for f in KEPT {
            assert_eq!(
                std::fs::read(dir.join(f)).unwrap(),
                f.as_bytes(),
                "{f} must survive the reset unchanged"
            );
        }
    }

    fn assert_db_gone(dir: &Path) {
        for f in DB {
            assert!(!dir.join(f).exists(), "{f} must be deleted");
        }
    }

    fn never_running() -> bool {
        false
    }

    #[test]
    fn marker_match_keeps_the_chain_data() {
        let d = tmp("match");
        populate(&d);
        write_marker(&d, NEW).unwrap();
        let out = reconcile_genesis(&d, NEW, || panic!("no reset, no probe")).unwrap();
        assert_eq!(out, GenesisOutcome::Kept);
        for f in DB {
            assert!(d.join(f).exists(), "{f} must be kept on a marker match");
        }
        assert_keys_kept(&d);
    }

    #[test]
    fn marker_match_is_case_insensitive_against_the_book() {
        let d = tmp("case");
        populate(&d);
        write_marker(&d, &NEW.to_ascii_uppercase().replacen("0X", "0x", 1)).unwrap();
        assert_eq!(
            reconcile_genesis(&d, NEW, never_running).unwrap(),
            GenesisOutcome::Kept
        );
    }

    #[test]
    fn marker_mismatch_wipes_the_chain_db_and_keeps_keys() {
        let d = tmp("mismatch");
        populate(&d);
        write_marker(&d, OLD).unwrap();
        let out = reconcile_genesis(&d, NEW, never_running).unwrap();
        let GenesisOutcome::Reset {
            previous, removed, ..
        } = out
        else {
            panic!("expected a reset, got {out:?}");
        };
        assert_eq!(previous.as_deref(), Some(OLD));
        assert_eq!(removed.len(), DB.len());
        assert_eq!(removed[0], "CURRENT", "CURRENT goes first");
        assert_db_gone(&d);
        assert_keys_kept(&d);
        assert_eq!(read_marker(&d).unwrap().as_deref(), Some(NEW));
    }

    #[test]
    fn missing_marker_with_chain_data_wipes() {
        let d = tmp("nomarker");
        populate(&d);
        let out = reconcile_genesis(&d, NEW, never_running).unwrap();
        assert!(
            matches!(out, GenesisOutcome::Reset { previous: None, .. }),
            "{out:?}"
        );
        assert_db_gone(&d);
        assert_keys_kept(&d);
        assert_eq!(read_marker(&d).unwrap().as_deref(), Some(NEW));
    }

    #[test]
    fn fresh_install_writes_the_marker() {
        let d = tmp("fresh").join("node"); // not created yet
        let out = reconcile_genesis(&d, NEW, || panic!("no reset, no probe")).unwrap();
        assert_eq!(out, GenesisOutcome::FreshMarked);
        assert_eq!(read_marker(&d).unwrap().as_deref(), Some(NEW));
        // Second start: kept.
        assert_eq!(
            reconcile_genesis(&d, NEW, never_running).unwrap(),
            GenesisOutcome::Kept
        );
    }

    #[test]
    fn keys_only_dir_without_marker_is_fresh_and_keeps_keys() {
        let d = tmp("keysonly");
        for f in KEPT {
            std::fs::write(d.join(f), f.as_bytes()).unwrap();
        }
        assert_eq!(
            reconcile_genesis(&d, NEW, never_running).unwrap(),
            GenesisOutcome::FreshMarked
        );
        assert_keys_kept(&d);
    }

    #[test]
    fn crash_between_wipe_and_marker_reruns_safely() {
        // Crash after a partial delete: some DB files remain, old marker in place.
        let d = tmp("crash-partial");
        populate(&d);
        write_marker(&d, OLD).unwrap();
        std::fs::remove_file(d.join("CURRENT")).unwrap();
        std::fs::remove_file(d.join("000013.sst")).unwrap();
        let out = reconcile_genesis(&d, NEW, never_running).unwrap();
        assert!(matches!(out, GenesisOutcome::Reset { .. }), "{out:?}");
        assert_db_gone(&d);
        assert_keys_kept(&d);
        assert_eq!(read_marker(&d).unwrap().as_deref(), Some(NEW));

        // Crash after the full delete, before the marker write.
        let d = tmp("crash-full");
        populate(&d);
        write_marker(&d, OLD).unwrap();
        wipe_chain_db(&d).unwrap();
        assert_eq!(
            read_marker(&d).unwrap().as_deref(),
            Some(OLD),
            "marker not yet rewritten"
        );
        let out = reconcile_genesis(&d, NEW, never_running).unwrap();
        assert_eq!(
            out,
            GenesisOutcome::Reset {
                previous: Some(OLD.to_string()),
                removed: vec![],
                bytes: 0,
            },
            "a rerun still reports the reset so the member sees the notice"
        );
        assert_keys_kept(&d);
        assert_eq!(read_marker(&d).unwrap().as_deref(), Some(NEW));
        // And a third start keeps everything.
        assert_eq!(
            reconcile_genesis(&d, NEW, never_running).unwrap(),
            GenesisOutcome::Kept
        );
    }

    #[test]
    fn stale_marker_temp_file_does_not_count_as_a_marker() {
        let d = tmp("tmpmarker");
        populate(&d);
        std::fs::write(d.join(format!("{GENESIS_MARKER_FILE}.tmp")), NEW).unwrap();
        let out = reconcile_genesis(&d, NEW, never_running).unwrap();
        assert!(matches!(out, GenesisOutcome::Reset { .. }), "{out:?}");
        assert_db_gone(&d);
    }

    #[test]
    fn bad_book_genesis_refuses_to_start_and_touches_nothing() {
        for bad in [
            "",
            "0x",
            "0x0000000000000000000000000000000000000000000000000000000000000000",
            "0f2b567f00000000000000000000000000000000000000000000000000000001",
            "0x0f2b567f",
            "0xzz2b567f00000000000000000000000000000000000000000000000000000001",
            "0x0f2b567f000000000000000000000000000000000000000000000000000000011",
        ] {
            let d = tmp("bad");
            populate(&d);
            write_marker(&d, OLD).unwrap();
            let err = reconcile_genesis(&d, bad, || panic!("never probe")).unwrap_err();
            assert_eq!(err, GenesisGateError::BadBookGenesis(bad.to_string()));
            assert!(err.to_string().contains("not started"));
            for f in DB {
                assert!(d.join(f).exists(), "{f} untouched when the book is bad");
            }
            assert_keys_kept(&d);
            assert_eq!(read_marker(&d).unwrap().as_deref(), Some(OLD));
        }
    }

    #[test]
    fn a_live_node_blocks_the_reset() {
        let d = tmp("live");
        populate(&d);
        write_marker(&d, OLD).unwrap();
        let err = reconcile_genesis(&d, NEW, || true).unwrap_err();
        assert_eq!(err, GenesisGateError::NodeStillRunning);
        for f in DB {
            assert!(d.join(f).exists(), "{f} untouched while a node runs");
        }
        assert_eq!(read_marker(&d).unwrap().as_deref(), Some(OLD));
    }

    #[test]
    fn only_rocksdb_names_are_chain_db_entries() {
        for f in DB {
            assert!(is_chain_db_entry(f), "{f}");
        }
        for f in KEPT {
            assert!(!is_chain_db_entry(f), "{f}");
        }
        for f in [
            GENESIS_MARKER_FILE,
            "chain-genesis.tmp",
            "encryption.meta.tmp",
            "LOG.old.",
            "MANIFEST-",
            "abc.sst",
            "x.log",
            "proposer.key.log",
            "CURRENT.bak",
        ] {
            assert!(!is_chain_db_entry(f), "{f}");
        }
    }

    #[test]
    fn subdirectories_are_never_deleted() {
        let d = tmp("subdir");
        populate(&d);
        std::fs::create_dir_all(d.join("000999.sst")).unwrap(); // a dir with a DB-like name
        std::fs::write(d.join("000999.sst").join("keep"), b"k").unwrap();
        reconcile_genesis(&d, NEW, never_running).unwrap();
        assert!(d.join("000999.sst").join("keep").exists());
    }
}
