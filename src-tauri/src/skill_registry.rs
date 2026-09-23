//! Hermes P2 — read the on-chain **SkillRegistry** (`addresses::skill_registry()`,
//! 0x2B6878…97c on 40204 after the 2026-09-12 `srp-s5-diskfix` reroll — the reproducible
//! CREATE2 deploy, was the plain-create 0x896cd293…) so Hermes ships WITH skills from the chain
//! instead of empty. `totalSkills()=3` (hello, hf-model-register, contract-deploy). PURE
//! `eth_call` reads via the shared RpcClient (Rule 1: never fabricates a skill; an RPC/decode
//! failure is an honest error). Mirrors `model_registry.rs` and reuses its ABI decoders.
//!
//! Interface (SkillRegistry, a ModelRegistry-parity contract):
//!   getAllSkillHashes() -> bytes32[]
//!   getSkill(bytes32)   -> (address owner, string name, string version, string manifestCID,
//!                           string description, bool isActive)
use serde::Serialize;
use serde_json::json;

use crate::model_registry::{be_usize, decode_bytes32_array, read_string_at, selector};
use crate::rpc::{RpcClient, RpcTransport};

/// A skill as Hermes sees it — sourced from the on-chain SkillRegistry.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RegistrySkill {
    /// The `0x`-hex `skillHash` — the stable id.
    pub id: String,
    /// Human name (e.g. "hf-model-register").
    pub name: String,
    /// Semver.
    pub version: String,
    /// IPFS CID of the WASM capsule manifest (empty for a not-yet-published skill).
    #[serde(rename = "manifestCid")]
    pub manifest_cid: String,
    /// Short description.
    pub description: String,
    /// `0x`-hex owner address.
    pub owner: String,
}

/// The `{to, data}` eth_call object for the SkillRegistry.
fn call_obj(sel: [u8; 4], tail: &[u8]) -> serde_json::Value {
    let mut data = Vec::with_capacity(4 + tail.len());
    data.extend_from_slice(&sel);
    data.extend_from_slice(tail);
    json!({
        "to": crate::addresses::skill_registry(),
        "data": format!("0x{}", hex::encode(data)),
    })
}

/// Decode `getSkill`'s return head → (owner, name, version, manifestCID, description). Head = 6
/// words: `0 owner | 1 name-off | 2 version-off | 3 manifestCID-off | 4 description-off | 5 isActive`.
pub fn decode_get_skill(ret: &[u8]) -> Result<(String, String, String, String, String), String> {
    if ret.len() < 32 * 6 {
        return Err("getSkill: short return (< 6 head words)".into());
    }
    let owner = format!("0x{}", hex::encode(&ret[12..32])); // address = low 20 bytes of word 0
    let name = read_string_at(ret, be_usize(&ret[32..64])?)?; // word 1
    let version = read_string_at(ret, be_usize(&ret[64..96])?)?; // word 2
    let manifest_cid = read_string_at(ret, be_usize(&ret[96..128])?)?; // word 3
    let description = read_string_at(ret, be_usize(&ret[128..160])?)?; // word 4
    Ok((owner, name, version, manifest_cid, description))
}

/// Read every registered skill from the on-chain registry (generic over transport for tests).
pub fn read_registry_skills<T: RpcTransport>(
    rpc: &RpcClient<T>,
) -> Result<Vec<RegistrySkill>, String> {
    let hashes_ret = rpc
        .eth_call(call_obj(selector("getAllSkillHashes()"), &[]))
        .map_err(|e| e.to_string())?;
    let hashes = decode_bytes32_array(&hashes_ret)?;
    let get_skill = selector("getSkill(bytes32)");
    let mut out = Vec::with_capacity(hashes.len());
    for h in hashes {
        let ret = rpc
            .eth_call(call_obj(get_skill, &h))
            .map_err(|e| e.to_string())?;
        let (owner, name, version, manifest_cid, description) = decode_get_skill(&ret)?;
        out.push(RegistrySkill {
            id: format!("0x{}", hex::encode(h)),
            name,
            version,
            manifest_cid,
            description,
            owner,
        });
    }
    Ok(out)
}

/// **Command — skills_registry_list.** Read the on-chain SkillRegistry OFF the main thread
/// (each `eth_call` is blocking network I/O). Returns the decoded skills, or an honest error.
#[tauri::command]
pub async fn skills_registry_list() -> std::result::Result<Vec<RegistrySkill>, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let rpc = crate::rpc::RpcClient::citrate();
        read_registry_skills(&rpc)
    })
    .await
    .map_err(|e| format!("skills_registry_list: background read failed: {e}"))?
}

#[cfg(test)]
mod tests {
    include!("skill_registry_tests.rs");
}
