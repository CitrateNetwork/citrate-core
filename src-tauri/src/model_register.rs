//! Hermes P3 / WP3.1 — the on-chain **ModelRegistry write** (`registerModel`).
//!
//! The read side (`model_registry.rs`) lists chain-registered models for the router's
//! registry source. This closes the loop: after a model is pulled from Hugging Face
//! (off-main-thread `model_catalog_download`), verified, and pinned to IPFS (a CID),
//! the `hf-model-pull-register` skill registers it on-chain so it appears in that
//! registry source for every node.
//!
//! Live ABI (contracts/src/ModelRegistry.sol @ 0xba36fa0d, verified against the
//! foundry `RegisterStarterModels.s.sol` deploy script):
//!
//!   registerModel(
//!     string name, string framework, string version, string ipfsCID,
//!     uint256 sizeBytes, uint256 inferencePrice,
//!     (string description, string[] inputShape, string[] outputShape,
//!      uint256 parameters, string license, string[] tags) metadata
//!   ) payable returns (bytes32)          // fee = REGISTRATION_FEE (0.1 SALT)
//!
//! Rule 3: NOTHING here signs. The calldata is submitted as a PENDING
//! SignatureCeremony; the human approves + broadcasts via the signing surface. Rule 1:
//! the calldata is byte-exact against `cast` (round-trip test) — no fabricated tx.
use serde_json::json;

use crate::model_registry::selector;

/// Per-call registration fee the ModelRegistry charges (`REGISTRATION_FEE = 0.1 ether`
/// in the contract). Sent as the tx `value`.
pub const REGISTRATION_FEE_WEI: u128 = 100_000_000_000_000_000; // 0.1 * 1e18

/// Explicit gas for `registerModel` (a calldata tx MUST carry explicit gas — it writes
/// several storage slots + calls the precompile, so it is heavier than a bond register).
const REGISTER_MODEL_GAS: u64 = 900_000;

/// The `registerModel` signature the selector + tuple layout derive from.
const REGISTER_MODEL_SIG: &str = "registerModel(string,string,string,string,uint256,uint256,(string,string[],string[],uint256,string,string[]))";

// --------------------------------------------------------------------------
// Minimal ABI encoder for exactly this call's shape (head/tail, recursive).
// Proven byte-exact against `cast calldata` in the tests — not a general library.
// --------------------------------------------------------------------------

/// A 32-byte big-endian word from a u128 (uint256 with the high 128 bits zero — ample
/// for sizes, prices, and parameter counts).
fn word_u128(n: u128) -> [u8; 32] {
    let mut w = [0u8; 32];
    w[16..32].copy_from_slice(&n.to_be_bytes());
    w
}

/// `bytes`/`string` tail: a length word then the data right-padded to a 32-byte multiple.
fn enc_bytes(b: &[u8]) -> Vec<u8> {
    let mut out = word_u128(b.len() as u128).to_vec();
    out.extend_from_slice(b);
    let pad = (32 - b.len() % 32) % 32;
    out.extend(std::iter::repeat_n(0u8, pad));
    out
}

fn enc_string(s: &str) -> Vec<u8> {
    enc_bytes(s.as_bytes())
}

/// One ABI value in an ordered block: either an inline static word or a dynamic tail
/// (which the block places after the heads and points at with an offset word).
enum Abi {
    Word([u8; 32]),
    Dyn(Vec<u8>),
}

/// Encode an ordered list of values as an ABI block: a head of one word per value
/// (static inline, dynamic = offset from the block start) followed by the dynamic tails
/// in order. This is the single rule that composes for the tuple and the top-level args.
fn encode_block(items: Vec<Abi>) -> Vec<u8> {
    let head_len = items.len() * 32;
    let mut head = Vec::with_capacity(head_len);
    let mut tail = Vec::new();
    let mut offset = head_len;
    for it in items {
        match it {
            Abi::Word(w) => head.extend_from_slice(&w),
            Abi::Dyn(t) => {
                head.extend_from_slice(&word_u128(offset as u128));
                offset += t.len();
                tail.extend_from_slice(&t);
            }
        }
    }
    head.extend_from_slice(&tail);
    head
}

/// `string[]`: a length word then the array's own block of (dynamic) string elements —
/// element offsets are relative to just after the length word. Empty → a single 0 word.
fn enc_string_array(items: &[String]) -> Vec<u8> {
    let mut out = word_u128(items.len() as u128).to_vec();
    out.extend_from_slice(&encode_block(
        items.iter().map(|s| Abi::Dyn(enc_string(s))).collect(),
    ));
    out
}

/// The `ModelMetadata` fields the register call carries. `input_shape`/`output_shape`
/// stay empty and `parameters` 0 — the same shape the on-chain starter-model script
/// uses (the wallet UI does not depend on them yet).
#[derive(Debug, Clone, Default)]
pub struct ModelMetadata {
    pub description: String,
    pub license: String,
    pub tags: Vec<String>,
}

/// Build the `registerModel` calldata: selector ++ ABI-encoded args. Byte-exact against
/// `cast calldata` for the same inputs (see tests). Pure — no I/O, no signing.
pub fn register_model_calldata(
    name: &str,
    framework: &str,
    version: &str,
    ipfs_cid: &str,
    size_bytes: u128,
    inference_price: u128,
    meta: &ModelMetadata,
) -> Vec<u8> {
    // The metadata tuple is itself a dynamic value (it contains dynamic members), so it
    // is encoded as its own block and placed as a Dyn at the top level.
    let tuple = encode_block(vec![
        Abi::Dyn(enc_string(&meta.description)),
        Abi::Dyn(enc_string_array(&[])), // inputShape (empty)
        Abi::Dyn(enc_string_array(&[])), // outputShape (empty)
        Abi::Word(word_u128(0)),         // parameters
        Abi::Dyn(enc_string(&meta.license)),
        Abi::Dyn(enc_string_array(&meta.tags)),
    ]);
    let args = encode_block(vec![
        Abi::Dyn(enc_string(name)),
        Abi::Dyn(enc_string(framework)),
        Abi::Dyn(enc_string(version)),
        Abi::Dyn(enc_string(ipfs_cid)),
        Abi::Word(word_u128(size_bytes)),
        Abi::Word(word_u128(inference_price)),
        Abi::Dyn(tuple),
    ]);
    let mut out = Vec::with_capacity(4 + args.len());
    out.extend_from_slice(&selector(REGISTER_MODEL_SIG));
    out.extend_from_slice(&args);
    out
}

/// The pending-ceremony tx JSON: from the member EOA, to the ModelRegistry, value = the
/// registration fee, data = the `registerModel` calldata (mirrors `storage.rs`).
fn encode_register_tx_json(from: &str, calldata: &[u8]) -> String {
    json!({
        "from": from,
        // REROLL-SENSITIVE: from the pinned address book, never hardcoded.
        "to": crate::addresses::model_registry(),
        "value": format!("0x{REGISTRATION_FEE_WEI:x}"),
        "data": format!("0x{}", hex::encode(calldata)),
        "gas": format!("0x{REGISTER_MODEL_GAS:x}"),
        "chainId": format!("0x{:x}", 40204u64),
    })
    .to_string()
}

/// **Command — models_registry_register.** Propose registering a pulled+verified+pinned
/// model on-chain. Builds the `registerModel` calldata and submits it as a PENDING
/// SignatureCeremony (Rule 3 — the human approves + broadcasts via the signing surface;
/// nothing signs here). `ipfs_cid` must already be a real pinned CID (the pull path pins
/// the verified GGUF first) — an empty CID is rejected up-front, mirroring the contract's
/// own `require`, so the ceremony never carries a tx that would revert.
#[tauri::command]
#[allow(clippy::too_many_arguments)] // IPC arg list fixed by the tested invoke contract
pub fn models_registry_register(
    custody: tauri::State<'_, crate::custody::CustodyState>,
    ceremony: tauri::State<'_, crate::ceremony::CeremonyState>,
    name: String,
    framework: String,
    version: String,
    ipfs_cid: String,
    size_bytes: u64,
    inference_price: u64,
    description: String,
    license: String,
    tags: Vec<String>,
) -> std::result::Result<(), String> {
    if name.trim().is_empty() {
        return Err("model name is required".into());
    }
    if ipfs_cid.trim().is_empty() {
        // The contract requires a non-empty CID; the model must be pinned first.
        return Err("model must be pinned to IPFS first — CID is required to register".into());
    }
    let meta = ModelMetadata {
        description,
        license,
        tags,
    };
    let calldata = register_model_calldata(
        &name,
        &framework,
        &version,
        &ipfs_cid,
        size_bytes as u128,
        inference_price as u128,
        &meta,
    );
    let wallet = crate::wallet::address_auto_unlocked(&custody.0).map_err(|e| e.to_string())?;
    let raw = encode_register_tx_json(&wallet.address, &calldata);
    let intent = crate::ceremony::SignatureIntent {
        origin: "agent:hermes".to_string(),
        kind: crate::ceremony::IntentKind::Transaction,
        chain_id: 40204,
        raw,
    };
    ceremony.0.request(intent);
    Ok(())
}

#[cfg(test)]
mod tests {
    include!("model_register_tests.rs");
}
