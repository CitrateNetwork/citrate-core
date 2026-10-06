// HUP-S1.2 / US-1.4 — the embedding llama-server core starts for Hermes's tool and skill ranking.

use super::*;

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "citrate-embed-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn server(dir: &Path, bin: PathBuf, model: PathBuf, port: u16) -> EmbedServer {
    EmbedServer::new(
        bin,
        model,
        port,
        dir.join("embed.key"),
        dir.join("embed-crashes.log"),
    )
}

#[test]
fn the_argv_is_loopback_embedding_only_with_cls_pooling_and_no_key() {
    let d = tmp("argv");
    let s = server(&d, d.join("llama-server"), d.join("bge.gguf"), 18185);
    let a = s.spawn_args();
    let pair = |flag: &str| {
        a.iter()
            .position(|x| x == flag)
            .and_then(|i| a.get(i + 1))
            .cloned()
    };
    assert_eq!(pair("--host").as_deref(), Some("127.0.0.1"));
    assert_eq!(pair("--port").as_deref(), Some("18185"));
    assert_eq!(pair("--pooling").as_deref(), Some("cls"), "BGE embeds its [CLS] token");
    assert_eq!(pair("-m"), Some(d.join("bge.gguf").to_string_lossy().to_string()));
    assert_eq!(pair("--ubatch-size").as_deref(), Some("512"), "one input, one micro-batch");
    assert!(a.iter().any(|x| x == "--embeddings"));
    assert!(a.iter().any(|x| x == "--no-webui"));
    assert!(
        !a.iter().any(|x| x.contains("api-key") || x == s.api_key.as_str()),
        "the key never goes on argv: {a:?}"
    );
}

#[test]
fn the_sidecar_gets_the_url_and_the_key_file_path_never_the_key() {
    let d = tmp("env");
    let s = server(&d, d.join("llama-server"), d.join("bge.gguf"), EMBED_PORT);
    let env = s.sidecar_env();
    assert_eq!(
        env,
        vec![
            (
                HERMES_EMBED_URL_ENV.to_string(),
                "http://127.0.0.1:18085".to_string()
            ),
            (
                HERMES_EMBED_KEY_FILE_ENV.to_string(),
                d.join("embed.key").to_string_lossy().to_string()
            ),
        ]
    );
    assert!(env.iter().all(|(_, v)| !v.contains(s.api_key.as_str())));
}

#[test]
fn a_missing_binary_or_model_or_a_different_model_never_starts() {
    let d = tmp("refuse");
    let s = server(&d, d.join("absent"), d.join("bge.gguf"), 18186);
    assert_eq!(s.ensure_started(), Err(EmbedError::BinaryNotFound));
    let sh = PathBuf::from("/bin/sleep");
    let s = server(&d, sh.clone(), d.join("absent.gguf"), 18186);
    assert_eq!(s.ensure_started(), Err(EmbedError::ModelNotFound));
    let model = d.join("other.gguf");
    std::fs::write(&model, b"GGUF not the pinned model").unwrap();
    let s = server(&d, sh, model, 18186);
    assert_eq!(s.ensure_started(), Err(EmbedError::ModelMismatch));
    assert!(!s.is_started());
    assert!(!d.join("embed.key").exists(), "no key file without a server");
}

#[test]
fn a_taken_port_is_refused() {
    let d = tmp("port");
    let held = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = held.local_addr().unwrap().port();
    let model = d.join("bge.gguf");
    std::fs::write(&model, b"x").unwrap();
    let s = server(&d, PathBuf::from("/bin/sleep"), model, port).assume_verified_for_test();
    assert_eq!(s.ensure_started(), Err(EmbedError::PortInUse(port)));
}

#[test]
fn the_pinned_digest_matches_the_runtime_deps_pin() {
    let pins = include_str!("../runtime-deps.sha256");
    assert!(
        pins.lines()
            .any(|l| l == format!("{EMBED_MODEL_SHA256}  {EMBED_MODEL_FILE}")),
        "runtime-deps.sha256 pins {EMBED_MODEL_FILE} at {EMBED_MODEL_SHA256}"
    );
}

#[test]
fn every_bundle_that_ships_the_bge_model_ships_the_gguf_too() {
    for (name, conf) in [
        ("tauri.bundle-node.conf.json", include_str!("../tauri.bundle-node.conf.json")),
        // macOS release bundle (owner decision 2026-10-06: ship the GGUF on Mac, budget raised).
        ("tauri.bundle-lite.conf.json", include_str!("../tauri.bundle-lite.conf.json")),
        ("tauri.local-run.conf.json", include_str!("../tauri.local-run.conf.json")),
        // Hermes runs on Linux and Windows too; without the GGUF its embedding server never starts.
        ("tauri.bundle-linux.conf.json", include_str!("../tauri.bundle-linux.conf.json")),
        ("tauri.bundle-windows.conf.json", include_str!("../tauri.bundle-windows.conf.json")),
    ] {
        assert!(
            conf.contains(&format!("\"{EMBED_MODEL_DIR}/*\"")),
            "{name} bundles the embedding GGUF"
        );
    }
}

#[cfg(unix)]
#[test]
fn the_manager_passes_the_embed_env_only_while_the_server_runs() {
    use std::os::unix::fs::PermissionsExt;
    let d = tmp("mgr");
    let model = d.join("bge.gguf");
    std::fs::write(&model, b"x").unwrap();
    let free = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = free.local_addr().unwrap().port();
    drop(free);
    // `sleep` rejects llama-server's argv and exits; the supervisor keeps restarting it, which is
    // enough to show the wiring (the live test below runs the real server).
    let embed = server(&d, PathBuf::from("/bin/sleep"), model, port).assume_verified_for_test();
    let mgr = crate::hermes::HermesManager::new(d.join("hermes"), d.join("t"), d.join("c"))
        .with_embed(embed);
    let keys = |env: &[(String, String)]| {
        env.iter()
            .filter(|(k, _)| k.starts_with("CITRATE_HERMES_EMBED"))
            .count()
    };
    assert_eq!(keys(&mgr.spec_env_for_test()), 0, "not started: nothing passed");
    let e = mgr.embed_for_test().unwrap();
    e.ensure_started().unwrap();
    let env = mgr.spec_env_for_test();
    assert_eq!(keys(&env), 2, "{env:?}");
    let kf = d.join("embed.key");
    let mode = std::fs::metadata(&kf).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "the key file is owner-only");
    assert_eq!(std::fs::read_to_string(&kf).unwrap(), e.api_key.as_str());
    mgr.stop();
    assert!(!kf.exists(), "the key file goes with the server");
    assert_eq!(keys(&mgr.spec_env_for_test()), 0);
}

/// Live: the bundled llama-server on the pinned BGE GGUF, started exactly as the app starts it,
/// answers a keyed `/v1/embeddings` with 768-dimension vectors and refuses an unkeyed one.
/// Run with `CITRATE_E2E_LLAMA_BIN=<llama-server> CITRATE_EMBED_GGUF=<bge-base-en-v1.5-f16.gguf>`.
#[test]
fn live_the_real_embedding_server_answers_with_the_key_only() {
    let (Ok(bin), Some(model)) = (
        std::env::var("CITRATE_E2E_LLAMA_BIN"),
        std::env::var_os(EMBED_MODEL_ENV),
    ) else {
        eprintln!("skipped: set CITRATE_E2E_LLAMA_BIN and {EMBED_MODEL_ENV}");
        return;
    };
    let d = tmp("live");
    let free = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = free.local_addr().unwrap().port();
    drop(free);
    let s = server(&d, PathBuf::from(bin), PathBuf::from(model), port);
    s.ensure_started().unwrap();
    let url = format!("{}/v1/embeddings", s.url());
    let body = r#"{"input":["node status","list my groups"],"model":"embedding","encoding_format":"float"}"#;
    let started = std::time::Instant::now();
    let answer = loop {
        let r = ureq::post(&url)
            .header("authorization", &format!("Bearer {}", s.api_key.as_str()))
            .header("content-type", "application/json")
            .send(body);
        if let Ok(mut resp) = r {
            break resp.body_mut().read_to_string().unwrap();
        }
        assert!(started.elapsed() < Duration::from_secs(60), "never answered");
        std::thread::sleep(Duration::from_millis(250));
    };
    let v: serde_json::Value = serde_json::from_str(&answer).unwrap();
    let data = v["data"].as_array().unwrap();
    assert_eq!(data.len(), 2);
    assert_eq!(data[0]["embedding"].as_array().unwrap().len(), 768);
    let unkeyed = ureq::post(&url)
        .header("content-type", "application/json")
        .send(body);
    assert!(unkeyed.is_err(), "an unkeyed request is refused");
    s.stop();
}

/// Live, end to end: core's Hermes manager starts the embedding server with the real sidecar, and a
/// session the sidecar opens ranks its tools with embeddings (`GET /sessions/:id/retrieval`).
/// Run with `CITRATE_E2E_HERMES_BIN=<citrate-agent-sidecar> CITRATE_E2E_LLAMA_BIN=<llama-server>
/// CITRATE_EMBED_GGUF=<bge-base-en-v1.5-f16.gguf>`.
#[test]
fn live_sessions_rank_with_the_embedding_server_core_starts() {
    let (Ok(hermes), Ok(llama), Some(model)) = (
        std::env::var("CITRATE_E2E_HERMES_BIN"),
        std::env::var("CITRATE_E2E_LLAMA_BIN"),
        std::env::var_os(EMBED_MODEL_ENV),
    ) else {
        eprintln!("skipped: set CITRATE_E2E_HERMES_BIN, CITRATE_E2E_LLAMA_BIN and {EMBED_MODEL_ENV}");
        return;
    };
    let d = tmp("live-e2e");
    let port = |()| {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let (eport, hport) = (port(()), port(()));
    let embed = server(&d, PathBuf::from(llama), PathBuf::from(model), eport);
    let mgr = crate::hermes::HermesManager::new(
        PathBuf::from(hermes),
        d.join("bearer.token"),
        d.join("crashes.log"),
    )
    .with_control_addr(&format!("127.0.0.1:{hport}"))
    .with_health_interval(Duration::from_secs(60))
    .with_embed(embed);
    mgr.start().unwrap();
    let health = format!("http://127.0.0.1:{eport}/health");
    let up = std::time::Instant::now();
    while !(crate::serve::http_health_ok(&health)
        && mgr.control_get_path("/sessions").map(|r| r.status).ok() == Some(200))
    {
        assert!(up.elapsed() < Duration::from_secs(60), "the servers did not come up");
        std::thread::sleep(Duration::from_millis(200));
    }
    let tools = r#"[{"name":"node_status","description":"Read this node's status","parameters":{"type":"object"},"annotations":{"effect":"none","trust":"trusted"}},{"name":"groups_list","description":"List the member's groups","parameters":{"type":"object"},"annotations":{"effect":"none","trust":"trusted"}}]"#;
    let body = crate::hermes::build_session_body(
        "You are Hermes.",
        tools,
        "http://127.0.0.1:9/v1",
        "unused",
        "m.gguf",
        8192,
    )
    .unwrap();
    let id = mgr.session_open(&body).unwrap();
    let r = mgr
        .control_get_path(&format!("/sessions/{id}/retrieval"))
        .unwrap();
    mgr.stop();
    let v: serde_json::Value = serde_json::from_str(&r.body).unwrap();
    assert_eq!(v["retrieval"], serde_json::json!({"mode": "embedding"}), "{v}");
}
