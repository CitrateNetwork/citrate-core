//! citrate-core — real wallet activity (tx history) from CitrateScan (CORE item 4).
//!
//! NOT @rule8: this is a PUBLIC, no-auth, read-only tx-history fetch. It handles
//! no key/seed/entropy, signs nothing, and moves no money — it reads the member's
//! own indexed 40204 transaction history and shapes it for the Wallet Activity tab.
//!
//! ## Data source (Rule 1 — named)
//! The **CitrateScan `txlist` endpoint** — the Etherscan-compatible public REST
//! surface on the explorer:
//!
//!   `GET https://explorer.citrate.ai/api/v1?module=account&action=txlist&address=0x..&limit=..`
//!
//! (source: `citrate-explorer/src/app/api/v1/route.ts` — `module=account`,
//! `action=txlist`, backed by `searchTransactions()` over the Neon chain index in
//! `citrate-explorer/src/lib/indexer/repository.ts`). The endpoint allows
//! ANONYMOUS access (no API key) at a low per-IP rate (2/sec); we send no key.
//!
//! ## The response contract (VERIFIED against explorer source, 2026-07-16)
//! The Etherscan envelope: `{ "status": "1"|"0", "message": <str>, "result": [...] }`.
//!  - `status:"1"` + `result:[...]` — the tx rows (newest first, `desc(timestamp)`).
//!  - `status:"0"` + `result:[]` — "No transactions found" OR "No transactions found
//!    (indexer not provisioned)". BOTH are HONEST EMPTY, not errors (Rule 1). We
//!    return an empty list, never a fabricated tx.
//!  - `status:"0"` + `result:null` — a hard failure (e.g. "invalid address"). We
//!    only call with a pre-validated address, so this is unexpected → an error.
//!
//! Each tx row is a raw Neon `transactions` row serialized by drizzle (source:
//! `citrate-explorer/src/lib/db/schema.ts`), with these fields we consume:
//!   - `hash`          (string, `0x…`)
//!   - `from`          (string, lowercased 0x address)
//!   - `to`            (string|null; null ⇒ contract creation)
//!   - `value`         (string of wei — the native SALT value, `default "0"`)
//!   - `status`        (int|null — 1 success, 0 failed, null pending)
//!   - `methodId`      (string|null — first 4 bytes of input, e.g. "0xa9059cbb")
//!   - `createdContract` (string|null — the deployed contract, for creations)
//!   - `timestamp`     (number|null — UNIX **SECONDS**: the block timestamp,
//!     `hexToNum(raw.timestamp)`; source: rpc.ts:101). We convert to epoch **MS**
//!     for the JS `rel()` helper, which does `Date.now() - ts` (source: core
//!     src/surfaces/Wallet.tsx:20).
//!
//! ## Honesty (Rule 1)
//! A fresh address / not-provisioned index → EMPTY list (with `indexerUnavailable`
//! set on the not-provisioned case so the UI can distinguish "no txs yet" from
//! "indexer syncing"). A transport/HTTP failure → `Err` (the store keeps its last
//! honest list). We NEVER fabricate a transaction.
//!
//! ## Testability
//! The single HTTP GET is behind an injectable [`ActivityHttpClient`] seam (mirrors
//! `ai.rs`'s `AiHttpClient` / `rpc.rs`'s `RpcTransport`), so tests script exact
//! response bodies with NO live socket (CI-safe). Production wires
//! [`UreqActivityClient`] (blocking `ureq`, rustls — the same transport the OIDC/AI
//! clients use).

// Activity seam: the ureq client only runs in a Tauri build; tests drive the mock
// seam. Allow dead_code on the production-only client surface (mirrors ai.rs/rpc.rs).
#![allow(dead_code)]

use serde::Serialize;
use serde_json::Value;

/// The canonical CitrateScan (explorer) host. Pinned per the spec — the tx-history
/// read always targets the public production explorer, never a webview-supplied URL.
pub const EXPLORER_BASE: &str = "https://explorer.citrate.ai";

/// How many recent transactions to request (the explorer caps `txlist` at 100; we
/// ask for a page that comfortably fills the Activity tab).
const TX_LIMIT: u32 = 25;

// ---------------------------------------------------------------------------
// Errors — coarse + secret-free (this module never sees key material: it does a
// public GET + shapes the public response). A failed fetch is honest, not fatal.
// ---------------------------------------------------------------------------

/// An error from the wallet-activity read. Every `Display` string is safe to
/// surface: this path handles no secret, so no variant can carry one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActivityError {
    /// The address was not a `0x`-prefixed 20-byte hex string (so we refuse to
    /// query the indexer with a malformed argument — fail closed).
    BadAddress(String),
    /// A network/transport error reaching the explorer (connection, TLS, non-2xx).
    Http(String),
    /// The response body was not the expected Etherscan envelope JSON.
    BadResponse(String),
}

impl std::fmt::Display for ActivityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ActivityError::BadAddress(m) => write!(f, "activity: bad wallet address: {m}"),
            ActivityError::Http(m) => write!(f, "activity: explorer request failed: {m}"),
            ActivityError::BadResponse(m) => write!(f, "activity: bad explorer response: {m}"),
        }
    }
}

impl std::error::Error for ActivityError {}

type Result<T> = std::result::Result<T, ActivityError>;

// ---------------------------------------------------------------------------
// The Activity entry crossing the invoke boundary — the SAME shape the JS
// `Activity {id, kind, amount, hash, ts}` renders, plus `status`/`direction` so
// the UI can mark failed txs + colour the sign. serde camelCase.
// ---------------------------------------------------------------------------

/// One row for the Wallet Activity tab, mapped from a CitrateScan `txlist` tx.
/// Every field is derived from the indexed on-chain tx (Rule 1 — never fabricated).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ActivityEntry {
    /// Stable id for the row — the tx hash (also used to DEDUPE against an
    /// optimistic just-sent entry on the JS side).
    pub id: String,
    /// A short human label: "Sent" / "Received" / "Self" / "Contract call" /
    /// "Contract creation". We never fabricate a decoded method name.
    pub kind: String,
    /// The signed SALT amount string, e.g. `"+12.41 SALT"` / `"−40.00 SALT"`
    /// (U+2212 MINUS for outgoing, matching the Wallet tab's sign convention). A
    /// zero-value tx carries no sign.
    pub amount: String,
    /// The transaction hash (`0x…`).
    pub hash: String,
    /// The tx timestamp as epoch **milliseconds** (the JS `rel()` helper does
    /// `Date.now() - ts`). `0` when the indexer had no timestamp for the row.
    pub ts: u64,
    /// The receipt status passthrough: `1` success, `0` failed, `null` pending —
    /// so the UI can mark a failed tx distinctly.
    pub status: Option<i64>,
    /// "in" (to == self), "out" (from == self), or "self" (both). Drives the sign.
    pub direction: String,
}

// ---------------------------------------------------------------------------
// HTTP seam — real `ureq` GET in production, injectable for tests (mirrors
// ai.rs's `AiHttpClient`). The seam takes the fully-built URL so a test transport
// can assert the exact request WITHOUT a live socket.
// ---------------------------------------------------------------------------

/// The single HTTP op this module needs: GET a URL, return the response body
/// string. Abstracted so tests inject a mock and assert the request shape (the
/// txlist URL) with no real network.
pub trait ActivityHttpClient {
    /// `GET url` and return the response body as a string, or a coarse
    /// [`ActivityError::Http`].
    fn get(&self, url: &str) -> Result<String>;
}

/// Production HTTP client: blocking `ureq` (rustls TLS), the same transport as the
/// OIDC / AI clients.
pub struct UreqActivityClient;

impl ActivityHttpClient for UreqActivityClient {
    fn get(&self, url: &str) -> Result<String> {
        let mut resp = ureq::get(url)
            .call()
            .map_err(|e| ActivityError::Http(e.to_string()))?;
        resp.body_mut()
            .read_to_string()
            .map_err(|e| ActivityError::Http(e.to_string()))
    }
}

// ---------------------------------------------------------------------------
// Address validation + amount formatting
// ---------------------------------------------------------------------------

/// Validate a `0x`-prefixed 20-byte hex address; returns the lowercased canonical
/// form (the index stores addresses lowercased, so we compare in that space).
fn validate_address(addr: &str) -> Result<String> {
    let stripped = addr.strip_prefix("0x").unwrap_or(addr);
    if stripped.len() != 40 || !stripped.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(ActivityError::BadAddress(addr.to_string()));
    }
    Ok(format!("0x{}", stripped.to_ascii_lowercase()))
}

/// Format a wei amount (decimal string) as a fixed-2-decimal SALT string with
/// thousands separators and a leading sign, e.g. `"+12.41 SALT"` / `"−40.00 SALT"`
/// (U+2212 for outgoing, matching the Wallet tab). A zero value carries NO sign.
/// `outgoing` is true for a "out"/"self" direction (funds leave the wallet).
///
/// SALT has 18 decimals ("grains"). We do the wei→SALT split on the decimal
/// string directly (no float), so a large value never loses precision.
fn format_salt_amount(value_wei: &str, outgoing: bool) -> String {
    // Parse to u128 (SALT amounts fit; a malformed/huge value falls back to a raw
    // wei render rather than fabricating a number).
    let wei: u128 = match value_wei.trim().parse() {
        Ok(w) => w,
        Err(_) => return format!("{value_wei} wei"),
    };
    let whole = wei / 10u128.pow(18);
    let frac = wei % 10u128.pow(18);
    // Two-decimal rounding: take the top 2 fractional digits (round half up).
    let hundredths_full = frac / 10u128.pow(16); // 0..=99 (truncated)
    let remainder = frac % 10u128.pow(16);
    let mut whole = whole;
    let mut hundredths = hundredths_full;
    // Round half up on the third decimal.
    if remainder >= 5 * 10u128.pow(15) {
        hundredths += 1;
        if hundredths == 100 {
            hundredths = 0;
            whole += 1;
        }
    }
    let sign = if wei == 0 {
        ""
    } else if outgoing {
        "\u{2212}" // U+2212 MINUS SIGN (matches Wallet.tsx's outgoing marker)
    } else {
        "+"
    };
    format!("{sign}{} SALT", group_thousands(whole, hundredths))
}

/// Render `whole.hundredths` with thousands separators on the integer part,
/// e.g. `(2500, 0) → "2,500.00"`. Grouping is computed from the RIGHT so it is
/// correct for any length (no fragile index arithmetic).
fn group_thousands(whole: u128, hundredths: u128) -> String {
    let digits = whole.to_string();
    let n = digits.len();
    let mut grouped = String::with_capacity(n + n / 3);
    for (i, ch) in digits.chars().enumerate() {
        // A separator precedes a digit whose count-from-the-right is a nonzero
        // multiple of 3 (and it is not the leading digit).
        let from_right = n - i;
        if i != 0 && from_right.is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(ch);
    }
    format!("{grouped}.{hundredths:02}")
}

// ---------------------------------------------------------------------------
// Envelope parsing + tx → ActivityEntry mapping
// ---------------------------------------------------------------------------

/// Classify one indexed tx into a `(kind, direction, outgoing)` triple, given the
/// self address. `to == null` ⇒ a contract creation. A non-trivial `methodId`
/// (present, not "0x") on a call to another address ⇒ "Contract call" (we do NOT
/// fabricate a decoded method name — Rule 1). Otherwise Sent/Received/Self.
fn classify(
    from: &str,
    to: Option<&str>,
    method_id: Option<&str>,
    self_addr: &str,
) -> (String, String, bool) {
    let from_self = from.eq_ignore_ascii_case(self_addr);
    let to_self = to.is_some_and(|t| t.eq_ignore_ascii_case(self_addr));

    // Contract creation (no `to`).
    if to.is_none() {
        return ("Contract creation".to_string(), "out".to_string(), true);
    }

    let has_method = method_id
        .map(|m| !m.is_empty() && m != "0x")
        .unwrap_or(false);

    let direction = if from_self && to_self {
        "self"
    } else if from_self {
        "out"
    } else {
        "in"
    };
    let outgoing = direction != "in";

    // A contract call FROM us (calldata present) — keep it honest + simple.
    if has_method && from_self && !to_self {
        return ("Contract call".to_string(), direction.to_string(), outgoing);
    }

    let kind = match direction {
        "self" => "Self",
        "out" => "Sent",
        _ => "Received",
    };
    (kind.to_string(), direction.to_string(), outgoing)
}

/// Map one raw explorer tx JSON object into an [`ActivityEntry`] for `self_addr`.
/// A row missing its `hash` is skipped (returns `None`) rather than fabricating an
/// id — every entry must trace to a real indexed tx (Rule 1).
fn map_tx(tx: &Value, self_addr: &str) -> Option<ActivityEntry> {
    let hash = tx.get("hash").and_then(Value::as_str)?.to_string();
    let from = tx.get("from").and_then(Value::as_str).unwrap_or("");
    let to = tx.get("to").and_then(Value::as_str);
    let value_wei = tx.get("value").and_then(Value::as_str).unwrap_or("0");
    let method_id = tx.get("methodId").and_then(Value::as_str);
    // status: an integer (1/0) or null. Passed through so the UI marks failed txs.
    let status = tx.get("status").and_then(Value::as_i64);
    // timestamp: UNIX SECONDS from the index → epoch MS for the JS rel() helper.
    let ts_secs = tx.get("timestamp").and_then(Value::as_u64).unwrap_or(0);
    let ts = ts_secs.saturating_mul(1000);

    let (kind, direction, outgoing) = classify(from, to, method_id, self_addr);
    let amount = format_salt_amount(value_wei, outgoing);

    Some(ActivityEntry {
        id: hash.clone(),
        kind,
        amount,
        hash,
        ts,
        status,
        direction,
    })
}

/// The parsed outcome of a `txlist` fetch: the mapped entries + whether the
/// indexer reported itself NOT provisioned (so the UI can show an honest
/// "indexer syncing" state distinct from "no transactions yet").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivityResult {
    pub entries: Vec<ActivityEntry>,
    pub indexer_unavailable: bool,
}

/// Parse a CitrateScan `txlist` envelope body into entries. HONEST handling:
///  - `status:"1"` + array `result` → map each row (skip a row missing a hash).
///  - `status:"0"` (no records / not provisioned) with `result:[]` or absent →
///    an EMPTY list; `indexer_unavailable` reflects the "not provisioned" message.
///  - `status:"0"` + `result:null` (a hard fail on a validated address) → an
///    error (we never fabricate). A non-JSON body → an error.
fn parse_txlist(body: &str, self_addr: &str) -> Result<ActivityResult> {
    let v: Value =
        serde_json::from_str(body).map_err(|e| ActivityError::BadResponse(e.to_string()))?;

    let status = v.get("status").and_then(Value::as_str);
    let message = v.get("message").and_then(Value::as_str).unwrap_or("");
    let result = v.get("result");

    // The honest "no records" path: status "0" with an empty/absent array result.
    // The explorer returns `noData(...)` as `{status:"0", message, result:[]}`.
    if status == Some("0") {
        match result {
            Some(Value::Array(arr)) if arr.is_empty() => {
                // "No transactions found (indexer not provisioned)" vs "No
                // transactions found" — flag the not-provisioned case for the UI.
                let indexer_unavailable = message.contains("not provisioned");
                return Ok(ActivityResult {
                    entries: Vec::new(),
                    indexer_unavailable,
                });
            }
            // result:null on a validated address is a real failure, not "empty".
            Some(Value::Null) | None => {
                return Err(ActivityError::BadResponse(format!(
                    "explorer returned failure: {message}"
                )));
            }
            // Any other status-0 shape with a populated array is unexpected but
            // harmless — fall through to map whatever rows are present.
            _ => {}
        }
    }

    // The success path (status "1") OR a status-0 with an unexpectedly populated
    // array: map the rows. A non-array result at this point is malformed.
    let arr = result
        .and_then(Value::as_array)
        .ok_or_else(|| ActivityError::BadResponse("result is not an array".to_string()))?;

    let entries = arr
        .iter()
        .filter_map(|tx| map_tx(tx, self_addr))
        .collect::<Vec<_>>();

    Ok(ActivityResult {
        entries,
        indexer_unavailable: false,
    })
}

/// Build the txlist request URL for `self_addr` (already validated + lowercased).
fn txlist_url(self_addr: &str) -> String {
    format!(
        "{EXPLORER_BASE}/api/v1?module=account&action=txlist&address={self_addr}&limit={TX_LIMIT}"
    )
}

/// **The real activity read.** Validate the address, GET the CitrateScan `txlist`
/// endpoint over `http`, and parse the envelope into entries. Honest empty on a
/// fresh/not-provisioned index; `Err` on transport/parse failure. `http` is
/// injected so tests script the response with no live socket (Rule 1: the mock is
/// a TEST transport, never the default).
pub fn read_activity<H: ActivityHttpClient>(http: &H, address: &str) -> Result<ActivityResult> {
    let self_addr = validate_address(address)?;
    let url = txlist_url(&self_addr);
    let body = http.get(&url)?;
    parse_txlist(&body, &self_addr)
}

// ---------------------------------------------------------------------------
// Tauri command — the Wallet Activity tab's real tx-history read. Returns the
// mapped entries (empty on a fresh/not-provisioned index). Requires the vault
// UNLOCKED only to read the wallet's PUBLIC address (the key is never touched).
// NOT @rule8: a public read, no signing, no money.
// ---------------------------------------------------------------------------

/// **Command — wallet_activity.** Read the member's REAL indexed 40204 tx history
/// from the CitrateScan `txlist` endpoint (data source named in this module's
/// docs, Rule 1) and shape it for the Wallet Activity tab. Requires the vault
/// unlocked (to read the wallet's public address; the key is never touched). A
/// fresh address / not-provisioned index returns an EMPTY list (honest, not an
/// error, never fabricated); a transport/parse failure returns an error so the
/// store keeps its last honest list.
#[tauri::command]
pub fn wallet_activity(
    custody: tauri::State<'_, crate::custody::CustodyState>,
) -> std::result::Result<Vec<ActivityEntry>, String> {
    let wallet = crate::wallet::address(&custody.0).map_err(|e| e.to_string())?;
    let http = UreqActivityClient;
    read_activity(&http, &wallet.address)
        .map(|r| r.entries)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    include!("activity_tests.rs");
}
