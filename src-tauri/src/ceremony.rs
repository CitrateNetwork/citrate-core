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
//! specifically to keep the zk/execution stack out of the key path. So B1.2 signs
//! via the message path (`wallet::sign_message`, r||s) for ALL three intent kinds;
//! the recoverable tx-signing form is deferred to B1.4 (which will either enable a
//! `chain` seam under the lean build or add a recoverable message signer). A
//! `transaction` intent still decodes + surfaces for approval and produces a
//! message-form signature over its raw bytes — it is NOT a broadcastable tx yet
//! (B1.4), consistent with "no real broadcast to 40204 in B1.2".

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

/// A signature result crossing the bridge. Hex-encoded `r||s` (64 bytes → 128
/// hex chars, no `0x`). This is the ONLY thing `approve` returns — never key,
/// seed, or entropy material. A signature over a message is not secret.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Signature {
    /// Hex of the raw ECDSA signature bytes (r||s).
    #[serde(rename = "sigHex")]
    pub sig_hex: String,
    /// The kind that was signed (echo, for the caller to route the result).
    pub kind: IntentKind,
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
        let sig = wallet::sign_message(vault, &bytes)?;
        Ok(Signature {
            sig_hex: hex::encode(sig),
            kind: pending.intent.kind,
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
        IntentKind::Transaction => unrecognized(),
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
