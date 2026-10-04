//! HUP-S9.4 (n5): a federated round's result arriving on this device.
//!
//! A round (FL_ROUND_V1, citrate-chain `docs/fl/FL_ROUND_V1.md`) ends with two artifacts the
//! coordinator operator publishes: the **round bundle** (`citrate-fl-round-bundle/1`, JSON, what
//! anyone needs to audit, replay and challenge the round) and the **merged adapter** (a GGUF LoRA
//! whose sha256 is the bundle's `adapter_sha256`). This module checks them before the eval gate:
//!
//! - the bundle is a v1 round with at least the round's (and the spec's floor of three) devices,
//!   and its chunk counts agree with its value count;
//! - the adapter file hashes to the bundle's `adapter_sha256` and is GGUF;
//! - the base model the round trained on (`config.base_model_sha256`) is the model this app serves.
//!
//! What it does not check, and says so: the round's on-chain record in `FederatedRoundLedger`
//! (not deployed on 40204) and the independent replay (`citrate-fl-replay`, an operator tool).
//! Only typed numbers and hex leave the parser.
//!
//! Fetching: `https` URLs (plain `http` only to this machine), no credentials in the URL, no
//! redirects followed, a size cap, streamed to a `.part` file while hashing and renamed to
//! `<sha256>.gguf` only when the bytes are exactly the expected ones. The member gives the URLs;
//! there is no default mirror. A round's adapter can also come with only its sha256 posted (no
//! bundle), in which case the provenance is whatever the member was told, and the eval gate is
//! the only check on what it does.
//!
//! Size caps are conservative placeholders, **pending owner sign-off**.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The bundle version this build reads.
pub const BUNDLE_VERSION: &str = "citrate-fl-round-bundle/1";
/// Largest bundle accepted (a 16-device round with 1024-wide chunks is well under this).
pub const MAX_BUNDLE_BYTES: u64 = 16 * 1024 * 1024;
/// Placeholder, pending owner sign-off: the largest adapter download accepted.
pub const MAX_ADAPTER_BYTES: u64 = 2 * 1024 * 1024 * 1024;
/// FL_ROUND_V1 §1: a round needs at least three devices.
pub const MIN_PARTICIPANTS_FLOOR: u16 = 3;
/// The chain a round must be recorded on to count as a Citrate network round.
pub const CITRATE_CHAIN_ID: u64 = 40204;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// Placeholder, pending owner sign-off: the longest a single download may take.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(60 * 60);

// ---------------------------------------------------------------------------
// The bundle
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct WireConfig {
    chain_id: u64,
    ledger: String,
    cluster_id: String,
    base_model_sha256: String,
    start_adapter_sha256: String,
    roster: Vec<String>,
    min_participants: u16,
    chunk_dim: u32,
}

#[derive(Deserialize)]
struct WireBundle {
    version: String,
    ordinal: u64,
    round_id: String,
    config_hash: String,
    config: WireConfig,
    participants: Vec<serde_json::Value>,
    n_values: u64,
    chunks: u32,
    input_hashes: Vec<String>,
    output_hashes: Vec<String>,
    adapter_sha256: String,
    record_digest: String,
    state_counts: [u64; 4],
    excluded: Vec<serde_json::Value>,
}

/// What core keeps from a round bundle: typed fields only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoundBundle {
    pub ordinal: u64,
    pub round_id: String,
    pub config_hash: String,
    pub chain_id: u64,
    pub ledger: String,
    pub cluster_id: String,
    pub base_model_sha256: String,
    pub start_adapter_sha256: String,
    pub roster: u32,
    pub min_participants: u16,
    pub participants: u32,
    pub excluded: u32,
    pub n_values: u64,
    pub chunks: u32,
    pub adapter_sha256: String,
    pub record_digest: String,
    /// Per-coordinate Belnap state counts over the round: `[neither, true, false, both]`.
    pub state_counts: [u64; 4],
}

/// `0x` + `2 * n` hex characters, returned lowercase without the prefix.
fn hex_n(field: &str, v: &str, n: usize) -> Result<String, String> {
    let body = v.strip_prefix("0x").unwrap_or(v);
    if body.len() != 2 * n || !body.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(format!("the bundle's {field} is not {n} bytes of hex"));
    }
    Ok(body.to_ascii_lowercase())
}

/// Parse and check a round bundle. Any missing or mistyped field is an error.
pub fn parse_bundle(body: &str) -> Result<RoundBundle, String> {
    if body.len() as u64 > MAX_BUNDLE_BYTES {
        return Err("the round bundle is too large".into());
    }
    let w: WireBundle = serde_json::from_str(body)
        .map_err(|_| "not a round bundle (citrate-fl-round-bundle/1 JSON)".to_string())?;
    if w.version != BUNDLE_VERSION {
        return Err(format!(
            "unsupported round bundle version {:?}; this build reads {BUNDLE_VERSION}",
            w.version
        ));
    }
    let c = &w.config;
    if c.min_participants < MIN_PARTICIPANTS_FLOOR {
        return Err(format!(
            "the round allows {} devices; a round needs at least three",
            c.min_participants
        ));
    }
    let participants = u32::try_from(w.participants.len())
        .map_err(|_| "the bundle lists too many participants".to_string())?;
    if participants < u32::from(c.min_participants) {
        return Err(format!(
            "the bundle lists {participants} participants, fewer than the round's minimum of {}",
            c.min_participants
        ));
    }
    if c.chunk_dim == 0 {
        return Err("the bundle's chunk_dim is 0".into());
    }
    let expect_chunks = w.n_values.div_ceil(u64::from(c.chunk_dim));
    if u64::from(w.chunks) != expect_chunks
        || w.input_hashes.len() != w.chunks as usize
        || w.output_hashes.len() != w.chunks as usize
    {
        return Err(format!(
            "the bundle's chunks ({}) do not match its {} values in chunks of {} and its hash lists",
            w.chunks, w.n_values, c.chunk_dim
        ));
    }
    for (i, h) in w
        .input_hashes
        .iter()
        .chain(w.output_hashes.iter())
        .enumerate()
    {
        hex_n(&format!("chunk hash {i}"), h, 32)?;
    }
    for a in &c.roster {
        hex_n("roster address", a, 20)?;
    }
    Ok(RoundBundle {
        ordinal: w.ordinal,
        round_id: hex_n("round_id", &w.round_id, 32)?,
        config_hash: hex_n("config_hash", &w.config_hash, 32)?,
        chain_id: c.chain_id,
        ledger: hex_n("ledger", &c.ledger, 20)?,
        cluster_id: hex_n("cluster_id", &c.cluster_id, 32)?,
        base_model_sha256: hex_n("base_model_sha256", &c.base_model_sha256, 32)?,
        start_adapter_sha256: hex_n("start_adapter_sha256", &c.start_adapter_sha256, 32)?,
        roster: u32::try_from(c.roster.len())
            .map_err(|_| "the bundle's roster is too long".to_string())?,
        min_participants: c.min_participants,
        participants,
        excluded: u32::try_from(w.excluded.len())
            .map_err(|_| "the bundle lists too many exclusions".to_string())?,
        n_values: w.n_values,
        chunks: w.chunks,
        adapter_sha256: hex_n("adapter_sha256", &w.adapter_sha256, 32)?,
        record_digest: hex_n("record_digest", &w.record_digest, 32)?,
        state_counts: w.state_counts,
    })
}

// ---------------------------------------------------------------------------
// Checking a round against this device
// ---------------------------------------------------------------------------

/// A round whose bundle and adapter were checked on this device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoundProvenance {
    pub round_id: String,
    pub ordinal: u64,
    pub chain_id: u64,
    pub ledger: String,
    pub cluster_id: String,
    pub base_model_sha256: String,
    pub start_adapter_sha256: String,
    pub adapter_sha256: String,
    pub record_digest: String,
    pub participants: u32,
    pub min_participants: u16,
    pub excluded: u32,
    pub state_counts: [u64; 4],
    /// Where the checked adapter file is.
    pub adapter_path: String,
    pub checked_at_ms: u64,
    /// What this build did not verify about the on-chain record, in plain words.
    pub chain_record: String,
    pub notes: Vec<String>,
}

fn sha256_gguf(path: &Path) -> Result<(String, bool), String> {
    let mut f =
        std::fs::File::open(path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    let mut head = Vec::with_capacity(4);
    loop {
        let n = f.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        if head.len() < 4 {
            let take = (4 - head.len()).min(n);
            head.extend_from_slice(&buf[..take]);
        }
        h.update(&buf[..n]);
    }
    Ok((hex::encode(h.finalize()), head.as_slice() == b"GGUF"))
}

/// Check a parsed bundle against an adapter file and the served base model's sha256.
pub fn check_round(
    b: &RoundBundle,
    adapter: &Path,
    served_base_sha256: &str,
    now_ms: u64,
) -> Result<RoundProvenance, String> {
    let (actual, is_gguf) = sha256_gguf(adapter)?;
    if !is_gguf {
        return Err("the round's adapter is not a GGUF file".into());
    }
    if actual != b.adapter_sha256 {
        return Err(format!(
            "the adapter's sha256 {actual} does not match the round's adapter_sha256 {}",
            b.adapter_sha256
        ));
    }
    if served_base_sha256.to_ascii_lowercase() != b.base_model_sha256 {
        return Err(format!(
            "this round trained an adapter for base model sha256 {}, but the model this app serves has sha256 {}; select the round's base model first",
            b.base_model_sha256, served_base_sha256
        ));
    }
    let mut notes = Vec::new();
    if b.chain_id != CITRATE_CHAIN_ID {
        notes.push(format!(
            "This round was recorded on chain {}, not the Citrate network ({CITRATE_CHAIN_ID}): a local or test round.",
            b.chain_id
        ));
    }
    if b.excluded > 0 {
        notes.push(format!(
            "The coordinator left out {} contribution(s) that failed its checks.",
            b.excluded
        ));
    }
    Ok(RoundProvenance {
        round_id: b.round_id.clone(),
        ordinal: b.ordinal,
        chain_id: b.chain_id,
        ledger: b.ledger.clone(),
        cluster_id: b.cluster_id.clone(),
        base_model_sha256: b.base_model_sha256.clone(),
        start_adapter_sha256: b.start_adapter_sha256.clone(),
        adapter_sha256: actual,
        record_digest: b.record_digest.clone(),
        participants: b.participants,
        min_participants: b.min_participants,
        excluded: b.excluded,
        state_counts: b.state_counts,
        adapter_path: adapter.to_string_lossy().to_string(),
        checked_at_ms: now_ms,
        chain_record: "The round's on-chain record (FederatedRoundLedger: accepted after its challenge window) is not checked by this build; the ledger is not deployed on 40204. Its independent replay (citrate-fl-replay) is an operator step. The eval gate still decides whether the adapter loads.".into(),
        notes,
    })
}

type HashCacheKey = (PathBuf, u64, Option<std::time::SystemTime>);

static MODEL_HASH_CACHE: Mutex<Option<(HashCacheKey, String)>> = Mutex::new(None);

/// SHA-256 of the served base model file. Remembered for the same path, length and modification
/// time, so a second check in one session does not re-read gigabytes.
pub fn served_model_sha256(path: &Path) -> Result<String, String> {
    let meta = std::fs::metadata(path)
        .map_err(|e| format!("cannot read the served model {}: {e}", path.display()))?;
    let key: HashCacheKey = (path.to_path_buf(), meta.len(), meta.modified().ok());
    if let Some((k, h)) = MODEL_HASH_CACHE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
    {
        if *k == key {
            return Ok(h.clone());
        }
    }
    let (h, _) = sha256_gguf(path)?;
    *MODEL_HASH_CACHE.lock().unwrap_or_else(|e| e.into_inner()) = Some((key, h.clone()));
    Ok(h)
}

// ---------------------------------------------------------------------------
// Fetching
// ---------------------------------------------------------------------------

fn is_loopback_host(host: &url::Host<&str>) -> bool {
    match host {
        url::Host::Domain(d) => d.eq_ignore_ascii_case("localhost"),
        url::Host::Ipv4(ip) => ip.is_loopback(),
        url::Host::Ipv6(ip) => ip.is_loopback(),
    }
}

/// `https` anywhere, `http` only on loopback; no credentials or fragment. A query is allowed
/// (mirrors sign their URLs).
pub fn check_fetch_url(raw: &str) -> Result<String, String> {
    let u = url::Url::parse(raw.trim()).map_err(|_| format!("not a URL: {raw:?}"))?;
    let host = u.host().ok_or_else(|| "the URL has no host".to_string())?;
    match u.scheme() {
        "https" => {}
        "http" if is_loopback_host(&host) => {}
        "http" => return Err("plain http is allowed only from this machine; use https".into()),
        s => return Err(format!("unsupported scheme {s:?}; use https")),
    }
    if !u.username().is_empty() || u.password().is_some() {
        return Err("the URL must not carry credentials".into());
    }
    if u.fragment().is_some() {
        return Err("the URL must not carry a fragment".into());
    }
    Ok(u.to_string())
}

/// GET `url` into `dest` (via a `.part` file), at most `max` bytes, hashing as it streams.
/// Returns the sha256 and whether the bytes start with the GGUF magic. On any error the partial
/// file is removed.
fn download(url: &str, part: &Path, max: u64) -> Result<(String, bool), String> {
    let url = check_fetch_url(url)?;
    let resp = ureq::get(&url)
        .config()
        .max_redirects(0)
        .http_status_as_error(false)
        .timeout_connect(Some(CONNECT_TIMEOUT))
        .timeout_global(Some(DOWNLOAD_TIMEOUT))
        .build()
        .call()
        .map_err(|e| format!("could not reach {url} ({e})"))?;
    let code = resp.status().as_u16();
    if !resp.status().is_success() {
        return Err(format!("{url} answered HTTP {code}"));
    }
    let mut reader = resp.into_body().into_reader();
    let mut out = std::fs::File::create(part).map_err(|e| e.to_string())?;
    let mut h = Sha256::new();
    let mut head = Vec::with_capacity(4);
    let mut total: u64 = 0;
    let mut buf = vec![0u8; 1 << 20];
    let res = (|| -> Result<(), String> {
        loop {
            let n = reader
                .read(&mut buf)
                .map_err(|e| format!("download failed ({e})"))?;
            if n == 0 {
                return Ok(());
            }
            total += n as u64;
            if total > max {
                return Err(format!("the file is larger than the {max}-byte limit"));
            }
            if head.len() < 4 {
                let take = (4 - head.len()).min(n);
                head.extend_from_slice(&buf[..take]);
            }
            h.update(&buf[..n]);
            out.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        }
    })();
    drop(out);
    if let Err(e) = res {
        let _ = std::fs::remove_file(part);
        return Err(e);
    }
    Ok((hex::encode(h.finalize()), head.as_slice() == b"GGUF"))
}

/// Fetch an adapter whose sha256 is known. Kept as `<dir>/<sha256>.gguf` only if the bytes hash
/// to it and are GGUF.
pub fn fetch_adapter(
    url: &str,
    expected_sha256: &str,
    dir: &Path,
    max: u64,
) -> Result<PathBuf, String> {
    let expected = expected_sha256.trim().to_ascii_lowercase();
    let expected = hex_n("expected sha256", &expected, 32).map_err(|_| {
        "the expected adapter hash must be a sha256 hex string (64 characters)".to_string()
    })?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let part = dir.join(format!("{expected}.gguf.part"));
    let (actual, is_gguf) = download(url, &part, max)?;
    if actual != expected {
        let _ = std::fs::remove_file(&part);
        return Err(format!(
            "the downloaded adapter's sha256 {actual} does not match the expected {expected}"
        ));
    }
    if !is_gguf {
        let _ = std::fs::remove_file(&part);
        return Err("the downloaded adapter is not a GGUF file".into());
    }
    let dest = dir.join(format!("{expected}.gguf"));
    std::fs::rename(&part, &dest).map_err(|e| e.to_string())?;
    Ok(dest)
}

/// Fetch a round bundle, then the adapter it names (checked against the bundle's hash).
pub fn fetch_round(
    bundle_url: &str,
    adapter_url: &str,
    dir: &Path,
    max_adapter: u64,
) -> Result<(RoundBundle, PathBuf), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let mut r = [0u8; 8];
    rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut r);
    let part = dir.join(format!("bundle-{}.json.part", hex::encode(r)));
    let res = download(bundle_url, &part, MAX_BUNDLE_BYTES).and_then(|_| {
        std::fs::read_to_string(&part).map_err(|e| format!("cannot read the bundle ({e})"))
    });
    let _ = std::fs::remove_file(&part);
    let b = parse_bundle(&res?)?;
    let path = fetch_adapter(adapter_url, &b.adapter_sha256, dir, max_adapter)?;
    Ok((b, path))
}

// ---------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------

/// `<app data>/adapters/incoming`: fetched adapters before the eval gate.
fn incoming_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    Ok(crate::fl_rounds::adapter_store(app)?.join("incoming"))
}

fn served_base_sha(app: &tauri::AppHandle) -> Result<String, String> {
    let serve = tauri::Manager::try_state::<crate::serve::ServeState>(app)
        .ok_or_else(|| "internal: managed state unavailable".to_string())?;
    served_model_sha256(&serve.0.current_model_path())
}

/// **Command — fl_round_import.** Check a round bundle and its merged adapter (local files)
/// against the served base model, and keep the round's provenance. Read-only apart from that.
#[tauri::command]
pub async fn fl_round_import(
    app_h: tauri::AppHandle,
    bundle_path: String,
    adapter_path: String,
) -> Result<RoundProvenance, String> {
    crate::blocking::off_main(move || {
        let fl = crate::fl_rounds::state(&app_h)?;
        let p = Path::new(bundle_path.trim());
        let meta = std::fs::metadata(p).map_err(|e| format!("cannot read {bundle_path}: {e}"))?;
        if !meta.is_file() || meta.len() > MAX_BUNDLE_BYTES {
            return Err(format!(
                "{bundle_path} is not a round bundle of a sensible size"
            ));
        }
        let body =
            std::fs::read_to_string(p).map_err(|e| format!("cannot read {bundle_path}: {e}"))?;
        let b = parse_bundle(&body)?;
        let prov = check_round(
            &b,
            Path::new(adapter_path.trim()),
            &served_base_sha(&app_h)?,
            crate::fl_rounds::now_ms(),
        )?;
        fl.record_round(prov.clone())?;
        Ok(prov)
    })
    .await
}

/// **Command — fl_round_fetch.** Download a round bundle and the merged adapter it names (https),
/// then check them as `fl_round_import` does.
#[tauri::command]
pub async fn fl_round_fetch(
    app_h: tauri::AppHandle,
    bundle_url: String,
    adapter_url: String,
) -> Result<RoundProvenance, String> {
    crate::blocking::off_main(move || {
        let fl = crate::fl_rounds::state(&app_h)?;
        let dir = incoming_dir(&app_h)?;
        let (b, path) = fetch_round(&bundle_url, &adapter_url, &dir, MAX_ADAPTER_BYTES)?;
        let prov = check_round(
            &b,
            &path,
            &served_base_sha(&app_h)?,
            crate::fl_rounds::now_ms(),
        )?;
        fl.record_round(prov.clone())?;
        Ok(prov)
    })
    .await
}

/// **Command — fl_adapter_fetch.** Download an adapter whose sha256 was published (https); the
/// file is kept only if it hashes to it. Returns the local path for the eval gate.
#[tauri::command]
pub async fn fl_adapter_fetch(
    app_h: tauri::AppHandle,
    url: String,
    expected_sha256: String,
) -> Result<String, String> {
    crate::blocking::off_main(move || {
        let dir = incoming_dir(&app_h)?;
        let p = fetch_adapter(&url, &expected_sha256, &dir, MAX_ADAPTER_BYTES)?;
        Ok(p.to_string_lossy().to_string())
    })
    .await
}

#[cfg(test)]
mod tests {
    include!("fl_intake_tests.rs");
}
