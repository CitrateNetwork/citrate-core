// HUP-S1.9 (live parity): the live parity run (src/agent/parity/live) opens real sidecar sessions
// with a body built from src/agent/parity/live/session-config.json. These tests pin that file to
// what `build_session_body` actually sends, so the live run cannot drift from the shipped config:
// a change to the body's keys or constants here fails until the JSON (and so the live run) agrees.

use super::*;

const SESSION_CONFIG: &str = include_str!("../../src/agent/parity/live/session-config.json");

const TOOLS: &str = r#"[{"type":"function","function":{"name":"node_status","description":"Read node vitals","parameters":{"type":"object","properties":{}}},"annotations":{"effect":"none","trust":"trusted"}},
 {"type":"function","function":{"name":"group_invite","description":"Mint an invite","parameters":{"type":"object"}},"annotations":{"effect":"write","trust":"untrusted"}}]"#;

fn config() -> serde_json::Value {
    serde_json::from_str(SESSION_CONFIG).expect("session-config.json is JSON")
}

fn sorted_keys(v: &serde_json::Value) -> Vec<String> {
    let mut k: Vec<String> = v
        .as_object()
        .map(|o| o.keys().cloned().collect())
        .unwrap_or_default();
    k.sort();
    k
}

fn strings(v: &serde_json::Value) -> Vec<String> {
    let mut k: Vec<String> = v
        .as_array()
        .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
        .unwrap_or_default();
    k.sort();
    k
}

#[test]
fn the_live_parity_session_config_matches_build_session_body() {
    let cfg = config();
    let body: serde_json::Value = serde_json::from_str(
        &build_session_body("p", TOOLS, "http://127.0.0.1:9/v1", "b", "m.gguf", 16_384).unwrap(),
    )
    .unwrap();
    assert_eq!(
        sorted_keys(&body),
        strings(&cfg["bodyKeys"]),
        "build_session_body's keys changed; update src/agent/parity/live/session-config.json and re-run the live parity suite"
    );
    assert_eq!(body["maxToolsPerRequest"], cfg["maxToolsPerRequest"]);
    assert_eq!(body["hicAware"], cfg["hicAware"]);
    assert_eq!(
        cfg["aiMaxTokens"].as_u64(),
        Some(u64::from(crate::ai::AI_MAX_TOKENS))
    );
    let divisor = cfg["maxTokensContextDivisor"].as_u64().unwrap();
    assert_eq!(
        body["maxTokens"].as_u64(),
        Some(u64::from(crate::ai::AI_MAX_TOKENS).min(16_384 / divisor))
    );
    for t in body["tools"].as_array().unwrap() {
        assert_eq!(sorted_keys(t), strings(&cfg["toolKeys"]));
        assert_eq!(t["host"], "core");
    }
}

#[test]
fn the_step_caps_in_the_live_config_are_sent_exactly_when_named() {
    // default_turn_cap is an owner decision: today core sends no maxSteps and the sidecar default
    // applies. If core starts sending a cap, the config must name it (and the live run uses it).
    let cfg = config();
    let body: serde_json::Value = serde_json::from_str(
        &build_session_body("p", TOOLS, "http://127.0.0.1:9/v1", "b", "m.gguf", 4_096).unwrap(),
    )
    .unwrap();
    for key in ["maxSteps", "maxToolCallsPerStep"] {
        assert_eq!(
            body.get(key),
            cfg.get(key),
            "{key}: build_session_body and session-config.json disagree"
        );
    }
    assert!(cfg["sidecarDefaults"]["maxSteps"].as_u64().is_some());
    assert!(cfg["sidecarDefaults"]["maxToolCallsPerStep"].as_u64().is_some());
}
