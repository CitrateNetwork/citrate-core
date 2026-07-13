//! citrate-core — the federation desktop house (Tauri full-node app).
//!
//! CORE-A1 bridge: the `config` domain is wired end-to-end for real (persisted
//! on-disk via tauri-plugin-store + OS-keyring status). Every other bridge
//! domain is registered but returns an honest `Unavailable` error — never
//! fabricated data (Rule 1). Chain reads happen in the webview via viem/wagmi
//! (D-13); signing routes through the Rust SignatureCeremony once it exists
//! (CORE-S2).

mod ceremony;
mod config;
mod custody;
mod oidc;
mod seam;
mod wallet;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .setup(|app| {
            // CORE-A2 — build the process-wide custody vault (real OS keyring +
            // app-data envelope), seeded with the persisted config.autolock (the
            // A1 single source of truth). @rule8: no secret bytes cross invoke.
            let handle = app.handle();
            let autolock = config::config_read(handle.clone())
                .map(|c| c.autolock)
                .unwrap_or_else(|_| config::AppConfig::default().autolock);
            let state = custody::build_custody_state(handle, autolock)
                .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
            app.manage(state);
            // CORE-A3 — the auth manager (real loopback-PKCE OIDC against
            // auth.citrate.ai; refresh token → the A2 vault; access token in
            // memory only; @rule8: no token crosses invoke). Consumes A2.
            app.manage(oidc::build_auth_state());
            // CORE-B1.2 — the SignatureCeremony: the SINGLE human-in-the-loop
            // signing path. Every signature intent (user or, later, agent /
            // micro-app) routes through one approval surface; the gated
            // `wallet::sign_message` is reachable ONLY from `approve`. @rule8:
            // this LIFTS Rule 3 — all signing goes through this ceremony. No
            // secret bytes cross invoke (sign_* return id / decoded / sig-hex).
            app.manage(ceremony::build_ceremony_state());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // config — the one genuinely-live domain (A1.4)
            config::config_read,
            config::config_write,
            config::config_keyring_status,
            // custody — the OS-keyring vault (A2). NOTE: custody_get is NOT here;
            // it is an in-process pub fn — no invoke command returns secret bytes.
            custody::custody_status,
            custody::custody_init,
            custody::custody_unlock,
            custody::custody_lock,
            custody::custody_put,
            custody::custody_list,
            custody::custody_keyring_status,
            // auth — real OIDC loopback-PKCE (A3). Replaces the A1 auth seam
            // stubs. Every command returns claim-derived AuthStatus or (); NO
            // command returns a token (ADV-8 boundary; see oidc::tests).
            oidc::auth_status,
            oidc::auth_login,
            oidc::auth_userinfo,
            oidc::auth_refresh,
            oidc::auth_logout,
            oidc::kyc_start,
            // signing — the B1.2 SignatureCeremony (the ONE HITL signing path).
            // sign_request returns a CeremonyId + decoded intent (NO signature);
            // sign_approve returns the signature hex ONLY (never key/seed/entropy
            // — I-2 compile barrier holds); sign_reject consumes with no sig. The
            // gated wallet::sign_message is reachable ONLY via approve (@rule8).
            ceremony::sign_request,
            ceremony::sign_approve,
            ceremony::sign_reject,
            // seam domains — honest Unavailable until each later phase (A1.3)
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

    /// ADV-8 boundary, asserted structurally against the real source: the
    /// invoke_handler registers exactly the seven metadata/status custody
    /// commands and NEVER `custody_get` (the in-process secret-reading pub fn).
    /// No invoke command returns secret bytes.
    #[test]
    fn no_custody_invoke_command_returns_secret_bytes() {
        let src = include_str!("lib.rs");
        // The registered custody commands (all return status / () / metadata).
        for cmd in [
            "custody::custody_status",
            "custody::custody_init",
            "custody::custody_unlock",
            "custody::custody_lock",
            "custody::custody_put",
            "custody::custody_list",
            "custody::custody_keyring_status",
        ] {
            assert!(src.contains(cmd), "custody command not registered: {cmd}");
        }
        // The secret-reading API must NEVER be registered as an invoke command.
        // Registration would take the form `<mod>::<fn>,` inside the
        // generate_handler![...] list. The needle is assembled from parts so
        // this test's own prose (which names the fn) cannot trip the check; the
        // only way it matches is a genuine handler-registration line.
        let getter = "custody_g".to_string() + "et";
        let needle = format!("custody::{getter},");
        assert!(
            !src.contains(&needle),
            "the in-process secret getter must not be an invoke command (ADV-8 boundary)"
        );
    }
}
