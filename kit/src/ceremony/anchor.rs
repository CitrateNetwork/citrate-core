//! HUP-S7.3 (core half): the anchor ceremony (ADR-2026-09-30-rule3-budgetable-signatures, D5).
//!
//! The nightly anchor commits one day of local decision records to `AnchorRegistry` on 40204.
//! It is signed by a separate **anchor key**, never by the wallet key:
//!
//! - **Separate key.** Generated from the OS CSPRNG, not derived from the wallet seed, sealed in
//!   the OS keyring under [`ANCHOR_KEY_ACCOUNT`]. Only its address leaves this module. The sidecar
//!   never sees it (it only batches records and builds calldata).
//! - **Single purpose, in code.** A request is accepted only for
//!   `AnchorRegistry.anchor(AnchorKind.NightlyMerkle, root)` on the pinned registry address, chain
//!   40204, value 0. The signer rebuilds the transaction from the day's commitment alone, so it
//!   cannot be pointed at another destination, selector, value or payload.
//! - **Single use.** Like [`super::SignatureCeremony`], a ceremony is consumed before anything is
//!   signed: a replayed or duplicate approval finds nothing.
//! - **Honest receipts.** [`receipt_confirms`] is true only for a mined receipt with status 1. A
//!   caller marks a day anchored only then.
//!
//! What this module does **not** decide (pending owner sign-off, O-5 in the ADR): how the anchor
//! key pays gas (relayer or a capped gas float), how the key is bound on chain as the member's
//! anchor delegate (the deployed contract records `msg.sender` and has no delegate registry), and
//! whether approval may run unattended at night (HIC-2) instead of one approval per day (HIC-1).
//! Until those are decided every anchor waits for an explicit approval of its [`AnchorCeremonyView`]
//! id, and the schedule that raises them ships off because `AnchorRegistry` is not deployed on
//! 40204 yet.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use citrate_wallet_core::{sign_eip155_legacy_tx, LegacyTxFields};
use serde::{Deserialize, Serialize};
use sha3::{Digest, Keccak256};
use zeroize::Zeroizing;

use super::DecodedAction;
use crate::custody::Keyring;
use crate::rpc::{RpcClient, RpcError, RpcTransport};

/// The keyring account holding the anchor key (32 raw secp256k1 secret bytes).
pub const ANCHOR_KEY_ACCOUNT: &str = "hermes-anchor-key-v1";
/// `keccak256("anchor(uint8,bytes32)")[..4]`.
pub const ANCHOR_SELECTOR: [u8; 4] = [0x9e, 0x62, 0x1f, 0x4c];
/// `AnchorRegistry.AnchorKind.NightlyMerkle`.
pub const NIGHTLY_MERKLE: u8 = 2;
/// The only chain an anchor is signed for.
pub const ANCHOR_CHAIN_ID: u64 = 40_204;
/// The origin shown on every anchor approval card.
pub const ANCHOR_ORIGIN: &str = "hermes:nightly-anchor";

/// Errors. None carries key material.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnchorError {
    /// The OS keyring could not be reached (fail closed).
    Keyring,
    /// The stored anchor key is not a valid secp256k1 secret. It is never silently replaced.
    KeyCorrupt,
    /// No anchor key exists yet.
    NoAnchorKey,
    /// The request is not the single call this signer makes.
    NotAnchorCall(String),
    /// The request's destination is not the pinned `AnchorRegistry`.
    RegistryMismatch,
    /// A different commitment is already pending for that day.
    DayConflict,
    /// No pending anchor ceremony with that id (never created or already consumed).
    UnknownCeremony,
    /// The RPC failed before the transaction was accepted.
    Rpc(String),
    /// Signing failed.
    Sign,
    /// The custody vault is locked: the anchor key is used only while the member is present.
    Locked,
    /// The live gas price or estimate is over the cap; nothing was signed.
    GasOverCap(String),
    /// The in-flight record could not be written before sending; nothing was sent.
    NotRecorded(String),
}

impl std::fmt::Display for AnchorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AnchorError::Keyring => write!(f, "the OS keyring is unavailable"),
            AnchorError::KeyCorrupt => write!(f, "the stored anchor key is not usable"),
            AnchorError::NoAnchorKey => write!(f, "no anchor key has been created yet"),
            AnchorError::NotAnchorCall(m) => write!(f, "not a nightly anchor call: {m}"),
            AnchorError::RegistryMismatch => {
                write!(f, "the destination is not the pinned AnchorRegistry")
            }
            AnchorError::DayConflict => {
                write!(f, "another commitment is already pending for that day")
            }
            AnchorError::UnknownCeremony => write!(f, "no pending anchor with that id"),
            AnchorError::Rpc(m) => write!(f, "chain RPC failed: {m}"),
            AnchorError::Sign => write!(f, "signing failed"),
            AnchorError::Locked => write!(f, "unlock your wallet first; nothing was signed"),
            AnchorError::GasOverCap(m) => write!(f, "{m}; nothing was signed"),
            AnchorError::NotRecorded(m) => write!(
                f,
                "the anchor could not be recorded before sending ({m}); nothing was sent"
            ),
        }
    }
}

impl std::error::Error for AnchorError {}

pub type Result<T> = std::result::Result<T, AnchorError>;

/// `anchor(NightlyMerkle, root)` calldata (68 bytes).
pub fn anchor_calldata(root: &[u8; 32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(68);
    out.extend_from_slice(&ANCHOR_SELECTOR);
    let mut word = [0u8; 32];
    word[31] = NIGHTLY_MERKLE;
    out.extend_from_slice(&word);
    out.extend_from_slice(root);
    out
}

/// Strictly decode `anchor(NightlyMerkle, root)` calldata and return the root. Any other length,
/// selector, kind or a dirty kind word is refused.
pub fn decode_anchor_calldata(data: &[u8]) -> Result<[u8; 32]> {
    if data.len() != 68 {
        return Err(AnchorError::NotAnchorCall(format!(
            "expected 68 bytes, got {}",
            data.len()
        )));
    }
    if data[..4] != ANCHOR_SELECTOR {
        return Err(AnchorError::NotAnchorCall("wrong selector".into()));
    }
    if data[4..35].iter().any(|b| *b != 0) || data[35] != NIGHTLY_MERKLE {
        return Err(AnchorError::NotAnchorCall(
            "kind is not NightlyMerkle".into(),
        ));
    }
    let mut root = [0u8; 32];
    root.copy_from_slice(&data[36..68]);
    Ok(root)
}

/// The last UTC day an anchor card may name (9999-12-31).
const MAX_ANCHOR_DAY: u64 = 2_932_896;

/// `YYYY-MM-DD` of a UTC day number (days since 1970-01-01), computed here so the date on an
/// approval card never comes from across the process boundary. `None` past 9999-12-31.
pub fn date_of_day(day: u64) -> Option<String> {
    if day > MAX_ANCHOR_DAY {
        return None;
    }
    // Proleptic Gregorian civil date from a day count (H. Hinnant's days_to_civil).
    let z = day + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + u64::from(m <= 2);
    Some(format!("{y:04}-{m:02}-{d:02}"))
}

fn parse_address(s: &str) -> Option<[u8; 20]> {
    let h = s.strip_prefix("0x")?;
    if h.len() != 40 {
        return None;
    }
    let mut out = [0u8; 20];
    hex::decode_to_slice(h, &mut out).ok()?;
    Some(out)
}

/// The `0x…` lowercase address of a verifying key.
pub fn address_of(vk: &k256::ecdsa::VerifyingKey) -> String {
    let point = vk.to_encoded_point(false);
    let h = Keccak256::digest(&point.as_bytes()[1..]);
    format!("0x{}", hex::encode(&h[12..]))
}

fn load_key(keyring: &dyn Keyring) -> Result<Option<k256::ecdsa::SigningKey>> {
    let Some(bytes) = keyring
        .get(ANCHOR_KEY_ACCOUNT)
        .map_err(|_| AnchorError::Keyring)?
    else {
        return Ok(None);
    };
    let bytes = Zeroizing::new(bytes);
    if bytes.len() != 32 {
        return Err(AnchorError::KeyCorrupt);
    }
    k256::ecdsa::SigningKey::from_slice(&bytes)
        .map(Some)
        .map_err(|_| AnchorError::KeyCorrupt)
}

/// The anchor key's address, or `None` when no key exists yet.
pub fn anchor_address(keyring: &dyn Keyring) -> Result<Option<String>> {
    Ok(load_key(keyring)?.map(|k| address_of(k.verifying_key())))
}

/// Create the anchor key if there is none (OS CSPRNG, independent of the wallet seed) and return
/// its address. An existing key is kept; a corrupt one is reported, never replaced.
pub fn ensure_anchor_key(keyring: &dyn Keyring) -> Result<String> {
    if let Some(k) = load_key(keyring)? {
        return Ok(address_of(k.verifying_key()));
    }
    let key = k256::ecdsa::SigningKey::random(&mut rand::rngs::OsRng);
    let secret = Zeroizing::new(key.to_bytes().to_vec());
    keyring
        .set(ANCHOR_KEY_ACCOUNT, &secret)
        .map_err(|_| AnchorError::Keyring)?;
    Ok(address_of(key.verifying_key()))
}

/// What a caller asks to anchor: one day's commitment and the unsigned call the batcher built
/// for it (crossed a process boundary, so it is rechecked here).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnchorRequest {
    /// UTC day number.
    pub day: u64,
    /// `YYYY-MM-DD`.
    pub date: String,
    #[serde(with = "hex32")]
    pub commitment: [u8; 32],
    pub to: String,
    pub chain_id: u64,
    pub value: u64,
    #[serde(with = "hex_bytes")]
    pub data: Vec<u8>,
}

/// The approval card for one pending anchor. No key material.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnchorCeremonyView {
    pub id: String,
    pub origin: String,
    pub day: u64,
    pub date: String,
    pub commitment: String,
    pub registry: String,
    pub chain_id: u64,
    pub decoded: DecodedAction,
}

/// Highest gas price an anchor is signed at (50 gwei). Placeholder, pending owner sign-off (O-5).
pub const PLACEHOLDER_MAX_GAS_PRICE_WEI: u128 = 50_000_000_000;
/// Highest gas limit an anchor is signed with. This only stops a hostile or broken estimate; it
/// must leave room for the real call. Measured on the anvil rehearsal (2026-10-01,
/// `scripts/anvil-anchor-e2e.sh`): the registry version the next redeploy ships keeps a second,
/// per-committer record and estimates about 335,000 gas, so the earlier 200,000 cap would have
/// refused every anchor there. Placeholder, pending owner sign-off (O-5).
pub const PLACEHOLDER_MAX_GAS_LIMIT: u64 = 400_000;
/// The two caps above are conservative placeholders until the owner decides O-5.
pub const GAS_CAPS_PENDING_OWNER_SIGNOFF: bool = true;

/// Receipt-poll budget and gas caps for [`AnchorCeremony::approve_and_broadcast`].
#[derive(Debug, Clone, Copy)]
pub struct AnchorTxConfig {
    pub poll_attempts: u32,
    pub poll_interval: std::time::Duration,
    /// Refuse to sign when the live gas price is above this (wei).
    pub max_gas_price_wei: u128,
    /// Refuse to sign when the gas estimate is above this.
    pub max_gas_limit: u64,
}

impl AnchorTxConfig {
    /// The given receipt poll with the placeholder gas caps.
    pub fn with_placeholder_caps(poll_attempts: u32, poll_interval: std::time::Duration) -> Self {
        AnchorTxConfig {
            poll_attempts,
            poll_interval,
            max_gas_price_wei: PLACEHOLDER_MAX_GAS_PRICE_WEI,
            max_gas_limit: PLACEHOLDER_MAX_GAS_LIMIT,
        }
    }
}

/// What an approval must hold before the anchor key is used.
pub struct AnchorGuards<'a> {
    /// The member's custody vault: the anchor key is used only while it is unlocked.
    pub vault: &'a crate::custody::CustodyVault,
    /// Records the signed, not yet sent transaction (day, commitment, hash) durably. Called once,
    /// after signing and before sending; if it fails nothing is sent. This is what lets a restart
    /// know a day is already on the way, so it never raises a second anchor for it.
    pub before_send: &'a dyn Fn(&AnchorReceipt) -> std::result::Result<(), String>,
    /// Called with the same record when the send failed and the node then says it does not hold
    /// the transaction (and the send error was not "already known"), so it was never accepted and
    /// can never be mined: the caller forgets the in-flight record, or the day would wait on it
    /// forever (for example an unfunded anchor key). Not called when the outcome is unknown (the
    /// lookup failed, or the node holds or already knows the transaction); then the record stays
    /// and the re-poll decides. The card stays pending for another try.
    pub not_sent: &'a dyn Fn(&AnchorReceipt),
}

/// Whether a send error proves the node did not accept the transaction. Only a JSON-RPC error
/// object does; a node that reports it already knows the transaction did accept it.
pub fn send_refused(e: &RpcError) -> bool {
    match e {
        RpcError::Node(m) => {
            let m = m.to_ascii_lowercase();
            !(m.contains("already known") || m.contains("known transaction"))
        }
        _ => false,
    }
}

/// What happened to a broadcast anchor. `block_number` / `status` are `None` while unknown.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnchorReceipt {
    pub day: u64,
    #[serde(with = "hex32")]
    pub commitment: [u8; 32],
    pub tx_hash: String,
    pub block_number: Option<u64>,
    pub status: Option<u64>,
    /// The transaction's nonce (absent in records from before it was kept).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nonce: Option<u64>,
    /// The anchor key's address that sent it (absent in records from before it was kept).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    /// HUP-S7.5 (D-27): gas the mined transaction used, once known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gas_used: Option<u64>,
    /// HUP-S7.5 (D-27): wei per gas actually paid (decimal), once known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effective_gas_price_wei: Option<String>,
}

/// A day may be marked anchored only on a mined receipt whose status is 1.
pub fn receipt_confirms(r: &AnchorReceipt) -> bool {
    r.block_number.is_some() && r.status == Some(1)
}

struct Pending {
    req: AnchorRequest,
    registry: [u8; 20],
}

/// Pending anchor ceremonies. Separate from the wallet ceremony on purpose: nothing here can
/// reach the wallet key, and nothing in the wallet ceremony can reach this key.
pub struct AnchorCeremony {
    pending: Mutex<BTreeMap<u64, Pending>>,
    next_id: AtomicU64,
}

impl Default for AnchorCeremony {
    fn default() -> Self {
        Self::new()
    }
}

fn view(id: u64, p: &Pending) -> AnchorCeremonyView {
    let registry = format!("0x{}", hex::encode(p.registry));
    AnchorCeremonyView {
        id: id.to_string(),
        origin: ANCHOR_ORIGIN.to_string(),
        day: p.req.day,
        date: p.req.date.clone(),
        commitment: format!("0x{}", hex::encode(p.req.commitment)),
        registry: registry.clone(),
        chain_id: ANCHOR_CHAIN_ID,
        decoded: DecodedAction {
            action: format!(
                "Anchor the decision records of {} (UTC) to AnchorRegistry",
                p.req.date
            ),
            cost: "0 SALT value; gas is paid by the anchor key".to_string(),
            destination: registry,
        },
    }
}

impl AnchorCeremony {
    pub fn new() -> Self {
        AnchorCeremony {
            pending: Mutex::new(BTreeMap::new()),
            next_id: AtomicU64::new(1),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<u64, Pending>> {
        self.pending.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Raise a pending anchor for one day. Refuses anything but `anchor(NightlyMerkle, commitment)`
    /// to `pinned_registry` on 40204 with no value. One pending ceremony per day: asking again
    /// returns the same card; a different commitment for that day is [`AnchorError::DayConflict`].
    /// Signs nothing.
    pub fn request(&self, req: AnchorRequest, pinned_registry: &str) -> Result<AnchorCeremonyView> {
        if req.chain_id != ANCHOR_CHAIN_ID {
            return Err(AnchorError::NotAnchorCall(format!(
                "chain {} is not {ANCHOR_CHAIN_ID}",
                req.chain_id
            )));
        }
        if req.value != 0 {
            return Err(AnchorError::NotAnchorCall(
                "an anchor moves no value".into(),
            ));
        }
        let pinned = parse_address(&pinned_registry.to_ascii_lowercase())
            .ok_or(AnchorError::RegistryMismatch)?;
        let to =
            parse_address(&req.to.to_ascii_lowercase()).ok_or(AnchorError::RegistryMismatch)?;
        if to != pinned {
            return Err(AnchorError::RegistryMismatch);
        }
        // The card shows a date: it must be the stated day's own date, computed here.
        if date_of_day(req.day).as_deref() != Some(req.date.as_str()) {
            return Err(AnchorError::NotAnchorCall(
                "the date does not match the day".into(),
            ));
        }
        if decode_anchor_calldata(&req.data)? != req.commitment {
            return Err(AnchorError::NotAnchorCall(
                "the calldata does not anchor the stated commitment".into(),
            ));
        }
        let mut map = self.lock();
        if let Some((id, p)) = map.iter().find(|(_, p)| p.req.day == req.day) {
            return if p.req.commitment == req.commitment {
                Ok(view(*id, p))
            } else {
                Err(AnchorError::DayConflict)
            };
        }
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let p = Pending {
            req,
            registry: pinned,
        };
        let v = view(id, &p);
        map.insert(id, p);
        Ok(v)
    }

    /// Every pending anchor card, oldest first.
    pub fn pending(&self) -> Vec<AnchorCeremonyView> {
        self.lock().iter().map(|(id, p)| view(*id, p)).collect()
    }

    /// Drop a pending anchor without signing.
    pub fn reject(&self, id: &str) -> Result<()> {
        let key = id
            .parse::<u64>()
            .map_err(|_| AnchorError::UnknownCeremony)?;
        self.lock()
            .remove(&key)
            .map(|_| ())
            .ok_or(AnchorError::UnknownCeremony)
    }

    /// Approve one pending anchor: consume it, sign `anchor(NightlyMerkle, commitment)` to the
    /// registry it was raised for with the anchor key, record it as in flight, broadcast, and poll
    /// for the receipt.
    ///
    /// `pinned_registry` is the address the caller pins now; if it no longer matches the one the
    /// ceremony was raised for, nothing is signed. Nonce, gas price and gas limit come from the
    /// live RPC for the anchor key's own address (nothing is guessed), and are refused above the
    /// caps in `cfg`. The anchor key is used only while `guards.vault` is unlocked. The returned
    /// receipt may be unmined, unknown (the poll failed after the send) or reverted: only
    /// [`receipt_confirms`] decides whether the day is anchored. A refusal before sending keeps the
    /// card pending.
    pub fn approve_and_broadcast<T: RpcTransport>(
        &self,
        keyring: &dyn Keyring,
        rpc: &RpcClient<T>,
        id: &str,
        pinned_registry: &str,
        cfg: AnchorTxConfig,
        guards: AnchorGuards<'_>,
    ) -> Result<AnchorReceipt> {
        let key = id
            .parse::<u64>()
            .map_err(|_| AnchorError::UnknownCeremony)?;
        if !guards.vault.is_unlocked() {
            return if self.lock().contains_key(&key) {
                Err(AnchorError::Locked)
            } else {
                Err(AnchorError::UnknownCeremony)
            };
        }
        // Consume first: a duplicate approval finds nothing.
        let p = self
            .lock()
            .remove(&key)
            .ok_or(AnchorError::UnknownCeremony)?;
        // Any refusal before the send puts the card back: nothing was sent.
        let keep = |p: Pending, e: AnchorError| -> AnchorError {
            self.lock().insert(key, p);
            e
        };
        let pinned = parse_address(&pinned_registry.to_ascii_lowercase())
            .ok_or(AnchorError::RegistryMismatch)?;
        if pinned != p.registry {
            return Err(AnchorError::RegistryMismatch);
        }
        let signer = load_key(keyring)?.ok_or(AnchorError::NoAnchorKey)?;
        let from = address_of(signer.verifying_key());
        let to_hex = format!("0x{}", hex::encode(p.registry));
        // The payload is rebuilt from the commitment alone: this signer cannot carry anything else.
        let data = anchor_calldata(&p.req.commitment);
        let rpc_err = |e: RpcError| AnchorError::Rpc(e.to_string());
        let nonce = rpc.pending_nonce(&from).map_err(rpc_err)?;
        let gas_price = rpc.gas_price().map_err(rpc_err)?;
        let gas_limit = rpc
            .estimate_gas(serde_json::json!({
                "from": from,
                "to": to_hex,
                "value": "0x0",
                "data": format!("0x{}", hex::encode(&data)),
            }))
            .map_err(rpc_err)?;
        if u128::from(gas_price) > cfg.max_gas_price_wei {
            return Err(keep(
                p,
                AnchorError::GasOverCap(format!(
                    "the network gas price ({gas_price} wei) is over the anchor cap ({} wei)",
                    cfg.max_gas_price_wei
                )),
            ));
        }
        if gas_limit > cfg.max_gas_limit {
            return Err(keep(
                p,
                AnchorError::GasOverCap(format!(
                    "the gas estimate ({gas_limit}) is over the anchor cap ({})",
                    cfg.max_gas_limit
                )),
            ));
        }
        let fields = LegacyTxFields {
            nonce,
            gas_price,
            gas_limit,
            to: Some(p.registry),
            value: 0,
            data,
        };
        let signed = sign_eip155_legacy_tx(&signer, &fields, ANCHOR_CHAIN_ID)
            .map_err(|_| AnchorError::Sign)?;
        drop(signer);
        let in_flight = AnchorReceipt {
            day: p.req.day,
            commitment: p.req.commitment,
            tx_hash: format!("0x{}", hex::encode(signed.hash)),
            block_number: None,
            status: None,
            nonce: Some(nonce),
            from: Some(from.clone()),
            gas_used: None,
            effective_gas_price_wei: None,
        };
        if let Err(e) = (guards.before_send)(&in_flight) {
            return Err(keep(p, AnchorError::NotRecorded(e)));
        }
        let tx_hash = match rpc.send_raw_transaction(&signed.raw) {
            Ok(h) => h,
            Err(e) => {
                // Nothing went out only when the node says so: it answers that it does not hold
                // the transaction, and the send error was not the node saying it already knows it.
                // Any other answer (a failed lookup included) keeps the day held as possibly sent.
                let not_held = matches!(rpc.transaction_known(&in_flight.tx_hash), Ok(false))
                    && (send_refused(&e) || !matches!(e, RpcError::Node(_)));
                if not_held {
                    (guards.not_sent)(&in_flight);
                    return Err(keep(p, rpc_err(e)));
                }
                return Err(rpc_err(e));
            }
        };
        let (block_number, status, gas_used, gas_price) =
            match rpc.poll_receipt(&tx_hash, cfg.poll_attempts, cfg.poll_interval) {
                Ok(r) => (
                    Some(r.block_number),
                    r.status,
                    r.gas_used,
                    r.effective_gas_price,
                ),
                // The transaction is already sent: whether the receipt poll timed out or failed,
                // report the hash with the receipt unknown, so the caller can keep waiting on it
                // instead of raising a second anchor for the same day.
                Err(_) => (None, None, None, None),
            };
        Ok(AnchorReceipt {
            day: p.req.day,
            commitment: p.req.commitment,
            tx_hash,
            block_number,
            status,
            nonce: Some(nonce),
            from: Some(from),
            gas_used,
            effective_gas_price_wei: gas_price.map(|p| p.to_string()),
        })
    }
}

mod hex32 {
    use serde::{Deserialize, Deserializer, Serializer};

    pub(super) fn serialize<S: Serializer>(v: &[u8; 32], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&format!("0x{}", hex::encode(v)))
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[u8; 32], D::Error> {
        let s = String::deserialize(d)?;
        let h = s.strip_prefix("0x").unwrap_or(&s);
        let mut out = [0u8; 32];
        hex::decode_to_slice(h, &mut out).map_err(serde::de::Error::custom)?;
        Ok(out)
    }
}

mod hex_bytes {
    use serde::{Deserialize, Deserializer, Serializer};

    pub(super) fn serialize<S: Serializer>(v: &[u8], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&format!("0x{}", hex::encode(v)))
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let s = String::deserialize(d)?;
        hex::decode(s.strip_prefix("0x").unwrap_or(&s)).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
#[path = "anchor_tests.rs"]
mod tests;
