// CORE item 4 — real wallet activity (tx history) from CitrateScan.
//
// Red-first. Covers, over a MOCKED HTTP seam (no live socket — CI-safe):
//   * the exact txlist URL shape (module=account&action=txlist&address=&limit=);
//   * the VERIFIED Etherscan envelope parse ({status, message, result:[...]}) with
//     the real drizzle tx-row field names (hash/from/to/value/status/methodId/
//     timestamp/createdContract);
//   * direction + kind classification (out/in/self, Sent/Received/Self, Contract
//     call, Contract creation) — no fabricated method name;
//   * the wei→SALT signed amount (U+2212 for outgoing, + for incoming, no sign for
//     zero) with 2-decimal + thousands grouping;
//   * the timestamp SECONDS→MS conversion the JS rel() helper needs;
//   * HONEST empty on status:"0" "No records" AND the not-provisioned flag;
//   * an error (never a fabricated tx) on result:null / non-JSON / transport fail;
//   * a bad address fails closed BEFORE any HTTP call.
//
// A documented #[ignore] live proof hits the real explorer at the bottom.

use super::*;
use std::cell::RefCell;

// ---------------------------------------------------------------------------
// A scripted mock HTTP client (mirrors ai_tests' mock http + rpc_tests' MockRpc).
// ---------------------------------------------------------------------------

struct MockHttp {
    urls: RefCell<Vec<String>>,
    response: std::result::Result<String, ActivityError>,
}
impl MockHttp {
    fn ok(body: &str) -> Self {
        MockHttp {
            urls: RefCell::new(Vec::new()),
            response: Ok(body.to_string()),
        }
    }
    fn err(e: ActivityError) -> Self {
        MockHttp {
            urls: RefCell::new(Vec::new()),
            response: Err(e),
        }
    }
    fn urls(&self) -> Vec<String> {
        self.urls.borrow().clone()
    }
}
impl ActivityHttpClient for MockHttp {
    fn get(&self, url: &str) -> Result<String> {
        self.urls.borrow_mut().push(url.to_string());
        self.response.clone()
    }
}

const SELF_ADDR: &str = "0x9858effd232b4033e47d90003d41ec34ecaeda94";
const OTHER_ADDR: &str = "0x1111111111111111111111111111111111111111";

/// Build a txlist envelope body with the given tx rows (real drizzle field names).
fn envelope(status: &str, message: &str, result: JsonRows) -> String {
    match result {
        JsonRows::Array(rows) => serde_json::json!({
            "status": status, "message": message, "result": rows
        })
        .to_string(),
        JsonRows::Null => serde_json::json!({
            "status": status, "message": message, "result": Value::Null
        })
        .to_string(),
    }
}
enum JsonRows {
    Array(Vec<Value>),
    Null,
}

fn tx(from: &str, to: Option<&str>, value: &str, ts_secs: u64, status: Option<i64>, method: Option<&str>) -> Value {
    let mut o = serde_json::Map::new();
    // hash is deterministic-ish from the args so rows are distinct.
    o.insert("hash".into(), Value::String(format!("0x{:064x}", ts_secs.wrapping_add(value.len() as u64))));
    o.insert("from".into(), Value::String(from.to_string()));
    o.insert("to".into(), to.map(|t| Value::String(t.to_string())).unwrap_or(Value::Null));
    o.insert("value".into(), Value::String(value.to_string()));
    o.insert("timestamp".into(), Value::from(ts_secs));
    o.insert("status".into(), status.map(Value::from).unwrap_or(Value::Null));
    o.insert("methodId".into(), method.map(|m| Value::String(m.to_string())).unwrap_or(Value::Null));
    Value::Object(o)
}

// ===========================================================================
// URL shape + address validation
// ===========================================================================

/// The read GETs the CitrateScan txlist endpoint with the validated (lowercased)
/// address and the account/txlist module/action — the VERIFIED public route.
#[test]
fn reads_the_txlist_endpoint_with_the_self_address() {
    let http = MockHttp::ok(&envelope("1", "OK", JsonRows::Array(vec![])));
    let _ = read_activity(&http, SELF_ADDR).expect("read");
    let urls = http.urls();
    assert_eq!(urls.len(), 1);
    let url = &urls[0];
    assert!(url.starts_with("https://explorer.citrate.ai/api/v1?"), "explorer base: {url}");
    assert!(url.contains("module=account"), "module: {url}");
    assert!(url.contains("action=txlist"), "action: {url}");
    assert!(url.contains(&format!("address={SELF_ADDR}")), "address: {url}");
    assert!(url.contains("limit="), "limit: {url}");
}

/// A malformed address fails closed BEFORE any HTTP call (never queries the
/// indexer with a bad arg).
#[test]
fn bad_address_fails_closed_without_http() {
    let http = MockHttp::ok(&envelope("1", "OK", JsonRows::Array(vec![])));
    let r = read_activity(&http, "not-an-address");
    assert!(matches!(r, Err(ActivityError::BadAddress(_))), "got: {r:?}");
    assert_eq!(http.urls().len(), 0, "no HTTP call on a bad address");
}

/// A mixed-case address is accepted and lowercased in the query (the index stores
/// addresses lowercased).
#[test]
fn address_is_lowercased_in_the_query() {
    let mixed = "0x9858EFFD232B4033E47D90003D41EC34ECAEDA94";
    let http = MockHttp::ok(&envelope("1", "OK", JsonRows::Array(vec![])));
    let _ = read_activity(&http, mixed).expect("read");
    assert!(http.urls()[0].contains(&format!("address={SELF_ADDR}")));
}

// ===========================================================================
// Envelope parse + direction/kind/amount mapping
// ===========================================================================

/// An outgoing native transfer (from == self) maps to "Sent" / "out" with a U+2212
/// signed SALT amount and the timestamp scaled seconds→ms.
#[test]
fn maps_an_outgoing_transfer() {
    let forty_salt = "40000000000000000000"; // 40 SALT in wei
    let body = envelope("1", "OK", JsonRows::Array(vec![
        tx(SELF_ADDR, Some(OTHER_ADDR), forty_salt, 1_700_000_000, Some(1), None),
    ]));
    let res = read_activity(&MockHttp::ok(&body), SELF_ADDR).expect("read");
    assert_eq!(res.entries.len(), 1);
    let e = &res.entries[0];
    assert_eq!(e.kind, "Sent");
    assert_eq!(e.direction, "out");
    assert_eq!(e.amount, "\u{2212}40.00 SALT", "outgoing uses U+2212 and 2 decimals");
    assert_eq!(e.status, Some(1));
    assert_eq!(e.ts, 1_700_000_000_000, "seconds → epoch ms for rel()");
    assert_eq!(e.id, e.hash, "id is the tx hash (dedupe key)");
}

/// An incoming transfer (to == self) maps to "Received" / "in" with a "+" sign.
#[test]
fn maps_an_incoming_transfer() {
    let body = envelope("1", "OK", JsonRows::Array(vec![
        tx(OTHER_ADDR, Some(SELF_ADDR), "12410000000000000000", 1_700_000_500, Some(1), None),
    ]));
    let res = read_activity(&MockHttp::ok(&body), SELF_ADDR).expect("read");
    let e = &res.entries[0];
    assert_eq!(e.kind, "Received");
    assert_eq!(e.direction, "in");
    assert_eq!(e.amount, "+12.41 SALT");
}

/// A self-send (from == to == self) maps to "Self" / "self" and is treated as
/// outgoing for the sign.
#[test]
fn maps_a_self_transfer() {
    let body = envelope("1", "OK", JsonRows::Array(vec![
        tx(SELF_ADDR, Some(SELF_ADDR), "5000000000000000000", 1_700_000_900, Some(1), None),
    ]));
    let e = &read_activity(&MockHttp::ok(&body), SELF_ADDR).expect("read").entries[0];
    assert_eq!(e.kind, "Self");
    assert_eq!(e.direction, "self");
    assert!(e.amount.starts_with('\u{2212}'));
}

/// A contract call FROM us (calldata present, distinct `to`) maps to "Contract
/// call" — we do NOT fabricate a decoded method name (Rule 1).
#[test]
fn maps_a_contract_call_without_fabricating_a_method() {
    let body = envelope("1", "OK", JsonRows::Array(vec![
        // methodId present (an ERC-20 transfer selector), value 0.
        tx(SELF_ADDR, Some(OTHER_ADDR), "0", 1_700_001_000, Some(1), Some("0xa9059cbb")),
    ]));
    let e = &read_activity(&MockHttp::ok(&body), SELF_ADDR).expect("read").entries[0];
    assert_eq!(e.kind, "Contract call");
    assert_eq!(e.direction, "out");
    // Zero value → no sign at all.
    assert_eq!(e.amount, "0.00 SALT");
}

/// A contract creation (`to` == null) maps to "Contract creation" / "out".
#[test]
fn maps_a_contract_creation() {
    let body = envelope("1", "OK", JsonRows::Array(vec![
        tx(SELF_ADDR, None, "0", 1_700_001_100, Some(1), Some("0x60806040")),
    ]));
    let e = &read_activity(&MockHttp::ok(&body), SELF_ADDR).expect("read").entries[0];
    assert_eq!(e.kind, "Contract creation");
    assert_eq!(e.direction, "out");
}

/// A failed tx (status 0) passes its status through so the UI can mark it.
#[test]
fn passes_failed_status_through() {
    let body = envelope("1", "OK", JsonRows::Array(vec![
        tx(SELF_ADDR, Some(OTHER_ADDR), "1000000000000000000", 1_700_001_200, Some(0), None),
    ]));
    let e = &read_activity(&MockHttp::ok(&body), SELF_ADDR).expect("read").entries[0];
    assert_eq!(e.status, Some(0), "failed status passthrough");
}

/// A pending tx (status null) is honest about it (None), not coerced to success.
#[test]
fn pending_status_is_none() {
    let body = envelope("1", "OK", JsonRows::Array(vec![
        tx(SELF_ADDR, Some(OTHER_ADDR), "1000000000000000000", 1_700_001_300, None, None),
    ]));
    let e = &read_activity(&MockHttp::ok(&body), SELF_ADDR).expect("read").entries[0];
    assert_eq!(e.status, None);
}

/// Large amounts group thousands correctly and never lose precision (decimal-only
/// math, no float): 2,500 SALT → "−2,500.00 SALT" outgoing.
#[test]
fn formats_large_amounts_with_thousands_grouping() {
    let body = envelope("1", "OK", JsonRows::Array(vec![
        tx(SELF_ADDR, Some(OTHER_ADDR), "2500000000000000000000", 1_700_001_400, Some(1), None),
    ]));
    let e = &read_activity(&MockHttp::ok(&body), SELF_ADDR).expect("read").entries[0];
    assert_eq!(e.amount, "\u{2212}2,500.00 SALT");
}

/// A row missing its `hash` is SKIPPED, not given a fabricated id (Rule 1).
#[test]
fn skips_a_row_missing_its_hash() {
    let mut bad = tx(SELF_ADDR, Some(OTHER_ADDR), "1000000000000000000", 1_700_001_500, Some(1), None);
    bad.as_object_mut().unwrap().remove("hash");
    let good = tx(OTHER_ADDR, Some(SELF_ADDR), "2000000000000000000", 1_700_001_600, Some(1), None);
    let body = envelope("1", "OK", JsonRows::Array(vec![bad, good]));
    let res = read_activity(&MockHttp::ok(&body), SELF_ADDR).expect("read");
    assert_eq!(res.entries.len(), 1, "the hash-less row is dropped, not faked");
}

// ===========================================================================
// HONEST empty / not-provisioned / failure (Rule 1)
// ===========================================================================

/// A fresh address ("No transactions found", status 0, empty array) → an EMPTY
/// list, NOT an error and NOT fabricated. `indexer_unavailable` stays false.
#[test]
fn no_records_is_honest_empty_not_error() {
    let body = envelope("0", "No transactions found", JsonRows::Array(vec![]));
    let res = read_activity(&MockHttp::ok(&body), SELF_ADDR).expect("empty is ok");
    assert!(res.entries.is_empty());
    assert!(!res.indexer_unavailable, "a fresh address is not 'indexer unavailable'");
}

/// The not-provisioned index ("... indexer not provisioned") → EMPTY list with the
/// `indexer_unavailable` flag SET, so the UI shows "indexer syncing", not "no txs".
#[test]
fn not_provisioned_flags_indexer_unavailable() {
    let body = envelope("0", "No transactions found (indexer not provisioned)", JsonRows::Array(vec![]));
    let res = read_activity(&MockHttp::ok(&body), SELF_ADDR).expect("empty is ok");
    assert!(res.entries.is_empty());
    assert!(res.indexer_unavailable, "not-provisioned is flagged for the UI");
}

/// A hard failure (`result: null`, e.g. an unexpected explorer error on a
/// validated address) is an ERROR — we never fabricate a tx.
#[test]
fn result_null_is_an_error_not_a_fabricated_empty() {
    let body = envelope("0", "some upstream failure", JsonRows::Null);
    let r = read_activity(&MockHttp::ok(&body), SELF_ADDR);
    assert!(matches!(r, Err(ActivityError::BadResponse(_))), "got: {r:?}");
}

/// A non-JSON body is a BadResponse error (never a fabricated list).
#[test]
fn non_json_body_is_an_error() {
    let r = read_activity(&MockHttp::ok("<html>502 Bad Gateway</html>"), SELF_ADDR);
    assert!(matches!(r, Err(ActivityError::BadResponse(_))), "got: {r:?}");
}

/// A transport error surfaces as `ActivityError::Http` — the store keeps its last
/// honest list rather than showing fabricated rows.
#[test]
fn transport_error_surfaces_as_http_error() {
    let http = MockHttp::err(ActivityError::Http("connection refused".into()));
    let r = read_activity(&http, SELF_ADDR);
    assert!(matches!(r, Err(ActivityError::Http(_))), "got: {r:?}");
}

/// Multiple rows are preserved in the explorer's order (newest-first,
/// desc(timestamp)); the read does not reorder or drop real rows.
#[test]
fn preserves_multiple_rows_in_order() {
    let body = envelope("1", "OK", JsonRows::Array(vec![
        tx(SELF_ADDR, Some(OTHER_ADDR), "40000000000000000000", 1_700_003_000, Some(1), None),
        tx(OTHER_ADDR, Some(SELF_ADDR), "12410000000000000000", 1_700_002_000, Some(1), None),
        tx(SELF_ADDR, Some(SELF_ADDR), "5000000000000000000", 1_700_001_000, Some(1), None),
    ]));
    let res = read_activity(&MockHttp::ok(&body), SELF_ADDR).expect("read");
    assert_eq!(res.entries.len(), 3);
    assert_eq!(res.entries[0].kind, "Sent");
    assert_eq!(res.entries[1].kind, "Received");
    assert_eq!(res.entries[2].kind, "Self");
    // ts strictly decreasing (order preserved, scaled to ms).
    assert!(res.entries[0].ts > res.entries[1].ts && res.entries[1].ts > res.entries[2].ts);
}

// ===========================================================================
// LIVE (documented, #[ignore]) — the REAL txlist read against the explorer
// ===========================================================================
//
// The honest live-path proof: read the REAL indexed history for a supplied
// address from the public CitrateScan endpoint (no auth). A fresh address / a
// not-yet-provisioned index honestly yields an empty list. Run explicitly:
//
//   CITRATE_ACTIVITY_ADDR=0x<address> \
//     cargo test --no-default-features activity::tests::live_real_activity_read -- --ignored --nocapture
#[test]
#[ignore = "live: reads REAL tx history from https://explorer.citrate.ai for a supplied address"]
fn live_real_activity_read() {
    let addr = std::env::var("CITRATE_ACTIVITY_ADDR")
        .expect("set CITRATE_ACTIVITY_ADDR to the address to read history for");
    let http = UreqActivityClient;
    match read_activity(&http, &addr) {
        Ok(res) => eprintln!(
            "[live] activity({addr}) = {} entries, indexer_unavailable={}",
            res.entries.len(),
            res.indexer_unavailable
        ),
        Err(e) => eprintln!("[live] read_activity error: {e}"),
    }
}
