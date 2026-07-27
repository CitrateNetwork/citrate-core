//! citrate-core — the SignatureCeremony (CORE-B1.2). @rule8 · the ONE
//! human-in-the-loop signing path. This module LIFTS Rule 3: until B1.2 there
//! were no signing code paths; after B1.2, **every** signature routes through
//! this ceremony and signing outside it is forbidden (CLAUDE.md rule 3).
//!
//! ## Why a ceremony (the invariant)
//! B1.1 sealed a real EVM signing key in the A2 vault and proved an in-process
//! sign (`wallet::sign_message`). That signer is now **gated**: it is
//! `pub(crate)` and is called from EXACTLY ONE place — [`SignatureCeremony::approve`].
//! No `#[tauri::command]` signs; no sidecar/agent/daemon signs. Any origin — the
//! local user, and later a node-agent, chat-agent, or micro-app — can only SUBMIT
//! an unsigned [`SignatureIntent`] via [`SignatureCeremony::request`], which
//! creates a PENDING ceremony and decodes it for human display but does NOT sign.
//! A signature is produced only when a human explicitly [`approve`](SignatureCeremony::approve)s
//! that specific [`CeremonyId`]. This is the on-chain-custody HITL invariant made
//! real: the key never signs without a per-request human approval.
//!
//! ## State machine (single-use)
//! - `request(intent) -> CeremonyId` — decode the intent, store it PENDING,
//!   return the id + the decoded view. **No key is touched.**
//! - `approve(id) -> Signature` — only if the ceremony EXISTS and is PENDING and
//!   (for undecodable calldata) a raw-mode ack was given. Unwraps the key via the
//!   B1.1 vault path, signs, zeroizes, and **CONSUMES** the ceremony (removes it
//!   from the pending map before signing — single-use, so a replayed/duplicate
//!   approval on the same id finds nothing and errors). Returns the signature
//!   bytes ONLY — never key/seed/entropy material.
//! - `reject(id)` — consumes the ceremony, produces no signature.
//!
//! ## Consumption ordering (replay defense — B1.2-ADV-10)
//! `approve` **removes the ceremony from the map first**, then signs the removed
//! copy. Two approvals racing the same id serialize on the map mutex; the first
//! `remove` wins and the second sees `None` → `UnknownCeremony`. One approval →
//! one signature, always.
//!
//! ## Raw-ack gate (B1.2-ADV-5)
//! If an intent's calldata cannot be decoded to a human-readable action, its
//! decoded view is [`DecodedAction::action`] == [`UNRECOGNIZED_ACTION`] and the
//! ceremony is flagged `requires_raw_ack`. `approve` REFUSES such a ceremony
//! unless called with an explicit `raw_ack = true` bound to that id — a blind
//! approval of undecodable calldata is impossible.
//!
//! ## Fail closed when locked (B1.2-ADV-3)
//! `approve` signs through `wallet::sign_message`, which reads the sealed entropy
//! via the A2 session gate; a locked vault fails closed (`Custody` → `VaultLocked`)
//! and no signature is produced.
//!
//! ## Command surface (I-2 — no secret crosses invoke)
//! `sign_request`/`sign_approve`/`sign_reject` are `#[tauri::command]`s that
//! return a CeremonyId / decoded intent / signature (hex) / status — NEVER
//! key/seed/entropy. `Signature` is deliberately hex, not raw key material; the
//! signature over a message is not secret. The compile barrier from B1.1 holds:
//! the wallet secret-reading fns remain plain `pub fn`s off the registry.
//!
//! ## Recoverable EIP-155 tx form is DEFERRED (honest scope)
//! The scope permitted `chain::sign_secp256k1` (recoverable EIP-155) IF available
//! under the lean `crypto` feature. It is NOT: in `citrate-wallet-core` the whole
//! `chain` module is `#[cfg(feature = "native")]`, and citrate-core pins the lean
//! `default-features = false, features = ["crypto"]` build (Cargo.toml B1.1-F-1)
//! specifically to keep the zk/execution stack out of the key path. So B1.2 signed
//! via the message path (`wallet::sign_message`, r||s) for ALL three intent kinds;
//! the recoverable tx-signing form was deferred to B1.4 (which would either enable
//! a `chain` seam under the lean build or add a recoverable message signer).
//!
//! **Both deferrals are now closed.** B1.4 added `wallet::sign_transaction` (real
//! EIP-155, recoverable) behind `approve_and_broadcast`, and `personal_sign` now
//! goes through `wallet::sign_personal` — EIP-191 prefix, keccak256, recoverable,
//! `v` in {27,28} — which is what the name always claimed. `typed_data` still takes
//! the message path: EIP-712 needs its own domain-separated hashing, and
//! approximating it silently would be the same defect this change is fixing.

// This module is consumed by the B1.2 command surface in `lib.rs` (wired) and by
// the B1.3 wagmi connector later. Some constructors/fields are part of the stable
// surface but only exercised by tests until B1.3; keep the non-test lib build
// quiet exactly like `wallet.rs`/`custody.rs` do for their staged consumers.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::custody::CustodyVault;
use crate::wallet::{self, WalletError};

/// The action string surfaced when calldata cannot be decoded to a human action.
/// A ceremony carrying this action is `requires_raw_ack` and cannot be approved
/// without an explicit raw-mode acknowledgement (B1.2-ADV-5).
pub const UNRECOGNIZED_ACTION: &str = "Unrecognized";

/// The kind of signature being requested. Mirrors the EIP-1193 request shapes a
/// later wagmi connector (B1.3) will route through the ceremony.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntentKind {
    /// `personal_sign` — an opaque UTF-8/byte message.
    PersonalSign,
    /// `eth_signTypedData` (EIP-712) — structured typed data.
    TypedData,
    /// A transaction to be signed (recoverable EIP-155 form deferred to B1.4).
    Transaction,
}

/// The human-readable decode of an intent, surfaced verbatim in the approval UI.
/// This is what the human sees and approves; it is NOT trusted to be benign —
/// the [`SignatureIntent::origin`] is displayed verbatim alongside it, and an
/// undecodable action shows [`UNRECOGNIZED_ACTION`] rather than a fabricated
/// benign summary (B1.2-ADV-5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecodedAction {
    /// A short human summary of what will be signed (or [`UNRECOGNIZED_ACTION`]).
    pub action: String,
    /// The value/cost this intent moves, human-readable (empty if none/unknown).
    pub cost: String,
    /// The destination address/domain this intent targets (empty if none/unknown).
    pub destination: String,
}

/// A signature intent submitted to the ceremony. Built by any origin (the user,
/// or later a node-agent / chat-agent / micro-app). The `origin` is set by the
/// caller/bridge and **displayed verbatim** — never trusted to be benign
/// (B1.2-ADV-5). Submitting an intent NEVER signs; it only creates a PENDING
/// ceremony a human must approve.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignatureIntent {
    /// Who is asking (an origin URL, "local-user", an agent id). Displayed as-is.
    pub origin: String,
    /// What kind of signature is requested.
    pub kind: IntentKind,
    /// The chain id this intent targets (40204 for Citrate; display + later bind).
    pub chain_id: u64,
    /// The raw payload to be signed (message bytes / typed-data JSON / tx bytes),
    /// hex-encoded (`0x…` optional). This is what the gated signer signs.
    pub raw: String,
}

/// The non-secret view returned by `request` (and re-inspectable by `status`):
/// the ceremony id + the true origin + the decoded action. Carries NO signature
/// and NO key material — safe to cross the invoke bridge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CeremonyView {
    /// The opaque single-use id the human must approve/reject explicitly.
    pub id: String,
    /// The TRUE origin, displayed verbatim (B1.2-ADV-5).
    pub origin: String,
    /// The requested kind.
    pub kind: IntentKind,
    /// The chain id.
    #[serde(rename = "chainId")]
    pub chain_id: u64,
    /// The human-readable decode surfaced for approval.
    pub decoded: DecodedAction,
    /// Whether approval is BLOCKED until an explicit raw-mode ack (undecodable
    /// calldata). The UI must not present a one-click approve for these
    /// (B1.2-ADV-5/6).
    #[serde(rename = "requiresRawAck")]
    pub requires_raw_ack: bool,
}

/// A signature result crossing the bridge. Hex, no `0x`. This is the ONLY thing
/// `approve` returns — never key, seed, or entropy material. A signature over a
/// message is not secret.
///
/// The length depends on the kind, and a caller must not assume one:
/// `personal_sign` is `r||s||v` (65 bytes → 130 hex chars, EIP-191 recoverable);
/// `typed_data` and `transaction` are `r||s` (64 bytes → 128 hex chars) from the
/// message path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Signature {
    /// Hex of the raw ECDSA signature bytes — `r||s||v` for `personal_sign`,
    /// `r||s` otherwise. **Serialized as `sigHex`**, not `sig_hex`.
    #[serde(rename = "sigHex")]
    pub sig_hex: String,
    /// The kind that was signed (echo, for the caller to route the result).
    pub kind: IntentKind,
}

/// Broadcast/poll configuration for [`SignatureCeremony::approve_and_broadcast`]:
/// the chain id to bind (EIP-155) plus the receipt-poll budget. Grouped into one
/// param so the approve+broadcast signature stays legible.
#[derive(Debug, Clone, Copy)]
pub struct BroadcastConfig {
    /// The EIP-155 chain id (40204 for Citrate).
    pub chain_id: u64,
    /// How many times to poll `eth_getTransactionReceipt` before timing out.
    pub poll_attempts: u32,
    /// The delay between receipt polls.
    pub poll_interval: std::time::Duration,
}

/// The result of a ceremony-approved transaction that was SIGNED (real EIP-155
/// legacy tx from the vault key) and BROADCAST to the live 40204 RPC (B1.4).
/// Carries only PUBLIC facts — the accepted tx hash, and (once mined) the block
/// number the node confirmed inclusion in. NEVER key/seed/entropy material.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BroadcastResult {
    /// The transaction hash the node accepted (`0x…`).
    #[serde(rename = "txHash")]
    pub tx_hash: String,
    /// The block number the tx was included in, once the receipt is available
    /// (`None` if broadcast succeeded but the receipt has not yet been polled).
    #[serde(rename = "blockNumber")]
    pub block_number: Option<u64>,
}

/// Errors from the ceremony surface. Deliberately coarse + secret-free: no
/// variant carries key/seed/entropy bytes, and the custody/vault failures all
/// collapse so a caller cannot probe vault internals through the ceremony.
#[derive(Debug, PartialEq, Eq)]
pub enum CeremonyError {
    /// No pending ceremony with that id (never created, already consumed by a
    /// prior approve/reject, or a replayed/duplicate approval — B1.2-ADV-10).
    UnknownCeremony,
    /// The ceremony carries undecodable calldata and was approved without the
    /// required explicit raw-mode ack (B1.2-ADV-5).
    RawAckRequired,
    /// The vault is locked / signing was denied — fail closed (B1.2-ADV-3).
    VaultLocked,
    /// No wallet is stored to sign with.
    NoWallet,
    /// The signer failed for a non-custody reason (bad payload, derivation).
    SignFailed,
    /// A transaction intent's payload could not be decoded to signable legacy-tx
    /// fields at broadcast time (B1.4) — e.g. calldata present with no gas.
    UndecodableTransaction,
    /// F-2 (B1.5): the tx intent's `from` does not match the vault wallet
    /// address. Caught BEFORE any nonce-fetch/sign so the ceremony never signs a
    /// tx the vault key cannot author, and never leaks the mismatch to the node
    /// as a downstream nonce/sender rejection. Carries NO key material — only the
    /// (public) claimed-vs-actual addresses, so the human sees why it was refused.
    FromMismatch { claimed: String, wallet: String },
    /// The signed tx could not be broadcast / confirmed on 40204 (B1.4). Carries
    /// the RPC error's PUBLIC message (node reason / transport / timeout) — never
    /// key material (the broadcast client never sees a key).
    Broadcast(String),
}

impl std::fmt::Display for CeremonyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CeremonyError::UnknownCeremony => write!(f, "ceremony: unknown or already-consumed id"),
            CeremonyError::RawAckRequired => {
                write!(
                    f,
                    "ceremony: undecodable calldata requires an explicit raw-mode ack"
                )
            }
            CeremonyError::VaultLocked => write!(f, "ceremony: vault locked or signing denied"),
            CeremonyError::NoWallet => write!(f, "ceremony: no wallet stored"),
            CeremonyError::SignFailed => write!(f, "ceremony: signing failed"),
            CeremonyError::UndecodableTransaction => {
                write!(f, "ceremony: transaction payload is not signable legacy-tx")
            }
            CeremonyError::FromMismatch { claimed, wallet } => write!(
                f,
                "ceremony: tx `from` ({claimed}) does not match this wallet ({wallet})"
            ),
            CeremonyError::Broadcast(m) => write!(f, "ceremony: broadcast failed: {m}"),
        }
    }
}

impl std::error::Error for CeremonyError {}

impl From<WalletError> for CeremonyError {
    /// Map the wallet signer's error onto the ceremony surface. Both `Custody`
    /// (locked / denied) and `NotFound` fail CLOSED with no signature.
    ///
    /// HONEST NOTE (@rule8): B1.1's `read_entropy` deliberately maps a locked-vault
    /// `custody_get` denial to `WalletError::NotFound`, not `Custody` — custody has
    /// no locked-vs-absent oracle (see custody.rs). So a LOCKED signer surfaces here
    /// as `NoWallet`, not `VaultLocked`. That is fail-closed either way (no key is
    /// read, no signature is produced); the two variants exist so a caller can hint
    /// the user ("unlock" vs "create a wallet"), but the security property does not
    /// depend on distinguishing them, and the locked path currently lands on
    /// `NoWallet`. `Custody` (from `put`/other paths) still maps to `VaultLocked`.
    /// A crisper locked signal would need a locked-vs-absent distinction in the
    /// wallet layer — flagged as a follow-up, NOT acted on here (out of B1.2 scope).
    fn from(e: WalletError) -> Self {
        match e {
            WalletError::Custody => CeremonyError::VaultLocked,
            WalletError::NotFound => CeremonyError::NoWallet,
            _ => CeremonyError::SignFailed,
        }
    }
}

type Result<T> = std::result::Result<T, CeremonyError>;

/// A pending ceremony held in the map until it is consumed by approve/reject.
/// Holds the full intent (to sign on approve) + the precomputed decode + the
/// raw-ack requirement.
#[derive(Debug, Clone)]
struct Pending {
    intent: SignatureIntent,
    decoded: DecodedAction,
    requires_raw_ack: bool,
}

/// The process-wide SignatureCeremony: the single approval surface. Holds the
/// pending ceremonies behind a mutex and mints monotonic ids. All signing goes
/// through [`approve`](Self::approve); no other path in the crate reaches the
/// gated `wallet::sign_message`.
pub struct SignatureCeremony {
    pending: Mutex<BTreeMap<u64, Pending>>,
    next_id: AtomicU64,
}

impl Default for SignatureCeremony {
    fn default() -> Self {
        Self::new()
    }
}

impl SignatureCeremony {
    pub fn new() -> Self {
        SignatureCeremony {
            pending: Mutex::new(BTreeMap::new()),
            next_id: AtomicU64::new(1),
        }
    }

    /// Lock the pending map, recovering from poison (a panic mid-op must not
    /// brick the signing surface). The only invariant under the lock is the
    /// pending map; a poisoned guard yields it intact.
    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<u64, Pending>> {
        self.pending.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Parse a `CeremonyId` string back to its numeric key. A malformed id is
    /// simply "unknown" (it can never match a minted id).
    fn parse_id(id: &str) -> Option<u64> {
        id.parse::<u64>().ok()
    }

    /// **Step 1 — request.** Decode the intent for human display and store it as
    /// a PENDING ceremony. Returns the id + decoded view. **Does NOT sign** and
    /// touches no key material. The returned `id` is what a human must later
    /// approve/reject *explicitly* — there is no "approve latest" (B1.2-ADV-6).
    pub fn request(&self, intent: SignatureIntent) -> CeremonyView {
        let decoded = decode_intent(&intent);
        let requires_raw_ack = decoded.action == UNRECOGNIZED_ACTION;
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let view = CeremonyView {
            id: id.to_string(),
            origin: intent.origin.clone(),
            kind: intent.kind,
            chain_id: intent.chain_id,
            decoded: decoded.clone(),
            requires_raw_ack,
        };
        self.lock().insert(
            id,
            Pending {
                intent,
                decoded,
                requires_raw_ack,
            },
        );
        view
    }

    /// Re-inspect a pending ceremony without consuming it (for the approval UI to
    /// re-render). `None` if it does not exist / was consumed.
    pub fn status(&self, id: &str) -> Option<CeremonyView> {
        let key = Self::parse_id(id)?;
        let map = self.lock();
        let p = map.get(&key)?;
        Some(CeremonyView {
            id: id.to_string(),
            origin: p.intent.origin.clone(),
            kind: p.intent.kind,
            chain_id: p.intent.chain_id,
            decoded: p.decoded.clone(),
            requires_raw_ack: p.requires_raw_ack,
        })
    }

    /// **Step 2 — approve.** The ONLY signing path in the crate. Consumes the
    /// ceremony (single-use), then signs via the gated `wallet::sign_message`.
    ///
    /// Guards, in order:
    /// 1. The id must map to a PENDING ceremony (`UnknownCeremony` otherwise —
    ///    this also rejects a replayed/duplicate approval, B1.2-ADV-10).
    /// 2. **Consume first:** the ceremony is REMOVED from the map before signing,
    ///    so a racing second approve on the same id serializes on the mutex, sees
    ///    `None`, and errors. One approval → one signature.
    /// 3. Undecodable calldata REQUIRES `raw_ack == true` (`RawAckRequired`
    ///    otherwise — B1.2-ADV-5). If the ack is missing, the (already-removed)
    ///    ceremony is RE-INSERTED so a legitimate re-approval with the ack still
    ///    works; the attack path (no ack) still cannot sign.
    /// 4. Sign through the vault (fails closed if LOCKED — B1.2-ADV-3).
    ///
    /// `raw_ack` MUST be bound to THIS id by the caller (the command passes the
    /// UI's explicit raw-mode acknowledgement). There is no auto-approve and no
    /// "approve latest" (B1.2-ADV-6): the caller names the id.
    pub fn approve(&self, vault: &CustodyVault, id: &str, raw_ack: bool) -> Result<Signature> {
        let key = Self::parse_id(id).ok_or(CeremonyError::UnknownCeremony)?;

        // Consume-first (B1.2-ADV-10): remove under the lock so a duplicate
        // approve cannot find the same ceremony a second time.
        let pending = {
            let mut map = self.lock();
            map.remove(&key).ok_or(CeremonyError::UnknownCeremony)?
        };

        // Raw-ack gate (B1.2-ADV-5): undecodable calldata is never approvable as
        // if benign. On a missing ack, re-insert so the human can retry WITH the
        // ack — but this call signs nothing.
        if pending.requires_raw_ack && !raw_ack {
            self.lock().insert(key, pending);
            return Err(CeremonyError::RawAckRequired);
        }

        // Sign through the gated signer. `payload_bytes` decodes the hex raw; the
        // signer reads the sealed entropy via the A2 session gate and fails closed
        // when locked (B1.2-ADV-3). The ceremony is ALREADY consumed, so even if
        // signing errors, this id cannot be re-approved (fail closed, single-use).
        let bytes = payload_bytes(&pending.intent.raw).ok_or(CeremonyError::SignFailed)?;
        let sig = match pending.intent.kind {
            // `personal_sign` means EIP-191: keccak256 over the prefixed message,
            // recoverable secp256k1, `v` in {27,28}. B1.2 signed these through
            // `sign_message` (SHA-256 prehash, 64 bytes, NOT recoverable) because
            // this crate could not reach a recoverable signer under the lean
            // `crypto` build — the module header above called that a deferral and
            // named the fix. A verifier following EIP-191 rejects the old form, and
            // nothing can recover the signer from it, so this was not a signature
            // anyone outside this process could use.
            IntentKind::PersonalSign => wallet::sign_personal(vault, &bytes)?.to_vec(),
            // TypedData and Transaction keep the message path. EIP-712 needs its own
            // domain-separated hashing, which is a separate piece of work and is NOT
            // silently approximated here; `sign_and_broadcast` is what produces a
            // real, recoverable transaction signature.
            IntentKind::TypedData | IntentKind::Transaction => wallet::sign_message(vault, &bytes)?,
        };
        Ok(Signature {
            sig_hex: hex::encode(sig),
            kind: pending.intent.kind,
        })
    }

    /// **Step 2 (transaction) — approve + sign + broadcast (B1.4).** The ONLY
    /// path that produces a REAL, broadcastable 40204 transaction. It preserves
    /// EVERY B1.2 invariant of [`approve`]:
    ///   * consume-first under the lock (single-use; a racing/replayed approve on
    ///     the same id sees `None` → `UnknownCeremony`, B1.2-ADV-10),
    ///   * undecodable calldata REQUIRES `raw_ack == true` (re-inserted on a
    ///     missing ack so a legitimate retry works; B1.2-ADV-5),
    ///   * signs through the vault (fails CLOSED if locked; B1.2-ADV-3),
    ///   * returns only PUBLIC facts (tx hash + block) — never key material.
    ///
    /// Flow after the guards clear: decode the tx intent → fetch the pending
    /// nonce (`eth_getTransactionCount(from,"pending")`) + gas price
    /// (`eth_gasPrice`) from the LIVE RPC (Rule 1, real values) → sign the real
    /// EIP-155 legacy tx with the vault key via `wallet::sign_transaction` (which
    /// calls the lean `sign_eip155_legacy_tx`, zeroizing) → broadcast the raw tx
    /// (`eth_sendRawTransaction`) → poll the receipt for block inclusion.
    ///
    /// `rpc` is injected so tests mock the transport (Rule 1: the mock is a TEST
    /// transport; production wires [`crate::rpc::RpcClient::citrate`]).
    pub fn approve_and_broadcast<T: crate::rpc::RpcTransport>(
        &self,
        vault: &CustodyVault,
        rpc: &crate::rpc::RpcClient<T>,
        id: &str,
        raw_ack: bool,
        cfg: BroadcastConfig,
    ) -> Result<BroadcastResult> {
        let BroadcastConfig {
            chain_id,
            poll_attempts,
            poll_interval,
        } = cfg;
        let key = Self::parse_id(id).ok_or(CeremonyError::UnknownCeremony)?;

        // Consume-first (B1.2-ADV-10): remove under the lock before any signing.
        let pending = {
            let mut map = self.lock();
            map.remove(&key).ok_or(CeremonyError::UnknownCeremony)?
        };

        // Raw-ack gate (B1.2-ADV-5): re-insert on a missing ack so the human can
        // retry WITH it; this call broadcasts nothing.
        if pending.requires_raw_ack && !raw_ack {
            self.lock().insert(key, pending);
            return Err(CeremonyError::RawAckRequired);
        }

        // Decode the tx intent to signable fields (may still be undecodable at
        // this depth — e.g. calldata with no gas). The ceremony is ALREADY
        // consumed (fail-closed single-use): a decode/sign/broadcast error here
        // cannot re-approve this id.
        let (parsed, _display) = crate::txdecode::decode_transaction(&pending.intent.raw)
            .ok_or(CeremonyError::UndecodableTransaction)?;

        // F-2 (B1.5): assert the tx `from` is THIS vault's wallet address BEFORE
        // any nonce-fetch/sign. A dApp (or a spoofing origin) can name any `from`;
        // if it is not the address the vault key derives to, the signed tx would
        // be authored by the wrong sender and the node would reject it downstream
        // on a nonce/sender mismatch — an opaque failure with a live-RPC round
        // trip already spent. Catch it here with a CLEAR, key-free error and sign
        // NOTHING. The compare is case-insensitive (the wallet emits lowercase
        // hex; a dApp may send EIP-55 checksummed). An ABSENT `from` is left to
        // the nonce block below (a dApp that omits `from` must supply a nonce, or
        // it errors `UndecodableTransaction`) — we cannot mismatch what was not
        // claimed, and no wrong-sender tx can be produced without a `from`.
        if let Some(claimed) = parsed.from.as_deref() {
            let wallet = wallet::address(vault)?.address;
            if !claimed.eq_ignore_ascii_case(&wallet) {
                return Err(CeremonyError::FromMismatch {
                    claimed: claimed.to_string(),
                    wallet,
                });
            }
        }

        // Nonce + gas price from the LIVE RPC (Rule 1 — never hardcoded). The
        // `from` the dApp names sources the pending nonce; absent a `from` we
        // cannot fetch a nonce, so the dApp must have supplied one.
        let fetched_nonce = match (&parsed.nonce, &parsed.from) {
            (Some(n), _) => *n,
            (None, Some(from)) => rpc
                .pending_nonce(from)
                .map_err(|e| CeremonyError::Broadcast(e.to_string()))?,
            (None, None) => return Err(CeremonyError::UndecodableTransaction),
        };
        let fetched_gas_price = match &parsed.gas_price {
            Some(g) => *g,
            None => rpc
                .gas_price()
                .map_err(|e| CeremonyError::Broadcast(e.to_string()))?,
        };

        let fields = parsed
            .finalize(fetched_nonce, fetched_gas_price)
            .ok_or(CeremonyError::UndecodableTransaction)?;

        // Sign the REAL tx with the vault key (fails closed if locked — the
        // gated signer reads the sealed entropy via the A2 session gate).
        let signed = wallet::sign_transaction(vault, &fields, chain_id)?;

        // Broadcast + poll for inclusion. `send_raw_transaction` returns the
        // node-accepted hash; the receipt poll confirms block inclusion.
        let tx_hash = rpc
            .send_raw_transaction(&signed.raw)
            .map_err(|e| CeremonyError::Broadcast(e.to_string()))?;
        let receipt = rpc.poll_receipt(&tx_hash, poll_attempts, poll_interval);
        let block_number = match receipt {
            Ok(r) => Some(r.block_number),
            // The tx WAS accepted (we have a hash); the receipt just did not land
            // within the poll budget. Return the hash honestly with no block yet
            // rather than failing (the caller can re-poll). A hard RPC error on
            // the poll surfaces as a broadcast error.
            Err(crate::rpc::RpcError::ReceiptTimeout) => None,
            Err(e) => return Err(CeremonyError::Broadcast(e.to_string())),
        };

        Ok(BroadcastResult {
            tx_hash,
            block_number,
        })
    }

    /// **Step 2' — reject.** Consume the ceremony with no signature. Unknown id
    /// (never created / already consumed) → `UnknownCeremony`.
    pub fn reject(&self, id: &str) -> Result<()> {
        let key = Self::parse_id(id).ok_or(CeremonyError::UnknownCeremony)?;
        let mut map = self.lock();
        map.remove(&key)
            .map(|_| ())
            .ok_or(CeremonyError::UnknownCeremony)
    }

    /// Count of currently-pending ceremonies (test/inspection seam).
    #[cfg(test)]
    fn pending_count(&self) -> usize {
        self.lock().len()
    }
}

/// Decode the hex `raw` payload to bytes. Accepts an optional `0x` prefix. A
/// malformed hex payload yields `None` (approve maps it to `SignFailed`; nothing
/// is signed).
fn payload_bytes(raw: &str) -> Option<Vec<u8>> {
    let s = raw.strip_prefix("0x").unwrap_or(raw);
    hex::decode(s).ok()
}

/// Decode a `SignatureIntent` into a human-readable [`DecodedAction`] for the
/// approval surface. This is the anti-spoof surface (B1.2-ADV-5): it never
/// invents a benign summary — an undecodable payload yields
/// [`UNRECOGNIZED_ACTION`], which flags the ceremony `requires_raw_ack`.
///
/// B1.2 decodes the shapes it can prove:
/// - `personal_sign` — a UTF-8 message is shown verbatim (truncated); a
///   non-UTF-8 message is shown as its byte length + hex head (still decodable —
///   the human sees exactly what will be signed).
/// - `typed_data` — EIP-712 JSON is parsed; the `primaryType` + `domain.name`
///   (and `verifyingContract` as destination) are surfaced. Non-JSON typed data
///   is `Unrecognized` (raw-ack gated).
/// - `transaction` — the raw is treated as opaque tx bytes; B1.2 surfaces its
///   length + chain id but CANNOT yet decode `{to, value, calldata}` (that needs
///   the RLP/tx decoder deferred to B1.4), so a transaction is `Unrecognized`
///   and raw-ack gated — honestly forcing an explicit raw acknowledgement rather
///   than fabricating a benign action.
fn decode_intent(intent: &SignatureIntent) -> DecodedAction {
    match intent.kind {
        IntentKind::PersonalSign => decode_personal_sign(intent),
        IntentKind::TypedData => decode_typed_data(intent),
        IntentKind::Transaction => decode_transaction_intent(intent),
    }
}

/// Decode a `transaction` intent for the approval UI (B1.4). B1.2 blanket-marked
/// every transaction `Unrecognized`; B1.4 runs the real legacy-tx decoder
/// ([`crate::txdecode::decode_transaction`]) so the human sees `{action, cost,
/// destination}` for a legible tx. A payload we CANNOT decode to a legible tx
/// (not a JSON tx object, opaque contract-creation with no init code, malformed
/// fields) STILL returns `Unrecognized`, so undecodable calldata remains raw-ack
/// gated (Rule 1 / B1.2-ADV-5 — no fabricated benign summary).
fn decode_transaction_intent(intent: &SignatureIntent) -> DecodedAction {
    match crate::txdecode::decode_transaction(&intent.raw) {
        Some((_, display)) => DecodedAction {
            action: display.action,
            cost: display.cost,
            destination: display.destination,
        },
        None => unrecognized(),
    }
}

/// The undecodable decode — surfaces `Unrecognized`, which flags raw-ack.
fn unrecognized() -> DecodedAction {
    DecodedAction {
        action: UNRECOGNIZED_ACTION.to_string(),
        cost: String::new(),
        destination: String::new(),
    }
}

/// Longest message preview shown in the decoded action (avoid a huge blob in the
/// approval view; the human sees the head + a length note).
const MSG_PREVIEW: usize = 120;

fn decode_personal_sign(intent: &SignatureIntent) -> DecodedAction {
    let Some(bytes) = payload_bytes(&intent.raw) else {
        // Malformed hex → we cannot show what will be signed → Unrecognized.
        return unrecognized();
    };
    match std::str::from_utf8(&bytes) {
        Ok(text) => {
            let preview: String = text.chars().take(MSG_PREVIEW).collect();
            let suffix = if text.chars().count() > MSG_PREVIEW {
                "…"
            } else {
                ""
            };
            DecodedAction {
                action: format!("Sign message: \"{preview}{suffix}\""),
                cost: "no funds moved".to_string(),
                destination: intent.origin.clone(),
            }
        }
        Err(_) => DecodedAction {
            // Non-UTF-8 but still fully shown as bytes — the human sees exactly
            // what will be signed (length + hex head), so this is DECODABLE (not
            // raw-gated): a binary personal_sign is a legitimate, showable action.
            action: format!("Sign {} raw bytes (binary message)", bytes.len()),
            cost: "no funds moved".to_string(),
            destination: intent.origin.clone(),
        },
    }
}

fn decode_typed_data(intent: &SignatureIntent) -> DecodedAction {
    let Some(bytes) = payload_bytes(&intent.raw) else {
        return unrecognized();
    };
    let Ok(json) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        // Typed data that is not JSON cannot be summarized → raw-ack gated.
        return unrecognized();
    };
    let primary = json
        .get("primaryType")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let domain_name = json
        .get("domain")
        .and_then(|d| d.get("name"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let verifying = json
        .get("domain")
        .and_then(|d| d.get("verifyingContract"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if primary.is_empty() {
        // No primaryType → not a recognizable EIP-712 payload → raw-ack gated.
        return unrecognized();
    }
    let action = if domain_name.is_empty() {
        format!("Sign typed data ({primary})")
    } else {
        format!("Sign typed data: {primary} on {domain_name}")
    };
    DecodedAction {
        action,
        cost: "no funds moved".to_string(),
        destination: verifying.to_string(),
    }
}

// ---------------------------------------------------------------------------
// Managed Tauri state + command surface (I-2: no secret crosses invoke).
// ---------------------------------------------------------------------------

/// Managed Tauri state: the process-wide SignatureCeremony.
pub struct CeremonyState(pub SignatureCeremony);

/// Build the managed ceremony state (empty, ready to accept intents).
pub fn build_ceremony_state() -> CeremonyState {
    CeremonyState(SignatureCeremony::new())
}

use tauri::State;

fn err_str(e: CeremonyError) -> String {
    e.to_string()
}

/// **Command — sign_request.** Submit a [`SignatureIntent`]; returns a PENDING
/// [`CeremonyView`] (id + true origin + decoded action). Signs NOTHING and
/// returns NO key material. Any origin (user / agent / micro-app) reaches the
/// signer ONLY through this door: it produces an intent → a ceremony, never a
/// signature (B1.2-ADV-1/7).
#[tauri::command]
pub fn sign_request(
    ceremony: State<'_, CeremonyState>,
    intent: SignatureIntent,
) -> std::result::Result<CeremonyView, String> {
    Ok(ceremony.0.request(intent))
}

/// **Command — sign_approve.** The ONLY command that yields a signature, and it
/// does so ONLY by consuming a pending ceremony bound to `id` (no "approve
/// latest", no auto-approve — B1.2-ADV-6). `rawAck` MUST be set explicitly to
/// approve an undecodable-calldata ceremony (B1.2-ADV-5). Returns the signature
/// hex ONLY — never key/seed/entropy (I-2). Fails closed if the vault is locked
/// (B1.2-ADV-3).
#[tauri::command]
pub fn sign_approve(
    ceremony: State<'_, CeremonyState>,
    custody: State<'_, crate::custody::CustodyState>,
    id: String,
    raw_ack: bool,
) -> std::result::Result<Signature, String> {
    ceremony
        .0
        .approve(&custody.0, &id, raw_ack)
        .map_err(err_str)
}

/// **Command — sign_and_broadcast (B1.4).** The ONLY command that produces a
/// REAL, broadcast 40204 transaction. It consumes a pending `transaction`
/// ceremony bound to `id` (no auto-approve / no "approve latest"), signs the real
/// EIP-155 legacy tx with the vault key, broadcasts to the live 40204 RPC, polls
/// the receipt, and returns the PUBLIC [`BroadcastResult`] (tx hash + block) —
/// NEVER key/seed/entropy (I-2). `rawAck` must be set explicitly to approve an
/// undecodable-calldata ceremony (B1.2-ADV-5); fails closed if the vault is
/// locked (B1.2-ADV-3). All B1.2 single-use/consume-first invariants hold.
#[tauri::command]
pub fn sign_and_broadcast(
    ceremony: State<'_, CeremonyState>,
    custody: State<'_, crate::custody::CustodyState>,
    id: String,
    raw_ack: bool,
) -> std::result::Result<BroadcastResult, String> {
    // Production wiring: the live 40204 RPC client + chain id, with a sane
    // receipt-poll budget (30 attempts × 2s = up to 60s for inclusion). Blocking
    // HTTP is correct here — the command runs off the async runtime.
    let rpc = crate::rpc::RpcClient::citrate();
    ceremony
        .0
        .approve_and_broadcast(
            &custody.0,
            &rpc,
            &id,
            raw_ack,
            BroadcastConfig {
                chain_id: crate::rpc::CITRATE_CHAIN_ID,
                poll_attempts: 30,
                poll_interval: std::time::Duration::from_secs(2),
            },
        )
        .map_err(err_str)
}

/// **Command — sign_reject.** Consume a pending ceremony with no signature.
/// Unknown/already-consumed id → error.
#[tauri::command]
pub fn sign_reject(
    ceremony: State<'_, CeremonyState>,
    id: String,
) -> std::result::Result<(), String> {
    ceremony.0.reject(&id).map_err(err_str)
}

#[cfg(test)]
mod tests {
    include!("ceremony_tests.rs");
}
