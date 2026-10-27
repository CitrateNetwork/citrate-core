// AMC-H1: the history command views.

use super::*;
use crate::history::tests::{tmp_dir, MemKeyring};
use crate::history::{HistoryError, HistoryStore};
use std::sync::Arc;

#[test]
fn status_while_off_says_off_and_shows_no_numbers() {
    let v = status_view(&HistoryState::off());
    assert!(!v.enabled);
    assert!(!v.available);
    assert_eq!(
        v.reason.as_deref(),
        Some(crate::history_recorder::OFF_MESSAGE)
    );
    assert_eq!(v.conversations, None, "unknown is not zero");
    assert_eq!(v.key_generation, None);
}

#[test]
fn status_of_a_broken_store_gives_the_reason() {
    let v = status_view(&HistoryState::with_store(Err(HistoryError::Unreadable(
        "x",
    ))));
    assert!(v.enabled);
    assert!(!v.available);
    assert!(v.reason.expect("reason").contains("cannot be read"));
    assert_eq!(v.conversations, None);
}

#[test]
fn status_of_a_ready_store_counts_its_conversations() {
    let d = tmp_dir("ipc-status");
    let kr = Arc::new(MemKeyring::default());
    let st = HistoryState::with_store(HistoryStore::open(&d, kr));
    let r = st.recorder().expect("on");
    r.start_chat("openai", "gpt-5.2", Some("x"), 1)
        .expect("start");
    let v = status_view(&st);
    assert!(v.available);
    assert_eq!(v.conversations, Some(1));
    assert_eq!(v.retention_days, None);
    assert_eq!(v.key_generation, Some(1));
    let json = serde_json::to_value(&v).expect("json");
    for k in [
        "enabled",
        "available",
        "reason",
        "conversations",
        "retentionDays",
        "keyGeneration",
    ] {
        assert!(json.get(k).is_some(), "{k}");
    }
    let _ = std::fs::remove_dir_all(d);
}
