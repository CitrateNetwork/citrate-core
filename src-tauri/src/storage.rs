//! CX-S2 (lane s2) — storage & pinning host commands (C-17).
//!
//! S2.1 wires the **kubo seam**: a small HTTP client over the local IPFS (kubo) daemon API
//! (`/api/v0/{add,pin/add,pin/ls,pin/rm,cat}`) plus a [`StorageManager`] that orchestrates
//! add / pin / unpin / retrieve / list over it and a local pin index. The HTTP transport is a
//! trait ([`KuboTransport`]) so tests drive fixture responses — no real daemon in CI (mirrors
//! `model.rs`'s injectable transport).
//!
//! What S2.1 does NOT do yet: the SALT **bond** behind a pin is recorded as-passed here; making
//! it a real ceremony-gated `IPFSIncentivesV3` bond is S2.2. `pinState` is honestly only
//! `pinned`/`unpinned` (live in the daemon or not) — the on-chain `challenged` state arrives with
//! the bond client. Commands are STATELESS: each builds a manager from the app data dir (the pin
//! index lives on disk), so no managed Tauri state / no spine edit.
//!
//! Command NAMES are frozen in `lib.rs`; bodies + signatures are filled in HERE.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The default kubo HTTP API endpoint (loopback; the daemon is LOCAL, nothing remote).
pub const DEFAULT_KUBO_API: &str = "http://127.0.0.1:5001";
/// Env override for the kubo API root (dev/tests point this at a stub or an alt port).
pub const KUBO_API_ENV: &str = "CITRATE_KUBO_API";

// ---------------------------------------------------------------------------
// Errors — coarse, secret-free.
// ---------------------------------------------------------------------------

/// A storage/pinning error. Every `Display` is safe to surface to the UI.
#[derive(Debug)]
pub enum StorageError {
    /// The kubo daemon HTTP call failed (not running, non-2xx, TLS/transport).
    Transport(String),
    /// A local filesystem op failed (read the file to add, write a retrieved file, the index).
    Io(String),
    /// A kubo response could not be parsed into the expected shape.
    Parse(String),
}

impl std::fmt::Display for StorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StorageError::Transport(m) => write!(f, "kubo transport error: {m}"),
            StorageError::Io(m) => write!(f, "storage io error: {m}"),
            StorageError::Parse(m) => write!(f, "kubo response parse error: {m}"),
        }
    }
}
impl std::error::Error for StorageError {}

type Result<T> = std::result::Result<T, StorageError>;

/// The result of adding a file to IPFS.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AddOutcome {
    pub cid: String,
    pub size_bytes: u64,
}

/// One row of the pinning file store — mirrors the bridge `PinRow` DTO 1:1 (camelCase).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PinRow {
    pub cid: String,
    pub size_bytes: u64,
    /// The SALT bond behind this pin. Empty until a real bond is placed (S2.2); free-form here.
    pub bond_salt: String,
    pub pin_state: PinState,
    /// Unix seconds when first added locally (0 for a pin discovered live but not tracked).
    pub added_at: u64,
}

/// The lifecycle of a pin. S2.1 distinguishes only live-in-daemon (`Pinned`) vs not (`Unpinned`);
/// `Pinning`/`Challenged` are reserved for the S2.2 on-chain bond/challenge flow.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PinState {
    Pinned,
    Pinning,
    Challenged,
    Unpinned,
}

// ---------------------------------------------------------------------------
// The kubo HTTP transport seam (injectable — mirrors model.rs::ModelTransport).
// ---------------------------------------------------------------------------

/// The kubo `/api/v0` operations the manager needs. Production is [`UreqKuboTransport`]; tests
/// inject a fixture. All ops are POST in the real kubo API.
pub trait KuboTransport: Send + Sync {
    /// `POST /api/v0/add` (multipart) → the added object's CID + size.
    fn add(&self, filename: &str, bytes: &[u8]) -> Result<AddOutcome>;
    /// `POST /api/v0/pin/add?arg=<cid>`.
    fn pin_add(&self, cid: &str) -> Result<()>;
    /// `POST /api/v0/pin/rm?arg=<cid>`.
    fn pin_rm(&self, cid: &str) -> Result<()>;
    /// `POST /api/v0/pin/ls?type=recursive` → the recursively-pinned CIDs.
    fn pin_ls(&self) -> Result<Vec<String>>;
    /// `POST /api/v0/cat?arg=<cid>` → the raw bytes.
    fn cat(&self, cid: &str) -> Result<Vec<u8>>;
}

/// Production transport: blocking `ureq` against the loopback kubo API.
pub struct UreqKuboTransport {
    api: String,
}

impl UreqKuboTransport {
    pub fn new(api: impl Into<String>) -> Self {
        UreqKuboTransport { api: api.into() }
    }
    /// Resolve the API root from the env override or the loopback default.
    pub fn from_env() -> Self {
        let api = std::env::var(KUBO_API_ENV).unwrap_or_else(|_| DEFAULT_KUBO_API.to_string());
        UreqKuboTransport::new(api)
    }
}

impl KuboTransport for UreqKuboTransport {
    fn add(&self, filename: &str, bytes: &[u8]) -> Result<AddOutcome> {
        // kubo /add is multipart/form-data with a single `file` part. Hand-build the body
        // (ureq carries no multipart helper); the boundary is fixed + collision-safe for one part.
        let boundary = "----citratecoreKUBOaddBOUNDARY7f3a";
        let mut body: Vec<u8> = Vec::with_capacity(bytes.len() + 256);
        body.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{filename}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
            )
            .as_bytes(),
        );
        body.extend_from_slice(bytes);
        body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
        let url = format!("{}/api/v0/add?cid-version=1", self.api);
        let resp = ureq::post(&url)
            .header(
                "Content-Type",
                &format!("multipart/form-data; boundary={boundary}"),
            )
            .send(&body[..])
            .map_err(|e| StorageError::Transport(e.to_string()))?;
        let text = resp
            .into_body()
            .read_to_string()
            .map_err(|e| StorageError::Transport(e.to_string()))?;
        parse_add_response(&text)
    }

    fn pin_add(&self, cid: &str) -> Result<()> {
        self.post_empty(&format!("{}/api/v0/pin/add?arg={cid}", self.api))
    }
    fn pin_rm(&self, cid: &str) -> Result<()> {
        self.post_empty(&format!("{}/api/v0/pin/rm?arg={cid}", self.api))
    }

    fn pin_ls(&self) -> Result<Vec<String>> {
        let url = format!("{}/api/v0/pin/ls?type=recursive", self.api);
        let text = ureq::post(&url)
            .send_empty()
            .map_err(|e| StorageError::Transport(e.to_string()))?
            .into_body()
            .read_to_string()
            .map_err(|e| StorageError::Transport(e.to_string()))?;
        parse_pin_ls(&text)
    }

    fn cat(&self, cid: &str) -> Result<Vec<u8>> {
        let url = format!("{}/api/v0/cat?arg={cid}", self.api);
        let mut buf = Vec::new();
        use std::io::Read;
        ureq::post(&url)
            .send_empty()
            .map_err(|e| StorageError::Transport(e.to_string()))?
            .into_body()
            .into_reader()
            .read_to_end(&mut buf)
            .map_err(|e| StorageError::Transport(e.to_string()))?;
        Ok(buf)
    }
}

impl UreqKuboTransport {
    fn post_empty(&self, url: &str) -> Result<()> {
        ureq::post(url)
            .send_empty()
            .map_err(|e| StorageError::Transport(e.to_string()))?;
        Ok(())
    }
}

/// Parse kubo `/add`'s JSON (`{"Name","Hash","Size"}`; `Size` is a decimal STRING).
fn parse_add_response(text: &str) -> Result<AddOutcome> {
    #[derive(Deserialize)]
    struct AddJson {
        #[serde(rename = "Hash")]
        hash: String,
        #[serde(rename = "Size", default)]
        size: String,
    }
    // kubo may stream multiple JSON objects (one per file); the LAST is the root add.
    let last = text
        .lines()
        .rfind(|l| !l.trim().is_empty())
        .ok_or_else(|| StorageError::Parse("empty add response".into()))?;
    let j: AddJson = serde_json::from_str(last).map_err(|e| StorageError::Parse(e.to_string()))?;
    let size = j.size.parse::<u64>().unwrap_or(0);
    Ok(AddOutcome {
        cid: j.hash,
        size_bytes: size,
    })
}

/// Parse kubo `/pin/ls`'s JSON (`{"Keys":{"<cid>":{"Type":"recursive"}}}`) → the CIDs.
fn parse_pin_ls(text: &str) -> Result<Vec<String>> {
    #[derive(Deserialize)]
    struct PinLs {
        #[serde(rename = "Keys", default)]
        keys: std::collections::BTreeMap<String, serde_json::Value>,
    }
    let j: PinLs = serde_json::from_str(text).map_err(|e| StorageError::Parse(e.to_string()))?;
    Ok(j.keys.into_keys().collect())
}

// ---------------------------------------------------------------------------
// The local pin index (cid → size/bond/added_at), persisted next to the app data.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
struct PinRecord {
    size_bytes: u64,
    bond_salt: String,
    added_at: u64,
}

/// The on-disk index of everything the app has added/pinned locally. Reconciled against the
/// daemon's live pin set on `list` (the daemon is the truth for *whether* a CID is pinned; the
/// index carries the metadata the daemon does not — size, the SALT bond, when we added it).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct PinIndex {
    records: std::collections::BTreeMap<String, PinRecord>,
}

impl PinIndex {
    fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }
    fn save(&self, path: &Path) -> Result<()> {
        let s =
            serde_json::to_string_pretty(self).map_err(|e| StorageError::Parse(e.to_string()))?;
        std::fs::write(path, s).map_err(|e| StorageError::Io(e.to_string()))
    }
}

// ---------------------------------------------------------------------------
// The manager: the kubo transport + the local ipfs dir (index + retrieved files).
// ---------------------------------------------------------------------------

/// Orchestrates the storage ops over the kubo transport + the local pin index.
pub struct StorageManager {
    transport: Box<dyn KuboTransport>,
    /// The app's ipfs working dir: `<data>/ipfs` (holds `pins.json` + `retrieved/`).
    dir: PathBuf,
    /// Injected clock (unix seconds) so tests are deterministic; production is wall-clock.
    now: fn() -> u64,
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// CORE-B-005: a `cid` is safe to use as a path component iff it is exactly one
/// `Normal` path component — no root/prefix (an absolute string discards the base
/// dir), no `..`/`.` traversal, and no embedded separator. This gates the
/// renderer-supplied cid before it is joined onto the retrieval dir.
fn is_safe_cid_component(cid: &str) -> bool {
    use std::path::Component;
    if cid.is_empty() {
        return false;
    }
    let mut comps = Path::new(cid).components();
    matches!(
        (comps.next(), comps.next()),
        (Some(Component::Normal(_)), None)
    )
}

impl StorageManager {
    /// Build a manager over an explicit ipfs dir + transport (production wall-clock).
    pub fn new(transport: Box<dyn KuboTransport>, dir: PathBuf) -> Self {
        StorageManager {
            transport,
            dir,
            now: unix_now,
        }
    }

    /// Test hook: a deterministic clock.
    #[cfg(test)]
    pub fn with_clock(mut self, now: fn() -> u64) -> Self {
        self.now = now;
        self
    }

    fn index_path(&self) -> PathBuf {
        self.dir.join("pins.json")
    }

    /// Add a local file to IPFS (does NOT pin it — pinning is the bonded step). Records size +
    /// added_at in the index so a later `list` can show it. Reads the file into memory (fine for
    /// documents/datasets; streaming very large models is a follow-on).
    pub fn add_file(&self, path: &Path) -> Result<AddOutcome> {
        let bytes = std::fs::read(path).map_err(|e| StorageError::Io(e.to_string()))?;
        let filename = path
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_else(|| "file".to_string());
        let out = self.transport.add(&filename, &bytes)?;
        std::fs::create_dir_all(&self.dir).map_err(|e| StorageError::Io(e.to_string()))?;
        let mut idx = PinIndex::load(&self.index_path());
        let rec = idx.records.entry(out.cid.clone()).or_default();
        rec.size_bytes = out.size_bytes;
        if rec.added_at == 0 {
            rec.added_at = (self.now)();
        }
        idx.save(&self.index_path())?;
        Ok(out)
    }

    /// Pin a CID, recording the SALT bond behind it. S2.1 records the bond as-passed; S2.2 makes
    /// it a real ceremony-gated on-chain bond.
    pub fn pin(&self, cid: &str, bond_salt: &str) -> Result<()> {
        self.transport.pin_add(cid)?;
        std::fs::create_dir_all(&self.dir).map_err(|e| StorageError::Io(e.to_string()))?;
        let mut idx = PinIndex::load(&self.index_path());
        let rec = idx.records.entry(cid.to_string()).or_default();
        rec.bond_salt = bond_salt.to_string();
        if rec.added_at == 0 {
            rec.added_at = (self.now)();
        }
        idx.save(&self.index_path())
    }

    /// Unpin a CID (releases the daemon pin; the index record is kept, marked by absence from the
    /// live set on the next `list`).
    pub fn unpin(&self, cid: &str) -> Result<()> {
        self.transport.pin_rm(cid)
    }

    /// Fetch a CID's raw bytes (no disk write) — used to compute the bond commitments (S2.2).
    pub fn cat(&self, cid: &str) -> Result<Vec<u8>> {
        self.transport.cat(cid)
    }

    /// Retrieve a CID's bytes to `<dir>/retrieved/<cid>` and return that path.
    ///
    /// CORE-B-005: `cid` is renderer-supplied and used as a path component, so it
    /// is validated FIRST to a single normal component — an absolute string (which
    /// discards the base dir) or one carrying `..`/separators is rejected before it
    /// reaches the filesystem, closing the path-traversal write sink.
    pub fn retrieve(&self, cid: &str) -> Result<PathBuf> {
        if !is_safe_cid_component(cid) {
            return Err(StorageError::Io(format!(
                "unsafe cid path component: {cid:?}"
            )));
        }
        let bytes = self.transport.cat(cid)?;
        let out_dir = self.dir.join("retrieved");
        std::fs::create_dir_all(&out_dir).map_err(|e| StorageError::Io(e.to_string()))?;
        let path = out_dir.join(cid);
        std::fs::write(&path, &bytes).map_err(|e| StorageError::Io(e.to_string()))?;
        Ok(path)
    }

    /// The pinning file store: the union of index records and the daemon's live pins. `pinState`
    /// is truth-from-the-daemon (`Pinned` iff live), so an index row the daemon no longer holds
    /// honestly reads `Unpinned`; a live pin not in the index is surfaced with empty metadata.
    pub fn list(&self) -> Result<Vec<PinRow>> {
        let idx = PinIndex::load(&self.index_path());
        let live: std::collections::BTreeSet<String> =
            self.transport.pin_ls()?.into_iter().collect();
        let mut rows: Vec<PinRow> = Vec::new();
        for (cid, rec) in &idx.records {
            rows.push(PinRow {
                cid: cid.clone(),
                size_bytes: rec.size_bytes,
                bond_salt: rec.bond_salt.clone(),
                pin_state: if live.contains(cid) {
                    PinState::Pinned
                } else {
                    PinState::Unpinned
                },
                added_at: rec.added_at,
            });
        }
        // Live pins the app didn't track (pinned out-of-band) — honestly surfaced, empty metadata.
        for cid in &live {
            if !idx.records.contains_key(cid) {
                rows.push(PinRow {
                    cid: cid.clone(),
                    size_bytes: 0,
                    bond_salt: String::new(),
                    pin_state: PinState::Pinned,
                    added_at: 0,
                });
            }
        }
        rows.sort_by(|a, b| b.added_at.cmp(&a.added_at).then(a.cid.cmp(&b.cid)));
        Ok(rows)
    }
}

// ---------------------------------------------------------------------------
// Tauri commands — stateless: each builds a manager from the app data dir.
// ---------------------------------------------------------------------------

fn build_manager<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> std::result::Result<StorageManager, String> {
    use tauri::Manager;
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("ipfs");
    Ok(StorageManager::new(
        Box::new(UreqKuboTransport::from_env()),
        dir,
    ))
}

/// **Command — storage_add.** Add a local file to IPFS; returns its CID + size.
#[tauri::command]
pub fn storage_add(app: tauri::AppHandle, path: String) -> std::result::Result<AddOutcome, String> {
    build_manager(&app)?
        .add_file(Path::new(&path))
        .map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// CX-S2.2 — the ceremony-gated IPFSIncentivesV3 model bond (@rule8 · money path).
// ---------------------------------------------------------------------------

use sha3::{Digest as _, Keccak256};

/// MIN_MODEL_BOND — 55 SALT (the #170 params). Wei = 55 × 1e18.
const MIN_MODEL_BOND_WEI: u128 = 55_000_000_000_000_000_000;
/// Explicit gas for `registerModel` (a calldata tx MUST carry explicit gas).
const REGISTER_MODEL_GAS: u64 = 300_000;

fn keccak256(bytes: &[u8]) -> [u8; 32] {
    let mut h = Keccak256::new();
    h.update(bytes);
    h.finalize().into()
}

/// The three 32-byte commitments the bond registers, from the file's bytes.
pub struct BondCommitments {
    pub comm_d: [u8; 32],
    pub data_hash: [u8; 32],
    pub data_commit: [u8; 32],
}

/// Compute (commD, dataHash, dataCommit) EXACTLY as the chain's canonical `citrate-commd`
/// (Poseidon-BN254 Merkle / sponge + keccak). Byte-exactness is pinned by a storage_tests vector
/// check against citrate-commd's frozen `commd_frozen_v1.rs` — a mismatch would register a
/// slashable bond, so this is a hard gate, not a nicety.
pub fn bond_commitments(bytes: &[u8]) -> BondCommitments {
    BondCommitments {
        comm_d: citrate_commd::compute_comm_d(bytes),
        data_hash: keccak256(bytes),
        data_commit: citrate_commd::compute_data_commit(bytes),
    }
}

/// The on-chain `bytes32 cid` = keccak256(dataUri). CONVENTION from the IPFSIncentivesV3 tests
/// (the contract treats cid as an opaque key); reconcile with the pinner cid when the seal/challenge
/// path activates (0x0130 + the trusted-setup ceremony — not live yet).
pub fn bond_cid(data_uri: &str) -> [u8; 32] {
    keccak256(data_uri.as_bytes())
}

/// ABI-encode `registerModel(bytes32 cid, bytes32 commD, bytes32 dataHash, bytes32 dataCommit,
/// string dataUri)`: 4 static words + a string-offset word (0xa0), then the string tail.
pub fn register_model_calldata(cid: [u8; 32], c: &BondCommitments, data_uri: &str) -> Vec<u8> {
    let selector = &keccak256(b"registerModel(bytes32,bytes32,bytes32,bytes32,string)")[..4];
    let mut out = Vec::with_capacity(4 + 32 * 6 + 64);
    out.extend_from_slice(selector);
    out.extend_from_slice(&cid);
    out.extend_from_slice(&c.comm_d);
    out.extend_from_slice(&c.data_hash);
    out.extend_from_slice(&c.data_commit);
    let mut offset = [0u8; 32]; // string offset = 5 head words × 32 = 160 = 0xa0
    offset[31] = 0xa0;
    out.extend_from_slice(&offset);
    let uri = data_uri.as_bytes();
    let mut len = [0u8; 32];
    len[24..].copy_from_slice(&(uri.len() as u64).to_be_bytes());
    out.extend_from_slice(&len);
    out.extend_from_slice(uri);
    let pad = (32 - uri.len() % 32) % 32;
    out.extend(std::iter::repeat_n(0u8, pad));
    out
}

/// The pending-ceremony tx JSON (mirrors node.rs::encode_activate_json): from the member EOA, to
/// IPFSIncentivesV3, value = the bond, data = registerModel calldata.
fn encode_bond_tx_json(from: &str, calldata: &[u8]) -> String {
    serde_json::json!({
        "from": from,
        // REROLL-SENSITIVE: sourced from the pinned address book (addresses.rs), never hardcoded —
        // a genesis reroll is an address-book update, not a code change.
        "to": crate::addresses::ipfs_incentives_v3(),
        "value": format!("0x{MIN_MODEL_BOND_WEI:x}"),
        "data": format!("0x{}", hex::encode(calldata)),
        "gas": format!("0x{REGISTER_MODEL_GAS:x}"),
        "chainId": format!("0x{:x}", 40204u64),
    })
    .to_string()
}

/// **Command — storage_pin.** Pin a CID locally AND place the network storage bond: fetch the
/// bytes, compute the canonical commitments, and submit a `registerModel` tx as a PENDING
/// SignatureCeremony (Rule 3 — the human approves via the signing flow; nothing signs here). The
/// bond appears in the ceremony pending list for approval. Returns `()` (the frozen DTO shape); the
/// ceremony is surfaced by the existing signing surface.
#[tauri::command]
pub fn storage_pin(
    app: tauri::AppHandle,
    custody: tauri::State<'_, crate::custody::CustodyState>,
    ceremony: tauri::State<'_, crate::ceremony::CeremonyState>,
    cid: String,
    bond_salt: String,
) -> std::result::Result<(), String> {
    let mgr = build_manager(&app)?;
    // Keep the file on THIS node (local pin), recording the bond marker.
    mgr.pin(&cid, &bond_salt).map_err(|e| e.to_string())?;
    // Fetch the bytes + compute the canonical commitments for the on-chain bond.
    let bytes = mgr.cat(&cid).map_err(|e| e.to_string())?;
    let data_uri = format!("ipfs://{cid}");
    let commits = bond_commitments(&bytes);
    let calldata = register_model_calldata(bond_cid(&data_uri), &commits, &data_uri);
    let wallet = crate::wallet::address_auto_unlocked(&custody.0).map_err(|e| e.to_string())?;
    let raw = encode_bond_tx_json(&wallet.address, &calldata);
    let intent = crate::ceremony::SignatureIntent {
        origin: "local-user".to_string(),
        kind: crate::ceremony::IntentKind::Transaction,
        chain_id: 40204,
        raw,
    };
    // Submit the PENDING ceremony (the user approves + broadcasts via the signing surface).
    ceremony.0.request(intent);
    Ok(())
}

/// **Command — storage_list.** The pinning file store (index ∪ live pins).
#[tauri::command]
pub fn storage_list(app: tauri::AppHandle) -> std::result::Result<Vec<PinRow>, String> {
    build_manager(&app)?.list().map_err(|e| e.to_string())
}

/// **Command — storage_retrieve.** Fetch a CID to a local file; returns its path.
#[tauri::command]
pub fn storage_retrieve(app: tauri::AppHandle, cid: String) -> std::result::Result<String, String> {
    build_manager(&app)?
        .retrieve(&cid)
        .map(|p| p.to_string_lossy().to_string())
        .map_err(|e| e.to_string())
}

/// **Command — storage_unpin.** Release a CID's pin.
#[tauri::command]
pub fn storage_unpin(app: tauri::AppHandle, cid: String) -> std::result::Result<(), String> {
    build_manager(&app)?.unpin(&cid).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    include!("storage_tests.rs");
}
