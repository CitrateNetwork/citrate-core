//! 0.5.1 -> 0.5.2 upgrade: the wallet vault and the app config (UPGRADE_051_052 §2, §3).
//!
//! The fixtures were written by the v0.5.1 kit itself (b521d98e): `CustodyVault::new` +
//! `ensure_auto_unlocked` + `wallet::import` + `put`, through an in-memory keyring whose entries
//! ship next to the envelope (fixtures/upgrade-v0.5.1/custody.keyring.json). The mnemonic is the
//! public Hardhat/Foundry test seed. This build must open that vault and derive the same address.

use crate::custody::{CustodyError, CustodyVault, Keyring};
use base64::Engine;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

type Result<T> = std::result::Result<T, CustodyError>;

const ENVELOPE: &[u8] = include_bytes!("../../fixtures/upgrade-v0.5.1/app-data/custody.enc");
const KEYRING: &str = include_str!("../../fixtures/upgrade-v0.5.1/custody.keyring.json");
const CONFIG: &str = include_str!("../../fixtures/upgrade-v0.5.1/app-data/config.json");

#[derive(Clone, Default)]
struct MapKeyring(Arc<Mutex<BTreeMap<String, Vec<u8>>>>);
impl Keyring for MapKeyring {
    fn get(&self, account: &str) -> Result<Option<Vec<u8>>> {
        Ok(self
            .0
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(account)
            .cloned())
    }
    fn set(&self, account: &str, secret: &[u8]) -> Result<()> {
        self.0
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(account.to_string(), secret.to_vec());
        Ok(())
    }
    fn delete(&self, account: &str) -> Result<()> {
        self.0
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(account);
        Ok(())
    }
}

fn meta() -> serde_json::Value {
    serde_json::from_str(KEYRING).unwrap_or_else(|e| panic!("keyring fixture: {e}"))
}

/// The 0.5.1 keyring, as the OS keyring holds it after the upgrade (the app keeps its bundle id,
/// so the same service and accounts).
fn keyring_v051() -> MapKeyring {
    let kr = MapKeyring::default();
    let m = meta();
    let entries = m["keyring"]
        .as_object()
        .unwrap_or_else(|| panic!("no keyring entries in the fixture"));
    for (account, v) in entries {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(v.as_str().unwrap_or_default())
            .unwrap_or_else(|e| panic!("{account}: {e}"));
        kr.0.lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(account.clone(), bytes);
    }
    kr
}

fn data_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("citrate-upgrade-v051-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap_or_else(|e| panic!("mkdir: {e}"));
    std::fs::write(d.join("custody.enc"), ENVELOPE).unwrap_or_else(|e| panic!("write: {e}"));
    d
}

#[test]
fn a_v051_vault_opens_and_derives_the_same_wallet() {
    let dir = data_dir("vault");
    let vault = CustodyVault::new(Box::new(keyring_v051()), dir.join("custody.enc"), 30);
    assert!(vault.is_initialized());
    vault
        .ensure_auto_unlocked()
        .unwrap_or_else(|e| panic!("the 0.5.1 vault must open with its device passphrase: {e}"));
    let info = crate::wallet::address(&vault).unwrap_or_else(|e| panic!("address: {e}"));
    assert_eq!(
        Some(info.address.as_str()),
        meta()["walletAddress"].as_str()
    );
    assert_eq!(
        Some(info.public_key_hex.as_str()),
        meta()["walletPublicKeyHex"].as_str()
    );
    // The backend-written connection record 0.5.1 sealed is still there, byte for byte.
    let conn = vault
        .custody_get("connection-github")
        .unwrap_or_else(|e| panic!("connection slot: {e}"));
    let rec: serde_json::Value =
        serde_json::from_slice(&conn).unwrap_or_else(|e| panic!("connection json: {e}"));
    assert_eq!(rec["scope"], "read");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_v051_vault_with_its_device_passphrase_gone_fails_closed_and_is_not_reissued() {
    // A keychain reset during the upgrade must never look like a fresh install: minting a new
    // passphrase over the 0.5.1 envelope would strand a funded wallet.
    let dir = data_dir("nopass");
    let kr = keyring_v051();
    kr.0.lock()
        .unwrap_or_else(|p| p.into_inner())
        .remove("custody-auto-passphrase");
    let vault = CustodyVault::new(Box::new(kr.clone()), dir.join("custody.enc"), 30);
    match vault.ensure_auto_unlocked() {
        Err(CustodyError::KeyringUnavailable) => {}
        other => panic!("expected KeyringUnavailable, got {other:?}"),
    }
    assert!(kr
        .0
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .get("custody-auto-passphrase")
        .is_none());
    assert_eq!(
        std::fs::read(dir.join("custody.enc")).unwrap_or_default(),
        ENVELOPE,
        "the 0.5.1 envelope is left as it was"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_v051_config_json_loads_with_every_setting_kept() {
    // tauri-plugin-store keeps the config under the "app" key (config.rs CONFIG_KEY).
    let store: serde_json::Value =
        serde_json::from_str(CONFIG).unwrap_or_else(|e| panic!("config.json: {e}"));
    let cfg: crate::config::AppConfig = serde_json::from_value(store["app"].clone())
        .unwrap_or_else(|e| panic!("a 0.5.1 config.json must load: {e}"));
    assert_eq!(cfg.autolock, 15);
    assert!(cfg.telemetry);
    assert_eq!(cfg.net, "testnet");
    assert_eq!(cfg.rpc, "local");
    assert_eq!(cfg.data_dir, "~/.citrate/core");
    assert_eq!(cfg.cpu_cap, 50);
    assert_eq!(cfg.channel, "stable");
    assert_eq!(cfg.core_membership_url, "https://membership.citrate.ai");
}
