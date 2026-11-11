//! 0.6.0 FLM-CORE-1: the federated-learning **offers read model**.
//!
//! In 0.6.0 a federated round is a `WorkMarket` offer (citrate-chain
//! `contracts/src/work/WorkMarket.sol`, merged in #451): a poster escrows SALT at `open`, workers
//! join if the price meets their floor, and the round's coordinator posts the payout. This module
//! is what core knows about those offers: which exist, what they ask for, and whether this member
//! should see each one. The matcher (FLM-CORE-2) and the join/open ceremonies (FLM-CORE-3) build on
//! it. Nothing here signs or sends a transaction.
//!
//! ## Data sources (Rule 7)
//!
//! | What | Source |
//! |---|---|
//! | The market address | this build's address book (`addresses::work_market`, generated from the canonical book). Absent = the market is not available in this build (r1006 has no WorkMarket). |
//! | Which chain | `eth_getBlockByNumber("0x0").hash` must equal the book's `genesisHash`; another generation reads nothing. |
//! | Offers | `WorkMarket` `OfferOpened` / `OfferClosed` logs (`eth_getLogs`), read forward from a persisted cursor in chunks of [`LOG_CHUNK_BLOCKS`], never past `head - CONFIRMATIONS`. |
//! | Terms | `getOffer(id)` (`eth_call`): requirements hash, job spec, coordinator, slots, join bond, allowlist, state. |
//! | Requirements | the offer's `requirementsCid` from the local IPFS node, at most [`MAX_REQUIREMENTS_BYTES`], accepted only if `keccak256(bytes) == requirementsHash`. |
//! | Group offers | the member's comms groups (`groupHash = keccak256(32-byte MLS group id)`) and each group's live roster; the poster's wallet is tied to a roster member only by a verified DeviceLink (wallet, member and device signatures all recover). |
//!
//! ## What a member sees
//!
//! An offer is shown only when its terms and requirements were read and verified, it is still Open
//! or Running before its deadline, and, for a group offer, the member is in that group **and** the
//! poster's wallet is linked to an `owner` or `admin` on that group's roster. Every hidden offer
//! carries the reason ([`Hidden`]), so the "why not" view (FLM-CORE-2) has something true to say.
//!
//! ## The requirements file
//!
//! `requirementsHash` is `keccak256` of the exact bytes at `requirementsCid` (the same hash the
//! poster's open ceremony commits). The file is JSON, version 1, with no other fields:
//! `{"v":1,"dataClasses":[..],"architectures":[..],"minVramGb":N,"estHours":N}`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha3::{Digest, Keccak256};

use crate::rpc::RpcTransport;

/// `OfferOpened(uint256 indexed id, Kind kind, address indexed poster, bytes32 indexed groupHash,
/// uint128 pricePerUnit, uint128 budget, uint64 deadline, string requirementsCid)`.
pub const OFFER_OPENED_SIG: &str =
    "OfferOpened(uint256,uint8,address,bytes32,uint128,uint128,uint64,string)";
/// `OfferClosed(uint256 indexed id, State finalState)`.
pub const OFFER_CLOSED_SIG: &str = "OfferClosed(uint256,uint8)";
/// Blocks per `eth_getLogs` (the node serves at most 1,001 and silently cuts a longer range).
pub const LOG_CHUNK_BLOCKS: u64 = 1000;
/// Chunks one sync reads before it stops and leaves the rest for the next sync.
pub const MAX_CHUNKS_PER_SYNC: u64 = 64;
/// The read model never reads the newest blocks: an offer appears once it is this deep.
pub const CONFIRMATIONS: u64 = 12;
/// The longest `requirementsCid` accepted (a CIDv1 base32 is ~60 characters).
pub const MAX_CID_LEN: usize = 128;
/// The largest requirements file fetched.
pub const MAX_REQUIREMENTS_BYTES: usize = 16 * 1024;
/// `getOffer(uint256)` returns the `Offer` struct as this many static words.
pub const OFFER_WORDS: usize = 38;

fn keccak(data: &[u8]) -> [u8; 32] {
    Keccak256::digest(data).into()
}

fn hex0x(b: &[u8]) -> String {
    format!("0x{}", hex::encode(b))
}

/// topic0 of an event signature.
pub fn topic(sig: &str) -> String {
    hex0x(&keccak(sig.as_bytes()))
}

/// Lowercase `0x` + 40 hex, or `None`.
fn norm_address(s: &str) -> Option<String> {
    let h = s.trim().strip_prefix("0x").unwrap_or(s.trim());
    (h.len() == 40 && h.bytes().all(|b| b.is_ascii_hexdigit()))
        .then(|| format!("0x{}", h.to_ascii_lowercase()))
}

/// `groupHash` for a comms group: `keccak256` of its 32-byte MLS group id (the daemon's hex id).
pub fn group_hash(group_id_hex: &str) -> Option<String> {
    let raw = hex::decode(group_id_hex.trim().trim_start_matches("0x")).ok()?;
    (raw.len() == 32).then(|| hex0x(&keccak(&raw)))
}

/// Why the read model refused a chunk, a log or a return. Every one leaves the model unchanged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MarketError {
    /// The chunk does not start where the cursor is (a gap or an overlap).
    Gap { expected: u64, got: u64 },
    /// A chunk whose end is before its start.
    BadRange { from: u64, to: u64 },
    /// A log came back outside the asked block range.
    OutOfRange { block: u64, from: u64, to: u64 },
    /// A log from another contract.
    WrongEmitter(String),
    /// A log that is neither OfferOpened nor OfferClosed, or does not decode.
    BadLog(String),
    /// The same offer id opened twice with different terms.
    Conflict(u64),
    /// OfferClosed for an offer the model never saw open.
    UnknownOffer(u64),
    /// A `getOffer` return of another shape.
    BadOffer(String),
}

impl std::fmt::Display for MarketError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MarketError::Gap { expected, got } => {
                write!(f, "log chunk starts at {got}, the cursor is at {expected}")
            }
            MarketError::BadRange { from, to } => write!(f, "bad block range {from}..{to}"),
            MarketError::OutOfRange { block, from, to } => {
                write!(f, "a log at block {block} came back for range {from}..{to}")
            }
            MarketError::WrongEmitter(a) => write!(f, "a log from {a}, not the market"),
            MarketError::BadLog(e) => write!(f, "a market log does not decode: {e}"),
            MarketError::Conflict(id) => write!(f, "offer {id} opened twice with different terms"),
            MarketError::UnknownOffer(id) => write!(f, "offer {id} closed but was never seen open"),
            MarketError::BadOffer(e) => write!(f, "getOffer return: {e}"),
        }
    }
}
impl std::error::Error for MarketError {}

/// `IWorkMarket.Kind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OfferKind {
    FlRound,
    Task,
}

/// `IWorkMarket.State`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OfferState {
    Open,
    Running,
    PayoutPosted,
    Disputed,
    Closed,
    Expired,
}

impl OfferState {
    fn from_code(c: u128) -> Option<Self> {
        Some(match c {
            0 => Self::Open,
            1 => Self::Running,
            2 => Self::PayoutPosted,
            3 => Self::Disputed,
            4 => Self::Closed,
            5 => Self::Expired,
            _ => return None,
        })
    }
}

/// One raw log as the node returned it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawLog {
    pub address: String,
    pub topics: Vec<String>,
    pub data: Vec<u8>,
    pub block_number: u64,
    pub log_index: u64,
}

fn hex_u64(v: &Value) -> Option<u64> {
    u64::from_str_radix(v.as_str()?.trim_start_matches("0x"), 16).ok()
}

/// Parse one `eth_getLogs` entry. `None` for anything malformed (the sync then refuses the chunk).
pub fn parse_log(v: &Value) -> Option<RawLog> {
    let topics = v
        .get("topics")?
        .as_array()?
        .iter()
        .map(|t| t.as_str().map(|s| s.to_ascii_lowercase()))
        .collect::<Option<Vec<_>>>()?;
    Some(RawLog {
        address: norm_address(v.get("address")?.as_str()?)?,
        topics,
        data: hex::decode(v.get("data")?.as_str()?.trim_start_matches("0x")).ok()?,
        block_number: hex_u64(v.get("blockNumber")?)?,
        log_index: hex_u64(v.get("logIndex")?)?,
    })
}

/// What an `OfferOpened` log says.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenedEvent {
    pub id: u64,
    pub kind: OfferKind,
    pub poster: String,
    /// `0x` + 64 hex; all zero for an open offer.
    pub group_hash: String,
    /// Wei per unit, decimal.
    pub price_per_unit: String,
    /// Wei escrowed, decimal.
    pub budget: String,
    pub deadline: u64,
    pub requirements_cid: String,
}

impl OpenedEvent {
    pub fn is_group(&self) -> bool {
        self.group_hash
            .trim_start_matches("0x")
            .bytes()
            .any(|b| b != b'0')
    }
}

/// A decoded market log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MarketEvent {
    Opened(OpenedEvent),
    Closed { id: u64, final_state: OfferState },
}

fn word(data: &[u8], i: usize) -> Option<&[u8]> {
    data.get(i * 32..(i + 1) * 32)
}

fn word_uint(w: &[u8], bits: usize) -> Option<u128> {
    let lead = 32 - bits / 8;
    if w.len() != 32 || w[..lead].iter().any(|b| *b != 0) {
        return None;
    }
    let mut b = [0u8; 16];
    b.copy_from_slice(&w[16..]);
    Some(u128::from_be_bytes(b))
}

fn topic_bytes(t: &str) -> Option<Vec<u8>> {
    let b = hex::decode(t.trim_start_matches("0x")).ok()?;
    (b.len() == 32).then_some(b)
}

fn topic_u64(t: &str) -> Option<u64> {
    u64::try_from(word_uint(&topic_bytes(t)?, 256)?).ok()
}

fn topic_address(t: &str) -> Option<String> {
    let b = topic_bytes(t)?;
    if b[..12].iter().any(|x| *x != 0) {
        return None;
    }
    Some(hex0x(&b[12..]))
}

/// A CID we are willing to hand to the IPFS node: printable base-encoded ASCII, bounded.
fn cid_is_plausible(s: &str) -> bool {
    !s.is_empty() && s.len() <= MAX_CID_LEN && s.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// Decode one market log (topic0 decides which event).
pub fn decode_event(log: &RawLog) -> Result<MarketEvent, MarketError> {
    let bad = |e: &str| {
        MarketError::BadLog(format!(
            "block {} log {}: {e}",
            log.block_number, log.log_index
        ))
    };
    let t0 = log.topics.first().ok_or_else(|| bad("no topics"))?;
    if *t0 == topic(OFFER_OPENED_SIG) {
        if log.topics.len() != 4 {
            return Err(bad("OfferOpened needs 4 topics"));
        }
        let id = topic_u64(&log.topics[1]).ok_or_else(|| bad("offer id"))?;
        let poster = topic_address(&log.topics[2]).ok_or_else(|| bad("poster"))?;
        let group = topic_bytes(&log.topics[3]).ok_or_else(|| bad("groupHash"))?;
        let d = &log.data;
        let kind = match word(d, 0).and_then(|w| word_uint(w, 8)) {
            Some(0) => OfferKind::FlRound,
            Some(1) => OfferKind::Task,
            _ => return Err(bad("kind")),
        };
        let price = word(d, 1)
            .and_then(|w| word_uint(w, 128))
            .ok_or_else(|| bad("price"))?;
        let budget = word(d, 2)
            .and_then(|w| word_uint(w, 128))
            .ok_or_else(|| bad("budget"))?;
        let deadline = word(d, 3)
            .and_then(|w| word_uint(w, 64))
            .and_then(|v| u64::try_from(v).ok())
            .ok_or_else(|| bad("deadline"))?;
        let off = word(d, 4)
            .and_then(|w| word_uint(w, 64))
            .and_then(|v| usize::try_from(v).ok())
            .filter(|o| *o == 5 * 32)
            .ok_or_else(|| bad("string offset"))?;
        let len = d
            .get(off..off + 32)
            .and_then(|w| word_uint(w, 64))
            .and_then(|v| usize::try_from(v).ok())
            .filter(|l| *l <= MAX_CID_LEN)
            .ok_or_else(|| bad("requirementsCid length"))?;
        let padded = len.div_ceil(32) * 32;
        if d.len() != off + 32 + padded {
            return Err(bad("data length"));
        }
        let raw = &d[off + 32..off + 32 + len];
        if d[off + 32 + len..].iter().any(|b| *b != 0) {
            return Err(bad("string padding"));
        }
        let cid = std::str::from_utf8(raw).map_err(|_| bad("requirementsCid is not UTF-8"))?;
        Ok(MarketEvent::Opened(OpenedEvent {
            id,
            kind,
            poster,
            group_hash: hex0x(&group),
            price_per_unit: price.to_string(),
            budget: budget.to_string(),
            deadline,
            requirements_cid: cid.to_string(),
        }))
    } else if *t0 == topic(OFFER_CLOSED_SIG) {
        if log.topics.len() != 2 || log.data.len() != 32 {
            return Err(bad("OfferClosed shape"));
        }
        let id = topic_u64(&log.topics[1]).ok_or_else(|| bad("offer id"))?;
        let final_state = word_uint(&log.data, 8)
            .and_then(OfferState::from_code)
            .ok_or_else(|| bad("final state"))?;
        Ok(MarketEvent::Closed { id, final_state })
    } else {
        Err(bad("not a market event"))
    }
}

/// The parts of `getOffer(id)` discovery needs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OfferTerms {
    pub state: OfferState,
    pub job_spec_hash: String,
    pub requirements_hash: String,
    pub allowlist_root: String,
    pub max_workers: u32,
    pub workers: u32,
    /// Wei, decimal.
    pub join_bond: String,
    pub coordinator: String,
    pub dispute_window: u32,
}

/// Decode `getOffer(id)` (38 static words) for the offer `opened` announced. The return must agree
/// with the event (poster, kind, price, budget, deadline, group), or it is refused.
pub fn decode_offer_terms(ret: &[u8], opened: &OpenedEvent) -> Result<OfferTerms, MarketError> {
    let bad = |e: &str| MarketError::BadOffer(format!("offer {}: {e}", opened.id));
    if ret.len() != OFFER_WORDS * 32 {
        return Err(bad(&format!(
            "{} bytes, expected {}",
            ret.len(),
            OFFER_WORDS * 32
        )));
    }
    let w = |i: usize| &ret[i * 32..(i + 1) * 32];
    let uint =
        |i: usize, bits: usize| word_uint(w(i), bits).ok_or_else(|| bad(&format!("word {i}")));
    let addr = |i: usize| {
        let x = w(i);
        if x[..12].iter().any(|b| *b != 0) {
            return Err(bad(&format!("word {i} is not an address")));
        }
        Ok(hex0x(&x[12..]))
    };
    let kind = match uint(0, 8)? {
        0 => OfferKind::FlRound,
        1 => OfferKind::Task,
        _ => return Err(bad("kind")),
    };
    let state = OfferState::from_code(uint(1, 8)?).ok_or_else(|| bad("state"))?;
    let agrees = kind == opened.kind
        && addr(2)? == opened.poster
        && hex0x(w(7)) == opened.group_hash
        && uint(9, 128)?.to_string() == opened.price_per_unit
        && uint(10, 128)?.to_string() == opened.budget
        && u64::try_from(uint(14, 64)?).ok() == Some(opened.deadline);
    if !agrees {
        return Err(bad("the offer's terms do not match its OfferOpened log"));
    }
    let u32_at = |i: usize| -> Result<u32, MarketError> {
        u32::try_from(uint(i, 32)?).map_err(|_| bad(&format!("word {i}")))
    };
    Ok(OfferTerms {
        state,
        job_spec_hash: hex0x(w(5)),
        requirements_hash: hex0x(w(6)),
        allowlist_root: hex0x(w(8)),
        dispute_window: u32_at(15)?,
        max_workers: u32_at(16)?,
        workers: u32_at(17)?,
        join_bond: uint(19, 128)?.to_string(),
        coordinator: addr(20)?,
    })
}

/// The requirements file (version 1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Requirements {
    pub v: u32,
    pub data_classes: Vec<String>,
    pub architectures: Vec<String>,
    pub min_vram_gb: u32,
    pub est_hours: u32,
}

/// Why a requirements file was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RequirementsError {
    TooLarge(usize),
    HashMismatch { expected: String, got: String },
    Malformed(String),
}

impl std::fmt::Display for RequirementsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RequirementsError::TooLarge(n) => {
                write!(
                    f,
                    "the requirements file is {n} bytes (max {MAX_REQUIREMENTS_BYTES})"
                )
            }
            RequirementsError::HashMismatch { expected, got } => write!(
                f,
                "the requirements file hashes to {got}, the offer committed {expected}"
            ),
            RequirementsError::Malformed(e) => write!(f, "the requirements file is malformed: {e}"),
        }
    }
}
impl std::error::Error for RequirementsError {}

fn label_ok(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._-".contains(&b))
}

/// Accept `bytes` as the requirements of an offer that committed `expected_hash`.
pub fn verify_requirements(
    bytes: &[u8],
    expected_hash: &str,
) -> Result<Requirements, RequirementsError> {
    if bytes.len() > MAX_REQUIREMENTS_BYTES {
        return Err(RequirementsError::TooLarge(bytes.len()));
    }
    let got = hex0x(&keccak(bytes));
    if got != expected_hash.to_ascii_lowercase() {
        return Err(RequirementsError::HashMismatch {
            expected: expected_hash.to_ascii_lowercase(),
            got,
        });
    }
    let r: Requirements =
        serde_json::from_slice(bytes).map_err(|e| RequirementsError::Malformed(e.to_string()))?;
    if r.v != 1 {
        return Err(RequirementsError::Malformed(format!("version {}", r.v)));
    }
    for list in [&r.data_classes, &r.architectures] {
        if list.is_empty() || list.len() > 32 || !list.iter().all(|s| label_ok(s)) {
            return Err(RequirementsError::Malformed(
                "dataClasses and architectures need 1-32 lowercase labels".into(),
            ));
        }
    }
    if r.min_vram_gb > 1024 || r.est_hours == 0 || r.est_hours > 24 * 90 {
        return Err(RequirementsError::Malformed(
            "minVramGb or estHours out of range".into(),
        ));
    }
    Ok(r)
}

/// Where an offer's requirements stand.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "status")]
pub enum RequirementsRead {
    /// Not fetched yet (or the last fetch failed and will be retried).
    NotRead { reason: String },
    /// Fetched and the hash matched.
    Verified { requirements: Requirements },
    /// Fetched and refused (a hash mismatch or a malformed file). Never retried.
    Refused { reason: String },
}

/// One offer in the read model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OfferEntry {
    pub opened: OpenedEvent,
    pub opened_block: u64,
    /// The final state from `OfferClosed`, if one was logged.
    pub closed: Option<OfferState>,
    /// The latest `getOffer` read, if any.
    pub terms: Option<OfferTerms>,
    pub requirements: RequirementsRead,
}

/// The persisted read model: offers known so far and the next block to read. Bound to one market
/// address and one chain generation; a model for anything else is discarded, not reused.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OfferBook {
    pub market: String,
    pub genesis_hash: String,
    pub next_block: u64,
    pub offers: BTreeMap<u64, OfferEntry>,
}

impl OfferBook {
    /// An empty model for `market` on the chain whose block 0 is `genesis_hash`, reading from
    /// `start_block` (0 for a market deployed in the genesis block).
    pub fn new(market: &str, genesis_hash: &str, start_block: u64) -> Self {
        Self {
            market: market.to_ascii_lowercase(),
            genesis_hash: genesis_hash.to_ascii_lowercase(),
            next_block: start_block,
            offers: BTreeMap::new(),
        }
    }

    /// Whether this model belongs to `market` on the generation `genesis_hash`.
    pub fn bound_to(&self, market: &str, genesis_hash: &str) -> bool {
        self.market.eq_ignore_ascii_case(market)
            && self.genesis_hash.eq_ignore_ascii_case(genesis_hash)
    }

    /// Apply the logs of blocks `from..=to`. All or nothing: any refused log leaves the model as it
    /// was and the cursor unmoved. Returns the ids of offers first seen in this chunk.
    pub fn apply_chunk(
        &mut self,
        from: u64,
        to: u64,
        logs: &[RawLog],
    ) -> Result<Vec<u64>, MarketError> {
        if from != self.next_block {
            return Err(MarketError::Gap {
                expected: self.next_block,
                got: from,
            });
        }
        if to < from {
            return Err(MarketError::BadRange { from, to });
        }
        let mut sorted: Vec<&RawLog> = logs.iter().collect();
        sorted.sort_by_key(|l| (l.block_number, l.log_index));
        sorted.dedup_by_key(|l| (l.block_number, l.log_index));
        let mut next = self.offers.clone();
        let mut fresh = Vec::new();
        for l in sorted {
            if l.block_number < from || l.block_number > to {
                return Err(MarketError::OutOfRange {
                    block: l.block_number,
                    from,
                    to,
                });
            }
            if l.address != self.market {
                return Err(MarketError::WrongEmitter(l.address.clone()));
            }
            match decode_event(l)? {
                MarketEvent::Opened(ev) => match next.get(&ev.id) {
                    Some(e) if e.opened == ev => {}
                    Some(_) => return Err(MarketError::Conflict(ev.id)),
                    None => {
                        fresh.push(ev.id);
                        let requirements = if cid_is_plausible(&ev.requirements_cid) {
                            RequirementsRead::NotRead {
                                reason: "not fetched yet".into(),
                            }
                        } else {
                            RequirementsRead::Refused {
                                reason: "the requirements CID is not a plausible CID".into(),
                            }
                        };
                        next.insert(
                            ev.id,
                            OfferEntry {
                                opened: ev,
                                opened_block: l.block_number,
                                closed: None,
                                terms: None,
                                requirements,
                            },
                        );
                    }
                },
                MarketEvent::Closed { id, final_state } => match next.get_mut(&id) {
                    Some(e) => e.closed = Some(final_state),
                    None => return Err(MarketError::UnknownOffer(id)),
                },
            }
        }
        self.offers = next;
        self.next_block = to.saturating_add(1);
        Ok(fresh)
    }

    /// Whether an offer can still take workers, from what the model knows (`now` = a block time).
    fn is_live(e: &OfferEntry, now: u64) -> bool {
        e.closed.is_none()
            && now <= e.opened.deadline
            && e.terms
                .as_ref()
                .is_none_or(|t| matches!(t.state, OfferState::Open | OfferState::Running))
    }

    /// Load from `path`; a missing file is `None`, a corrupt one is an error.
    pub fn load(path: &Path) -> Result<Option<Self>, String> {
        match std::fs::read(path) {
            Ok(b) => serde_json::from_slice(&b)
                .map(Some)
                .map_err(|_| "the offers read model is corrupt".to_string()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(format!("reading the offers read model: {}", e.kind())),
        }
    }

    /// Persist atomically.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let json = serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.kind().to_string())?;
        }
        crate::atomic_file::replace(path, &json)
            .map_err(|e| format!("saving the offers read model: {}", e.kind()))
    }
}

// ── who may see an offer ────────────────────────────────────────────

/// One of the member's comms groups and its live roster `(comms address, role)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupMembership {
    pub group_id: String,
    pub roster: Vec<(String, String)>,
}

/// A wallet tied to a comms member address by a verified DeviceLink.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalletLink {
    pub wallet: String,
    pub member: String,
}

/// What visibility is decided against.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MemberContext {
    pub groups: Vec<GroupMembership>,
    pub links: Vec<WalletLink>,
}

/// Why an offer is not shown.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "reason")]
pub enum Hidden {
    /// Closed, expired, past its deadline, or its payout is already posted.
    NotTakingWorkers,
    /// The offer's terms (`getOffer`) have not been read.
    TermsNotRead,
    /// The requirements have not been fetched yet.
    RequirementsNotRead { detail: String },
    /// The requirements file was refused (hash mismatch or malformed).
    RequirementsRefused { detail: String },
    /// A group offer for a group this member is not in.
    NotYourGroup,
    /// A group offer whose poster is not an owner or admin of that group (or whose wallet is not
    /// linked to any roster member).
    PosterNotAdmin,
}

fn same_address(a: &str, b: &str) -> bool {
    match (norm_address(a), norm_address(b)) {
        (Some(x), Some(y)) => x == y,
        _ => false,
    }
}

/// Whether `wallet` is linked to an `owner` or `admin` on `roster`.
pub fn is_group_admin(roster: &[(String, String)], links: &[WalletLink], wallet: &str) -> bool {
    links
        .iter()
        .filter(|l| same_address(&l.wallet, wallet))
        .any(|l| {
            roster.iter().any(|(member, role)| {
                same_address(member, &l.member) && (role == "owner" || role == "admin")
            })
        })
}

/// Whether this member should see `entry` at block time `now`, and if not, why.
pub fn visibility(entry: &OfferEntry, ctx: &MemberContext, now: u64) -> Result<(), Hidden> {
    if !OfferBook::is_live(entry, now) {
        return Err(Hidden::NotTakingWorkers);
    }
    if entry.terms.is_none() {
        return Err(Hidden::TermsNotRead);
    }
    if entry.opened.is_group() {
        let group = ctx
            .groups
            .iter()
            .find(|g| group_hash(&g.group_id).as_deref() == Some(entry.opened.group_hash.as_str()))
            .ok_or(Hidden::NotYourGroup)?;
        if !is_group_admin(&group.roster, &ctx.links, &entry.opened.poster) {
            return Err(Hidden::PosterNotAdmin);
        }
    }
    match &entry.requirements {
        RequirementsRead::Verified { .. } => Ok(()),
        RequirementsRead::NotRead { reason } => Err(Hidden::RequirementsNotRead {
            detail: reason.clone(),
        }),
        RequirementsRead::Refused { reason } => Err(Hidden::RequirementsRefused {
            detail: reason.clone(),
        }),
    }
}

// ── reading the chain and IPFS ──────────────────────────────────────

fn rpc_result<T: RpcTransport>(t: &T, method: &str, params: Value) -> Result<Value, String> {
    let resp = t
        .call(json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params }))
        .map_err(|e| format!("{method}: {e}"))?;
    if let Some(e) = resp.get("error") {
        return Err(format!("{method}: {e}"));
    }
    resp.get("result")
        .cloned()
        .ok_or_else(|| format!("{method}: no result"))
}

/// `eth_getBlockByNumber("0x0").hash`, lowercase.
pub fn read_genesis_hash<T: RpcTransport>(t: &T) -> Result<String, String> {
    let b = rpc_result(t, "eth_getBlockByNumber", json!(["0x0", false]))?;
    let h = b
        .get("hash")
        .and_then(Value::as_str)
        .ok_or("block 0 has no hash")?;
    let raw = hex::decode(h.trim_start_matches("0x")).map_err(|_| "block 0 hash is not hex")?;
    if raw.len() != 32 {
        return Err("block 0 hash is not 32 bytes".into());
    }
    Ok(hex0x(&raw))
}

/// The latest block number and timestamp.
pub fn read_head<T: RpcTransport>(t: &T) -> Result<(u64, u64), String> {
    let b = rpc_result(t, "eth_getBlockByNumber", json!(["latest", false]))?;
    let n = b
        .get("number")
        .and_then(hex_u64)
        .ok_or("latest block has no number")?;
    let ts = b
        .get("timestamp")
        .and_then(hex_u64)
        .ok_or("latest block has no timestamp")?;
    Ok((n, ts))
}

fn get_offer<T: RpcTransport>(t: &T, market: &str, id: u64) -> Result<Vec<u8>, String> {
    let mut data = keccak(b"getOffer(uint256)")[..4].to_vec();
    let mut w = [0u8; 32];
    w[24..].copy_from_slice(&id.to_be_bytes());
    data.extend_from_slice(&w);
    let v = rpc_result(
        t,
        "eth_call",
        json!([{ "to": market, "data": hex0x(&data) }, "latest"]),
    )?;
    hex::decode(
        v.as_str()
            .ok_or("eth_call result is not a string")?
            .trim_start_matches("0x"),
    )
    .map_err(|_| "eth_call result is not hex".to_string())
}

/// What one sync did.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncReport {
    /// The last block read (inclusive), if any chunk was applied.
    pub read_to: Option<u64>,
    /// Blocks left between the cursor and `head - CONFIRMATIONS` after this sync.
    pub behind: u64,
    pub new_offers: Vec<u64>,
    /// Offers whose terms could not be read this time (retried next sync).
    pub terms_failed: Vec<u64>,
    /// Why the sync stopped early, if it did.
    pub stopped: Option<String>,
}

/// Read forward from the cursor to `head - CONFIRMATIONS` (at most [`MAX_CHUNKS_PER_SYNC`]
/// chunks), then refresh the terms of every live offer. A refused chunk stops the sync with the
/// model at its last good cursor.
pub fn sync<T: RpcTransport>(t: &T, book: &mut OfferBook, head: u64, now: u64) -> SyncReport {
    let mut report = SyncReport::default();
    let target = head.saturating_sub(CONFIRMATIONS);
    let (opened, closed) = (topic(OFFER_OPENED_SIG), topic(OFFER_CLOSED_SIG));
    let mut chunks = 0;
    while book.next_block <= target && chunks < MAX_CHUNKS_PER_SYNC {
        let from = book.next_block;
        let to = from.saturating_add(LOG_CHUNK_BLOCKS - 1).min(target);
        let filter = json!([{
            "address": book.market,
            "fromBlock": format!("0x{from:x}"),
            "toBlock": format!("0x{to:x}"),
            "topics": [[opened, closed]],
        }]);
        let logs = match rpc_result(t, "eth_getLogs", filter) {
            Ok(Value::Array(a)) => a,
            Ok(_) => {
                report.stopped = Some("eth_getLogs did not return a list".into());
                break;
            }
            Err(e) => {
                report.stopped = Some(e);
                break;
            }
        };
        let parsed: Option<Vec<RawLog>> = logs.iter().map(parse_log).collect();
        let Some(parsed) = parsed else {
            report.stopped = Some(format!("a market log in {from}..{to} is malformed"));
            break;
        };
        match book.apply_chunk(from, to, &parsed) {
            Ok(fresh) => {
                report.new_offers.extend(fresh);
                report.read_to = Some(to);
            }
            Err(e) => {
                report.stopped = Some(e.to_string());
                break;
            }
        }
        chunks += 1;
    }
    report.behind = if book.next_block <= target {
        target - book.next_block + 1
    } else {
        0
    };
    let market = book.market.clone();
    for (id, e) in book.offers.iter_mut() {
        if !OfferBook::is_live(e, now) {
            continue;
        }
        match get_offer(t, &market, *id)
            .and_then(|r| decode_offer_terms(&r, &e.opened).map_err(|x| x.to_string()))
        {
            Ok(terms) => e.terms = Some(terms),
            Err(_) => report.terms_failed.push(*id),
        }
    }
    report
}

/// Where requirements files come from.
pub trait RequirementsSource {
    /// At most `max + 1` bytes of the object at `cid` (so an oversize file is detected, not read).
    fn fetch(&self, cid: &str, max: usize) -> Result<Vec<u8>, String>;
}

/// The local IPFS node's `cat`, bounded by `length` and a body limit.
pub struct KuboRequirements {
    api: String,
}

impl KuboRequirements {
    pub fn from_env() -> Self {
        Self {
            api: std::env::var(crate::storage::KUBO_API_ENV)
                .unwrap_or_else(|_| crate::storage::DEFAULT_KUBO_API.to_string()),
        }
    }
}

impl RequirementsSource for KuboRequirements {
    fn fetch(&self, cid: &str, max: usize) -> Result<Vec<u8>, String> {
        if !cid_is_plausible(cid) {
            return Err("not a plausible CID".into());
        }
        let url = format!("{}/api/v0/cat?arg={cid}&length={}", self.api, max + 1);
        let mut resp = ureq::post(&url)
            .config()
            .timeout_global(Some(std::time::Duration::from_secs(10)))
            .max_redirects(0)
            .build()
            .send_empty()
            .map_err(|e| format!("ipfs cat: {e}"))?;
        resp.body_mut()
            .with_config()
            .limit((max + 1) as u64)
            .read_to_vec()
            .map_err(|e| format!("ipfs cat: {e}"))
    }
}

/// Fetch and verify the requirements of every live offer whose terms are read and whose
/// requirements are not settled yet. A transport failure stays `NotRead` (retried); a hash
/// mismatch or malformed file is `Refused` for good.
pub fn fetch_requirements<S: RequirementsSource>(src: &S, book: &mut OfferBook, now: u64) {
    for e in book.offers.values_mut() {
        if !OfferBook::is_live(e, now)
            || !matches!(e.requirements, RequirementsRead::NotRead { .. })
        {
            continue;
        }
        let Some(terms) = &e.terms else { continue };
        e.requirements = match src.fetch(&e.opened.requirements_cid, MAX_REQUIREMENTS_BYTES) {
            Err(reason) => RequirementsRead::NotRead { reason },
            Ok(bytes) => match verify_requirements(&bytes, &terms.requirements_hash) {
                Ok(requirements) => RequirementsRead::Verified { requirements },
                Err(e) => RequirementsRead::Refused {
                    reason: e.to_string(),
                },
            },
        };
    }
}

// ── the command ─────────────────────────────────────────────────────

/// One offer as the app shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OfferView {
    #[serde(flatten)]
    pub entry: OfferEntry,
    /// `None` when shown; the reason when hidden.
    pub hidden: Option<Hidden>,
}

/// The read model's answer to the app.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketView {
    /// `false` when this build or chain has no market; `reason` says why.
    pub available: bool,
    pub reason: Option<String>,
    pub market: Option<String>,
    pub sync: Option<SyncReport>,
    pub offers: Vec<OfferView>,
}

impl MarketView {
    fn unavailable(reason: impl Into<String>) -> Self {
        Self {
            available: false,
            reason: Some(reason.into()),
            market: None,
            sync: None,
            offers: Vec::new(),
        }
    }
}

/// Sync `book` against the chain and IPFS, then decide visibility. The book is reset when it
/// belongs to another market or generation.
pub fn refresh<T: RpcTransport, S: RequirementsSource>(
    t: &T,
    src: &S,
    market: &str,
    expected_genesis: &str,
    stored: Option<OfferBook>,
    ctx: &MemberContext,
) -> (MarketView, Option<OfferBook>) {
    let genesis = match read_genesis_hash(t) {
        Ok(g) => g,
        Err(e) => {
            return (
                MarketView::unavailable(format!("the chain could not be read: {e}")),
                stored,
            )
        }
    };
    if !genesis.eq_ignore_ascii_case(expected_genesis) {
        return (
            MarketView::unavailable(format!(
                "the chain's block 0 is {genesis}, this build's address book is for {expected_genesis}"
            )),
            stored,
        );
    }
    let (head, now) = match read_head(t) {
        Ok(h) => h,
        Err(e) => {
            return (
                MarketView::unavailable(format!("the chain head could not be read: {e}")),
                stored,
            )
        }
    };
    let mut book = stored
        .filter(|b| b.bound_to(market, &genesis))
        .unwrap_or_else(|| OfferBook::new(market, &genesis, 0));
    let report = sync(t, &mut book, head, now);
    fetch_requirements(src, &mut book, now);
    let offers = book
        .offers
        .values()
        .map(|e| OfferView {
            entry: e.clone(),
            hidden: visibility(e, ctx, now).err(),
        })
        .collect();
    (
        MarketView {
            available: true,
            reason: None,
            market: Some(book.market.clone()),
            sync: Some(report),
            offers,
        },
        Some(book),
    )
}

/// `<app data>/fl/market.json`.
fn book_path<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Result<PathBuf, String> {
    use tauri::Manager;
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("fl")
        .join("market.json"))
}

/// The member's groups and rosters, and the wallets linked to comms members by verified links.
fn member_context(app: &tauri::AppHandle) -> MemberContext {
    let mut ctx = MemberContext::default();
    if let Ok(groups) = crate::comms::list_groups(app) {
        for (group_id, _name) in groups {
            if let Ok(roster) = crate::comms::roster_of(app, &group_id) {
                ctx.groups.push(GroupMembership { group_id, roster });
            }
        }
    }
    let mut links = Vec::new();
    if let Ok(p) = crate::device_link::store_path(app) {
        if let Ok(s) = crate::device_link::DeviceLinkStore::load(&p) {
            links.extend(s.links);
        }
    }
    if let Ok(p) = crate::device_link_share::peer_store_path(app) {
        if let Ok(s) = crate::device_link_share::PeerLinkStore::load(&p) {
            links.extend(s.links);
        }
    }
    ctx.links = links
        .into_iter()
        .filter(|l| crate::device_link::verify_link(l).is_ok())
        .map(|l| WalletLink {
            wallet: l.wallet,
            member: l.member,
        })
        .collect();
    ctx
}

/// **fl_market_offers** — the WorkMarket offers this member can see, with the reason for each one
/// it cannot. Reads the chain (public 40204 RPC), the local IPFS node and the comms rosters; writes
/// only the read model's cache. Signs nothing.
#[tauri::command]
pub async fn fl_market_offers(app: tauri::AppHandle) -> Result<MarketView, String> {
    let Some(market) = crate::addresses::work_market() else {
        return Ok(MarketView::unavailable(
            "WorkMarket is not in this build's address book (it arrives with the 0.6.0 genesis)",
        ));
    };
    tauri::async_runtime::spawn_blocking(move || {
        let path = book_path(&app)?;
        let stored = OfferBook::load(&path).unwrap_or(None);
        let ctx = member_context(&app);
        let (view, book) = refresh(
            &crate::rpc::HttpTransport::citrate(),
            &KuboRequirements::from_env(),
            market,
            crate::addresses::genesis_hash(),
            stored,
            &ctx,
        );
        if let Some(b) = book {
            b.save(&path)?;
        }
        Ok(view)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
#[path = "fl_market_tests.rs"]
mod tests;
