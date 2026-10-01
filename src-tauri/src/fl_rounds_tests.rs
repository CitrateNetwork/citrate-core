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
fn data_explanation_is_honest_that_the_trajectory_export_is_not_wired() {
    let plan = build_plan(
        RoundProposal::default(),
        live_view(3, 0, 0, "shadow"),
        "m.gguf",
        &gpu_device(),
        NOW,
    )
    .unwrap();
    assert!(
        plan.explain.data.contains("not wired"),
        "{}",
        plan.explain.data
    );
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
