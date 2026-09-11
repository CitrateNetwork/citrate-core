//! Hermes WP0.2b — read the on-chain **ModelRegistry** (a backbone of the app layer;
//! `addresses::model_registry()` on 40204) so the router's registry source lists REAL,
//! chain-registered models. PURE `eth_call` reads via the shared RpcClient — it NEVER
//! fabricates a model (Rule 1); an RPC/decoding failure is an honest error, not an empty
//! list dressed as truth.
//!
//! Interface (contracts/src/ModelRegistry.sol):
//!   getAllModelHashes() -> bytes32[]
//!   getModel(bytes32)   -> (address owner, string name, string framework, string version,
//!                           string ipfsCID, uint256 inferencePrice, uint256 totalInferences,
//!                           bool isActive)
//!
//! Selectors are computed from the signature at call time (like storage.rs `registerModel`),
//! so there is no pinned-const drift to police. The dynamic-ABI decoders are the risky part
//! and are unit-tested by round-trip in `model_registry_tests.rs`.
use serde::Serialize;
use serde_json::json;
use sha3::{Digest as _, Keccak256};

use crate::rpc::{RpcClient, RpcTransport};

/// 4-byte selector = keccak256(signature)[..4].
fn selector(sig: &str) -> [u8; 4] {
    let mut h = Keccak256::new();
    h.update(sig.as_bytes());
    let out = h.finalize();
    [out[0], out[1], out[2], out[3]]
}

/// A model as the router sees it — sourced from the on-chain registry. Not-ready by default
/// (must be pulled + verified locally before it can serve); the router marks it "download to use".
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RegistryModel {
    /// The `0x`-hex `modelHash` — the stable id.
    pub id: String,
    /// Human name (from `getModel`).
    pub name: String,
    /// `0x`-hex owner address.
    pub owner: String,
    /// IPFS CID of the model weights.
    #[serde(rename = "ipfsCid")]
    pub ipfs_cid: String,
}

/// The `{to, data}` eth_call object for the ModelRegistry, `data = selector ++ tail`.
fn call_obj(sel: [u8; 4], tail: &[u8]) -> serde_json::Value {
    let mut data = Vec::with_capacity(4 + tail.len());
    data.extend_from_slice(&sel);
    data.extend_from_slice(tail);
    json!({
        "to": crate::addresses::model_registry(),
        "data": format!("0x{}", hex::encode(data)),
    })
}

/// A 32-byte big-endian word → usize (bounds-checked: the high 24 bytes must be zero, so an
/// absurd offset/length from a malformed return errors instead of truncating).
fn be_usize(word: &[u8]) -> Result<usize, String> {
    if word.len() != 32 {
        return Err("abi word: not 32 bytes".into());
    }
    if word[..24].iter().any(|&b| b != 0) {
        return Err("abi word: value too large for usize".into());
    }
    let mut b = [0u8; 8];
    b.copy_from_slice(&word[24..32]);
    Ok(u64::from_be_bytes(b) as usize)
}

/// Decode an ABI `bytes32[]` return (a single dynamic return): head offset word, then length,
/// then the hashes.
pub fn decode_bytes32_array(ret: &[u8]) -> Result<Vec<[u8; 32]>, String> {
    if ret.len() < 64 {
        return Err("bytes32[]: short return".into());
    }
    let offset = be_usize(&ret[0..32])?;
    if offset + 32 > ret.len() {
        return Err("bytes32[]: offset past end".into());
    }
    let len = be_usize(&ret[offset..offset + 32])?;
    let start = offset + 32;
    if start + len.saturating_mul(32) > ret.len() {
        return Err("bytes32[]: length exceeds data".into());
    }
    let mut out = Vec::with_capacity(len);
    for i in 0..len {
        let s = start + i * 32;
        let mut h = [0u8; 32];
        h.copy_from_slice(&ret[s..s + 32]);
        out.push(h);
    }
    Ok(out)
}

/// Read a dynamic `string` at head-relative offset `off`.
fn read_string_at(ret: &[u8], off: usize) -> Result<String, String> {
    if off + 32 > ret.len() {
        return Err("abi string: offset past end".into());
    }
    let len = be_usize(&ret[off..off + 32])?;
    let s = off + 32;
    if s + len > ret.len() {
        return Err("abi string: length exceeds data".into());
    }
    String::from_utf8(ret[s..s + len].to_vec()).map_err(|_| "abi string: invalid utf-8".into())
}

/// Decode `getModel`'s return head → (owner, name, ipfsCID). Head = 8 words:
/// `0 owner | 1 name-off | 2 framework-off | 3 version-off | 4 ipfsCID-off | 5 price | 6 infer | 7 active`.
/// Dynamic fields (strings) hold a head-relative offset; static fields hold the value inline.
pub fn decode_get_model(ret: &[u8]) -> Result<(String, String, String), String> {
    if ret.len() < 32 * 8 {
        return Err("getModel: short return (< 8 head words)".into());
    }
    let owner = format!("0x{}", hex::encode(&ret[12..32])); // address = low 20 bytes of word 0
    let name_off = be_usize(&ret[32..64])?; // word 1
    let cid_off = be_usize(&ret[128..160])?; // word 4
    let name = read_string_at(ret, name_off)?;
    let ipfs_cid = read_string_at(ret, cid_off)?;
    Ok((owner, name, ipfs_cid))
}

/// Read every registered model from the on-chain registry. Generic over the transport so it is
/// testable with a mock; production wires `RpcClient::citrate()`. One `getAllModelHashes` call +
/// one `getModel` per hash.
pub fn read_registry_models<T: RpcTransport>(rpc: &RpcClient<T>) -> Result<Vec<RegistryModel>, String> {
    let hashes_ret = rpc
        .eth_call(call_obj(selector("getAllModelHashes()"), &[]))
        .map_err(|e| e.to_string())?;
    let hashes = decode_bytes32_array(&hashes_ret)?;
    let get_model = selector("getModel(bytes32)");
    let mut out = Vec::with_capacity(hashes.len());
    for h in hashes {
        let ret = rpc.eth_call(call_obj(get_model, &h)).map_err(|e| e.to_string())?;
        let (owner, name, ipfs_cid) = decode_get_model(&ret)?;
        out.push(RegistryModel {
            id: format!("0x{}", hex::encode(h)),
            name,
            owner,
            ipfs_cid,
        });
    }
    Ok(out)
}

/// **Command — models_registry_list.** Read the on-chain ModelRegistry OFF the main thread
/// (each `eth_call` is blocking network I/O — a sync command would freeze the UI, the v0.2.3
/// beach-ball lesson). Returns the decoded models, or an honest error string (never a fake list).
#[tauri::command]
pub async fn models_registry_list() -> std::result::Result<Vec<RegistryModel>, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let rpc = crate::rpc::RpcClient::citrate();
        read_registry_models(&rpc)
    })
    .await
    .map_err(|e| format!("models_registry_list: background read failed: {e}"))?
}

#[cfg(test)]
mod tests {
    include!("model_registry_tests.rs");
}
