//! The wallet key module (K6): the only code that can read the sealed wallet entropy.
//!
//! Declared as the private child `wallet::key`. Raw key material does not leave it:
//! - [`read_entropy`] and [`derive_key_from_entropy`] are private to this module, so nothing else
//!   in the crate can name them (the compiler enforces it, aliases included).
//! - The vault serves a `wallet-` slot only through `CustodyVault::wallet_slot_get`, which takes a
//!   [`WalletKeyAccess`]. Its field is private here, so only this module can make one. Plain
//!   `custody_get` refuses every `wallet-` slot, so knowing the slot name is not enough either.
//!
//! What it exports to `wallet` (and nothing wider):
//! - [`signing_key`]: the ONE way to get a signing key. It runs the network gate first.
//! - [`stored_info`]: the stored wallet's address and public key, no signing key.
//! - [`scoped_secret`]: an HKDF-SHA256 output over the entropy, no signing key.
//! - [`refuse_if_present`]: whether a wallet is already stored, for create/import.
//!
//! `wallet_tests::wallet_key_material_is_reachable_only_through_the_key_module` scans kit/ and
//! src-tauri/ as a lexical backstop for the same rule.

use citrate_wallet_core::{secp256k1_from_mnemonic, UnifiedKey};
use zeroize::Zeroizing;

use super::{info_of, Result, WalletError, WalletInfo, DEFAULT_ACCOUNT_INDEX, WALLET_ENTROPY_SLOT};
use crate::custody::{CustodyError, CustodyVault};
use crate::rpc::SignatureScope;

/// The capability `CustodyVault::wallet_slot_get` requires. The private field means only this
/// module can construct one; it is neither `Clone` nor `Copy`, and it is consumed by each read.
pub(crate) struct WalletKeyAccess(());

/// HKDF salt (domain) for [`scoped_secret`]. Fixed forever: changing it re-derives every scoped
/// secret (a different comms identity), so it is a hard versioning boundary.
const SCOPED_SECRET_SALT: &[u8] = b"citrate/wallet/scoped-secret/v1";

/// **The one way a signer gets the wallet key (K6).** Checks the network gate first
/// ([`crate::rpc::check_wallet_signing`]: refuses when no network is set, and a chain-committing
/// `scope` must be the target chain), and only then reads the sealed entropy and derives the key.
/// A new signer (EIP-712, say, with `SignatureScope::Chain(domain.chainId)`) has no other way to
/// reach the key, so it cannot skip the gate.
pub(super) fn signing_key(vault: &CustodyVault, scope: SignatureScope) -> Result<UnifiedKey> {
    crate::rpc::check_wallet_signing(scope).map_err(|e| match e {
        crate::rpc::RpcError::WrongNetwork(m) => WalletError::WrongNetwork(m),
        other => WalletError::WrongNetwork(other.to_string()),
    })?;
    let entropy = read_entropy(vault)?;
    derive_key_from_entropy(&entropy)
    // `entropy` zeroizes on drop here.
}

/// The stored wallet's non-secret identity (address + public key), re-derived from the sealed
/// entropy. The derived key is dropped (and zeroized) before this returns; only the identity
/// leaves the module.
pub(super) fn stored_info(vault: &CustodyVault) -> Result<WalletInfo> {
    let entropy = read_entropy(vault)?;
    let key = derive_key_from_entropy(&entropy)?;
    Ok(info_of(&key))
    // `entropy` (Zeroizing) + `key` zeroize on drop here.
}

/// A deterministic 32-byte secret, `HKDF-SHA256(salt, entropy)` expanded with `info ‖ counter`,
/// that is always a valid secp256k1 scalar. One-way: it reveals nothing about the entropy or the
/// wallet signing key (`BIP32(entropy, m/44'/60'/0'/0/0)`). See `wallet::derive_scoped_secret`.
pub(super) fn scoped_secret(vault: &CustodyVault, info: &[u8]) -> Result<Zeroizing<[u8; 32]>> {
    use hkdf::Hkdf;
    use sha2::Sha256;
    let entropy = read_entropy(vault)?;
    let hk = Hkdf::<Sha256>::new(Some(SCOPED_SECRET_SALT), &entropy);
    // Deterministic valid-scalar search: info ‖ counter(LE). counter 0 succeeds with overwhelming
    // probability; the loop only exists so the derivation can never fail on a bad-scalar draw.
    for counter in 0u16..=u16::MAX {
        let mut okm = Zeroizing::new([0u8; 32]);
        let mut ctx = Vec::with_capacity(info.len() + 2);
        ctx.extend_from_slice(info);
        ctx.extend_from_slice(&counter.to_le_bytes());
        hk.expand(&ctx, &mut okm[..])
            .map_err(|_| WalletError::Derivation)?;
        if k256::ecdsa::SigningKey::from_slice(&okm[..]).is_ok() {
            return Ok(okm);
        }
    }
    Err(WalletError::Derivation)
    // `entropy` zeroizes on drop; each rejected `okm` zeroizes as it leaves scope.
}

/// Create/import's pre-existence probe: `AlreadyExists` if a wallet is stored, `Ok(())` when the
/// read is denied (absent slot OR locked vault; the seal `put` that follows is the authoritative
/// lock gate), and any other custody error as `Custody`. The bytes read are dropped (zeroized)
/// here.
pub(super) fn refuse_if_present(vault: &CustodyVault) -> Result<()> {
    match vault.wallet_slot_get(WalletKeyAccess(()), WALLET_ENTROPY_SLOT) {
        Ok(_existing) => Err(WalletError::AlreadyExists),
        Err(CustodyError::Denied) => Ok(()),
        Err(e) => Err(e.into()),
    }
}

/// Derive the `UnifiedKey` for the default account from raw BIP39 entropy, in-process. The
/// entropy → mnemonic → seed → `m/44'/60'/0'/0/0` chain is the single derivation used to re-derive
/// the stored wallet, so signing and the displayed address cannot diverge.
///
/// The intermediate mnemonic string is held in a `Zeroizing` buffer and wiped on return; the
/// returned `UnifiedKey`'s inner `k256::SigningKey` zeroizes on drop. The caller owns `entropy`'s
/// lifetime and MUST zeroize it (it arrives in a `Zeroizing` from the vault).
fn derive_key_from_entropy(entropy: &[u8]) -> Result<UnifiedKey> {
    // Re-encode entropy → mnemonic (the total, loss-less inverse of `to_entropy`).
    let mnemonic =
        bip39::Mnemonic::from_entropy(entropy).map_err(|_| WalletError::InvalidMnemonic)?;
    // `Mnemonic` does not zeroize without the crate's `zeroize` feature (not enabled), so render
    // to a `Zeroizing<String>` we control. `Mnemonic` has no secret-wiping Drop, so an explicit
    // `drop` would be a no-op (clippy `drop_non_drop`); it falls out of scope at return.
    let phrase: Zeroizing<String> = Zeroizing::new(mnemonic.to_string());
    let key = secp256k1_from_mnemonic(&phrase, DEFAULT_ACCOUNT_INDEX)
        .map_err(|_| WalletError::Derivation)?;
    // `phrase` zeroizes here on drop.
    Ok(key)
}

/// Read the sealed entropy from the vault. The read requires an unlocked session (fails closed
/// `Denied` when locked) and returns a `Zeroizing` buffer.
///
/// B1.2-R2 (CLOSED in B1.5): custody returns the SAME opaque `Denied` for BOTH "locked" and
/// "absent slot", deliberately, so the passphrase path has no wrong-vs-empty oracle. That opacity
/// is a custody crypto property and must NOT change. The wallet layer still gives a crisper UX
/// signal without re-introducing the oracle: `is_unlocked()` reflects only the SESSION state
/// (already invoke-observable via `custody_status`), never anything passphrase-derived. On a
/// `Denied` read, a LOCKED vault → `Custody` ("unlock" hint) and an UNLOCKED vault → `NotFound`
/// ("create a wallet" hint). Fail-closed either way: no key is read, no signature produced.
fn read_entropy(vault: &CustodyVault) -> Result<Zeroizing<Vec<u8>>> {
    match vault.wallet_slot_get(WalletKeyAccess(()), WALLET_ENTROPY_SLOT) {
        Ok(e) => Ok(e),
        Err(CustodyError::Denied) => {
            if vault.is_unlocked() {
                // Unlocked but the slot read denied → the slot is genuinely absent.
                Err(WalletError::NotFound)
            } else {
                // Locked → fail closed with the locked signal (B1.2-R2).
                Err(WalletError::Custody)
            }
        }
        Err(e) => Err(e.into()),
    }
}
