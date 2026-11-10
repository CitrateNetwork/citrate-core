//! Updater end to end (RC52 G8): the in-app driver for the K1-L `update-e2e` scenario.
//!
//! The packaged app's update flow is click-driven (src/shell/updater.ts: "Update available", then
//! Install, then Restart), and the Linux acceptance harness has no click driver. A test build made
//! with the `updater-e2e` cargo feature runs the same three plugin calls itself once at start-up:
//! `check()` (the JS `check`), `download_and_install()` (the JS `downloadAndInstall`, which verifies
//! the minisign signature against the pinned public key before anything is written) and
//! `request_restart()` (what the process plugin's `relaunch` calls). Each outcome is one
//! `updater-e2e:` line on stderr, which the harness reads.
//!
//! Only `guard` exists without the feature (for its unit tests). No release build enables the
//! feature (tripwire: upgrade_updater_feed_tests), and even in a test build the driver does
//! nothing unless `CITRATE_UPDATER_E2E` is set, the pinned key is neither release key (v1, v2,
//! compared by key id and key bytes), and every endpoint is on loopback. How to run it: docs/UPDATER_E2E.md.

use serde_json::Value;

/// The shipping config: its updater public key is one the driver must never run against.
const SHIPPING_CONF: &str = include_str!("../tauri.conf.json");

/// The release updater public keys (public material). v1, key id F6E74B821EAAEB32, was pinned
/// before v0.3.0 (installs older than that still trust it); v2, key id 8EC382DD107F08C5, since.
/// The shipping config's key is added at run time, so a future rotation is covered too.
const RELEASE_PUBKEYS: [&str; 2] = [
    "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IEY2RTc0QjgyMUVBQUVCMzIKUldReTY2b2Vna3ZuOXN2OG9kNG5NTHdsUHJaOEYyUkhKN0QrVDR3clJSaXNaY1ppMW9Xa3dSK3UK",
    "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDhFQzM4MkREMTA3RjA4QzUKUldURkNIOFEzWUxEanZsN096Vk1WWm9WMllUUE5GNXVUYVFYeGRQNFluRnVlVG5vWVZnV0F6eXAK",
];

/// The minisign Ed25519 public keys in a tauri updater `pubkey` value (base64 of the minisign
/// public key file): every line that decodes to `Ed` + 8-byte key id + 32-byte key, whatever the
/// comment line, whitespace or line endings around it.
fn minisign_keys(pubkey_b64: &str) -> Result<Vec<Vec<u8>>, String> {
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD;
    let raw = b64
        .decode(pubkey_b64.trim())
        .map_err(|e| format!("pubkey is not base64: {e}"))?;
    let text = String::from_utf8(raw).map_err(|_| "pubkey is not a minisign key file")?;
    let keys: Vec<Vec<u8>> = text
        .lines()
        .filter_map(|l| b64.decode(l.trim()).ok())
        .filter(|k| k.len() == 42 && k.starts_with(b"Ed"))
        .collect();
    if keys.is_empty() {
        return Err("no minisign public key in the pinned pubkey".into());
    }
    Ok(keys)
}

/// Whether this build's updater config (the `plugins.updater` object) is one the driver may act
/// on: a public key that is neither release key (by key id or key bytes), and at least one
/// endpoint, all on loopback.
pub(crate) fn guard(updater: &Value) -> Result<(), String> {
    let shipping: Value =
        serde_json::from_str(SHIPPING_CONF).map_err(|e| format!("tauri.conf.json: {e}"))?;
    let shipping_key = shipping["plugins"]["updater"]["pubkey"]
        .as_str()
        .ok_or("tauri.conf.json has no updater pubkey")?;
    let mut release = Vec::new();
    for k in RELEASE_PUBKEYS.iter().chain([&shipping_key]) {
        release.extend(minisign_keys(k).map_err(|e| format!("release key: {e}"))?);
    }
    let pinned = updater["pubkey"].as_str().unwrap_or_default();
    if pinned.trim().is_empty() {
        return Err("no updater pubkey in this build".into());
    }
    for k in minisign_keys(pinned)? {
        if release
            .iter()
            .any(|r| r[2..10] == k[2..10] || r[10..] == k[10..])
        {
            return Err("this build pins a release updater key".into());
        }
    }
    let endpoints = updater["endpoints"]
        .as_array()
        .filter(|e| !e.is_empty())
        .ok_or("no updater endpoints in this build")?;
    for e in endpoints {
        let raw = e.as_str().unwrap_or_default();
        let url = url::Url::parse(raw).map_err(|err| format!("endpoint {raw:?}: {err}"))?;
        let loopback = match url.host() {
            Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
            Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
            Some(url::Host::Domain(d)) => d == "localhost",
            None => false,
        };
        if !loopback {
            return Err(format!("endpoint {raw} is not on loopback"));
        }
    }
    Ok(())
}

/// Start the driver (setup hook). Returns at once; the check runs on the async runtime.
#[cfg(feature = "updater-e2e")]
pub(crate) fn start(app: &tauri::AppHandle) {
    use tauri_plugin_updater::UpdaterExt;

    if std::env::var_os("CITRATE_UPDATER_E2E").is_none() {
        return;
    }
    let version = app.package_info().version.to_string();
    eprintln!(
        "updater-e2e: running version={version} pid={}",
        std::process::id()
    );
    let config = app
        .config()
        .plugins
        .0
        .get("updater")
        .cloned()
        .unwrap_or(Value::Null);
    if let Err(e) = guard(&config) {
        eprintln!("updater-e2e: not run: {e}");
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let updater = match app.updater() {
            Ok(u) => u,
            Err(e) => {
                eprintln!("updater-e2e: check-error error={e}");
                return;
            }
        };
        match updater.check().await {
            Ok(None) => eprintln!("updater-e2e: no-update current={version}"),
            Ok(Some(update)) => {
                eprintln!("updater-e2e: available version={}", update.version);
                match update.download_and_install(|_, _| {}, || {}).await {
                    Ok(()) => {
                        eprintln!(
                            "updater-e2e: installed version={}; restarting",
                            update.version
                        );
                        app.request_restart();
                    }
                    Err(e) => {
                        eprintln!("updater-e2e: refused version={} error={e}", update.version)
                    }
                }
            }
            Err(e) => eprintln!("updater-e2e: check-error error={e}"),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::guard;
    use serde_json::json;

    const THROWAWAY: &str = include_str!("../../fixtures/updater-e2e/throwaway-a.pub");
    const LOOPBACK: &str = "http://127.0.0.1:18477/latest.json";

    fn release_key() -> String {
        let c: serde_json::Value = serde_json::from_str(super::SHIPPING_CONF)
            .unwrap_or_else(|e| panic!("tauri.conf.json: {e}"));
        c["plugins"]["updater"]["pubkey"]
            .as_str()
            .unwrap_or_else(|| panic!("no pubkey"))
            .to_string()
    }

    /// The v1 release updater public key (key id F6E74B821EAAEB32), pinned before v0.3.0
    /// (src-tauri/tauri.conf.json at 1b63e019^). Public material.
    const V1_PUBKEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IEY2RTc0QjgyMUVBQUVCMzIKUldReTY2b2Vna3ZuOXN2OG9kNG5NTHdsUHJaOEYyUkhKN0QrVDR3clJSaXNaY1ppMW9Xa3dSK3UK";

    fn b64(s: &str) -> String {
        use base64::Engine;
        let raw = base64::engine::general_purpose::STANDARD
            .decode(s.trim())
            .unwrap_or_else(|e| panic!("base64: {e}"));
        String::from_utf8(raw).unwrap_or_else(|e| panic!("utf8: {e}"))
    }

    fn enc(text: &str) -> String {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD.encode(text)
    }

    fn refused(pubkey: &str) -> String {
        let cfg = json!({ "pubkey": pubkey, "endpoints": [LOOPBACK] });
        guard(&cfg).expect_err("a release key was accepted")
    }

    #[test]
    fn the_driver_refuses_the_v1_release_key() {
        let err = refused(V1_PUBKEY);
        assert!(err.contains("release updater key"), "{err}");
    }

    #[test]
    fn the_driver_refuses_the_v2_release_key_under_another_comment() {
        let text = b64(&release_key());
        let key_line = text.lines().nth(1).unwrap_or_else(|| panic!("no key line"));
        for rewritten in [
            format!("untrusted comment: test build key\n{key_line}\n"),
            format!("untrusted comment: minisign public key: 0000000000000000\r\n{key_line}\r\n"),
            format!("untrusted comment: x\n  {key_line}  \n"),
        ] {
            let err = refused(&enc(&rewritten));
            assert!(err.contains("release updater key"), "{rewritten:?}: {err}");
        }
    }

    #[test]
    fn the_driver_refuses_the_v2_release_key_without_a_trailing_newline() {
        let text = b64(&release_key());
        let err = refused(&enc(text.trim_end()));
        assert!(err.contains("release updater key"), "{err}");
    }

    #[test]
    fn the_driver_refuses_a_pubkey_it_cannot_decode() {
        for bad in [
            "not base64 at all",
            &enc("untrusted comment: no key line\n"),
        ] {
            let cfg = json!({ "pubkey": bad, "endpoints": [LOOPBACK] });
            assert!(guard(&cfg).is_err(), "{bad}");
        }
    }

    #[test]
    fn the_driver_runs_with_a_throwaway_key_and_a_loopback_feed() {
        for ep in [
            LOOPBACK,
            "http://localhost:9/latest.json",
            "http://[::1]:9/x",
        ] {
            let cfg = json!({ "pubkey": THROWAWAY.trim(), "endpoints": [ep] });
            assert_eq!(guard(&cfg), Ok(()), "{ep}");
        }
    }

    #[test]
    fn the_driver_refuses_a_build_that_pins_the_release_key() {
        let cfg = json!({ "pubkey": release_key(), "endpoints": [LOOPBACK] });
        let err = guard(&cfg).expect_err("release key accepted");
        assert!(err.contains("release updater key"), "{err}");
        // Surrounding whitespace does not hide it.
        let cfg = json!({ "pubkey": format!("{}\n", release_key()), "endpoints": [LOOPBACK] });
        assert!(guard(&cfg).is_err());
    }

    #[test]
    fn the_driver_refuses_a_feed_off_loopback() {
        for ep in [
            "https://citrate-cdn.nyc3.cdn.digitaloceanspaces.com/downloads/updater/latest.json",
            "https://github.com/CitrateNetwork/citrate-core/releases/latest/download/latest.json",
            "http://10.0.0.5:18477/latest.json",
            "http://127.0.0.1.example.com/latest.json",
        ] {
            let cfg = json!({ "pubkey": THROWAWAY.trim(), "endpoints": [LOOPBACK, ep] });
            let err = guard(&cfg).expect_err(ep);
            assert!(err.contains("not on loopback"), "{ep}: {err}");
        }
    }

    #[test]
    fn the_driver_refuses_a_config_without_a_key_or_endpoints() {
        for cfg in [
            json!({ "endpoints": [LOOPBACK] }),
            json!({ "pubkey": "  ", "endpoints": [LOOPBACK] }),
            json!({ "pubkey": THROWAWAY.trim() }),
            json!({ "pubkey": THROWAWAY.trim(), "endpoints": [] }),
            json!({ "pubkey": THROWAWAY.trim(), "endpoints": ["not a url"] }),
            serde_json::Value::Null,
        ] {
            assert!(guard(&cfg).is_err(), "{cfg}");
        }
    }
}
