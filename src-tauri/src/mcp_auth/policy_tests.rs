// L6-WP06 local tool policy tests (owner brief 2026-11-28: writes blocked by default, unknown
// tool treated as write, annotations plus the matrix allowlist).

use super::policy::*;
use crate::mcp_servers::ProbeHints;

fn hints(read_only: Option<bool>, destructive: Option<bool>) -> ProbeHints {
    ProbeHints {
        read_only_hint: read_only,
        destructive_hint: destructive,
        ..ProbeHints::default()
    }
}

#[test]
fn writes_are_blocked_by_default_and_allowed_only_when_the_member_enables_them() {
    let none = ProbeHints::default();
    assert_eq!(
        decide(Some("gmail"), "send_message", &none, false),
        Decision::Blocked(WRITES_OFF)
    );
    assert_eq!(
        decide(Some("gmail"), "send_message", &none, true),
        Decision::Allow
    );
    assert_eq!(
        decide(Some("stripe"), "send_money", &none, false),
        Decision::Blocked(WRITES_OFF)
    );
    assert_eq!(
        decide(Some("gmail"), "search_threads", &none, false),
        Decision::Allow
    );
    // A server that says a write tool is read-only does not get it past the matrix list.
    assert_eq!(
        decide(
            Some("gmail"),
            "trash_message",
            &hints(Some(true), None),
            false
        ),
        Decision::Blocked(WRITES_OFF)
    );
}

#[test]
fn an_unknown_tool_is_treated_as_a_write() {
    let none = ProbeHints::default();
    // Listed provider, unlisted tool (even one annotated read-only).
    assert_eq!(
        classify(Some("notion"), "notion-new-tool", &none),
        ToolClass::Write
    );
    assert_eq!(
        classify(
            Some("notion"),
            "notion-new-tool",
            &hints(Some(true), Some(false))
        ),
        ToolClass::Write
    );
    // A server without a list and without annotations.
    assert_eq!(classify(None, "anything", &none), ToolClass::Write);
    assert_eq!(
        classify(Some("linear"), "list_issues", &none),
        ToolClass::Write
    );
    assert_eq!(
        decide(None, "anything", &none, false),
        Decision::Blocked(WRITES_OFF)
    );
}

#[test]
fn annotations_decide_for_servers_without_a_list_and_destructive_always_wins() {
    assert_eq!(
        classify(Some("linear"), "list_issues", &hints(Some(true), None)),
        ToolClass::Read
    );
    assert_eq!(
        classify(None, "get", &hints(Some(true), Some(false))),
        ToolClass::Read
    );
    assert_eq!(
        classify(None, "get", &hints(Some(false), None)),
        ToolClass::Write
    );
    assert_eq!(
        classify(None, "get", &hints(Some(true), Some(true))),
        ToolClass::Write
    );
    assert_eq!(
        classify(
            Some("gdrive"),
            "read_file_content",
            &hints(None, Some(true))
        ),
        ToolClass::Write
    );
    assert_eq!(
        classify(
            Some("huggingface"),
            "hub_repo_search",
            &ProbeHints::default()
        ),
        ToolClass::Read
    );
}

#[test]
fn the_matrix_lists_are_disjoint_and_unique_per_provider() {
    let mut seen = std::collections::HashSet::new();
    for row in PROVIDER_TOOLS {
        assert!(seen.insert(row.provider), "{} listed twice", row.provider);
        for r in row.reads {
            assert!(!row.writes.contains(r), "{}: {r} is both", row.provider);
        }
    }
}

#[test]
fn a_tool_needing_a_scope_that_was_not_granted_is_unavailable() {
    let none = ProbeHints::default();
    let missing = vec!["write".to_string()];
    // Needs the missing scope: unavailable, even with writes on.
    assert_eq!(
        decide_scoped(
            Some("linear"),
            "save_issue",
            &none,
            true,
            &["write"],
            &missing
        ),
        Decision::Blocked(SCOPE_NOT_GRANTED)
    );
    // Needs only granted scopes, or nothing known: the normal decision.
    let read = hints(Some(true), None);
    assert_eq!(
        decide_scoped(
            Some("linear"),
            "list_issues",
            &read,
            false,
            &["read"],
            &missing
        ),
        Decision::Allow
    );
    assert_eq!(
        decide_scoped(None, "x", &none, false, &[], &missing),
        Decision::Blocked(WRITES_OFF)
    );
    // A full grant (nothing missing) never blocks on scope.
    assert_eq!(
        decide_scoped(
            Some("gmail"),
            "search_threads",
            &none,
            false,
            &["gmail.readonly"],
            &[]
        ),
        Decision::Allow
    );
}
