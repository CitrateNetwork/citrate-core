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

seam_cmd!(auth_userinfo, "auth", "userinfo");
seam_cmd!(auth_sign_out, "auth", "signOut");
seam_cmd!(wallet_balances, "wallet", "balances");
seam_cmd!(wallet_activity, "wallet", "activity");
seam_cmd!(node_status, "node", "status");
seam_cmd!(node_start, "node", "start");
seam_cmd!(node_stop, "node", "stop");
seam_cmd!(memory_assert, "memory", "assert");
seam_cmd!(memory_recall, "memory", "recall");
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
        let r = wallet_balances();
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
            auth_userinfo(),
            node_status(),
            memory_recall(),
            membership_entitlement(),
            comms_connections(),
        ] {
            assert!(r.is_err());
            assert!(r.unwrap_err().starts_with("unavailable:"));
        }
    }
}
