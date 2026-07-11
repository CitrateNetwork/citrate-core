//! citrate-core — the federation desktop house (Tauri full-node app).
//!
//! CORE-S0 scaffold: no Tauri commands are registered yet. Chain reads happen
//! in the webview via viem/wagmi (D-13); signing will route through the Rust
//! SignatureCeremony once it exists (CORE-S2). No mocked data anywhere (Rule 1).

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
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
