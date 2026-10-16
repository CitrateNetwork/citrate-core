// HUP-S6.7 hardening: only the Contract reader's own window can queue requests for the main window.

use super::*;

fn req(op: &str) -> serde_json::Value {
    serde_json::json!({ "v": 1, "type": "contract.request", "id": "c1", "op": op, "args": {} })
}

#[test]
fn only_the_reader_can_queue_and_only_main_can_drain() {
    let inbox = ContractInbox::default();
    for other in [
        "popout-browser",
        "popout-media",
        "popout-monitor",
        "popout-diff",
        "main",
        "",
    ] {
        assert!(
            inbox.queue_request(other, req("write")).is_err(),
            "{other} must not queue"
        );
    }
    inbox
        .queue_request(READER_LABEL, req("initial"))
        .expect("reader queues");
    assert!(inbox.drain_for("popout-browser").is_err());
    assert!(inbox.drain_for(READER_LABEL).is_err());
    let got = inbox.drain_for(MAIN_LABEL).expect("main drains");
    assert_eq!(got, vec![req("initial")]);
    assert!(inbox.drain_for(MAIN_LABEL).expect("empty").is_empty());
}

#[test]
fn the_queue_and_each_request_are_bounded() {
    let inbox = ContractInbox::default();
    let big = serde_json::json!({ "pad": "x".repeat(MAX_REQUEST_BYTES) });
    assert!(inbox.queue_request(READER_LABEL, big).is_err());
    for _ in 0..MAX_QUEUED {
        inbox
            .queue_request(READER_LABEL, req("view"))
            .expect("room");
    }
    assert!(
        inbox.queue_request(READER_LABEL, req("view")).is_err(),
        "full"
    );
    assert_eq!(
        inbox.drain_for(MAIN_LABEL).expect("drain").len(),
        MAX_QUEUED
    );
    inbox
        .queue_request(READER_LABEL, req("view"))
        .expect("room again");
}

#[test]
fn the_event_name_matches_the_webview_bridge() {
    let bridge = include_str!("../../src/popout/bridge.ts");
    assert!(bridge.contains(&format!("POPOUT_EVENT = \"{POPOUT_EVENT}\"")));
    let channel = include_str!("../../src/popout/contractChannel.ts");
    assert!(channel.contains("type: \"contract.inbox\""));
    assert!(channel.contains("\"popout_contract_send\""));
    assert!(channel.contains("\"popout_contract_take\""));
}

#[test]
fn the_reader_capability_grants_exactly_the_one_relay_command() {
    let cap: serde_json::Value =
        serde_json::from_str(include_str!("../capabilities/popout-contract.json")).expect("json");
    assert_eq!(cap["windows"], serde_json::json!([READER_LABEL]));
    assert_eq!(
        cap["permissions"],
        serde_json::json!(["contract-reader-relay"])
    );
    assert!(cap.get("remote").is_none());
    let perm = include_str!("../permissions/contract-reader.toml");
    assert!(perm.contains("identifier = \"contract-reader-relay\""));
    assert!(perm.contains("commands.allow = [\"popout_contract_send\"]"));
}
