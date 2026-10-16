// HUP-S4.3 — tests for the get_verified_source lookup. Included from verified_source.rs.
use super::*;
use serde_json::json;
use std::sync::Mutex;

/// A scripted CitrateScan: answers by URL substring, records every URL asked for.
struct Scripted {
    routes: Vec<(&'static str, u16, String)>,
    seen: Mutex<Vec<String>>,
}

impl Scripted {
    fn new(routes: Vec<(&'static str, u16, String)>) -> Self {
        Scripted {
            routes,
            seen: Mutex::new(Vec::new()),
        }
    }
    fn seen(&self) -> Vec<String> {
        self.seen.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

impl ScanHttp for Scripted {
    fn get(&self, url: &str) -> Result<ScanReply, String> {
        self.seen
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(url.to_string());
        // Longest matching pattern wins, so "/source" beats the bare contract route.
        self.routes
            .iter()
            .filter(|(p, _, _)| url.contains(p))
            .max_by_key(|(p, _, _)| p.len())
            .map(|(_, status, body)| ScanReply {
                status: *status,
                body: body.clone(),
            })
            .ok_or_else(|| format!("no route for {url}"))
    }
}

const ADDR: &str = "0xABCDEFabcdef0123456789012345678901234567";
const BASE: &str = "https://scan.test";

fn new_route_body(status: &str, verified: bool, with_source: bool) -> String {
    json!({
        "address": ADDR.to_lowercase(),
        "status": status,
        "verified": verified,
        "matchType": if status == "verified" { json!("full") } else if status == "partial-match" { json!("partial") } else { Value::Null },
        "contractName": if with_source { json!("src/Foo.sol:Foo") } else { Value::Null },
        "compilerVersion": if with_source { json!("v0.8.26+commit.8a97fa7a") } else { Value::Null },
        "sourceHash": Value::Null,
        "source": if with_source { json!("contract Foo {}") } else { Value::Null },
        "sourceChars": if with_source { 15 } else { 0 },
        "sourceTruncated": false,
        "abi": if with_source { json!([{"type":"function","name":"x"}]) } else { Value::Null },
        "verifiedAt": if with_source { json!("2026-09-01T00:00:00.000Z") } else { Value::Null },
        "note": ""
    })
    .to_string()
}

#[test]
fn verified_contract_comes_back_with_source_and_compiler_metadata() {
    let http = Scripted::new(vec![(
        "/source",
        200,
        new_route_body("verified", true, true),
    )]);
    let v = lookup(&http, BASE, ADDR).expect("lookup");
    assert_eq!(
        http.seen(),
        vec![format!(
            "{BASE}/api/contract/{}/source?maxSourceChars={MAX_SOURCE_CHARS}",
            ADDR.to_lowercase()
        )]
    );
    assert_eq!(v.status, "verified");
    assert!(v.verified);
    assert_eq!(
        v.compiler_version.as_deref(),
        Some("v0.8.26+commit.8a97fa7a")
    );
    assert_eq!(v.contract_name.as_deref(), Some("src/Foo.sol:Foo"));
    assert_eq!(v.source.as_deref(), Some("contract Foo {}"));
    assert!(v.abi.is_some());
    assert_eq!(v.address, ADDR.to_lowercase());
}

#[test]
fn unverified_is_an_honest_answer_with_no_source() {
    let http = Scripted::new(vec![(
        "/source",
        200,
        new_route_body("unverified", false, false),
    )]);
    let v = lookup(&http, BASE, ADDR).expect("lookup");
    assert_eq!(v.status, "unverified");
    assert!(!v.verified);
    assert!(v.source.is_none() && v.abi.is_none());
    assert!(v.note.to_lowercase().contains("not verified"), "{}", v.note);
}

#[test]
fn unavailable_passes_through_and_is_never_called_unverified() {
    let http = Scripted::new(vec![(
        "/source",
        200,
        new_route_body("unavailable", false, false),
    )]);
    let v = lookup(&http, BASE, ADDR).expect("lookup");
    assert_eq!(v.status, "unavailable");
    assert!(!v.verified);
}

#[test]
fn a_body_claiming_verified_with_a_partial_status_is_not_verified() {
    let http = Scripted::new(vec![(
        "/source",
        200,
        new_route_body("partial-match", true, true),
    )]);
    let v = lookup(&http, BASE, ADDR).expect("lookup");
    assert_eq!(v.status, "partial-match");
    assert!(!v.verified, "verified requires status verified");
}

#[test]
fn an_unknown_status_is_an_error_not_a_guess() {
    let http = Scripted::new(vec![(
        "/source",
        200,
        new_route_body("trusted", true, true),
    )]);
    let err = lookup(&http, BASE, ADDR).expect_err("unknown status");
    // The error text reaches the model unfenced, so it never echoes the remote string.
    assert!(!err.contains("trusted"), "{err}");
}

#[test]
fn a_verified_status_without_the_verified_flag_is_an_error_not_a_verified_answer() {
    let http = Scripted::new(vec![(
        "/source",
        200,
        new_route_body("verified", false, true),
    )]);
    assert!(lookup(&http, BASE, ADDR).is_err());
}

#[test]
fn source_and_abi_sent_with_an_unverified_or_unavailable_status_are_dropped() {
    for status in ["unverified", "unavailable"] {
        let http = Scripted::new(vec![("/source", 200, new_route_body(status, false, true))]);
        let v = lookup(&http, BASE, ADDR).expect("lookup");
        assert_eq!(v.status, status);
        assert!(!v.verified);
        assert!(
            v.source.is_none(),
            "{status}: source must not reach the agent"
        );
        assert!(v.abi.is_none(), "{status}: abi must not reach the agent");
    }
}

#[test]
fn an_older_explorer_without_the_source_route_falls_back_to_the_contract_route() {
    let contract = json!({
        "address": ADDR,
        "isContract": true,
        "verification": {
            "verified": true, "status": "verified", "matchType": "full",
            "contractName": "A.sol:A", "compilerVersion": "v0.8.26", "source": "contract A {}",
            "abi": [], "verifiedAt": "2026-09-01T00:00:00Z"
        }
    })
    .to_string();
    let http = Scripted::new(vec![
        ("/source?maxSourceChars", 404, "not found".into()),
        ("/api/contract/", 200, contract),
    ]);
    let v = lookup(&http, BASE, ADDR).expect("lookup");
    let seen = http.seen();
    assert_eq!(seen.len(), 2);
    assert_eq!(
        seen[1],
        format!("{BASE}/api/contract/{}", ADDR.to_lowercase())
    );
    assert_eq!(v.status, "verified");
    assert!(v.verified);
    assert_eq!(v.source.as_deref(), Some("contract A {}"));
}

#[test]
fn fallback_partial_and_eoa_and_unverified_are_all_not_verified() {
    let partial = json!({"isContract": true, "verification": {
        "verified": false, "status": "partial-match", "matchType": "partial", "source": "contract B {}"}})
    .to_string();
    let http = Scripted::new(vec![
        ("/source?maxSourceChars", 404, String::new()),
        ("/api/contract/", 200, partial),
    ]);
    let v = lookup(&http, BASE, ADDR).expect("lookup");
    assert_eq!(v.status, "partial-match");
    assert!(!v.verified);

    let eoa = json!({"isContract": false, "codeSize": 0}).to_string();
    let http = Scripted::new(vec![
        ("/source?maxSourceChars", 404, String::new()),
        ("/api/contract/", 200, eoa),
    ]);
    let v = lookup(&http, BASE, ADDR).expect("lookup");
    assert_eq!(v.status, "unverified");
    assert!(v.note.contains("no contract code"), "{}", v.note);

    let unv =
        json!({"isContract": true, "verification": {"verified": false, "status": "unverified"}})
            .to_string();
    let http = Scripted::new(vec![
        ("/source?maxSourceChars", 404, String::new()),
        ("/api/contract/", 200, unv),
    ]);
    let v = lookup(&http, BASE, ADDR).expect("lookup");
    assert_eq!(v.status, "unverified");
    assert!(v.source.is_none());
}

#[test]
fn rate_limits_and_server_errors_are_reported_not_hidden() {
    let http = Scripted::new(vec![("/source", 429, "{}".into())]);
    let e = lookup(&http, BASE, ADDR).expect_err("429");
    assert!(e.contains("rate limit"), "{e}");
    let http = Scripted::new(vec![("/source", 502, "{}".into())]);
    let e = lookup(&http, BASE, ADDR).expect_err("502");
    assert!(e.contains("502"), "{e}");
}

#[test]
fn a_malformed_address_never_reaches_the_network() {
    let http = Scripted::new(vec![]);
    assert!(lookup(&http, BASE, "0x1234").is_err());
    assert!(lookup(&http, BASE, "not an address").is_err());
    assert!(http.seen().is_empty());
}

#[test]
fn oversized_source_is_truncated_on_a_char_boundary_and_flagged() {
    let big = "é".repeat(MAX_SOURCE_CHARS + 10);
    let body =
        json!({"status":"verified","verified":true,"matchType":"full","source":big,"note":"n"})
            .to_string();
    let http = Scripted::new(vec![("/source", 200, body)]);
    let v = lookup(&http, BASE, ADDR).expect("lookup");
    assert_eq!(
        v.source.as_deref().map(|s| s.chars().count()),
        Some(MAX_SOURCE_CHARS)
    );
    assert!(v.source_truncated);
}
