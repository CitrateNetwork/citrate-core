//! citrate-core — seam-domain commands (CORE-A1 · A1.3).
//!
//! Every bridge domain except `config` is registered but NOT yet wired. Its
//! command returns an honest `Unavailable` error (Rule 1): in the real desktop
//! app an unwired domain says so — it never returns fabricated data. Each later
//! phase replaces one of these with a real implementation copying the config
//! round-trip shape.

/// The single honest "not wired yet" error, returned as `Err(String)` so the
/// TS side receives it on the `invoke` rejection path and maps it to the UI's
/// "coming"/seam state.
fn unavailable(domain: &str, op: &str) -> Result<serde_json::Value, String> {
    Err(format!(
        "unavailable: bridge domain \"{domain}\" op \"{op}\" is not wired in this build"
    ))
}

macro_rules! seam_cmd {
    ($name:ident, $domain:literal, $op:literal) => {
        #[tauri::command]
        pub fn $name() -> Result<serde_json::Value, String> {
            unavailable($domain, $op)
        }
    };
}

// NOTE: `auth_*` seam stubs were replaced by the real OIDC commands in CORE-A3
// (see `oidc.rs`). The `auth` bridge domain is now genuinely wired.
// NOTE: `node_*` seam stubs were replaced by the real citrate-node wiring in
// CORE-C1.1 (see `node.rs`). The `node` bridge domain is now genuinely wired.
// NOTE: `wallet_balances` was replaced by the REAL liquid (eth_getBalance) +
// claimable read in `earnings.rs`. `wallet_activity` remains a seam stub (on-chain
// tx history needs an indexer/explorer, not plain RPC — a later phase).
seam_cmd!(wallet_activity, "wallet", "activity");
seam_cmd!(memory_assert, "memory", "assert");
// NOTE: `memory_recall` was replaced by the real citrate-memories mcp_serve
// wiring in CORE-C3 (see `memory.rs`). recall/search/neighbors are genuinely
// wired; only the `assert` WRITE path remains a seam stub (it routes through the
// SignatureCeremony in a later WP).
seam_cmd!(chat_backend, "chat", "backend");
seam_cmd!(membership_entitlement, "membership", "entitlement");
seam_cmd!(commissary_catalog, "commissary", "catalog");
seam_cmd!(comms_connections, "comms", "connections");

#[cfg(test)]
mod tests {
    use super::*;

    /// An unwired seam domain returns an honest `unavailable:` error, never a
    /// value. This is the Rule-1 guarantee at the Rust boundary.
    #[test]
    fn seam_command_is_honestly_unavailable() {
        // wallet_activity is still a seam (on-chain history needs an indexer);
        // wallet_balances is now REAL (earnings.rs) so it's no longer here.
        let r = wallet_activity();
        assert!(r.is_err());
        let msg = r.unwrap_err();
        assert!(msg.starts_with("unavailable:"), "got: {msg}");
        assert!(msg.contains("wallet"));
    }

    /// Every seam command is unavailable — spot-check a few more so a future
    /// accidental "return fabricated data" regression is caught.
    #[test]
    fn all_seam_domains_report_unavailable() {
        for r in [
            wallet_activity(),
            commissary_catalog(),
            membership_entitlement(),
            comms_connections(),
        ] {
            assert!(r.is_err());
            assert!(r.unwrap_err().starts_with("unavailable:"));
        }
    }
}
