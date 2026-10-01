// HUP-S10.5 — the privacy contracts, as tests:
//
// 1. Offline matrix (`src/privacy/offline-matrix.json`): every feature names a probe, and each
//    Rust probe runs the real code path with the network unreachable (a closed loopback port)
//    and must behave as the matrix says: "works" features succeed, "unavailable"/"degrades"
//    features fail with a member-readable message that leaks no secret, within a deadline.
// 2. Telemetry consent (`src/privacy/telemetry-fields.json`): the consent screen's field list
//    is exactly the fields the Rust bundle serializes, and the endpoint is the pinned one.
// 3. Default budget values (`src/privacy/budget-defaults.json`): conservative placeholders,
//    marked pending owner sign-off, never above the ceilings the Rule-3 ADR allows.
//
// Written red-first: the probes for the telemetry send needed `post_report` (absent before this
// WP) and the JSON files did not exist, so this failed to compile, then went green.

use std::collections::BTreeSet;
use std::sync::mpsc;
use std::time::Duration;

const OFFLINE_MATRIX: &str = include_str!("../../src/privacy/offline-matrix.json");
const TELEMETRY_FIELDS: &str = include_str!("../../src/privacy/telemetry-fields.json");
const BUDGET_DEFAULTS: &str = include_str!("../../src/privacy/budget-defaults.json");

/// How a probe behaved with no network.
#[derive(Debug)]
enum Probe {
    /// The feature worked.
    Worked,
    /// The feature failed with this member-facing message.
    FailedHonestly(String),
    /// The feature misbehaved (leaked a secret, pretended to succeed, ...).
    Dishonest(String),
}

/// A loopback URL whose port nothing listens on: connection refused, the same as offline.
fn closed_url(path: &str) -> String {
    let l = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = l.local_addr().expect("addr").port();
    drop(l);
    format!("http://127.0.0.1:{port}{path}")
}

fn tmpdir(tag: &str) -> std::path::PathBuf {
    use rand::RngCore;
    let mut r = [0u8; 8];
    rand::thread_rng().fill_bytes(&mut r);
    let d = std::env::temp_dir().join(format!("n4-offline-{tag}-{}", hex::encode(r)));
    std::fs::create_dir_all(&d).expect("tmpdir");
    d
}

fn from_result(r: std::result::Result<(), String>) -> Probe {
    match r {
        Ok(()) => Probe::Worked,
        Err(m) if m.trim().is_empty() => Probe::Dishonest("empty error message".into()),
        Err(m) => Probe::FailedHonestly(m),
    }
}

struct NoKeychain;
impl crate::local_data::KeychainOps for NoKeychain {
    fn present(&self, _s: &str, _a: &str) -> Option<bool> {
        Some(false)
    }
    fn read_text(&self, _s: &str, _a: &str) -> Option<String> {
        None
    }
    fn delete(&self, _s: &str, _a: &str) -> std::result::Result<(), String> {
        Ok(())
    }
}

fn run_probe(name: &str) -> Probe {
    match name {
        "local_chat_is_loopback" => {
            let u = crate::ai::local_base_url(8080);
            match url::Url::parse(&u)
                .ok()
                .and_then(|p| p.host_str().map(str::to_string))
            {
                Some(h) if h == "127.0.0.1" || h == "localhost" => Probe::Worked,
                other => Probe::Dishonest(format!("local chat is not on loopback: {other:?}")),
            }
        }
        "journal_export_offline" => {
            let d = tmpdir("journal");
            let p = d.join("probe.citrate-journal");
            let out = crate::journal_export::export_to_path(
                &p,
                "an offline passphrase",
                b"{\"pages\":[]}",
            )
            .and_then(|_| crate::journal_export::import_from_path(&p, "an offline passphrase"))
            .map(|_| ())
            .map_err(|e| e.to_string());
            let _ = std::fs::remove_dir_all(&d);
            from_result(out)
        }
        "recovery_kit_offline" => {
            use crate::recovery_kit::{open_kit, seal_kit, KitKey};
            let keys = vec![KitKey {
                account: "memory-store-key".into(),
                key: zeroize::Zeroizing::new(vec![9u8; 32]),
            }];
            let out = seal_kit(b"an offline passphrase", &keys, "2026-10-01")
                .and_then(|f| open_kit(b"an offline passphrase", &f))
                .map(|_| ())
                .map_err(|e| e.to_string());
            from_result(out)
        }
        "local_data_offline" => {
            let base = tmpdir("localdata");
            let data = base.join(crate::local_data::BUNDLE_ID);
            let _ = std::fs::create_dir_all(&data);
            let _ = std::fs::write(data.join("config.json"), b"{}");
            let plan = crate::local_data::build_plan(
                &crate::local_data::AppRoots {
                    data_dir: data,
                    others: vec![],
                },
                &NoKeychain,
                crate::local_data::DeleteOptions::default(),
            );
            let _ = std::fs::remove_dir_all(&base);
            if plan.entries.len() == 1 {
                Probe::Worked
            } else {
                Probe::Dishonest(format!("unexpected plan: {:?}", plan.entries))
            }
        }
        "diagnostics_prepare_offline" => {
            let b = crate::telemetry::build_bundle(
                "rpt_00".into(),
                "0.0.0",
                "macos",
                "",
                "",
                &[],
                "/Users/x",
            );
            if b.report_id == "rpt_00" {
                Probe::Worked
            } else {
                Probe::Dishonest("bundle changed the id".into())
            }
        }
        "telemetry_send_unreachable" => {
            let r = crate::telemetry::post_report(&closed_url("/report"), "{}");
            match r {
                Err(m) if m.starts_with("couldn't send the report") => Probe::FailedHonestly(m),
                other => Probe::Dishonest(format!("{other:?}")),
            }
        }
        "chain_rpc_unreachable" => {
            use citrate_core_kit::rpc::{HttpTransport, RpcClient};
            let c = RpcClient::with_transport(HttpTransport::new(closed_url("/")));
            match c.block_number() {
                Ok(n) => Probe::Dishonest(format!("read a block number offline: {n}")),
                Err(e) => from_result(Err(e.to_string())),
            }
        }
        "ai_provider_unreachable" => {
            use crate::ai::AiHttpClient;
            let secret = "sk-offline-probe-0123456789";
            let r = crate::ai::UreqAiClient.post_json(
                &closed_url("/v1/chat/completions"),
                secret,
                &serde_json::json!({"model": "x", "messages": []}),
            );
            match r {
                Ok(body) => Probe::Dishonest(format!("got a reply offline: {body}")),
                Err(e) if e.to_string().contains(secret) => {
                    Probe::Dishonest("error echoes the key".into())
                }
                Err(e) => from_result(Err(e.to_string())),
            }
        }
        "oidc_unreachable" => {
            use citrate_core_kit::oidc::HttpClient;
            match citrate_core_kit::oidc::UreqClient
                .get(&closed_url("/.well-known/jwks.json"), None)
            {
                Ok(body) => Probe::Dishonest(format!("got a reply offline: {body}")),
                Err(e) => from_result(Err(e.to_string())),
            }
        }
        "model_download_unreachable" => {
            use crate::model::ModelTransport;
            match crate::model::UreqModelTransport::new(closed_url("/model.gguf")).total_size() {
                Ok(n) => Probe::Dishonest(format!("got a size offline: {n}")),
                Err(e) => from_result(Err(e.to_string())),
            }
        }
        other => Probe::Dishonest(format!("no probe named {other}")),
    }
}

/// Run a probe with a deadline: a feature that hangs offline is not degrading honestly.
fn run_probe_bounded(name: &str) -> Probe {
    let (tx, rx) = mpsc::channel();
    let owned = name.to_string();
    std::thread::spawn(move || {
        let _ = tx.send(run_probe(&owned));
    });
    match rx.recv_timeout(Duration::from_secs(45)) {
        Ok(p) => p,
        Err(_) => Probe::Dishonest("hung for more than 45 s with no network".into()),
    }
}

fn matrix() -> serde_json::Value {
    serde_json::from_str(OFFLINE_MATRIX).expect("offline-matrix.json parses")
}

#[test]
fn every_offline_matrix_feature_is_probed_or_says_why_not() {
    let m = matrix();
    let features = m["features"].as_array().expect("features");
    assert!(features.len() >= 10);
    let mut ids = BTreeSet::new();
    let mut unprobed = 0;
    for f in features {
        let id = f["id"].as_str().expect("id");
        assert!(ids.insert(id.to_string()), "duplicate id {id}");
        let offline = f["offline"].as_str().expect("offline");
        assert!(
            matches!(offline, "works" | "degrades" | "unavailable"),
            "{id}: {offline}"
        );
        assert!(
            !f["behaviour"].as_str().unwrap_or("").is_empty(),
            "{id}: behaviour"
        );
        let probe = f["probe"].as_str().expect("probe");
        if probe == "none" {
            unprobed += 1;
            assert!(
                !f["unprobedReason"].as_str().unwrap_or("").is_empty(),
                "{id}: an unprobed feature must say why"
            );
        } else {
            assert!(
                probe.starts_with("rust:"),
                "{id}: unknown probe kind {probe}"
            );
        }
    }
    // Pinned so a new unprobed row is a deliberate, reviewed change.
    assert_eq!(
        unprobed, 2,
        "unprobed features changed; update this pin on purpose"
    );
}

#[test]
fn each_feature_degrades_honestly_with_no_network() {
    let m = matrix();
    let mut failures = Vec::new();
    for f in m["features"].as_array().expect("features") {
        let id = f["id"].as_str().unwrap_or("?");
        let Some(name) = f["probe"].as_str().and_then(|p| p.strip_prefix("rust:")) else {
            continue;
        };
        let want = f["offline"].as_str().unwrap_or("");
        let got = run_probe_bounded(name);
        let ok = match (&got, want) {
            (Probe::Worked, "works") => true,
            (Probe::FailedHonestly(msg), "unavailable" | "degrades") => {
                !msg.contains('\u{2014}') && msg.len() < 2000
            }
            _ => false,
        };
        if !ok {
            failures.push(format!("{id} (want {want}): {got:?}"));
        }
    }
    assert!(
        failures.is_empty(),
        "offline matrix mismatches:\n{}",
        failures.join("\n")
    );
}

#[test]
fn a_probe_that_does_not_exist_is_caught() {
    // Negative control: the harness must be able to fail.
    match run_probe("no_such_probe") {
        Probe::Dishonest(why) => assert!(why.contains("no probe named")),
        other => panic!("expected a dishonest probe, got {other:?}"),
    }
}

// ---------- telemetry consent ----------

#[test]
fn consent_screen_lists_exactly_the_fields_the_bundle_sends() {
    let t: serde_json::Value = serde_json::from_str(TELEMETRY_FIELDS).expect("parses");
    assert_eq!(t["default"], "off");
    assert_eq!(
        t["endpoint"].as_str(),
        Some(crate::telemetry::TELEMETRY_INGEST_URL)
    );
    let listed: BTreeSet<String> = t["fields"]
        .as_array()
        .expect("fields")
        .iter()
        .map(|f| f["key"].as_str().expect("key").to_string())
        .collect();
    let b = crate::telemetry::build_bundle(
        "rpt_ab".into(),
        "0.0.0",
        "macos",
        "c",
        "n",
        &["e".into()],
        "/h",
    );
    let v = serde_json::to_value(&b).expect("serializes");
    let sent: BTreeSet<String> = v.as_object().expect("object").keys().cloned().collect();
    assert_eq!(
        listed, sent,
        "the consent screen must list exactly what is sent"
    );
    for f in t["fields"].as_array().expect("fields") {
        assert!(!f["what"].as_str().unwrap_or("").is_empty());
    }
    assert!(t["neverSent"].as_array().is_some_and(|a| a.len() >= 4));
}

// ---------- default budget values ----------

/// Parse a decimal SALT amount into 18-decimal base units (no floats).
fn salt_units(s: &str) -> u128 {
    let (int, frac) = s.split_once('.').unwrap_or((s, ""));
    assert!(frac.len() <= 18, "too many decimals: {s}");
    let i: u128 = int.parse().expect("integer part");
    let f: u128 = if frac.is_empty() {
        0
    } else {
        format!("{frac:0<18}").parse().expect("fraction")
    };
    i * 10u128.pow(18) + f
}

fn salt(n: u128) -> u128 {
    n * 10u128.pow(18)
}

#[test]
fn budget_defaults_are_marked_pending_owner_sign_off() {
    let b: serde_json::Value = serde_json::from_str(BUDGET_DEFAULTS).expect("parses");
    assert_eq!(b["status"], "placeholder-pending-owner-sign-off");
    assert!(b["note"]
        .as_str()
        .unwrap_or("")
        .contains("pending owner sign-off"));
    assert!(b["note"].as_str().unwrap_or("").contains("HIC-1"));
}

#[test]
fn budget_defaults_stay_within_the_rule3_adr_ceilings() {
    let b: serde_json::Value = serde_json::from_str(BUDGET_DEFAULTS).expect("parses");
    // SIWE (ADR O-2: max_count 50, expiry at most 30 days).
    let siwe_count = b["siwe"]["maxCount"].as_u64().expect("siwe.maxCount");
    let siwe_days = b["siwe"]["expiresDays"].as_u64().expect("siwe.expiresDays");
    assert!((1..=50).contains(&siwe_count));
    assert!((1..=30).contains(&siwe_days));
    // x402 (ADR D3 table).
    let x = &b["x402"];
    let per_sig = salt_units(x["perSignatureMaxSalt"].as_str().expect("per sig"));
    let per_rec = salt_units(x["perRecipientWindowMaxSalt"].as_str().expect("per rec"));
    let global = salt_units(x["globalWindowMaxSalt"].as_str().expect("global"));
    assert!(per_sig > 0 && per_sig <= salt(1));
    assert!(per_rec <= salt(10));
    assert!(global <= salt(20));
    assert!(per_sig <= per_rec && per_rec <= global, "caps must nest");
    assert_eq!(
        x["windowHours"].as_u64(),
        Some(24),
        "the ADR window is a rolling 24 h"
    );
    assert!((1..=200).contains(&x["maxCount"].as_u64().expect("max count")));
    assert!((1..=10).contains(&x["validityMaxMinutes"].as_u64().expect("validity")));
    assert!((1..=7).contains(&x["expiresDays"].as_u64().expect("expiry")));
    // Daemons (HIC-3): bounded and non-zero.
    let d = &b["daemons"];
    assert!((1..=24).contains(&d["maxRunsPerDay"].as_u64().expect("runs")));
    assert!((1..=60).contains(&d["maxMinutesPerRun"].as_u64().expect("minutes")));
    assert!((1..=200_000).contains(&d["maxModelTokensPerRun"].as_u64().expect("tokens")));
}

#[test]
fn budget_defaults_link_to_the_medusa_budgets_instead_of_copying_them() {
    let b: serde_json::Value = serde_json::from_str(BUDGET_DEFAULTS).expect("parses");
    let rel = b["medusaBudgets"].as_str().expect("medusaBudgets");
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(rel);
    assert!(
        path.is_file(),
        "{path:?} must exist (Rule 9: one source of truth)"
    );
    assert!(b.get("medusa").is_none());
}

#[test]
fn salt_unit_parser_is_exact() {
    assert_eq!(salt_units("0.1"), 10u128.pow(17));
    assert_eq!(salt_units("1"), salt(1));
    assert_eq!(salt_units("2.5"), salt(2) + 5 * 10u128.pow(17));
}
