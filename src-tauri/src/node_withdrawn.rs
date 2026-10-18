//! Start-up recovery from a withdrawn branch (core #326).
//!
//! An operator rollback (`citrate rollback`, citrate-chain #305) removes blocks from the fleet
//! and records them as withdrawn in each fleet node's own database, so the fleet refuses that
//! branch from then on. A desktop node that followed the withdrawn branch before the rollback
//! still holds it in its chain database, and nothing in that database says it was withdrawn: it
//! would sit on a dead branch. So the network's canonical block just above each rollback point
//! is compiled in here ([`FORK_CHECKS`]). Once this node has a block at that height, it is
//! compared with the canonical one; on a different hash the node is stopped, its chain database
//! files are moved aside to a sibling `node.withdrawn-<date>` folder, and it starts again with an
//! empty chain database and syncs from the fleet.
//!
//! What is moved is only what RocksDB writes ([`crate::node_genesis::is_chain_db_entry`]):
//! `proposer.key`, `noise.key`, `node.toml`, the genesis marker and every other file stay in the
//! data dir, so the node keeps its identity and the wallet is never touched (it does not live in
//! the node data dir). A healthy database is never moved: the only trigger is this node's own
//! RPC returning a block at the check height whose hash differs from the compiled-in one. A
//! failed read, a node still below the height, or another genesis does nothing.
//!
//! Data source (Rule 7): this node's own RPC (`eth_blockNumber`, `eth_getBlockByNumber`).

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::rpc::RpcTransport;

/// The canonical block just above one operator rollback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ForkCheck {
    /// The genesis this check belongs to (the address book's `genesisHash`). A check never
    /// applies to another genesis.
    pub genesis: &'static str,
    /// The first height above the kept block.
    pub height: u64,
    /// The network's canonical block at `height`.
    pub canonical: &'static str,
    /// For the log line and the notice.
    pub label: &'static str,
}

/// Every rollback a desktop node must not stay behind. Read from the fleet after DGX's rollback
/// (fed#289, 2026-10-16): 365,784 = `0x63dcd4a8…2c56` (kept), 365,785 = `0x51482ff9…f3d2`
/// (rpc-1), whose parent is the kept block.
pub const FORK_CHECKS: &[ForkCheck] = &[ForkCheck {
    genesis: "0x1dcfc4909dd9fdcb0f03c823d627e4b2032cc86bef5fc4561030d6c64b9775f1",
    height: 365_785,
    canonical: "0x51482ff9724a2f0cbf4fe678f3ce556f6222334d5b2a3f5eba8a2b52af0cf3d2",
    label: "the 2026-10-15 rollback to block 365,784",
}];

/// The notice the Node screen shows after a move.
pub const WITHDRAWN_NOTICE: &str = "This node had followed a branch of the chain that the network \
     withdrew. Its chain data was moved aside and it is syncing again from the network. Your \
     wallet and node keys were kept.";

/// What one check concluded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// The node's RPC did not answer (or answered something unreadable): do nothing.
    Unknown,
    /// The node is still below the check height: check again later.
    NotReached,
    /// The node's block at the check height is the canonical one: this check is done.
    Canonical,
    /// The node's block at the check height is another block: move the chain data aside.
    Withdrawn { local: String },
}

/// The checks that apply to a node of genesis `genesis`.
pub fn checks_for(genesis: &str) -> Vec<ForkCheck> {
    FORK_CHECKS
        .iter()
        .filter(|c| c.genesis.eq_ignore_ascii_case(genesis))
        .copied()
        .collect()
}

/// Decide one check from what the node's RPC returned: its head height and its block hash at
/// the check height (each `None` when the read failed).
pub fn verdict(check: &ForkCheck, local_head: Option<u64>, local_hash: Option<&str>) -> Verdict {
    let Some(head) = local_head else {
        return Verdict::Unknown;
    };
    if head < check.height {
        return Verdict::NotReached;
    }
    match local_hash {
        None => Verdict::Unknown,
        Some(h) if h.eq_ignore_ascii_case(check.canonical) => Verdict::Canonical,
        Some(h) => Verdict::Withdrawn {
            local: h.to_string(),
        },
    }
}

fn rpc_result<T: RpcTransport>(t: &T, method: &str, params: Value) -> Option<Value> {
    let resp = t
        .call(json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params }))
        .ok()?;
    if resp.get("error").is_some() {
        return None;
    }
    resp.get("result").cloned()
}

/// Run `check` against the node behind `local`.
pub fn assess<T: RpcTransport>(local: &T, check: &ForkCheck) -> Verdict {
    let head = rpc_result(local, "eth_blockNumber", json!([]))
        .and_then(|v| v.as_str().map(str::to_string))
        .and_then(|s| u64::from_str_radix(s.trim_start_matches("0x"), 16).ok());
    if head.is_none_or(|h| h < check.height) {
        return verdict(check, head, None);
    }
    let hash = rpc_result(
        local,
        "eth_getBlockByNumber",
        json!([format!("0x{:x}", check.height), false]),
    )
    .and_then(|b| b.get("hash").and_then(Value::as_str).map(str::to_string));
    verdict(check, head, hash.as_deref())
}

/// The folder a withdrawn chain database is moved to: `<data_dir>.withdrawn-<date>` next to the
/// data dir, or `-2`, `-3`, … after it when that name is taken (e.g. by an earlier manual move).
pub fn aside_dir(data_dir: &Path, date: &str) -> Option<PathBuf> {
    let parent = data_dir.parent()?;
    let name = data_dir.file_name()?.to_string_lossy().to_string();
    let base = format!("{name}.withdrawn-{date}");
    (1..1000u32)
        .map(|i| {
            if i == 1 {
                parent.join(&base)
            } else {
                parent.join(format!("{base}-{i}"))
            }
        })
        .find(|p| !p.exists())
}

/// Move the chain database files of `data_dir` (and nothing else) into a new
/// [`aside_dir`]. Call with the node stopped. Returns the folder and the moved names; with no
/// chain database files it moves nothing, creates no folder and returns `Ok(None)`.
pub fn move_chain_db_aside(
    data_dir: &Path,
    date: &str,
) -> Result<Option<(PathBuf, Vec<String>)>, String> {
    let names = crate::node_genesis::chain_db_entries(data_dir).map_err(|e| format!("{e}"))?;
    if names.is_empty() {
        return Ok(None);
    }
    let dest = aside_dir(data_dir, date)
        .ok_or_else(|| format!("no free folder name next to {}", data_dir.display()))?;
    std::fs::create_dir_all(&dest).map_err(|e| format!("create {}: {e}", dest.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| format!("protect {}: {e}", dest.display()))?;
    }
    for name in &names {
        std::fs::rename(data_dir.join(name), dest.join(name))
            .map_err(|e| format!("move {name} to {}: {e}", dest.display()))?;
    }
    Ok(Some((dest, names)))
}

/// Today's date (UTC) as `YYYY-MM-DD`, for the folder name.
pub fn today_utc() -> String {
    let days = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() / 86_400)
        .unwrap_or(0);
    civil_date(days)
}

/// Days since 1970-01-01 → `YYYY-MM-DD` (Howard Hinnant's civil-from-days).
pub fn civil_date(days: u64) -> String {
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

#[cfg(test)]
mod tests {
    include!("node_withdrawn_tests.rs");
}
