//! citrate-core — seamless wallet provisioning (`wallet_ensure_ready`).
//!
//! The runtime bridge that closes the onboarding "Wallet link unavailable" bug.
//! Before this, `custody_init`/`unlock` and `wallet::create` ran only in tests, so
//! a fresh install had an uninitialized, LOCKED vault and NO wallet — every wallet
//! read (balances / address / link) failed closed, and the membership grant fell
//! back to a derived placeholder address instead of the user's real EOA.
//!
//! `wallet_ensure_ready` makes provisioning real and idempotent: it device-
//! provisions the custody vault (auto passphrase in the OS keyring — the owner-
//! approved no-passphrase model, security boundary = the OS login) and mints the
//! wallet silently on first run, returning the wallet's PUBLIC address. The
//! onboarding flow calls it after sign-in and before the membership grant, so the
//! wallet-link + grant bind the REAL device wallet, not a placeholder.
//!
//! HONESTY (Rule 1) / @rule8: returns ONLY the public address. No key / seed /
//! entropy ever crosses this boundary — `wallet::create`/`address` derive
//! in-process and zeroize. It signs NOTHING (Rule 3 untouched). For beta the
//! one-time BIP39 mnemonic from `create` is intentionally dropped (device-bound
//! recovery only — an explicit user-facing backup flow is a later WP).

use serde::Serialize;

/// The non-secret result of provisioning: the wallet's public EVM address, and
/// whether this call newly minted the wallet. Address only — never key material.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct WalletReady {
    pub address: String,
    /// `true` if THIS call minted the wallet; `false` if it already existed.
    pub created: bool,
}

/// Testable core of [`wallet_ensure_ready`], over a plain `&CustodyVault` so it
/// runs headless without a Tauri runtime. Idempotently device-provisions the
/// vault, then ensures the wallet exists, returning its public address.
fn ensure_ready_inner(
    vault: &crate::custody::CustodyVault,
) -> std::result::Result<WalletReady, String> {
    // 1. Seamless device-bound provisioning: init + unlock the vault under the
    //    keyring-held device passphrase (no user passphrase). Fails closed on an
    //    unreachable / reset keychain rather than clobbering an existing vault.
    vault.ensure_auto_unlocked().map_err(|e| e.to_string())?;

    // 2. Ensure a wallet exists. `address` returns the existing wallet's public
    //    identity; `NotFound` (unlocked, empty slot) means mint one now. Both
    //    require the unlocked session established above.
    match crate::wallet::address(vault) {
        Ok(info) => Ok(WalletReady {
            address: info.address,
            created: false,
        }),
        Err(crate::wallet::WalletError::NotFound) => {
            // Mint the wallet silently. The one-time mnemonic is dropped here for
            // beta (device-bound recovery only); `WalletCreate` zeroizes it on drop.
            let created = crate::wallet::create(vault).map_err(|e| e.to_string())?;
            Ok(WalletReady {
                address: created.address,
                created: true,
            })
            // `created.mnemonic` (Zeroizing) wipes on drop here.
        }
        Err(e) => Err(e.to_string()),
    }
}

/// `wallet_ensure_ready` — idempotently device-provision the custody vault and
/// ensure the wallet exists, returning its public address. Safe to call on every
/// launch: an already-provisioned, already-unlocked device short-circuits to a
/// cheap read. Registered in `lib.rs`'s invoke handler.
#[tauri::command]
pub fn wallet_ensure_ready(
    custody: tauri::State<'_, crate::custody::CustodyState>,
) -> std::result::Result<WalletReady, String> {
    ensure_ready_inner(&custody.0)
}

#[cfg(test)]
mod tests {
    include!("provisioning_tests.rs");
}
