//! HUP-S2.3 — the web-signing budget store (ADR-2026-09-30-rule3-budgetable-signatures, D1-D4).
//!
//! A `WebSigningBudget` lets the member pre-approve, once, a bounded number of Sign-In with
//! Ethereum signatures for ONE allowlisted https origin. This module holds the budgets, their
//! counters, the per-origin nonce ledger, the rolling-window reservations and the hash-chained
//! decision records. It NEVER signs and never touches a key: the only signer call stays in
//! `ceremony.rs` (`SignatureCeremony::request_siwe_budgeted`), which takes this store's lock for
//! the whole check, reserve, record and sign sequence (D4, `WebSigningBudget.tla` `Decide`/`Sign`).
//!
//! Defaults change nothing for members: there are no budgets until the member grants one in
//! Settings → Budgets, so every sign-in request asks (HIC-1) exactly as before.
//!
//! Persistence (D4 "Storage"): one JSON file in the app data dir, integrity-protected with
//! HMAC-SHA256 under a 32-byte key sealed in the OS keychain. A missing key with an existing file,
//! an unreadable file or a MAC mismatch means NO budgets (fail closed to HIC-1) until the member
//! resets the store, which keeps the old file aside. Counters, the nonce ledger and the records
//! persist across restarts, so a restart never resets a cap.
//!
//! The x402 budget type (B-2) is defined here so the two kinds can never be widened into each
//! other, but it is INERT: the asset allowlist is empty until a wrapped-SALT token that implements
//! the pinned authorization type exists on 40204 (owner decision O-1).

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use crate::custody::Keyring;
use crate::siwe::{self, BudgetableSiwe, SiweCheckContext, SiweReject};

// ---------------------------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------------------------

/// Owner decision O-3 (accepted 2026-09-30, ADR sign-off block): content from the same
/// allowlisted origin does not taint a sign-in to that origin. Everything else taints.
pub const O3_SAME_ORIGIN_EXEMPT: bool = true;
/// D2 #21: at most one auto-approved sign-in per origin per 30 s.
pub const SIWE_MIN_GAP_MS: u64 = 30_000;
/// D2 #21: the rolling window (not a calendar day).
pub const SIWE_WINDOW_MS: u64 = 24 * 60 * 60 * 1000;
/// D2 #21: at most 20 auto-approved sign-ins per origin per rolling window.
pub const SIWE_WINDOW_MAX: u32 = 20;
/// O-2 ceiling (accepted): no SIWE budget may allow more than 50 sign-ins.
pub const MAX_COUNT_CEILING: u32 = 50;
/// O-2 ceiling (accepted): no SIWE budget may live longer than 30 days.
pub const MAX_TTL_MS_CEILING: u64 = 30 * 24 * 60 * 60 * 1000;
/// The shortest budget the store accepts (a budget that expires immediately is a mistake).
pub const MIN_TTL_MS: u64 = 60 * 1000;
/// PLACEHOLDER, pending owner sign-off ("default budget values" WP): the count the Budgets form
/// proposes. Conservative and well inside the O-2 ceiling. No budget exists until a member grants
/// one, so this value changes nothing on its own.
pub const PLACEHOLDER_DEFAULT_MAX_COUNT: u32 = 10;
/// PLACEHOLDER, pending owner sign-off: the lifetime (days) the Budgets form proposes.
pub const PLACEHOLDER_DEFAULT_TTL_DAYS: u32 = 7;
/// The two placeholders above are not final values. The UI says so.
pub const DEFAULTS_PENDING_OWNER_SIGNOFF: bool = true;
/// The in-app Hermes loop. Budgets are scoped to the principal that asked for them and are never
/// inherited across principals (red-team correction 6).
pub const DEFAULT_PRINCIPAL: &str = "hermes";
/// Keychain account for the store's integrity key.
pub const MAC_KEY_ACCOUNT: &str = "web-signing-budgets-mac-v1";
/// Keychain account for the generation of the newest budget file this device saved. A file
/// older than that (a restored copy, still validly MACed) is refused, so spent counts, the nonce
/// ledger and the rate slots can never be rolled back by replacing the file.
pub const GENERATION_ACCOUNT: &str = "web-signing-budgets-generation-v1";
/// The `prev_hash` of the first decision record.
pub const GENESIS_HASH: &str = "0x0000000000000000000000000000000000000000000000000000000000000000";
/// How many records the UI snapshot returns (newest first). The file keeps all of them.
pub const SNAPSHOT_RECORDS: usize = 200;

const FILE_VERSION: u32 = 1;

// ---------------------------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------------------------

/// B-1: a per-origin sign-in budget (D4 "Shape").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebSigningBudget {
    pub id: u64,
    /// Normalized `https://host[:port]`.
    pub origin: String,
    pub principal: String,
    pub chain_id: u64,
    pub max_count: u32,
    pub used_count: u32,
    pub granted_at_ms: u64,
    pub expires_at_ms: u64,
    /// The wallet the member granted it for (lower-case). A different active wallet voids it.
    pub wallet_address: String,
    pub granted_record_id: u64,
    pub revoked_at_ms: Option<u64>,
}

/// B-2: a capped x402 budget (D3). Defined so it is a separate type from B-1; never granted while
/// [`X402_ASSET_ALLOWLIST`] is empty.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct X402Budget {
    pub id: u64,
    pub recipient: String,
    pub asset: String,
    /// Base units, decimal string (compared as U256 when B-2 is enabled).
    pub per_signature_max: String,
    pub per_recipient_window_max: String,
    pub max_count: u32,
    pub used_count: u32,
    pub expires_at_ms: u64,
    pub granted_record_id: u64,
    pub revoked_at_ms: Option<u64>,
}

/// An asset that may be paid under a B-2 budget: the EIP-712 domain it must match exactly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct X402Asset {
    pub chain_id: u64,
    pub verifying_contract: &'static str,
    pub name: &'static str,
    pub version: &'static str,
}

/// O-1: EMPTY until a wrapped-SALT token implementing the pinned authorization type is deployed
/// and pinned from on-chain truth. While empty, B-2 is inert.
pub const X402_ASSET_ALLOWLIST: &[X402Asset] = &[];

/// The structured x402 request the escalation router would send (never raw typed data).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct X402Request {
    pub recipient: String,
    pub asset: String,
    pub amount: String,
}

/// B-2 eligibility. Inert today: the asset allowlist is empty, so every x402 request is HIC-1.
/// Tainted context is never budgetable for x402 (no O-3 exemption).
pub fn x402_budgetable(req: &X402Request, taint: &TaskTaint) -> Result<(), FallThrough> {
    if !matches!(taint, TaskTaint::Clean) {
        return Err(FallThrough::Tainted);
    }
    if !X402_ASSET_ALLOWLIST
        .iter()
        .any(|a| a.verifying_contract.eq_ignore_ascii_case(&req.asset))
    {
        return Err(FallThrough::X402Inert);
    }
    // Reaching here needs a non-empty allowlist, which needs its own reviewed change (S1.5).
    Err(FallThrough::X402Inert)
}

/// The longest validity window core gives an x402 authorization (D3 `validity_max`, 10 min).
pub const X402_VALIDITY_MAX_S: u64 = 600;
/// `validAfter` is set this far before "now" so a chain clock a little behind the member's does
/// not reject a fresh authorization as "not yet valid" (D3 only requires `validAfter <= now`).
pub const X402_VALID_AFTER_SKEW_S: u64 = 30;

/// Why core would not build an x402 authorization from a request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum X402BuildError {
    /// The request names a different asset than the allowlist entry it is checked against.
    AssetMismatch,
    /// The request's payee differs from the recipient the member approved.
    RecipientMismatch,
    /// Zero, malformed or out-of-range amount, or a malformed address.
    Malformed(crate::eip712::Eip712Error),
    ZeroAmount,
    /// Validity of zero or above [`X402_VALIDITY_MAX_S`].
    Validity,
}

impl X402Asset {
    /// The EIP-712 domain this asset's authorizations are hashed under.
    pub fn domain(&self) -> Result<crate::eip712::Domain, X402BuildError> {
        Ok(crate::eip712::Domain {
            name: self.name.to_string(),
            version: self.version.to_string(),
            chain_id: self.chain_id,
            verifying_contract: crate::eip712::parse_address(self.verifying_contract)
                .map_err(X402BuildError::Malformed)?,
        })
    }
}

/// Build the pinned `TransferWithAuthorization` for one x402 request (ADR D3 "Core builds the
/// bytes it signs"): the asset must be `asset` (an allowlist entry), the payee must be the
/// recipient the member approved, `from` is the member's active wallet, the window is at most
/// [`X402_VALIDITY_MAX_S`], and `nonce` comes from core ([`fresh_x402_nonce`]), never the request.
///
/// This builds and hashes; it does not decide budgetability ([`x402_budgetable`]) and never signs.
pub fn build_x402_authorization(
    asset: &X402Asset,
    approved_recipient: &str,
    req: &X402Request,
    from: &str,
    now_s: u64,
    validity_s: u64,
    nonce: [u8; 32],
) -> Result<crate::eip712::TransferWithAuthorization, X402BuildError> {
    use crate::eip712::{parse_address, parse_u256_dec, TransferWithAuthorization};
    if !req.asset.eq_ignore_ascii_case(asset.verifying_contract) {
        return Err(X402BuildError::AssetMismatch);
    }
    if !req.recipient.eq_ignore_ascii_case(approved_recipient) {
        return Err(X402BuildError::RecipientMismatch);
    }
    if validity_s == 0 || validity_s > X402_VALIDITY_MAX_S {
        return Err(X402BuildError::Validity);
    }
    let value = parse_u256_dec(&req.amount).map_err(X402BuildError::Malformed)?;
    if value == [0u8; 32] {
        return Err(X402BuildError::ZeroAmount);
    }
    let to = parse_address(&req.recipient).map_err(X402BuildError::Malformed)?;
    let from = parse_address(from).map_err(X402BuildError::Malformed)?;
    let valid_before = now_s
        .checked_add(validity_s)
        .ok_or(X402BuildError::Validity)?;
    TransferWithAuthorization::new(
        from,
        to,
        value,
        now_s.saturating_sub(X402_VALID_AFTER_SKEW_S),
        valid_before,
        nonce,
    )
    .map_err(X402BuildError::Malformed)
}

/// A 32-byte authorization nonce from the OS CSPRNG (D3: core generates it, never the caller).
pub fn fresh_x402_nonce() -> [u8; 32] {
    use rand::RngCore as _;
    let mut n = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut n);
    n
}

/// What the current agent task has read that it did not author (D2 #19).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskTaint {
    /// Nothing untrusted in context.
    Clean,
    /// Untrusted content from these sources (web origins, or `ext` for search results, third-party
    /// skill or MCP output, other principals' memory).
    Sources(Vec<String>),
    /// The caller could not say. Treated as tainted.
    Unknown,
}

/// Which frame raised the request, as core attests it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FrameKind {
    Top,
    Sub,
}

/// Which browser the page is in, as core attests it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserMode {
    Managed,
    Attach,
}

/// D2 #1-3: the origin core read over its OWN session with the managed browser. Built only by
/// core's attestor, never from the request body, the page or the model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OriginAttestation {
    pub origin: String,
    pub frame: FrameKind,
    pub mode: BrowserMode,
}

/// The kinds of decision record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordKind {
    #[default]
    BudgetGranted,
    BudgetRevoked,
    AllBudgetsRevoked,
    StoreReset,
    AutoSign,
}

/// The outcome of a decision record. Only `AutoSign` records move past `Final`-at-creation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordStatus {
    /// Write-ahead: reserved, signer not yet run.
    Reserved,
    Signed,
    NotSigned,
    /// The process stopped between the reservation and the outcome (never reported as not signed).
    OutcomeUnknown,
    /// A member decision (grant, revoke) that has no signer outcome.
    Final,
}

/// D4 "After-the-fact visibility": one hash-chained record per decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecisionRecord {
    pub record_id: u64,
    pub kind: RecordKind,
    pub budget_id: Option<u64>,
    pub principal: String,
    pub origin: String,
    /// keccak256 of the exact signed bytes (AutoSign only).
    pub payload_digest: Option<String>,
    pub statement: Option<String>,
    pub nonce: Option<String>,
    pub request_id: Option<String>,
    pub signer_address: Option<String>,
    pub at_ms: u64,
    pub remaining_after: Option<u32>,
    pub note: Option<String>,
    pub prev_hash: String,
    pub hash: String,
    /// Not covered by `hash` (it changes after the record is appended).
    pub status: RecordStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Reservation {
    origin: String,
    at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct LedgerEntry {
    origin: String,
    nonce: String,
    keep_until_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct BudgetFile {
    version: u32,
    next_budget_id: u64,
    budgets: Vec<WebSigningBudget>,
    x402_budgets: Vec<X402Budget>,
    ledger: Vec<LedgerEntry>,
    reservations: Vec<Reservation>,
    records: Vec<DecisionRecord>,
    /// Bumped on every save and sealed in the keychain (rollback protection). Absent in files
    /// written before it existed (read as 0).
    #[serde(default)]
    generation: u64,
    /// The newest record id copied into the local decision records the nightly anchor covers
    /// (US-2.3 AC3). Covered by the MAC like everything else.
    #[serde(default)]
    exported_through: u64,
}

impl Default for BudgetFile {
    fn default() -> Self {
        BudgetFile {
            version: FILE_VERSION,
            next_budget_id: 1,
            budgets: Vec::new(),
            x402_budgets: Vec::new(),
            ledger: Vec::new(),
            reservations: Vec::new(),
            records: Vec::new(),
            generation: 0,
            exported_through: 0,
        }
    }
}

#[derive(Serialize, Deserialize)]
struct Envelope {
    v: u32,
    body: String,
    mac: String,
}

/// Whether the store can be trusted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", content = "reason", rename_all = "snake_case")]
pub enum StoreHealth {
    Ok,
    /// Fail closed: no budgets apply. The reason is plain language for Settings → Budgets.
    Failed(String),
}

/// Errors from member actions on the store (grant, revoke, reset).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BudgetError {
    BadOrigin(String),
    OverCeiling,
    LiveBudgetExists,
    UnknownBudget,
    StoreUnavailable,
    /// Reset was asked for while the store verifies: nothing to recover, so nothing is wiped.
    StoreHealthy,
    Persist,
}

impl std::fmt::Display for BudgetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BudgetError::BadOrigin(r) => write!(f, "this site cannot have a sign-in budget: {r}"),
            BudgetError::OverCeiling => write!(
                f,
                "a sign-in budget allows 1 to {MAX_COUNT_CEILING} sign-ins and lasts at most 30 days"
            ),
            BudgetError::LiveBudgetExists => write!(
                f,
                "this site already has an active budget; revoke it first to grant a new one"
            ),
            BudgetError::UnknownBudget => write!(f, "no budget with that id"),
            BudgetError::StoreUnavailable => write!(
                f,
                "budgets are off because the budget file could not be verified; every sign-in asks you"
            ),
            BudgetError::StoreHealthy => {
                write!(f, "the budget file is healthy; nothing to reset")
            }
            BudgetError::Persist => write!(f, "the budget file could not be saved"),
        }
    }
}

impl std::error::Error for BudgetError {}

/// Why a budgeted request goes to an ordinary HIC-1 card instead (never silently dropped).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FallThrough {
    StoreUnavailable,
    HicRequired,
    NotAttested,
    NotTopFrame,
    AttachMode,
    NoBudget,
    BudgetRevoked,
    BudgetExpired,
    BudgetExhausted,
    WalletChanged,
    NoWallet,
    Tainted,
    TaintUnknown,
    Message(SiweReject),
    NonceReused,
    RateGap,
    RateWindow,
    WriteAheadFailed,
    ExpiredBeforeSigning,
    SignerUnavailable,
    X402Inert,
}

impl std::fmt::Display for FallThrough {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s: String = match self {
            FallThrough::StoreUnavailable => {
                "budgets are off because the budget file could not be verified".into()
            }
            FallThrough::HicRequired => {
                "this session read untrusted content, so this sign-in needs your explicit approval".into()
            }
            FallThrough::NotAttested => {
                "Citrate Core could not confirm which site asked, so it asks you".into()
            }
            FallThrough::NotTopFrame => "the request came from an embedded frame, not the page itself".into(),
            FallThrough::AttachMode => "budgets do not apply to an attached browser".into(),
            FallThrough::NoBudget => "this site has no sign-in budget".into(),
            FallThrough::BudgetRevoked => "the budget for this site was revoked".into(),
            FallThrough::BudgetExpired => "the budget for this site has expired".into(),
            FallThrough::BudgetExhausted => "the budget for this site is used up".into(),
            FallThrough::WalletChanged => {
                "the budget was granted for a different wallet than the one active now".into()
            }
            FallThrough::NoWallet => "no wallet is available to sign with".into(),
            FallThrough::Tainted => {
                "this task read content from another source, so sign-ins need your approval for the rest of it".into()
            }
            FallThrough::TaintUnknown => {
                "Citrate Core could not tell what this task has read, so it asks you".into()
            }
            FallThrough::Message(r) => r.to_string(),
            FallThrough::NonceReused => "this sign-in nonce was already used for this site".into(),
            FallThrough::RateGap => "another sign-in to this site happened less than 30 seconds ago".into(),
            FallThrough::RateWindow => "this site reached 20 automatic sign-ins in the last 24 hours".into(),
            FallThrough::WriteAheadFailed => {
                "the decision record could not be saved, so nothing was signed automatically".into()
            }
            FallThrough::ExpiredBeforeSigning => "the budget or the message expired before signing".into(),
            FallThrough::SignerUnavailable => "the wallet is locked or unavailable".into(),
            FallThrough::X402Inert => {
                "automatic payments are off until an allowlisted payment token exists".into()
            }
        };
        f.write_str(&s)
    }
}

/// The inputs to one budgeted SIWE decision. Built by core: the attestation and the wallet come
/// from core, never from the request body.
pub struct SiweEvalInput<'a> {
    pub message: &'a str,
    pub attestation: Option<&'a OriginAttestation>,
    pub taint: &'a TaskTaint,
    /// The sidecar marked this call `hic: "required"`. Budget paths always refuse it.
    pub hic_required: bool,
    pub principal: &'a str,
    /// The member's active wallet address, if any.
    pub wallet_address: Option<&'a str>,
}

/// An eligible request, ready to reserve.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SiwePlan {
    pub budget_id: u64,
    pub origin: String,
    pub principal: String,
    pub signer_address: String,
    pub payload_digest: String,
    pub checked: BudgetableSiwe,
    pub budget_expires_at_ms: u64,
}

/// A budget as Settings → Budgets shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BudgetView {
    pub id: u64,
    pub origin: String,
    pub principal: String,
    pub chain_id: u64,
    pub max_count: u32,
    pub used_count: u32,
    pub remaining: u32,
    pub granted_at_ms: u64,
    pub expires_at_ms: u64,
    pub revoked_at_ms: Option<u64>,
    /// `active`, `revoked`, `expired`, `used_up` or `wallet_changed`.
    pub status: String,
}

/// Everything the Budgets UI needs, in one read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BudgetSnapshot {
    pub health: StoreHealth,
    pub budgets: Vec<BudgetView>,
    /// Newest first, at most [`SNAPSHOT_RECORDS`].
    pub records: Vec<DecisionRecord>,
    /// Hash of the newest record ([`GENESIS_HASH`] when there are none). The nightly anchor
    /// (HUP-S7.3) commits to this.
    pub head_hash: String,
    pub record_count: usize,
}

// ---------------------------------------------------------------------------------------------
// The gate
// ---------------------------------------------------------------------------------------------

struct GateInner {
    file: BudgetFile,
    health: StoreHealth,
    mac_key: Option<Zeroizing<Vec<u8>>>,
}

/// The process-wide budget store. Its mutex IS the dedicated budget lock of D4: every grant,
/// revoke and auto-approval takes it, and the ceremony holds it across check, reserve, record and
/// sign. It is not the ceremony's pending-map lock.
pub struct BudgetGate {
    path: PathBuf,
    keyring: Box<dyn Keyring>,
    inner: Mutex<GateInner>,
}

/// A held budget lock. Only the ceremony (this crate) drives the eligible-path methods.
pub struct GateGuard<'a> {
    gate: &'a BudgetGate,
    inner: MutexGuard<'a, GateInner>,
}

impl BudgetGate {
    /// Open (or create) the store at `path`, recovering any open reservation as outcome unknown.
    /// `_now_ms` is accepted for symmetry with the other calls; recovery does not depend on it.
    pub fn open(path: PathBuf, keyring: Box<dyn Keyring>, _now_ms: u64) -> Self {
        let (file, health, mac_key) = load(&path, keyring.as_ref());
        BudgetGate {
            path,
            keyring,
            inner: Mutex::new(GateInner {
                file,
                health,
                mac_key,
            }),
        }
    }

    fn lock_inner(&self) -> MutexGuard<'_, GateInner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Take the dedicated budget lock.
    pub fn lock(&self) -> GateGuard<'_> {
        GateGuard {
            gate: self,
            inner: self.lock_inner(),
        }
    }

    pub fn health(&self) -> StoreHealth {
        self.lock_inner().health.clone()
    }

    /// D4 Grant: the member's HIC-1 decision in Settings → Budgets (no wallet signature, O-4).
    /// Never widens a live budget: an origin with an active (or used-up but unexpired) budget must
    /// be revoked first.
    pub fn grant(
        &self,
        origin_input: &str,
        principal: &str,
        max_count: u32,
        ttl_ms: u64,
        wallet_address: &str,
        now_ms: u64,
    ) -> Result<WebSigningBudget, BudgetError> {
        let mut g = self.lock_inner();
        if g.health != StoreHealth::Ok {
            return Err(BudgetError::StoreUnavailable);
        }
        let origin = siwe::normalize_allowlist_origin(origin_input)
            .map_err(|r| BudgetError::BadOrigin(r.to_string()))?;
        if max_count == 0
            || max_count > MAX_COUNT_CEILING
            || !(MIN_TTL_MS..=MAX_TTL_MS_CEILING).contains(&ttl_ms)
        {
            return Err(BudgetError::OverCeiling);
        }
        let live = g.file.budgets.iter().any(|b| {
            b.origin == origin
                && b.principal == principal
                && b.revoked_at_ms.is_none()
                && now_ms < b.expires_at_ms
        });
        if live {
            return Err(BudgetError::LiveBudgetExists);
        }
        let mut next = g.file.clone();
        let id = next.next_budget_id;
        next.next_budget_id += 1;
        let rid = append_record(
            &mut next,
            RecordDraft {
                kind: RecordKind::BudgetGranted,
                budget_id: Some(id),
                principal: principal.to_string(),
                origin: origin.clone(),
                at_ms: now_ms,
                remaining_after: Some(max_count),
                note: Some(format!(
                    "{max_count} sign-ins until {}",
                    fmt_ms(now_ms.saturating_add(ttl_ms))
                )),
                ..RecordDraft::default()
            },
            RecordStatus::Final,
        );
        let budget = WebSigningBudget {
            id,
            origin,
            principal: principal.to_string(),
            chain_id: siwe::CHAIN_ALLOWLIST[0],
            max_count,
            used_count: 0,
            granted_at_ms: now_ms,
            expires_at_ms: now_ms.saturating_add(ttl_ms),
            wallet_address: wallet_address.to_ascii_lowercase(),
            granted_record_id: rid,
            revoked_at_ms: None,
        };
        next.budgets.push(budget.clone());
        persist(
            &self.path,
            key_ref(&g.mac_key),
            self.keyring.as_ref(),
            &mut next,
        )
        .map_err(|_| BudgetError::Persist)?;
        g.file = next;
        Ok(budget)
    }

    /// D4 Revocation: immediate. Takes the budget lock, so an auto-approval that has not acquired
    /// it yet cannot sign under this budget. The tombstone is kept in memory even if saving fails
    /// (the safe direction); the error tells the member the file was not updated.
    pub fn revoke(&self, id: u64, now_ms: u64) -> Result<(), BudgetError> {
        let mut g = self.lock_inner();
        let idx = g
            .file
            .budgets
            .iter()
            .position(|b| b.id == id)
            .ok_or(BudgetError::UnknownBudget)?;
        if g.file.budgets[idx].revoked_at_ms.is_some() {
            return Ok(());
        }
        g.file.budgets[idx].revoked_at_ms = Some(now_ms);
        let (principal, origin) = (
            g.file.budgets[idx].principal.clone(),
            g.file.budgets[idx].origin.clone(),
        );
        append_record(
            &mut g.file,
            RecordDraft {
                kind: RecordKind::BudgetRevoked,
                budget_id: Some(id),
                principal,
                origin,
                at_ms: now_ms,
                ..RecordDraft::default()
            },
            RecordStatus::Final,
        );
        if g.health != StoreHealth::Ok {
            return Ok(());
        }
        let inner = &mut *g;
        persist(
            &self.path,
            key_ref(&inner.mac_key),
            self.keyring.as_ref(),
            &mut inner.file,
        )
        .map_err(|_| BudgetError::Persist)
    }

    /// "Stop all autonomy" / `budget_revoke_all`: every unrevoked budget at once, under the lock.
    /// Returns how many were revoked.
    pub fn revoke_all(&self, reason: &str, now_ms: u64) -> Result<u32, BudgetError> {
        let mut g = self.lock_inner();
        let mut n = 0u32;
        for b in g.file.budgets.iter_mut() {
            if b.revoked_at_ms.is_none() {
                b.revoked_at_ms = Some(now_ms);
                n += 1;
            }
        }
        append_record(
            &mut g.file,
            RecordDraft {
                kind: RecordKind::AllBudgetsRevoked,
                principal: "member".into(),
                at_ms: now_ms,
                note: Some(reason.chars().take(200).collect()),
                ..RecordDraft::default()
            },
            RecordStatus::Final,
        );
        if g.health != StoreHealth::Ok {
            return Ok(n);
        }
        let inner = &mut *g;
        persist(
            &self.path,
            key_ref(&inner.mac_key),
            self.keyring.as_ref(),
            &mut inner.file,
        )
        .map_err(|_| BudgetError::Persist)?;
        Ok(n)
    }

    /// After an integrity failure, the member may reset: the unverifiable file is kept aside as
    /// `<file>.corrupt-<ms>`, a fresh key is sealed, and the store starts empty (no budgets).
    /// A healthy store is never reset: the check runs under the budget lock, so a reset cannot
    /// wipe live counters, the nonce ledger or the records.
    pub fn reset_after_integrity_failure(&self, now_ms: u64) -> Result<(), BudgetError> {
        let mut g = self.lock_inner();
        if g.health == StoreHealth::Ok {
            return Err(BudgetError::StoreHealthy);
        }
        if self.path.exists() {
            let mut aside = self.path.clone().into_os_string();
            aside.push(format!(".corrupt-{now_ms}"));
            std::fs::rename(&self.path, PathBuf::from(aside)).map_err(|_| BudgetError::Persist)?;
        }
        let key = new_key();
        self.keyring
            .set(MAC_KEY_ACCOUNT, &key)
            .map_err(|_| BudgetError::StoreUnavailable)?;
        let mut file = BudgetFile::default();
        append_record(
            &mut file,
            RecordDraft {
                kind: RecordKind::StoreReset,
                principal: "member".into(),
                at_ms: now_ms,
                note: Some(
                    "the previous budget file could not be verified and was kept aside".into(),
                ),
                ..RecordDraft::default()
            },
            RecordStatus::Final,
        );
        persist(
            &self.path,
            Some(key.as_slice()),
            self.keyring.as_ref(),
            &mut file,
        )
        .map_err(|_| BudgetError::Persist)?;
        g.file = file;
        g.mac_key = Some(key);
        g.health = StoreHealth::Ok;
        Ok(())
    }

    /// The id of a live budget (not revoked, not expired, not used up) for `origin_input` and
    /// `principal`, granted for `wallet`. Used before sharing the member's address with a page:
    /// only a site the member gave a sign-in budget learns the address without asking.
    pub fn live_budget_for(
        &self,
        origin_input: &str,
        principal: &str,
        wallet: Option<&str>,
        now_ms: u64,
    ) -> Option<u64> {
        let wallet = wallet?;
        let origin = siwe::normalize_allowlist_origin(origin_input).ok()?;
        let g = self.lock_inner();
        if g.health != StoreHealth::Ok {
            return None;
        }
        g.file
            .budgets
            .iter()
            .filter(|b| b.origin == origin && b.principal == principal)
            .max_by_key(|b| b.id)
            .filter(|b| status_of(b, now_ms, Some(wallet)) == "active")
            .map(|b| b.id)
    }

    /// Up to `limit` records not yet copied into the local decision records, oldest first. The
    /// list stops before the first record whose outcome is still open (`Reserved`), so a record
    /// is only ever exported once it is final.
    pub fn records_to_export(&self, limit: usize) -> Vec<DecisionRecord> {
        let g = self.lock_inner();
        if g.health != StoreHealth::Ok {
            return Vec::new();
        }
        let after = g.file.exported_through;
        g.file
            .records
            .iter()
            .filter(|r| r.record_id > after)
            .take_while(|r| r.status != RecordStatus::Reserved)
            .take(limit)
            .cloned()
            .collect()
    }

    /// Records up to `through` are in the local decision records. Never moves backwards; an id
    /// past the newest record is refused.
    pub fn mark_exported(&self, through: u64) -> Result<(), BudgetError> {
        let mut g = self.lock_inner();
        if g.health != StoreHealth::Ok {
            return Err(BudgetError::StoreUnavailable);
        }
        if through > g.file.records.last().map(|r| r.record_id).unwrap_or(0) {
            return Err(BudgetError::UnknownBudget);
        }
        if through <= g.file.exported_through {
            return Ok(());
        }
        let mut next = g.file.clone();
        next.exported_through = through;
        persist(
            &self.path,
            key_ref(&g.mac_key),
            self.keyring.as_ref(),
            &mut next,
        )
        .map_err(|_| BudgetError::Persist)?;
        g.file = next;
        Ok(())
    }

    /// Everything Settings → Budgets shows. `wallet` is the active wallet (for the
    /// `wallet_changed` status).
    pub fn snapshot(&self, now_ms: u64, wallet: Option<&str>) -> BudgetSnapshot {
        let g = self.lock_inner();
        let mut budgets: Vec<BudgetView> = g
            .file
            .budgets
            .iter()
            .map(|b| BudgetView {
                id: b.id,
                origin: b.origin.clone(),
                principal: b.principal.clone(),
                chain_id: b.chain_id,
                max_count: b.max_count,
                used_count: b.used_count,
                remaining: b.max_count.saturating_sub(b.used_count),
                granted_at_ms: b.granted_at_ms,
                expires_at_ms: b.expires_at_ms,
                revoked_at_ms: b.revoked_at_ms,
                status: status_of(b, now_ms, wallet).to_string(),
            })
            .collect();
        budgets.sort_by_key(|b| std::cmp::Reverse(b.id));
        let records: Vec<DecisionRecord> = g
            .file
            .records
            .iter()
            .rev()
            .take(SNAPSHOT_RECORDS)
            .cloned()
            .collect();
        BudgetSnapshot {
            health: g.health.clone(),
            budgets,
            records,
            head_hash: g
                .file
                .records
                .last()
                .map(|r| r.hash.clone())
                .unwrap_or_else(|| GENESIS_HASH.to_string()),
            record_count: g.file.records.len(),
        }
    }
}

fn status_of(b: &WebSigningBudget, now_ms: u64, wallet: Option<&str>) -> &'static str {
    if b.revoked_at_ms.is_some() {
        "revoked"
    } else if now_ms >= b.expires_at_ms {
        "expired"
    } else if wallet.is_some_and(|w| !w.eq_ignore_ascii_case(&b.wallet_address)) {
        "wallet_changed"
    } else if b.used_count >= b.max_count {
        "used_up"
    } else {
        "active"
    }
}

impl GateGuard<'_> {
    /// D2 + D4: decide whether this sign-in may be auto-approved. Every failing check returns the
    /// reason; the ceremony turns it into a pending HIC-1 card.
    pub fn evaluate_siwe(
        &self,
        inp: &SiweEvalInput<'_>,
        now_ms: u64,
    ) -> Result<SiwePlan, FallThrough> {
        let f = &self.inner.file;
        if self.inner.health != StoreHealth::Ok {
            return Err(FallThrough::StoreUnavailable);
        }
        if inp.hic_required {
            return Err(FallThrough::HicRequired);
        }
        // D2 #1-3 provenance.
        let att = inp.attestation.ok_or(FallThrough::NotAttested)?;
        if att.mode != BrowserMode::Managed {
            return Err(FallThrough::AttachMode);
        }
        if att.frame != FrameKind::Top {
            return Err(FallThrough::NotTopFrame);
        }
        // D2 #4 allowlist: the attested origin must be one the member granted a budget for.
        let origin =
            siwe::normalize_allowlist_origin(&att.origin).map_err(|_| FallThrough::NoBudget)?;
        let budget = f
            .budgets
            .iter()
            .filter(|b| b.origin == origin && b.principal == inp.principal)
            .max_by_key(|b| b.id)
            .ok_or(FallThrough::NoBudget)?;
        // D2 #20 budget live.
        if budget.revoked_at_ms.is_some() {
            return Err(FallThrough::BudgetRevoked);
        }
        if now_ms >= budget.expires_at_ms {
            return Err(FallThrough::BudgetExpired);
        }
        let wallet = inp.wallet_address.ok_or(FallThrough::NoWallet)?;
        if !wallet.eq_ignore_ascii_case(&budget.wallet_address) {
            return Err(FallThrough::WalletChanged);
        }
        if budget.used_count >= budget.max_count {
            return Err(FallThrough::BudgetExhausted);
        }
        // D2 #19 taint.
        taint_ok(inp.taint, &origin)?;
        // D2 #6-18 message.
        let checked = siwe::check_budgetable(
            inp.message,
            &SiweCheckContext {
                attested_origin: &origin,
                wallet_address: wallet,
                now_ms,
            },
        )
        .map_err(FallThrough::Message)?;
        // D2 #12 nonce ledger.
        if f.ledger
            .iter()
            .any(|e| e.origin == origin && e.nonce == checked.nonce)
        {
            return Err(FallThrough::NonceReused);
        }
        // D2 #21 rate.
        let mine = f.reservations.iter().filter(|r| r.origin == origin);
        if mine
            .clone()
            .any(|r| now_ms < r.at_ms.saturating_add(SIWE_MIN_GAP_MS))
        {
            return Err(FallThrough::RateGap);
        }
        let in_window = mine.filter(|r| in_window(r.at_ms, now_ms)).count();
        if in_window >= SIWE_WINDOW_MAX as usize {
            return Err(FallThrough::RateWindow);
        }
        Ok(SiwePlan {
            budget_id: budget.id,
            origin,
            principal: inp.principal.to_string(),
            signer_address: wallet.to_ascii_lowercase(),
            payload_digest: keccak_hex(inp.message.as_bytes()),
            checked,
            budget_expires_at_ms: budget.expires_at_ms,
        })
    }

    /// Reserve, write-ahead: debit the counter, record the nonce and the window slot, and append
    /// the `Reserved` decision record, all persisted BEFORE any signature. If saving fails nothing
    /// changes in memory and nothing may be signed. Returns the record id.
    pub fn reserve(&mut self, plan: &SiwePlan, now_ms: u64) -> Result<u64, FallThrough> {
        let mut next = self.inner.file.clone();
        let b = next
            .budgets
            .iter_mut()
            .find(|b| b.id == plan.budget_id)
            .ok_or(FallThrough::NoBudget)?;
        if b.revoked_at_ms.is_some() || b.used_count >= b.max_count {
            return Err(FallThrough::BudgetExhausted);
        }
        b.used_count += 1;
        let remaining = b.max_count - b.used_count;
        // Prune ledger entries whose message has expired (a replay of one fails the expiry check)
        // and reservations outside the rolling window; neither can affect a decision any more.
        next.ledger.retain(|e| e.keep_until_ms >= now_ms);
        next.reservations
            .retain(|r| in_window(r.at_ms, now_ms) || r.at_ms > now_ms);
        next.ledger.push(LedgerEntry {
            origin: plan.origin.clone(),
            nonce: plan.checked.nonce.clone(),
            keep_until_ms: plan
                .checked
                .expiration_ms
                .saturating_add(siwe::CLOCK_SKEW_MS),
        });
        next.reservations.push(Reservation {
            origin: plan.origin.clone(),
            at_ms: now_ms,
        });
        let rid = append_record(
            &mut next,
            RecordDraft {
                kind: RecordKind::AutoSign,
                budget_id: Some(plan.budget_id),
                principal: plan.principal.clone(),
                origin: plan.origin.clone(),
                payload_digest: Some(plan.payload_digest.clone()),
                statement: plan.checked.statement.clone(),
                nonce: Some(plan.checked.nonce.clone()),
                request_id: plan.checked.request_id.clone(),
                signer_address: Some(plan.signer_address.clone()),
                at_ms: now_ms,
                remaining_after: Some(remaining),
                note: None,
            },
            RecordStatus::Reserved,
        );
        persist(
            &self.gate.path,
            key_ref(&self.inner.mac_key),
            self.gate.keyring.as_ref(),
            &mut next,
        )
        .map_err(|_| FallThrough::WriteAheadFailed)?;
        self.inner.file = next;
        Ok(rid)
    }

    /// Re-read the clock right before the signer (TLA `Sign`): the budget and the message must
    /// both still be unexpired. Revocation cannot happen here because the lock is held.
    pub fn still_signable(&self, plan: &SiwePlan, now_ms: u64) -> Result<(), FallThrough> {
        let revoked = self
            .inner
            .file
            .budgets
            .iter()
            .any(|b| b.id == plan.budget_id && b.revoked_at_ms.is_some());
        if revoked {
            return Err(FallThrough::BudgetRevoked);
        }
        if now_ms >= plan.budget_expires_at_ms || now_ms >= plan.checked.expiration_ms {
            return Err(FallThrough::ExpiredBeforeSigning);
        }
        Ok(())
    }

    /// Sign-ins left under budget `id` (0 if unknown).
    pub fn remaining_of(&self, id: u64) -> u32 {
        self.inner
            .file
            .budgets
            .iter()
            .find(|b| b.id == id)
            .map(|b| b.max_count.saturating_sub(b.used_count))
            .unwrap_or(0)
    }

    /// Close an `AutoSign` record with the signer outcome. Saving is best effort: if it fails the
    /// file still says `Reserved`, which recovery turns into `OutcomeUnknown` (never a false "not
    /// signed").
    pub fn close(&mut self, record_id: u64, status: RecordStatus) {
        if let Some(r) = self
            .inner
            .file
            .records
            .iter_mut()
            .find(|r| r.record_id == record_id)
        {
            r.status = status;
        }
        let inner = &mut *self.inner;
        let _ = persist(
            &self.gate.path,
            key_ref(&inner.mac_key),
            self.gate.keyring.as_ref(),
            &mut inner.file,
        );
    }
}

fn taint_ok(taint: &TaskTaint, origin: &str) -> Result<(), FallThrough> {
    match taint {
        TaskTaint::Clean => Ok(()),
        TaskTaint::Unknown => Err(FallThrough::TaintUnknown),
        TaskTaint::Sources(srcs) => {
            let all_same = srcs.iter().all(|s| {
                siwe::normalize_allowlist_origin(s)
                    .map(|n| n == origin)
                    .unwrap_or(false)
            });
            if srcs.is_empty() || (O3_SAME_ORIGIN_EXEMPT && all_same) {
                Ok(())
            } else {
                Err(FallThrough::Tainted)
            }
        }
    }
}

/// `t` lies in the rolling window ending at `end`: `end - W < t <= end`.
fn in_window(t: u64, end: u64) -> bool {
    t <= end && end < t.saturating_add(SIWE_WINDOW_MS)
}

// ---------------------------------------------------------------------------------------------
// Records
// ---------------------------------------------------------------------------------------------

#[derive(Default)]
struct RecordDraft {
    kind: RecordKind,
    budget_id: Option<u64>,
    principal: String,
    origin: String,
    payload_digest: Option<String>,
    statement: Option<String>,
    nonce: Option<String>,
    request_id: Option<String>,
    signer_address: Option<String>,
    at_ms: u64,
    remaining_after: Option<u32>,
    note: Option<String>,
}

fn append_record(file: &mut BudgetFile, d: RecordDraft, status: RecordStatus) -> u64 {
    let record_id = file.records.last().map(|r| r.record_id + 1).unwrap_or(1);
    let prev_hash = file
        .records
        .last()
        .map(|r| r.hash.clone())
        .unwrap_or_else(|| GENESIS_HASH.to_string());
    let mut rec = DecisionRecord {
        record_id,
        kind: d.kind,
        budget_id: d.budget_id,
        principal: d.principal,
        origin: d.origin,
        payload_digest: d.payload_digest,
        statement: d.statement,
        nonce: d.nonce,
        request_id: d.request_id,
        signer_address: d.signer_address,
        at_ms: d.at_ms,
        remaining_after: d.remaining_after,
        note: d.note,
        prev_hash,
        hash: String::new(),
        status,
    };
    rec.hash = record_hash(&rec);
    file.records.push(rec);
    record_id
}

/// SHA-256 over `prev_hash` and the record's immutable fields (everything except `hash` and
/// `status`), as canonical JSON.
pub fn record_hash(r: &DecisionRecord) -> String {
    let body = serde_json::json!([
        r.record_id,
        r.kind,
        r.budget_id,
        r.principal,
        r.origin,
        r.payload_digest,
        r.statement,
        r.nonce,
        r.request_id,
        r.signer_address,
        r.at_ms,
        r.remaining_after,
        r.note,
    ]);
    let mut h = Sha256::new();
    h.update(r.prev_hash.as_bytes());
    h.update(body.to_string().as_bytes());
    format!("0x{}", hex::encode(h.finalize()))
}

/// Verify a record chain given oldest first.
pub fn verify_chain(records: &[DecisionRecord]) -> bool {
    let mut prev = GENESIS_HASH.to_string();
    for r in records {
        if r.prev_hash != prev || record_hash(r) != r.hash {
            return false;
        }
        prev = r.hash.clone();
    }
    true
}

fn keccak_hex(bytes: &[u8]) -> String {
    use sha3::Keccak256;
    format!("0x{}", hex::encode(Keccak256::digest(bytes)))
}

/// A compact UTC date for record notes (`YYYY-MM-DD HH:MM UTC`).
fn fmt_ms(ms: u64) -> String {
    let secs = (ms / 1000) as i64;
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02} UTC",
        rem / 3600,
        (rem % 3600) / 60
    )
}

// ---------------------------------------------------------------------------------------------
// Persistence
// ---------------------------------------------------------------------------------------------

type HmacSha256 = Hmac<Sha256>;

fn key_ref(k: &Option<Zeroizing<Vec<u8>>>) -> Option<&[u8]> {
    k.as_ref().map(|v| v.as_slice())
}

fn new_key() -> Zeroizing<Vec<u8>> {
    use rand::RngCore;
    let mut k = Zeroizing::new(vec![0u8; 32]);
    rand::rngs::OsRng.fill_bytes(&mut k);
    k
}

fn mac_of(key: &[u8], body: &str) -> Option<Vec<u8>> {
    let mut m = <HmacSha256 as Mac>::new_from_slice(key).ok()?;
    m.update(body.as_bytes());
    Some(m.finalize().into_bytes().to_vec())
}

fn load(
    path: &Path,
    keyring: &dyn Keyring,
) -> (BudgetFile, StoreHealth, Option<Zeroizing<Vec<u8>>>) {
    let failed = |why: &str| {
        (
            BudgetFile::default(),
            StoreHealth::Failed(why.to_string()),
            None,
        )
    };
    let key = match keyring.get(MAC_KEY_ACCOUNT) {
        Ok(k) => k.map(Zeroizing::new),
        Err(_) => return failed("the OS keychain is unavailable, so budgets are off"),
    };
    let sealed_generation = match keyring.get(GENERATION_ACCOUNT) {
        Ok(None) => 0,
        Ok(Some(raw)) => match std::str::from_utf8(&raw)
            .ok()
            .and_then(|t| t.parse::<u64>().ok())
        {
            Some(n) => n,
            None => return failed("the budget file's saved version in the keychain is damaged"),
        },
        Err(_) => return failed("the OS keychain is unavailable, so budgets are off"),
    };
    if !path.exists() && sealed_generation > 0 {
        return failed("the budget file is missing although this device saved one before");
    }
    if !path.exists() {
        let key = match key {
            Some(k) if k.len() == 32 => k,
            _ => {
                let k = new_key();
                if keyring.set(MAC_KEY_ACCOUNT, &k).is_err() {
                    return failed("the OS keychain is unavailable, so budgets are off");
                }
                k
            }
        };
        return (BudgetFile::default(), StoreHealth::Ok, Some(key));
    }
    let Some(key) = key.filter(|k| k.len() == 32) else {
        return failed("the budget file's integrity key is missing from the keychain");
    };
    let Ok(raw) = std::fs::read_to_string(path) else {
        return failed("the budget file could not be read");
    };
    let Ok(env) = serde_json::from_str::<Envelope>(&raw) else {
        return failed("the budget file is damaged");
    };
    let Ok(tag) = hex::decode(&env.mac) else {
        return failed("the budget file is damaged");
    };
    let verified = <HmacSha256 as Mac>::new_from_slice(&key)
        .map(|mut m| {
            m.update(env.body.as_bytes());
            m.verify_slice(&tag).is_ok()
        })
        .unwrap_or(false);
    if !verified || env.v != FILE_VERSION {
        return failed("the budget file failed its integrity check");
    }
    let Ok(mut file) = serde_json::from_str::<BudgetFile>(&env.body) else {
        return failed("the budget file is damaged");
    };
    if !verify_chain(&file.records) {
        return failed("the budget decision records failed their integrity check");
    }
    if file.generation < sealed_generation {
        return failed(
            "the budget file is older than the last one saved on this device, so it may have been replaced",
        );
    }
    if file.exported_through > file.records.last().map(|r| r.record_id).unwrap_or(0) {
        return failed("the budget file is damaged");
    }
    // D8 crash recovery: a reservation whose outcome was never recorded is outcome_unknown.
    let mut changed = false;
    for r in file.records.iter_mut() {
        if r.status == RecordStatus::Reserved {
            r.status = RecordStatus::OutcomeUnknown;
            changed = true;
        }
    }
    if changed {
        // Best effort: if this save fails the in-memory view still says outcome_unknown, and the
        // next successful save writes it.
        let _ = persist(path, Some(key.as_slice()), keyring, &mut file);
    }
    (file, StoreHealth::Ok, Some(key))
}

/// Save `file` with the next generation, then seal that generation in the keychain. The file is
/// written first: a crash before the keychain update leaves the file one generation ahead, which
/// [`load`] accepts as the newer state. Sealing is best effort (a keychain write failure keeps the
/// protection at the last generation that was sealed); the save itself is what callers rely on.
fn persist(
    path: &Path,
    key: Option<&[u8]>,
    keyring: &dyn Keyring,
    file: &mut BudgetFile,
) -> std::io::Result<()> {
    let key = key.ok_or_else(|| std::io::Error::other("no integrity key"))?;
    let mut next = file.clone();
    next.generation = file.generation.saturating_add(1);
    write_file(path, key, &next)?;
    file.generation = next.generation;
    let _ = keyring.set(GENERATION_ACCOUNT, next.generation.to_string().as_bytes());
    Ok(())
}

fn write_file(path: &Path, key: &[u8], file: &BudgetFile) -> std::io::Result<()> {
    use std::io::Write;
    let body = serde_json::to_string(file).map_err(std::io::Error::other)?;
    let mac = mac_of(key, &body).ok_or_else(|| std::io::Error::other("mac"))?;
    let env = Envelope {
        v: FILE_VERSION,
        body,
        mac: hex::encode(mac),
    };
    let text = serde_json::to_string(&env).map_err(std::io::Error::other)?;
    if let Some(dir) = path.parent() {
        crate::fsutil::ensure_private_dir(dir)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    {
        let mut f = crate::fsutil::create_secret_file(&tmp)?;
        f.write_all(text.as_bytes())?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    include!("web_budget_tests.rs");
}
