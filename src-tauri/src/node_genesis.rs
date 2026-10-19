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
//! | `node.toml` | core (`ensure_node_config`) | set aside as `node.toml.<old genesis>` (K6) |
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
//!    deleted directory never looks like a complete database), then set the
//!    previous network's `node.toml` aside ([`retire_node_config`]; K6), make
//!    both durable (directory fsync), and only then write the marker
//!    atomically (temp file, fsync, rename, directory fsync).
//!
//! The marker is the commit point of the whole migration, database AND node
//! config. A crash or error anywhere before its rename leaves the old marker or
//! none, so the next start runs every step again; each is idempotent (a
//! `node.toml` already set aside is simply absent). Once the marker names the
//! new genesis, the old `node.toml` is already gone, so `ensure_node_config`
//! can only write a fresh one from the active profile: the previous network's
//! bootnodes and chain id never survive a completed migration.
//!
//! ## Who may hold the database (SCL-S8.5a, red-team RT-11)
//! Before deleting anything the reset takes the chain database lock (RocksDB's
//! `LOCK` file, the way RocksDB takes it; [`crate::node_holder`]) and holds it
//! until the marker is written. A process holding that lock refuses the reset
//! with [`GenesisGateError::ChainDbHeld`], naming the holder, even when it does
//! not answer on the local RPC (a starting or wedged orphan). The RPC check is
//! kept as a second signal. The app's startup cleanup runs before any node
//! start is admitted ([`crate::startup_barrier`]), so the lock is normally free.
//!
//! **Residual crash window.** `LOCK` is itself a RocksDB file and is deleted
//! (the set of deleted files is unchanged); it is deleted last. From that
//! unlink until the lock handle closes after the marker write, a process that
//! starts a node in that instant can create a new `LOCK` and open the
//! database; on macOS and Linux the lock still held is on the unlinked file.
//! Core's own node is not started until this function returns. Only a process
//! outside this app's control (another copy of the app, a hand-started node)
//! can use the window, and the next start's holder check names it. A crash of
//! core during the reset releases the lock with the process and leaves the
//! old marker, so the next start repeats the reset under the lock.

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
    /// Another process holds the chain database lock (SCL-S8.5a), whether or not it answers
    /// on the local RPC. Nothing was deleted.
    ChainDbHeld(crate::node_holder::NodeBlocker),
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
            GenesisGateError::ChainDbHeld(b) => write!(f, "{b}"),
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
    /// DB was deleted, the previous `node.toml` set aside and the marker rewritten.
    Reset {
        /// The genesis the old marker named, if there was one.
        previous: Option<String>,
        /// The entries deleted, by file name, sorted.
        removed: Vec<String>,
        /// Their total size in bytes.
        bytes: u64,
        /// Where the previous network's `node.toml` was set aside; `None` when there was
        /// none (or an interrupted earlier attempt already set it aside).
        retired_config: Option<PathBuf>,
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

/// The chain DB entries currently in `data_dir`, sorted, `CURRENT` first and `LOCK` last (the
/// reset holds the database lock while it deletes, so the lock file goes last).
pub(crate) fn chain_db_entries(data_dir: &Path) -> Result<Vec<String>, GenesisGateError> {
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
    names.sort_by_key(|n| {
        (
            n != "CURRENT",
            n == crate::node_holder::CHAIN_DB_LOCK_FILE,
            n.clone(),
        )
    });
    Ok(names)
}

/// The entries a reset left in `data_dir` (everything that is not chain DB:
/// key material, `node.toml`, the marker, ...), sorted. For the reset log line.
pub fn kept_entries(data_dir: &Path) -> Vec<String> {
    let Ok(rd) = std::fs::read_dir(data_dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = rd
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| !is_chain_db_entry(n))
        .collect();
    names.sort();
    names
}

/// The support-log line for a genesis reset (v0.5.0 C3 finding 2): the removed
/// chain DB files under `removed:` and what is still in the data dir under `kept:`.
pub fn reset_log_message(
    previous: Option<&str>,
    book_genesis: &str,
    removed: &[String],
    bytes: u64,
    data_dir: &Path,
    kept: &[String],
) -> String {
    let list = |v: &[String]| {
        if v.is_empty() {
            "none".to_string()
        } else {
            v.join(", ")
        }
    };
    format!(
        "[node] chain genesis changed ({} -> {}): removed {} chain database file(s), {} bytes, \
         from {}; removed: {}; kept: {}",
        previous.unwrap_or("unrecorded"),
        book_genesis,
        removed.len(),
        bytes,
        data_dir.display(),
        list(removed),
        list(kept)
    )
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

/// K6: on a genesis change, set the previous network's `node.toml` aside as
/// `node.toml.<first 10 characters of its genesis>` (or `node.toml.unrecorded`), so the next
/// spawn writes a fresh one from the active profile. A member's hand edits are in that file.
/// Returns where it went; `None` when there was no `node.toml`. An existing file of that name is
/// never overwritten (a numeric suffix is added).
pub(crate) fn retire_node_config(
    data_dir: &Path,
    previous_genesis: Option<&str>,
) -> std::io::Result<Option<PathBuf>> {
    #[cfg(test)]
    if test_hooks::FAIL_RETIRE.with(|f| f.get()) {
        return Err(std::io::Error::other("injected retire failure (test)"));
    }
    let path = data_dir.join("node.toml");
    if std::fs::symlink_metadata(&path).is_err() {
        return Ok(None);
    }
    let tag: String = previous_genesis
        .map(|g| g.chars().take(10).collect())
        .unwrap_or_else(|| "unrecorded".to_string());
    let tag: String = tag.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    let mut dest = data_dir.join(format!("node.toml.{tag}"));
    let mut n = 1;
    while std::fs::symlink_metadata(&dest).is_ok() {
        dest = data_dir.join(format!("node.toml.{tag}.{n}"));
        n += 1;
    }
    std::fs::rename(&path, &dest)?;
    Ok(Some(dest))
}

/// Make the directory entries of `dir` (deletions, renames) durable before a later step
/// depends on them. A no-op where directories cannot be opened for sync (Windows).
fn sync_dir(dir: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        std::fs::File::open(dir)?.sync_all()?;
    }
    #[cfg(not(unix))]
    {
        let _ = dir;
    }
    Ok(())
}

/// Test-only fault injection for the migration steps (see the crash-safety tests).
#[cfg(test)]
pub(crate) mod test_hooks {
    use std::cell::Cell;
    thread_local! {
        /// `retire_node_config` fails as if the rename had failed.
        pub static FAIL_RETIRE: Cell<bool> = const { Cell::new(false) };
    }
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
    sync_dir(data_dir).map_err(io)?;
    Ok(())
}

/// Make `data_dir` safe to start a node for `book_genesis`.
///
/// `node_running` is asked only when a reset is needed; when it reports a live
/// node the reset is refused rather than deleting a database in use. The reset
/// also requires the chain database lock (SCL-S8.5a): a process holding it
/// refuses the reset even when `node_running` reports nothing.
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
    // SCL-S8.5a: the database lock must be ours before anything is deleted, whether or not a
    // holder answers on the RPC. Held until the marker is written (see the module doc).
    let lock = match crate::node_holder::acquire_chain_db_lock(data_dir) {
        Ok(lock) => lock,
        Err(crate::node_holder::DbLockError::Held(holder)) => {
            return Err(GenesisGateError::ChainDbHeld(
                crate::node_holder::NodeBlocker::chain_database(holder, data_dir),
            ))
        }
        Err(crate::node_holder::DbLockError::Io(e)) => return Err(GenesisGateError::Io(e)),
    };
    if has_chain_data && node_running() {
        return Err(GenesisGateError::NodeStillRunning);
    }
    let (removed, bytes) = wipe_chain_db(data_dir)?;
    // K6: the previous network's node.toml goes before the marker commits the switch, so a
    // crash or a failure here leaves the old marker and the next start repeats the migration.
    let retired_config = retire_node_config(data_dir, marker.as_deref())
        .map_err(|e| GenesisGateError::Io(format!("set the previous node.toml aside: {e}")))?;
    sync_dir(data_dir).map_err(|e| GenesisGateError::Io(format!("sync the node data dir: {e}")))?;
    write_marker(data_dir, &book)?;
    drop(lock);
    Ok(GenesisOutcome::Reset {
        previous: marker,
        removed,
        bytes,
        retired_config,
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

    /// The previous network's node config (K6): set aside, never deleted, on a reset.
    const OLD_CONFIG: &str = "# old network bootnodes, hand-edited\n";
    /// Where a reset from [`OLD`] sets it aside.
    const OLD_CONFIG_ASIDE: &str = "node.toml.0x0f2b567f";

    fn populate(dir: &Path) {
        for f in KEPT.iter().chain(DB.iter()) {
            std::fs::write(dir.join(f), f.as_bytes()).unwrap();
        }
        std::fs::write(dir.join("node.toml"), OLD_CONFIG).unwrap();
    }

    /// The K6 invariant: once the marker names the new genesis, no `node.toml` of the old
    /// network is left for `ensure_node_config` to keep, and the old one is preserved aside.
    fn assert_config_set_aside(dir: &Path, aside: &str) {
        assert!(
            !dir.join("node.toml").exists(),
            "the old node.toml must not survive a completed migration"
        );
        assert_eq!(
            std::fs::read_to_string(dir.join(aside)).unwrap(),
            OLD_CONFIG,
            "the old node.toml is kept aside as {aside}"
        );
    }

    /// C3 finding 2: the reset log line names removed files as removed and the
    /// files still in the data dir as kept, with single spaces throughout.
    #[test]
    fn the_reset_log_line_lists_removed_and_kept_files_correctly() {
        let d = tmp("logline");
        populate(&d);
        let out = reconcile_genesis(&d, NEW, || false).unwrap();
        let GenesisOutcome::Reset {
            previous,
            removed,
            bytes,
            ..
        } = out
        else {
            panic!("expected a reset: {out:?}");
        };
        let kept = kept_entries(&d);
        let msg = reset_log_message(previous.as_deref(), NEW, &removed, bytes, &d, &kept);
        assert!(!msg.contains("  "), "no run of spaces: {msg:?}");
        assert!(!msg.contains("key material kept"), "{msg}");
        let (head, kept_part) = msg.split_once("; kept: ").expect("a kept section");
        let removed_part = head.split_once("; removed: ").expect("a removed section").1;
        for f in DB {
            assert!(
                removed_part.split(", ").any(|x| x == *f),
                "{f} listed as removed: {msg}"
            );
            assert!(
                !kept_part.split(", ").any(|x| x == *f),
                "{f} not listed as kept: {msg}"
            );
        }
        for f in KEPT.iter().chain([&GENESIS_MARKER_FILE]) {
            assert!(
                kept_part.split(", ").any(|x| x == *f),
                "{f} listed as kept: {msg}"
            );
            assert!(
                !removed_part.split(", ").any(|x| x == *f),
                "{f} not listed as removed: {msg}"
            );
        }
        assert!(msg.starts_with(&format!(
            "[node] chain genesis changed (unrecorded -> {NEW}): removed {} chain database file(s), {bytes} bytes, from {};",
            DB.len(),
            d.display()
        )), "{msg}");
        let _ = std::fs::remove_dir_all(&d);
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

        assert_config_set_aside(&d, OLD_CONFIG_ASIDE);

        // Crash after the full delete, before the node.toml is set aside.
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
                retired_config: Some(d.join(OLD_CONFIG_ASIDE)),
            },
            "a rerun still reports the reset so the member sees the notice"
        );
        assert_keys_kept(&d);
        assert_config_set_aside(&d, OLD_CONFIG_ASIDE);
        assert_eq!(read_marker(&d).unwrap().as_deref(), Some(NEW));
        // And a third start keeps everything.
        assert_eq!(
            reconcile_genesis(&d, NEW, never_running).unwrap(),
            GenesisOutcome::Kept
        );
    }

    /// K6 (Codex P2 on core#363): setting node.toml aside is part of the migration the
    /// marker commits. A crash after it but before the marker write leaves the old marker; the
    /// next start completes the switch without setting anything aside twice.
    #[test]
    fn crash_after_the_config_is_set_aside_but_before_the_marker_completes_next_start() {
        let d = tmp("crash-after-retire");
        populate(&d);
        write_marker(&d, OLD).unwrap();
        wipe_chain_db(&d).unwrap();
        retire_node_config(&d, Some(OLD)).unwrap();
        assert_eq!(read_marker(&d).unwrap().as_deref(), Some(OLD));
        let out = reconcile_genesis(&d, NEW, never_running).unwrap();
        assert_eq!(
            out,
            GenesisOutcome::Reset {
                previous: Some(OLD.to_string()),
                removed: vec![],
                bytes: 0,
                retired_config: None,
            }
        );
        assert_config_set_aside(&d, OLD_CONFIG_ASIDE);
        assert!(
            !d.join(format!("{OLD_CONFIG_ASIDE}.1")).exists(),
            "nothing set aside twice"
        );
        assert_eq!(read_marker(&d).unwrap().as_deref(), Some(NEW));
        assert_eq!(
            reconcile_genesis(&d, NEW, never_running).unwrap(),
            GenesisOutcome::Kept
        );
    }

    /// K6 (Codex P2 on core#363): if setting node.toml aside fails, the start is refused with
    /// the OLD marker still in place (the old code had already committed the new one, so the
    /// next start returned Kept and the old network's config survived). The next start retries
    /// and completes the migration.
    #[test]
    fn a_failed_config_set_aside_does_not_commit_the_marker_and_is_retried() {
        let d = tmp("retire-fails");
        populate(&d);
        write_marker(&d, OLD).unwrap();
        test_hooks::FAIL_RETIRE.with(|f| f.set(true));
        let err = reconcile_genesis(&d, NEW, never_running);
        test_hooks::FAIL_RETIRE.with(|f| f.set(false));
        let err = err.expect_err("a failed set-aside refuses the start");
        assert!(
            matches!(&err, GenesisGateError::Io(m) if m.contains("node.toml")),
            "{err:?}"
        );
        assert_eq!(
            read_marker(&d).unwrap().as_deref(),
            Some(OLD),
            "the new marker is not committed"
        );
        assert_eq!(
            std::fs::read_to_string(d.join("node.toml")).unwrap(),
            OLD_CONFIG
        );
        assert_db_gone(&d);

        let out = reconcile_genesis(&d, NEW, never_running).unwrap();
        let GenesisOutcome::Reset { retired_config, .. } = out else {
            panic!("the retry completes the reset: {out:?}");
        };
        assert_eq!(retired_config, Some(d.join(OLD_CONFIG_ASIDE)));
        assert_config_set_aside(&d, OLD_CONFIG_ASIDE);
        assert_keys_kept(&d);
        assert_eq!(read_marker(&d).unwrap().as_deref(), Some(NEW));
    }

    /// Every interruption point of the migration, followed by a fresh start: the result is the
    /// same completed state, and a marker naming the new genesis never coexists with the old
    /// network's node.toml.
    #[test]
    fn every_interruption_point_converges_on_a_completed_migration() {
        type Step = fn(&Path);
        let steps: &[(&str, Step)] = &[
            ("nothing done", |_| {}),
            ("CURRENT deleted", |d| {
                std::fs::remove_file(d.join("CURRENT")).unwrap()
            }),
            ("database wiped", |d| {
                wipe_chain_db(d).unwrap();
            }),
            ("database wiped, config set aside", |d| {
                wipe_chain_db(d).unwrap();
                retire_node_config(d, Some(OLD)).unwrap();
            }),
            ("marker temp written", |d| {
                wipe_chain_db(d).unwrap();
                retire_node_config(d, Some(OLD)).unwrap();
                std::fs::write(d.join(format!("{GENESIS_MARKER_FILE}.tmp")), NEW).unwrap();
            }),
        ];
        for (name, crash_at) in steps {
            let d = tmp("interrupt");
            populate(&d);
            write_marker(&d, OLD).unwrap();
            crash_at(&d);
            if read_marker(&d).unwrap().as_deref() == Some(NEW) {
                panic!("{name}: the marker cannot be committed before the config is aside");
            }
            let out = reconcile_genesis(&d, NEW, never_running).unwrap();
            assert!(
                matches!(out, GenesisOutcome::Reset { .. }),
                "{name}: {out:?}"
            );
            assert_db_gone(&d);
            assert_keys_kept(&d);
            assert_config_set_aside(&d, OLD_CONFIG_ASIDE);
            assert_eq!(read_marker(&d).unwrap().as_deref(), Some(NEW), "{name}");
        }
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

    /// SCL-S8.5a / RT-11: an orphan that holds the chain database lock but never answers on
    /// the local RPC must block the reset. Nothing is deleted, the marker is unchanged, and
    /// the holder is named.
    #[test]
    fn an_orphan_holding_the_db_lock_blocks_the_reset_even_when_rpc_is_silent() {
        let d = tmp("orphan-lock");
        populate(&d);
        write_marker(&d, OLD).unwrap();
        let fx = crate::node_holder::tests::hold_chain_db_lock(&d);
        let err = reconcile_genesis(&d, NEW, never_running).expect_err("held lock refuses");
        let GenesisGateError::ChainDbHeld(blocker) = &err else {
            panic!("expected ChainDbHeld, got {err:?}");
        };
        assert_eq!(
            blocker.resource,
            crate::node_holder::BlockedResource::ChainDatabase
        );
        #[cfg(unix)]
        assert_eq!(blocker.pid, Some(fx.pid), "the holder is named");
        assert!(
            err.to_string().contains("no chain data was deleted"),
            "{err}"
        );
        for f in DB {
            assert!(d.join(f).exists(), "{f} untouched while the lock is held");
        }
        assert_keys_kept(&d);
        assert_eq!(read_marker(&d).unwrap().as_deref(), Some(OLD));

        // Once the holder is gone the reset proceeds as before.
        fx.stop();
        let out = reconcile_genesis(&d, NEW, never_running).unwrap();
        assert!(matches!(out, GenesisOutcome::Reset { .. }), "{out:?}");
        assert_db_gone(&d);
        assert_keys_kept(&d);
    }

    /// A free lock (LOCK present, nobody holding it) lets the reset proceed, and LOCK is
    /// deleted last so the lock is held over every other deletion.
    #[test]
    fn a_free_lock_proceeds_and_lock_is_deleted_last() {
        let d = tmp("free-lock");
        populate(&d);
        write_marker(&d, OLD).unwrap();
        let out = reconcile_genesis(&d, NEW, never_running).unwrap();
        let GenesisOutcome::Reset { removed, .. } = out else {
            panic!("expected a reset, got {out:?}");
        };
        assert_eq!(removed.first().map(String::as_str), Some("CURRENT"));
        assert_eq!(removed.last().map(String::as_str), Some("LOCK"));
        let mut sorted = removed.clone();
        sorted.sort();
        let mut expected: Vec<String> = DB.iter().map(|s| s.to_string()).collect();
        expected.sort();
        assert_eq!(sorted, expected, "the deleted set is unchanged");
        assert_db_gone(&d);
        assert_keys_kept(&d);
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
