//! citrate-core — the federation desktop house (Tauri full-node app).
//!
//! CORE-A1 bridge: the `config` domain is wired end-to-end for real (persisted
//! on-disk via tauri-plugin-store + OS-keyring status). Every other bridge
//! domain is registered but returns an honest `Unavailable` error — never
//! fabricated data (Rule 1). Chain reads happen in the webview via viem/wagmi
//! (D-13); signing routes through the Rust SignatureCeremony once it exists
//! (CORE-S2).

mod config;
mod seam;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            // config — the one genuinely-live domain (A1.4)
            config::config_read,
            config::config_write,
            config::config_keyring_status,
            // seam domains — honest Unavailable until each later phase (A1.3)
            seam::auth_userinfo,
            seam::auth_sign_out,
            seam::wallet_balances,
            seam::wallet_activity,
            seam::node_status,
            seam::node_start,
            seam::node_stop,
            seam::memory_assert,
            seam::memory_recall,
            seam::chat_backend,
            seam::membership_entitlement,
            seam::commissary_catalog,
            seam::comms_connections,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    /// The Tauri config must parse and carry the citrate-core identity.
    /// Guards against template cruft ("cta-citrate-core") reappearing.
    #[test]
    fn tauri_conf_parses_with_citrate_identity() {
        let conf: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json"))
            .expect("tauri.conf.json must be valid JSON");
        assert_eq!(conf["identifier"], "ai.citrate.core");
        assert_eq!(conf["productName"], "Citrate Core");
    }

    /// tauri.conf.json version and the Cargo package version must not drift.
    #[test]
    fn tauri_conf_version_matches_cargo_version() {
        let conf: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json"))
            .expect("tauri.conf.json must be valid JSON");
        assert_eq!(conf["version"], env!("CARGO_PKG_VERSION"));
    }
}
