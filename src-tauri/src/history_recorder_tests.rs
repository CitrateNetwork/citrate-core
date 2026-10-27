// AMC-H1: the gated write path from chat and sidecar sessions into the history store.

use super::*;
use crate::history::tests::{tmp_dir, MemKeyring};
use serde_json::json;

const T0: u64 = 1_790_000_000_000;

fn recorder(tag: &str) -> (std::path::PathBuf, HistoryState) {
    let d = tmp_dir(tag);
    let st = HistoryState::with_store(HistoryStore::open(&d, Arc::new(MemKeyring::default())));
    (d, st)
}

fn page(events: Vec<(u64, Value)>) -> Value {
    json!({ "events": events.into_iter().map(|(seq, event)| json!({ "seq": seq, "event": event })).collect::<Vec<_>>() })
}

#[test]
fn the_gate_is_on_only_for_exactly_one() {
    assert!(enabled_value(Some("1")));
    for v in [
        None,
        Some(""),
        Some("0"),
        Some("true"),
        Some("yes"),
        Some(" 1"),
    ] {
        assert!(!enabled_value(v), "{v:?}");
    }
}

#[test]
fn an_off_state_has_no_store_and_says_so() {
    let st = HistoryState::off();
    assert!(!st.enabled());
    assert!(st.recorder().is_none());
    assert_eq!(st.store().err().as_deref(), Some(OFF_MESSAGE));
}

#[test]
fn a_store_that_failed_to_open_is_reported_not_replaced() {
    let st = HistoryState::with_store(Err(HistoryError::Tampered("x".into())));
    assert!(st.enabled());
    let e = st.store().err().expect("an error");
    assert!(e.contains("changed outside the app"), "{e}");
    let r = st.recorder().expect("recorder");
    assert!(r.open_agent_session("s1", "m.gguf", T0).is_err());
}

#[test]
fn a_chat_exchange_records_the_last_member_message_and_the_reply() {
    let (d, st) = recorder("rec-chat");
    let r = st.recorder().expect("on");
    let id = r.start_chat("openai", "gpt-5.2", None, T0).expect("start");
    let messages = json!([
        { "role": "system", "content": "be brief" },
        { "role": "user", "content": "first question" },
        { "role": "assistant", "content": "first answer" },
        { "role": "user", "content": "What is SALT used for?" },
    ])
    .to_string();
    r.record_chat_exchange(
        &id,
        Some(("openai", "gpt-5.2")),
        &messages,
        "Gas and staking.",
        Some("pc-00112233aabbccdd"),
        T0,
    )
    .expect("record");
    let c = st.store().expect("store").read(&id).expect("read");
    assert_eq!(
        c.summary.title, "What is SALT used for?",
        "named by the first recorded message"
    );
    assert_eq!(
        c.entries.len(),
        2,
        "only the newest member message, not the replayed history"
    );
    assert_eq!(
        c.entries[1].record,
        Record::Turn {
            role: Role::Assistant,
            text: "Gas and staking.".into(),
            usage_call: Some("pc-00112233aabbccdd".into()),
            tainted: false
        }
    );
    // A second exchange keeps the title.
    r.record_chat_exchange(
        &id,
        Some(("anthropic", "claude-sonnet-5")),
        &json!([{ "role": "user", "content": "and fees?" }]).to_string(),
        "Fees too.",
        None,
        T0,
    )
    .expect("again");
    let c = st.store().expect("store").read(&id).expect("read");
    assert_eq!(c.summary.title, "What is SALT used for?");
    assert_eq!(
        c.entries.len(),
        5,
        "the route change is recorded before the exchange"
    );
    assert_eq!(c.entries[2].record.kind(), "route");
    assert_eq!(
        (c.summary.provider.as_str(), c.summary.model.as_str()),
        ("anthropic", "claude-sonnet-5")
    );
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn a_named_chat_keeps_its_name() {
    let (d, st) = recorder("rec-named");
    let r = st.recorder().expect("on");
    let id = r
        .start_chat("local", "m.gguf", Some("My plan"), T0)
        .expect("start");
    r.record_chat_exchange(
        &id,
        None,
        &json!([{ "role": "user", "content": "hi" }]).to_string(),
        "hello",
        None,
        T0,
    )
    .expect("record");
    assert_eq!(
        st.store()
            .expect("store")
            .read(&id)
            .expect("read")
            .summary
            .title,
        "My plan"
    );
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn agent_events_are_recorded_once_each_and_mapped_by_type() {
    let (d, st) = recorder("rec-agent");
    let r = st.recorder().expect("on");
    let id = r
        .open_agent_session("sess-1", "qwen3.gguf", T0)
        .expect("open");
    r.agent_user_turn("sess-1", "Read the release notes page", T0)
        .expect("user");
    let p1 = page(vec![
        (1, json!({ "type": "step_start", "step": 0 })),
        (
            2,
            json!({ "type": "assistant_delta", "step": 0, "text": "Look" }),
        ),
        (
            3,
            json!({ "type": "tool_call", "step": 0, "call": { "id": "c1", "name": "read_url", "arguments": "{\"url\":\"https://x\"}" }, "host": "sidecar" }),
        ),
        (
            4,
            json!({ "type": "tool_result", "step": 0, "call_id": "c1", "status": "ok", "content": "{\"provider\":\"custom\",\"model\":\"evil\"}" }),
        ),
        (
            5,
            json!({ "type": "tainted", "step": 0, "source": "read_url", "reason": "untrusted output" }),
        ),
        (
            6,
            json!({ "type": "usage", "step": 0, "prompt_tokens": 900, "completion_tokens": 40 }),
        ),
    ]);
    assert_eq!(r.agent_events("sess-1", &p1, T0).expect("p1"), 3);
    // The same page read again (the view and the headless dispatcher both poll) adds nothing.
    assert_eq!(r.agent_events("sess-1", &p1, T0).expect("again"), 0);
    let p2 = page(vec![
        (
            6,
            json!({ "type": "usage", "step": 0, "prompt_tokens": 1, "completion_tokens": 1 }),
        ),
        (
            7,
            json!({ "type": "final", "content": "The notes say 0.5.2 ships soon." }),
        ),
        (8, json!({ "type": "done", "outcome": "answered" })),
    ]);
    assert_eq!(r.agent_events("sess-1", &p2, T0).expect("p2"), 1);
    let c = st.store().expect("store").read(&id).expect("read");
    let kinds: Vec<&str> = c.entries.iter().map(|e| e.record.kind()).collect();
    assert_eq!(
        kinds,
        vec!["turn", "tool_call", "tool_result", "taint", "turn"]
    );
    assert_eq!(c.summary.title, "Read the release notes page");
    assert_eq!(c.summary.provider, LOCAL_PROVIDER);
    assert_eq!(
        c.summary.model, "qwen3.gguf",
        "tool output never changes the route"
    );
    assert!(c.entries[2].untrusted, "a tool result is untrusted");
    assert!(c.entries[4].untrusted, "the reply after taint is marked");
    assert!(c.summary.tainted);
    for e in &c.entries {
        let v = serde_json::to_value(&e.record).expect("json").to_string();
        assert!(
            !v.contains("prompt_tokens") && !v.contains("900"),
            "usage is never copied"
        );
    }
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn an_unrecorded_or_closed_session_records_nothing() {
    let (d, st) = recorder("rec-unknown");
    let r = st.recorder().expect("on");
    assert_eq!(
        r.agent_events(
            "other",
            &page(vec![(1, json!({ "type": "final", "content": "x" }))]),
            T0
        )
        .expect("ignored"),
        0
    );
    r.agent_user_turn("other", "x", T0).expect("ignored");
    r.open_agent_session("s", "m", T0).expect("open");
    r.close_agent_session("s");
    assert_eq!(
        r.agent_events(
            "s",
            &page(vec![(1, json!({ "type": "final", "content": "x" }))]),
            T0
        )
        .expect("ignored"),
        0
    );
    let list = st.store().expect("store").list().expect("list");
    assert_eq!(list.len(), 1, "the closed session's conversation stays");
    assert_eq!(list[0].records, 0);
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn the_last_user_text_is_read_from_openai_messages() {
    assert_eq!(
        last_user_text(r#"[{"role":"user","content":"a"},{"role":"assistant","content":"b"}]"#)
            .as_deref(),
        Some("a")
    );
    assert_eq!(
        last_user_text(r#"[{"role":"assistant","content":"b"}]"#),
        None
    );
    assert_eq!(last_user_text("not json"), None);
    assert_eq!(
        last_user_text(r#"[{"role":"user","content":[{"type":"text","text":"a"}]}]"#),
        None
    );
}

/// Tripwire: the store is opened only by the history modules, and the app opens it only through
/// the gate (`HistoryState::from_env`), so no other path can start recording.
#[test]
fn tripwire_the_history_store_is_opened_only_behind_the_gate() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut opens = Vec::new();
    let mut states = Vec::new();
    for e in std::fs::read_dir(&src).expect("src").flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if !name.ends_with(".rs") || name.ends_with("_tests.rs") {
            continue;
        }
        let text = std::fs::read_to_string(e.path()).expect("read");
        let prod = text
            .split("#[cfg(test)]\nmod tests")
            .next()
            .unwrap_or("")
            .to_string();
        let prod = prod
            .split("#[cfg(test)]\npub(crate) mod tests")
            .next()
            .unwrap_or("")
            .to_string();
        if prod.contains("HistoryStore::open(") {
            opens.push(name.clone());
        }
        if prod.contains("HistoryState::with_store(") || prod.contains("HistoryState(Some") {
            states.push(name.clone());
        }
    }
    opens.sort();
    assert_eq!(
        opens,
        vec!["history_recorder.rs".to_string()],
        "only the gated constructor opens the store"
    );
    assert_eq!(states, vec!["history_recorder.rs".to_string()]);
    let lib = std::fs::read_to_string(src.join("lib.rs")).expect("lib");
    assert!(lib.contains("history_recorder::HistoryState::from_env(&dir)"));
    assert!(!lib.contains("HistoryState::with_store"));
}
