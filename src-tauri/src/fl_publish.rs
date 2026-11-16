//! citrate-core 0.5.4 — **publish a round's requirements file** before `WorkMarket.open` (FL design
//! board 07, Open a round).
//!
//! A WorkMarket offer commits `requirementsHash = keccak256(bytes)` of a small JSON file and names
//! its `requirementsCid`; every member's offers read model (FLM-CORE-1, `fl_market.rs`) fetches
//! the file from IPFS and accepts it only when the hash matches. This command writes that file:
//! it validates the requirements, serializes them in the one format the read model accepts
//! (`{"v":1,"dataClasses":[..],"architectures":[..],"minVramGb":N,"estHours":N}`, no other
//! fields), adds and pins the bytes on the app's bundled IPFS node (kubo, `storage.rs`), and
//! returns the CID and the hash the open ceremony commits.
//!
//! Rule 7: the data source is the member's own form; the sink is the bundled kubo API on the
//! loopback (`CITRATE_KUBO_API`, default `http://127.0.0.1:5001`). Nothing here signs or sends a
//! transaction (Rule 3).

use serde::{Deserialize, Serialize};
use sha3::{Digest, Keccak256};

use crate::storage::{KuboTransport, UreqKuboTransport};

/// The requirements file, version 1 (the shape `fl_market.rs` `Requirements` parses).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Requirements {
    pub v: u32,
    pub data_classes: Vec<String>,
    pub architectures: Vec<String>,
    pub min_vram_gb: u32,
    pub est_hours: u32,
}

/// What a publish returns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishedRequirements {
    pub cid: String,
    /// `0x` + keccak256 of the exact bytes at `cid`.
    pub requirements_hash: String,
    pub bytes: u64,
}

/// The read model's caps (fl_market.rs MAX_REQUIREMENTS_BYTES, MAX_CID_LEN).
pub const MAX_REQUIREMENTS_BYTES: usize = 16 * 1024;
pub const MAX_CID_LEN: usize = 128;

fn token(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.')
}

impl Requirements {
    pub fn problem(&self) -> Option<String> {
        if self.v != 1 {
            return Some("the requirements file must be version 1".into());
        }
        if self.data_classes.is_empty()
            || self.data_classes.len() > 32
            || !self.data_classes.iter().all(|c| token(c))
        {
            return Some(
                "name one to 32 data classes (letters, digits, dots, dashes or underscores)".into(),
            );
        }
        if self.architectures.len() > 32 || !self.architectures.iter().all(|c| token(c)) {
            return Some(
                "architectures must be letters, digits, dots, dashes or underscores".into(),
            );
        }
        if self.min_vram_gb == 0 || self.min_vram_gb > 1024 {
            return Some("minimum GPU memory must be 1 to 1024 GB".into());
        }
        if self.est_hours == 0 || self.est_hours > 720 {
            return Some("estimated hours must be 1 to 720".into());
        }
        None
    }

    /// The exact bytes published and hashed.
    pub fn to_bytes(&self) -> Result<Vec<u8>, String> {
        serde_json::to_vec(self).map_err(|e| e.to_string())
    }
}

/// Validate, add and pin on `kubo`, and return the CID and hash.
pub fn publish(
    req: &Requirements,
    kubo: &dyn KuboTransport,
) -> Result<PublishedRequirements, String> {
    if let Some(p) = req.problem() {
        return Err(p);
    }
    let bytes = req.to_bytes()?;
    if bytes.len() > MAX_REQUIREMENTS_BYTES {
        return Err("the requirements file is too large".into());
    }
    let added = kubo
        .add("requirements.json", &bytes)
        .map_err(|e| format!("the app's IPFS node did not take the requirements file: {e}"))?;
    if added.cid.is_empty() || added.cid.len() > MAX_CID_LEN {
        return Err("the app's IPFS node returned an unusable CID".into());
    }
    kubo.pin_add(&added.cid)
        .map_err(|e| format!("the requirements file was added but not pinned: {e}"))?;
    Ok(PublishedRequirements {
        cid: added.cid,
        requirements_hash: format!("0x{}", hex::encode(Keccak256::digest(&bytes))),
        bytes: bytes.len() as u64,
    })
}

/// **fl_requirements_publish** — add and pin a round's requirements file on the bundled IPFS node;
/// returns the `requirementsCid` and `requirementsHash` the open ceremony commits.
#[tauri::command]
pub async fn fl_requirements_publish(
    requirements: Requirements,
) -> Result<PublishedRequirements, String> {
    crate::blocking::off_main(move || publish(&requirements, &UreqKuboTransport::from_env())).await
}

#[cfg(test)]
#[path = "fl_publish_tests.rs"]
mod tests;
