//! HUP-S6.7 (US-6.3) — the **contract reader** backend.
//!
//! The Contract reader pop-out opens any address on chain 40204 (or on the member's local anvil
//! fork during a hello-mint dry run) and lets the member read it:
//!
//! - **Verified source and ABI** come from CitrateScan's public contract endpoint
//!   (`GET {EXPLORER_BASE}/api/contract/{address}`; source: citrate-explorer
//!   `src/app/api/contract/[addr]/route.ts`). A full match is "verified"; a partial match is shown
//!   as a partial match, never as verified; an unverified contract has no ABI and the member can
//!   paste one instead. The explorer host is pinned, never a webview-supplied URL.
//! - **View calls** are `eth_call` against the read target (40204's public RPC, or a loopback fork
//!   URL). A read carries no sender and changes nothing.
//! - **Write calls** become a PENDING SignatureCeremony (Rule 3): this module builds the
//!   transaction JSON and asks the node for a gas estimate; the member reviews and approves in the
//!   ceremony, which signs and broadcasts. Writes target 40204 only. A failed estimate is a
//!   refusal with the node's reason, never a guessed gas limit.
//!
//! The pop-out window itself holds none of these commands (least privilege). It hands requests to
//! Rust (`popout_contract.rs`, which checks the sender), and the main window calls these commands.
use serde::Serialize;
use sha3::{Digest as _, Keccak256};

/// The ceremony origin of a write the member started in the Contract reader.
pub const READER_ORIGIN: &str = "local-user:contract-reader";
/// Calldata larger than this is refused (a function call, not a payload upload).
pub const MAX_CALLDATA_BYTES: usize = 64 * 1024;
/// The highest gas limit a reader write may carry (a block-sized call is never proposed).
/// This cap and the 20 % headroom in [`with_headroom`] are conservative defaults, pending owner
/// sign-off.
pub const MAX_WRITE_GAS: u64 = 15_000_000;
/// The smallest gas limit a write carries (the intrinsic cost of a transaction).
const MIN_WRITE_GAS: u64 = 21_000;
/// Explorer bodies larger than this are refused (verified sources are bounded by the explorer at
/// 2 MiB of source; the ABI and metadata sit on top).
const MAX_EXPLORER_BODY: usize = 8 * 1024 * 1024;
/// How long one explorer request may take.
const EXPLORER_TIMEOUT_SECS: u64 = 20;
const LOOPBACK_HOSTS: [&str; 3] = ["127.0.0.1", "localhost", "[::1]"];

// ---------------------------------------------------------------------------
// Inputs
// ---------------------------------------------------------------------------

/// Validate a `0x` + 40-hex address and return it lowercased.
pub fn normalize_address(s: &str) -> Result<String, String> {
    let t = s.trim();
    let hex_part = t
        .strip_prefix("0x")
        .or_else(|| t.strip_prefix("0X"))
        .ok_or_else(|| "an address starts with 0x".to_string())?;
    if hex_part.len() != 40 || !hex_part.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("an address is 0x followed by 40 hex digits".to_string());
    }
    Ok(format!("0x{}", hex_part.to_ascii_lowercase()))
}

/// The EIP-55 mixed-case form of a lowercased `0x` address.
pub fn checksum_address(lower: &str) -> String {
    let body = lower.trim_start_matches("0x");
    let hash = Keccak256::digest(body.as_bytes());
    let mut out = String::with_capacity(42);
    out.push_str("0x");
    for (i, c) in body.chars().enumerate() {
        let nibble = (hash[i / 2] >> (if i % 2 == 0 { 4 } else { 0 })) & 0x0f;
        if c.is_ascii_alphabetic() && nibble >= 8 {
            out.push(c.to_ascii_uppercase());
        } else {
            out.push(c);
        }
    }
    out
}

/// Parse `0x` calldata: at least a 4-byte selector, at most [`MAX_CALLDATA_BYTES`].
pub fn parse_calldata(s: &str) -> Result<Vec<u8>, String> {
    let t = s.trim();
    let h = t.strip_prefix("0x").unwrap_or(t);
    if h.len() > MAX_CALLDATA_BYTES * 2 {
        return Err(format!(
            "calldata is larger than {MAX_CALLDATA_BYTES} bytes"
        ));
    }
    let bytes = hex::decode(h).map_err(|_| "calldata is not valid hex".to_string())?;
    if bytes.len() < 4 {
        return Err("calldata needs at least a 4-byte function selector".to_string());
    }
    Ok(bytes)
}

/// Parse a decimal wei amount (empty or absent is zero).
pub fn parse_value_wei(s: Option<&str>) -> Result<u128, String> {
    match s.map(str::trim) {
        None | Some("") => Ok(0),
        Some(v) if v.chars().all(|c| c.is_ascii_digit()) => v
            .parse::<u128>()
            .map_err(|_| "the value is too large".to_string()),
        Some(_) => Err("the value must be a whole number of wei (decimal)".to_string()),
    }
}

/// Where reads go: chain 40204's public RPC, or the member's local anvil fork (loopback only).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadTarget {
    Citrate,
    Fork(String),
}

impl ReadTarget {
    pub fn rpc_url(&self) -> String {
        match self {
            ReadTarget::Citrate => crate::rpc::CITRATE_RPC_URL.to_string(),
            ReadTarget::Fork(u) => u.clone(),
        }
    }
}

/// `None`, `"citrate"` or `"40204"` is chain 40204; anything else must be an `http://` loopback
/// URL with no credentials (the local fork). Every other URL is refused, so the webview can never
/// point a read at an arbitrary host.
pub fn parse_target(s: Option<&str>) -> Result<ReadTarget, String> {
    let raw = match s.map(str::trim) {
        None | Some("") | Some("citrate") | Some("40204") => return Ok(ReadTarget::Citrate),
        Some(r) => r,
    };
    let url = url::Url::parse(raw).map_err(|_| "the read target is not a URL".to_string())?;
    let host = url.host_str().unwrap_or_default();
    let loopback = LOOPBACK_HOSTS.contains(&host);
    if url.scheme() != "http" || !loopback || !url.username().is_empty() || url.password().is_some()
    {
        return Err(
            "reads go to chain 40204 or to a local fork at an http:// loopback address".to_string(),
        );
    }
    // Hand on the URL that was checked (its normalized serialization), never the raw text,
    // which another URL parser could read with a different host.
    Ok(ReadTarget::Fork(url.to_string()))
}

// ---------------------------------------------------------------------------
// Explorer
// ---------------------------------------------------------------------------

/// The explorer HTTP seam. Production is [`UreqExplorer`]; tests answer from fixtures.
pub trait ExplorerHttp {
    /// GET `url`; returns the status and body for any status (non-2xx is not a transport error).
    fn get(&self, url: &str) -> Result<(u16, String), String>;
    /// POST `body` as JSON to `url`; same contract as [`ExplorerHttp::get`].
    fn post_json(&self, url: &str, body: &serde_json::Value) -> Result<(u16, String), String>;
}

/// The real explorer client: blocking `ureq`, bounded time and body size.
pub struct UreqExplorer;

impl UreqExplorer {
    fn read(mut resp: ureq::http::Response<ureq::Body>) -> Result<(u16, String), String> {
        let status = resp.status().as_u16();
        let body = resp
            .body_mut()
            .with_config()
            .limit(MAX_EXPLORER_BODY as u64)
            .read_to_string()
            .map_err(|e| format!("could not read the CitrateScan response: {e}"))?;
        Ok((status, body))
    }
}

impl ExplorerHttp for UreqExplorer {
    fn get(&self, url: &str) -> Result<(u16, String), String> {
        let resp = ureq::get(url)
            .config()
            .timeout_global(Some(std::time::Duration::from_secs(EXPLORER_TIMEOUT_SECS)))
            .http_status_as_error(false)
            .build()
            .call()
            .map_err(|e| format!("could not reach CitrateScan: {e}"))?;
        Self::read(resp)
    }

    fn post_json(&self, url: &str, body: &serde_json::Value) -> Result<(u16, String), String> {
        // Verification recompiles on the explorer and can take tens of seconds.
        let resp = ureq::post(url)
            .config()
            .timeout_global(Some(std::time::Duration::from_secs(300)))
            .http_status_as_error(false)
            .build()
            .send_json(body)
            .map_err(|e| format!("could not reach CitrateScan: {e}"))?;
        Self::read(resp)
    }
}

/// What CitrateScan knows about an address's source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SourceStatus {
    /// A full (exact) recompile match: the source and ABI are verified.
    Verified,
    /// A metadata-stripped match: shown for reference, not verified.
    Partial,
    /// A contract with no verification record. The member can paste an ABI.
    Unverified,
    /// No contract code at this address.
    NotContract,
}

/// The reader's view of an address's verified source.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifiedSource {
    pub status: SourceStatus,
    pub is_contract: bool,
    pub code_size: Option<u64>,
    pub contract_name: Option<String>,
    pub compiler_version: Option<String>,
    /// The ABI array, only when the explorer returned an array.
    pub abi: Option<serde_json::Value>,
    pub source: Option<String>,
    /// The explorer's own note (partial match, not verified, EOA), passed through.
    pub note: Option<String>,
}

fn str_field(v: &serde_json::Value, k: &str) -> Option<String> {
    v.get(k).and_then(|x| x.as_str()).map(str::to_string)
}

/// Parse the explorer's `/api/contract/{addr}` body.
pub fn parse_explorer_contract(body: &str) -> Result<VerifiedSource, String> {
    let v: serde_json::Value = serde_json::from_str(body)
        .map_err(|_| "CitrateScan returned something that is not JSON".to_string())?;
    let is_contract = v
        .get("isContract")
        .and_then(|x| x.as_bool())
        .ok_or_else(|| "CitrateScan's answer has no isContract field".to_string())?;
    let code_size = v.get("codeSize").and_then(|x| x.as_u64());
    if !is_contract {
        return Ok(VerifiedSource {
            status: SourceStatus::NotContract,
            is_contract,
            code_size,
            contract_name: None,
            compiler_version: None,
            abi: None,
            source: None,
            note: str_field(&v, "note"),
        });
    }
    let null = serde_json::Value::Null;
    let ver = v.get("verification").unwrap_or(&null);
    let verified = ver.get("verified").and_then(|x| x.as_bool()) == Some(true);
    let match_type = str_field(ver, "matchType");
    let status = if verified && match_type.as_deref() == Some("full") {
        SourceStatus::Verified
    } else if match_type.is_some() {
        SourceStatus::Partial
    } else {
        SourceStatus::Unverified
    };
    let abi = ver.get("abi").filter(|a| a.is_array()).cloned();
    Ok(VerifiedSource {
        status,
        is_contract,
        code_size,
        contract_name: str_field(ver, "contractName"),
        compiler_version: str_field(ver, "compilerVersion"),
        abi: if status == SourceStatus::Unverified {
            None
        } else {
            abi
        },
        source: if status == SourceStatus::Unverified {
            None
        } else {
            str_field(ver, "source")
        },
        note: str_field(ver, "note"),
    })
}

/// Map an explorer status that is not 2xx to an honest message.
pub fn explorer_status_error(status: u16, body: &str) -> String {
    let detail = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| str_field(&v, "error"))
        .unwrap_or_default();
    match status {
        429 => "CitrateScan is rate limiting requests from this address; try again in a minute"
            .to_string(),
        400 => format!("CitrateScan refused the request: {detail}"),
        _ if detail.is_empty() => format!("CitrateScan answered HTTP {status}"),
        _ => format!("CitrateScan answered HTTP {status}: {detail}"),
    }
}

/// Read an address's verified source from CitrateScan.
pub fn fetch_verified_source(
    http: &dyn ExplorerHttp,
    address: &str,
) -> Result<VerifiedSource, String> {
    let address = normalize_address(address)?;
    let url = format!("{}/api/contract/{address}", crate::activity::EXPLORER_BASE);
    let (status, body) = http.get(&url)?;
    if !(200..300).contains(&status) {
        return Err(explorer_status_error(status, &body));
    }
    parse_explorer_contract(&body)
}

// ---------------------------------------------------------------------------
// RPC reads
// ---------------------------------------------------------------------------

/// One JSON-RPC call through a kit client's transport; returns `result` or the node's message.
pub(crate) fn rpc_raw<T: crate::rpc::RpcTransport>(
    client: &crate::rpc::RpcClient<T>,
    method: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let body = client.build_request(method, params);
    let resp = client
        .transport()
        .call(body)
        .map_err(|e| format!("the RPC could not be reached: {e}"))?;
    if let Some(err) = resp.get("error") {
        let msg = err
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("unknown node error");
        return Err(format!("the node refused: {msg}"));
    }
    resp.get("result")
        .cloned()
        .ok_or_else(|| "the node's answer has no result".to_string())
}

/// `eth_call({to, data}, "latest")` → the raw `0x` return data. No sender, no value.
pub fn view_call<T: crate::rpc::RpcTransport>(
    client: &crate::rpc::RpcClient<T>,
    to: &str,
    data: &[u8],
) -> Result<String, String> {
    let call = serde_json::json!({ "to": to, "data": format!("0x{}", hex::encode(data)) });
    let out = rpc_raw(client, "eth_call", serde_json::json!([call, "latest"]))?;
    let s = out
        .as_str()
        .ok_or_else(|| "the node's eth_call result is not a string".to_string())?;
    let h = s
        .strip_prefix("0x")
        .ok_or_else(|| "the node's eth_call result is not 0x hex".to_string())?;
    if !h.chars().all(|c| c.is_ascii_hexdigit()) || h.len() % 2 != 0 {
        return Err("the node's eth_call result is not valid hex".to_string());
    }
    Ok(format!("0x{}", h.to_ascii_lowercase()))
}

/// `eth_getCode(address, "latest")` → the byte length of the deployed code (0 = no contract).
pub fn code_size<T: crate::rpc::RpcTransport>(
    client: &crate::rpc::RpcClient<T>,
    address: &str,
) -> Result<usize, String> {
    let out = rpc_raw(
        client,
        "eth_getCode",
        serde_json::json!([address, "latest"]),
    )?;
    let s = out
        .as_str()
        .and_then(|s| s.strip_prefix("0x"))
        .ok_or_else(|| "the node's eth_getCode result is not 0x hex".to_string())?;
    if s.len() % 2 != 0 || !s.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("the node's eth_getCode result is not valid hex".to_string());
    }
    Ok(s.len() / 2)
}

// ---------------------------------------------------------------------------
// Writes (ceremony only)
// ---------------------------------------------------------------------------

/// The estimate plus 20 % headroom, at least the intrinsic cost, at most [`MAX_WRITE_GAS`].
pub fn with_headroom(estimate: u64) -> u64 {
    let padded = estimate.saturating_add(estimate / 5);
    padded.clamp(MIN_WRITE_GAS, MAX_WRITE_GAS)
}

/// Ask the node what the write costs. A failed estimate (the call would revert, or the node is
/// unreachable) refuses the write with the node's reason.
pub fn estimate_write_gas<T: crate::rpc::RpcTransport>(
    client: &crate::rpc::RpcClient<T>,
    from: &str,
    to: &str,
    data: &[u8],
    value_wei: u128,
) -> Result<u64, String> {
    let call = serde_json::json!({
        "from": from,
        "to": to,
        "value": format!("0x{value_wei:x}"),
        "data": format!("0x{}", hex::encode(data)),
    });
    client
        .estimate_gas(call)
        .map(with_headroom)
        .map_err(|e| format!("the write was not proposed: the node could not estimate it ({e})"))
}

/// The ceremony intent for a reader write: a 40204 transaction to `to` with explicit gas.
pub fn write_intent(
    from: &str,
    to: &str,
    data: &[u8],
    value_wei: u128,
    gas: u64,
) -> crate::ceremony::SignatureIntent {
    let raw = serde_json::json!({
        "from": from,
        "to": to,
        "value": format!("0x{value_wei:x}"),
        "data": format!("0x{}", hex::encode(data)),
        "gas": format!("0x{gas:x}"),
        "chainId": format!("0x{:x}", crate::rpc::CITRATE_CHAIN_ID),
    })
    .to_string();
    crate::ceremony::SignatureIntent {
        origin: READER_ORIGIN.to_string(),
        kind: crate::ceremony::IntentKind::Transaction,
        chain_id: crate::rpc::CITRATE_CHAIN_ID,
        raw,
    }
}

fn client_for(target: &ReadTarget) -> crate::rpc::RpcClient<crate::rpc::HttpTransport> {
    crate::rpc::RpcClient::with_transport(crate::rpc::HttpTransport::new(target.rpc_url()))
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

/// **contract_source** — CitrateScan's verified source and ABI for an address.
#[tauri::command]
pub async fn contract_source(address: String) -> std::result::Result<VerifiedSource, String> {
    crate::blocking::off_main(move || contract_source_sync(address)).await
}

/// Blocking body of [`contract_source`]; reached only through [`crate::blocking::off_main`].
pub fn contract_source_sync(address: String) -> std::result::Result<VerifiedSource, String> {
    fetch_verified_source(&UreqExplorer, &address)
}

/// **contract_code_size** — the deployed code size at `address` on the read target (0 = none).
#[tauri::command]
pub async fn contract_code_size(
    target: Option<String>,
    address: String,
) -> std::result::Result<usize, String> {
    crate::blocking::off_main(move || contract_code_size_sync(target, address)).await
}

/// Blocking body of [`contract_code_size`]; reached only through [`crate::blocking::off_main`].
pub fn contract_code_size_sync(
    target: Option<String>,
    address: String,
) -> std::result::Result<usize, String> {
    let target = parse_target(target.as_deref())?;
    let address = normalize_address(&address)?;
    code_size(&client_for(&target), &address)
}

/// **contract_view_call** — a read-only `eth_call`; returns the raw `0x` return data.
#[tauri::command]
pub async fn contract_view_call(
    target: Option<String>,
    address: String,
    calldata: String,
) -> std::result::Result<String, String> {
    crate::blocking::off_main(move || contract_view_call_sync(target, address, calldata)).await
}

/// Blocking body of [`contract_view_call`]; reached only through [`crate::blocking::off_main`].
pub fn contract_view_call_sync(
    target: Option<String>,
    address: String,
    calldata: String,
) -> std::result::Result<String, String> {
    let target = parse_target(target.as_deref())?;
    let address = normalize_address(&address)?;
    let data = parse_calldata(&calldata)?;
    view_call(&client_for(&target), &address, &data)
}

/// **contract_write_propose** — open a PENDING SignatureCeremony for a write call on 40204. The
/// member reviews and approves it in the ceremony (Rule 3); nothing signs here.
#[tauri::command]
pub async fn contract_write_propose(
    app_h: tauri::AppHandle,
    address: String,
    calldata: String,
    value_wei: Option<String>,
) -> std::result::Result<crate::ceremony::CeremonyView, String> {
    crate::blocking::off_main(move || {
        let custody = tauri::Manager::try_state::<crate::custody::CustodyState>(&app_h)
            .ok_or_else(|| "internal: managed state unavailable".to_string())?;
        let ceremony = tauri::Manager::try_state::<crate::ceremony::CeremonyState>(&app_h)
            .ok_or_else(|| "internal: managed state unavailable".to_string())?;
        contract_write_propose_sync(custody, ceremony, address, calldata, value_wei)
    })
    .await
}

/// Blocking body of [`contract_write_propose`]; reached only through [`crate::blocking::off_main`].
pub fn contract_write_propose_sync(
    custody: tauri::State<'_, crate::custody::CustodyState>,
    ceremony: tauri::State<'_, crate::ceremony::CeremonyState>,
    address: String,
    calldata: String,
    value_wei: Option<String>,
) -> std::result::Result<crate::ceremony::CeremonyView, String> {
    let to = normalize_address(&address)?;
    let data = parse_calldata(&calldata)?;
    let value = parse_value_wei(value_wei.as_deref())?;
    let wallet = crate::wallet::address_auto_unlocked(&custody.0).map_err(|e| e.to_string())?;
    let client = crate::rpc::RpcClient::citrate();
    let gas = estimate_write_gas(&client, &wallet.address, &to, &data, value)?;
    Ok(ceremony
        .0
        .request(write_intent(&wallet.address, &to, &data, value, gas)))
}

#[cfg(test)]
mod tests {
    include!("contract_reader_tests.rs");
}
