// BC-3.2 — llama-server sidecar + provider routing tests. RED-FIRST.
//
// CI-safe: NO real llama-server. The supervisor stub pattern (a long-lived shell
// binary resolved shell-free at spawn time, as in supervisor_tests.rs /
// node_tests.rs) proves the spawn/health/stop wiring WITHOUT the heavy binary.
// The provider-selection logic is a PURE fn over honest inputs, unit-tested for
// each state. A real llama-server inference is a documented manual proof.

use super::*;
use std::path::PathBuf;

/// A long-lived stub child binary that exists on the host (issue #47). Unix:
/// `/bin/sleep` (a coreutil on macOS + Linux). Windows: `ping.exe` (always present
/// in System32; kept alive by [`long_lived_args`]). The Windows path is verified by
/// the team; only the Unix path is built/run here.
fn sleep_bin() -> PathBuf {
    #[cfg(unix)]
    {
        for c in ["/bin/sleep", "/usr/bin/sleep"] {
            let p = PathBuf::from(c);
            if p.exists() {
                return p;
            }
        }
        panic!("no sleep binary");
    }
    #[cfg(windows)]
    {
        let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".to_string());
        PathBuf::from(root).join("System32").join("ping.exe")
    }
}

/// The argv that keeps [`sleep_bin`] alive for a lifecycle test. Unix: `sleep 3600`.
/// Windows: `ping 127.0.0.1 -n 999` (~999s of pings). Paired with [`sleep_bin`] so
/// the binary and its keep-alive args always match on each platform.
fn long_lived_args() -> Vec<String> {
    #[cfg(unix)]
    {
        vec!["3600".to_string()]
    }
    #[cfg(windows)]
    {
        vec!["127.0.0.1".to_string(), "-n".to_string(), "999".to_string()]
    }
}

/// A fresh unique temp dir.
fn tmp_dir(tag: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    p.push(format!(
        "citrate-core-serve-{tag}-{nanos}-{:?}",
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&p).expect("mk tmpdir");
    p
}

/// Build a manager over a stub (sleep) binary + a temp model path that EXISTS
/// (so the "model ready" gate passes) + a crash-record path.
fn stub_manager(tag: &str) -> (LlamaServerManager, PathBuf) {
    let dir = tmp_dir(tag);
    let model_path = dir.join("model.gguf");
    std::fs::write(&model_path, b"GGUF-fake").unwrap();
    let crash = dir.join("crash.jsonl");
    let mgr = LlamaServerManager::new(sleep_bin(), model_path, crash, 18081);
    (mgr, dir)
}

// ---------------------------------------------------------------------------
// (1) fail closed: start refuses when the model is NOT ready (no Ready model).
// ---------------------------------------------------------------------------

#[test]
fn start_refuses_when_model_not_ready() {
    let dir = tmp_dir("nomodel");
    // A model path that does NOT exist → not ready.
    let model_path = dir.join("absent.gguf");
    let crash = dir.join("crash.jsonl");
    let mgr = LlamaServerManager::new(sleep_bin(), model_path, crash, 18082);
    let r = mgr.start_if_ready(false);
    assert!(matches!(r, Err(ServeError::ModelNotReady)), "got {r:?}");
    assert_eq!(mgr.status().state, "stopped");
}

// ---------------------------------------------------------------------------
// (2) fail closed: start refuses when the llama-server binary isn't bundled.
// ---------------------------------------------------------------------------

#[test]
fn start_refuses_when_binary_missing() {
    let dir = tmp_dir("nobin");
    let model_path = dir.join("model.gguf");
    std::fs::write(&model_path, b"GGUF").unwrap();
    let crash = dir.join("crash.jsonl");
    let mgr = LlamaServerManager::new(
        PathBuf::from("/nonexistent/llama-server-xyz"),
        model_path,
        crash,
        18083,
    );
    // The model IS ready, but the binary is not bundled → honest BinaryNotFound.
    let r = mgr.start_if_ready(true);
    assert!(matches!(r, Err(ServeError::BinaryNotFound(_))), "got {r:?}");
}

// ---------------------------------------------------------------------------
// (3) spawn wiring: start (model ready + binary present) reaches Running under
// the supervisor; stop releases cleanly (no orphan). Uses the sleep stub.
// ---------------------------------------------------------------------------

#[test]
fn start_reaches_running_then_stop_is_clean() {
    // F-2 (deterministic): two overrides make this test race-free.
    //  1. A long-lived stub argv (`sleep 3600`): the production llama-server argv
    //     (`-m ... --host ...`) fed to `/bin/sleep` is REJECTED as bad usage and
    //     the child exits instantly → crash-loop, so "Running" was only ever a
    //     transient window the poll caught by luck. A valid duration arg makes the
    //     child genuinely SIT in Running.
    //  2. A long /health interval: with a real long-lived child, the default 5s
    //     probe would fire against the never-answering stub port and flip
    //     Running→Unhealthy→Backoff mid-poll. A long interval removes that race.
    // The health-driven restart itself keeps its own dedicated coverage elsewhere.
    let (mgr, _dir) = stub_manager("wiring");
    let mgr = mgr
        .with_spawn_args(long_lived_args())
        .with_health_interval(std::time::Duration::from_secs(3600));
    mgr.start_if_ready(true).expect("stub llama-server starts");
    // Poll status until Running.
    let mut running = false;
    for _ in 0..100 {
        if mgr.status().state == "running" {
            running = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert!(running, "llama-server stub must reach Running under the supervisor");
    mgr.stop();
    assert_eq!(mgr.status().state, "stopped");
}

// ---------------------------------------------------------------------------
// (4) double start is rejected (idempotent guard), not a second spawn.
// ---------------------------------------------------------------------------

#[test]
fn double_start_is_rejected() {
    let (mgr, _dir) = stub_manager("double");
    mgr.start_if_ready(true).expect("first start");
    std::thread::sleep(std::time::Duration::from_millis(150));
    let r = mgr.start_if_ready(true);
    assert!(matches!(r, Err(ServeError::AlreadyRunning)), "got {r:?}");
    mgr.stop();
}

// ---------------------------------------------------------------------------
// (5) the local baseURL is built from the configured port (no key). The ai.rs
// harness routes to THIS when the model is Ready + the server is healthy.
// ---------------------------------------------------------------------------

#[test]
fn local_base_url_is_loopback_v1_on_the_port() {
    let (mgr, _dir) = stub_manager("baseurl");
    assert_eq!(mgr.base_url(), "http://127.0.0.1:18081/v1");
    // A loopback-only bind (never 0.0.0.0) — the sidecar is local, no remote.
    assert!(mgr.base_url().starts_with("http://127.0.0.1:"));
    assert!(mgr.base_url().ends_with("/v1"));
}

// ---------------------------------------------------------------------------
// (6) the spawn spec is the grounded llama-server CLI: `-m <model> --host
// 127.0.0.1 --port <p> --ctx-size <n>` — a structural proof the args are right.
// ---------------------------------------------------------------------------

#[test]
fn spawn_args_are_the_grounded_llama_server_cli() {
    let (mgr, _dir) = stub_manager("args");
    let args = mgr.spawn_args_for_test();
    // -m <model path>
    let m = args.iter().position(|a| a == "-m").expect("-m flag");
    assert!(args[m + 1].ends_with("model.gguf"));
    // --host 127.0.0.1 (loopback only)
    let h = args.iter().position(|a| a == "--host").expect("--host flag");
    assert_eq!(args[h + 1], "127.0.0.1");
    // --port <the configured port>
    let p = args.iter().position(|a| a == "--port").expect("--port flag");
    assert_eq!(args[p + 1], "18081");
    // --ctx-size <n>
    assert!(args.iter().any(|a| a == "--ctx-size"));
}

// ---------------------------------------------------------------------------
// (7) the health check carries a NON-ZERO startup grace (LLAMA_START_GRACE) so a
// cold llama.cpp load (mmap a multi-GB GGUF + build the Metal graph, longer than
// one 5s probe interval) is not killed mid-load and crash-looped to Failed — the
// bug seen in the wild (9 consecutive "health check failed" records). Pins the
// wiring so a regression to grace: ZERO is caught.
// ---------------------------------------------------------------------------

#[test]
fn health_check_carries_the_cold_start_grace() {
    let (mgr, _dir) = stub_manager("grace");
    let spec = mgr.build_spec();
    let hc = spec
        .health_check
        .expect("the llama-server sidecar must have a health check");
    assert_eq!(hc.grace, LLAMA_START_GRACE, "wired grace must be LLAMA_START_GRACE");
    assert!(
        hc.grace >= std::time::Duration::from_secs(60),
        "the cold-start grace ({:?}) must comfortably exceed a slow GGUF load",
        hc.grace
    );
}

// ---------------------------------------------------------------------------
// PROVIDER SELECTION — the honest inference-state harness (ai.rs). One pure fn
// over (local ready+healthy, gateway key configured) → the honest state the
// frontend renders. Every branch is unit-tested. LOCAL wins over gateway wins
// over demo; downloading + no-model are honest partial states.
// ---------------------------------------------------------------------------

use crate::ai::{select_inference_state, InferenceState, ProviderInputs};

#[test]
fn selection_prefers_local_when_ready_and_healthy() {
    let st = select_inference_state(ProviderInputs {
        model_ready: true,
        server_healthy: true,
        downloading: false,
        gateway_key_configured: true,
    });
    assert_eq!(st, InferenceState::Ready); // local
}

#[test]
fn selection_falls_back_to_gateway_when_local_unhealthy_but_key_present() {
    // Model ready but the server is NOT healthy → local-fallback to the gateway.
    let st = select_inference_state(ProviderInputs {
        model_ready: true,
        server_healthy: false,
        downloading: false,
        gateway_key_configured: true,
    });
    assert_eq!(st, InferenceState::LocalFallback);
}

#[test]
fn selection_is_gateway_only_when_no_model_but_key_present() {
    let st = select_inference_state(ProviderInputs {
        model_ready: false,
        server_healthy: false,
        downloading: false,
        gateway_key_configured: true,
    });
    assert_eq!(st, InferenceState::GatewayOnly);
}

#[test]
fn selection_reports_downloading_over_gateway_when_in_flight() {
    // While downloading (and no ready local server), the honest state is
    // Downloading — the UI shows progress and routes to the gateway meanwhile.
    let st = select_inference_state(ProviderInputs {
        model_ready: false,
        server_healthy: false,
        downloading: true,
        gateway_key_configured: true,
    });
    assert_eq!(st, InferenceState::Downloading);
}

#[test]
fn selection_is_demo_when_nothing_configured() {
    // No local model, not downloading, no gateway key → the built-in demo.
    let st = select_inference_state(ProviderInputs {
        model_ready: false,
        server_healthy: false,
        downloading: false,
        gateway_key_configured: false,
    });
    assert_eq!(st, InferenceState::Demo);
}

#[test]
fn selection_no_model_when_no_key_and_not_downloading_but_model_absent() {
    // A distinct honest "no-model" is folded into Demo when no gateway either;
    // but when the model is absent AND not downloading AND no key, the state is
    // Demo (the only usable path). The NoModel state is reserved for "model
    // absent, no gateway, and the user hasn't started a download" surfaced to the
    // onboarding step — asserted here via the serialization contract.
    assert_eq!(InferenceState::NoModel.as_str(), "no-model");
    assert_eq!(InferenceState::Ready.as_str(), "ready");
    assert_eq!(InferenceState::LocalFallback.as_str(), "local-fallback");
    assert_eq!(InferenceState::Downloading.as_str(), "downloading");
    assert_eq!(InferenceState::GatewayOnly.as_str(), "gateway-only");
    assert_eq!(InferenceState::Demo.as_str(), "demo");
}

// ---------------------------------------------------------------------------
// (CX-S1.5) runtime model switch. select_model gates on the target's readiness,
// repoints `-m`, and respawns on the new model.
// ---------------------------------------------------------------------------

#[test]
fn select_model_rejects_a_not_ready_target_without_swapping() {
    let (mgr, dir) = stub_manager("select-gate");
    let before = mgr.current_model_path();
    let r = mgr.select_model(dir.join("other.gguf"), false);
    assert!(matches!(r, Err(ServeError::ModelNotReady)), "got {r:?}");
    // Fails closed: the active model is untouched (no half-applied switch).
    assert_eq!(mgr.current_model_path(), before);
    assert_eq!(mgr.status().state, "stopped");
}

#[test]
fn select_model_repoints_the_m_flag_and_respawns() {
    let (mgr, dir) = stub_manager("select-swap");
    let mgr = mgr
        .with_spawn_args(long_lived_args())
        .with_health_interval(std::time::Duration::from_secs(3600));
    // A second, present model to switch onto.
    let new_model = dir.join("model2.gguf");
    std::fs::write(&new_model, b"GGUF-2").unwrap();

    mgr.select_model(new_model.clone(), true)
        .expect("select starts the sidecar on the new model");

    // The active model + the grounded `-m` argv both reflect the switch.
    assert_eq!(mgr.current_model_file(), "model2.gguf");
    let args = mgr.spawn_args_for_test();
    let m = args.iter().position(|a| a == "-m").expect("-m present");
    assert_eq!(args[m + 1], new_model.to_string_lossy());

    // And the sidecar actually reaches Running on the new model.
    let mut running = false;
    for _ in 0..100 {
        if mgr.status().state == "running" {
            running = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert!(running, "sidecar must reach Running after select_model");
    mgr.stop();
    assert_eq!(mgr.status().state, "stopped");
}

// ---------------------------------------------------------------------------
// PBA-L7b-001 — the local llama-server is AUTHENTICATED. llama.cpp reflects any
// `Origin` with credentials, so an unauthenticated server on a fixed loopback port
// is drivable by any web page the member visits (free GPU, slot exhaustion, and
// `/props` leaking the model path / OS username). Every session mints a fresh
// random API key, hands it to the child via the `LLAMA_API_KEY` env var (NOT argv:
// argv is world-readable through `ps`), and disables the endpoints the app never
// uses (web UI, `/slots`).
// ---------------------------------------------------------------------------

#[test]
fn pba_l7b_001_spawn_spec_carries_a_per_session_api_key_in_env_not_argv() {
    let (mgr, _dir) = stub_manager("apikey");
    let spec = mgr.build_spec();
    let key = spec
        .env
        .iter()
        .find(|(k, _)| k == LLAMA_API_KEY_ENV)
        .map(|(_, v)| v.clone())
        .expect("llama-server must be spawned with LLAMA_API_KEY (PBA-L7b-001)");
    assert_eq!(key.len(), 64, "a 256-bit hex key");
    assert!(key.chars().all(|c| c.is_ascii_hexdigit()));
    assert_eq!(key, mgr.api_key().as_str(), "the client sends exactly the key the server got");
    // Never on argv (world-readable via `ps`).
    assert!(
        !spec.args.iter().any(|a| a.contains(key.as_str())),
        "the API key must not appear on the argv"
    );
    // Per-session: two managers never share a key.
    let (other, _d2) = stub_manager("apikey2");
    assert_ne!(other.api_key().as_str(), key.as_str());
}

#[test]
fn pba_l7b_001_spawn_args_disable_the_web_ui_and_slots_endpoint() {
    let (mgr, _dir) = stub_manager("nowebui");
    let args = mgr.spawn_args_for_test();
    assert!(args.iter().any(|a| a == "--no-webui"), "web UI must be off: {args:?}");
    assert!(args.iter().any(|a| a == "--no-slots"), "/slots must be off: {args:?}");
}

// PBA-L7b-009 — a squatter already holding the fixed loopback port would receive
// the member's full chat context (and now the session API key). Refuse to start
// rather than hand traffic to whoever holds the port.
#[test]
fn pba_l7b_009_start_refuses_when_the_port_is_already_held() {
    let squat = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("bind a squatter");
    let port = squat.local_addr().unwrap().port();
    let dir = tmp_dir("squat");
    let model_path = dir.join("model.gguf");
    std::fs::write(&model_path, b"GGUF").unwrap();
    let mgr = LlamaServerManager::new(sleep_bin(), model_path, dir.join("crash.jsonl"), port);
    let r = mgr.start_if_ready(true);
    assert!(matches!(r, Err(ServeError::PortInUse(p)) if p == port), "got {r:?}");
    assert_eq!(mgr.status().state, "stopped");
    drop(squat);
}
