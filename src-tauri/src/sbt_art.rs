//! citrate-core — BC-5.3: the REAL on-chain SBT emblem read.
//! T1 identity surface (a PURE READ — no key material, no signing).
//!
//! The post-reroll `CitrateMemberSBT` generates the member emblem WHOLLY
//! ON-CHAIN: `tokenURI(uint256) -> string` returns a
//! `data:application/json;base64,<json>` whose `image` field is itself a
//! `data:image/svg+xml;base64,<svg>` rendered by `MemberEmblem.render(owner)`.
//! This module reads that AUTHORITATIVE art from the live 40204 RPC via
//! `eth_call` — the local deterministic emblem (`src/identity/sbtArt.ts`) is only
//! an honest, labelled OFFLINE FALLBACK when this read is unavailable (Rule 1: the
//! caption always names which source is shown; we never fabricate an emblem).
//!
//! ## The sub -> tokenId -> tokenURI path (grounded against core-membership)
//! The SBT is keyed by the member's `subHash = keccak256(sub_utf8_bytes)` — the
//! raw OIDC sub NEVER goes on chain (matches core-membership's `subHashOf`, which
//! is `keccak256(toHex(sub))` == keccak of the sub's UTF-8 bytes). We:
//!   1. `isSubBound(subHash) -> bool` — has a token EVER been minted for this sub?
//!      A member with no SBT reads `false` and we return `None` (honest — no
//!      fabricated art). This gate is done FIRST so we never depend on a revert.
//!   2. `tokenIdForSub(subHash) -> uint256` — the member's tokenId (it reverts for
//!      an unbound sub, hence the isSubBound gate above).
//!   3. `tokenURI(tokenId) -> string` — the authoritative `data:` URI.
//!
//! ## Why a RUNTIME keccak here (deviation from the pinned-selector house style)
//! Every OTHER contract read in this lean tree PINS its 4-byte selector with a
//! keccak drift test so no runtime keccak is needed. That works because a selector
//! is a CONSTANT. The `subHash` is NOT a constant — it is `keccak256(sub)` for a
//! per-member sub, so it MUST be computed at runtime. We reuse `sha3::Keccak256`,
//! which is ALREADY compiled into the tree as a normal dependency of
//! citrate-wallet-core (its Keccak-256 address derivation) — no new heavy crate.
//! The three selectors above are still PINNED + drift-tested like the rest.
//!
//! ## Secret discipline (@rule8 / Rule 3)
//! A PURE READ. Never sees key material, never signs; the `#[tauri::command]`
//! returns only the decoded PUBLIC `data:` URI string (or `None`). Coarse,
//! secret-free errors. Mirrors `staking.rs`/`grant_status.rs` house style
//! (pinned selectors + keccak drift tests, injectable RpcClient, decode helpers).

// Some helper surface (calldata builders, selector accessors) is reached only by
// tests until the full frontend round-trip lands (mirrors staking.rs).
#![allow(dead_code)]

use base64::Engine;
use sha3::{Digest, Keccak256};

use crate::grant_status::citrate_member_sbt;

/// 4-byte selector for `isSubBound(bytes32)` — `keccak256("isSubBound(bytes32)")[..4]`.
/// PINNED here + proven by the `is_sub_bound_selector_is_keccak_of_signature` drift
/// test (Rule 11). True once a token has ever been minted for the subHash; the
/// honest existence gate (a member with no SBT reads `false` -> we return `None`).
const IS_SUB_BOUND_SELECTOR: [u8; 4] = [0xde, 0xe2, 0xef, 0x9d];

/// 4-byte selector for `tokenIdForSub(bytes32)` —
/// `keccak256("tokenIdForSub(bytes32)")[..4]`. PINNED + drift-tested. Maps the
/// member subHash to their SBT tokenId (reverts for an unbound sub — hence the
/// `isSubBound` gate).
const TOKEN_ID_FOR_SUB_SELECTOR: [u8; 4] = [0xf8, 0x85, 0x6b, 0xd9];

/// 4-byte selector for `tokenURI(uint256)` — `keccak256("tokenURI(uint256)")[..4]`.
/// The canonical ERC-721 `tokenURI` (0xc87b56dd); PINNED + drift-tested. Returns the
/// authoritative on-chain `data:application/json;base64,...` emblem URI.
const TOKEN_URI_SELECTOR: [u8; 4] = [0xc8, 0x7b, 0x56, 0xdd];

/// Selector for `isSubBound(bytes32)`.
pub fn is_sub_bound_selector() -> [u8; 4] {
    IS_SUB_BOUND_SELECTOR
}

/// Selector for `tokenIdForSub(bytes32)`.
pub fn token_id_for_sub_selector() -> [u8; 4] {
    TOKEN_ID_FOR_SUB_SELECTOR
}

/// Selector for `tokenURI(uint256)`.
pub fn token_uri_selector() -> [u8; 4] {
    TOKEN_URI_SELECTOR
}

/// Errors from the SBT-art reader. Coarse + secret-free (this module never sees key
/// material — it does public `eth_call` reads).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SbtArtError {
    /// The live RPC read failed (transport / node error / missing field).
    Rpc(String),
    /// The `eth_call` return did not decode (short word, bad ABI string, non-utf8).
    Decode(String),
}

impl std::fmt::Display for SbtArtError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SbtArtError::Rpc(m) => write!(f, "sbt-art: rpc error: {m}"),
            SbtArtError::Decode(m) => write!(f, "sbt-art: decode error: {m}"),
        }
    }
}

impl std::error::Error for SbtArtError {}

/// The member subHash: `keccak256(sub_utf8_bytes)` as a 32-byte word. Matches
/// core-membership's `subHashOf` (`keccak256(toHex(sub))`, which hashes the sub's
/// UTF-8 bytes). The raw sub NEVER goes on chain — only this hash.
pub fn sub_hash(sub: &str) -> [u8; 32] {
    let h = Keccak256::digest(sub.as_bytes());
    let mut out = [0u8; 32];
    out.copy_from_slice(&h);
    out
}

/// Build `selector ++ 32-byte word` calldata (36 bytes) for a single-bytes32/uint256
/// argument. The word is used verbatim (already 32 bytes, right-aligned for a uint,
/// full-width for a bytes32).
fn encode_word_calldata(selector: [u8; 4], word: &[u8; 32]) -> Vec<u8> {
    let mut calldata = Vec::with_capacity(36);
    calldata.extend_from_slice(&selector);
    calldata.extend_from_slice(word);
    calldata
}

/// The `eth_call` object for `isSubBound(subHash)` on the SBT: `{to, data}`.
fn is_sub_bound_call(sub_hash: &[u8; 32]) -> serde_json::Value {
    let calldata = encode_word_calldata(is_sub_bound_selector(), sub_hash);
    serde_json::json!({
        "to": citrate_member_sbt(),
        "data": format!("0x{}", hex::encode(calldata)),
    })
}

/// The `eth_call` object for `tokenIdForSub(subHash)` on the SBT: `{to, data}`.
fn token_id_for_sub_call(sub_hash: &[u8; 32]) -> serde_json::Value {
    let calldata = encode_word_calldata(token_id_for_sub_selector(), sub_hash);
    serde_json::json!({
        "to": citrate_member_sbt(),
        "data": format!("0x{}", hex::encode(calldata)),
    })
}

/// The `eth_call` object for `tokenURI(tokenId)` on the SBT: `{to, data}`.
fn token_uri_call(token_id_word: &[u8; 32]) -> serde_json::Value {
    let calldata = encode_word_calldata(token_uri_selector(), token_id_word);
    serde_json::json!({
        "to": citrate_member_sbt(),
        "data": format!("0x{}", hex::encode(calldata)),
    })
}

/// Decode an ABI `bool` return (a 32-byte word; non-zero low byte == true). A short
/// return errors rather than silently reading `false`.
fn decode_bool_word(ret: &[u8]) -> Result<bool, SbtArtError> {
    if ret.len() < 32 {
        return Err(SbtArtError::Decode(format!(
            "expected a 32-byte bool word, got {} bytes",
            ret.len()
        )));
    }
    Ok(ret[0..32].iter().any(|&b| b != 0))
}

/// Decode an ABI dynamic `string` return: `[offset(32)][length(32)][utf8 bytes ...]`
/// (offset is 0x20 for a single return value). Returns the decoded UTF-8 string.
/// Refuses a malformed/short/over-long/non-utf8 encoding (Rule 1 — never a
/// fabricated or truncated URI).
pub fn decode_abi_string(ret: &[u8]) -> Result<String, SbtArtError> {
    if ret.len() < 64 {
        return Err(SbtArtError::Decode(format!(
            "abi string: need at least 64 bytes (offset+length), got {}",
            ret.len()
        )));
    }
    // Word 0: byte offset to the (length,data) block. For a single string return it
    // is 0x20; read it rather than assume, but bound it into the buffer.
    let offset = decode_u64_word(&ret[0..32], "abi string offset")? as usize;
    let len_start = offset
        .checked_add(32)
        .ok_or_else(|| SbtArtError::Decode("abi string: offset overflow".into()))?;
    if len_start > ret.len() {
        return Err(SbtArtError::Decode(
            "abi string: offset points past the return buffer".into(),
        ));
    }
    let len = decode_u64_word(&ret[offset..len_start], "abi string length")? as usize;
    let data_end = len_start
        .checked_add(len)
        .ok_or_else(|| SbtArtError::Decode("abi string: length overflow".into()))?;
    if data_end > ret.len() {
        return Err(SbtArtError::Decode(format!(
            "abi string: declared length {len} exceeds the return buffer"
        )));
    }
    String::from_utf8(ret[len_start..data_end].to_vec())
        .map_err(|_| SbtArtError::Decode("abi string: bytes are not valid utf-8".into()))
}

/// Decode a 32-byte word into a `u64`, refusing any value beyond `u64` (an offset or
/// length far larger than any real return — a malformed encoding) rather than
/// truncate. Used for the ABI-string offset/length words.
fn decode_u64_word(word: &[u8], what: &str) -> Result<u64, SbtArtError> {
    if word.len() < 32 {
        return Err(SbtArtError::Decode(format!(
            "{what}: expected a 32-byte word, got {}",
            word.len()
        )));
    }
    if word[0..24].iter().any(|&b| b != 0) {
        return Err(SbtArtError::Decode(format!("{what}: exceeds u64")));
    }
    let mut low = [0u8; 8];
    low.copy_from_slice(&word[24..32]);
    Ok(u64::from_be_bytes(low))
}

/// A decoded on-chain emblem: the raw `data:image/svg+xml;base64,...` image URI the
/// `tokenURI` JSON's `image` field carries. This is the value the frontend renders.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SbtEmblem {
    /// The `data:` URI of the tokenURI JSON itself (`data:application/json;base64,`).
    pub token_uri: String,
    /// The `image` field extracted from the decoded JSON — a
    /// `data:image/svg+xml;base64,...` the frontend renders directly.
    pub image_data_uri: String,
}

/// Extract the `image` data-URI from a `tokenURI` `data:application/json;base64,...`
/// value: base64-decode the JSON, parse it, read the `image` string field. Rule 1:
/// every step is real decoding of the on-chain value — a malformed URI / JSON / or a
/// missing `image` field errors rather than fabricating art.
pub fn extract_image_data_uri(token_uri: &str) -> Result<String, SbtArtError> {
    const PREFIX: &str = "data:application/json;base64,";
    let b64 = token_uri
        .strip_prefix(PREFIX)
        .ok_or_else(|| SbtArtError::Decode(format!("tokenURI is not a {PREFIX} data-uri")))?;
    let json_bytes = base64::engine::general_purpose::STANDARD
        .decode(b64.as_bytes())
        .map_err(|_| SbtArtError::Decode("tokenURI json: bad base64".into()))?;
    let json: serde_json::Value = serde_json::from_slice(&json_bytes)
        .map_err(|_| SbtArtError::Decode("tokenURI json: not valid json".into()))?;
    let image = json
        .get("image")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| SbtArtError::Decode("tokenURI json: no string `image` field".into()))?;
    if !image.starts_with("data:image/") {
        return Err(SbtArtError::Decode(
            "tokenURI `image` is not a data:image/... uri".into(),
        ));
    }
    Ok(image.to_string())
}

/// **The real on-chain emblem read.** For a member's OIDC `sub`, on the live 40204
/// RPC via `eth_call`:
///   1. `isSubBound(keccak256(sub))` — `false` -> the member has NO SBT -> `Ok(None)`
///      (honest; no fabricated art).
///   2. `tokenIdForSub(keccak256(sub))` -> the tokenId.
///   3. `tokenURI(tokenId)` -> the `data:application/json;base64,...` URI; decode its
///      `image` field to the `data:image/svg+xml;base64,...` the UI renders.
///
/// Rule 1: the art comes from chain, not a sim. `rpc` is injected so tests script a
/// mock transport; production wires [`crate::rpc::RpcClient::citrate`].
pub fn read_sbt_emblem<T: crate::rpc::RpcTransport>(
    rpc: &crate::rpc::RpcClient<T>,
    sub: &str,
) -> Result<Option<SbtEmblem>, SbtArtError> {
    let sh = sub_hash(sub);

    // 1. Existence gate — a member with no SBT honestly reads false -> None.
    let bound_ret = rpc
        .eth_call(is_sub_bound_call(&sh))
        .map_err(|e| SbtArtError::Rpc(e.to_string()))?;
    if !decode_bool_word(&bound_ret)? {
        return Ok(None);
    }

    // 2. tokenIdForSub -> the 32-byte tokenId word (used verbatim as the tokenURI arg).
    let token_id_ret = rpc
        .eth_call(token_id_for_sub_call(&sh))
        .map_err(|e| SbtArtError::Rpc(e.to_string()))?;
    if token_id_ret.len() < 32 {
        return Err(SbtArtError::Decode(format!(
            "tokenIdForSub: expected a 32-byte uint256 word, got {} bytes",
            token_id_ret.len()
        )));
    }
    let mut token_id_word = [0u8; 32];
    token_id_word.copy_from_slice(&token_id_ret[0..32]);

    // 3. tokenURI(tokenId) -> the data:application/json;base64 URI.
    let uri_ret = rpc
        .eth_call(token_uri_call(&token_id_word))
        .map_err(|e| SbtArtError::Rpc(e.to_string()))?;
    let token_uri = decode_abi_string(&uri_ret)?;
    let image_data_uri = extract_image_data_uri(&token_uri)?;

    Ok(Some(SbtEmblem {
        token_uri,
        image_data_uri,
    }))
}

/// `sbt_token_uri` command — read the member's REAL on-chain SBT emblem `image`
/// data-URI for their OIDC `sub`, from the live 40204 RPC. Returns the
/// `data:image/svg+xml;base64,...` string the UI renders, or `None` honestly when
/// the member has NO SBT (Rule 1 — never fabricated art). A PURE READ: @rule8 /
/// Rule 3 — no signing, no custody, no key material. The `sub` is the OIDC subject
/// string; the raw sub never goes on chain (only `keccak256(sub)`).
#[tauri::command]
pub fn sbt_token_uri(sub: String) -> std::result::Result<Option<String>, String> {
    let rpc = crate::rpc::RpcClient::citrate();
    read_sbt_emblem(&rpc, &sub)
        .map(|opt| opt.map(|e| e.image_data_uri))
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    include!("sbt_art_tests.rs");
}
