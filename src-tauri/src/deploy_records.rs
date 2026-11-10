//! CORE-A3 (0.5.3) — **durable deploy records**, encrypted per project.
//!
//! Before this, what the app knew about a deploy lived only in memory: the D-4 gate store keeps at
//! most [`crate::deploy_gate::MAX_GATE_RECORDS`] records and forgets them on restart. A deploy
//! record keeps, for one confirmed contract creation of one hello-mint project:
//!
//! | field | data source |
//! |---|---|
//! | address, block, status | `eth_getTransactionReceipt` on 40204 (the creation must have succeeded) |
//! | deployer, init code | `eth_getTransactionByHash` on 40204 (`to` must be empty: a creation) |
//! | init-code hash | keccak256 of that init code ([`crate::deploy_gate::initcode_hash`]) |
//! | contract name, ABI, constructor arguments | the project's forge artifact (`contracts/out/Token.sol/<Contract>.json`): its creation bytecode must be a prefix of the init code on chain; the rest is the constructor arguments |
//! | compiler settings, gate record | the D-4 gate store: the READY record for exactly this init code |
//! | standard-JSON input | `forge verify-contract --show-standard-json-input` in the project (as `postdeploy_verify` uses); when forge cannot produce it the record says why |
//!
//! Nothing is invented: a field the source does not give is absent with the reason.
//!
//! **Storage.** One sealed file per project, `<app data>/deploy-records/<project id>.sealed`, where
//! the project id is a SHA-256 of the project's canonical folder path. The file is AES-256-GCM under
//! a 32-byte key in the OS keyring (account [`KEY_ACCOUNT`], the legacy service like the node and
//! memory keys; listed by "delete my local data" and backed up by the recovery kit). The header
//! and the project id are the associated data, so a file copied to another project's name, or
//! edited, does not open. Files are owner-only (`0600` in a `0700` folder) and replaced atomically.
//! The key is minted only while no sealed file exists: if files exist and the key is gone, every
//! read and save refuses (restore the key from a recovery kit) instead of minting a new key over
//! data it can no longer open.
//!
//! Rule 3: nothing here signs, holds a wallet key or sends a transaction. Chain reads only.
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use aes_gcm::aead::{Aead, AeadCore, KeyInit, OsRng, Payload};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::Digest as _;
use zeroize::Zeroizing;

use crate::custody::Keyring;
use crate::deploy_gate::{CompilerSettings, GateRecord, GateStore, Verdict};
use crate::postdeploy::HelloMintProject;

/// The keyring account (legacy `ai.citrate.core` service) holding the records key.
pub const KEY_ACCOUNT: &str = "deploy-records-key";
/// The app-data folder holding the sealed files.
pub const RECORDS_DIR: &str = "deploy-records";
/// The sealed-file extension.
const EXT: &str = "sealed";
/// File magic and format version.
const MAGIC: &[u8; 8] = b"CITRDPL\0";
const FORMAT_VERSION: u8 = 1;
const NONCE_LEN: usize = 12;
const HEADER_LEN: usize = 8 + 1 + NONCE_LEN;
const TAG_LEN: usize = 16;
const KEY_LEN: usize = 32;
/// Domain of the project id hash.
const PROJECT_ID_DOMAIN: &[u8] = b"citrate.deploy-records.project.v1";
/// How many records one project keeps (a save beyond it is refused, never an eviction).
pub const MAX_RECORDS_PER_PROJECT: usize = 500;
/// The largest standard-JSON input kept in a record.
pub const MAX_STANDARD_JSON_BYTES: usize = 16 * 1024 * 1024;
/// The largest sealed file read or written.
pub const MAX_FILE_BYTES: usize = 64 * 1024 * 1024;
/// The largest forge artifact read.
const MAX_ARTIFACT_BYTES: u64 = 32 * 1024 * 1024;

/// One confirmed contract creation of a project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeployRecord {
    pub chain_id: u64,
    /// The created contract, lowercase `0x` hex.
    pub address: String,
    pub tx_hash: String,
    pub block_number: u64,
    /// The creation transaction's sender, lowercase `0x` hex.
    pub deployer: String,
    /// `0x` + keccak256 of the init code on chain.
    pub initcode_hash: String,
    pub contract_name: String,
    /// The forge artifact's ABI, as it is.
    pub abi: serde_json::Value,
    /// The init code past the artifact's creation bytecode (`0x` hex; `0x` when none).
    pub constructor_args_hex: String,
    pub compiler: CompilerSettings,
    /// The solc standard-JSON input, when forge produced it.
    pub standard_json_input: Option<String>,
    /// Why there is no standard-JSON input, when there is none.
    pub standard_json_note: Option<String>,
    /// The D-4 gate record this init code was READY under.
    pub gate: GateRecord,
    pub recorded_at_ms: u64,
}

/// A record without its two large fields, as the list returns it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeployRecordSummary {
    pub chain_id: u64,
    pub address: String,
    pub tx_hash: String,
    pub block_number: u64,
    pub deployer: String,
    pub initcode_hash: String,
    pub contract_name: String,
    pub solc_version: String,
    pub abi_entries: usize,
    pub standard_json_bytes: Option<usize>,
    pub standard_json_note: Option<String>,
    pub gate_verdict: Verdict,
    pub gate_evaluated_at_ms: u64,
    pub recorded_at_ms: u64,
}

impl DeployRecord {
    pub fn summary(&self) -> DeployRecordSummary {
        DeployRecordSummary {
            chain_id: self.chain_id,
            address: self.address.clone(),
            tx_hash: self.tx_hash.clone(),
            block_number: self.block_number,
            deployer: self.deployer.clone(),
            initcode_hash: self.initcode_hash.clone(),
            contract_name: self.contract_name.clone(),
            solc_version: self.compiler.solc_version.clone(),
            abi_entries: self.abi.as_array().map_or(0, Vec::len),
            standard_json_bytes: self.standard_json_input.as_ref().map(String::len),
            standard_json_note: self.standard_json_note.clone(),
            gate_verdict: self.gate.verdict,
            gate_evaluated_at_ms: self.gate.evaluated_at_ms,
            recorded_at_ms: self.recorded_at_ms,
        }
    }
}

/// What a save returns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeployRecordSaved {
    pub record: DeployRecordSummary,
    /// `true` when this transaction was already recorded (nothing was written).
    pub already_saved: bool,
    /// How many records the project holds now.
    pub count: usize,
}

/// The plaintext of one sealed file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProjectFile {
    project_id: String,
    records: Vec<DeployRecord>,
}

// ------------------------------------------------------------------------ project id

/// The project id: hex SHA-256(domain ‖ canonical project folder path), first 32 hex chars.
pub fn project_id(root: &Path) -> String {
    let mut h = sha2::Sha256::new();
    h.update(PROJECT_ID_DOMAIN);
    h.update(root.as_os_str().as_encoded_bytes());
    hex::encode(h.finalize())[..32].to_string()
}

// ------------------------------------------------------------------------ assembly (pure)

/// The deployed artifact's creation bytecode, ABI and name, from the project's forge output.
pub(crate) struct Artifact {
    pub creation: Vec<u8>,
    pub abi: serde_json::Value,
}

/// Read `contracts/out/Token.sol/<Contract>.json`. Symlinks and oversized files are refused.
pub(crate) fn read_artifact(p: &HelloMintProject) -> Result<Artifact, String> {
    let path = p
        .contracts_dir
        .join("out/Token.sol")
        .join(format!("{}.json", p.contract_name));
    let meta = std::fs::symlink_metadata(&path)
        .map_err(|_| "the compiled artifact was not found (run forge build)".to_string())?;
    if !meta.is_file() || meta.len() > MAX_ARTIFACT_BYTES {
        return Err("the compiled artifact is not a forge artifact file".to_string());
    }
    let art: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(&path).map_err(|e| format!("artifact: {}", e.kind()))?,
    )
    .map_err(|_| "the compiled artifact is not forge JSON".to_string())?;
    let creation = art
        .pointer("/bytecode/object")
        .and_then(|v| v.as_str())
        .and_then(|s| hex::decode(s.trim_start_matches("0x")).ok())
        .filter(|b| !b.is_empty())
        .ok_or("the artifact has no creation bytecode")?;
    let abi = art
        .get("abi")
        .filter(|a| a.is_array())
        .cloned()
        .ok_or("the artifact has no ABI")?;
    Ok(Artifact { creation, abi })
}

/// What the chain says about the creation transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ChainCreation {
    pub tx_hash: String,
    pub block_number: u64,
    pub address: String,
    pub deployer: String,
    pub initcode: Vec<u8>,
}

fn hex_u64(v: &serde_json::Value) -> Option<u64> {
    v.as_str()
        .and_then(|s| s.strip_prefix("0x"))
        .and_then(|h| u64::from_str_radix(h, 16).ok())
}

/// Read the creation from 40204: the receipt must show a successful creation, and the transaction
/// must be a creation (no `to`) whose input is the init code.
pub(crate) fn read_creation<T: crate::rpc::RpcTransport>(
    client: &crate::rpc::RpcClient<T>,
    tx_hash: &str,
) -> Result<ChainCreation, String> {
    let hash = crate::postdeploy::parse_tx_hash(tx_hash)?;
    let receipt = crate::contract_reader::rpc_raw(
        client,
        "eth_getTransactionReceipt",
        serde_json::json!([hash]),
    )?;
    let r = crate::postdeploy::parse_receipt(&receipt)?
        .ok_or("the deploy is not confirmed yet; save its record once it is")?;
    if r.status != Some(1) {
        return Err("the deploy reverted; nothing was deployed, so there is no record".to_string());
    }
    let address = r
        .contract_address
        .ok_or("that transaction did not create a contract")?;
    let tx = crate::contract_reader::rpc_raw(
        client,
        "eth_getTransactionByHash",
        serde_json::json!([hash]),
    )?;
    if tx.is_null() {
        return Err("the node has the receipt but not the transaction; try again".to_string());
    }
    if tx.get("to").is_some_and(|t| !t.is_null()) {
        return Err("that transaction is a call, not a contract creation".to_string());
    }
    let deployer = tx
        .get("from")
        .and_then(|f| f.as_str())
        .ok_or("the transaction has no sender")
        .and_then(|f| {
            crate::contract_reader::normalize_address(f).map_err(|_| "the sender is malformed")
        })?;
    let input = tx
        .get("input")
        .and_then(|i| i.as_str())
        .and_then(|s| s.strip_prefix("0x"))
        .and_then(|s| hex::decode(s).ok())
        .filter(|b| !b.is_empty())
        .ok_or("the transaction carries no init code")?;
    let block_number = tx
        .get("blockNumber")
        .and_then(hex_u64)
        .unwrap_or(r.block_number);
    Ok(ChainCreation {
        tx_hash: hash,
        block_number,
        address,
        deployer,
        initcode: input,
    })
}

/// Assemble the record from its sources. `standard_json` is forge's answer for this address.
pub(crate) fn assemble(
    p: &HelloMintProject,
    chain: ChainCreation,
    artifact: Artifact,
    gate: &GateStore,
    standard_json: Result<String, String>,
    now_ms: u64,
) -> Result<DeployRecord, String> {
    if !chain.initcode.starts_with(&artifact.creation) {
        return Err(
            "the code this transaction deployed is not this project's compiled contract"
                .to_string(),
        );
    }
    let initcode_hash = crate::deploy_gate::initcode_hash(&chain.initcode);
    let rec = gate.get(&initcode_hash).ok_or(
        "this build has no deploy-gate record in this session (the app restarted since the \
         deploy); run the gate again on the same build, then save the record",
    )?;
    if rec.verdict != Verdict::Ready {
        return Err("the gate's latest evaluation of this build is NOT READY".to_string());
    }
    let (standard_json_input, standard_json_note) = match standard_json {
        Ok(s) if s.len() > MAX_STANDARD_JSON_BYTES => (
            None,
            Some(format!(
                "the standard-JSON input is larger than {} MiB and was not kept",
                MAX_STANDARD_JSON_BYTES / (1024 * 1024)
            )),
        ),
        Ok(s) if serde_json::from_str::<serde_json::Value>(&s).is_err() => (
            None,
            Some("forge printed a standard-JSON input that is not JSON".to_string()),
        ),
        Ok(s) => (Some(s), None),
        Err(e) => (None, Some(e)),
    };
    let args = &chain.initcode[artifact.creation.len()..];
    Ok(DeployRecord {
        chain_id: crate::rpc::CITRATE_CHAIN_ID,
        address: chain.address,
        tx_hash: chain.tx_hash,
        block_number: chain.block_number,
        deployer: chain.deployer,
        initcode_hash,
        contract_name: p.contract_name.clone(),
        abi: artifact.abi,
        constructor_args_hex: format!("0x{}", hex::encode(args)),
        compiler: rec.compiler.clone(),
        standard_json_input,
        standard_json_note,
        gate: rec,
        recorded_at_ms: now_ms,
    })
}

// ------------------------------------------------------------------------ the sealed store

/// The sealed per-project store under one folder, keyed from `keyring`.
pub struct DeployRecordStore<'a> {
    dir: PathBuf,
    keyring: &'a dyn Keyring,
}

/// Serializes load-modify-save across commands in this process.
static STORE_LOCK: Mutex<()> = Mutex::new(());

impl<'a> DeployRecordStore<'a> {
    pub fn new(dir: impl Into<PathBuf>, keyring: &'a dyn Keyring) -> Self {
        Self {
            dir: dir.into(),
            keyring,
        }
    }

    fn file_for(&self, project_id: &str) -> PathBuf {
        self.dir.join(format!("{project_id}.{EXT}"))
    }

    /// Whether any sealed file exists in the folder.
    fn any_sealed_file(&self) -> Result<bool, String> {
        match std::fs::read_dir(&self.dir) {
            Ok(entries) => Ok(entries.filter_map(Result::ok).any(|e| {
                e.path()
                    .extension()
                    .is_some_and(|x| x == std::ffi::OsStr::new(EXT))
            })),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(format!(
                "the deploy records folder cannot be read: {}",
                e.kind()
            )),
        }
    }

    /// The records key. Minted only when `mint` and no sealed file exists yet.
    fn key(&self, mint: bool) -> Result<Option<Zeroizing<Vec<u8>>>, String> {
        let unreachable =
            || "the keychain is not reachable; deploy records stay closed".to_string();
        match self.keyring.get(KEY_ACCOUNT).map_err(|_| unreachable())? {
            Some(k) if k.len() == KEY_LEN => Ok(Some(Zeroizing::new(k))),
            Some(_) => Err("the deploy records key in the keychain is malformed".to_string()),
            None => {
                if self.any_sealed_file()? {
                    return Err(
                        "deploy records exist but their key is not in the keychain; \
                                restore it from your recovery kit"
                            .to_string(),
                    );
                }
                if !mint {
                    return Ok(None);
                }
                let mut k = Zeroizing::new(vec![0u8; KEY_LEN]);
                rand::thread_rng().fill_bytes(&mut k);
                self.keyring
                    .set(KEY_ACCOUNT, &k)
                    .map_err(|_| unreachable())?;
                Ok(Some(k))
            }
        }
    }

    fn aad(nonce_header: &[u8], project_id: &str) -> Vec<u8> {
        let mut aad = nonce_header.to_vec();
        aad.extend_from_slice(project_id.as_bytes());
        aad
    }

    fn seal(key: &[u8], project_id: &str, file: &ProjectFile) -> Result<Vec<u8>, String> {
        let plain = Zeroizing::new(serde_json::to_vec(file).map_err(|e| e.to_string())?);
        // A fresh random 96-bit nonce from the OS RNG for every seal.
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        let mut head = MAGIC.to_vec();
        head.push(FORMAT_VERSION);
        head.extend_from_slice(&nonce);
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
        let ct = cipher
            .encrypt(
                &nonce,
                Payload {
                    msg: &plain,
                    aad: &Self::aad(&head, project_id),
                },
            )
            .map_err(|_| "the deploy records could not be sealed".to_string())?;
        let mut out = head;
        out.extend_from_slice(&ct);
        if out.len() > MAX_FILE_BYTES {
            return Err("this project's deploy records would exceed the file limit".to_string());
        }
        Ok(out)
    }

    fn open(key: &[u8], project_id: &str, bytes: &[u8]) -> Result<ProjectFile, String> {
        let damaged = || "this project's deploy records file is damaged or was moved".to_string();
        if bytes.len() < HEADER_LEN + TAG_LEN || &bytes[..8] != MAGIC {
            return Err(damaged());
        }
        if bytes[8] != FORMAT_VERSION {
            return Err("this deploy records file was written by a newer version".to_string());
        }
        let head = &bytes[..HEADER_LEN];
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
        let plain = Zeroizing::new(
            cipher
                .decrypt(
                    Nonce::from_slice(&bytes[9..HEADER_LEN]),
                    Payload {
                        msg: &bytes[HEADER_LEN..],
                        aad: &Self::aad(head, project_id),
                    },
                )
                .map_err(|_| damaged())?,
        );
        let file: ProjectFile = serde_json::from_slice(&plain).map_err(|_| damaged())?;
        if file.project_id != project_id {
            return Err(damaged());
        }
        Ok(file)
    }

    fn read_file(&self, project_id: &str) -> Result<Option<Vec<u8>>, String> {
        let path = self.file_for(project_id);
        match std::fs::symlink_metadata(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(format!(
                "the deploy records file cannot be read: {}",
                e.kind()
            )),
            Ok(m) if !m.is_file() => Err("the deploy records file is not a plain file".to_string()),
            Ok(m) if m.len() as usize > MAX_FILE_BYTES => {
                Err("this project's deploy records file is larger than the limit".to_string())
            }
            Ok(_) => std::fs::read(&path)
                .map(Some)
                .map_err(|e| format!("the deploy records file cannot be read: {}", e.kind())),
        }
    }

    fn load_locked(&self, project_id: &str) -> Result<Vec<DeployRecord>, String> {
        let Some(bytes) = self.read_file(project_id)? else {
            // Still check the key, so a lost key is reported even for a project with no file.
            self.key(false)?;
            return Ok(Vec::new());
        };
        let key = self.key(false)?.ok_or(
            "deploy records exist but their key is not in the keychain; restore it from your \
             recovery kit",
        )?;
        Ok(Self::open(&key, project_id, &bytes)?.records)
    }

    /// Every record of the project, oldest first.
    pub fn load(&self, project_id: &str) -> Result<Vec<DeployRecord>, String> {
        let _g = STORE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        self.load_locked(project_id)
    }

    /// Add `rec` to the project. A transaction already recorded is not written again.
    pub fn save(&self, project_id: &str, rec: DeployRecord) -> Result<DeployRecordSaved, String> {
        let _g = STORE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut records = self.load_locked(project_id)?;
        if let Some(existing) = records.iter().find(|r| r.tx_hash == rec.tx_hash) {
            return Ok(DeployRecordSaved {
                record: existing.summary(),
                already_saved: true,
                count: records.len(),
            });
        }
        if records.len() >= MAX_RECORDS_PER_PROJECT {
            return Err(format!(
                "this project already holds {MAX_RECORDS_PER_PROJECT} deploy records"
            ));
        }
        let key = self
            .key(true)?
            .ok_or("internal: the deploy records key was not minted")?;
        let summary = rec.summary();
        records.push(rec);
        let file = ProjectFile {
            project_id: project_id.to_string(),
            records,
        };
        let sealed = Self::seal(&key, project_id, &file)?;
        let path = self.file_for(project_id);
        let staged = crate::atomic_file::stage_secret(&path, &sealed)
            .map_err(|e| format!("the deploy record could not be written: {}", e.kind()))?;
        crate::atomic_file::commit_staged(&staged, &path)
            .map_err(|e| format!("the deploy record could not be written: {}", e.kind()))?;
        Ok(DeployRecordSaved {
            record: summary,
            already_saved: false,
            count: file.records.len(),
        })
    }
}

/// Read the creation, the artifact and the gate record, and save the record. `standard_json` is
/// called with the created address only once the creation checks out.
pub(crate) fn save_deploy_record<T: crate::rpc::RpcTransport>(
    store: &DeployRecordStore<'_>,
    client: &crate::rpc::RpcClient<T>,
    p: &HelloMintProject,
    gate: &GateStore,
    tx_hash: &str,
    standard_json: impl FnOnce(&str) -> Result<String, String>,
    now_ms: u64,
) -> Result<DeployRecordSaved, String> {
    let pid = project_id(&p.root);
    let chain = read_creation(client, tx_hash)?;
    if let Some(r) = store
        .load(&pid)?
        .iter()
        .find(|r| r.tx_hash == chain.tx_hash)
    {
        return store.save(&pid, r.clone());
    }
    let artifact = read_artifact(p)?;
    let sj = standard_json(&chain.address);
    let rec = assemble(p, chain, artifact, gate, sj, now_ms)?;
    store.save(&pid, rec)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default()
}

fn records_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    use tauri::Manager;
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join(RECORDS_DIR))
}

/// **deploy_record_save** — keep a durable, encrypted record of a confirmed hello-mint deploy.
/// Data sources: `eth_getTransactionReceipt` and `eth_getTransactionByHash` on the 40204 RPC, the
/// project's forge artifact, the in-memory D-4 gate store (`DeployGateState`) and `forge
/// verify-contract --show-standard-json-input`. Writes `<app data>/deploy-records/<id>.sealed`
/// (key in the OS keyring). Signs nothing.
#[tauri::command]
pub async fn deploy_record_save(
    app_h: tauri::AppHandle,
    project_dir: String,
    tx_hash: String,
) -> std::result::Result<DeployRecordSaved, String> {
    crate::blocking::off_main(move || {
        let gate = tauri::Manager::try_state::<crate::deploy_gate::DeployGateState>(&app_h)
            .ok_or_else(|| "internal: managed state unavailable".to_string())?;
        let p = crate::postdeploy::open_project(Path::new(&project_dir))?;
        let keyring = crate::custody::OsKeyring::legacy();
        let store = DeployRecordStore::new(records_dir(&app_h)?, &keyring);
        save_deploy_record(
            &store,
            &crate::rpc::RpcClient::citrate(),
            &p,
            &gate.0,
            &tx_hash,
            |addr| crate::postdeploy::forge_standard_json(&p, addr),
            now_ms(),
        )
    })
    .await
}

/// **deploy_records_list** — the project's saved deploy records, oldest first, without the ABI
/// and standard-JSON bodies. Data source: the project's sealed file in
/// `<app data>/deploy-records/`, opened with the key in the OS keyring. Read-only.
#[tauri::command]
pub async fn deploy_records_list(
    app_h: tauri::AppHandle,
    project_dir: String,
) -> std::result::Result<Vec<DeployRecordSummary>, String> {
    crate::blocking::off_main(move || {
        let p = crate::postdeploy::open_project(Path::new(&project_dir))?;
        let keyring = crate::custody::OsKeyring::legacy();
        let store = DeployRecordStore::new(records_dir(&app_h)?, &keyring);
        Ok(store
            .load(&project_id(&p.root))?
            .iter()
            .map(DeployRecord::summary)
            .collect())
    })
    .await
}

/// **deploy_record_get** — one saved record in full (ABI, constructor arguments, compiler
/// settings, standard-JSON input, gate record), by contract address. Data source: the project's
/// sealed file in `<app data>/deploy-records/`. Read-only.
#[tauri::command]
pub async fn deploy_record_get(
    app_h: tauri::AppHandle,
    project_dir: String,
    address: String,
) -> std::result::Result<Option<DeployRecord>, String> {
    crate::blocking::off_main(move || {
        let p = crate::postdeploy::open_project(Path::new(&project_dir))?;
        let address = crate::contract_reader::normalize_address(&address)?;
        let keyring = crate::custody::OsKeyring::legacy();
        let store = DeployRecordStore::new(records_dir(&app_h)?, &keyring);
        Ok(store
            .load(&project_id(&p.root))?
            .into_iter()
            .rev()
            .find(|r| r.address == address))
    })
    .await
}

#[cfg(test)]
mod tests {
    include!("deploy_records_tests.rs");
}
