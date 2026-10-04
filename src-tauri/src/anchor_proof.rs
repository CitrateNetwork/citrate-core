//! HUP-S7.3 (US-7.2 AC3): prove that a past decision is in an anchored day, checked by core.
//!
//! The Hermes sidecar keeps the decision records and builds the inclusion proof
//! (`GET /anchor/proof?seq=N`, crate `citrate-agent-anchor` in citrate-agent-runtime). Core does not
//! take its word for any of it. For one record, core checks three links on its own:
//!
//! 1. **The record is the leaf.** The sidecar sends the exact bytes the record hash covers
//!    (`recordCanonical`); core hashes them: `SHA-256("citrate.agent-records.v1\n" || bytes)` must
//!    equal the proof's `record_hash`.
//! 2. **The leaf is in the day's batch.** The RFC 6962 audit path (leaf `SHA-256(0x00 || h)`, node
//!    `SHA-256(0x01 || l || r)`, RFC 9162 section 2.1.3.2 verification) leads to the header's tree
//!    root, the record's `seq` sits at its leaf position, and the header is well formed. The day
//!    commitment is recomputed here from the header
//!    (`SHA-256("citrate.agent-anchor.nightly.v1\n" || be32 v || be64 day || be64 first_seq ||
//!    be64 last_seq || be64 count || tree_root)`).
//! 3. **The batch is on chain.** Core reads `AnchorRegistry` on 40204 itself (`eth_call`, never
//!    through the sidecar): the anchor for the commitment must exist, be of kind `NightlyMerkle`,
//!    and (for "proven by you") have been sent by this install's anchor key. On the next registry
//!    version, which keeps one record per committer, core asks for its own key's record first
//!    (`getAnchorBy`); on the deployed version it reads the first committer (`getAnchor`) and says
//!    so honestly when another account sent the same value first.
//!
//! These hashing rules are a second copy of the runtime's on purpose: the point of the check is to
//! not depend on the process that produced the proof. The copy is pinned by a fixture captured from
//! the real sidecar (`tests/fixtures/anchor/sidecar-proofs-5.json`) and by the anvil rehearsal
//! (`scripts/anvil-anchor-e2e.sh`).
//!
//! Data sources (Rule 7): the sidecar's `/anchor/proof` and `/anchor/records` (loopback, bearer);
//! the registry address from the generated 40204 address book (`chain_agent::anchor_registry`);
//! the chain through the live 40204 RPC. Nothing here signs or sends (Rule 3).

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sha3::Keccak256;

use crate::rpc::{RpcClient, RpcTransport};

/// Domain of a decision record's hash (citrate-agent-records).
pub const RECORD_DOMAIN: &[u8] = b"citrate.agent-records.v1\n";
/// Domain of the anchored day commitment (citrate-agent-anchor).
pub const COMMITMENT_DOMAIN: &[u8] = b"citrate.agent-anchor.nightly.v1\n";
/// The only batch header version this build understands.
pub const BATCH_VERSION: u32 = 1;
/// `AnchorRegistry.AnchorKind.NightlyMerkle`.
pub const NIGHTLY_MERKLE: u8 = 2;
/// The longest audit path accepted (a tree of 2^64 leaves).
pub const MAX_PATH: usize = 64;

// ---------------------------------------------------------------------------------------------
// the proof, as the sidecar sends it

/// The batch header (`citrate_agent_anchor::BatchHeader`). Hashes are 64 hex digits, no `0x`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BatchHeader {
    pub v: u32,
    pub day: u64,
    pub first_seq: u64,
    pub last_seq: u64,
    pub count: u64,
    pub tree_root: String,
}

/// One record's inclusion proof (`citrate_agent_anchor::AnchorProof`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnchorProof {
    pub header: BatchHeader,
    pub seq: u64,
    pub leaf_index: u64,
    pub record_hash: String,
    pub path: Vec<String>,
}

/// The `/anchor/proof` answer. Only the fields core uses are read; everything the sidecar claims
/// about verification is shown as its claim, never as the result.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SidecarProof {
    pub seq: u64,
    pub commitment: String,
    pub proof: AnchorProof,
    /// The bytes the record hash covers. Absent from an older sidecar.
    #[serde(default)]
    pub record_canonical: Option<String>,
    /// The sidecar's own confirmation record for the day, if it has one.
    #[serde(default)]
    pub anchored: Option<Value>,
}

fn sha256(parts: &[&[u8]]) -> [u8; 32] {
    let mut h = Sha256::new();
    for p in parts {
        h.update(p);
    }
    h.finalize().into()
}

fn keccak(data: &[u8]) -> [u8; 32] {
    let mut h = Keccak256::new();
    h.update(data);
    h.finalize().into()
}

/// 32 bytes from 64 hex digits, with or without `0x`.
pub fn hex32(s: &str) -> Option<[u8; 32]> {
    let h = s.strip_prefix("0x").unwrap_or(s);
    if h.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    hex::decode_to_slice(h, &mut out).ok()?;
    Some(out)
}

/// RFC 6962 leaf hash.
pub fn leaf_hash(data: &[u8; 32]) -> [u8; 32] {
    sha256(&[&[0u8], data])
}

/// RFC 6962 interior node hash.
pub fn node_hash(l: &[u8; 32], r: &[u8; 32]) -> [u8; 32] {
    sha256(&[&[1u8], l, r])
}

/// RFC 9162 section 2.1.3.2: does `path` lead from `leaf` at `index` in a tree of `size` leaves to
/// `root`?
pub fn verify_path(
    leaf: &[u8; 32],
    index: u64,
    size: u64,
    path: &[[u8; 32]],
    root: &[u8; 32],
) -> bool {
    if index >= size || path.len() > MAX_PATH {
        return false;
    }
    let (mut fnode, mut snode) = (index, size - 1);
    let mut r = leaf_hash(leaf);
    for p in path {
        if snode == 0 {
            return false;
        }
        // The right edge of an unbalanced level (`fnode == snode`) hashes on the left too
        // (RFC 9162 section 2.1.3.2 step 5.2); without it the last leaf of a tree whose size is
        // not a power of two never verifies.
        if fnode & 1 == 1 || fnode == snode {
            r = node_hash(p, &r);
            while fnode & 1 == 0 && fnode != 0 {
                fnode >>= 1;
                snode >>= 1;
            }
        } else {
            r = node_hash(&r, p);
        }
        fnode >>= 1;
        snode >>= 1;
    }
    snode == 0 && &r == root
}

/// The anchored value for a batch header.
pub fn commitment(h: &BatchHeader, tree_root: &[u8; 32]) -> [u8; 32] {
    sha256(&[
        COMMITMENT_DOMAIN,
        &h.v.to_be_bytes(),
        &h.day.to_be_bytes(),
        &h.first_seq.to_be_bytes(),
        &h.last_seq.to_be_bytes(),
        &h.count.to_be_bytes(),
        tree_root,
    ])
}

/// The record hash of the exact bytes a record hash covers.
pub fn record_hash_of(canonical: &str) -> [u8; 32] {
    sha256(&[RECORD_DOMAIN, canonical.as_bytes()])
}

/// Link 2: check the proof on its own and return the day commitment it proves into. `Err` names
/// the first thing that does not hold.
pub fn check_inclusion(p: &AnchorProof) -> Result<[u8; 32], String> {
    let h = &p.header;
    if h.v != BATCH_VERSION {
        return Err(format!("unknown batch version {}", h.v));
    }
    let well_formed =
        h.count > 0 && h.last_seq >= h.first_seq && h.last_seq - h.first_seq == h.count - 1;
    if !well_formed {
        return Err("the batch header is not a contiguous run of records".into());
    }
    if h.first_seq.checked_add(p.leaf_index) != Some(p.seq) {
        return Err("the record number does not sit at its leaf position".into());
    }
    let root = hex32(&h.tree_root).ok_or("the tree root is not 32 bytes of hex")?;
    let leaf = hex32(&p.record_hash).ok_or("the record hash is not 32 bytes of hex")?;
    let path = p
        .path
        .iter()
        .map(|s| hex32(s))
        .collect::<Option<Vec<_>>>()
        .ok_or("a path hash is not 32 bytes of hex")?;
    if !verify_path(&leaf, p.leaf_index, h.count, &path, &root) {
        return Err("the audit path does not lead to the batch's tree root".into());
    }
    Ok(commitment(h, &root))
}

/// Milliseconds in a UTC day (the batch day of a record is `ts_ms / DAY_MS`).
const DAY_MS: u64 = 86_400_000;

/// Link 1: the canonical bytes hash to the proof's leaf, and the record those bytes describe is
/// the proven one: its own `seq` is the proof's and its own timestamp falls on the batch's UTC
/// day (the same checks as the runtime's `verify_record_proof`). `None` when the sidecar sent no
/// bytes.
pub fn record_bound(p: &SidecarProof) -> Option<bool> {
    let canonical = p.record_canonical.as_deref()?;
    if hex32(&p.proof.record_hash) != Some(record_hash_of(canonical)) {
        return Some(false);
    }
    let body = serde_json::from_str::<Value>(canonical).ok();
    let seq = body
        .as_ref()
        .and_then(|b| b.get("seq"))
        .and_then(Value::as_u64);
    let ts = body
        .as_ref()
        .and_then(|b| b.get("ts_ms"))
        .and_then(Value::as_u64);
    Some(seq == Some(p.proof.seq) && ts.map(|t| t / DAY_MS) == Some(p.proof.header.day))
}

// ---------------------------------------------------------------------------------------------
// the registry, read by core

fn selector(sig: &str) -> [u8; 4] {
    let h = keccak(sig.as_bytes());
    [h[0], h[1], h[2], h[3]]
}

/// `getAnchor(bytes32)`: the first committer's record (both registry versions).
pub fn get_anchor_calldata(root: &[u8; 32]) -> Vec<u8> {
    let mut out = selector("getAnchor(bytes32)").to_vec();
    out.extend_from_slice(root);
    out
}

fn addr_word(addr: &[u8; 20]) -> [u8; 32] {
    let mut w = [0u8; 32];
    w[12..].copy_from_slice(addr);
    w
}

/// `getAnchorBy(address,bytes32)`: one committer's record (next registry version only).
pub fn get_anchor_by_calldata(committer: &[u8; 20], root: &[u8; 32]) -> Vec<u8> {
    let mut out = selector("getAnchorBy(address,bytes32)").to_vec();
    out.extend_from_slice(&addr_word(committer));
    out.extend_from_slice(root);
    out
}

/// The `AnchorNotFound()` revert selector.
pub fn anchor_not_found_selector() -> [u8; 4] {
    selector("AnchorNotFound()")
}

/// An `Anchor` struct as the registry returns it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OnChainAnchor {
    pub kind: u8,
    pub root: String,
    pub committer: String,
    pub block_number: u64,
    pub timestamp: u64,
}

fn word_u64(w: &[u8]) -> Result<u64, String> {
    if w.len() != 32 || w[..24].iter().any(|b| *b != 0) {
        return Err("a number in the registry answer does not fit".into());
    }
    let mut b = [0u8; 8];
    b.copy_from_slice(&w[24..]);
    Ok(u64::from_be_bytes(b))
}

/// Decode `(uint8 kind, bytes32 root, address committer, uint256 block_number, uint256 timestamp)`.
pub fn decode_anchor(ret: &[u8]) -> Result<OnChainAnchor, String> {
    if ret.len() != 5 * 32 {
        return Err(format!(
            "the registry answer is {} bytes, not an Anchor",
            ret.len()
        ));
    }
    let w = |i: usize| &ret[i * 32..(i + 1) * 32];
    let kind = word_u64(w(0))?;
    let kind = u8::try_from(kind).map_err(|_| "the anchor kind does not fit".to_string())?;
    if w(2)[..12].iter().any(|b| *b != 0) {
        return Err("the committer is not an address".into());
    }
    Ok(OnChainAnchor {
        kind,
        root: format!("0x{}", hex::encode(w(1))),
        committer: format!("0x{}", hex::encode(&w(2)[12..])),
        block_number: word_u64(w(3))?,
        timestamp: word_u64(w(4))?,
    })
}

/// What a raw `eth_call` came back with.
enum CallResult {
    Data(Vec<u8>),
    /// Reverted, with the revert data when the node gives it.
    Reverted(Vec<u8>),
}

fn raw<T: RpcTransport>(rpc: &RpcClient<T>, method: &str, params: Value) -> Result<Value, String> {
    let body = rpc.build_request(method, params);
    rpc.transport().call(body).map_err(|e| e.to_string())
}

fn hex_bytes(v: &Value) -> Option<Vec<u8>> {
    let s = v.as_str()?;
    hex::decode(s.strip_prefix("0x").unwrap_or(s)).ok()
}

/// The revert data a node printed into a revert message: the last `0x` run of hex digits that is
/// at least a selector long and a whole number of bytes, after the word "reverted". `None` when
/// there is none (for example "Unknown reason").
pub fn revert_data_in_message(msg: &str) -> Option<Vec<u8>> {
    let at = msg.find("reverted")?;
    let tail = &msg[at..];
    let mut found = None;
    let mut rest = tail;
    while let Some(i) = rest.find("0x") {
        let digits: String = rest[i + 2..]
            .chars()
            .take_while(|c| c.is_ascii_hexdigit())
            .collect();
        if digits.len() >= 8 && digits.len().is_multiple_of(2) {
            found = hex::decode(&digits).ok();
        }
        rest = &rest[i + 2 + digits.len()..];
    }
    found
}

fn call<T: RpcTransport>(rpc: &RpcClient<T>, to: &str, data: &[u8]) -> Result<CallResult, String> {
    let r = raw(
        rpc,
        "eth_call",
        json!([{"to": to, "data": format!("0x{}", hex::encode(data))}, "latest"]),
    )?;
    if let Some(e) = r.get("error") {
        // Nodes report a revert as an error with the revert data in `data` (anvil, geth) or
        // inside the message (the Citrate node on 40204: "execution reverted: ... Contract call
        // reverted: 0x46716752 (gas used: ...)"); anything else is a transport-level failure.
        let msg = e.get("message").and_then(Value::as_str).unwrap_or("");
        let data = e
            .get("data")
            .and_then(hex_bytes)
            .or_else(|| revert_data_in_message(msg));
        return match data {
            Some(d) => Ok(CallResult::Reverted(d)),
            None if msg.contains("revert") => Ok(CallResult::Reverted(Vec::new())),
            None => Err(format!("eth_call failed: {e}")),
        };
    }
    r.get("result")
        .and_then(hex_bytes)
        .map(CallResult::Data)
        .ok_or_else(|| "eth_call: no result".to_string())
}

fn has_code<T: RpcTransport>(rpc: &RpcClient<T>, addr: &str) -> Result<bool, String> {
    let r = raw(rpc, "eth_getCode", json!([addr, "latest"]))?;
    if let Some(e) = r.get("error") {
        return Err(format!("eth_getCode failed: {e}"));
    }
    let code = r
        .get("result")
        .and_then(Value::as_str)
        .ok_or("eth_getCode: no result")?;
    Ok(!matches!(code, "0x" | "0x0" | ""))
}

fn parse_addr(s: &str) -> Option<[u8; 20]> {
    let h = s.strip_prefix("0x")?;
    if h.len() != 40 {
        return None;
    }
    let mut out = [0u8; 20];
    hex::decode_to_slice(h, &mut out).ok()?;
    Some(out)
}

/// What the registry says about one commitment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ChainCheck {
    /// No `AnchorRegistry` in the 40204 address book.
    NotInBook,
    /// The book names an address with no contract code.
    NoCode { registry: String },
    /// The registry has no anchor for this commitment.
    NotAnchored { registry: String },
    /// Anchored. `by_you` is true only when this install's anchor key sent it.
    Anchored {
        registry: String,
        anchor: OnChainAnchor,
        by_you: bool,
    },
    /// The registry answered with something that is not this commitment's nightly anchor.
    Mismatch { registry: String, detail: String },
    /// The chain could not be read.
    Unreachable { registry: String, detail: String },
}

fn interpret(
    registry: &str,
    root: &[u8; 32],
    me: Option<&[u8; 20]>,
    ret: CallResult,
) -> Option<ChainCheck> {
    match ret {
        CallResult::Reverted(d) if d.starts_with(&anchor_not_found_selector()) => None,
        CallResult::Reverted(d) => Some(ChainCheck::Mismatch {
            registry: registry.to_string(),
            detail: format!(
                "the registry refused the read (revert data 0x{})",
                hex::encode(d)
            ),
        }),
        CallResult::Data(d) => Some(match decode_anchor(&d) {
            Err(e) => ChainCheck::Mismatch {
                registry: registry.to_string(),
                detail: e,
            },
            Ok(a) if a.kind != NIGHTLY_MERKLE => ChainCheck::Mismatch {
                registry: registry.to_string(),
                detail: format!(
                    "the value is anchored as kind {}, not a nightly batch",
                    a.kind
                ),
            },
            Ok(a) if hex32(&a.root) != Some(*root) => ChainCheck::Mismatch {
                registry: registry.to_string(),
                detail: "the registry returned a different value".into(),
            },
            Ok(a) => {
                let by_you = me.is_some_and(|m| parse_addr(&a.committer) == Some(*m));
                ChainCheck::Anchored {
                    registry: registry.to_string(),
                    anchor: a,
                    by_you,
                }
            }
        }),
    }
}

/// Link 3: read the registry for `root`. `me` is this install's anchor key address, when one
/// exists. Read-only `eth_call`s; nothing is signed or sent.
pub fn check_chain<T: RpcTransport>(
    rpc: &RpcClient<T>,
    registry: Option<&str>,
    root: &[u8; 32],
    me: Option<&str>,
) -> ChainCheck {
    let Some(registry) = registry else {
        return ChainCheck::NotInBook;
    };
    let unreachable = |detail: String| ChainCheck::Unreachable {
        registry: registry.to_string(),
        detail,
    };
    match has_code(rpc, registry) {
        Ok(true) => {}
        Ok(false) => {
            return ChainCheck::NoCode {
                registry: registry.to_string(),
            }
        }
        Err(e) => return unreachable(e),
    }
    let me20 = me.and_then(|m| parse_addr(&m.to_ascii_lowercase()));
    // The next registry version keeps one record per committer: ask for ours first. The deployed
    // version has no such function and reverts with no data; fall through to the first committer.
    if let Some(m) = &me20 {
        match call(rpc, registry, &get_anchor_by_calldata(m, root)) {
            Ok(r @ CallResult::Data(_)) => {
                if let Some(c) = interpret(registry, root, Some(m), r) {
                    return c;
                }
            }
            Ok(CallResult::Reverted(_)) => {}
            Err(e) => return unreachable(e),
        }
    }
    match call(rpc, registry, &get_anchor_calldata(root)) {
        Ok(r) => interpret(registry, root, me20.as_ref(), r).unwrap_or(ChainCheck::NotAnchored {
            registry: registry.to_string(),
        }),
        Err(e) => unreachable(e),
    }
}

// ---------------------------------------------------------------------------------------------
// the verdict

/// What the member sees for one record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProofVerdict {
    pub seq: u64,
    pub day: u64,
    pub date: Option<String>,
    /// The commitment recomputed by core (`None` when the proof does not hold).
    pub commitment: Option<String>,
    /// Link 2.
    pub inclusion_ok: bool,
    pub inclusion_error: Option<String>,
    /// Link 1. `None` when the sidecar sent no record bytes.
    pub record_bound: Option<bool>,
    /// Link 3.
    pub chain: ChainCheck,
    /// All three links hold and this install's anchor key sent the anchor.
    pub proven: bool,
    pub line: String,
    /// The record as the sidecar holds it (parsed from the hashed bytes), for display.
    pub record: Option<Value>,
}

/// Put the three links together. `chain` is called only when the proof holds, with the
/// commitment core computed (never the sidecar's).
pub fn verdict(p: &SidecarProof, chain: impl FnOnce(&[u8; 32]) -> ChainCheck) -> ProofVerdict {
    let day = p.proof.header.day;
    let date = crate::ceremony::anchor::date_of_day(day);
    let bound = record_bound(p);
    let record = p
        .record_canonical
        .as_deref()
        .and_then(|c| serde_json::from_str::<Value>(c).ok());
    let base = |chain: ChainCheck, line: String| ProofVerdict {
        seq: p.seq,
        day,
        date: date.clone(),
        commitment: None,
        inclusion_ok: false,
        inclusion_error: None,
        record_bound: bound,
        chain,
        proven: false,
        line,
        record: record.clone(),
    };
    if p.proof.seq != p.seq {
        return base(
            ChainCheck::NotInBook,
            "The proof Hermes returned is for another record; nothing is proven.".into(),
        );
    }
    let c = match check_inclusion(&p.proof) {
        Ok(c) => c,
        Err(e) => {
            let mut v = base(
                ChainCheck::NotInBook,
                format!("The proof does not hold: {e}. Nothing is proven."),
            );
            v.inclusion_error = Some(e);
            return v;
        }
    };
    if hex32(&p.commitment) != Some(c) {
        let mut v = base(
            ChainCheck::NotInBook,
            "Hermes named a different day value than its own proof leads to; nothing is proven."
                .into(),
        );
        v.inclusion_error = Some("the stated commitment does not match the header".into());
        return v;
    }
    let chain = chain(&c);
    let record_ok = bound == Some(true);
    let when = date.clone().unwrap_or_else(|| format!("day {day}"));
    let line = match (&chain, record_ok) {
        (_, false) if bound.is_none() => {
            "This version of Hermes did not send the record's bytes, so the record cannot be tied to its proof yet.".to_string()
        }
        (_, false) => {
            "The record Hermes holds does not match the proven leaf (its bytes, number or day differ): it was changed after it was written, or it is not this record.".to_string()
        }
        (ChainCheck::Anchored { anchor, by_you: true, .. }, true) => format!(
            "Proven. Record #{} is in the decisions of {when} (UTC), anchored on 40204 in block {} by your anchor key.",
            p.seq, anchor.block_number
        ),
        (ChainCheck::Anchored { anchor, by_you: false, .. }, true) => format!(
            "The day's value is on 40204 (block {}), but it was sent by {}, not by your anchor key. The record is in the batch; the anchor is not yours.",
            anchor.block_number, anchor.committer
        ),
        (ChainCheck::NotAnchored { .. }, true) => format!(
            "The proof holds, but the decisions of {when} are not anchored on 40204 yet."
        ),
        (ChainCheck::NotInBook, true) => {
            "The proof holds on this device. AnchorRegistry is not in the 40204 address book yet, so it cannot be checked on chain.".to_string()
        }
        (ChainCheck::NoCode { registry }, true) => format!(
            "The proof holds on this device, but there is no contract at the AnchorRegistry address {registry}."
        ),
        (ChainCheck::Mismatch { detail, .. }, true) => format!(
            "The proof holds on this device, but the registry's answer does not match: {detail}."
        ),
        (ChainCheck::Unreachable { detail, .. }, true) => format!(
            "The proof holds on this device. The chain could not be read just now ({detail})."
        ),
    };
    let proven = record_ok && matches!(chain, ChainCheck::Anchored { by_you: true, .. });
    ProofVerdict {
        seq: p.seq,
        day,
        date,
        commitment: Some(format!("0x{}", hex::encode(c))),
        inclusion_ok: true,
        inclusion_error: None,
        record_bound: bound,
        chain,
        proven,
        line,
        record,
    }
}

// ---------------------------------------------------------------------------------------------
// commands

/// The largest page the record list asks for.
pub const MAX_PAGE: u32 = 100;

fn running_manager(
    app: &tauri::AppHandle,
) -> Result<&'static crate::hermes::HermesManager, String> {
    let m = crate::hermes::chain::manager_for(app)?;
    if !m.is_running() {
        return Err("Hermes is not running, so the decision records cannot be read.".into());
    }
    Ok(m)
}

/// **hermes_anchor_records**: the member's retained decision records, newest first, each with
/// its day's anchor state (the sidecar's `/anchor/records`). Read-only.
#[tauri::command]
pub async fn hermes_anchor_records(
    app: tauri::AppHandle,
    before: Option<u64>,
    limit: Option<u32>,
) -> Result<Value, String> {
    crate::blocking::off_main(move || {
        let m = running_manager(&app)?;
        m.anchor_records(before, limit.map(|l| l.clamp(1, MAX_PAGE)))
            .map_err(|e| e.to_string())
    })
    .await
}

/// **hermes_anchor_proof**: prove one past decision: fetch its proof from Hermes, check the
/// record bytes, the audit path and the day value in core, then read `AnchorRegistry` on 40204.
/// Read-only; nothing is signed or sent.
#[tauri::command]
pub async fn hermes_anchor_proof(app: tauri::AppHandle, seq: u64) -> Result<ProofVerdict, String> {
    crate::blocking::off_main(move || {
        let m = running_manager(&app)?;
        let p = m.anchor_proof(seq).map_err(|e| e.to_string())?;
        let registry = crate::chain_agent::anchor_registry();
        // The anchor key's address only (no key material leaves the kit); absent before the
        // member ever turned anchoring on.
        let me = crate::ceremony::anchor::anchor_address(
            &citrate_core_kit::custody::OsKeyring::with_service(crate::CUSTODY_KEYRING_SERVICE),
        )
        .ok()
        .flatten();
        let rpc = RpcClient::citrate();
        Ok(verdict(&p, |c| {
            check_chain(&rpc, registry.as_deref(), c, me.as_deref())
        }))
    })
    .await
}

#[cfg(test)]
#[path = "anchor_proof_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "anchor_e2e_tests.rs"]
mod e2e_tests;
