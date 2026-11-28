//! Agent specs and their pins (L1-WP03 step 2). A spec names the program Core runs and the file
//! whose sha256 must equal the reviewed pin; nothing is spawned until that check passes.
//! Data source: `release/agents.lock.json` (WP13) rows, read by [`locked_launch`].

use serde_json::Value;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};

/// What the host runs for one agent.
#[derive(Debug, Clone)]
pub struct AgentSpec {
    /// The `agents.lock.json` id (`codex`, `claude`, ...).
    pub id: String,
    /// The program, run with a cleared environment (the caller passes the whole env).
    pub command: PathBuf,
    pub args: Vec<String>,
    /// The file the pin covers: the native binary, or a Node agent's installed entry script.
    pub pinned_file: PathBuf,
    pub sha256: [u8; 32],
}

/// The launch row of one locked agent: its command, arguments and fixed environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockedLaunch {
    pub id: String,
    pub version: String,
    pub cmd: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

/// `id`'s launch row in an `agents.lock.json` document; refused when the lock has no such row.
pub fn locked_launch(lock_json: &str, id: &str) -> Result<LockedLaunch, String> {
    let lock: Value =
        serde_json::from_str(lock_json).map_err(|e| format!("agents.lock.json: {e}"))?;
    let row = lock["agents"]
        .as_array()
        .and_then(|rows| rows.iter().find(|r| r["id"].as_str() == Some(id)))
        .ok_or_else(|| format!("\"{id}\" is not in agents.lock.json"))?;
    let strs = |v: &Value| -> Vec<String> {
        v.as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default()
    };
    let launch = &row["launch"];
    let cmd = launch["cmd"]
        .as_str()
        .filter(|c| !c.is_empty())
        .ok_or_else(|| format!("{id}: no launch.cmd"))?;
    let env = launch["env"]
        .as_object()
        .map(|m| {
            m.iter()
                .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string())))
                .collect()
        })
        .unwrap_or_default();
    Ok(LockedLaunch {
        id: id.to_string(),
        version: row["version"].as_str().unwrap_or("").to_string(),
        cmd: cmd.to_string(),
        args: strs(&launch["args"]),
        env,
    })
}

/// The sha256 of a file, read in full.
pub fn sha256_file(path: &Path) -> std::io::Result<[u8; 32]> {
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().into())
}

/// A 64-hex sha256 as bytes.
pub fn parse_sha256(hex_str: &str) -> Result<[u8; 32], String> {
    let v = hex::decode(hex_str).map_err(|_| format!("not a sha256: {hex_str:?}"))?;
    v.try_into()
        .map_err(|_| format!("not a sha256: {hex_str:?}"))
}

/// Refuses unless `spec.pinned_file` hashes to `spec.sha256`. The reason names both values.
pub fn verify_pin(spec: &AgentSpec) -> Result<(), String> {
    let got = sha256_file(&spec.pinned_file).map_err(|e| {
        format!(
            "{}: cannot read pinned file {}: {e}",
            spec.id,
            spec.pinned_file.display()
        )
    })?;
    if got != spec.sha256 {
        return Err(format!(
            "{}: {} has sha256 {}, which does not match the pinned {}; not started",
            spec.id,
            spec.pinned_file.display(),
            hex::encode(got),
            hex::encode(spec.sha256)
        ));
    }
    Ok(())
}
