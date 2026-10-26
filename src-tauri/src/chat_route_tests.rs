// K2 (0.5.2): the member's chat route ("Local model only" in Hermes › Controls).

use super::*;
use crate::ai::{select_inference_state, InferenceState, ProviderInputs};
use std::path::PathBuf;

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("citrate-chat-route-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("tmp");
    d
}

fn inputs(
    model_ready: bool,
    server_healthy: bool,
    downloading: bool,
    gateway: bool,
) -> ProviderInputs {
    ProviderInputs {
        model_ready,
        server_healthy,
        downloading,
        gateway_key_configured: gateway,
    }
}

#[test]
fn the_default_is_ask_first() {
    let d = tmp("default");
    assert_eq!(ChatRoute::default(), ChatRoute::AskFirst);
    assert_eq!(load(&d).expect("load").route, ChatRoute::AskFirst);
    assert_eq!(effective(&d), ChatRoute::AskFirst);
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn local_only_persists_and_reads_back() {
    let d = tmp("persist");
    let st = set(&d, ChatRoute::LocalOnly, 42).expect("set");
    assert_eq!(st.route, ChatRoute::LocalOnly);
    assert_eq!(effective(&d), ChatRoute::LocalOnly);
    let back = load(&d).expect("load");
    assert_eq!(back.route, ChatRoute::LocalOnly);
    assert_eq!(back.changed_at_ms, Some(42));
    let text = std::fs::read_to_string(d.join(ROUTE_FILE)).expect("file");
    assert!(text.contains("\"local_only\""), "{text}");
    set(&d, ChatRoute::AskFirst, 43).expect("back");
    assert_eq!(effective(&d), ChatRoute::AskFirst);
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn an_unreadable_setting_counts_as_local_only() {
    // A read error never widens what may leave the device.
    let d = tmp("corrupt");
    std::fs::write(d.join(ROUTE_FILE), "{not json").expect("write");
    assert!(load(&d).is_err());
    assert_eq!(effective(&d), ChatRoute::LocalOnly);
    let st = status(&d);
    assert_eq!(st.route, ChatRoute::LocalOnly);
    assert!(st.load_error.is_some());
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn the_wire_names_are_snake_case() {
    assert_eq!(
        serde_json::to_string(&ChatRoute::LocalOnly).expect("json"),
        "\"local_only\""
    );
    assert_eq!(
        serde_json::to_string(&ChatRoute::AskFirst).expect("json"),
        "\"ask_first\""
    );
    assert!(
        serde_json::from_str::<ChatRoute>("\"gateway\"").is_err(),
        "no third value"
    );
}

#[test]
fn local_only_never_reports_a_gateway_state() {
    for ready in [false, true] {
        for healthy in [false, true] {
            for downloading in [false, true] {
                for gw in [false, true] {
                    let s = select_route(
                        inputs(ready, healthy, downloading, gw),
                        ChatRoute::LocalOnly,
                    );
                    assert!(
                        !matches!(
                            s,
                            InferenceState::GatewayOnly
                                | InferenceState::LocalStopped
                                | InferenceState::AskGateway
                        ),
                        "{ready} {healthy} {downloading} {gw} -> {s:?}"
                    );
                    if ready && healthy {
                        assert_eq!(s, InferenceState::Ready);
                    }
                }
            }
        }
    }
    // The local model is set up but stopped: offer only to start it.
    assert_eq!(
        select_route(inputs(true, false, false, true), ChatRoute::LocalOnly),
        InferenceState::LocalOnlyStopped
    );
    assert_eq!(
        select_route(inputs(true, false, false, false), ChatRoute::LocalOnly),
        InferenceState::LocalOnlyStopped
    );
    // No local model, a gateway key saved: the clear "local only" state.
    assert_eq!(
        select_route(inputs(false, false, false, true), ChatRoute::LocalOnly),
        InferenceState::LocalOnly
    );
    assert_eq!(
        select_route(inputs(false, false, true, true), ChatRoute::LocalOnly),
        InferenceState::LocalOnly
    );
    assert_eq!(InferenceState::LocalOnly.as_str(), "local-only");
    assert_eq!(
        InferenceState::LocalOnlyStopped.as_str(),
        "local-only-stopped"
    );
}

#[test]
fn ask_first_keeps_todays_routing_and_closes_the_silent_gateway_case() {
    // Unchanged: local when ready, ask when the local model is stopped, demo with nothing.
    assert_eq!(
        select_route(inputs(true, true, false, true), ChatRoute::AskFirst),
        InferenceState::Ready
    );
    assert_eq!(
        select_route(inputs(true, false, false, true), ChatRoute::AskFirst),
        InferenceState::LocalStopped
    );
    assert_eq!(
        select_route(inputs(false, false, false, false), ChatRoute::AskFirst),
        InferenceState::Demo
    );
    assert_eq!(
        select_route(inputs(false, false, true, false), ChatRoute::AskFirst),
        InferenceState::Downloading
    );
    // The silent case: a gateway key and no local model was "gateway-only" (a route). Now it asks.
    assert_eq!(
        select_inference_state(inputs(false, false, false, true)),
        InferenceState::GatewayOnly
    );
    assert_eq!(
        select_route(inputs(false, false, false, true), ChatRoute::AskFirst),
        InferenceState::AskGateway
    );
    // A download in flight with a gateway key used the gateway meanwhile. Now it asks too.
    assert_eq!(
        select_route(inputs(false, false, true, true), ChatRoute::AskFirst),
        InferenceState::AskGateway
    );
    assert_eq!(InferenceState::AskGateway.as_str(), "ask-gateway");
}

#[test]
fn remote_chat_is_refused_under_local_only() {
    let d = tmp("refuse");
    assert!(
        remote_chat_allowed(&d).is_ok(),
        "ask first: the member's explicit choice may send"
    );
    set(&d, ChatRoute::LocalOnly, 1).expect("set");
    let e = remote_chat_allowed(&d).expect_err("refused");
    assert!(e.contains("Local only"), "{e}");
    std::fs::write(d.join(ROUTE_FILE), "garbage").expect("write");
    assert!(
        remote_chat_allowed(&d).is_err(),
        "unreadable counts as local only"
    );
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn the_commands_are_registered_and_in_the_main_window_acl() {
    let acl = include_str!("../permissions/main-window.toml");
    let lib = include_str!("lib.rs");
    for cmd in ["chat_route_get", "chat_route_set"] {
        assert!(
            acl.contains(&format!("\"{cmd}\"")),
            "{cmd} in main-window.toml"
        );
        assert!(
            lib.contains(&format!("chat_route::{cmd}")),
            "{cmd} registered in lib.rs"
        );
    }
}

#[test]
fn the_remote_chat_commands_check_the_route() {
    let ai = include_str!("ai.rs");
    for f in ["pub async fn ai_chat(", "pub async fn ai_chat_tools("] {
        let at = ai.find(f).expect(f);
        let body = &ai[at..at + 900.min(ai.len() - at)];
        assert!(
            body.contains("chat_route::remote_chat_allowed"),
            "{f} checks the route"
        );
    }
    let serve = include_str!("serve.rs");
    assert!(
        serve.contains("chat_route::select_route"),
        "model_inference_state applies the route"
    );
}

// ---------------------------------------------------------------------------
// STD (Codex P2 on #372 at 33410a0): an existing file must name its route, and local_only never
// falls back to the demo agent or a download-meanwhile route.
// ---------------------------------------------------------------------------

#[test]
fn an_existing_file_without_a_route_fails_closed() {
    // Adversarial / damaged files that parse as JSON but carry no route. Only an ABSENT file
    // gets the ask_first default; anything present must say which route it is.
    for (i, text) in [
        "{}",
        r#"{"changedAtMs":42}"#,
        r#"{"route":null}"#,
        r#"{"route":"","changedAtMs":1}"#,
        r#"{"route":"gateway"}"#,
        "[]",
        "null",
        "",
    ]
    .iter()
    .enumerate()
    {
        let d = tmp(&format!("missing-route-{i}"));
        std::fs::write(d.join(ROUTE_FILE), text).expect("write");
        assert!(load(&d).is_err(), "{text:?} must not load");
        assert_eq!(effective(&d), ChatRoute::LocalOnly, "{text:?} fails closed");
        let st = status(&d);
        assert_eq!(st.route, ChatRoute::LocalOnly, "{text:?}");
        assert!(st.load_error.is_some(), "{text:?} reports why");
        assert!(
            remote_chat_allowed(&d).is_err(),
            "{text:?} refuses remote chat"
        );
        let _ = std::fs::remove_dir_all(d);
    }
    // A complete file still loads (the timestamp alone is optional).
    let d = tmp("route-only");
    std::fs::write(d.join(ROUTE_FILE), r#"{"route":"ask_first"}"#).expect("write");
    assert_eq!(load(&d).expect("load").route, ChatRoute::AskFirst);
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn local_only_with_no_local_model_and_no_key_is_local_only_not_demo() {
    assert_eq!(
        select_route(inputs(false, false, false, false), ChatRoute::LocalOnly),
        InferenceState::LocalOnly
    );
    assert_eq!(
        select_route(inputs(false, false, true, false), ChatRoute::LocalOnly),
        InferenceState::LocalOnly,
        "a download in flight is not a usable local model"
    );
}

/// Tripwire: every input combination under local_only yields only a local state. A new
/// InferenceState that is a remote or demo route must be added to this list on purpose.
#[test]
fn tripwire_local_only_never_yields_a_demo_or_remote_state() {
    for ready in [false, true] {
        for healthy in [false, true] {
            for downloading in [false, true] {
                for gw in [false, true] {
                    let s = select_route(
                        inputs(ready, healthy, downloading, gw),
                        ChatRoute::LocalOnly,
                    );
                    assert!(
                        matches!(
                            s,
                            InferenceState::Ready | InferenceState::LocalOnlyStopped | InferenceState::LocalOnly
                        ),
                        "local_only with ready={ready} healthy={healthy} downloading={downloading} gateway={gw} gave {s:?}"
                    );
                }
            }
        }
    }
}

/// #420: overlapping `chat_route_set` calls each write their own temp file, so none fails with a
/// spurious error and the stored route is exactly one of the values written, never a mix.
#[test]
fn concurrent_sets_never_fail_and_leave_one_written_value() {
    const THREADS: u64 = 8;
    const SETS: u64 = 30;
    let d = tmp("concurrent");
    let route_of = |t: u64| {
        if t.is_multiple_of(2) {
            ChatRoute::LocalOnly
        } else {
            ChatRoute::AskFirst
        }
    };
    let errors: Vec<String> = std::thread::scope(|s| {
        let handles: Vec<_> = (0..THREADS)
            .map(|t| {
                let d = d.clone();
                s.spawn(move || {
                    (0..SETS)
                        .filter_map(|i| set(&d, route_of(t), t * 1000 + i).err())
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().expect("set thread"))
            .collect()
    });
    assert!(
        errors.is_empty(),
        "no set fails; {} of {} failed, first: {:?}",
        errors.len(),
        THREADS * SETS,
        errors.first()
    );
    let back = load(&d).expect("the stored route parses");
    let at = back.changed_at_ms.expect("a written value carries its time");
    assert!(
        at / 1000 < THREADS && at % 1000 < SETS && back.route == route_of(at / 1000),
        "the stored route is one of the values written: {back:?}"
    );
    let leftovers: Vec<String> = std::fs::read_dir(&d)
        .expect("dir")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n != ROUTE_FILE)
        .collect();
    assert!(leftovers.is_empty(), "no temp file is left: {leftovers:?}");
    let _ = std::fs::remove_dir_all(d);
}
