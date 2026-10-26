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
//! nothing unless `CITRATE_UPDATER_E2E` is set, the pinned key is not the release key, and every
//! endpoint is on loopback. How to run it: docs/UPDATER_E2E.md.

use serde_json::Value;

/// The shipping config: its updater public key is the one the driver must never run against.
const SHIPPING_CONF: &str = include_str!("../tauri.conf.json");

/// Whether this build's updater config (the `plugins.updater` object) is one the driver may act
/// on: a public key other than the release key, and at least one endpoint, all on loopback.
pub(crate) fn guard(_updater: &Value) -> Result<(), String> {
    Ok(())
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
