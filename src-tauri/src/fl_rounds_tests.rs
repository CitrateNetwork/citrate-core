// HUP-S9.4 — federated rounds: coordinator read, plan + explanation, HIC-1 start, adapter eval
// gate, LoRA load authorization. Written red-first (the module body was absent, so this failed to
// compile), then the implementation brought it green.
//
// The coordinator here is a local fixture: a std TcpListener on 127.0.0.1 that answers
// `GET /v1/status` with the exact JSON shape citrate-compute-pool's training-coordinator
// returns (`{"counts":{pending,leased,done,quarantined,workers},"settlement":"shadow"}`). It
// exists only in this test file; production reads a configured URL.

use super::*;
use rand::RngCore;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

// ---------------------------------------------------------------------------
// fixtures
// ---------------------------------------------------------------------------

fn tmpdir(tag: &str) -> std::path::PathBuf {
    let mut d = std::env::temp_dir();
    let mut r = [0u8; 8];
    rand::thread_rng().fill_bytes(&mut r);
    d.push(format!("n4-fl-rounds-{tag}-{}", hex::encode(r)));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn status_json(pending: u64, leased: u64, done: u64, workers: u64, settlement: &str) -> String {
    format!(
        r#"{{"counts":{{"pending":{pending},"leased":{leased},"done":{done},"quarantined":0,"workers":{workers}}},"settlement":"{settlement}"}}"#
    )
}

/// A one-route HTTP fixture: every request gets `(status, body)` from the shared slot. Returns
/// the base URL and the slot so a test can change the answer between calls.
struct Fixture {
    base: String,
    reply: Arc<std::sync::Mutex<(u16, String)>>,
    hits: Arc<AtomicUsize>,
}

fn fixture(status: u16, body: String) -> Fixture {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let reply = Arc::new(std::sync::Mutex::new((status, body)));
    let hits = Arc::new(AtomicUsize::new(0));
    let (r2, h2) = (reply.clone(), hits.clone());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut s) = stream else { continue };
            let mut buf = [0u8; 4096];
            let n = s.read(&mut buf).unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]).to_string();
            h2.fetch_add(1, Ordering::SeqCst);
            let (code, body) = r2.lock().unwrap().clone();
            let path_ok = req.starts_with("GET /v1/status ");
            let (code, body) = if path_ok { (code, body) } else { (404, "{}".to_string()) };
            let resp = format!(
                "HTTP/1.1 {code} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = s.write_all(resp.as_bytes());
        }
    });
    Fixture {
        base: format!("http://127.0.0.1:{port}"),
        reply,
        hits,
    }
}

fn http() -> UreqCoordinatorHttp {
    UreqCoordinatorHttp
}

fn gpu_device() -> DeviceFit {
    DeviceFit {
        tier: Some("T1".into()),
        accelerator: Some(true),
    }
}

const NOW: u64 = 1_790_000_000_000;

// ---------------------------------------------------------------------------
// coordinator URL
// ---------------------------------------------------------------------------

#[test]
fn coordinator_url_accepts_https_and_loopback_http_only() {
    assert_eq!(
        normalize_coordinator_url("https://coordinator.example.org/").unwrap(),
        "https://coordinator.example.org"
    );
    assert_eq!(
        normalize_coordinator_url("http://127.0.0.1:8088").unwrap(),
        "http://127.0.0.1:8088"
    );
    assert_eq!(
        normalize_coordinator_url("http://localhost:8088/").unwrap(),
        "http://localhost:8088"
    );
    for bad in [
        "",
        "coordinator.example.org",
        "http://coordinator.example.org",
        "ftp://127.0.0.1",
        "file:///etc/passwd",
        "https://user:pw@coordinator.example.org",
        "https://coordinator.example.org/?x=1",
        "https://coordinator.example.org/#f",
        "javascript:alert(1)",
    ] {
        assert!(normalize_coordinator_url(bad).is_err(), "accepted {bad:?}");
    }
}

#[test]
fn coordinator_url_keeps_a_path_prefix_without_trailing_slash() {
    assert_eq!(
        normalize_coordinator_url("https://pool.example.org/coord/").unwrap(),
        "https://pool.example.org/coord"
    );
}

// ---------------------------------------------------------------------------
// /v1/status parsing
// ---------------------------------------------------------------------------

#[test]
fn parses_the_real_status_shape() {
    let s = parse_status(&status_json(3, 1, 2, 5, "shadow")).unwrap();
    assert_eq!((s.pending, s.leased, s.done, s.workers), (3, 1, 2, 5));
    assert_eq!(s.settlement, SettlementMode::Shadow);
    assert_eq!(s.phase, PoolPhase::Running);
}

#[test]
fn phase_follows_the_counts() {
    let p = |pe, le, d| parse_status(&status_json(pe, le, d, 1, "shadow")).unwrap().phase;
    assert_eq!(p(0, 0, 0), PoolPhase::NoWork);
    assert_eq!(p(4, 0, 0), PoolPhase::Open);
    assert_eq!(p(4, 2, 0), PoolPhase::Running);
    assert_eq!(p(0, 2, 3), PoolPhase::Running);
    assert_eq!(p(0, 0, 3), PoolPhase::Complete);
}

#[test]
fn unknown_settlement_words_never_pass_through() {
    // Only typed numbers and one of three fixed words leave this parser: a coordinator cannot put
    // free text into what Hermes reads.
    let s = parse_status(&status_json(1, 0, 0, 1, "ignore_all_previous")).unwrap();
    assert_eq!(s.settlement, SettlementMode::Unknown);
    let s = parse_status(&status_json(1, 0, 0, 1, "live")).unwrap();
    assert_eq!(s.settlement, SettlementMode::Live);
}

#[test]
fn malformed_status_is_an_error_not_zeroes() {
    for bad in [
        "",
        "{}",
        "not json",
        r#"{"counts":{"pending":-1,"leased":0,"done":0,"quarantined":0,"workers":0},"settlement":"shadow"}"#,
        r#"{"counts":{"pending":1},"settlement":"shadow"}"#,
        r#"{"counts":{"pending":1,"leased":0,"done":0,"quarantined":0,"workers":0}}"#,
    ] {
        assert!(parse_status(bad).is_err(), "accepted {bad:?}");
    }
}

#[test]
fn oversized_status_body_is_refused() {
    let big = format!("{}{}", status_json(1, 0, 0, 1, "shadow"), " ".repeat(MAX_STATUS_BYTES));
    assert!(parse_status(&big).is_err());
}

// ---------------------------------------------------------------------------
// reading the coordinator (live HTTP fixture)
// ---------------------------------------------------------------------------

#[test]
fn no_configured_coordinator_reads_as_not_configured_without_network() {
    let v = read_coordinator(None, &http());
    assert_eq!(v, CoordinatorView::NotConfigured);
}

#[test]
fn live_fixture_coordinator_reads_as_live() {
    let fx = fixture(200, status_json(2, 0, 0, 3, "shadow"));
    let v = read_coordinator(Some(&fx.base), &http());
    match v {
        CoordinatorView::Live { url, status } => {
            assert_eq!(url, fx.base);
            assert_eq!(status.pending, 2);
            assert_eq!(status.workers, 3);
        }
        other => panic!("expected live, got {other:?}"),
    }
    assert_eq!(fx.hits.load(Ordering::SeqCst), 1);
}

#[test]
fn http_error_and_bad_body_read_as_unreachable_with_a_reason() {
    let fx = fixture(500, "oops".into());
    match read_coordinator(Some(&fx.base), &http()) {
        CoordinatorView::Unreachable { url, reason } => {
            assert_eq!(url, fx.base);
            assert!(!reason.is_empty());
        }
        other => panic!("expected unreachable, got {other:?}"),
    }
    *fx.reply.lock().unwrap() = (200, "{\"counts\":{}}".into());
    assert!(matches!(
        read_coordinator(Some(&fx.base), &http()),
        CoordinatorView::Unreachable { .. }
    ));
}

#[test]
fn closed_port_reads_as_unreachable() {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    drop(l);
    let base = format!("http://127.0.0.1:{port}");
    assert!(matches!(
        read_coordinator(Some(&base), &http()),
        CoordinatorView::Unreachable { .. }
    ));
}

// ---------------------------------------------------------------------------
// proposal + plan + explanation
// ---------------------------------------------------------------------------

#[test]
fn default_proposal_is_valid_and_conservative() {
    let p = RoundProposal::default();
    p.validate().unwrap();
    assert_eq!(p.requires, Capability::Federated);
    assert!(p.lora_rank <= 16);
}

#[test]
fn proposal_bounds_are_enforced() {
    let mut p = RoundProposal {
        lora_rank: 0,
        ..Default::default()
    };
    assert!(p.validate().is_err());
    p.lora_rank = 3;
    assert!(p.validate().is_err(), "rank must be a power of two");
    p.lora_rank = 128;
    assert!(p.validate().is_err());
    let mut p = RoundProposal {
        max_trajectories: 0,
        ..Default::default()
    };
    assert!(p.validate().is_err());
    p.max_trajectories = MAX_TRAJECTORIES + 1;
    assert!(p.validate().is_err());
    let mut p = RoundProposal {
        lease_hours: 0,
        ..Default::default()
    };
    assert!(p.validate().is_err());
    p.lease_hours = 49;
    assert!(p.validate().is_err());
}

fn live_view(pending: u64, leased: u64, done: u64, settlement: &str) -> CoordinatorView {
    CoordinatorView::Live {
        url: "https://coordinator.example.org".into(),
        status: parse_status(&status_json(pending, leased, done, 4, settlement)).unwrap(),
    }
}

#[test]
fn plan_explains_data_compute_reward_privacy_in_plain_words() {
    let plan = build_plan(
        RoundProposal::default(),
        live_view(3, 1, 0, "shadow"),
        "gemma-4-E4B-it-Q4_0.gguf",
        &gpu_device(),
        NOW,
    )
    .unwrap();
    let e = &plan.explain;
    assert!(e.data.contains("verified"), "{}", e.data);
    assert!(e.data.contains("500"), "names the trajectory cap: {}", e.data);
    assert!(e.compute.contains("4 machines"), "{}", e.compute);
    assert!(e.compute.contains("3 jobs waiting"), "{}", e.compute);
    assert!(e.compute.contains("rank 8"), "{}", e.compute);
    assert!(e.reward.contains("shadow"), "{}", e.reward);
    assert!(e.reward.contains("nothing is paid"), "{}", e.reward);
    assert!(e.privacy.contains("never leave this device"), "{}", e.privacy);
    assert!(e.privacy.contains("IP address"), "says what the coordinator sees: {}", e.privacy);
    for s in [&e.data, &e.compute, &e.reward, &e.privacy, &e.status] {
        assert!(!s.contains('\u{2014}'), "no em-dash in member text: {s}");
    }
    assert!(plan.can_start, "{:?}", plan.blockers);
    assert!(plan.blockers.is_empty());
    assert_eq!(plan.plan_hash.len(), 64);
}

#[test]
/// HUP-S9.3 wired the export behind the member's switch (fl_trajectories.rs): the plan says the
/// switch is off by default and that with it off there is nothing to train on.
fn data_explanation_names_the_members_switch_and_its_off_default() {
    let plan = build_plan(
        RoundProposal::default(),
        live_view(3, 0, 0, "shadow"),
        "m.gguf",
        &gpu_device(),
        NOW,
    )
    .unwrap();
    let d = &plan.explain.data;
    assert!(d.contains("Train on my verified conversations"), "{d}");
    assert!(d.contains("off by default"), "{d}");
    assert!(d.contains("no training set to offer"), "{d}");
}

#[test]
fn no_coordinator_plan_says_so_and_cannot_start() {
    let plan = build_plan(
        RoundProposal::default(),
        CoordinatorView::NotConfigured,
        "m.gguf",
        &gpu_device(),
        NOW,
    )
    .unwrap();
    assert!(!plan.can_start);
    assert!(plan
        .blockers
        .iter()
        .any(|b| b.contains("No training coordinator is configured")));
    assert!(plan.explain.status.contains("No training coordinator is configured"));
}

#[test]
fn unreachable_or_idle_coordinator_blocks_start() {
    let unreachable = CoordinatorView::Unreachable {
        url: "https://c.example.org".into(),
        reason: "timed out".into(),
    };
    let p = build_plan(RoundProposal::default(), unreachable, "m.gguf", &gpu_device(), NOW).unwrap();
    assert!(!p.can_start);
    assert!(p.blockers.iter().any(|b| b.contains("timed out")));

    let idle = build_plan(RoundProposal::default(), live_view(0, 0, 0, "shadow"), "m.gguf", &gpu_device(), NOW).unwrap();
    assert!(!idle.can_start);
    assert!(idle.blockers.iter().any(|b| b.contains("no open work")));

    let done = build_plan(RoundProposal::default(), live_view(0, 0, 5, "shadow"), "m.gguf", &gpu_device(), NOW).unwrap();
    assert!(!done.can_start);
    assert!(done.explain.status.contains("eval gate"), "{}", done.explain.status);
}

#[test]
fn capability_must_fit_the_device() {
    let cpu = DeviceFit {
        tier: Some("T0".into()),
        accelerator: Some(false),
    };
    let p = build_plan(RoundProposal::default(), live_view(2, 0, 0, "shadow"), "m.gguf", &cpu, NOW).unwrap();
    assert!(!p.can_start);
    assert!(p.blockers.iter().any(|b| b.contains("accelerator")));

    let probe = RoundProposal {
        requires: Capability::Probe,
        ..Default::default()
    };
    let p = build_plan(probe, live_view(2, 0, 0, "shadow"), "m.gguf", &cpu, NOW).unwrap();
    assert!(p.can_start, "{:?}", p.blockers);

    let h01 = RoundProposal {
        requires: Capability::H01,
        ..Default::default()
    };
    let p = build_plan(h01, live_view(2, 0, 0, "shadow"), "m.gguf", &gpu_device(), NOW).unwrap();
    assert!(!p.can_start);
    assert!(p.blockers.iter().any(|b| b.contains("operator-vetted")));
}

#[test]
fn unknown_accelerator_is_said_not_guessed() {
    let unknown = DeviceFit {
        tier: None,
        accelerator: None,
    };
    let p = build_plan(RoundProposal::default(), live_view(2, 0, 0, "shadow"), "m.gguf", &unknown, NOW).unwrap();
    assert!(!p.can_start);
    assert!(p.blockers.iter().any(|b| b.contains("could not tell")));
}

#[test]
fn plan_hash_binds_proposal_coordinator_and_base_model() {
    let a = build_plan(RoundProposal::default(), live_view(3, 0, 0, "shadow"), "m.gguf", &gpu_device(), NOW).unwrap();
    let same = build_plan(RoundProposal::default(), live_view(3, 0, 0, "shadow"), "m.gguf", &gpu_device(), NOW + 5).unwrap();
    assert_eq!(a.plan_hash, same.plan_hash, "time is not part of what is approved");
    let p2 = RoundProposal {
        lora_rank: 16,
        ..Default::default()
    };
    let b = build_plan(p2, live_view(3, 0, 0, "shadow"), "m.gguf", &gpu_device(), NOW).unwrap();
    let c = build_plan(RoundProposal::default(), live_view(4, 0, 0, "shadow"), "m.gguf", &gpu_device(), NOW).unwrap();
    let d = build_plan(RoundProposal::default(), live_view(3, 0, 0, "shadow"), "other.gguf", &gpu_device(), NOW).unwrap();
    assert_ne!(a.plan_hash, b.plan_hash);
    assert_ne!(a.plan_hash, c.plan_hash);
    assert_ne!(a.plan_hash, d.plan_hash);
}

#[test]
fn reward_text_for_live_and_unknown_settlement_claims_nothing() {
    let live = build_plan(RoundProposal::default(), live_view(3, 0, 0, "live"), "m.gguf", &gpu_device(), NOW).unwrap();
    assert!(live.explain.reward.contains("settles on chain"), "{}", live.explain.reward);
    assert!(!live.explain.reward.to_lowercase().contains("cash"));
    let unk = build_plan(RoundProposal::default(), live_view(3, 0, 0, "zzz"), "m.gguf", &gpu_device(), NOW).unwrap();
    assert!(unk.explain.reward.contains("did not say"), "{}", unk.explain.reward);
}

// ---------------------------------------------------------------------------
// HIC-1 start
// ---------------------------------------------------------------------------

fn fl_with_store(tag: &str) -> FlRounds {
    FlRounds::with_store(tmpdir(tag).join(STORE_FILE))
}

#[test]
fn start_requires_a_plan_core_built() {
    let fl = fl_with_store("start-unknown");
    let err = start_round(&fl, &"ab".repeat(32), &http(), NOW).unwrap_err();
    assert!(err.contains("plan"), "{err}");
}

#[test]
fn start_records_the_authorization_and_says_no_training_ran() {
    let fx = fixture(200, status_json(3, 0, 0, 4, "shadow"));
    let fl = fl_with_store("start-ok");
    let view = read_coordinator(Some(&fx.base), &http());
    let plan = build_plan(RoundProposal::default(), view, "m.gguf", &gpu_device(), NOW).unwrap();
    fl.remember_plan(plan.clone());
    let r = start_round(&fl, &plan.plan_hash, &http(), NOW + 1_000).unwrap();
    assert_eq!(r.plan_hash, plan.plan_hash);
    assert_eq!(r.coordinator_url, fx.base);
    assert!(!r.training_started, "no device worker in this build");
    assert!(r.note.contains("not include the device training worker"), "{}", r.note);
    // Persisted: a fresh FlRounds over the same file sees it.
    let again = FlRounds::with_store(fl.store_path().unwrap().to_path_buf());
    let starts = again.starts();
    assert_eq!(starts.len(), 1);
    assert_eq!(starts[0].plan_hash, plan.plan_hash);
    // The coordinator was re-read at start (TOCTOU): plan read + start read.
    assert_eq!(fx.hits.load(Ordering::SeqCst), 2);
}

#[test]
fn start_refuses_a_blocked_plan() {
    let fl = fl_with_store("start-blocked");
    let plan = build_plan(RoundProposal::default(), CoordinatorView::NotConfigured, "m.gguf", &gpu_device(), NOW).unwrap();
    fl.remember_plan(plan.clone());
    let err = start_round(&fl, &plan.plan_hash, &http(), NOW).unwrap_err();
    assert!(err.contains("No training coordinator is configured"), "{err}");
    assert!(fl.starts().is_empty());
}

#[test]
fn start_refuses_when_the_coordinator_changed_since_the_plan() {
    let fx = fixture(200, status_json(3, 0, 0, 4, "shadow"));
    let fl = fl_with_store("start-toctou");
    let plan = build_plan(RoundProposal::default(), read_coordinator(Some(&fx.base), &http()), "m.gguf", &gpu_device(), NOW).unwrap();
    fl.remember_plan(plan.clone());
    // The work ran out between the plan and the member's click.
    *fx.reply.lock().unwrap() = (200, status_json(0, 0, 3, 4, "shadow"));
    let err = start_round(&fl, &plan.plan_hash, &http(), NOW).unwrap_err();
    assert!(err.contains("changed"), "{err}");
    assert!(fl.starts().is_empty());
    // And when it is down entirely.
    *fx.reply.lock().unwrap() = (503, "".into());
    assert!(start_round(&fl, &plan.plan_hash, &http(), NOW).is_err());
}

#[test]
fn start_refuses_a_settlement_mode_change() {
    let fx = fixture(200, status_json(3, 0, 0, 4, "shadow"));
    let fl = fl_with_store("start-settle");
    let plan = build_plan(RoundProposal::default(), read_coordinator(Some(&fx.base), &http()), "m.gguf", &gpu_device(), NOW).unwrap();
    fl.remember_plan(plan.clone());
    *fx.reply.lock().unwrap() = (200, status_json(3, 0, 0, 4, "live"));
    let err = start_round(&fl, &plan.plan_hash, &http(), NOW).unwrap_err();
    assert!(err.contains("changed"), "{err}");
}

#[test]
fn start_refuses_a_stale_plan_and_a_second_start() {
    let fx = fixture(200, status_json(3, 0, 0, 4, "shadow"));
    let fl = fl_with_store("start-stale");
    let plan = build_plan(RoundProposal::default(), read_coordinator(Some(&fx.base), &http()), "m.gguf", &gpu_device(), NOW).unwrap();
    fl.remember_plan(plan.clone());
    let err = start_round(&fl, &plan.plan_hash, &http(), NOW + PLAN_TTL_MS + 1).unwrap_err();
    assert!(err.contains("plan again"), "{err}");
    start_round(&fl, &plan.plan_hash, &http(), NOW + 1).unwrap();
    let err = start_round(&fl, &plan.plan_hash, &http(), NOW + 2).unwrap_err();
    assert!(err.contains("already"), "{err}");
    assert_eq!(fl.starts().len(), 1);
}

#[test]
fn plan_memory_is_bounded() {
    let fl = FlRounds::default();
    let mut first = None;
    for i in 0..(MAX_PLANS as u32 + 3) {
        let p = RoundProposal {
            max_trajectories: 10 + i,
            ..Default::default()
        };
        let plan = build_plan(p, CoordinatorView::NotConfigured, "m.gguf", &gpu_device(), NOW).unwrap();
        if first.is_none() {
            first = Some(plan.plan_hash.clone());
        }
        fl.remember_plan(plan);
    }
    assert!(fl.lookup_plan(first.as_deref().unwrap()).is_none(), "oldest evicted");
}

// ---------------------------------------------------------------------------
// coordinator setting persistence
// ---------------------------------------------------------------------------

#[test]
fn coordinator_setting_round_trips_and_validates() {
    let fl = fl_with_store("setting");
    assert_eq!(fl.coordinator_setting(), None);
    fl.set_coordinator_setting(Some("https://pool.example.org/")).unwrap();
    assert_eq!(fl.coordinator_setting().as_deref(), Some("https://pool.example.org"));
    assert!(fl.set_coordinator_setting(Some("http://pool.example.org")).is_err());
    assert_eq!(fl.coordinator_setting().as_deref(), Some("https://pool.example.org"), "bad value keeps the old one");
    let again = FlRounds::with_store(fl.store_path().unwrap().to_path_buf());
    assert_eq!(again.coordinator_setting().as_deref(), Some("https://pool.example.org"));
    fl.set_coordinator_setting(None).unwrap();
    assert_eq!(fl.coordinator_setting(), None);
}

#[test]
fn env_override_wins_and_is_labelled() {
    let fl = fl_with_store("env");
    fl.set_coordinator_setting(Some("https://a.example.org")).unwrap();
    let c = resolve_coordinator(&fl, Some("https://b.example.org"));
    assert_eq!(c.url.as_deref(), Some("https://b.example.org"));
    assert_eq!(c.source, CoordinatorSource::Env);
    let c = resolve_coordinator(&fl, None);
    assert_eq!(c.source, CoordinatorSource::Settings);
    let c = resolve_coordinator(&fl, Some("http://not-loopback.example.org"));
    assert_eq!(c.source, CoordinatorSource::Invalid);
    assert_eq!(c.url, None);
    let empty = fl_with_store("env-empty");
    let c = resolve_coordinator(&empty, None);
    assert_eq!(c.source, CoordinatorSource::None);
}

// ---------------------------------------------------------------------------
// eval scorecards + the gate decision
// ---------------------------------------------------------------------------

fn tools_card(model: &str, valid: f64, correct: f64, args: f64, inj: Option<f64>, adapter: Option<&str>) -> String {
    let mut v = serde_json::json!({
        "model": model,
        "datasetVersion": "toolcall-v1+injection-v1",
        "n": 80,
        "validToolCallRate": valid,
        "correctToolRate": correct,
        "argsOkRate": args,
        "injectionResistRate": inj,
        "failures": [],
        "failureReasons": {},
        "startedAt": "2026-10-01T00:00:00Z",
        "finishedAt": "2026-10-01T00:05:00Z",
        "scoring": "deterministic"
    });
    if let Some(a) = adapter {
        v["adapterSha256"] = serde_json::json!(a);
    }
    v.to_string()
}

fn qa_card(model: &str, pass: f64, kp: f64, cit: Option<f64>, false_abs: Option<f64>, adapter: Option<&str>) -> String {
    let mut sc = serde_json::json!({
        "datasetVersion": "qa-v1",
        "model": model,
        "startedAt": "2026-10-01T00:00:00Z",
        "n": 150,
        "passRate": pass,
        "keyPointCoverage": kp,
        "citationHitRate": 0.5,
        "citationValidity": cit,
        "abstentionRate": 0.9,
        "falseAbstentionRate": false_abs,
        "byCategory": {},
        "failures": [],
        "failureReasons": {}
    });
    if let Some(a) = adapter {
        sc["adapterSha256"] = serde_json::json!(a);
    }
    serde_json::json!({ "scorecard": sc, "items": [] }).to_string()
}

const SHA: &str = "1111111111111111111111111111111111111111111111111111111111111111";

#[test]
fn parses_both_real_scorecard_shapes() {
    let t = parse_tools_scorecard(&tools_card("m", 1.0, 0.98, 0.95, Some(1.0), None)).unwrap();
    assert_eq!(t.n, 80);
    assert_eq!(t.adapter_sha256, None);
    let q = parse_qa_scorecard(&qa_card("m", 0.6, 0.7, Some(0.9), Some(0.1), Some(SHA))).unwrap();
    assert_eq!(q.n, 150);
    assert_eq!(q.adapter_sha256.as_deref(), Some(SHA));
    assert!(parse_tools_scorecard("{}").is_err());
    assert!(parse_qa_scorecard(&tools_card("m", 1.0, 1.0, 1.0, None, None)).is_err());
    // the committed T0 result in eval/results parses as a tools scorecard
    let real = include_str!("../../eval/results/2026-09-30-gemma-4-E4B-it-Q4_0-T0.json");
    let t = parse_tools_scorecard(real).unwrap();
    assert_eq!(t.model, "gemma-4-E4B-it-Q4_0");
}

fn pair(base: &str, cand: &str) -> EvalPair {
    EvalPair {
        base_tools: parse_tools_scorecard(base).unwrap(),
        candidate_tools: parse_tools_scorecard(cand).unwrap(),
        base_qa: None,
        candidate_qa: None,
    }
}

#[test]
fn better_on_every_metric_is_accepted() {
    let d = decide_eval_gate(
        SHA,
        &pair(
            &tools_card("m", 0.9, 0.8, 0.8, Some(0.9), None),
            &tools_card("m", 0.95, 0.85, 0.8, Some(0.95), Some(SHA)),
        ),
    );
    assert_eq!(d.verdict, GateVerdict::Accept, "{:?}", d.reasons);
    assert!(d.composite_candidate > d.composite_base);
}

#[test]
fn equal_scores_are_rejected_because_nothing_improved() {
    let d = decide_eval_gate(
        SHA,
        &pair(
            &tools_card("m", 0.9, 0.8, 0.8, Some(0.9), None),
            &tools_card("m", 0.9, 0.8, 0.8, Some(0.9), Some(SHA)),
        ),
    );
    assert_eq!(d.verdict, GateVerdict::Reject);
    assert!(d.reasons.iter().any(|r| r.contains("did not improve")), "{:?}", d.reasons);
}

#[test]
fn any_regression_rejects_even_if_the_composite_rises() {
    let d = decide_eval_gate(
        SHA,
        &pair(
            &tools_card("m", 0.8, 0.8, 0.8, Some(1.0), None),
            &tools_card("m", 1.0, 1.0, 1.0, Some(0.95), Some(SHA)),
        ),
    );
    assert_eq!(d.verdict, GateVerdict::Reject);
    assert!(d.reasons.iter().any(|r| r.contains("injectionResistRate")), "{:?}", d.reasons);
}

#[test]
fn a_metric_that_vanished_rejects() {
    let d = decide_eval_gate(
        SHA,
        &pair(
            &tools_card("m", 0.8, 0.8, 0.8, Some(1.0), None),
            &tools_card("m", 0.9, 0.9, 0.9, None, Some(SHA)),
        ),
    );
    assert_eq!(d.verdict, GateVerdict::Reject);
    assert!(d.reasons.iter().any(|r| r.contains("missing")), "{:?}", d.reasons);
}

#[test]
fn scorecards_must_be_comparable_and_bound_to_the_adapter() {
    let base = tools_card("m", 0.8, 0.8, 0.8, Some(1.0), None);
    let better = |m: &str, a: Option<&str>| tools_card(m, 0.9, 0.9, 0.9, Some(1.0), a);
    // candidate not stamped with this adapter
    let d = decide_eval_gate(SHA, &pair(&base, &better("m", None)));
    assert_eq!(d.verdict, GateVerdict::Reject);
    let other = "2".repeat(64);
    let d = decide_eval_gate(SHA, &pair(&base, &better("m", Some(&other))));
    assert_eq!(d.verdict, GateVerdict::Reject);
    // base stamped with an adapter is not a base run
    let d = decide_eval_gate(SHA, &pair(&tools_card("m", 0.8, 0.8, 0.8, Some(1.0), Some(SHA)), &better("m", Some(SHA))));
    assert_eq!(d.verdict, GateVerdict::Reject);
    // different base model
    let d = decide_eval_gate(SHA, &pair(&base, &better("other", Some(SHA))));
    assert_eq!(d.verdict, GateVerdict::Reject);
    assert!(d.reasons.iter().any(|r| r.contains("model")), "{:?}", d.reasons);
    // different dataset size
    let mut cand: serde_json::Value = serde_json::from_str(&better("m", Some(SHA))).unwrap();
    cand["n"] = serde_json::json!(79);
    let d = decide_eval_gate(SHA, &pair(&base, &cand.to_string()));
    assert_eq!(d.verdict, GateVerdict::Reject);
}

#[test]
fn qa_pair_counts_and_false_abstention_rising_rejects() {
    let mut p = pair(
        &tools_card("m", 0.8, 0.8, 0.8, Some(1.0), None),
        &tools_card("m", 0.9, 0.9, 0.9, Some(1.0), Some(SHA)),
    );
    p.base_qa = Some(parse_qa_scorecard(&qa_card("m", 0.6, 0.7, Some(0.9), Some(0.1), None)).unwrap());
    p.candidate_qa = Some(parse_qa_scorecard(&qa_card("m", 0.7, 0.8, Some(0.9), Some(0.2), Some(SHA))).unwrap());
    let d = decide_eval_gate(SHA, &p);
    assert_eq!(d.verdict, GateVerdict::Reject);
    assert!(d.reasons.iter().any(|r| r.contains("falseAbstentionRate")), "{:?}", d.reasons);
    p.candidate_qa = Some(parse_qa_scorecard(&qa_card("m", 0.7, 0.8, Some(0.9), Some(0.05), Some(SHA))).unwrap());
    let d = decide_eval_gate(SHA, &p);
    assert_eq!(d.verdict, GateVerdict::Accept, "{:?}", d.reasons);
    // only one side of the QA pair is an error, not a silent skip
    p.base_qa = None;
    let d = decide_eval_gate(SHA, &p);
    assert_eq!(d.verdict, GateVerdict::Reject);
}

// ---------------------------------------------------------------------------
// adapter file + gate record + load authorization
// ---------------------------------------------------------------------------

fn write_adapter(dir: &std::path::Path, body: &[u8]) -> (std::path::PathBuf, String) {
    let p = dir.join("adapter.gguf");
    std::fs::write(&p, body).unwrap();
    let h = hex::encode(<sha2::Sha256 as sha2::Digest>::digest(body));
    (p, h)
}

fn gate_request(dir: &std::path::Path, adapter: &std::path::Path, sha: &str, better: bool) -> AdapterGateRequest {
    let base = dir.join("base.json");
    let cand = dir.join("cand.json");
    std::fs::write(&base, tools_card("m", 0.8, 0.8, 0.8, Some(1.0), None)).unwrap();
    let c = if better { 0.9 } else { 0.7 };
    std::fs::write(&cand, tools_card("m", c, c, c, Some(1.0), Some(sha))).unwrap();
    AdapterGateRequest {
        adapter_path: adapter.to_string_lossy().to_string(),
        expected_sha256: sha.to_string(),
        base_tools_path: base.to_string_lossy().to_string(),
        candidate_tools_path: cand.to_string_lossy().to_string(),
        base_qa_path: None,
        candidate_qa_path: None,
        round_result_path: None,
    }
}

#[test]
fn gate_refuses_a_non_gguf_or_wrong_hash_adapter() {
    let d = tmpdir("gate-file");
    let (p, h) = write_adapter(&d, b"NOTGGUF-bytes");
    let err = evaluate_adapter(&gate_request(&d, &p, &h, true), NOW).unwrap_err();
    assert!(err.contains("GGUF"), "{err}");
    let (p, _h) = write_adapter(&d, b"GGUF\x03\x00\x00\x00adapter");
    let err = evaluate_adapter(&gate_request(&d, &p, &"0".repeat(64), true), NOW).unwrap_err();
    assert!(err.contains("does not match"), "{err}");
    let err = evaluate_adapter(&gate_request(&d, &p, "xyz", true), NOW).unwrap_err();
    assert!(err.contains("sha256"), "{err}");
}

#[test]
fn gate_records_accept_and_reject_bound_to_the_hash() {
    let d = tmpdir("gate-rec");
    let (p, h) = write_adapter(&d, b"GGUF\x03\x00\x00\x00adapter-a");
    let rec = evaluate_adapter(&gate_request(&d, &p, &h, true), NOW).unwrap();
    assert_eq!(rec.adapter_sha256, h);
    assert_eq!(rec.decision.verdict, GateVerdict::Accept);
    assert_eq!(rec.base_model, "m");
    let worse = evaluate_adapter(&gate_request(&d, &p, &h, false), NOW).unwrap();
    assert_eq!(worse.decision.verdict, GateVerdict::Reject);
}

#[test]
fn load_needs_an_accepted_gate_for_this_exact_file_and_base() {
    let d = tmpdir("load");
    let store = tmpdir("load-adapters");
    let fl = fl_with_store("load-store");
    let (p, h) = write_adapter(&d, b"GGUF\x03\x00\x00\x00adapter-b");
    // no record
    assert!(authorize_load(&fl, &h, "m.gguf", &store).unwrap_err().contains("eval gate"));
    // rejected record
    fl.record_gate(evaluate_adapter(&gate_request(&d, &p, &h, false), NOW).unwrap()).unwrap();
    assert!(authorize_load(&fl, &h, "m.gguf", &store).unwrap_err().contains("rejected"));
    // accepted record supersedes; the served file is a content-addressed copy in the app's store
    fl.record_gate(evaluate_adapter(&gate_request(&d, &p, &h, true), NOW).unwrap()).unwrap();
    let served = authorize_load(&fl, &h, "m.gguf", &store).unwrap();
    assert_eq!(served, adapter_store_path(&store, &h));
    assert_eq!(std::fs::read(&served).unwrap(), std::fs::read(&p).unwrap());
    assert_eq!(
        authorize_load(&fl, &h, "M.GGUF", &store).unwrap(),
        served,
        "file stem compare is case-insensitive; an existing good copy is reused"
    );
    // a different base model is refused
    assert!(authorize_load(&fl, &h, "other.gguf", &store).unwrap_err().contains("base model"));
}

#[test]
fn a_source_swapped_after_the_gate_is_refused_and_after_load_is_not_served() {
    let d = tmpdir("swap");
    let store = tmpdir("swap-adapters");
    let fl = FlRounds::default();
    let (p, h) = write_adapter(&d, b"GGUF\x03\x00\x00\x00adapter-e");
    fl.record_gate(evaluate_adapter(&gate_request(&d, &p, &h, true), NOW).unwrap()).unwrap();
    // swapped between gate and load: refused, no copy left behind
    std::fs::write(&p, b"GGUF\x03\x00\x00\x00swapped").unwrap();
    assert!(authorize_load(&fl, &h, "m.gguf", &store).unwrap_err().contains("changed"));
    assert!(!adapter_store_path(&store, &h).exists());
    // restore, load, then swap the source: the served copy (what a crash restart re-reads) holds
    std::fs::write(&p, b"GGUF\x03\x00\x00\x00adapter-e").unwrap();
    let served = authorize_load(&fl, &h, "m.gguf", &store).unwrap();
    std::fs::write(&p, b"GGUF\x03\x00\x00\x00swapped-again").unwrap();
    assert_eq!(std::fs::read(&served).unwrap(), b"GGUF\x03\x00\x00\x00adapter-e");
    // a tampered copy is detected and replaced from a good source, or refused if the source is bad
    std::fs::write(&served, b"GGUF\x03\x00\x00\x00tampered").unwrap();
    assert!(authorize_load(&fl, &h, "m.gguf", &store).is_err());
    std::fs::write(&p, b"GGUF\x03\x00\x00\x00adapter-e").unwrap();
    let again = authorize_load(&fl, &h, "m.gguf", &store).unwrap();
    assert_eq!(std::fs::read(&again).unwrap(), b"GGUF\x03\x00\x00\x00adapter-e");
}

#[test]
fn a_later_reject_revokes_an_earlier_accept() {
    let d = tmpdir("revoke");
    let store = tmpdir("revoke-adapters");
    let fl = FlRounds::default();
    let (p, h) = write_adapter(&d, b"GGUF\x03\x00\x00\x00adapter-c");
    fl.record_gate(evaluate_adapter(&gate_request(&d, &p, &h, true), NOW).unwrap()).unwrap();
    fl.record_gate(evaluate_adapter(&gate_request(&d, &p, &h, false), NOW + 1).unwrap()).unwrap();
    assert!(authorize_load(&fl, &h, "m.gguf", &store).is_err());
}

#[test]
fn gate_records_persist() {
    let d = tmpdir("persist");
    let fl = fl_with_store("persist-store");
    let (p, h) = write_adapter(&d, b"GGUF\x03\x00\x00\x00adapter-d");
    fl.record_gate(evaluate_adapter(&gate_request(&d, &p, &h, true), NOW).unwrap()).unwrap();
    let again = FlRounds::with_store(fl.store_path().unwrap().to_path_buf());
    assert_eq!(again.gate(&h).unwrap().decision.verdict, GateVerdict::Accept);
}

#[test]
fn a_corrupt_store_starts_empty_and_is_not_overwritten_silently() {
    let dir = tmpdir("corrupt");
    let path = dir.join(STORE_FILE);
    std::fs::write(&path, b"{not json").unwrap();
    let fl = FlRounds::with_store(path.clone());
    assert_eq!(fl.coordinator_setting(), None);
    assert!(fl.load_error().is_some());
    // writes are refused while the file is unreadable, so nothing the member had is clobbered
    assert!(fl.set_coordinator_setting(Some("https://a.example.org")).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"{not json");
}

// ---------------------------------------------------------------------------
// llama-server argv: --lora
// ---------------------------------------------------------------------------

#[test]
fn serve_argv_carries_the_loaded_adapter_and_a_model_switch_clears_it() {
    let dir = tmpdir("argv");
    let m = crate::serve::LlamaServerManager::new(
        dir.join("no-such-bin"),
        dir.join("m.gguf"),
        dir.join("crash.jsonl"),
        18099,
    );
    assert!(!m.spawn_args_for_test().iter().any(|a| a == "--lora"));
    m.set_lora(Some(dir.join("adapter.gguf")));
    let args = m.spawn_args_for_test();
    let i = args.iter().position(|a| a == "--lora").unwrap();
    assert_eq!(args[i + 1], dir.join("adapter.gguf").to_string_lossy());
    assert_eq!(m.lora(), Some(dir.join("adapter.gguf")));
    // Switching the base model (binary missing, so the restart itself fails) still drops the
    // adapter: an adapter is tied to the base it was trained on.
    let _ = m.select_model(dir.join("other.gguf"), true);
    assert_eq!(m.lora(), None);
    assert!(!m.spawn_args_for_test().iter().any(|a| a == "--lora"));
}

#[test]
fn a_new_gate_record_unloads_the_served_adapter_unless_it_still_fits() {
    let store = std::path::Path::new("/adapters");
    let sha = SHA;
    let served = adapter_store_path(store, sha);
    let mut rec = AdapterGateRecord {
        adapter_sha256: sha.into(),
        adapter_path: "/src/a.gguf".into(),
        base_model: "m".into(),
        decided_at_ms: NOW,
        round: None,
        decision: GateDecision {
            verdict: GateVerdict::Accept,
            reasons: vec![],
            metrics: vec![],
            composite_base: 0.5,
            composite_candidate: 0.6,
        },
    };
    // accepted again for the served base: stays
    assert!(!must_unload_after_gate(&rec, Some(&served), store, "m.gguf"));
    // not the served adapter: nothing to do
    assert!(!must_unload_after_gate(&rec, None, store, "m.gguf"));
    let other = adapter_store_path(store, &"2".repeat(64));
    assert!(!must_unload_after_gate(&rec, Some(&other), store, "m.gguf"));
    // accepted, but measured on another base than the one being served (found by TLC)
    rec.base_model = "other".into();
    assert!(must_unload_after_gate(&rec, Some(&served), store, "m.gguf"));
    // rejected
    rec.base_model = "m".into();
    rec.decision.verdict = GateVerdict::Reject;
    assert!(must_unload_after_gate(&rec, Some(&served), store, "m.gguf"));
}

// ===========================================================================
// HUP-S9.4 rest (fan-out 6): the local-devnet round flow (citrate-chain docs/fl/FL_ROUND_V1.md),
// per-round consent for the device worker (D-29), and re-applying a loaded adapter after a
// restart. Written red-first: these names did not exist when this block was added.
// ===========================================================================

/// The receipt `scripts/fl/devnet-round-e2e.sh` (citrate-chain) wrote for a three-device round
/// on a throwaway local devnet, copied as data. The trainer was the fixture trainer.
const DEVNET_RECEIPT: &str = include_str!("../tests/fixtures/fl/devnet-round-receipt-2026-10-01.json");
const DEVNET_ROUND: &str = "0x29a0bbad3829ef8c62a4ba4b8c6bb61f0e893db107b033a80e55171f159a3dfe";
const DEVNET_ADAPTER: &str = "ae4ed1b17bac676a1890f3b55e3f62a6a094517dfc3df318523507b279b766bd";
const DEVNET_DIGEST: &str = "0x693073fe0498b87124bfa8b7629c6e6ef7c628de047e1fe81651815255bc9ecf";

fn receipt_value() -> serde_json::Value {
    serde_json::from_str(DEVNET_RECEIPT).unwrap()
}

/// A receipt whose merged adapter is `sha` (the devnet one otherwise).
fn receipt_for(sha: &str) -> String {
    let mut v = receipt_value();
    v["aggregate"]["adapter_sha256"] = serde_json::Value::String(sha.into());
    v.to_string()
}

#[test]
fn the_devnet_round_receipt_parses_and_names_the_merged_adapter() {
    let r = parse_round_result(DEVNET_RECEIPT).unwrap();
    assert_eq!(r.round_id, DEVNET_ROUND);
    assert_eq!(r.adapter_sha256, DEVNET_ADAPTER);
    assert_eq!(r.record_digest, DEVNET_DIGEST);
    assert_eq!(r.chain_id, 1337);
    assert_eq!(r.participants, 3);
    assert_eq!(r.ledger, "0x768cced6b5d55bca63e76ef806f1eef06ef68bba");
}

#[test]
fn a_round_result_is_refused_unless_accepted_and_replayed_clean() {
    type Edit = fn(&mut serde_json::Value);
    let cases: Vec<(&str, Edit)> = vec![
        ("rejected round", |v| v["round0_status"] = "Rejected".into()),
        ("no status", |v| {
            v.as_object_mut().unwrap().remove("round0_status");
        }),
        ("replay mismatch", |v| v["replay"]["mismatches"] = serde_json::json!(["chunk 3"])),
        ("merged adapter unchecked", |v| v["replay"]["merged_adapter_checked"] = false.into()),
        ("chain digest differs", |v| v["record_digest"]["chain"] = format!("0x{}", "0".repeat(64)).into()),
        ("replay digest differs", |v| v["replay"]["record_digest"] = format!("0x{}", "1".repeat(64)).into()),
        ("aggregate names another round", |v| v["aggregate"]["round_id"] = format!("0x{}", "2".repeat(64)).into()),
        ("replay names another round", |v| v["replay"]["round_id"] = format!("0x{}", "3".repeat(64)).into()),
        ("adapter hash not hex", |v| v["aggregate"]["adapter_sha256"] = "zz".into()),
        ("round id not hex", |v| {
            v["round_id"] = "0x1234".into();
        }),
        ("fewer than three devices", |v| v["aggregate"]["participants"] = 2.into()),
        ("no replay", |v| {
            v.as_object_mut().unwrap().remove("replay");
        }),
    ];
    for (what, edit) in cases {
        let mut v = receipt_value();
        edit(&mut v);
        assert!(parse_round_result(&v.to_string()).is_err(), "accepted: {what}");
    }
    // A top-level `status` is the general name; `round0_status` is the devnet script's.
    let mut v = receipt_value();
    v.as_object_mut().unwrap().remove("round0_status");
    v["status"] = "Accepted".into();
    assert!(parse_round_result(&v.to_string()).is_ok());
    assert!(parse_round_result("{}").is_err());
    assert!(parse_round_result(&"x".repeat(MAX_ROUND_RESULT_BYTES + 1)).is_err());
}

#[test]
fn the_gate_binds_an_adapter_to_its_round_result() {
    let d = tmpdir("gate-round");
    let (p, h) = write_adapter(&d, b"GGUF\x03\x00\x00\x00merged-adapter");
    let rr = d.join("round.json");
    std::fs::write(&rr, receipt_for(&h)).unwrap();
    let mut req = gate_request(&d, &p, &h, true);
    req.round_result_path = Some(rr.to_string_lossy().to_string());
    let rec = evaluate_adapter(&req, NOW).unwrap();
    let round = rec.round.clone().unwrap();
    assert_eq!(round.round_id, DEVNET_ROUND);
    assert_eq!(round.record_digest, DEVNET_DIGEST);
    assert_eq!(round.chain_id, 1337);
    assert_eq!(rec.decision.verdict, GateVerdict::Accept);
    // With a round result, the expected hash may be left empty: it is the round's merged adapter.
    req.expected_sha256 = String::new();
    assert_eq!(evaluate_adapter(&req, NOW).unwrap().adapter_sha256, h);
    // A round result naming another adapter is refused, whatever hash the member typed.
    std::fs::write(&rr, receipt_for(&"4".repeat(64))).unwrap();
    req.expected_sha256 = h.clone();
    let err = evaluate_adapter(&req, NOW).unwrap_err();
    assert!(err.contains("round"), "{err}");
    // Without a round result the record says so (None), as before.
    let plain = evaluate_adapter(&gate_request(&d, &p, &h, true), NOW).unwrap();
    assert!(plain.round.is_none());
    // And an empty expected hash without a round result is still refused.
    let mut no_hash = gate_request(&d, &p, &h, true);
    no_hash.expected_sha256 = String::new();
    assert!(evaluate_adapter(&no_hash, NOW).is_err());
}

#[test]
fn a_round_id_in_the_proposal_is_validated_and_bound_into_the_plan_hash() {
    let bad = RoundProposal {
        round_id: Some("0x1234".into()),
        ..Default::default()
    };
    assert!(bad.validate().is_err());
    let upper = RoundProposal {
        round_id: Some(DEVNET_ROUND.to_ascii_uppercase().replacen("0X", "0x", 1)),
        ..Default::default()
    };
    assert!(upper.validate().is_ok());
    // No round id: the proposal serializes exactly as before, so earlier plan hashes hold.
    let plain = serde_json::to_value(RoundProposal::default()).unwrap();
    assert!(plain.get("roundId").is_none(), "{plain}");
    let with = RoundProposal {
        round_id: Some(DEVNET_ROUND.into()),
        ..Default::default()
    };
    let view = live_view(3, 0, 0, "shadow");
    let a = build_plan(RoundProposal::default(), view.clone(), "m.gguf", &gpu_device(), NOW).unwrap();
    let b = build_plan(with, view, "m.gguf", &gpu_device(), NOW).unwrap();
    assert_ne!(a.plan_hash, b.plan_hash);
    assert_eq!(b.proposal.round_id.as_deref(), Some(DEVNET_ROUND));
    assert!(b.explain.compute.contains(DEVNET_ROUND), "{}", b.explain.compute);
}

fn plan_for_round(fl: &FlRounds, base: &str, round: Option<&str>, trajectories: u32) -> RoundPlan {
    let p = RoundProposal {
        round_id: round.map(str::to_string),
        max_trajectories: trajectories,
        ..Default::default()
    };
    let plan = build_plan(p, read_coordinator(Some(base), &http()), "m.gguf", &gpu_device(), NOW).unwrap();
    fl.remember_plan(plan.clone());
    plan
}

fn consent_rounds_on_disk(fl: &FlRounds) -> Vec<String> {
    let raw = std::fs::read_to_string(fl.consent_path().unwrap()).unwrap();
    let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
    v["rounds"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap().to_string())
        .collect()
}

#[test]
fn a_start_for_a_round_writes_the_device_worker_consent_file() {
    let fx = fixture(200, status_json(3, 0, 0, 4, "shadow"));
    let fl = fl_with_store("consent");
    let plan = plan_for_round(&fl, &fx.base, Some(DEVNET_ROUND), 500);
    let r = start_round(&fl, &plan.plan_hash, &http(), NOW + 1).unwrap();
    assert_eq!(r.round_id.as_deref(), Some(DEVNET_ROUND));
    let path = r.consent_file.clone().unwrap();
    assert_eq!(std::path::PathBuf::from(&path), fl.consent_path().unwrap());
    assert!(r.note.contains("CITRATE_FL_CONSENT_FILE"), "{}", r.note);
    assert!(!r.training_started);
    // The exact shape citrate-compute-pool's training-worker reads: {"rounds":["0x…"]}.
    assert_eq!(consent_rounds_on_disk(&fl), vec![DEVNET_ROUND.to_string()]);
    assert_eq!(fl.consented_rounds().unwrap(), vec![DEVNET_ROUND.to_string()]);
    assert_eq!(fl.starts()[0].round_id.as_deref(), Some(DEVNET_ROUND));
    // A second plan for the same round (different numbers) cannot consent twice.
    let again = plan_for_round(&fl, &fx.base, Some(DEVNET_ROUND), 400);
    let err = start_round(&fl, &again.plan_hash, &http(), NOW + 2).unwrap_err();
    assert!(err.contains("already"), "{err}");
    assert_eq!(fl.starts().len(), 1);
}

#[test]
fn consent_can_be_withdrawn_and_is_then_gone_from_the_worker_file() {
    let fx = fixture(200, status_json(3, 0, 0, 4, "shadow"));
    let fl = fl_with_store("consent-revoke");
    let plan = plan_for_round(&fl, &fx.base, Some(DEVNET_ROUND), 500);
    start_round(&fl, &plan.plan_hash, &http(), NOW + 1).unwrap();
    fl.revoke_consent(&DEVNET_ROUND.to_ascii_uppercase().replacen("0X", "0x", 1), NOW + 5)
        .unwrap();
    assert!(consent_rounds_on_disk(&fl).is_empty());
    assert_eq!(fl.starts()[0].revoked_at_ms, Some(NOW + 5));
    // Withdrawing what was never given is an error, not a silent success.
    let err = fl.revoke_consent(DEVNET_ROUND, NOW + 6).unwrap_err();
    assert!(err.contains("no consent"), "{err}");
    // Persisted across a restart.
    let again = FlRounds::with_store(fl.store_path().unwrap().to_path_buf());
    assert_eq!(again.starts()[0].revoked_at_ms, Some(NOW + 5));
    assert!(again.consented_rounds().unwrap().is_empty());
    // After withdrawing, a fresh approval for the same round is allowed again.
    let fresh = plan_for_round(&again, &fx.base, Some(DEVNET_ROUND), 300);
    start_round(&again, &fresh.plan_hash, &http(), NOW + 7).unwrap();
    assert_eq!(consent_rounds_on_disk(&again), vec![DEVNET_ROUND.to_string()]);
}

#[test]
fn a_round_listed_in_the_worker_file_can_always_be_withdrawn() {
    // The panel lists the worker file and offers Withdraw for every round in it. A round can be
    // there without a live start record (the app stopped between writing consent and recording
    // the start, or the file was edited by hand). Withdrawing lowers authority, so it must work.
    let fl = fl_with_store("consent-orphan");
    let cp = fl.consent_path().unwrap();
    std::fs::create_dir_all(cp.parent().unwrap()).unwrap();
    std::fs::write(&cp, format!("{{\"rounds\":[\"{DEVNET_ROUND}\"]}}")).unwrap();
    assert!(fl.starts().is_empty());
    fl.revoke_consent(DEVNET_ROUND, NOW + 1).unwrap();
    assert!(consent_rounds_on_disk(&fl).is_empty());
    // Still an error when the round is in neither place.
    let err = fl.revoke_consent(DEVNET_ROUND, NOW + 2).unwrap_err();
    assert!(err.contains("no consent"), "{err}");
}

#[test]
fn a_start_without_a_round_writes_no_consent() {
    let fx = fixture(200, status_json(3, 0, 0, 4, "shadow"));
    let fl = fl_with_store("consent-none");
    let plan = plan_for_round(&fl, &fx.base, None, 500);
    let r = start_round(&fl, &plan.plan_hash, &http(), NOW + 1).unwrap();
    assert!(r.consent_file.is_none());
    assert!(r.round_id.is_none());
    assert!(!fl.consent_path().unwrap().exists());
    assert!(fl.consented_rounds().unwrap().is_empty());
}

#[test]
fn an_unreadable_consent_file_is_not_overwritten_and_nothing_starts() {
    let fx = fixture(200, status_json(3, 0, 0, 4, "shadow"));
    let fl = fl_with_store("consent-corrupt");
    let cp = fl.consent_path().unwrap();
    std::fs::create_dir_all(cp.parent().unwrap()).unwrap();
    std::fs::write(&cp, b"{not json").unwrap();
    let plan = plan_for_round(&fl, &fx.base, Some(DEVNET_ROUND), 500);
    assert!(start_round(&fl, &plan.plan_hash, &http(), NOW + 1).is_err());
    assert_eq!(std::fs::read(&cp).unwrap(), b"{not json");
    assert!(fl.starts().is_empty());
}

#[test]
fn a_round_start_needs_a_place_for_the_consent_file() {
    let fx = fixture(200, status_json(3, 0, 0, 4, "shadow"));
    let fl = FlRounds::default();
    let plan = plan_for_round(&fl, &fx.base, Some(DEVNET_ROUND), 500);
    assert!(start_round(&fl, &plan.plan_hash, &http(), NOW + 1).is_err());
    assert!(fl.starts().is_empty());
}

#[test]
fn a_loaded_adapter_is_remembered_and_reapplied_after_a_restart() {
    let d = tmpdir("restore");
    let store = tmpdir("restore-adapters");
    let fl = fl_with_store("restore-store");
    let (p, h) = write_adapter(&d, b"GGUF\x03\x00\x00\x00adapter-r");
    fl.record_gate(evaluate_adapter(&gate_request(&d, &p, &h, true), NOW).unwrap()).unwrap();
    let served = authorize_load(&fl, &h, "m.gguf", &store).unwrap();
    fl.set_active(&h, "m.gguf").unwrap();
    // The app restarts: a fresh state over the same file, nothing served yet.
    let again = FlRounds::with_store(fl.store_path().unwrap().to_path_buf());
    assert_eq!(again.active().unwrap().sha256, h);
    assert_eq!(restore_active(&again, "m.gguf", &store).unwrap(), Some(served.clone()));
    // On another base it is not applied, and it is kept for when that base is served again.
    assert_eq!(restore_active(&again, "other.gguf", &store).unwrap(), None);
    assert!(again.active().is_some());
    // The copy is re-hashed: a damaged copy with a changed source is refused, not served.
    std::fs::write(&served, b"GGUF\x03\x00\x00\x00tampered").unwrap();
    std::fs::write(&p, b"GGUF\x03\x00\x00\x00changed").unwrap();
    assert!(restore_active(&again, "m.gguf", &store).is_err());
}

#[test]
fn unload_a_reject_or_a_base_switch_ends_the_reapply() {
    let d = tmpdir("restore-end");
    let store = tmpdir("restore-end-adapters");
    let fl = fl_with_store("restore-end-store");
    let (p, h) = write_adapter(&d, b"GGUF\x03\x00\x00\x00adapter-s");
    fl.record_gate(evaluate_adapter(&gate_request(&d, &p, &h, true), NOW).unwrap()).unwrap();
    authorize_load(&fl, &h, "m.gguf", &store).unwrap();
    // unload
    fl.set_active(&h, "m.gguf").unwrap();
    fl.forget_active().unwrap();
    assert_eq!(restore_active(&fl, "m.gguf", &store).unwrap(), None);
    // a later REJECT for that adapter
    fl.set_active(&h, "m.gguf").unwrap();
    fl.record_gate(evaluate_adapter(&gate_request(&d, &p, &h, false), NOW + 1).unwrap()).unwrap();
    assert!(fl.active().is_none());
    // a re-ACCEPT does not bring it back by itself: loading again is the member's click
    fl.record_gate(evaluate_adapter(&gate_request(&d, &p, &h, true), NOW + 2).unwrap()).unwrap();
    assert_eq!(restore_active(&fl, "m.gguf", &store).unwrap(), None);
    // an ACCEPT measured on another base than the one it was loaded on also ends it
    fl.set_active(&h, "m.gguf").unwrap();
    let mut other = evaluate_adapter(&gate_request(&d, &p, &h, true), NOW + 3).unwrap();
    other.base_model = "other".into();
    fl.record_gate(other).unwrap();
    assert!(fl.active().is_none());
    // a gate record for a different adapter leaves it alone
    fl.record_gate(evaluate_adapter(&gate_request(&d, &p, &h, true), NOW + 4).unwrap()).unwrap();
    fl.set_active(&h, "m.gguf").unwrap();
    let (p2, h2) = {
        let d2 = tmpdir("restore-end-2");
        let r = write_adapter(&d2, b"GGUF\x03\x00\x00\x00adapter-t");
        (r.0, r.1)
    };
    let d2 = p2.parent().unwrap().to_path_buf();
    fl.record_gate(evaluate_adapter(&gate_request(&d2, &p2, &h2, false), NOW + 5).unwrap()).unwrap();
    assert_eq!(fl.active().unwrap().sha256, h);
    // set_active refuses an adapter that is not accepted for that base
    assert!(fl.set_active(&h2, "m.gguf").is_err());
    assert!(fl.set_active(&h, "other.gguf").is_err());
}

#[test]
fn two_concurrent_starts_for_one_round_consent_once() {
    let fx = fixture(200, status_json(3, 0, 0, 4, "shadow"));
    let fl = Arc::new(fl_with_store("consent-race"));
    let a = plan_for_round(&fl, &fx.base, Some(DEVNET_ROUND), 500);
    let b = plan_for_round(&fl, &fx.base, Some(DEVNET_ROUND), 499);
    let hs: Vec<_> = [a.plan_hash, b.plan_hash]
        .into_iter()
        .map(|h| {
            let fl = fl.clone();
            std::thread::spawn(move || start_round(&fl, &h, &http(), NOW + 1).is_ok())
        })
        .collect();
    let ok: usize = hs.into_iter().map(|h| usize::from(h.join().unwrap())).sum();
    assert_eq!(ok, 1);
    assert_eq!(fl.starts().len(), 1);
    assert_eq!(consent_rounds_on_disk(&fl), vec![DEVNET_ROUND.to_string()]);
}

#[test]
fn the_start_path_puts_the_remembered_adapter_into_the_server_argv() {
    let d = tmpdir("reapply");
    let store = tmpdir("reapply-adapters");
    let fl = fl_with_store("reapply-store");
    let (p, h) = write_adapter(&d, b"GGUF\x03\x00\x00\x00adapter-u");
    fl.record_gate(evaluate_adapter(&gate_request(&d, &p, &h, true), NOW).unwrap()).unwrap();
    authorize_load(&fl, &h, "m.gguf", &store).unwrap();
    fl.set_active(&h, "m.gguf").unwrap();
    let again = FlRounds::with_store(fl.store_path().unwrap().to_path_buf());
    let mgr = crate::serve::LlamaServerManager::new(d.join("no-such-bin"), d.join("m.gguf"), d.join("crash.jsonl"), 18098);
    reapply_into(&again, &mgr, &store);
    assert_eq!(mgr.lora(), Some(adapter_store_path(&store, &h)));
    let args = mgr.spawn_args_for_test();
    let i = args.iter().position(|a| a == "--lora").unwrap();
    assert_eq!(args[i + 1], adapter_store_path(&store, &h).to_string_lossy());
    assert!(again.restore_error().is_none());
    // An adapter already set is left alone.
    let other = d.join("other-adapter.gguf");
    mgr.set_lora(Some(other.clone()));
    reapply_into(&again, &mgr, &store);
    assert_eq!(mgr.lora(), Some(other));
    // On another base nothing is set; a failed re-apply is reported, not served.
    let mgr2 = crate::serve::LlamaServerManager::new(d.join("no-such-bin"), d.join("other.gguf"), d.join("crash.jsonl"), 18097);
    reapply_into(&again, &mgr2, &store);
    assert_eq!(mgr2.lora(), None);
    std::fs::write(adapter_store_path(&store, &h), b"GGUF\x03\x00\x00\x00tampered").unwrap();
    std::fs::write(&p, b"GGUF\x03\x00\x00\x00changed").unwrap();
    let mgr3 = crate::serve::LlamaServerManager::new(d.join("no-such-bin"), d.join("m.gguf"), d.join("crash.jsonl"), 18096);
    reapply_into(&again, &mgr3, &store);
    assert_eq!(mgr3.lora(), None);
    assert!(again.restore_error().unwrap().contains("was not put back"));
}

#[test]
fn only_a_base_switch_with_an_adapter_set_drops_it() {
    let a = std::path::Path::new("/m/a.gguf");
    let b = std::path::Path::new("/m/b.gguf");
    let l = std::path::Path::new("/adapters/x.gguf");
    assert!(base_switch_drops_adapter(Some(l), a, b));
    assert!(!base_switch_drops_adapter(Some(l), a, a));
    assert!(!base_switch_drops_adapter(None, a, b));
}

/// HUP-S9.4 hardening: gate decisions, loads and unloads are serialized, and a load names the base
/// model it was authorized for, so a reject or a model switch in between cannot leave an adapter on.
#[test]
fn adapter_gate_load_and_unload_run_one_at_a_time_and_loads_are_bound_to_the_base() {
    let src = include_str!("fl_rounds.rs");
    for cmd in ["pub async fn fl_adapter_gate(", "pub async fn fl_adapter_load(", "pub async fn fl_adapter_unload("] {
        let i = src.find(cmd).expect(cmd);
        let body = &src[i..i + src[i..].find("\n}\n").expect("end")];
        assert!(body.contains("adapter_lock()"), "{cmd} must hold the adapter lock");
    }
    let i = src.find("pub async fn fl_adapter_load(").expect("load");
    let body = &src[i..i + src[i..].find("\n}\n").expect("end")];
    assert!(body.contains("LoraChoice::For"), "a load names its base model");
}
