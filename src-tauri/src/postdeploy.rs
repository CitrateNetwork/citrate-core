//! HUP-S6.6 (US-6.1 tail) — **after the deploy**: the steps a hello-mint project takes once its
//! contract creation is confirmed on chain 40204.
//!
//! 1. **Receipt** — `eth_getTransactionReceipt` on 40204 gives the deployed address (a reverted
//!    creation deployed nothing, and says so).
//! 2. **Verify** — the project's standard-JSON compiler input (from `forge verify-contract
//!    --show-standard-json-input`, run in the project's `contracts/`) is submitted to CitrateScan's
//!    public verifier (`POST {EXPLORER_BASE}/api/verify`; source: citrate-explorer
//!    `src/app/api/verify/route.ts`). The explorer recompiles and diffs; its verdict is reported
//!    as it is, including rate limits and failures. The chain's own `citrate_verifyContract` RPC
//!    is operator-only and is not used.
//! 3. **Switch the site** — the page's `app/.env.local` is rewritten to `VITE_TARGET=citrate` and
//!    the deployed address, after checking that the address holds code on 40204.
//! 4. **Pin to IPFS** — the built page (`app/dist`, from `npm run build`) is added as one folder
//!    to the app's bundled IPFS daemon (the kubo API that `storage.rs` already uses) and pinned.
//!    If IPFS is not running, nothing is pinned and the member is told how to start it.
//! 5. **Vercel export** — a deployable project folder (`vercel-export/`: the page sources, a
//!    `vercel.json` and a `.env.production` with the 40204 settings). The member deploys it with
//!    their own Vercel account; Citrate never signs in to Vercel or deploys for them.
//!
//! Nothing here holds a key or signs (Rule 3). The project directory is chosen by the member;
//! it must carry the hello-mint `citrate-template.lock.json`, and the only files written are
//! `app/.env.local` and the `vercel-export/` folder (which is only ever replaced when Citrate
//! wrote it).
use std::path::{Path, PathBuf};

use serde::Serialize;
use sha3::{Digest as _, Keccak256};

use crate::contract_reader::{checksum_address, normalize_address, ExplorerHttp};

/// The lock file every rendered template writes (templates/renderer `LOCK_FILE`).
pub const LOCK_FILE: &str = "citrate-template.lock.json";
/// The Vercel export folder, inside the project.
pub const EXPORT_DIR: &str = "vercel-export";
/// Marks an export folder as written by Citrate (so it may be replaced).
const EXPORT_MARKER: &str = ".citrate-vercel-export";
/// The IPFS folder name the site is added under (the CID is this folder's).
const SITE_ROOT: &str = "site";
/// Bounds on what a site pin uploads. Conservative defaults, pending owner sign-off.
const MAX_SITE_FILES: usize = 2_000;
const MAX_SITE_BYTES: u64 = 100 * 1024 * 1024;
/// Bounds on what a Vercel export copies (sources only; dependencies are installed by Vercel).
/// Conservative default, pending owner sign-off.
const MAX_EXPORT_FILES: usize = 5_000;
/// The bundled kubo daemon's local gateway (ipfs.rs moves it to this port).
const LOCAL_GATEWAY: &str = "http://127.0.0.1:48080";
/// A public gateway. It can serve the site only while some node that has it is reachable.
/// Placeholder pending owner sign-off (the owner may prefer a Citrate-run gateway).
const PUBLIC_GATEWAY: &str = "https://ipfs.io";
/// Folders and files of `app/` that a Vercel export leaves out.
const EXPORT_SKIP: [&str; 5] = ["node_modules", "dist", ".vercel", ".env.local", ".git"];

// ---------------------------------------------------------------------------
// The project
// ---------------------------------------------------------------------------

/// A rendered hello-mint project on disk.
#[derive(Debug, Clone)]
pub struct HelloMintProject {
    pub root: PathBuf,
    pub app_dir: PathBuf,
    pub contracts_dir: PathBuf,
    /// The contract's Solidity name (the template's `contract` parameter).
    pub contract_name: String,
    /// The solc version the template pins.
    pub solc: String,
}

fn is_identifier(s: &str) -> bool {
    let mut c = s.chars();
    matches!(c.next(), Some(f) if f.is_ascii_alphabetic())
        && s.len() <= 64
        && s.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

/// `0.8.36` or `0.8.36+commit.abcdef12`.
pub(crate) fn is_compiler_version(s: &str) -> bool {
    let (semver, commit) = match s.split_once("+commit.") {
        Some((a, b)) => (a, Some(b)),
        None => (s, None),
    };
    let parts: Vec<&str> = semver.split('.').collect();
    let semver_ok = parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.len() <= 2 && p.chars().all(|c| c.is_ascii_digit()));
    let commit_ok = commit
        .is_none_or(|c| (6..=40).contains(&c.len()) && c.chars().all(|ch| ch.is_ascii_hexdigit()));
    semver_ok && commit_ok
}

/// Open a hello-mint project: the root lock file must name the `hello-mint` template.
pub fn open_project(dir: &Path) -> Result<HelloMintProject, String> {
    let root = dir
        .canonicalize()
        .map_err(|_| "the project folder does not exist".to_string())?;
    let lock_text = std::fs::read_to_string(root.join(LOCK_FILE)).map_err(|_| {
        format!("this folder has no {LOCK_FILE}; open the hello-mint project folder")
    })?;
    let lock: serde_json::Value =
        serde_json::from_str(&lock_text).map_err(|_| format!("{LOCK_FILE} is not valid JSON"))?;
    if lock.get("template").and_then(|v| v.as_str()) != Some("hello-mint") {
        return Err("this is not a hello-mint project".to_string());
    }
    let contract_name = lock
        .get("params")
        .and_then(|p| p.get("contract"))
        .and_then(|v| v.as_str())
        .filter(|s| is_identifier(s))
        .ok_or_else(|| format!("{LOCK_FILE} has no valid contract name"))?
        .to_string();
    let solc = lock
        .get("solc")
        .and_then(|v| v.as_str())
        .filter(|s| is_compiler_version(s))
        .ok_or_else(|| format!("{LOCK_FILE} has no valid solc version"))?
        .to_string();
    let app_dir = root.join("app");
    let contracts_dir = root.join("contracts");
    if !app_dir.is_dir() || !contracts_dir.is_dir() {
        return Err("the project is missing its app/ or contracts/ folder".to_string());
    }
    Ok(HelloMintProject {
        root,
        app_dir,
        contracts_dir,
        contract_name,
        solc,
    })
}

// ---------------------------------------------------------------------------
// Site configuration
// ---------------------------------------------------------------------------

const SITE_KEYS: [&str; 3] = ["VITE_TARGET", "VITE_CONTRACT_ADDRESS", "VITE_FORK_RPC_URL"];

fn env_key(line: &str) -> Option<&str> {
    let t = line.trim_start();
    if t.starts_with('#') {
        return None;
    }
    t.split_once('=').map(|(k, _)| k.trim())
}

fn read_env(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

fn env_value<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    text.lines()
        .filter(|l| env_key(l) == Some(key))
        .filter_map(|l| l.split_once('=').map(|(_, v)| v.trim()))
        .next_back()
}

/// The 40204 settings block written into the page's env files.
fn citrate_env_block(address_lower: &str) -> String {
    format!(
        "# Set by Citrate after the deploy (HUP-S6.6): the page targets chain 40204.\nVITE_TARGET=citrate\nVITE_CONTRACT_ADDRESS={}\n",
        checksum_address(address_lower)
    )
}

/// Point the page at chain 40204 and the deployed contract. Keeps every other line of
/// `app/.env.local`; replaces the target, address and fork URL lines. Returns the file path.
pub fn switch_site_to_citrate(p: &HelloMintProject, address: &str) -> Result<PathBuf, String> {
    let addr = normalize_address(address)?;
    let path = p.app_dir.join(".env.local");
    let mut out = String::new();
    for line in read_env(&path).lines() {
        if env_key(line).is_some_and(|k| SITE_KEYS.contains(&k)) {
            continue;
        }
        if line.starts_with("# Set by Citrate after the deploy") {
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out.push_str(&citrate_env_block(&addr));
    std::fs::write(&path, out).map_err(|e| format!("could not write {}: {e}", path.display()))?;
    Ok(path)
}

/// The contract the page targets on chain 40204, if it has been switched (lowercased).
pub fn site_contract(p: &HelloMintProject) -> Result<Option<String>, String> {
    let text = read_env(&p.app_dir.join(".env.local"));
    if env_value(&text, "VITE_TARGET") != Some("citrate") {
        return Ok(None);
    }
    match env_value(&text, "VITE_CONTRACT_ADDRESS") {
        None | Some("") => Ok(None),
        Some(a) => normalize_address(a).map(Some),
    }
}

// ---------------------------------------------------------------------------
// Receipt
// ---------------------------------------------------------------------------

/// A confirmed deploy transaction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeployReceipt {
    pub tx_hash: String,
    pub block_number: u64,
    /// 1 = success, 0 = reverted.
    pub status: Option<u64>,
    /// The created contract (lowercased), only for a successful creation.
    pub contract_address: Option<String>,
}

fn hex_u64(v: &serde_json::Value) -> Option<u64> {
    v.as_str()
        .and_then(|s| s.strip_prefix("0x"))
        .and_then(|h| u64::from_str_radix(h, 16).ok())
}

/// Validate a `0x` + 64-hex transaction hash.
pub fn parse_tx_hash(s: &str) -> Result<String, String> {
    let t = s.trim();
    let h = t
        .strip_prefix("0x")
        .ok_or_else(|| "a transaction hash starts with 0x".to_string())?;
    if h.len() != 64 || !h.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("a transaction hash is 0x followed by 64 hex digits".to_string());
    }
    Ok(format!("0x{}", h.to_ascii_lowercase()))
}

/// Parse `eth_getTransactionReceipt`'s result (`null` while pending).
pub fn parse_receipt(v: &serde_json::Value) -> Result<Option<DeployReceipt>, String> {
    if v.is_null() {
        return Ok(None);
    }
    let tx_hash = v
        .get("transactionHash")
        .and_then(|x| x.as_str())
        .ok_or_else(|| "the receipt has no transaction hash".to_string())?
        .to_string();
    let block_number = v
        .get("blockNumber")
        .and_then(hex_u64)
        .ok_or_else(|| "the receipt has no block number".to_string())?;
    let status = v.get("status").and_then(hex_u64);
    let contract_address = match (status, v.get("contractAddress").and_then(|x| x.as_str())) {
        (Some(1), Some(a)) => Some(normalize_address(a)?),
        _ => None,
    };
    Ok(Some(DeployReceipt {
        tx_hash,
        block_number,
        status,
        contract_address,
    }))
}

// ---------------------------------------------------------------------------
// Verification (CitrateScan)
// ---------------------------------------------------------------------------

/// What is submitted to CitrateScan's verifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyRequest {
    pub address: String,
    /// The solc standard-JSON input (a JSON document, as text).
    pub standard_json: String,
    /// `0.8.36` or `0.8.36+commit.<hash>`.
    pub compiler_version: String,
    /// The ABI-encoded constructor arguments, if any.
    pub constructor_args_hex: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum VerifyStatus {
    /// A full match: verified.
    Verified,
    /// A metadata-stripped match only: not verified.
    Partial,
    /// The explorer compiled the source and it did not match (or the input was refused).
    Failed,
    /// The explorer could not be reached, was rate limiting, busy, or answered unexpectedly.
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyOutcome {
    pub status: VerifyStatus,
    pub guid: Option<String>,
    pub match_type: Option<String>,
    pub contract_name: Option<String>,
    pub message: String,
}

fn s_field(v: &serde_json::Value, k: &str) -> Option<String> {
    v.get(k).and_then(|x| x.as_str()).map(str::to_string)
}

/// Read the verifier's answer, whatever its HTTP status.
pub fn parse_verify_response(status: u16, body: &str) -> VerifyOutcome {
    let unavailable = |message: String| VerifyOutcome {
        status: VerifyStatus::Unavailable,
        guid: None,
        match_type: None,
        contract_name: None,
        message,
    };
    if status == 429 || status == 503 || status >= 500 {
        return unavailable(crate::contract_reader::explorer_status_error(status, body));
    }
    let Ok(v) = serde_json::from_str::<serde_json::Value>(body) else {
        return unavailable(format!(
            "CitrateScan answered HTTP {status} with something that is not JSON"
        ));
    };
    let match_type = s_field(&v, "matchType");
    let message = s_field(&v, "message")
        .or_else(|| s_field(&v, "error"))
        .unwrap_or_default();
    let verdict = match (s_field(&v, "status").as_deref(), match_type.as_deref()) {
        (Some("pass"), Some("full")) => VerifyStatus::Verified,
        (Some("pass"), _) => VerifyStatus::Partial,
        (Some("fail"), _) => VerifyStatus::Failed,
        _ if status == 400 || status == 422 => VerifyStatus::Failed,
        _ => {
            return unavailable(format!(
                "CitrateScan answered HTTP {status} without a verdict"
            ))
        }
    };
    VerifyOutcome {
        status: verdict,
        guid: s_field(&v, "guid"),
        match_type,
        contract_name: s_field(&v, "contractName"),
        message,
    }
}

/// Check the inputs, then submit them to CitrateScan's verifier.
pub fn submit_verification(
    http: &dyn ExplorerHttp,
    req: &VerifyRequest,
) -> Result<VerifyOutcome, String> {
    let address = normalize_address(&req.address)?;
    if !is_compiler_version(&req.compiler_version) {
        return Err("the compiler version must look like 0.8.36".to_string());
    }
    if serde_json::from_str::<serde_json::Value>(&req.standard_json)
        .map(|v| !v.is_object())
        .unwrap_or(true)
    {
        return Err("the compiler input is not a standard-JSON object".to_string());
    }
    let args = match req.constructor_args_hex.as_deref().map(str::trim) {
        None | Some("") | Some("0x") => None,
        Some(a) => {
            let h = a.strip_prefix("0x").unwrap_or(a);
            if h.len() % 2 != 0 || !h.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err("the constructor arguments are not valid hex".to_string());
            }
            Some(h.to_ascii_lowercase())
        }
    };
    let mut body = serde_json::json!({
        "address": address,
        "format": "solidity-standard-json-input",
        "compilerVersion": req.compiler_version,
        "source": req.standard_json,
    });
    if let Some(a) = args {
        body["constructorArguments"] = serde_json::Value::String(a);
    }
    let url = format!("{}/api/verify", crate::activity::EXPLORER_BASE);
    match http.post_json(&url, &body) {
        Ok((status, text)) => Ok(parse_verify_response(status, &text)),
        Err(e) => Ok(VerifyOutcome {
            status: VerifyStatus::Unavailable,
            guid: None,
            match_type: None,
            contract_name: None,
            message: e,
        }),
    }
}

/// The `forge` arguments that print the project contract's standard-JSON input.
pub fn forge_standard_json_args(p: &HelloMintProject, address: &str) -> Vec<String> {
    vec![
        "verify-contract".to_string(),
        address.to_string(),
        format!("src/Token.sol:{}", p.contract_name),
        "--show-standard-json-input".to_string(),
    ]
}

/// The forge binary: `CITRATE_FORGE_BIN`, else `forge` on PATH, else Foundry's default install.
fn forge_bin() -> Result<PathBuf, String> {
    if let Ok(p) = std::env::var("CITRATE_FORGE_BIN") {
        let path = PathBuf::from(p);
        return if path.is_file() {
            Ok(path)
        } else {
            Err("CITRATE_FORGE_BIN does not point at a file".to_string())
        };
    }
    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            let c = dir.join("forge");
            if c.is_file() {
                return Ok(c);
            }
        }
    }
    if let Some(home) = std::env::var_os("HOME") {
        let c = PathBuf::from(home).join(".foundry/bin/forge");
        if c.is_file() {
            return Ok(c);
        }
    }
    Err(
        "forge is not installed, so the compiler input for verification cannot be produced"
            .to_string(),
    )
}

/// Run forge in `contracts/` to get the standard-JSON input of the project contract.
fn forge_standard_json(p: &HelloMintProject, address: &str) -> Result<String, String> {
    let bin = forge_bin()?;
    let out = std::process::Command::new(bin)
        .current_dir(&p.contracts_dir)
        .args(forge_standard_json_args(p, address))
        .output()
        .map_err(|e| format!("forge could not be run: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let first = err
            .lines()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("no output");
        return Err(format!(
            "forge could not produce the compiler input: {first}"
        ));
    }
    String::from_utf8(out.stdout).map_err(|_| "forge printed something that is not text".into())
}

// ---------------------------------------------------------------------------
// IPFS site pin
// ---------------------------------------------------------------------------

/// Every file of the built site (`app/dist`), relative paths with `/`, sorted. Symlinks are
/// refused (never followed), and the site must have an `index.html` at its root.
pub fn collect_site_files(dist: &Path) -> Result<Vec<(String, Vec<u8>)>, String> {
    // The build folder itself must not be a link either (it would pin whatever it points at).
    if std::fs::symlink_metadata(dist).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(format!(
            "the build folder is a link ({}); links are not pinned",
            dist.display()
        ));
    }
    if !dist.join("index.html").is_file() {
        return Err(
            "the page is not built yet: run npm run build in the app folder first".to_string(),
        );
    }
    let mut out = Vec::new();
    let mut total: u64 = 0;
    let mut stack = vec![dist.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries =
            std::fs::read_dir(&dir).map_err(|e| format!("could not read the build: {e}"))?;
        for entry in entries.flatten() {
            let path = entry.path();
            let meta = std::fs::symlink_metadata(&path)
                .map_err(|e| format!("could not read the build: {e}"))?;
            if meta.file_type().is_symlink() {
                return Err(format!(
                    "the build contains a link ({}); links are not pinned",
                    path.display()
                ));
            }
            if meta.is_dir() {
                stack.push(path);
                continue;
            }
            total = total.saturating_add(meta.len());
            if out.len() >= MAX_SITE_FILES || total > MAX_SITE_BYTES {
                return Err("the build is too large to pin (over 2,000 files or 100 MB)".into());
            }
            let rel = path
                .strip_prefix(dist)
                .map_err(|_| "a build file is outside the build folder".to_string())?
                .components()
                .map(|c| c.as_os_str().to_string_lossy().to_string())
                .collect::<Vec<_>>()
                .join("/");
            let bytes =
                std::fs::read(&path).map_err(|e| format!("could not read the build: {e}"))?;
            out.push((rel, bytes));
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

fn pct_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// One kubo `add` multipart body: the root folder, every nested folder, then every file.
/// Returns the boundary and the body. The boundary is derived from the content, so it cannot
/// appear in it by accident.
pub fn directory_multipart(files: &[(String, Vec<u8>)]) -> (String, Vec<u8>) {
    let mut h = Keccak256::new();
    for (n, b) in files {
        h.update(n.as_bytes());
        h.update(b);
    }
    let boundary = format!("citrate-site-{}", hex::encode(&h.finalize()[..16]));
    let mut dirs = std::collections::BTreeSet::new();
    for (name, _) in files {
        let parts: Vec<&str> = name.split('/').collect();
        for i in 1..parts.len() {
            dirs.insert(parts[..i].join("/"));
        }
    }
    let mut body = Vec::new();
    let mut part = |filename: &str, ctype: &str, bytes: &[u8]| {
        body.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{filename}\"\r\nContent-Type: {ctype}\r\n\r\n"
            )
            .as_bytes(),
        );
        body.extend_from_slice(bytes);
        body.extend_from_slice(b"\r\n");
    };
    part(SITE_ROOT, "application/x-directory", b"");
    for d in &dirs {
        part(
            &pct_encode(&format!("{SITE_ROOT}/{d}")),
            "application/x-directory",
            b"",
        );
    }
    for (name, bytes) in files {
        part(
            &pct_encode(&format!("{SITE_ROOT}/{name}")),
            "application/octet-stream",
            bytes,
        );
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    (boundary, body)
}

/// The site folder's CID from kubo's newline-delimited `add` output.
pub fn parse_add_root(ndjson: &str) -> Result<String, String> {
    ndjson
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .find(|v| v.get("Name").and_then(|n| n.as_str()) == Some(SITE_ROOT))
        .and_then(|v| v.get("Hash").and_then(|h| h.as_str()).map(str::to_string))
        .filter(|c| !c.is_empty() && c.chars().all(|ch| ch.is_ascii_alphanumeric()))
        .ok_or_else(|| "IPFS did not report the site folder's CID".to_string())
}

/// A pinned site.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SitePin {
    pub cid: String,
    pub files: usize,
    pub bytes: u64,
    pub local_gateway_url: String,
    pub public_gateway_url: String,
    pub note: String,
}

pub fn site_pin_result(cid: &str, files: usize, bytes: u64) -> SitePin {
    SitePin {
        cid: cid.to_string(),
        files,
        bytes,
        local_gateway_url: format!("{LOCAL_GATEWAY}/ipfs/{cid}/"),
        public_gateway_url: format!("{PUBLIC_GATEWAY}/ipfs/{cid}/"),
        note: "Pinned on this node. Public gateways can serve it while this node (or another node that pinned it) is online and reachable.".to_string(),
    }
}

fn kubo_api() -> String {
    std::env::var(crate::storage::KUBO_API_ENV)
        .unwrap_or_else(|_| crate::storage::DEFAULT_KUBO_API.to_string())
}

/// Add the built site to the local kubo daemon as one pinned folder.
fn pin_site(p: &HelloMintProject, api: &str) -> Result<SitePin, String> {
    let files = collect_site_files(&p.app_dir.join("dist"))?;
    let bytes: u64 = files.iter().map(|(_, b)| b.len() as u64).sum();
    let (boundary, body) = directory_multipart(&files);
    let url = format!("{api}/api/v0/add?cid-version=1&pin=true&progress=false");
    let resp = ureq::post(&url)
        .config()
        .timeout_global(Some(std::time::Duration::from_secs(120)))
        .build()
        .header(
            "Content-Type",
            &format!("multipart/form-data; boundary={boundary}"),
        )
        .send(&body[..])
        .map_err(|e| {
            format!("IPFS is not running in this app ({e}). Start it from Storage, then pin again. Nothing was pinned.")
        })?;
    let text = resp
        .into_body()
        .read_to_string()
        .map_err(|e| format!("could not read the IPFS answer: {e}"))?;
    let cid = parse_add_root(&text)?;
    Ok(site_pin_result(&cid, files.len(), bytes))
}

// ---------------------------------------------------------------------------
// Vercel export
// ---------------------------------------------------------------------------

/// A Vercel-ready project folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VercelExport {
    pub dir: String,
    pub files: usize,
    /// What the member runs to deploy it with their own Vercel account.
    pub commands: Vec<String>,
}

fn copy_tree(from: &Path, to: &Path, count: &mut usize) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|e| format!("could not create {}: {e}", to.display()))?;
    let entries = std::fs::read_dir(from).map_err(|e| format!("could not read the app: {e}"))?;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if EXPORT_SKIP.contains(&name.as_str())
            || (name.starts_with(".env") && name.ends_with(".local"))
        {
            continue;
        }
        let path = entry.path();
        let meta =
            std::fs::symlink_metadata(&path).map_err(|e| format!("could not read the app: {e}"))?;
        if meta.file_type().is_symlink() {
            continue; // never follow a link out of the project
        }
        let dest = to.join(&name);
        if meta.is_dir() {
            copy_tree(&path, &dest, count)?;
        } else {
            *count += 1;
            if *count > MAX_EXPORT_FILES {
                return Err("the app has too many files to export (over 5,000)".to_string());
            }
            std::fs::copy(&path, &dest).map_err(|e| format!("could not copy {name}: {e}"))?;
        }
    }
    Ok(())
}

/// Write `vercel-export/`: the page sources, `vercel.json` and `.env.production` for 40204.
pub fn vercel_export(p: &HelloMintProject) -> Result<VercelExport, String> {
    let address = site_contract(p)?.ok_or_else(|| {
        "switch the site to chain 40204 first (it needs the deployed contract address)".to_string()
    })?;
    let dir = p.root.join(EXPORT_DIR);
    if dir.exists() {
        if !dir.join(EXPORT_MARKER).is_file() {
            return Err(format!(
                "{} exists and was not written by Citrate; move it away first",
                dir.display()
            ));
        }
        std::fs::remove_dir_all(&dir).map_err(|e| format!("could not replace the export: {e}"))?;
    }
    let mut count = 0usize;
    copy_tree(&p.app_dir, &dir, &mut count)?;
    let vercel = serde_json::json!({
        "$schema": "https://openapi.vercel.sh/vercel.json",
        "framework": "vite",
        "installCommand": "npm install",
        "buildCommand": "npm run build",
        "outputDirectory": "dist",
    });
    let write = |name: &str, text: String| {
        std::fs::write(dir.join(name), text).map_err(|e| format!("could not write {name}: {e}"))
    };
    write(
        "vercel.json",
        serde_json::to_string_pretty(&vercel).map_err(|e| e.to_string())? + "\n",
    )?;
    write(".env.production", citrate_env_block(&address))?;
    write(
        EXPORT_MARKER,
        "Written by Citrate (HUP-S6.6). Citrate replaces this folder on the next export.\n"
            .to_string(),
    )?;
    Ok(VercelExport {
        dir: dir.display().to_string(),
        files: count + 3,
        commands: vec![
            format!("cd \"{}\"", dir.display()),
            "npx vercel deploy --prod".to_string(),
        ],
    })
}

// ---------------------------------------------------------------------------
// Status
// ---------------------------------------------------------------------------

/// Where a project stands after its deploy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PostDeployStatus {
    pub contract_name: String,
    /// The 40204 contract the page targets, once switched.
    pub site_contract: Option<String>,
    /// Whether `app/dist/index.html` exists (the page has been built).
    pub built: bool,
    /// The Vercel export folder, if one was written.
    pub export_dir: Option<String>,
}

pub fn project_status(p: &HelloMintProject) -> Result<PostDeployStatus, String> {
    let export = p.root.join(EXPORT_DIR);
    Ok(PostDeployStatus {
        contract_name: p.contract_name.clone(),
        site_contract: site_contract(p)?,
        built: p.app_dir.join("dist/index.html").is_file(),
        export_dir: export
            .join(EXPORT_MARKER)
            .is_file()
            .then(|| export.display().to_string()),
    })
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

/// **postdeploy_status** — where a hello-mint project stands after its deploy (local reads).
#[tauri::command]
pub async fn postdeploy_status(
    project_dir: String,
) -> std::result::Result<PostDeployStatus, String> {
    crate::blocking::off_main(move || postdeploy_status_sync(project_dir)).await
}

/// Blocking body of [`postdeploy_status`]; reached only through [`crate::blocking::off_main`].
pub fn postdeploy_status_sync(
    project_dir: String,
) -> std::result::Result<PostDeployStatus, String> {
    project_status(&open_project(Path::new(&project_dir))?)
}

/// **postdeploy_receipt** — the deploy transaction's receipt on 40204 (`None` while pending).
#[tauri::command]
pub async fn postdeploy_receipt(
    tx_hash: String,
) -> std::result::Result<Option<DeployReceipt>, String> {
    crate::blocking::off_main(move || postdeploy_receipt_sync(tx_hash)).await
}

/// Blocking body of [`postdeploy_receipt`]; reached only through [`crate::blocking::off_main`].
pub fn postdeploy_receipt_sync(
    tx_hash: String,
) -> std::result::Result<Option<DeployReceipt>, String> {
    let hash = parse_tx_hash(&tx_hash)?;
    let client = crate::rpc::RpcClient::citrate();
    let v = crate::contract_reader::rpc_raw(
        &client,
        "eth_getTransactionReceipt",
        serde_json::json!([hash]),
    )?;
    parse_receipt(&v)
}

/// **postdeploy_verify** — submit the project contract's source to CitrateScan's verifier.
#[tauri::command]
pub async fn postdeploy_verify(
    project_dir: String,
    address: String,
    constructor_args_hex: Option<String>,
) -> std::result::Result<VerifyOutcome, String> {
    crate::blocking::off_main(move || {
        postdeploy_verify_sync(project_dir, address, constructor_args_hex)
    })
    .await
}

/// Blocking body of [`postdeploy_verify`]; reached only through [`crate::blocking::off_main`].
pub fn postdeploy_verify_sync(
    project_dir: String,
    address: String,
    constructor_args_hex: Option<String>,
) -> std::result::Result<VerifyOutcome, String> {
    let p = open_project(Path::new(&project_dir))?;
    let address = normalize_address(&address)?;
    let standard_json = forge_standard_json(&p, &address)?;
    submit_verification(
        &crate::contract_reader::UreqExplorer,
        &VerifyRequest {
            address,
            standard_json,
            compiler_version: p.solc.clone(),
            constructor_args_hex,
        },
    )
}

/// What a site switch wrote.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SiteSwitch {
    pub env_path: String,
    pub address: String,
}

/// **postdeploy_switch_site** — point the page at chain 40204 and the deployed contract. Refused
/// unless the address holds code on 40204.
#[tauri::command]
pub async fn postdeploy_switch_site(
    project_dir: String,
    address: String,
) -> std::result::Result<SiteSwitch, String> {
    crate::blocking::off_main(move || postdeploy_switch_site_sync(project_dir, address)).await
}

/// Blocking body of [`postdeploy_switch_site`]; reached only through [`crate::blocking::off_main`].
pub fn postdeploy_switch_site_sync(
    project_dir: String,
    address: String,
) -> std::result::Result<SiteSwitch, String> {
    let p = open_project(Path::new(&project_dir))?;
    switch_site_checked(&crate::rpc::RpcClient::citrate(), &p, &address)
}

/// Switch the site only after `client` (40204 in production) shows code at `address`.
pub fn switch_site_checked<T: crate::rpc::RpcTransport>(
    client: &crate::rpc::RpcClient<T>,
    p: &HelloMintProject,
    address: &str,
) -> std::result::Result<SiteSwitch, String> {
    let address = normalize_address(address)?;
    let size = crate::contract_reader::code_size(client, &address)?;
    if size == 0 {
        return Err("there is no contract code at this address on chain 40204 yet".to_string());
    }
    let path = switch_site_to_citrate(p, &address)?;
    Ok(SiteSwitch {
        env_path: path.display().to_string(),
        address: checksum_address(&address),
    })
}

/// **postdeploy_pin_site** — add the built page to the app's IPFS daemon as one pinned folder.
#[tauri::command]
pub async fn postdeploy_pin_site(project_dir: String) -> std::result::Result<SitePin, String> {
    crate::blocking::off_main(move || postdeploy_pin_site_sync(project_dir)).await
}

/// Blocking body of [`postdeploy_pin_site`]; reached only through [`crate::blocking::off_main`].
pub fn postdeploy_pin_site_sync(project_dir: String) -> std::result::Result<SitePin, String> {
    let p = open_project(Path::new(&project_dir))?;
    if site_contract(&p)?.is_none() {
        return Err("switch the site to chain 40204 before pinning it".to_string());
    }
    pin_site(&p, &kubo_api())
}

/// **postdeploy_vercel_export** — write the Vercel-ready `vercel-export/` folder (no account
/// actions: the member deploys it with their own Vercel login).
#[tauri::command]
pub async fn postdeploy_vercel_export(
    project_dir: String,
) -> std::result::Result<VercelExport, String> {
    crate::blocking::off_main(move || postdeploy_vercel_export_sync(project_dir)).await
}

/// Blocking body of [`postdeploy_vercel_export`]; reached only through [`crate::blocking::off_main`].
pub fn postdeploy_vercel_export_sync(
    project_dir: String,
) -> std::result::Result<VercelExport, String> {
    vercel_export(&open_project(Path::new(&project_dir))?)
}

#[cfg(test)]
mod tests {
    include!("postdeploy_tests.rs");
}
