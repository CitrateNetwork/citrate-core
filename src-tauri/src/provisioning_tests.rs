// provisioning — `wallet_ensure_ready` core-flow tests.
//
// Exercise `ensure_ready_inner` over a real `CustodyVault` backed by an in-memory
// keyring (the `Keyring` trait + `CustodyVault::new` are public in kit), so the
// full provision → mint → read path runs headless. The device-passphrase
// init/unlock mechanics themselves are proven in kit's `custody_tests`; here we
// prove the wallet is genuinely provisioned + idempotent through this command's
// helper.

use super::*;
use crate::custody::{CustodyError, CustodyVault, Keyring};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// In-memory keyring fake (mirrors kit's headless test seam).
#[derive(Default)]
struct MemKeyring {
    store: Mutex<HashMap<String, Vec<u8>>>,
}
impl Keyring for MemKeyring {
    fn get(&self, account: &str) -> std::result::Result<Option<Vec<u8>>, CustodyError> {
        Ok(self.store.lock().unwrap().get(account).cloned())
    }
    fn set(&self, account: &str, secret: &[u8]) -> std::result::Result<(), CustodyError> {
        self.store
            .lock()
            .unwrap()
            .insert(account.to_string(), secret.to_vec());
        Ok(())
    }
    fn delete(&self, account: &str) -> std::result::Result<(), CustodyError> {
        self.store.lock().unwrap().remove(account);
        Ok(())
    }
}

/// A shared handle so two vault instances (modelling an app restart) can back onto
/// the SAME keyring.
struct SharedMem(Arc<MemKeyring>);
impl Keyring for SharedMem {
    fn get(&self, account: &str) -> std::result::Result<Option<Vec<u8>>, CustodyError> {
        self.0.get(account)
    }
    fn set(&self, account: &str, secret: &[u8]) -> std::result::Result<(), CustodyError> {
        self.0.set(account, secret)
    }
    fn delete(&self, account: &str) -> std::result::Result<(), CustodyError> {
        self.0.delete(account)
    }
}

fn unique_envelope_path() -> PathBuf {
    let mut p = std::env::temp_dir();
    let n = {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        N.fetch_add(1, Ordering::Relaxed)
    };
    p.push(format!(
        "citrate-core-provisioning-test-{}-{}.enc",
        std::process::id(),
        n
    ));
    let _ = std::fs::remove_file(&p);
    p
}

fn vault_over(keyring: Arc<MemKeyring>, path: PathBuf) -> CustodyVault {
    CustodyVault::new(Box::new(SharedMem(keyring)), path, 0)
}

fn is_evm_address(addr: &str) -> bool {
    let hex = addr.strip_prefix("0x").unwrap_or("");
    hex.len() == 40 && hex.chars().all(|c| c.is_ascii_hexdigit())
}

#[test]
fn ensure_ready_fresh_provisions_and_mints_wallet() {
    let v = vault_over(Arc::new(MemKeyring::default()), unique_envelope_path());
    let ready = ensure_ready_inner(&v).expect("fresh provision + mint succeeds");
    assert!(ready.created, "a fresh install mints a new wallet");
    assert!(
        is_evm_address(&ready.address),
        "returns a real 0x EVM address, got {}",
        ready.address
    );
    assert!(v.is_initialized() && v.is_unlocked(), "vault is provisioned");
}

#[test]
fn ensure_ready_is_idempotent_same_address() {
    let v = vault_over(Arc::new(MemKeyring::default()), unique_envelope_path());
    let first = ensure_ready_inner(&v).unwrap();
    let second = ensure_ready_inner(&v).expect("second call is a no-op read");
    assert!(first.created, "first call minted");
    assert!(!second.created, "second call found the existing wallet");
    assert_eq!(
        first.address, second.address,
        "the wallet address is stable across calls"
    );
}

#[test]
fn ensure_ready_survives_app_restart() {
    // Same keyring + envelope path across two instances models relaunching the app:
    // provisioning must recognise the existing wallet and return the SAME address.
    let keyring = Arc::new(MemKeyring::default());
    let path = unique_envelope_path();

    let v1 = vault_over(keyring.clone(), path.clone());
    let first = ensure_ready_inner(&v1).unwrap();
    assert!(first.created);
    drop(v1);

    let v2 = vault_over(keyring, path);
    let after = ensure_ready_inner(&v2).expect("restart provision succeeds");
    assert!(!after.created, "the wallet persisted across the restart");
    assert_eq!(first.address, after.address);
}

#[test]
fn ensure_ready_fails_closed_when_keychain_reset_loses_passphrase() {
    // Envelope survives but the device passphrase is gone (keychain partial reset):
    // provisioning must fail closed, never mint a new wallet over the stranded vault.
    let keyring = Arc::new(MemKeyring::default());
    let path = unique_envelope_path();
    let v1 = vault_over(keyring.clone(), path.clone());
    ensure_ready_inner(&v1).unwrap();
    drop(v1);

    // Drop only the auto-passphrase entry (leave master + envelope).
    keyring.delete("custody-auto-passphrase").unwrap();
    let v2 = vault_over(keyring, path);
    let err = ensure_ready_inner(&v2).expect_err("must fail closed on a lost passphrase");
    assert!(
        !err.is_empty(),
        "surfaces an honest error string, got empty"
    );
    assert!(!v2.is_unlocked(), "the vault was not silently re-provisioned");
}
