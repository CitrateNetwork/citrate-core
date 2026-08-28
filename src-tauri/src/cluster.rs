//! CX-S4 (lane s4) — group private P2P cluster host commands (C-20).
//!
//! ## S4.1 — roster → allowed-peers derivation (the RBAC→network boundary)
//! A group's cluster is a private P2P mesh among its members. The authorization boundary is the
//! group roster: the cluster admits a peer connection ONLY from an address in the roster. [`allowed_peers`]
//! is that derivation — canonical, de-duplicated, stable across nodes — and is the set the hybrid
//! Noise identities are minted from in S4.2 (D-24: Noise-identity + libp2p-gossipsub). S4.1 ships +
//! TESTS the derivation; the live transport (dialing, gossipsub, connectivity, shared files) is S4.2,
//! so the `cluster_*` commands below stay honest `not wired` until then (Rule 1 — never fake a peer).
//!
//! The S4.1 cluster VIEW (who your peers WOULD be) is composed on the frontend from the existing
//! `groups_roster` command; this module owns the canonical authorization algorithm the transport
//! will enforce. Names frozen; registered in lib.rs.

/// Derive the cluster's allowed-peer set from a group roster (CX-S4.1). The cluster admits P2P
/// connections ONLY from addresses in this set — the RBAC boundary the S4.2 transport enforces when
/// minting per-peer Noise identities. Canonicalizes each address (strip `0x`, lowercase), drops
/// anything that is not a 20-byte hex address, de-duplicates, and sorts — so two nodes computing the
/// set from the same roster get byte-identical results (a stable mesh membership).
// Proven by cluster_tests in S4.1; its live consumer (per-peer Noise identity minting + gossipsub
// admission) lands in S4.2, so it reads as unused for exactly one WP (the S3.1 primitive pattern).
#[allow(dead_code)]
pub(crate) fn allowed_peers(roster: &[String]) -> Vec<String> {
    let mut set: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for raw in roster {
        if let Some(addr) = canonical_address(raw) {
            set.insert(addr);
        }
    }
    set.into_iter().collect()
}

/// Canonical form of an EVM address for the peer set: lowercase, no `0x`, exactly 40 hex chars.
/// Returns `None` for anything that is not a well-formed 20-byte address (dropped from the set).
fn canonical_address(raw: &str) -> Option<String> {
    let h = raw.trim().trim_start_matches("0x").to_ascii_lowercase();
    if h.len() == 40 && h.bytes().all(|b| b.is_ascii_hexdigit()) {
        Some(h)
    } else {
        None
    }
}

fn not_wired(cmd: &str) -> Result<(), String> {
    Err(format!("cluster::{cmd} is not wired yet (CX-S4.2 libp2p transport)"))
}

#[tauri::command]
pub fn cluster_status() -> Result<(), String> {
    not_wired("status")
}

#[tauri::command]
pub fn cluster_join() -> Result<(), String> {
    not_wired("join")
}

#[tauri::command]
pub fn cluster_peers() -> Result<(), String> {
    not_wired("peers")
}

#[tauri::command]
pub fn cluster_share_file() -> Result<(), String> {
    not_wired("share_file")
}

#[tauri::command]
pub fn cluster_leave() -> Result<(), String> {
    not_wired("leave")
}

#[cfg(test)]
mod tests {
    include!("cluster_tests.rs");
}
