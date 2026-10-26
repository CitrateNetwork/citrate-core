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
//! | [`GENESIS_MARKER_FILE`] | core (this module) | rewritten last (the commit point) |
//! | [`MIGRATION_INTENT_FILE`] | core (this module) | written first, removed after the commit |
//!
//! Deletion is by positive match on RocksDB's own file names
//! ([`is_chain_db_entry`]): anything not recognised as a database file is
//! kept, so key material the node adds later survives by default.
//!
//! ## Invariants (K6, crash safety)
//! - **I1. Nothing runs on a profile that is not ready.** `NodeManager::start` refuses a
//!   network profile that is not `ready()` before it touches the data dir, so no migration step
//!   below runs for it (and no node starts). A bad book genesis refuses here too, touching nothing.
//! - **I2. The marker is the single commit point.** The migration is a sequence of idempotent
//!   steps: write the intent, delete the chain DB, set the previous `node.toml` aside, fsync the
//!   directory, write the marker. Only the marker rename commits it. Removing the intent after
//!   the commit is cleanup. Every migration attempt mints a fresh random nonce and records it in
//!   its intent and, at the commit, in the marker: an intent is committed only when the marker
//!   carries **its** nonce. A marker that merely names the intent's target genesis (the old
//!   marker, when a launch rolls back to the genesis it already names) is not a commit.
//! - **I3. Every uncommitted step is retried on the next launch, whatever the profile is
//!   then.** The intent ([`MIGRATION_INTENT_FILE`]) is made durable before the first destructive
//!   step. While the marker does not carry its nonce, the data dir is mid-migration: the next
//!   launch starts a new attempt (new nonce, same source genesis) to the genesis that launch
//!   targets, and that attempt deletes every chain DB entry left before it writes any marker.
//!   This holds however many launches crash in a row and whatever genesis each one targets,
//!   including the genesis the old marker names (a partly deleted database is never kept).
//! - **I4. A node never opens a data dir that is mid-migration or of another network.** The node
//!   is spawned only after [`reconcile_genesis`] returns `Ok`, which it does only with the marker
//!   naming the book genesis, no intent left, no chain DB of another genesis and no `node.toml`
//!   of another network. A `node.toml` whose network is not recorded (no marker) is never kept.
//!
//! ## Steps
//! 1. Validate the book genesis; a zero or unparsable hash refuses the start.
//! 2. No intent and the marker equals the book genesis: nothing to do.
//!    An intent whose nonce the marker carries is a committed migration whose cleanup was
//!    interrupted: the intent is removed and step 2 is evaluated again. Any other intent
//!    (no nonce, an unreadable file, or a nonce the marker does not carry) is uncommitted.
//! 3. No intent, no marker, no chain DB and no `node.toml`: a fresh data dir, marked.
//! 4. Otherwise migrate, holding the database lock: write the intent with a fresh nonce (temp
//!    file, fsync, rename, directory fsync; an existing uncommitted intent keeps its source
//!    genesis and takes the new target), delete every chain DB entry (`CURRENT` first, so a
//!    partly deleted directory never looks like a complete database) and check that none is
//!    left, set the previous network's `node.toml` aside ([`retire_node_config`]), fsync the
//!    directory, write the marker (genesis and nonce) atomically, then remove the intent.
//!
//! A crash or error anywhere before the marker rename leaves the intent (or, before the intent
//! was durable, an untouched database), so the next start runs every step again; each is
//! idempotent (a `node.toml` already set aside is simply absent). Once the marker names the new
//! genesis, the old `node.toml` is already gone, so `ensure_node_config` can only write a fresh
//! one from the active profile: the previous network's bootnodes and chain id never survive a
//! completed migration. This holds for a database with no marker too (Codex P2 round 3 on
//! core#363): the intent is written before it is deleted, and a marker-less dir that still has a
//! `node.toml` is migrated, not freshly marked.
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

/// The migration intent: present from before the first destructive step of a migration until
/// after its commit (the marker write). See the module doc, invariants I2 and I3.
pub const MIGRATION_INTENT_FILE: &str = "chain-genesis.migrating";

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
    /// No chain data, no marker, no intent and no `node.toml`: a fresh data dir, marked.
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
    // Tests can stop the delete after the first `k` entries, as a crash there would.
    #[cfg(test)]
    let stop_at = test_hooks::STOP_WIPE_AFTER.with(|n| n.get());
    #[cfg(test)]
    let todo: &[String] = match stop_at {
        Some(k) => &names[..k.min(names.len())],
        None => &names,
    };
    #[cfg(not(test))]
    let todo: &[String] = &names;
    for name in todo {
        let path = data_dir.join(name);
        bytes = bytes.saturating_add(std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0));
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(GenesisGateError::Io(format!("remove {name}: {e}"))),
        }
    }
    // A stop set for more entries than were left still stops the run here (a crash right after
    // the last unlink), so every crash point is reachable whatever an earlier attempt deleted.
    #[cfg(test)]
    if stop_at.is_some() {
        return Err(GenesisGateError::Io(
            "injected stop mid-delete (test)".into(),
        ));
    }
    #[cfg(test)]
    if let Some(name) = test_hooks::APPEAR_AFTER_WIPE.with(|n| n.take()) {
        std::fs::write(data_dir.join(name), name.as_bytes())
            .map_err(|e| GenesisGateError::Io(format!("test hook: {e}")))?;
    }
    Ok((names, bytes))
}

/// The suffix a previous network's `node.toml` is set aside under: the first 10 characters of its
/// genesis, or `unrecorded`.
fn aside_tag(previous_genesis: Option<&str>) -> String {
    let tag: String = previous_genesis
        .map(|g| g.chars().take(10).collect())
        .unwrap_or_else(|| "unrecorded".to_string());
    tag.chars().filter(|c| c.is_ascii_alphanumeric()).collect()
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
    let tag = aside_tag(previous_genesis);
    let mut dest = data_dir.join(format!("node.toml.{tag}"));
    let mut n = 1;
    while std::fs::symlink_metadata(&dest).is_ok() {
        dest = data_dir.join(format!("node.toml.{tag}.{n}"));
        n += 1;
    }
    std::fs::rename(&path, &dest)?;
    Ok(Some(dest))
}

/// Where a switch away from `previous_genesis` left the previous `node.toml`: the newest
/// `node.toml.<tag>[.n]` [`retire_node_config`] names for it. A launch that finishes a switch
/// interrupted after the set-aside gets `retired_config: None` from the reset (nothing left to move)
/// and still has to tell the member where the file is.
pub(crate) fn set_aside_node_config(
    data_dir: &Path,
    previous_genesis: Option<&str>,
) -> Option<PathBuf> {
    let tag = aside_tag(previous_genesis);
    let mut found = None;
    let mut dest = data_dir.join(format!("node.toml.{tag}"));
    let mut n = 1;
    while std::fs::symlink_metadata(&dest).is_ok() {
        found = Some(dest);
        dest = data_dir.join(format!("node.toml.{tag}.{n}"));
        n += 1;
    }
    found
}

/// The member-facing notice for a reset: [`CHAIN_RESET_NOTICE`], plus where the previous network's
/// node settings were kept when there were any, so a hand edit there is never dropped silently.
pub fn reset_notice(set_aside_config: Option<&Path>) -> String {
    match set_aside_config {
        Some(p) => format!(
            "{CHAIN_RESET_NOTICE}. The previous network's node settings were kept in {}; new \
             settings were written for this network.",
            p.display()
        ),
        None => CHAIN_RESET_NOTICE.to_string(),
    }
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
        /// The migration stops (returns an error, as a crash would leave the disk) right after
        /// this step.
        pub static STOP_AFTER: Cell<Option<super::Step>> = const { Cell::new(None) };
        /// `wipe_chain_db` stops after deleting this many entries (a crash mid-delete), or after
        /// the last one when fewer were left.
        pub static STOP_WIPE_AFTER: Cell<Option<usize>> = const { Cell::new(None) };
        /// A chain database file of this name appears once `wipe_chain_db` has deleted the
        /// entries it listed (one written between the listing and the end of the delete).
        pub static APPEAR_AFTER_WIPE: Cell<Option<&'static str>> = const { Cell::new(None) };
    }
}

/// The migration steps, for the crash-injection tests ([`test_hooks::STOP_AFTER`]).
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Step {
    IntentWritten,
    DbWiped,
    ConfigRetired,
    DirSynced,
    MarkerWritten,
}

/// Test-only: stop the migration after `step` as a crash there would.
fn checkpoint(_step: &'static str) -> Result<(), GenesisGateError> {
    #[cfg(test)]
    {
        let hit = test_hooks::STOP_AFTER.with(|s| s.get()).is_some_and(|s| {
            matches!(
                (s, _step),
                (Step::IntentWritten, "intent")
                    | (Step::DbWiped, "wipe")
                    | (Step::ConfigRetired, "retire")
                    | (Step::DirSynced, "sync")
                    | (Step::MarkerWritten, "marker")
            )
        });
        if hit {
            return Err(GenesisGateError::Io(format!(
                "injected stop after {_step} (test)"
            )));
        }
    }
    Ok(())
}

fn marker_path(data_dir: &Path) -> PathBuf {
    data_dir.join(GENESIS_MARKER_FILE)
}

/// The marker: its first line is the genesis; a migration that committed it adds a
/// `migration <nonce>` line (I2). A marker written for a fresh data dir has no nonce.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Marker {
    /// The genesis, lowercase and trimmed.
    pub genesis: String,
    /// The nonce of the migration that committed this marker, if any.
    pub nonce: Option<String>,
}

/// The marker, if present.
pub fn read_marker_record(data_dir: &Path) -> Result<Option<Marker>, GenesisGateError> {
    let text = match std::fs::read_to_string(marker_path(data_dir)) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(GenesisGateError::Io(format!("read marker: {e}"))),
    };
    let genesis = text
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    let nonce = text
        .lines()
        .skip(1)
        .find_map(|l| l.trim().strip_prefix("migration "))
        .and_then(parse_nonce);
    Ok(Some(Marker { genesis, nonce }))
}

/// The genesis the marker names, lowercase and trimmed; `None` when absent.
#[cfg(test)]
pub fn read_marker(data_dir: &Path) -> Result<Option<String>, GenesisGateError> {
    Ok(read_marker_record(data_dir)?.map(|m| m.genesis))
}

/// A migration nonce: 32 lowercase hex digits (128 random bits).
fn parse_nonce(v: &str) -> Option<String> {
    let v = v.trim().to_ascii_lowercase();
    (v.len() == 32 && v.bytes().all(|b| b.is_ascii_hexdigit())).then_some(v)
}

/// A fresh nonce for one migration attempt (I2).
fn new_nonce() -> String {
    format!("{:032x}", rand::random::<u128>())
}

/// Write `contents` to `data_dir/name` atomically and durably: temp file, fsync, rename,
/// directory fsync.
fn write_durable(data_dir: &Path, name: &str, contents: &str) -> std::io::Result<()> {
    use std::io::Write;
    std::fs::create_dir_all(data_dir)?;
    let tmp = data_dir.join(format!("{name}.tmp"));
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(contents.as_bytes())?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, data_dir.join(name))?;
    sync_dir(data_dir)
}

/// Write the marker atomically: temp file, fsync, rename, directory fsync. `nonce` is the
/// committing migration's (I2); `None` for a fresh data dir.
fn write_marker_with(
    data_dir: &Path,
    genesis: &str,
    nonce: Option<&str>,
) -> Result<(), GenesisGateError> {
    let body = match nonce {
        Some(n) => format!("{genesis}\nmigration {n}\n"),
        None => format!("{genesis}\n"),
    };
    write_durable(data_dir, GENESIS_MARKER_FILE, &body)
        .map_err(|e| GenesisGateError::Io(format!("write marker: {e}")))
}

/// Write a marker with no migration nonce (a fresh data dir).
fn write_marker(data_dir: &Path, genesis: &str) -> Result<(), GenesisGateError> {
    write_marker_with(data_dir, genesis, None)
}

/// An uncommitted (or not yet cleaned up) migration, from [`MIGRATION_INTENT_FILE`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationIntent {
    /// The genesis the data belonged to when the migration began (`None`: not recorded).
    pub from: Option<String>,
    /// The genesis the migration was moving to (`None`: unreadable).
    pub to: Option<String>,
    /// This attempt's nonce (`None`: missing or unreadable, which never matches a marker, so
    /// the migration is rolled forward).
    pub nonce: Option<String>,
}

impl MigrationIntent {
    /// Whether `marker` is this migration's commit (I2): it carries this intent's nonce (and
    /// names its target). A marker that only names the same genesis is not.
    fn committed_by(&self, marker: Option<&Marker>) -> bool {
        match (marker, &self.nonce) {
            (Some(m), Some(n)) => {
                m.nonce.as_deref() == Some(n.as_str())
                    && self.to.as_deref() == Some(m.genesis.as_str())
            }
            _ => false,
        }
    }
}

fn intent_path(data_dir: &Path) -> PathBuf {
    data_dir.join(MIGRATION_INTENT_FILE)
}

/// The migration intent, if one is present. Its contents are only ever replaced by rename, so
/// it is complete or absent; anything unparsable still counts as an intent (with no target),
/// which rolls the migration forward rather than keeping a possibly half-deleted database.
pub fn read_intent(data_dir: &Path) -> Result<Option<MigrationIntent>, GenesisGateError> {
    let text = match std::fs::read_to_string(intent_path(data_dir)) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            // Present but unreadable as text: still an intent.
            if std::fs::symlink_metadata(intent_path(data_dir)).is_ok() {
                eprintln!("[node] the migration intent is unreadable ({e}); rolling forward");
                return Ok(Some(MigrationIntent {
                    from: None,
                    to: None,
                    nonce: None,
                }));
            }
            return Err(GenesisGateError::Io(format!("read migration intent: {e}")));
        }
    };
    let field = |key: &str| {
        text.lines()
            .find_map(|l| l.strip_prefix(key))
            .map(|v| v.trim().to_ascii_lowercase())
            .and_then(|v| validate_book_genesis(&v).ok())
    };
    let nonce = text
        .lines()
        .find_map(|l| l.strip_prefix("nonce "))
        .and_then(parse_nonce);
    Ok(Some(MigrationIntent {
        from: field("from "),
        to: field("to "),
        nonce,
    }))
}

fn write_intent(data_dir: &Path, intent: &MigrationIntent) -> Result<(), GenesisGateError> {
    let from = intent.from.as_deref().unwrap_or("unrecorded");
    let to = intent.to.as_deref().unwrap_or("unrecorded");
    let nonce = intent.nonce.as_deref().unwrap_or("unrecorded");
    write_durable(
        data_dir,
        MIGRATION_INTENT_FILE,
        &format!("nonce {nonce}\nfrom {from}\nto {to}\n"),
    )
    .map_err(|e| GenesisGateError::Io(format!("write the migration intent: {e}")))
}

/// Remove the intent after the commit (cleanup; I2). Absent is fine.
fn clear_intent(data_dir: &Path) -> Result<(), GenesisGateError> {
    let io = |e: std::io::Error| GenesisGateError::Io(format!("remove the migration intent: {e}"));
    for p in [
        intent_path(data_dir),
        data_dir.join(format!("{MIGRATION_INTENT_FILE}.tmp")),
    ] {
        match std::fs::remove_file(&p) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(io(e)),
        }
    }
    sync_dir(data_dir).map_err(io)
}

/// Make `data_dir` safe to start a node for `book_genesis` (see the module doc: invariants
/// I1 to I4 and the steps).
///
/// `node_running` is asked only when a reset is needed and chain data is present; when it
/// reports a live node the reset is refused rather than deleting a database in use. The reset
/// also requires the chain database lock (SCL-S8.5a): a process holding it refuses the reset
/// even when `node_running` reports nothing.
pub fn reconcile_genesis(
    data_dir: &Path,
    book_genesis: &str,
    node_running: impl FnOnce() -> bool,
) -> Result<GenesisOutcome, GenesisGateError> {
    let book = validate_book_genesis(book_genesis)?;
    let marker_record = read_marker_record(data_dir)?;
    let marker = marker_record.as_ref().map(|m| m.genesis.clone());
    let mut intent = read_intent(data_dir)?;
    // A migration whose commit (the marker carrying its nonce) happened but whose cleanup did
    // not: finish it. Any other intent is uncommitted and rolled forward below (I2, I3).
    if let Some(i) = &intent {
        if i.committed_by(marker_record.as_ref()) {
            clear_intent(data_dir)?;
            intent = None;
        }
    }
    if intent.is_none() && marker.as_deref() == Some(book.as_str()) {
        return Ok(GenesisOutcome::Kept);
    }
    let has_chain_data = !chain_db_entries(data_dir)?.is_empty();
    let has_config = std::fs::symlink_metadata(data_dir.join("node.toml")).is_ok();
    if intent.is_none() && marker.is_none() && !has_chain_data && !has_config {
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
    // I3: the intent is durable before the first destructive step. An earlier, uncommitted
    // intent keeps the genesis the data came from (it names the set-aside config) and takes this
    // launch's target, so a profile change between launches rolls forward to the new target.
    // Every attempt mints its own nonce (I2): only a marker this attempt writes commits it.
    let previous = match &intent {
        Some(i) => i.from.clone(),
        None => marker.clone(),
    };
    let nonce = new_nonce();
    write_intent(
        data_dir,
        &MigrationIntent {
            from: previous.clone(),
            to: Some(book.clone()),
            nonce: Some(nonce.clone()),
        },
    )?;
    checkpoint("intent")?;
    let (removed, bytes) = wipe_chain_db(data_dir)?;
    // The delete is complete before any marker is written: whatever an earlier attempt left.
    let left = chain_db_entries(data_dir)?;
    if !left.is_empty() {
        return Err(GenesisGateError::Io(format!(
            "chain database entries remain after the delete: {}",
            left.join(", ")
        )));
    }
    checkpoint("wipe")?;
    // K6: the previous network's node.toml goes before the marker commits the switch.
    let retired_config = retire_node_config(data_dir, previous.as_deref())
        .map_err(|e| GenesisGateError::Io(format!("set the previous node.toml aside: {e}")))?;
    checkpoint("retire")?;
    sync_dir(data_dir).map_err(|e| GenesisGateError::Io(format!("sync the node data dir: {e}")))?;
    checkpoint("sync")?;
    // I2: the commit point, carrying this attempt's nonce.
    write_marker_with(data_dir, &book, Some(&nonce))?;
    checkpoint("marker")?;
    clear_intent(data_dir)?;
    drop(lock);
    Ok(GenesisOutcome::Reset {
        previous,
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

    /// MAC round 5 on core#363: a chain database entry that is present once the delete has run
    /// (written after the delete listed the directory) refuses the start with the OLD marker
    /// and the intent in place; nothing is committed over it. The next start deletes it and
    /// completes the migration.
    #[test]
    fn an_entry_that_survives_the_delete_does_not_commit_the_marker() {
        let d = tmp("relist");
        populate(&d);
        write_marker(&d, OLD).unwrap();
        test_hooks::APPEAR_AFTER_WIPE.with(|n| n.set(Some("000999.sst")));
        let out = reconcile_genesis(&d, NEW, never_running);
        test_hooks::APPEAR_AFTER_WIPE.with(|n| n.set(None));
        let err = out.expect_err("a database entry left after the delete refuses the start");
        assert!(
            matches!(&err, GenesisGateError::Io(m) if m.contains("000999.sst")),
            "{err:?}"
        );
        assert_eq!(
            read_marker(&d).unwrap().as_deref(),
            Some(OLD),
            "the new marker is not committed"
        );
        assert!(read_intent(&d).unwrap().is_some(), "the intent stays");
        assert_eq!(
            std::fs::read_to_string(d.join("node.toml")).unwrap(),
            OLD_CONFIG,
            "the config is not set aside before the delete is complete"
        );

        let out = reconcile_genesis(&d, NEW, never_running).unwrap();
        assert!(matches!(out, GenesisOutcome::Reset { .. }), "{out:?}");
        assert_db_gone(&d);
        assert!(!d.join("000999.sst").exists());
        assert_config_set_aside(&d, OLD_CONFIG_ASIDE);
        assert_keys_kept(&d);
        assert_eq!(read_marker(&d).unwrap().as_deref(), Some(NEW));
        assert!(read_intent(&d).unwrap().is_none());
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

    // -----------------------------------------------------------------------------------
    // Round 3 (Codex P2 on core#363): invariants M1-M4 with an intent journal. Every crash
    // point, from a marked AND an unrecorded database, followed by a launch for the same, a
    // third or the previous genesis, converges on a committed migration to that launch's genesis.
    // -----------------------------------------------------------------------------------

    const THIRD: &str = "0x3333330000000000000000000000000000000000000000000000000000000003";
    const UNRECORDED_ASIDE: &str = "node.toml.unrecorded";

    /// Where a crash is injected.
    #[derive(Debug, Clone, Copy)]
    enum Crash {
        After(Step),
        MidWipe,
        RetireFails,
    }

    const CRASHES: &[Crash] = &[
        Crash::After(Step::IntentWritten),
        Crash::MidWipe,
        Crash::After(Step::DbWiped),
        Crash::RetireFails,
        Crash::After(Step::ConfigRetired),
        Crash::After(Step::DirSynced),
        Crash::After(Step::MarkerWritten),
    ];

    /// Run one migration to `to` that is interrupted at `crash`; returns its (failed) result.
    fn interrupted(d: &Path, to: &str, crash: Crash) -> Result<GenesisOutcome, GenesisGateError> {
        match crash {
            Crash::After(step) => test_hooks::STOP_AFTER.with(|s| s.set(Some(step))),
            Crash::MidWipe => test_hooks::STOP_WIPE_AFTER.with(|n| n.set(Some(1))),
            Crash::RetireFails => test_hooks::FAIL_RETIRE.with(|f| f.set(true)),
        }
        let r = reconcile_genesis(d, to, never_running);
        test_hooks::STOP_AFTER.with(|s| s.set(None));
        test_hooks::STOP_WIPE_AFTER.with(|n| n.set(None));
        test_hooks::FAIL_RETIRE.with(|f| f.set(false));
        r
    }

    /// M4: the committed state for `genesis`, with the original config kept aside exactly once.
    fn assert_committed(d: &Path, genesis: &str, aside: &str, ctx: &str) {
        assert_eq!(
            read_marker(d).unwrap().as_deref(),
            Some(genesis),
            "{ctx}: marker"
        );
        assert_eq!(read_intent(d).unwrap(), None, "{ctx}: no intent left");
        assert!(
            !d.join(format!("{MIGRATION_INTENT_FILE}.tmp")).exists(),
            "{ctx}: no intent temp left"
        );
        assert!(
            chain_db_entries(d).unwrap().is_empty(),
            "{ctx}: no chain DB left"
        );
        assert!(
            !d.join("node.toml").exists(),
            "{ctx}: no old node.toml left"
        );
        let copies: Vec<String> = std::fs::read_dir(d)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.starts_with("node.toml."))
            .collect();
        assert_eq!(
            copies,
            vec![aside.to_string()],
            "{ctx}: set aside exactly once"
        );
        assert_eq!(
            std::fs::read_to_string(d.join(aside)).unwrap(),
            OLD_CONFIG,
            "{ctx}: the old config is preserved"
        );
        assert_keys_kept(d);
    }

    #[test]
    fn m1_m4_every_crash_point_and_every_next_profile_converges() {
        for marked in [true, false] {
            let aside = if marked {
                OLD_CONFIG_ASIDE
            } else {
                UNRECORDED_ASIDE
            };
            for crash in CRASHES {
                for next in [NEW, THIRD, OLD] {
                    let ctx = format!("marked={marked} crash={crash:?} next={next}");
                    let d = tmp("m-matrix");
                    populate(&d);
                    if marked {
                        write_marker(&d, OLD).unwrap();
                    }
                    let first = interrupted(&d, NEW, *crash);
                    assert!(first.is_err(), "{ctx}: the injected crash stops the run");
                    // M2: nothing is committed before the marker; the intent is there until then.
                    let committed = matches!(crash, Crash::After(Step::MarkerWritten));
                    assert_eq!(
                        read_marker(&d).unwrap().as_deref() == Some(NEW),
                        committed,
                        "{ctx}: the marker is the commit point"
                    );
                    assert!(read_intent(&d).unwrap().is_some(), "{ctx}: intent present");
                    if !committed {
                        assert!(
                            !(read_marker(&d).unwrap().as_deref() == Some(NEW)
                                && d.join("node.toml").exists()),
                            "{ctx}: never the new marker beside the old config"
                        );
                    }
                    // The next launch, whatever its profile.
                    let out = reconcile_genesis(&d, next, never_running)
                        .unwrap_or_else(|e| panic!("{ctx}: {e}"));
                    if committed && next == NEW {
                        assert_eq!(out, GenesisOutcome::Kept, "{ctx}");
                    } else {
                        assert!(
                            matches!(out, GenesisOutcome::Reset { .. }),
                            "{ctx}: {out:?}"
                        );
                    }
                    assert_committed(&d, next, aside, &ctx);
                    // And the launch after that keeps everything.
                    assert_eq!(
                        reconcile_genesis(&d, next, || panic!("no probe")).unwrap(),
                        GenesisOutcome::Kept,
                        "{ctx}"
                    );
                    let _ = std::fs::remove_dir_all(&d);
                }
            }
        }
    }

    /// Codex P2 round 3, the reported case: a database with no marker is wiped, then the run
    /// stops before node.toml is set aside. The next launch must not take the fresh path and
    /// keep the previous network's config.
    #[test]
    fn unrecorded_db_wiped_then_crash_does_not_keep_the_old_config() {
        let d = tmp("unrec-crash");
        populate(&d);
        assert!(interrupted(&d, NEW, Crash::After(Step::DbWiped)).is_err());
        assert!(chain_db_entries(&d).unwrap().is_empty(), "the DB is gone");
        assert!(d.join("node.toml").exists(), "the config is still there");
        assert_eq!(read_marker(&d).unwrap(), None, "no marker");
        let out = reconcile_genesis(&d, NEW, never_running).unwrap();
        assert!(
            matches!(&out, GenesisOutcome::Reset { previous: None, retired_config: Some(p), .. }
                if p == &d.join(UNRECORDED_ASIDE)),
            "{out:?}"
        );
        assert_committed(&d, NEW, UNRECORDED_ASIDE, "unrecorded crash");
    }

    /// Codex P2 round 3: the same database with no marker, where setting node.toml aside fails.
    #[test]
    fn unrecorded_db_retire_failure_is_retried() {
        let d = tmp("unrec-retire");
        populate(&d);
        let err = interrupted(&d, NEW, Crash::RetireFails).expect_err("refused");
        assert!(
            matches!(&err, GenesisGateError::Io(m) if m.contains("node.toml")),
            "{err:?}"
        );
        assert_eq!(read_marker(&d).unwrap(), None);
        assert_eq!(
            std::fs::read_to_string(d.join("node.toml")).unwrap(),
            OLD_CONFIG
        );
        reconcile_genesis(&d, NEW, never_running).unwrap();
        assert_committed(&d, NEW, UNRECORDED_ASIDE, "unrecorded retire failure");
    }

    /// M4: a dir left by an older build's interrupted migration (no marker, no intent, no
    /// database, but a node.toml) is migrated, not freshly marked with that config kept.
    #[test]
    fn a_marker_less_dir_with_a_node_toml_and_no_db_is_migrated_not_fresh() {
        let d = tmp("legacy-leftover");
        for f in KEPT {
            std::fs::write(d.join(f), f.as_bytes()).unwrap();
        }
        std::fs::write(d.join("node.toml"), OLD_CONFIG).unwrap();
        let out = reconcile_genesis(&d, NEW, || panic!("no DB, no probe")).unwrap();
        assert!(
            matches!(out, GenesisOutcome::Reset { previous: None, .. }),
            "{out:?}"
        );
        assert_committed(&d, NEW, UNRECORDED_ASIDE, "legacy leftover");
    }

    /// M3: an intent that cannot be parsed still means "mid-migration": rolled forward, never
    /// treated as absent (which could keep a half-deleted database on a marker match).
    #[test]
    fn an_unreadable_intent_rolls_the_migration_forward() {
        let d = tmp("bad-intent");
        populate(&d);
        write_marker(&d, OLD).unwrap();
        std::fs::write(d.join(MIGRATION_INTENT_FILE), b"\xff\xfe garbage").unwrap();
        let out = reconcile_genesis(&d, OLD, never_running).unwrap();
        assert!(matches!(out, GenesisOutcome::Reset { .. }), "{out:?}");
        assert_eq!(read_marker(&d).unwrap().as_deref(), Some(OLD));
        assert_eq!(read_intent(&d).unwrap(), None);
        assert!(chain_db_entries(&d).unwrap().is_empty());
        assert!(!d.join("node.toml").exists());
    }

    /// M3: the profile changes back to the previous genesis after a migration began. The marker
    /// still names it, but the database may be half deleted: it is never kept.
    #[test]
    fn a_profile_changed_back_to_the_old_genesis_mid_migration_is_not_kept() {
        let d = tmp("back-to-old");
        populate(&d);
        write_marker(&d, OLD).unwrap();
        assert!(interrupted(&d, NEW, Crash::MidWipe).is_err());
        assert!(!d.join("CURRENT").exists(), "CURRENT went first");
        assert!(
            !chain_db_entries(&d).unwrap().is_empty(),
            "the rest is still there"
        );
        let out = reconcile_genesis(&d, OLD, never_running).unwrap();
        assert!(
            matches!(out, GenesisOutcome::Reset { .. }),
            "not Kept: {out:?}"
        );
        assert_committed(&d, OLD, OLD_CONFIG_ASIDE, "back to old");
    }

    /// M2: the intent keeps the genesis the data came from across launches, so the config set
    /// aside is named for the network it belonged to even when the target changed in between.
    #[test]
    fn the_intent_keeps_the_source_genesis_when_the_target_changes() {
        let d = tmp("intent-source");
        populate(&d);
        write_marker(&d, OLD).unwrap();
        let ends = |d: &Path| {
            read_intent(d)
                .unwrap()
                .map(|i| (i.from, i.to, i.nonce.is_some()))
        };
        assert!(interrupted(&d, NEW, Crash::After(Step::IntentWritten)).is_err());
        assert_eq!(ends(&d), Some((Some(OLD.into()), Some(NEW.into()), true)));
        let first_nonce = read_intent(&d).unwrap().unwrap().nonce;
        assert!(interrupted(&d, THIRD, Crash::After(Step::DbWiped)).is_err());
        assert_eq!(ends(&d), Some((Some(OLD.into()), Some(THIRD.into()), true)));
        assert_ne!(
            read_intent(&d).unwrap().unwrap().nonce,
            first_nonce,
            "a retarget is a new attempt with a new nonce"
        );
        let out = reconcile_genesis(&d, THIRD, never_running).unwrap();
        let GenesisOutcome::Reset { previous, .. } = out else {
            panic!("{out:?}")
        };
        assert_eq!(previous.as_deref(), Some(OLD));
        assert_committed(&d, THIRD, OLD_CONFIG_ASIDE, "target changed");
    }

    /// M2: a committed migration whose intent removal was interrupted is only cleaned up: the
    /// data is kept (nothing is wiped after the commit).
    #[test]
    fn a_committed_migration_with_a_leftover_intent_is_only_cleaned_up() {
        let d = tmp("committed-cleanup");
        populate(&d);
        std::fs::remove_file(d.join("node.toml")).unwrap();
        let n = new_nonce();
        write_marker_with(&d, NEW, Some(&n)).unwrap();
        write_intent(
            &d,
            &MigrationIntent {
                from: Some(OLD.into()),
                to: Some(NEW.into()),
                nonce: Some(n),
            },
        )
        .unwrap();
        assert_eq!(
            reconcile_genesis(&d, NEW, || panic!("no reset")).unwrap(),
            GenesisOutcome::Kept
        );
        assert_eq!(read_intent(&d).unwrap(), None);
        for f in DB {
            assert!(d.join(f).exists(), "{f}: nothing is wiped after the commit");
        }
    }

    /// I2 (MAC round 4 on core#363): a marker that names the intent's target genesis but not
    /// its nonce is not that migration's commit. The data is never kept: the migration is
    /// rolled forward. Covers a marker with no nonce, another migration's nonce, and an intent
    /// with no (or an unreadable) nonce.
    #[test]
    fn a_marker_naming_the_target_without_the_intent_nonce_is_not_a_commit() {
        let n = new_nonce();
        let other = new_nonce();
        let cases: &[(Option<&str>, Option<&str>)] = &[
            (None, Some(n.as_str())),
            (Some(other.as_str()), Some(n.as_str())),
            (Some(n.as_str()), None),
        ];
        for (marker_nonce, intent_nonce) in cases {
            let ctx = format!("marker={marker_nonce:?} intent={intent_nonce:?}");
            let d = tmp("nonce-mismatch");
            populate(&d);
            write_marker_with(&d, OLD, *marker_nonce).unwrap();
            write_intent(
                &d,
                &MigrationIntent {
                    from: Some(OLD.into()),
                    to: Some(OLD.into()),
                    nonce: intent_nonce.map(str::to_string),
                },
            )
            .unwrap();
            let out = reconcile_genesis(&d, OLD, never_running).unwrap();
            assert!(
                matches!(out, GenesisOutcome::Reset { .. }),
                "{ctx}: not Kept: {out:?}"
            );
            assert_committed(&d, OLD, OLD_CONFIG_ASIDE, &ctx);
            let _ = std::fs::remove_dir_all(&d);
        }
    }

    /// Marker format: the first line is the genesis, a committing migration adds its nonce, and
    /// a marker from before nonces (one line) still reads as that genesis with no nonce.
    #[test]
    fn the_marker_records_the_committing_nonce_and_old_markers_still_read() {
        let d = tmp("marker-format");
        std::fs::write(d.join(GENESIS_MARKER_FILE), format!("{NEW}\n")).unwrap();
        assert_eq!(
            read_marker_record(&d).unwrap(),
            Some(Marker {
                genesis: NEW.into(),
                nonce: None
            })
        );
        let n = new_nonce();
        assert_eq!(n.len(), 32);
        assert_ne!(n, new_nonce(), "fresh per attempt");
        write_marker_with(&d, OLD, Some(&n)).unwrap();
        assert_eq!(read_marker(&d).unwrap().as_deref(), Some(OLD));
        assert_eq!(read_marker_record(&d).unwrap().unwrap().nonce, Some(n));
        let _ = std::fs::remove_dir_all(&d);
    }

    /// M3, the sequence MAC reproduced on ff2c18e (round 4 on core#363): a migration to NEW
    /// crashes mid-delete; the next launch targets OLD (the genesis the marker still names) and
    /// crashes too; the launch after that must not keep the half-deleted database.
    #[test]
    fn crash_then_retarget_to_the_marker_genesis_then_crash_then_launch_is_not_kept() {
        for second in CRASHES {
            if matches!(second, Crash::After(Step::MarkerWritten)) {
                continue; // that one commits OLD; covered by the full matrix below
            }
            let ctx = format!("second={second:?}");
            let d = tmp("double-crash");
            populate(&d);
            write_marker(&d, OLD).unwrap();
            assert!(interrupted(&d, NEW, Crash::MidWipe).is_err(), "{ctx}");
            assert!(!d.join("CURRENT").exists(), "{ctx}: CURRENT went first");
            assert!(interrupted(&d, OLD, *second).is_err(), "{ctx}");
            assert_eq!(
                read_marker(&d).unwrap().as_deref(),
                Some(OLD),
                "{ctx}: the old marker"
            );
            let out = reconcile_genesis(&d, OLD, never_running).unwrap();
            assert!(
                matches!(out, GenesisOutcome::Reset { .. }),
                "{ctx}: a partly deleted DB is never kept: {out:?}"
            );
            assert_committed(&d, OLD, OLD_CONFIG_ASIDE, &ctx);
            let _ = std::fs::remove_dir_all(&d);
        }
    }

    /// M1-M4 across two consecutive crashes: every first crash point (to NEW), then a launch for
    /// every target crashing at every point, then a launch for every target, from a marked and
    /// an unrecorded database. The third launch always ends committed to its own genesis with no
    /// chain DB left (nothing partly deleted is kept), and the launch after it keeps everything.
    #[test]
    fn m1_m4_two_consecutive_crashes_at_every_point_and_every_profile_converge() {
        let targets = [NEW, THIRD, OLD];
        let mut cases = 0usize;
        for marked in [true, false] {
            let aside = if marked {
                OLD_CONFIG_ASIDE
            } else {
                UNRECORDED_ASIDE
            };
            for first in CRASHES {
                for mid in targets {
                    for second in CRASHES {
                        for last in targets {
                            let ctx = format!(
                                "marked={marked} first={first:?}->NEW second={second:?}->{mid} last={last}"
                            );
                            let d = tmp("m2-matrix");
                            populate(&d);
                            if marked {
                                write_marker(&d, OLD).unwrap();
                            }
                            assert!(interrupted(&d, NEW, *first).is_err(), "{ctx}");
                            let first_committed =
                                matches!(first, Crash::After(Step::MarkerWritten));
                            // The second launch: it only escapes its injected crash when there
                            // is nothing to migrate (the first committed NEW and it targets NEW).
                            let r2 = interrupted(&d, mid, *second);
                            let mut committed_to = None;
                            match &r2 {
                                Ok(out) => {
                                    assert!(first_committed && mid == NEW, "{ctx}: {out:?}");
                                    assert_eq!(out, &GenesisOutcome::Kept, "{ctx}");
                                    committed_to = Some(NEW);
                                }
                                Err(_) => {
                                    if matches!(second, Crash::After(Step::MarkerWritten)) {
                                        committed_to = Some(mid);
                                    }
                                }
                            }
                            // Whatever is on disk now: the marker names the genesis of a
                            // committed attempt only alongside an empty chain DB, or it is the
                            // original marker (I2).
                            let m = read_marker(&d).unwrap();
                            if m.as_deref() != Some(OLD) && m.is_some() {
                                assert!(
                                    chain_db_entries(&d).unwrap().is_empty(),
                                    "{ctx}: a new marker only after the full delete"
                                );
                                assert!(
                                    !d.join("node.toml").exists(),
                                    "{ctx}: a new marker only after the config is aside"
                                );
                            }
                            let out = reconcile_genesis(&d, last, never_running)
                                .unwrap_or_else(|e| panic!("{ctx}: {e}"));
                            if committed_to == Some(last) {
                                assert_eq!(out, GenesisOutcome::Kept, "{ctx}");
                            } else {
                                assert!(
                                    matches!(out, GenesisOutcome::Reset { .. }),
                                    "{ctx}: {out:?}"
                                );
                            }
                            assert_committed(&d, last, aside, &ctx);
                            assert_eq!(
                                reconcile_genesis(&d, last, || panic!("no probe")).unwrap(),
                                GenesisOutcome::Kept,
                                "{ctx}"
                            );
                            let _ = std::fs::remove_dir_all(&d);
                            cases += 1;
                        }
                    }
                }
            }
        }
        assert_eq!(cases, 2 * 7 * 3 * 7 * 3, "the full matrix ran");
    }

    #[test]
    fn the_intent_is_durable_before_anything_is_deleted() {
        let d = tmp("intent-first");
        populate(&d);
        write_marker(&d, OLD).unwrap();
        assert!(interrupted(&d, NEW, Crash::After(Step::IntentWritten)).is_err());
        assert!(read_intent(&d).unwrap().is_some());
        for f in DB {
            assert!(d.join(f).exists(), "{f}: nothing deleted before the intent");
        }
        assert_eq!(
            std::fs::read_to_string(d.join("node.toml")).unwrap(),
            OLD_CONFIG
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

    /// RC52 G2: a launch that finishes an interrupted switch names the copy the earlier attempt
    /// set aside, the newest one for that genesis, and never another genesis's copy.
    #[test]
    fn the_set_aside_config_lookup_names_the_newest_copy_for_that_genesis() {
        let d = tmp("aside-lookup");
        assert_eq!(set_aside_node_config(&d, Some(OLD)), None);
        std::fs::write(d.join("node.toml"), "first").unwrap();
        let first = retire_node_config(&d, Some(OLD)).unwrap();
        assert_eq!(first, Some(d.join(OLD_CONFIG_ASIDE)));
        assert_eq!(set_aside_node_config(&d, Some(OLD)), first);
        std::fs::write(d.join("node.toml"), "second").unwrap();
        let second = retire_node_config(&d, Some(OLD)).unwrap();
        assert_eq!(set_aside_node_config(&d, Some(OLD)), second);
        assert_ne!(first, second);
        assert_eq!(set_aside_node_config(&d, Some(NEW)), None);
        assert_eq!(set_aside_node_config(&d, None), None);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_reset_notice_names_the_set_aside_config_only_when_there_is_one() {
        assert_eq!(reset_notice(None), CHAIN_RESET_NOTICE);
        let p = Path::new("/data/node/node.toml.0x0f2b567f");
        let n = reset_notice(Some(p));
        assert!(n.starts_with(CHAIN_RESET_NOTICE), "{n}");
        assert!(n.contains("/data/node/node.toml.0x0f2b567f"), "{n}");
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
