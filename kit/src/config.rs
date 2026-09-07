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
    /// CORE-D3.C — the core-membership base URL. The S3 onboarding checkout
    /// opens `{core_membership_url}/checkout` in an in-app popup (the money +
    /// entitlement grant happen server-side). Overridable to a preview/prod
    /// domain. `#[serde(default)]` so a config.json persisted BEFORE this field
    /// existed still deserializes (it takes the prod default) — otherwise a
    /// pre-D3.C on-disk store would fail `config_read`.
    #[serde(rename = "coreMembershipUrl", default = "default_core_membership_url")]
    pub core_membership_url: String,
}

/// The prod core-membership base URL default (also the serde backfill for a
/// pre-D3.C persisted config.json that lacks the field).
fn default_core_membership_url() -> String {
    "https://core-membership.vercel.app".into()
}

/// CORE-B-004 — validate a `coreMembershipUrl` before it is persisted and, later,
/// loaded into an app-branded in-app checkout window titled "Citrate Membership".
///
/// The value drives the money step of the onboarding path (the popup where the
/// member enters card details), so an unvalidated field let the renderer (or any
/// same-user process writing `config.json`) point that window at attacker content
/// PERSISTENTLY. We require:
///   * the `https` scheme (never `http`/`file`/`javascript:` — mirrors
///     `shell::open_external` and `ai::validate_https_base_url`), and
///   * a host on the core-membership allowlist: exactly `core-membership.vercel.app`
///     (prod) OR a `core-membership-*.vercel.app` Vercel preview deploy.
///
/// Any other value is rejected so `apply` keeps the previous (trusted) value
/// rather than persisting a hostile one. The OIDC authority uses the same
/// pin-don't-trust discipline (`oidc::AuthorityConfig::production`).
pub(crate) fn is_valid_core_membership_url(candidate: &str) -> bool {
    let Ok(parsed) = url::Url::parse(candidate) else {
        return false;
    };
    if parsed.scheme() != "https" {
        return false;
    }
    match parsed.host_str() {
        Some(host) => {
            host == "core-membership.vercel.app"
                || (host.starts_with("core-membership-") && host.ends_with(".vercel.app"))
        }
        None => false,
    }
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
            core_membership_url: default_core_membership_url(),
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
    #[serde(rename = "coreMembershipUrl")]
    pub core_membership_url: Option<String>,
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
        if let Some(v) = p.core_membership_url {
            // CORE-B-004: only accept an https URL on the core-membership allowlist.
            // A rejected value leaves the previous (trusted) URL in place rather
            // than persisting attacker-controlled checkout content.
            if is_valid_core_membership_url(&v) {
                self.core_membership_url = v;
            }
        }
        self
    }

    /// CORE-D3.C — the full S3 checkout URL (`{core_membership_url}/checkout`)
    /// the popup navigates to. Trims a trailing slash on the base so we never
    /// emit a doubled `//checkout`.
    pub fn checkout_url(&self) -> String {
        format!(
            "{}/checkout",
            self.core_membership_url.trim_end_matches('/')
        )
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
        // CORE-D3.C — the core-membership base URL default (prod).
        assert_eq!(d.core_membership_url, "https://core-membership.vercel.app");
    }

    /// CORE-D3.C — the checkout URL is `{coreMembershipUrl}/checkout`, and an
    /// override base (e.g. a preview domain, trailing slash tolerated) is
    /// honored without doubling the slash.
    #[test]
    fn checkout_url_derives_from_base_and_is_overridable() {
        assert_eq!(
            AppConfig::default().checkout_url(),
            "https://core-membership.vercel.app/checkout"
        );
        let preview = AppConfig::default().apply(AppConfigPatch {
            core_membership_url: Some("https://core-membership-preview.vercel.app/".into()),
            ..Default::default()
        });
        // Trailing slash on the base does not produce a doubled `//checkout`.
        assert_eq!(
            preview.checkout_url(),
            "https://core-membership-preview.vercel.app/checkout"
        );
    }

    /// CORE-D3.C — a config.json persisted BEFORE this field existed (no
    /// `coreMembershipUrl` key) must still deserialize, backfilling the prod
    /// default. Otherwise `config_read` would fail on an existing user's store.
    #[test]
    fn pre_d3c_config_backfills_core_membership_url() {
        let legacy = serde_json::json!({
            "net": "testnet",
            "rpc": "local",
            "dataDir": "~/.citrate/core",
            "cpuCap": 50,
            "autolock": 30,
            "channel": "stable",
            "telemetry": false,
            "sigPolicy": "hitl"
        });
        let cfg: AppConfig = serde_json::from_value(legacy).unwrap();
        assert_eq!(
            cfg.core_membership_url,
            "https://core-membership.vercel.app"
        );
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
        assert!(json.get("coreMembershipUrl").is_some());
        // round-trips back to an equal value
        let back: AppConfig = serde_json::from_value(json).unwrap();
        assert_eq!(back, cfg);
    }

    /// CORE-B-004 tripwire: a hostile `coreMembershipUrl` patch (non-https scheme,
    /// or an off-allowlist host) must be REJECTED — `apply` keeps the previous
    /// trusted value rather than persisting attacker checkout content. Before the
    /// fix, `apply` assigned the field unchecked and these would all take effect.
    #[test]
    fn hostile_core_membership_url_is_rejected_and_previous_kept() {
        let base = AppConfig::default();
        let prev = base.core_membership_url.clone();
        for hostile in [
            "http://core-membership.vercel.app",             // cleartext
            "https://core-membershlp.example",               // look-alike host
            "https://evil.example/checkout",                 // arbitrary host
            "file:///etc/passwd",                            // file scheme
            "javascript:alert(1)",                           // js scheme
            "https://attacker.core-membership.vercel.app.evil.com", // suffix trick
            "not a url",
        ] {
            let merged = base.clone().apply(AppConfigPatch {
                core_membership_url: Some(hostile.to_string()),
                ..Default::default()
            });
            assert_eq!(
                merged.core_membership_url, prev,
                "hostile url {hostile:?} must be rejected, previous value kept"
            );
        }
    }

    /// CORE-B-004: legitimate prod + Vercel preview hosts are still accepted, and
    /// the derived checkout URL always starts with `https://`.
    #[test]
    fn allowlisted_core_membership_urls_are_accepted_and_checkout_is_https() {
        for ok in [
            "https://core-membership.vercel.app",
            "https://core-membership-preview.vercel.app",
            "https://core-membership-git-main-citrate.vercel.app",
        ] {
            let merged = AppConfig::default().apply(AppConfigPatch {
                core_membership_url: Some(ok.to_string()),
                ..Default::default()
            });
            assert_eq!(merged.core_membership_url, ok, "allowlisted url {ok:?} accepted");
            assert!(
                merged.checkout_url().starts_with("https://"),
                "checkout url must always be https"
            );
        }
    }
}
