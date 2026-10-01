//! HUP-S4.3 — `get_verified_source`: a contract's verified source, ABI, and compiler metadata,
//! read from CitrateScan. READ-ONLY: a public GET, no key, nothing signed, nothing sent on-chain.
//!
//! ## Data source (Rule 1, named)
//! CitrateScan's verified-source lookup (citrate-explorer `src/lib/verify/verifiedSource.ts`):
//!
//!   `GET {EXPLORER_BASE}/api/contract/{address}/source?maxSourceChars=N`
//!
//! backed by the explorer's recompile-and-diff records (`contract_verifications`). An explorer
//! deployment that predates that route answers 404; core then reads the `verification` object of
//! the contract page route, `GET {EXPLORER_BASE}/api/contract/{address}`, which carries the same
//! facts for verified contracts.
//!
//! ## Honest statuses
//! - `verified`: full match (metadata included). The ONLY status with `verified: true`.
//! - `partial-match`: matches only after stripping metadata; source shown, NOT verified.
//! - `unverified`: no verified source (or no contract code at the address).
//! - `unavailable`: the explorer's verification store could not be read; unknown, not "unverified".
//!
//! A status the explorer sends that core does not know is an error, never coerced.

use serde::Serialize;
use serde_json::Value;

/// Longest source handed to the agent (chars). The explorer is asked for the same bound.
pub const MAX_SOURCE_CHARS: usize = 60_000;
/// Largest response body read from the explorer.
const MAX_BODY_BYTES: u64 = 2 * 1024 * 1024;
const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

/// One HTTP answer (status + body).
pub struct ScanReply {
    pub status: u16,
    pub body: String,
}

/// The single GET this module needs; tests script it.
pub trait ScanHttp {
    fn get(&self, url: &str) -> Result<ScanReply, String>;
}

/// Production: blocking `ureq` (rustls), non-2xx returned as a status, bounded body.
pub struct UreqScan;

impl ScanHttp for UreqScan {
    fn get(&self, url: &str) -> Result<ScanReply, String> {
        let resp = ureq::get(url)
            .config()
            .timeout_global(Some(TIMEOUT))
            .http_status_as_error(false)
            .max_redirects(0)
            .build()
            .call()
            .map_err(|e| format!("CitrateScan request failed: {e}"))?;
        let status = resp.status().as_u16();
        let body = resp
            .into_body()
            .with_config()
            .limit(MAX_BODY_BYTES)
            .read_to_string()
            .map_err(|e| format!("CitrateScan response unreadable: {e}"))?;
        Ok(ScanReply { status, body })
    }
}

/// What the agent tool returns.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct VerifiedSource {
    pub address: String,
    /// verified | partial-match | unverified | unavailable
    pub status: String,
    pub verified: bool,
    pub match_type: Option<String>,
    pub contract_name: Option<String>,
    pub compiler_version: Option<String>,
    pub source_hash: Option<String>,
    pub source: Option<String>,
    pub source_truncated: bool,
    pub abi: Option<Value>,
    pub verified_at: Option<String>,
    pub note: String,
}

const KNOWN_STATUSES: &[&str] = &["verified", "partial-match", "unverified", "unavailable"];

fn validate_address(addr: &str) -> Result<String, String> {
    let hex = addr
        .strip_prefix("0x")
        .ok_or_else(|| "expected a 0x-prefixed 20-byte address".to_string())?;
    if hex.len() != 40 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("expected a 0x-prefixed 20-byte address".into());
    }
    Ok(format!("0x{}", hex.to_ascii_lowercase()))
}

fn str_field(v: &Value, k: &str) -> Option<String> {
    v.get(k).and_then(Value::as_str).map(str::to_string)
}

fn truncate_chars(s: String) -> (String, bool) {
    match s.char_indices().nth(MAX_SOURCE_CHARS) {
        Some((byte, _)) => (s[..byte].to_string(), true),
        None => (s, false),
    }
}

fn default_note(status: &str) -> &'static str {
    match status {
        "verified" => "Verified: the recompiled bytecode matches the deployed code exactly.",
        "partial-match" => "Partial match only: the shown source may differ from what was deployed. This is not a verified match.",
        "unavailable" => "CitrateScan's verification store is unavailable, so it is unknown whether this contract is verified.",
        _ => "Not verified: no verified source is recorded for this address. Do not assume what its code does.",
    }
}

/// Shape a verification object (either route) into the tool result. Fails on an unknown status.
fn shape(address: &str, v: &Value, explorer_truncated: bool) -> Result<VerifiedSource, String> {
    let status = v
        .get("status")
        .and_then(Value::as_str)
        .ok_or_else(|| "CitrateScan answered without a verification status".to_string())?;
    if !KNOWN_STATUSES.contains(&status) {
        // The remote string is not echoed: error text reaches the model outside the fence.
        return Err("CitrateScan sent a verification status this app does not know".to_string());
    }
    let flag = v.get("verified").and_then(Value::as_bool) == Some(true);
    if status == "verified" && !flag {
        return Err("CitrateScan sent a contradictory verification answer".to_string());
    }
    let verified = status == "verified" && flag;
    let (source, core_truncated) = match str_field(v, "source")
        .filter(|_| status != "unverified" && status != "unavailable")
    {
        Some(s) => {
            let (s, t) = truncate_chars(s);
            (Some(s), t)
        }
        None => (None, false),
    };
    let abi = v
        .get("abi")
        .filter(|a| a.is_array())
        .cloned()
        .filter(|_| status == "verified" || status == "partial-match");
    let note = str_field(v, "note")
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| default_note(status).to_string());
    Ok(VerifiedSource {
        address: address.to_string(),
        status: status.to_string(),
        verified,
        match_type: str_field(v, "matchType"),
        contract_name: str_field(v, "contractName"),
        compiler_version: str_field(v, "compilerVersion"),
        source_hash: str_field(v, "sourceHash"),
        source,
        source_truncated: core_truncated || explorer_truncated,
        abi,
        verified_at: str_field(v, "verifiedAt"),
        note,
    })
}

fn http_error(status: u16) -> String {
    if status == 429 {
        "CitrateScan rate limit reached; try again in a moment".to_string()
    } else {
        format!("CitrateScan answered HTTP {status}")
    }
}

fn parse_json(body: &str) -> Result<Value, String> {
    serde_json::from_str(body)
        .map_err(|_| "CitrateScan sent a response that is not JSON".to_string())
}

/// Look up the verified source for `address` on the explorer at `base`.
pub fn lookup(http: &dyn ScanHttp, base: &str, address: &str) -> Result<VerifiedSource, String> {
    let addr = validate_address(address)?;
    let base = base.trim_end_matches('/');
    let primary = http.get(&format!(
        "{base}/api/contract/{addr}/source?maxSourceChars={MAX_SOURCE_CHARS}"
    ))?;
    match primary.status {
        200 => {
            let v = parse_json(&primary.body)?;
            let truncated = v.get("sourceTruncated").and_then(Value::as_bool) == Some(true);
            shape(&addr, &v, truncated)
        }
        404 => {
            // An explorer deployment without the source route: read the contract page route.
            let fallback = http.get(&format!("{base}/api/contract/{addr}"))?;
            if fallback.status != 200 {
                return Err(http_error(fallback.status));
            }
            let v = parse_json(&fallback.body)?;
            if v.get("isContract").and_then(Value::as_bool) == Some(false) {
                let mut out = shape(&addr, &serde_json::json!({"status": "unverified"}), false)?;
                out.note = "Not verified: there is no contract code at this address (an externally owned account).".into();
                return Ok(out);
            }
            let ver = v.get("verification").ok_or_else(|| {
                "CitrateScan's contract answer has no verification field".to_string()
            })?;
            shape(&addr, ver, false)
        }
        s => Err(http_error(s)),
    }
}

/// The agent tool's command: `get_verified_source(address)` against the pinned explorer host.
#[tauri::command]
pub async fn contract_verified_source(address: String) -> Result<VerifiedSource, String> {
    crate::blocking::off_main(move || contract_verified_source_sync(address)).await
}

/// Blocking body of [`contract_verified_source`]; reached only through [`crate::blocking::off_main`].
pub fn contract_verified_source_sync(address: String) -> Result<VerifiedSource, String> {
    lookup(&UreqScan, crate::activity::EXPLORER_BASE, &address)
}

#[cfg(test)]
mod tests {
    include!("verified_source_tests.rs");
}
