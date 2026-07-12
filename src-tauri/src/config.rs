//! citrate-core — config domain (CORE-A1 · A1.4).
//!
//! The ONE genuinely-live bridge domain in A1: app config persisted to a real
//! on-disk Tauri store (`config.json` in the app data dir), plus an OS-keyring
//! status probe. Every later bridge domain copies this exact round-trip shape.
//!
//! Rule 1: no fabricated data. `config_read` returns real persisted values (or
//! documented defaults on first run); `config_keyring_status` reports the true
//! platform result of a keyring probe.

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Runtime};
use tauri_plugin_store::StoreExt;

/// Persisted app config. Field names mirror the TS `AppConfig` and the
/// AppState fields the Settings surface reads/writes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AppConfig {
    pub net: String,
    pub rpc: String,
    #[serde(rename = "dataDir")]
    pub data_dir: String,
    #[serde(rename = "cpuCap")]
    pub cpu_cap: u32,
    pub autolock: u32,
    pub channel: String,
    pub telemetry: bool,
    #[serde(rename = "sigPolicy")]
    pub sig_policy: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        AppConfig {
            net: "testnet".into(),
            rpc: "local".into(),
            data_dir: "~/.citrate/core".into(),
            cpu_cap: 50,
            autolock: 30,
            channel: "stable".into(),
            telemetry: false,
            sig_policy: "hitl".into(),
        }
    }
}

/// A sparse patch — only the fields the caller is changing. Matches
/// `Partial<AppConfig>` on the TS side.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct AppConfigPatch {
    pub net: Option<String>,
    pub rpc: Option<String>,
    #[serde(rename = "dataDir")]
    pub data_dir: Option<String>,
    #[serde(rename = "cpuCap")]
    pub cpu_cap: Option<u32>,
    pub autolock: Option<u32>,
    pub channel: Option<String>,
    pub telemetry: Option<bool>,
    #[serde(rename = "sigPolicy")]
    pub sig_policy: Option<String>,
}

impl AppConfig {
    /// Apply a sparse patch, returning the merged config.
    pub fn apply(mut self, p: AppConfigPatch) -> Self {
        if let Some(v) = p.net {
            self.net = v;
        }
        if let Some(v) = p.rpc {
            self.rpc = v;
        }
        if let Some(v) = p.data_dir {
            self.data_dir = v;
        }
        if let Some(v) = p.cpu_cap {
            self.cpu_cap = v;
        }
        if let Some(v) = p.autolock {
            self.autolock = v;
        }
        if let Some(v) = p.channel {
            self.channel = v;
        }
        if let Some(v) = p.telemetry {
            self.telemetry = v;
        }
        if let Some(v) = p.sig_policy {
            self.sig_policy = v;
        }
        self
    }
}

/// Store file name inside the app data dir. This is the real on-disk file the
/// round-trip proof reads back after restart.
const STORE_FILE: &str = "config.json";
const CONFIG_KEY: &str = "app";

/// Load the persisted config from the on-disk store, or defaults on first run.
fn load_config<R: Runtime>(app: &AppHandle<R>) -> Result<AppConfig, String> {
    let store = app.store(STORE_FILE).map_err(|e| e.to_string())?;
    match store.get(CONFIG_KEY) {
        Some(v) => serde_json::from_value(v).map_err(|e| e.to_string()),
        None => Ok(AppConfig::default()),
    }
}

/// Persist the config to the on-disk store (flushed immediately).
fn save_config<R: Runtime>(app: &AppHandle<R>, cfg: &AppConfig) -> Result<(), String> {
    let store = app.store(STORE_FILE).map_err(|e| e.to_string())?;
    let val = serde_json::to_value(cfg).map_err(|e| e.to_string())?;
    store.set(CONFIG_KEY, val);
    store.save().map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn config_read<R: Runtime>(app: AppHandle<R>) -> Result<AppConfig, String> {
    load_config(&app)
}

#[tauri::command]
pub fn config_write<R: Runtime>(
    app: AppHandle<R>,
    patch: AppConfigPatch,
) -> Result<AppConfig, String> {
    let merged = load_config(&app)?.apply(patch);
    save_config(&app, &merged)?;
    Ok(merged)
}

/// OS-keyring availability. Probes the real platform keyring with a benign
/// read of a well-known probe entry. "available" means the keyring backend is
/// reachable; "unavailable" means it is not (e.g. no secret service on a
/// headless Linux box). Never fabricated.
#[tauri::command]
pub fn config_keyring_status() -> String {
    match keyring::Entry::new("ai.citrate.core", "keyring-probe") {
        Ok(entry) => match entry.get_password() {
            // Reachable and either has or lacks the probe entry — both mean the
            // backend is available.
            Ok(_) => "available".into(),
            Err(keyring::Error::NoEntry) => "available".into(),
            // Any other error means the backend could not be reached.
            Err(_) => "unavailable".into(),
        },
        Err(_) => "unavailable".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Defaults must match the TS `DEFAULT_APP_CONFIG` contract exactly.
    #[test]
    fn default_config_matches_contract() {
        let d = AppConfig::default();
        assert_eq!(d.net, "testnet");
        assert_eq!(d.rpc, "local");
        assert_eq!(d.data_dir, "~/.citrate/core");
        assert_eq!(d.cpu_cap, 50);
        assert_eq!(d.autolock, 30);
        assert_eq!(d.channel, "stable");
        assert!(!d.telemetry);
        assert_eq!(d.sig_policy, "hitl");
    }

    /// A sparse patch merges over defaults — the round-trip merge logic the
    /// `config_write` command relies on (A1.4). Proven without a live app
    /// handle so it runs headless.
    #[test]
    fn patch_merges_over_defaults() {
        let patch = AppConfigPatch {
            net: Some("local".into()),
            telemetry: Some(true),
            autolock: Some(15),
            ..Default::default()
        };
        let merged = AppConfig::default().apply(patch);
        assert_eq!(merged.net, "local");
        assert!(merged.telemetry);
        assert_eq!(merged.autolock, 15);
        // untouched fields keep their defaults
        assert_eq!(merged.rpc, "local");
        assert_eq!(merged.channel, "stable");
        assert_eq!(merged.sig_policy, "hitl");
    }

    /// The config JSON must (de)serialize with the camelCase keys the TS bridge
    /// sends/expects — this is the wire contract across the invoke boundary.
    #[test]
    fn config_serde_uses_camelcase_wire_keys() {
        let cfg = AppConfig::default();
        let json = serde_json::to_value(&cfg).unwrap();
        assert!(json.get("dataDir").is_some());
        assert!(json.get("cpuCap").is_some());
        assert!(json.get("sigPolicy").is_some());
        // round-trips back to an equal value
        let back: AppConfig = serde_json::from_value(json).unwrap();
        assert_eq!(back, cfg);
    }
}
