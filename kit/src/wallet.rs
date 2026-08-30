//! citrate-core — wallet keystore (CORE-B1.1). @rule8 · T1 signing-key custody.
//!
//! The FIRST non-reissuable secret in the A2 custody vault: a real EVM
//! (secp256k1) signing key, mnemonic-recoverable. This module is the integration
//! layer between the pinned `citrate-wallet-core` crypto library and the A2 vault
//! (`custody.rs`). It follows Option A (D-B1.1-1): **the A2 vault is the SOLE
//! custody store.** We use `citrate-wallet-core` ONLY as a stateless crypto lib —
//! BIP39 create/import, BIP44 secp256k1 HD derivation (`m/44'/60'/0'/0/0`),
//! `UnifiedKey` signing/address derivation. We NEVER touch its `KeyManager`,
//! `keys.json`, `save`, `session`, `unlock`, or password domain. No second
//! keystore, no second unlock domain, no key material on disk in the clear.
//!
//! ## What is sealed (D-B1.1-3 — the raw BIP39 entropy)
//! We seal the raw BIP39 **entropy** (16/32 bytes) — NOT the 64-byte seed, NOT
//! the mnemonic string. Rationale:
//! - It is the smallest canonical unit: 32 bytes for a 24-word mnemonic vs 64 for
//!   the derived seed. Less at-rest ciphertext, less to zeroize.
//! - It is the CANONICAL backup unit: the mnemonic ⇄ entropy map is total and
//!   loss-less (`Mnemonic::from_entropy` / `Mnemonic::to_entropy`), so the entropy
//!   IS the wallet — the displayed 24-word phrase is exactly a re-encoding of it.
//! - It is re-derivable: entropy → mnemonic → (empty-passphrase) seed →
//!   `m/44'/60'/0'/0/0` reproduces the same key every time, so we never need to
//!   store the seed or the derived private key.
//! - Storing the mnemonic STRING would put human-readable secret words at rest and
//!   is strictly larger; storing the SEED would double the at-rest secret and
//!   discard the (equivalent) checksum the entropy carries. Entropy is the tight,
//!   auditable choice.
//!
//! The mnemonic is reconstructed in-process only, shown exactly once at create for
//! the user to back up, and never persisted / logged / Debug-printed.
//!
//! ## Custody boundary (mirrors A2's I-2 + A3's `oidc-` reservation)
//! The entropy is sealed into the A2 vault under the backend-reserved
//! **`wallet-`** slot prefix. `custody.rs` already reserves the `oidc-` prefix and
//! rejects any INVOKE-path `custody_put` that targets a reserved slot (the A3-01 /
//! DR-3 guard). B1.1 EXTENDS that same guard to `wallet-` so a compromised/XSS'd
//! webview can never plant or overwrite the wallet key via the raw
//! `invoke("custody_put", ...)`. The in-process `put`/`custody_get` (used here) is
//! unrestricted — that is how the backend legitimately stores the key.
//!
//! ## Unlock-gated, fail-closed, zeroized
//! Every operation REQUIRES the vault UNLOCKED and fails closed (`Denied`) if
//! locked — create, import, and sign all go through `custody.rs`'s session gate.
//! The secret is unwrapped from the vault only in-process, only for the duration
//! of a derive/sign, and the entropy + derived key buffers are zeroized after use.
//!
//! ## No `#[tauri::command]` here returns secret material (I-2)
//! Like `custody_get`, the functions that touch the mnemonic/entropy/seed/private
//! key are plain in-process `pub fn`s, NOT invoke commands. The command-registry
//! enumeration test (B1.1-ADV-2) proves no invoke command returns secret bytes.
//! The interactive one-time backup display is out of B1.1's headless scope (B1.2+
//! ceremony/UI); the create fn RETURNS the mnemonic in-process for the UI to show
//! once — it is the caller's contract to display-and-drop, never to persist.

// B1.1 delivers the wallet keystore as an in-process seam consumed by B1.2 (the
// SignatureCeremony + command surface). Until B1.2 wires it, the public API here
// has no non-test caller, exactly like custody.rs's `custody_get`/`clear_slot`
// carried `#[allow(dead_code)]` for their unwired A3/B1 consumers. Scoped to this
// module so the non-test lib build does not flag the deliberately-in-process API.
#![allow(dead_code)]

use citrate_wallet_core::{
    secp256k1_from_mnemonic, sign_eip155_legacy_tx, sign_recoverable, LegacyTxFields, SignedTx,
    UnifiedKey,
};
use zeroize::{Zeroize, Zeroizing};

use crate::custody::{CustodyError, CustodyVault};

/// Slot-name prefix reserved for the wallet keystore (the B1.1 mirror of A3's
/// `oidc-` reservation). The single default account is sealed under
/// [`WALLET_ENTROPY_SLOT`]. The invoke-path `custody_put` rejects this prefix
/// (see `custody::is_backend_reserved_slot`), so the webview can never plant or
/// overwrite the wallet key; the in-process `put`/`custody_get` used here is
/// unrestricted.
pub const WALLET_SLOT_PREFIX: &str = "wallet-";

/// The vault slot holding the sealed raw BIP39 entropy for the default account
/// (`m/44'/60'/0'/0/0`). Its bytes NEVER leave the Rust process (in-process
/// `custody_get`, never an invoke).
pub const WALLET_ENTROPY_SLOT: &str = "wallet-entropy-0";

/// The single default account index for B1.1 (`m/44'/60'/0'/0/0`). Multi-account
/// HD is a later phase.
pub const DEFAULT_ACCOUNT_INDEX: u32 = 0;

/// The non-secret, display-safe result of creating a wallet: the derived EVM
/// address, the uncompressed public key (hex), and the mnemonic to show EXACTLY
/// ONCE for backup. The mnemonic field is the ONLY secret; the caller MUST
/// display it once and drop it, and MUST NOT persist or log it. It is not
/// `Serialize` and its `Debug` is redacted so it cannot accidentally cross the
/// invoke boundary or land in a log.
pub struct WalletCreate {
    /// The derived EVM address (`0x…`, EIP-55-agnostic lowercase from the crate).
    pub address: String,
    /// The uncompressed secp256k1 public key, hex (non-secret, for display).
    pub public_key_hex: String,
    /// The 24-word BIP39 mnemonic — SHOWN ONCE for backup, never persisted.
    /// Held in a `Zeroizing<String>` so it is wiped when this struct drops.
    pub mnemonic: Zeroizing<String>,
}

impl std::fmt::Debug for WalletCreate {
    /// B1.1-ADV-S: never expose the mnemonic through `Debug`. Address + pubkey are
    /// non-secret; the mnemonic is redacted.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WalletCreate")
            .field("address", &self.address)
            .field("public_key_hex", &self.public_key_hex)
            .field("mnemonic", &"<redacted>")
            .finish()
    }
}

/// The non-secret, display-safe identity of a stored wallet (address + pubkey).
/// Carries NO secret material, so it is safe to surface across the bridge.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct WalletInfo {
    pub address: String,
    #[serde(rename = "publicKeyHex")]
    pub public_key_hex: String,
}

/// Wallet keystore errors. Deliberately coarse and secret-free (B1.1-ADV-S): no
/// variant carries mnemonic/seed/key bytes and none is derived from crate error
/// text that could echo secret material.
#[derive(Debug, PartialEq, Eq)]
pub enum WalletError {
    /// The vault is locked / unavailable, or a custody op was denied. Fail closed.
    Custody,
    /// The supplied mnemonic is empty, short, mis-worded, or checksum-invalid.
    InvalidMnemonic,
    /// A wallet already exists in the vault (create refuses to clobber).
    AlreadyExists,
    /// No wallet is stored (sign/read with an empty keystore).
    NotFound,
    /// Key derivation failed (should be unreachable for a validated mnemonic).
    Derivation,
}

impl std::fmt::Display for WalletError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WalletError::Custody => write!(f, "wallet: custody vault locked or unavailable"),
            WalletError::InvalidMnemonic => write!(f, "wallet: invalid mnemonic"),
            WalletError::AlreadyExists => write!(f, "wallet: a wallet already exists"),
            WalletError::NotFound => write!(f, "wallet: no wallet stored"),
            WalletError::Derivation => write!(f, "wallet: key derivation failed"),
        }
    }
}

impl std::error::Error for WalletError {}

impl From<CustodyError> for WalletError {
    /// Collapse EVERY custody error to the opaque `Custody` variant, so a custody
    /// error string (or its oracle) never leaks through the wallet surface. A
    /// locked/absent/corrupt vault all fail closed the same way.
    fn from(_: CustodyError) -> Self {
        WalletError::Custody
    }
}

type Result<T> = std::result::Result<T, WalletError>;

/// Derive the `UnifiedKey` for the default account from raw BIP39 entropy,
/// in-process. The entropy → mnemonic → seed → `m/44'/60'/0'/0/0` chain is the
/// single derivation used by create/import/sign so they cannot diverge.
///
/// The intermediate mnemonic string is held in a `Zeroizing` buffer and wiped on
/// return; the returned `UnifiedKey`'s inner `k256::SigningKey` zeroizes on drop.
/// The caller owns `entropy`'s lifetime and MUST zeroize it (it arrives in a
/// `Zeroizing` from `custody_get`).
fn derive_key_from_entropy(entropy: &[u8]) -> Result<UnifiedKey> {
    // Re-encode entropy → mnemonic (the total, loss-less inverse of `to_entropy`).
    let mnemonic =
        bip39::Mnemonic::from_entropy(entropy).map_err(|_| WalletError::InvalidMnemonic)?;
    // `Mnemonic` does not zeroize without the crate's `zeroize` feature (we did
    // not enable it — flagged for the reviewer), so render to a
    // `Zeroizing<String>` we control. The `Mnemonic` itself carries no secret-
    // wiping Drop, so an explicit `drop` would be a no-op (clippy `drop_non_drop`);
    // it falls out of scope at return with the same effect.
    let phrase: Zeroizing<String> = Zeroizing::new(mnemonic.to_string());
    let key = secp256k1_from_mnemonic(&phrase, DEFAULT_ACCOUNT_INDEX)
        .map_err(|_| WalletError::Derivation)?;
    // `phrase` zeroizes here on drop.
    Ok(key)
}

/// The non-secret display identity of a `UnifiedKey`.
fn info_of(key: &UnifiedKey) -> WalletInfo {
    WalletInfo {
        address: key.derive_address(),
        public_key_hex: hex::encode(key.public_key_bytes()),
    }
}

/// **In-process only.** Create a new wallet: generate a 24-word BIP39 mnemonic,
/// derive the secp256k1 default account, and SEAL the raw entropy into the A2
/// vault's backend-reserved `wallet-entropy-0` slot. Returns the derived
/// address + pubkey (non-secret) AND the mnemonic to show EXACTLY ONCE for backup.
///
/// Custody: requires the vault UNLOCKED (fails closed `Custody` if locked — the
/// seal goes through `vault.put`, which denies when locked). Refuses to clobber
/// an existing wallet (`AlreadyExists`). The entropy buffer is zeroized after
/// sealing (`vault.put` zeroizes the buffer it is handed); the intermediate seed
/// lives only inside the crate's `Zeroizing` and is gone before this returns.
///
/// This is a plain `pub fn`, NEVER a `#[tauri::command]` — the mnemonic/entropy
/// never cross the invoke boundary (B1.1-ADV-2). The one-time UI display is the
/// caller's contract (B1.2+); it must display-and-drop, never persist.
pub fn create(vault: &CustodyVault) -> Result<WalletCreate> {
    // Refuse to overwrite an existing wallet. This read requires an unlocked
    // vault; a locked vault fails closed here (`Custody`) before we mint anything.
    match vault.custody_get(WALLET_ENTROPY_SLOT) {
        Ok(_existing) => return Err(WalletError::AlreadyExists),
        Err(CustodyError::Denied) => { /* not found OR locked — probe distinguishes below */ }
        Err(e) => return Err(e.into()),
    }
    // Fail closed if the vault is locked: attempt the store on a fresh slot only
    // when unlocked. `custody_get` returns `Denied` for BOTH "locked" and "absent
    // slot" (no oracle), so we cannot tell them apart on the read; the `put` below
    // is the authoritative gate — it `Denied`s when locked and succeeds only when
    // unlocked, so a locked vault never reaches a seal.

    // Generate a 24-word (256-bit) mnemonic via the crate's vetted BIP39 impl.
    let mnemonic = bip39::Mnemonic::generate(24).map_err(|_| WalletError::Derivation)?;
    let phrase: Zeroizing<String> = Zeroizing::new(mnemonic.to_string());
    // Raw entropy is the canonical unit we seal (D-B1.1-3). (`mnemonic` has no
    // secret-wiping Drop; it falls out of scope at return.)
    let mut entropy: Vec<u8> = mnemonic.to_entropy();

    // Derive the account for its non-secret display identity BEFORE sealing, so a
    // derivation failure aborts without leaving a half-created wallet.
    let key = secp256k1_from_mnemonic(&phrase, DEFAULT_ACCOUNT_INDEX)
        .map_err(|_| WalletError::Derivation)?;
    let info = info_of(&key);
    drop(key); // the SigningKey zeroizes on drop; nothing else holds it

    // Seal the entropy in the backend-reserved wallet slot. `vault.put` requires
    // an unlocked session (fails closed `Denied` → `Custody` when locked) and
    // ZEROIZES the `entropy` buffer we hand it. This is the in-process `put`
    // (unrestricted); the INVOKE-path `custody_put` would reject a `wallet-` slot.
    let put = vault.put(WALLET_ENTROPY_SLOT, &mut entropy);
    entropy.zeroize(); // belt-and-suspenders (put already zeroized it)
    put.map_err(WalletError::from)?;

    Ok(WalletCreate {
        address: info.address,
        public_key_hex: info.public_key_hex,
        mnemonic: phrase,
    })
}

/// **In-process only.** Import a user-supplied mnemonic: validate + derive the
/// secp256k1 default account, then SEAL its raw entropy into the same
/// `wallet-entropy-0` slot (same path as [`create`]). Returns the derived
/// non-secret identity. Requires the vault UNLOCKED (fails closed if locked).
/// Refuses to clobber an existing wallet (`AlreadyExists`).
///
/// The inbound `mnemonic` is caller-owned; we validate it, re-encode to entropy,
/// seal, and zeroize our copies. Errors (never panics) on an empty/short/
/// checksum-invalid mnemonic (`InvalidMnemonic`).
pub fn import(vault: &CustodyVault, mnemonic: &str) -> Result<WalletInfo> {
    match vault.custody_get(WALLET_ENTROPY_SLOT) {
        Ok(_existing) => return Err(WalletError::AlreadyExists),
        Err(CustodyError::Denied) => {}
        Err(e) => return Err(e.into()),
    }

    // Parse + checksum-validate the mnemonic (the crate normalizes + validates).
    let parsed =
        bip39::Mnemonic::parse(mnemonic.trim()).map_err(|_| WalletError::InvalidMnemonic)?;
    let phrase: Zeroizing<String> = Zeroizing::new(parsed.to_string());
    let mut entropy: Vec<u8> = parsed.to_entropy();

    // Derive for the non-secret identity, using the SAME derivation as create.
    let key = secp256k1_from_mnemonic(&phrase, DEFAULT_ACCOUNT_INDEX)
        .map_err(|_| WalletError::Derivation)?;
    let info = info_of(&key);
    drop(key); // SigningKey zeroizes on drop
    drop(phrase); // Zeroizing<String> wiped here; import does not return the mnemonic

    let put = vault.put(WALLET_ENTROPY_SLOT, &mut entropy);
    entropy.zeroize();
    put.map_err(WalletError::from)?;

    Ok(info)
}

/// **In-process only.** The stored wallet's non-secret identity (address +
/// pubkey), re-derived from the sealed entropy. Requires the vault UNLOCKED
/// (fails closed if locked). `NotFound` if no wallet is stored.
///
/// NOTE: this returns ONLY the non-secret `WalletInfo`. It is still a plain
/// `pub fn` (never an invoke command) because it must `custody_get` the entropy
/// to re-derive — the secret is unwrapped in-process and zeroized here; only the
/// address/pubkey leave.
pub fn address(vault: &CustodyVault) -> Result<WalletInfo> {
    let entropy = read_entropy(vault)?;
    let key = derive_key_from_entropy(&entropy)?;
    Ok(info_of(&key))
    // `entropy` (Zeroizing) + `key` zeroize on drop here.
}

/// [`address`], but self-healing the passphrase-less vault first.
///
/// WHY THIS EXISTS. The vault auto-locks (~30 min) and, in the device-bound model,
/// there is no user passphrase to re-enter — only the keyring device secret can
/// re-open it. `address` fails closed on a locked vault, so EVERY command that
/// merely wanted the member's own address started returning "custody vault locked
/// or unavailable" once the app had been open for half an hour.
///
/// That is not a security boundary — the same device secret is right there, and
/// `wallet_balances` already self-healed exactly this way. It just had not been
/// applied consistently, so validator activation, staking, withdrawals, transfers
/// and the activity feed all failed closed on an idle app while balances kept
/// working (observed 2026-08-06: "Validator activation unavailable — wallet:
/// custody vault locked or unavailable" with 32,000 SALT already bonded on chain).
///
/// `ensure_auto_unlocked` is idempotent and fails closed on a reset keychain, so
/// this weakens nothing: a vault that genuinely cannot be re-opened still errors.
pub fn address_auto_unlocked(vault: &CustodyVault) -> Result<WalletInfo> {
    vault.ensure_auto_unlocked()?;
    address(vault)
}

/// **In-process only.** Read the sealed entropy from the vault. `custody_get`
/// requires an unlocked session (fails closed `Denied` when locked) and returns
/// a `Zeroizing` buffer.
///
/// B1.2-R2 (CLOSED in B1.5): custody's `custody_get` returns the SAME opaque
/// `Denied` for BOTH "locked" and "absent slot" — deliberately, so the passphrase
/// path has no wrong-vs-empty oracle (see custody.rs). That opacity is a *custody
/// crypto* property and must NOT change. But the wallet layer can still give a
/// crisper UX signal WITHOUT re-introducing that oracle: `is_unlocked()` reflects
/// only the SESSION state (a boolean the app already exposes via `custody_status`
/// on the invoke surface), never anything passphrase-derived. So on a `Denied`
/// read we consult it: a LOCKED vault → `Custody` (→ `VaultLocked` at the ceremony,
/// "unlock" hint); an UNLOCKED vault where the read still denied → `NotFound`
/// (absent slot, "create a wallet" hint). This is fail-closed either way (no key
/// is read, no signature produced); it only improves the ADV-3 signal
/// (locked-approve now surfaces `VaultLocked`, not `NoWallet`). It adds no
/// oracle: `is_unlocked` is passphrase-independent and already invoke-observable.
fn read_entropy(vault: &CustodyVault) -> Result<Zeroizing<Vec<u8>>> {
    match vault.custody_get(WALLET_ENTROPY_SLOT) {
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

/// **In-process only.** Sign a message with the stored wallet's default account.
/// Requires the vault UNLOCKED (fails closed if locked). Reads the sealed entropy,
/// re-derives the `UnifiedKey`, signs, and zeroizes the secret buffers. Returns
/// the raw ECDSA signature bytes (r||s, 64 bytes — non-recoverable form; the
/// recoverable EIP-155 tx path is B1.2/B1.4 via `chain::sign_secp256k1`).
///
/// This is the in-process sign PROOF for B1.1 (WP-3). It is deliberately NOT a
/// `#[tauri::command]` — no invoke command signs, and none returns key material.
///
/// **B1.2 gating (@rule8):** this is `pub(crate)`, not `pub`, and the ONLY
/// sanctioned caller is [`crate::ceremony::SignatureCeremony::approve`] — the
/// single human-in-the-loop signing path. No other code path in the crate signs
/// (B1.2-ADV-1/7). CLAUDE.md rule 3: all signing goes through the ceremony;
/// signing outside it is forbidden. The call-site invariant is asserted
/// structurally in `ceremony_tests::adv1_adv7_signer_only_reachable_via_approve`.
pub(crate) fn sign_message(vault: &CustodyVault, message: &[u8]) -> Result<Vec<u8>> {
    let entropy = read_entropy(vault)?;
    let key = derive_key_from_entropy(&entropy)?;
    let sig = key.sign(message);
    Ok(sig)
    // `entropy` + `key` zeroize on drop here.
}

/// **In-process only.** Sign a REAL legacy EIP-155 transaction with the stored
/// wallet's default account (CORE-B1.4). Requires the vault UNLOCKED (fails
/// closed if locked). Reads the sealed entropy, re-derives the `UnifiedKey`,
/// unwraps its secp256k1 `SigningKey`, and calls the LEAN upstream primitive
/// `citrate_wallet_core::sign_eip155_legacy_tx(&key, &fields, chain_id)` — the
/// same audited crypto (keccak + k256 recoverable + RLP) exercised by the
/// wallet-core spec-vector tests. Returns the [`SignedTx`] (raw RLP ready for
/// `eth_sendRawTransaction` + the tx hash + v/r/s) — NEVER key material.
///
/// The entropy + derived `UnifiedKey` zeroize on drop here; the wallet-core
/// signer zeroizes its intermediate signing-hash buffer (WAL-04) before return.
///
/// **B1.4 gating (@rule8):** like [`sign_message`], this is `pub(crate)` and the
/// ONLY sanctioned caller is [`crate::ceremony::SignatureCeremony`]'s transaction
/// approval path. No other code path in the crate signs a transaction. CLAUDE.md
/// rule 3: all signing goes through the ceremony; signing outside it is forbidden.
pub(crate) fn sign_transaction(
    vault: &CustodyVault,
    fields: &LegacyTxFields,
    chain_id: u64,
) -> Result<SignedTx> {
    let entropy = read_entropy(vault)?;
    let key = derive_key_from_entropy(&entropy)?;
    // Unwrap the secp256k1 signing key. The B1.1 derivation always yields a
    // Secp256k1 UnifiedKey for `m/44'/60'/0'/0/0`; a non-secp key would be a
    // derivation bug (fail closed rather than sign with the wrong curve).
    let signed = match &key {
        UnifiedKey::Secp256k1(sk) => {
            sign_eip155_legacy_tx(sk, fields, chain_id).map_err(|_| WalletError::Derivation)?
        }
        _ => return Err(WalletError::Derivation),
    };
    Ok(signed)
    // `entropy` + `key` (its inner k256::SigningKey) zeroize on drop here; the
    // wallet-core signer already zeroized the intermediate signing hash.
}

/// The EIP-191 `personal_sign` prehash: `keccak256("\x19Ethereum Signed Message:\n"
/// ‖ len ‖ message)`.
///
/// Pure and public so a caller can compute what it is about to ask a human to
/// approve, and so the prefix itself is testable against the fixed vectors every
/// Ethereum implementation agrees on.
pub fn eip191_prehash(message: &[u8]) -> [u8; 32] {
    use sha3::{Digest, Keccak256};
    let mut h = Keccak256::new();
    h.update(b"\x19Ethereum Signed Message:\n");
    h.update(message.len().to_string().as_bytes());
    h.update(message);
    let out = h.finalize();
    let mut prehash = [0u8; 32];
    prehash.copy_from_slice(&out);
    prehash
}

/// **In-process only.** Sign `message` as a real Ethereum `personal_sign`:
/// EIP-191 prefix, keccak256, recoverable secp256k1, `r ‖ s ‖ v` with `v` in
/// {27, 28}. Requires the vault UNLOCKED (fails closed if locked).
///
/// ## Why this exists, and why it is not a refactor
///
/// `sign_message` signs the raw bytes with k256's `Signer`, which prehashes with
/// **SHA-256** and returns a **64-byte non-recoverable** signature. That is not
/// `personal_sign` — nothing outside this process can recover the signer from it,
/// and any verifier that follows EIP-191 will reject it. `IntentKind::PersonalSign`
/// nevertheless used it, because B1.2 could not reach a recoverable signer under
/// the lean `crypto` build; the module header called that a deferral and named the
/// fix as *"add a recoverable message signer"*. This is that.
///
/// The concrete thing it unblocks: citrate-quorum's Rooms surface authenticates to
/// the citrate-comms relay with SIWE, which the relay verifies by RECOVERING the
/// signer from an EIP-191 signature. Without this, a human's room seat could not be
/// their wallet — the app had to fall back to a separate relay-only identity, and
/// say so on the roster.
///
/// **@rule8 gating (identical to the other two signers):** `pub(crate)`, and the
/// ONLY sanctioned caller is [`crate::ceremony::SignatureCeremony`]'s approval
/// path. `ceremony_tests::adv1_adv7_signer_only_reachable_via_approve` scans for
/// any other call site. CLAUDE.md rule 3: all signing goes through the ceremony.
pub(crate) fn sign_personal(vault: &CustodyVault, message: &[u8]) -> Result<[u8; 65]> {
    let entropy = read_entropy(vault)?;
    let key = derive_key_from_entropy(&entropy)?;
    // Same fail-closed rule as `sign_transaction`: the B1.1 derivation always
    // yields a Secp256k1 key for m/44'/60'/0'/0/0, so anything else is a
    // derivation bug and must not be signed with the wrong curve.
    let sk = match &key {
        UnifiedKey::Secp256k1(sk) => sk,
        _ => return Err(WalletError::Derivation),
    };
    let mut prehash = eip191_prehash(message);
    let (r, s, rec) = sign_recoverable(sk, &prehash).map_err(|_| WalletError::Derivation)?;
    // The prehash is a pre-image commitment to a private-key signature; the tx
    // signer zeroizes its equivalent buffer, so this one does too (WAL-04).
    prehash.zeroize();

    let mut out = [0u8; 65];
    out[..32].copy_from_slice(&r);
    out[32..64].copy_from_slice(&s);
    // `v` is 27/28 for personal_sign — the +27 offset every EIP-191 verifier
    // expects. A bare 0/1 recovery id here is the classic "signature verifies
    // nowhere" bug, so it is asserted in the tests.
    out[64] = rec + 27;
    Ok(out)
    // `entropy` + `key` zeroize on drop here.
}

/// **Verify, don't sign.** Recover the Ethereum address that produced an EIP-191 `personal_sign`
/// over `message` — the inverse of [`sign_personal`]. This is how a peer's *foreign* identity
/// binding is checked: recover the signer and confirm it equals the address the binding claims,
/// so nobody can assert a handle for an address they don't control. No key material, no vault —
/// pure over the public signature. `sig` is `r ‖ s ‖ v` with `v` in {27, 28} (or {0, 1}).
pub fn recover_personal(message: &[u8], sig: &[u8; 65]) -> Result<String> {
    use k256::ecdsa::{RecoveryId, Signature, VerifyingKey};
    use sha3::{Digest, Keccak256};

    let prehash = eip191_prehash(message);
    let signature = Signature::from_slice(&sig[..64]).map_err(|_| WalletError::Derivation)?;
    // Accept both the EIP-191 {27,28} offset and a bare {0,1} recovery id.
    let raw_v = sig[64];
    let v = if raw_v >= 27 { raw_v - 27 } else { raw_v };
    let rec_id = RecoveryId::from_byte(v).ok_or(WalletError::Derivation)?;
    let vk = VerifyingKey::recover_from_prehash(&prehash, &signature, rec_id)
        .map_err(|_| WalletError::Derivation)?;
    // address = last 20 bytes of keccak256(uncompressed_pubkey[1..]) (drop the 0x04 tag).
    let enc = vk.to_encoded_point(false);
    let pub_bytes = enc.as_bytes();
    let mut h = Keccak256::new();
    h.update(&pub_bytes[1..]);
    let out = h.finalize();
    Ok(format!("0x{}", hex::encode(&out[12..])))
}

/// Convenience: recover from a `0x`-prefixed (or bare) hex signature string.
pub fn recover_personal_hex(message: &[u8], sig_hex: &str) -> Result<String> {
    let clean = sig_hex.strip_prefix("0x").unwrap_or(sig_hex);
    let bytes = hex::decode(clean).map_err(|_| WalletError::Derivation)?;
    let sig: [u8; 65] = bytes.as_slice().try_into().map_err(|_| WalletError::Derivation)?;
    recover_personal(message, &sig)
}

#[cfg(test)]
mod tests {
    include!("wallet_tests.rs");
}
