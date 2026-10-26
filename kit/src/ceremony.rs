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
//! ## Lifetime and chain binding
//! A pending request is approvable for [`PENDING_TTL`]. Time is measured on both the monotonic
//! and the wall clock and the larger elapsed value counts, so setting the wall clock back cannot
//! extend a request and a machine that slept past the TTL finds it expired on wake. Expired
//! requests are removed (their ids are remembered as expired, so a late approval says so) and
//! the pending set is never written to disk, so a restart drops every request. Ids start at a
//! random base per process, so an id from an earlier run names nothing in a later one.
//!
//! The chain id on the approval card ([`CeremonyView::chain_id`]) is the request's
//! [`SignatureIntent::chain_id`], and it is the value the signature commits to: immediately
//! before each signer call, `ready_to_sign` re-checks the TTL and compares the card's chain id
//! with every chain id the bytes to be signed name (the EIP-155 chain of a transaction and any
//! `chainId` in its payload; any `Chain ID:` line of a message). A mismatch, or a value that
//! cannot be read, refuses with [`CeremonyError::ChainMismatch`] and nothing is signed.
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
//! `v` in {27,28} — which is what the name always claimed. PBA-L4-007: `typed_data`
//! (and a `transaction` on the plain `approve` path) is now REFUSED with
//! `UnsupportedSigningKind` instead of signing SHA-256 of the raw bytes: EIP-712
//! needs its own domain-separated hashing, and approximating it silently was a
//! signing oracle under a misleading label.
//!
//! ## The one budgeted exception (HUP-S2.3, ADR-2026-09-30-rule3-budgetable-signatures)
//! [`SignatureCeremony::request_siwe_budgeted`] may sign a hardened Sign-In with Ethereum message
//! (EIP-4361, `personal_sign`) WITHOUT a per-signature click, but only inside a budget the member
//! granted in Settings → Budgets, for a core-attested top-frame origin, under the dedicated budget
//! lock, with the decision record written before the signature. It takes message text only, so no
//! transaction, permit or other typed data can reach it; everything that fails a check becomes an
//! ordinary pending ceremony here. The generic `SessionBudget` below stays dormant (CORE-G2).

// This module is consumed by the B1.2 command surface in `lib.rs` (wired) and by
// the B1.3 wagmi connector later. Some constructors/fields are part of the stable
// surface but only exercised by tests until B1.3; keep the non-test lib build
// quiet exactly like `wallet.rs`/`custody.rs` do for their staged consumers.
#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::custody::CustodyVault;
use crate::wallet::{self, WalletError};

/// HUP-S7.3: the anchor ceremony, a single-purpose signer for the nightly anchor with its own
/// no-funds key (Rule-3 ADR D5). It never touches the wallet key.
pub mod anchor;

/// How long a pending signature request stays approvable: 15 minutes.
///
/// Nonce and gas are read live at approval, so this bounds only how long a card can wait for the
/// member. It equals the approval window of the external MCP inbox
/// (`node_mcp_approvals::REQUEST_TTL_MS` is defined from it), so a request and its card expire
/// together.
pub const PENDING_TTL: std::time::Duration = std::time::Duration::from_millis(PENDING_TTL_MS);

/// [`PENDING_TTL`] in milliseconds.
pub const PENDING_TTL_MS: u64 = 15 * 60 * 1000;

/// One reading of the ceremony clock: monotonic time plus wall-clock epoch ms.
#[derive(Debug, Clone, Copy)]
pub struct ClockReading {
    /// Monotonic time. Never goes back, but may not advance while the machine sleeps.
    pub mono: std::time::Instant,
    /// Wall-clock epoch ms. Advances across sleep, but can be set back.
    pub wall_ms: u64,
}

/// The ceremony's clock (injected so tests can move time).
pub type CeremonyClock = Box<dyn Fn() -> ClockReading + Send + Sync>;

/// The system clock: `Instant::now()` and the wall clock (0 if it reads before the epoch).
pub fn system_clock() -> ClockReading {
    ClockReading {
        mono: std::time::Instant::now(),
        wall_ms: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
            .unwrap_or(0),
    }
}

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

/// S7.5 (RT-5) — a scoped, session-lived authorization a member approves ONCE so a social node's
/// many routine signatures (co-pins, cluster joins) do not each demand a fresh per-tx prompt.
///
/// It does NOT replace the ceremony signer and does NOT hold a key: a covered intent is still signed
/// by `wallet::sign_message` through the ONE gated path ([`SignatureCeremony::approve`]). The budget
/// only substitutes the member's ONE prior *scoped* approval for the per-tx human prompt — a
/// deliberate, bounded relaxation of the single-use property, documented in
/// `docs/adr/ADR-2026-08-29-session-ceremony-budget.md`. Default is **no budget** = today's behavior
/// exactly: every intent prompts. Fail-closed: an intent is covered only if EVERY bound holds.
///
/// NOTE: this is the vetted PRIMITIVE. It is NOT yet consulted by `request`/`approve` — wiring the
/// auto-approve path into the live signer lands only after the ADR + a security sign-off (@rule8, T1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionBudget {
    /// Origins whose intents this budget covers (matched verbatim against `SignatureIntent::origin`).
    pub origins: Vec<String>,
    /// Intent kinds this budget covers.
    pub kinds: Vec<IntentKind>,
    /// The chain this budget is scoped to (an intent on another chain is never covered).
    pub chain_id: u64,
    /// The maximum number of signatures this one approval authorizes.
    pub max_ops: u32,
    /// Wall-clock expiry (epoch ms). After this, the budget covers nothing.
    pub expires_at_ms: u64,
    /// Signatures consumed so far (starts 0).
    used_ops: u32,
}

impl SessionBudget {
    /// Establish a budget (as a member-signed policy would). `used_ops` starts at 0.
    pub fn new(
        origins: Vec<String>,
        kinds: Vec<IntentKind>,
        chain_id: u64,
        max_ops: u32,
        expires_at_ms: u64,
    ) -> Self {
        SessionBudget {
            origins,
            kinds,
            chain_id,
            max_ops,
            expires_at_ms,
            used_ops: 0,
        }
    }

    /// Whether this budget covers `intent` at `now_ms` — EVERY bound must hold (fail-closed): not
    /// expired, ops remaining, chain matches, origin allowed, kind allowed. A covered intent may be
    /// signed without a fresh human prompt (the budget IS the authorization).
    pub fn covers(&self, intent: &SignatureIntent, now_ms: u64) -> bool {
        now_ms < self.expires_at_ms
            && self.used_ops < self.max_ops
            && intent.chain_id == self.chain_id
            && self.origins.iter().any(|o| o == &intent.origin)
            && self.kinds.contains(&intent.kind)
    }

    /// Consume one op after a covered signature. Returns the remaining count, or `Err` if the budget
    /// is already exhausted (a defensive guard — `covers` should have gated this).
    pub fn consume(&mut self) -> std::result::Result<u32, &'static str> {
        if self.used_ops >= self.max_ops {
            return Err("session budget exhausted");
        }
        self.used_ops += 1;
        Ok(self.max_ops - self.used_ops)
    }

    /// Signatures still authorized (0 once exhausted).
    pub fn remaining(&self) -> u32 {
        self.max_ops.saturating_sub(self.used_ops)
    }

    /// Whether the budget has expired at `now_ms`.
    pub fn is_expired(&self, now_ms: u64) -> bool {
        now_ms >= self.expires_at_ms
    }
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
    /// HUP-S7.5 (D-27): the mined receipt's status (1 succeeded, 0 reverted), once known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<u64>,
    /// HUP-S7.5 (D-27): the gas the mined transaction used, once known.
    #[serde(rename = "gasUsed", default, skip_serializing_if = "Option::is_none")]
    pub gas_used: Option<u64>,
    /// HUP-S7.5 (D-27): wei per gas actually paid (decimal), once known.
    #[serde(
        rename = "effectiveGasPriceWei",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub effective_gas_price_wei: Option<String>,
    /// HUP-S7.5 (D-27): the SALT value the transaction carried (wei, decimal).
    #[serde(rename = "valueWei", default, skip_serializing_if = "Option::is_none")]
    pub value_wei: Option<String>,
    /// The recipient (`0x` + 40 hex), `None` for a contract creation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    /// The origin the ceremony displayed (who asked for the transaction).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub origin: String,
}

/// HUP-S7.5 (D-27): something told about every transaction [`sign_and_broadcast`] sent. The app
/// installs one that hands Hermes's own transactions (by origin) to its metering, so the daily
/// report carries their gas and SALT. It sees public facts only and cannot change the result.
pub type BroadcastObserver = Box<dyn Fn(&BroadcastResult) + Send + Sync>;

static BROADCAST_OBSERVER: std::sync::OnceLock<BroadcastObserver> = std::sync::OnceLock::new();

/// Install the broadcast observer (once per process; a second call is refused and returns false).
pub fn set_broadcast_observer(f: BroadcastObserver) -> bool {
    BROADCAST_OBSERVER.set(f).is_ok()
}

fn notify_broadcast(r: &BroadcastResult) {
    if let Some(f) = BROADCAST_OBSERVER.get() {
        f(r);
    }
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
    /// PBA-L4-007: this intent kind has no honest signer on the `approve` path
    /// (`typed_data` needs EIP-712 hashing; a `transaction` is signed only by
    /// `approve_and_broadcast` as a real EIP-155 tx). Nothing was signed.
    UnsupportedSigningKind,
    /// The signed tx could not be broadcast / confirmed on 40204 (B1.4). Carries
    /// the RPC error's PUBLIC message (node reason / transport / timeout) — never
    /// key material (the broadcast client never sees a key).
    Broadcast(String),
    /// K6: the network gate refused the signature before the wallet key was read: this build
    /// has no network set, or the request commits to another chain id. Nothing was signed.
    WrongNetwork(String),
    /// The signed transaction was handed to the node and may have been accepted, but the outcome
    /// is not known: the node accepted it and the receipt poll then failed, or the send itself
    /// failed in transport (the node may have received it). Carries the tx hash so the member
    /// can look it up before trying again; never reported as "not sent".
    SubmittedOutcomeUnknown { tx_hash: String, reason: String },
    /// The request was not approved within [`PENDING_TTL`]. It has been removed. Nothing was
    /// signed.
    Expired,
    /// The chain id shown on the approval card is not the chain id the signature would commit
    /// to. Nothing was signed.
    ChainMismatch {
        /// The chain id the card showed.
        shown: u64,
        /// The chain id the bytes to be signed name (or why it could not be read).
        signed: String,
    },
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
            CeremonyError::UnsupportedSigningKind => write!(
                f,
                "ceremony: this request type cannot be signed here (typed data needs EIP-712 hashing, which Citrate Core does not implement yet; transactions are signed only when broadcast)"
            ),
            CeremonyError::Broadcast(m) => write!(f, "ceremony: broadcast failed: {m}"),
            CeremonyError::WrongNetwork(m) => write!(f, "ceremony: {m}; nothing was signed"),
            CeremonyError::SubmittedOutcomeUnknown { tx_hash, reason } => {
                write!(f, "ceremony: submitted, outcome unknown (tx {tx_hash}): {reason}")
            }
            CeremonyError::Expired => write!(
                f,
                "ceremony: this request expired before it was approved, so nothing was signed; ask again for a fresh request"
            ),
            CeremonyError::ChainMismatch { shown, signed } => write!(
                f,
                "ceremony: the approval showed chain {shown} but the request is for chain {signed}, so nothing was signed"
            ),
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
            WalletError::WrongNetwork(m) => CeremonyError::WrongNetwork(m),
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
    /// When the request was opened. Kept on a raw-ack re-insert, so a retry never restarts the
    /// TTL.
    created: ClockReading,
}

/// How many expired ids are remembered (oldest forgotten first). Only used to tell "expired"
/// from "unknown" in a late approval's error.
const EXPIRED_IDS_KEPT: usize = 4096;

/// The pending requests, plus the ids that expired unapproved.
#[derive(Default)]
struct Book {
    pending: BTreeMap<u64, Pending>,
    expired: BTreeSet<u64>,
}

impl Book {
    fn remember_expired(&mut self, key: u64) {
        self.expired.insert(key);
        while self.expired.len() > EXPIRED_IDS_KEPT {
            self.expired.pop_first();
        }
    }
}

/// The process-wide SignatureCeremony: the single approval surface. Holds the
/// pending ceremonies behind a mutex and mints monotonic ids. All signing goes
/// through [`approve`](Self::approve); no other path in the crate reaches the
/// gated `wallet::sign_message`.
pub struct SignatureCeremony {
    book: Mutex<Book>,
    /// keccak256 hashes of the signed transactions this app has handed to
    /// `eth_sendRawTransaction`, persisted across restarts ([`crate::submitted_log`]). A node error
    /// on a RE-submission of the same bytes never proves the tx was refused (the first may have
    /// been accepted and mined), so only a first submission can be "not sent" (contract v1.1 §1).
    submitted: std::sync::Arc<crate::submitted_log::SubmittedTxLog>,
    next_id: AtomicU64,
    /// The first id this ceremony hands out (random per process).
    first_id: u64,
    /// HUP-S6: ABIs of contracts deployed from a READY-gated artifact (see [`crate::abi_book`]).
    abis: crate::abi_book::AbiBook,
    /// How long a pending request stays approvable.
    ttl: std::time::Duration,
    /// The clock the TTL is measured with.
    clock: CeremonyClock,
}

impl Default for SignatureCeremony {
    fn default() -> Self {
        Self::new()
    }
}

impl SignatureCeremony {
    pub fn new() -> Self {
        Self::with_clock(PENDING_TTL, Box::new(system_clock))
    }

    /// A ceremony whose pending requests expire after `ttl`, measured with `clock`.
    pub fn with_clock(ttl: std::time::Duration, clock: CeremonyClock) -> Self {
        let first_id = random_first_id();
        SignatureCeremony {
            book: Mutex::new(Book::default()),
            // No record location: uncertain, so no send through it is ever "not sent" (C1-V6).
            submitted: std::sync::Arc::new(crate::submitted_log::SubmittedTxLog::unpersisted()),
            next_id: AtomicU64::new(first_id),
            first_id,
            abis: crate::abi_book::AbiBook::default(),
            ttl,
            clock,
        }
    }

    /// Whether `id` named a request that expired unapproved (and was removed).
    pub fn is_expired(&self, id: &str) -> bool {
        let Some(key) = Self::parse_id(id) else {
            return false;
        };
        self.live().expired.contains(&key)
    }

    /// How many requests this ceremony has opened.
    pub fn opened(&self) -> u64 {
        self.next_id.load(Ordering::SeqCst) - self.first_id
    }

    /// Whether a request opened at `created` has outlived the TTL at `now`. The larger of the
    /// monotonic and the wall-clock elapsed time counts: a wall clock set back cannot extend a
    /// request, and sleep (which the monotonic clock may not count) still expires it.
    fn expired(&self, created: &ClockReading, now: &ClockReading) -> bool {
        let mono = now.mono.saturating_duration_since(created.mono);
        let wall = std::time::Duration::from_millis(now.wall_ms.saturating_sub(created.wall_ms));
        mono.max(wall) >= self.ttl
    }

    /// Lock the book and remove every expired request first, so nothing below the lock can see
    /// one. The clock is read under the lock, which orders an expiry against a concurrent
    /// consume.
    fn live(&self) -> std::sync::MutexGuard<'_, Book> {
        let mut book = self.lock();
        let now = (self.clock)();
        let stale: Vec<u64> = book
            .pending
            .iter()
            .filter(|(_, p)| self.expired(&p.created, &now))
            .map(|(k, _)| *k)
            .collect();
        for key in stale {
            book.pending.remove(&key);
            book.remember_expired(key);
        }
        book
    }

    /// Consume a live request: under one lock, expired requests are removed and then `key` is
    /// taken out. An expired id answers [`CeremonyError::Expired`]; an unknown or already
    /// consumed one [`CeremonyError::UnknownCeremony`].
    fn take_live(&self, key: u64) -> Result<Pending> {
        let mut book = self.live();
        if book.expired.contains(&key) {
            return Err(CeremonyError::Expired);
        }
        book.pending
            .remove(&key)
            .ok_or(CeremonyError::UnknownCeremony)
    }

    /// The last step before a wallet signer is called. Re-checks the TTL (time passes between
    /// the consume and here, e.g. while the nonce is read) and binds the chain: every chain id
    /// the bytes to be signed name must equal the chain id the card showed. Returns that chain
    /// id, which is the one a transaction is signed for. `signer_chain` is the EIP-155 chain id
    /// the transaction signer would use (`None` for a message).
    fn ready_to_sign(&self, key: u64, p: &Pending, signer_chain: Option<u64>) -> Result<u64> {
        if self.expired(&p.created, &(self.clock)()) {
            self.lock().remember_expired(key);
            return Err(CeremonyError::Expired);
        }
        let shown = p.intent.chain_id;
        let named = committed_chain(&p.intent, signer_chain)
            .map_err(|signed| CeremonyError::ChainMismatch { shown, signed })?;
        if let Some(other) = named.into_iter().find(|c| *c != shown) {
            return Err(CeremonyError::ChainMismatch {
                shown,
                signed: other.to_string(),
            });
        }
        Ok(shown)
    }

    /// A ceremony whose first-submission record is `log` (production passes one persisted under
    /// the app data dir, so the record survives a restart).
    pub fn with_submitted_log(log: crate::submitted_log::SubmittedTxLog) -> Self {
        let mut c = Self::new();
        c.submitted = std::sync::Arc::new(log);
        c
    }

    /// The shared first-submission record, for other signers of this app (e.g. the anchor).
    pub fn submitted_log(&self) -> std::sync::Arc<crate::submitted_log::SubmittedTxLog> {
        self.submitted.clone()
    }

    /// HUP-S6: let calls to `address` be decoded from `abi`. Only for a contract whose on-chain
    /// code is exactly a READY-gated artifact's runtime code (core's `postdeploy` checks both).
    pub fn register_gated_contract(&self, address: [u8; 20], abi: crate::abi_book::ContractAbi) {
        self.abis.register(address, abi);
    }

    /// Gated contracts registered.
    pub fn gated_contracts(&self) -> usize {
        self.abis.len()
    }

    /// [`decode_intent`], plus: a transaction calling a registered gated contract whose call is
    /// not otherwise understood is decoded from that contract's ABI. Anything the ABI does not
    /// decode exactly stays [`UNRECOGNIZED_ACTION`] (raw-ack gated).
    fn decode_with_book(&self, intent: &SignatureIntent) -> DecodedAction {
        let decoded = decode_intent(intent);
        if intent.kind != IntentKind::Transaction || decoded.action != UNRECOGNIZED_ACTION {
            return decoded;
        }
        let Some((tx, display)) = crate::txdecode::decode_transaction(&intent.raw) else {
            return decoded;
        };
        let Some(to) = tx.to else {
            return decoded;
        };
        let Some(abi) = self.abis.get(&to) else {
            return decoded;
        };
        match abi.decode_call(&tx.data, tx.value) {
            Some(call) => DecodedAction {
                action: format!(
                    "Call {call} on {} at {} (a contract you deployed from a READY gated build; value {} wei)",
                    abi.name, display.destination, tx.value
                ),
                cost: display.cost,
                destination: display.destination,
            },
            None => decoded,
        }
    }

    /// Lock the pending map, recovering from poison (a panic mid-op must not
    /// brick the signing surface). The only invariant under the lock is the
    /// pending map; a poisoned guard yields it intact.
    fn lock(&self) -> std::sync::MutexGuard<'_, Book> {
        self.book.lock().unwrap_or_else(|e| e.into_inner())
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
        let decoded = self.decode_with_book(&intent);
        let requires_raw_ack = decoded.action == UNRECOGNIZED_ACTION;
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let created = (self.clock)();
        let view = CeremonyView {
            id: id.to_string(),
            origin: intent.origin.clone(),
            kind: intent.kind,
            chain_id: intent.chain_id,
            decoded: decoded.clone(),
            requires_raw_ack,
        };
        self.live().pending.insert(
            id,
            Pending {
                intent,
                decoded,
                requires_raw_ack,
                created,
            },
        );
        view
    }

    /// Re-inspect a pending ceremony without consuming it (for the approval UI to
    /// re-render). `None` if it does not exist / was consumed.
    pub fn status(&self, id: &str) -> Option<CeremonyView> {
        let key = Self::parse_id(id)?;
        let book = self.live();
        let p = book.pending.get(&key)?;
        Some(CeremonyView {
            id: id.to_string(),
            origin: p.intent.origin.clone(),
            kind: p.intent.kind,
            chain_id: p.intent.chain_id,
            decoded: p.decoded.clone(),
            requires_raw_ack: p.requires_raw_ack,
        })
    }

    /// **Step 2 — approve.** The message-signing path. Consumes the ceremony
    /// (single-use), then signs `personal_sign` via the gated EIP-191
    /// `wallet::sign_personal`; `typed_data` / `transaction` are refused
    /// (PBA-L4-007, see `UnsupportedSigningKind`).
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
        // approve cannot find the same ceremony a second time. An expired request
        // is refused here.
        let pending = self.take_live(key)?;

        // Raw-ack gate (B1.2-ADV-5): undecodable calldata is never approvable as
        // if benign. On a missing ack, re-insert (with its original creation time,
        // so the TTL is not restarted) so the human can retry WITH the ack — but
        // this call signs nothing.
        if pending.requires_raw_ack && !raw_ack {
            self.lock().pending.insert(key, pending);
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
            IntentKind::PersonalSign => {
                self.ready_to_sign(key, &pending, None)?;
                wallet::sign_personal(vault, &bytes)?.to_vec()
            }
            // PBA-L4-007: TypedData and Transaction are REFUSED here. They used to be
            // signed as SHA-256(raw dApp bytes) with the wallet key — no EIP-712 domain
            // separation, no chain id — under a UI label that said "Sign typed data":
            // a signing oracle with a misleading label, producing a signature no
            // EIP-712 verifier accepts. EIP-712 needs its own domain-separated hashing
            // (not implemented under the lean `crypto` build), and a transaction is
            // signed as a real EIP-155 tx only by `approve_and_broadcast`. The ceremony
            // is already consumed, so the refused request cannot be retried blind.
            IntentKind::TypedData | IntentKind::Transaction => {
                return Err(CeremonyError::UnsupportedSigningKind);
            }
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
        self.approve_and_broadcast_gated(vault, rpc, id, raw_ack, cfg, crate::rpc::network_gate())
    }

    /// [`Self::approve_and_broadcast`] against an explicit network gate (K6). With `Some`, the
    /// RPC's chain id and block-0 hash are checked after the ceremony is consumed and BEFORE any
    /// nonce read or signature: on a mismatch, or a gate with no values set, nothing is signed.
    pub fn approve_and_broadcast_gated<T: crate::rpc::RpcTransport>(
        &self,
        vault: &CustodyVault,
        rpc: &crate::rpc::RpcClient<T>,
        id: &str,
        raw_ack: bool,
        cfg: BroadcastConfig,
        gate: Option<&crate::rpc::ExpectedNetwork>,
    ) -> Result<BroadcastResult> {
        let BroadcastConfig {
            chain_id,
            poll_attempts,
            poll_interval,
        } = cfg;
        let key = Self::parse_id(id).ok_or(CeremonyError::UnknownCeremony)?;

        // Consume-first (B1.2-ADV-10): remove under the lock before any signing.
        // An expired request is refused here, before any RPC call.
        let pending = self.take_live(key)?;

        // Raw-ack gate (B1.2-ADV-5): re-insert on a missing ack (keeping its
        // creation time) so the human can retry WITH it; this call broadcasts nothing.
        if pending.requires_raw_ack && !raw_ack {
            self.lock().pending.insert(key, pending);
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

        // K6: the endpoint must be the network this build targets before anything is read for
        // or signed into the transaction. A same-chain-id network with another genesis would
        // accept the signature, so the block-0 hash is what is checked.
        if let Some(expected) = gate {
            rpc.verify_network(expected)
                .map_err(|e| CeremonyError::Broadcast(format!("{e}; nothing was signed")))?;
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

        // Last check, then sign the REAL tx with the vault key (fails closed if
        // locked — the gated signer reads the sealed entropy via the A2 session
        // gate). The chain id signed is the one the card showed, after checking it
        // equals the signer's chain and any chainId the payload names.
        let bound_chain = self.ready_to_sign(key, &pending, Some(chain_id))?;
        let signed = wallet::sign_transaction(vault, &fields, bound_chain)?;

        // Broadcast + poll for inclusion. `send_raw_transaction` returns the
        // node-accepted hash; the receipt poll confirms block inclusion.
        //
        // UI truth contract v1.1 §1: a send error proves the node refused the tx ONLY when it is
        // on the definitive list AND this is the first submission of these signed bytes. Anything
        // else (a transport failure, an unlisted or ambiguous message, any error on a re-send) is
        // an unknown outcome carrying keccak256 of the signed raw tx, which is the tx hash.
        let local_hash = {
            use sha3::{Digest, Keccak256};
            format!("0x{}", hex::encode(Keccak256::digest(&signed.raw)))
        };
        let sender = wallet::address(vault)?.address;
        let first_submission = self
            .submitted
            .try_record_for(&local_hash, &sender, fields.nonce)
            .map_err(|e| {
                CeremonyError::Broadcast(format!(
                    "the submission could not be recorded durably, so nothing was sent: {e}"
                ))
            })?;
        let tx_hash = match rpc.send_raw_transaction_gated(&signed.raw, gate) {
            Ok(h) => h,
            // Definitive only for a first submission that no other attempt overlapped (#347 P1).
            Err(e)
                if first_submission
                    && self.submitted.sole_attempt(&local_hash)
                    && is_definitive_send_error(&e) =>
            {
                return Err(CeremonyError::Broadcast(e.to_string()))
            }
            Err(e) => {
                return Err(CeremonyError::SubmittedOutcomeUnknown {
                    tx_hash: local_hash,
                    reason: e.to_string(),
                })
            }
        };
        let receipt = rpc.poll_receipt(&tx_hash, poll_attempts, poll_interval);
        let mined = match receipt {
            Ok(r) => Some(r),
            // The tx WAS accepted (we have a hash); the receipt just did not land
            // within the poll budget. Return the hash honestly with no block yet
            // rather than failing (the caller can re-poll).
            Err(crate::rpc::RpcError::ReceiptTimeout) => None,
            // A hard RPC error on the poll comes AFTER the node accepted the tx: the outcome is
            // unknown, never "not sent" (C1-V #347 review).
            Err(e) => {
                return Err(CeremonyError::SubmittedOutcomeUnknown {
                    tx_hash,
                    reason: e.to_string(),
                })
            }
        };

        Ok(BroadcastResult {
            tx_hash,
            block_number: mined.as_ref().map(|r| r.block_number),
            status: mined.as_ref().and_then(|r| r.status),
            gas_used: mined.as_ref().and_then(|r| r.gas_used),
            effective_gas_price_wei: mined
                .as_ref()
                .and_then(|r| r.effective_gas_price)
                .map(|p| p.to_string()),
            value_wei: Some(parsed.value.to_string()),
            to: parsed.to.map(|a| format!("0x{}", hex::encode(a))),
            origin: pending.intent.origin.clone(),
        })
    }

    /// **Step 2' — reject.** Consume the ceremony with no signature. Unknown id
    /// (never created / already consumed) → `UnknownCeremony`.
    pub fn reject(&self, id: &str) -> Result<()> {
        let key = Self::parse_id(id).ok_or(CeremonyError::UnknownCeremony)?;
        self.take_live(key).map(|_| ())
    }

    /// Count of currently-pending ceremonies (test/inspection seam).
    #[cfg(test)]
    fn pending_count(&self) -> usize {
        self.live().pending.len()
    }
}

/// A random first id for a new ceremony, so ids from an earlier run (before a restart) never
/// name a request in this one. Ids stay below 2^53 so they survive a JavaScript number.
fn random_first_id() -> u64 {
    use rand::RngCore;
    let mut b = [0u8; 4];
    let r = match rand::rngs::OsRng.try_fill_bytes(&mut b) {
        Ok(()) => u32::from_le_bytes(b),
        // No OS randomness: fall back to the wall clock, which still differs between runs.
        Err(_) => (system_clock().wall_ms & 0xffff_ffff) as u32,
    };
    // 31 random bits shifted by 20: a base in [2^20, 2^51), with 2^20 ids per run before the
    // next base could be reached.
    (u64::from(r >> 1) + 1) << 20
}

/// Every chain id the bytes to be signed commit to, for [`SignatureCeremony::ready_to_sign`].
/// `Err` carries a description of a chain value that could not be read or is ambiguous, which
/// refuses the signature.
/// - A transaction commits to the EIP-155 chain id the signer uses (`signer_chain`, required)
///   and to any `chainId` its payload names.
/// - A `personal_sign` message commits to the chain on any `Chain ID:` line (Sign-In with
///   Ethereum); a message with none commits to no chain.
/// - Typed data is never signed here.
fn committed_chain(
    intent: &SignatureIntent,
    signer_chain: Option<u64>,
) -> std::result::Result<Vec<u64>, String> {
    match intent.kind {
        IntentKind::Transaction => {
            let signer = signer_chain.ok_or_else(|| "unknown (no signer chain)".to_string())?;
            let mut out = vec![signer];
            if let Some(named) = payload_chain_id(&intent.raw)? {
                out.push(named);
            }
            Ok(out)
        }
        IntentKind::PersonalSign => {
            let bytes = payload_bytes(&intent.raw).ok_or_else(|| "unreadable".to_string())?;
            message_chain_ids(&bytes)
        }
        IntentKind::TypedData => Err("typed data (not signed here)".to_string()),
    }
}

/// The `chainId` a transaction payload names, if any. A number, a `0x` hex string or a decimal
/// string is read; anything else present under the key is unreadable.
fn payload_chain_id(raw: &str) -> std::result::Result<Option<u64>, String> {
    let json: serde_json::Value =
        serde_json::from_str(raw).map_err(|_| "unreadable payload".to_string())?;
    let Some(v) = json.get("chainId") else {
        return Ok(None);
    };
    let parsed = match v {
        serde_json::Value::Number(n) => n.as_u64(),
        serde_json::Value::String(t) => match t.strip_prefix("0x").or_else(|| t.strip_prefix("0X"))
        {
            Some(h) if !h.is_empty() => u64::from_str_radix(h, 16).ok(),
            Some(_) => None,
            None => t.parse::<u64>().ok(),
        },
        _ => None,
    };
    parsed.map(Some).ok_or_else(|| format!("unreadable ({v})"))
}

/// The chain ids on a message's `Chain ID:` lines (trimmed, case-insensitive; the same rule the
/// network gate in `wallet` applies to Sign-In with Ethereum messages). A line whose value is not
/// a decimal u64, or two lines naming different chains, are refused. Non-UTF-8 bytes name no
/// chain.
fn message_chain_ids(message: &[u8]) -> std::result::Result<Vec<u64>, String> {
    let Ok(text) = std::str::from_utf8(message) else {
        return Ok(Vec::new());
    };
    let mut found: Option<u64> = None;
    for line in text.lines() {
        let t = line.trim();
        let Some(head) = t.get(..9) else { continue };
        if !head.eq_ignore_ascii_case("chain id:") {
            continue;
        }
        let value = t[9..].trim();
        let id = value
            .parse::<u64>()
            .map_err(|_| format!("unreadable ({value})"))?;
        match found {
            Some(prev) if prev != id => return Err(format!("ambiguous ({prev} and {id})")),
            _ => found = Some(id),
        }
    }
    Ok(found.into_iter().collect())
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
// HUP-S2.3 — budgeted Sign-In with Ethereum (ADR-2026-09-30-rule3-budgetable-signatures).
// ---------------------------------------------------------------------------

/// A Sign-In with Ethereum request that MAY be auto-approved inside a member-granted
/// [`crate::web_budget::WebSigningBudget`]. It carries the message text only: there is no kind
/// field, so a transaction, a permit or other typed data cannot be submitted here (D1 closed list,
/// `OnlyClosedList`). Anything that is not a budgetable SIWE becomes an ordinary HIC-1 card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SiweSignRequest {
    /// The exact message text to sign with `personal_sign` (EIP-191).
    pub message: String,
    /// Core's own attestation of the top-frame origin (D2 #1-3). `None` when core cannot attest
    /// it, which always means an HIC-1 card. Never taken from the request body.
    pub attestation: Option<crate::web_budget::OriginAttestation>,
    /// What the current agent task has read (D2 #19).
    pub taint: crate::web_budget::TaskTaint,
    /// The sidecar marked the call `hic: "required"`: never budgeted.
    pub hic_required: bool,
    /// Who asked (the in-app Hermes loop, or one external client). Budgets are per principal.
    pub principal: String,
    /// The origin the caller says the page has. Display only, and only on a card, where it is
    /// marked as not verified when core could not attest it.
    pub claimed_origin: String,
}

/// The result of [`SignatureCeremony::request_siwe_budgeted`]: either a signature produced under
/// a budget (with its decision record), or a pending HIC-1 ceremony and the plain-language reason
/// it was not budgetable. A request is never dropped (`FallThroughLive`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum BudgetedOutcome {
    AutoSigned {
        signature: Signature,
        #[serde(rename = "recordId")]
        record_id: u64,
        #[serde(rename = "budgetId")]
        budget_id: u64,
        remaining: u32,
        origin: String,
    },
    Pending {
        ceremony: CeremonyView,
        reason: String,
    },
}

impl SignatureCeremony {
    /// **The budgeted path (B-1 only).** Under the dedicated budget lock, in one critical section
    /// (`WebSigningBudget.tla` `Decide` → `Sign` → `Return`):
    /// 1. evaluate every D2 check and the budget (live, unexpired, count left, same wallet, rate);
    /// 2. reserve write-ahead: counter, nonce ledger, window slot and the `Reserved` decision
    ///    record are persisted BEFORE any signature (`RecordBeforeSignature`);
    /// 3. re-read the clock: if the budget or the message expired meanwhile, sign nothing;
    /// 4. sign `personal_sign` through the gated EIP-191 signer and close the record.
    ///
    /// Any failure returns an ordinary pending ceremony (HIC-1) carrying the same message, so the
    /// member can still approve it by id. Revocation takes the same lock, so a revoke that commits
    /// first wins. `clock` returns epoch ms and is read twice (steps 1 and 3).
    pub fn request_siwe_budgeted(
        &self,
        vault: &CustodyVault,
        gate: &crate::web_budget::BudgetGate,
        req: SiweSignRequest,
        clock: &dyn Fn() -> u64,
    ) -> BudgetedOutcome {
        use crate::web_budget::{FallThrough, RecordStatus, SiweEvalInput};
        // K6: the same network gate the signer applies, checked before the budget is evaluated so
        // a refused sign-in reserves nothing and uses no budget. `sign_personal` checks it again.
        if let Err(e) =
            crate::rpc::check_wallet_signing(wallet::personal_message_scope(req.message.as_bytes()))
        {
            return self.siwe_fall_through(&req, FallThrough::Network(e.to_string()));
        }
        let mut guard = gate.lock();
        let now = clock();
        let wallet_addr = wallet::address(vault).ok().map(|w| w.address);
        let input = SiweEvalInput {
            message: &req.message,
            attestation: req.attestation.as_ref(),
            taint: &req.taint,
            hic_required: req.hic_required,
            principal: &req.principal,
            wallet_address: wallet_addr.as_deref(),
        };
        let plan = match guard.evaluate_siwe(&input, now) {
            Ok(p) => p,
            Err(why) => {
                drop(guard);
                return self.siwe_fall_through(&req, why);
            }
        };
        let record_id = match guard.reserve(&plan, now) {
            Ok(r) => r,
            Err(why) => {
                drop(guard);
                return self.siwe_fall_through(&req, why);
            }
        };
        if let Err(why) = guard.still_signable(&plan, clock()) {
            guard.close(record_id, RecordStatus::NotSigned);
            drop(guard);
            return self.siwe_fall_through(&req, why);
        }
        match wallet::sign_personal(vault, req.message.as_bytes()) {
            Ok(sig) => {
                guard.close(record_id, RecordStatus::Signed);
                let remaining = guard.remaining_of(plan.budget_id);
                BudgetedOutcome::AutoSigned {
                    signature: Signature {
                        sig_hex: hex::encode(sig),
                        kind: IntentKind::PersonalSign,
                    },
                    record_id,
                    budget_id: plan.budget_id,
                    remaining,
                    origin: plan.origin,
                }
            }
            Err(e) => {
                guard.close(record_id, RecordStatus::NotSigned);
                drop(guard);
                let why = match e {
                    WalletError::WrongNetwork(m) => FallThrough::Network(m),
                    _ => FallThrough::SignerUnavailable,
                };
                self.siwe_fall_through(&req, why)
            }
        }
    }

    /// Turn a non-budgetable SIWE into an ordinary pending `personal_sign` ceremony (HIC-1).
    fn siwe_fall_through(
        &self,
        req: &SiweSignRequest,
        why: crate::web_budget::FallThrough,
    ) -> BudgetedOutcome {
        let origin = match &req.attestation {
            Some(a) => a.origin.clone(),
            None => format!("{} (site not verified by Citrate Core)", req.claimed_origin),
        };
        let view = self.request(SignatureIntent {
            origin,
            kind: IntentKind::PersonalSign,
            chain_id: crate::rpc::CITRATE_CHAIN_ID,
            raw: hex::encode(req.message.as_bytes()),
        });
        BudgetedOutcome::Pending {
            ceremony: view,
            reason: why.to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// Managed Tauri state + command surface (I-2: no secret crosses invoke).
// ---------------------------------------------------------------------------

/// Managed Tauri state: the process-wide SignatureCeremony.
pub struct CeremonyState(pub SignatureCeremony);

/// Build the managed ceremony state with its first-submission record persisted at `path`.
pub fn build_ceremony_state_at(path: std::path::PathBuf) -> CeremonyState {
    CeremonyState(SignatureCeremony::with_submitted_log(
        crate::submitted_log::SubmittedTxLog::open(path),
    ))
}

/// Build the managed ceremony state when the app data dir can't be resolved, so the
/// first-submission record has nowhere to persist.
pub fn build_ceremony_state_unpersisted() -> CeremonyState {
    CeremonyState(SignatureCeremony::with_submitted_log(
        crate::submitted_log::SubmittedTxLog::unpersisted(),
    ))
}

use tauri::State;

fn err_str(e: CeremonyError) -> String {
    e.to_string()
}

/// UI truth contract v1.1 §1 (chain#326, verified against the node's send handler and mempool):
/// the `eth_sendRawTransaction` errors that prove the node did not accept the tx. Every handler
/// error uses code -32602, so the MESSAGE decides, matched exactly or by its fixed prefix. Only
/// meaningful on the first submission of the signed bytes; the caller checks that.
///
/// K6: a network-gate refusal is raised before `eth_sendRawTransaction` is called, so it is
/// definitive as well.
fn is_definitive_send_error(e: &crate::rpc::RpcError) -> bool {
    const EXACT: &[&str] = &[
        "Rate limit exceeded. Try again later.",
        "Missing transaction data",
        "Invalid tx hex",
        "Invalid hex",
        "Failed to submit transaction: Full",
        "Failed to submit transaction: SenderLimitExceeded",
        "Failed to submit transaction: InvalidSignature",
    ];
    const PREFIX: &[&str] = &[
        "Failed to parse transaction: ",
        "Insufficient funds: account balance ",
        "Failed to submit transaction: GasPriceTooLow {",
        "Failed to submit transaction: InsufficientBalance {",
        "Failed to submit transaction: InvalidTransaction(",
        "Failed to submit transaction: NonceTooLow {",
    ];
    match e {
        crate::rpc::RpcError::RateLimited | crate::rpc::RpcError::WrongNetwork(_) => true,
        crate::rpc::RpcError::Node(m) => {
            EXACT.contains(&m.as_str()) || PREFIX.iter().any(|p| m.starts_with(p))
        }
        _ => false,
    }
}

/// The error string `sign_and_broadcast` returns. Every failure established BEFORE the node could
/// have received the tx is prefixed "not submitted: " so the app may say "not sent"; an unknown
/// outcome after submission carries "submitted, outcome unknown (tx <hash>)" instead (C1-V #347).
fn broadcast_err_str(e: CeremonyError) -> String {
    match e {
        CeremonyError::SubmittedOutcomeUnknown { .. } => err_str(e),
        other => format!("not submitted: {}", err_str(other)),
    }
}

/// **Command — sign_request.** Submit a [`SignatureIntent`]; returns a PENDING
/// [`CeremonyView`] (id + true origin + decoded action). Signs NOTHING and
/// returns NO key material. Any origin (user / agent / micro-app) reaches the
/// signer ONLY through this door: it produces an intent → a ceremony, never a
/// signature (B1.2-ADV-1/7).
#[tauri::command]
pub async fn sign_request(
    app_h: tauri::AppHandle,
    intent: SignatureIntent,
) -> std::result::Result<CeremonyView, String> {
    // HUP-S0.1: the blocking body runs on the blocking pool, never the main thread.
    crate::blocking::off_main(move || {
        let st0 = tauri::Manager::try_state::<CeremonyState>(&app_h)
            .ok_or_else(|| "internal: managed state unavailable".to_string())?;
        sign_request_sync(st0, intent)
    })
    .await
}

/// Blocking body of [`sign_request`]; reached only through [`crate::blocking::off_main`].
pub fn sign_request_sync(
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
pub async fn sign_approve(
    app_h: tauri::AppHandle,
    id: String,
    raw_ack: bool,
) -> std::result::Result<Signature, String> {
    // HUP-S0.1: the blocking body runs on the blocking pool, never the main thread.
    crate::blocking::off_main(move || {
        let st0 = tauri::Manager::try_state::<CeremonyState>(&app_h)
            .ok_or_else(|| "internal: managed state unavailable".to_string())?;
        let st1 = tauri::Manager::try_state::<crate::custody::CustodyState>(&app_h)
            .ok_or_else(|| "internal: managed state unavailable".to_string())?;
        sign_approve_sync(st0, st1, id, raw_ack)
    })
    .await
}

/// Blocking body of [`sign_approve`]; reached only through [`crate::blocking::off_main`].
pub fn sign_approve_sync(
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
pub async fn sign_and_broadcast(
    app_h: tauri::AppHandle,
    id: String,
    raw_ack: bool,
) -> std::result::Result<BroadcastResult, String> {
    // HUP-S0.1: the blocking body runs on the blocking pool, never the main thread.
    crate::blocking::off_main(move || {
        let st0 = tauri::Manager::try_state::<CeremonyState>(&app_h)
            .ok_or_else(|| "internal: managed state unavailable".to_string())?;
        let st1 = tauri::Manager::try_state::<crate::custody::CustodyState>(&app_h)
            .ok_or_else(|| "internal: managed state unavailable".to_string())?;
        sign_and_broadcast_sync(st0, st1, id, raw_ack)
    })
    .await
}

/// HUP-S7.5 (D-27): approve, sign and broadcast a pending ceremony, then tell the broadcast
/// observer (the app's metering) about the result. [`sign_and_broadcast_sync`] goes through here,
/// so a test can prove the observer hears every broadcast without the Tauri state.
pub fn approve_broadcast_and_notify<T: crate::rpc::RpcTransport>(
    ceremony: &SignatureCeremony,
    vault: &CustodyVault,
    rpc: &crate::rpc::RpcClient<T>,
    id: &str,
    raw_ack: bool,
    cfg: BroadcastConfig,
) -> Result<BroadcastResult> {
    let result = ceremony.approve_and_broadcast(vault, rpc, id, raw_ack, cfg)?;
    notify_broadcast(&result);
    Ok(result)
}

/// Blocking body of [`sign_and_broadcast`]; reached only through [`crate::blocking::off_main`].
pub fn sign_and_broadcast_sync(
    ceremony: State<'_, CeremonyState>,
    custody: State<'_, crate::custody::CustodyState>,
    id: String,
    raw_ack: bool,
) -> std::result::Result<BroadcastResult, String> {
    // Production wiring: the live 40204 RPC client + chain id, with a sane
    // receipt-poll budget (30 attempts × 2s = up to 60s for inclusion). Blocking
    // HTTP is correct here — the command runs off the async runtime.
    let rpc = crate::rpc::RpcClient::citrate();
    // HUP-S7.5 (D-27): the observer (the app's metering) is told about the result before it is
    // returned unchanged.
    approve_broadcast_and_notify(
        &ceremony.0,
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
    .map_err(broadcast_err_str)
}

/// **Command — sign_reject.** Consume a pending ceremony with no signature.
/// Unknown/already-consumed id → error.
#[tauri::command]
pub async fn sign_reject(app_h: tauri::AppHandle, id: String) -> std::result::Result<(), String> {
    // HUP-S0.1: the blocking body runs on the blocking pool, never the main thread.
    crate::blocking::off_main(move || {
        let st0 = tauri::Manager::try_state::<CeremonyState>(&app_h)
            .ok_or_else(|| "internal: managed state unavailable".to_string())?;
        sign_reject_sync(st0, id)
    })
    .await
}

/// Blocking body of [`sign_reject`]; reached only through [`crate::blocking::off_main`].
pub fn sign_reject_sync(
    ceremony: State<'_, CeremonyState>,
    id: String,
) -> std::result::Result<(), String> {
    ceremony.0.reject(&id).map_err(err_str)
}

#[cfg(test)]
mod tests {
    include!("ceremony_tests.rs");
    include!("ceremony_budget_tests.rs");
    include!("ceremony_expiry_tests.rs");
}
