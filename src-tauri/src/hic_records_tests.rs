//! HUP-S2.6 (core): the HIC outbox, its export to the sidecar's `POST /records/core`, and the
//! events core records.
//!
//! BDD map (US-7.2 AC1, "local decision records for every HIC-1/2 event"):
//! - The outbox is hash-chained and tamper-evident: `appends_chain_and_a_tampered_line_fails_closed`.
//! - Export copies every record once, in order, in the sidecar's wire form, and keeps the cursor on
//!   a failure: `export_sends_each_record_once_and_keeps_the_cursor_on_failure`.
//! - Bounds (pending owner sign-off): `too_many_unexported_records_fail_closed`,
//!   `compaction_keeps_the_chain_verifiable`.
//! - The rules match the sidecar's: `validation_matches_the_sidecar_rules`.
//! - Event mapping: `escalation_spend_is_hic2_inside_the_budget_and_hic1_when_confirmed`,
//!   `approval_cards_are_the_only_webview_kinds`.

use super::*;
use std::sync::Mutex as StdMutex;

struct FakeLink {
    posts: StdMutex<Vec<(String, serde_json::Value)>>,
    status: StdMutex<u16>,
}

impl FakeLink {
    fn new(status: u16) -> Self {
        FakeLink {
            posts: StdMutex::new(Vec::new()),
            status: StdMutex::new(status),
        }
    }
    fn sent_ids(&self) -> Vec<u64> {
        self.posts
            .lock()
            .unwrap()
            .iter()
            .flat_map(|(_, b)| {
                b["records"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|r| r["recordId"].as_u64().unwrap())
                    .collect::<Vec<_>>()
            })
            .collect()
    }
}

impl SidecarLink for FakeLink {
    fn get(&self, _path: &str) -> Result<(u16, String), String> {
        Err("unused".into())
    }
    fn post(&self, path: &str, body: &str) -> Result<(u16, String), String> {
        self.posts
            .lock()
            .unwrap()
            .push((path.to_string(), serde_json::from_str(body).unwrap()));
        let s = *self.status.lock().unwrap();
        Ok((
            s,
            r#"{"error":"the records could not be written"}"#.to_string(),
        ))
    }
}

/// A fresh folder, removed when dropped.
struct TempDir(std::path::PathBuf);
impl TempDir {
    fn path(&self) -> &std::path::Path {
        &self.0
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn tempdir() -> TempDir {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let p = std::env::temp_dir().join(format!(
        "core-hic-outbox-{}-{}",
        std::process::id(),
        N.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    TempDir(p)
}

fn outbox() -> (TempDir, HicOutbox) {
    let d = tempdir();
    let o = HicOutbox::new(d.path().join(OUTBOX_DIR));
    (d, o)
}

#[test]
fn appends_chain_and_a_tampered_line_fails_closed() {
    let (_d, o) = outbox();
    let a = o
        .append(
            grant_added("/home/m/proj", &["g-1".into(), "g-2".into()]),
            10,
        )
        .unwrap();
    let b = o.append(grant_revoked("g-1"), 11).unwrap();
    assert_eq!((a.record_id, b.record_id), (1, 2));
    assert_eq!(a.prev_hash, GENESIS);
    assert_eq!(b.prev_hash, a.hash);
    assert_eq!(o.records().unwrap().len(), 2);

    // Edit the first record's subject on disk: the chain no longer verifies, and the outbox
    // takes and exports nothing.
    let file = o.dir().join("outbox.jsonl");
    let text = std::fs::read_to_string(&file).unwrap();
    std::fs::write(&file, text.replacen("/home/m/proj", "/home/m/other", 1)).unwrap();
    assert!(o.records().unwrap_err().contains("broken"));
    assert!(o.append(grant_reset(), 12).is_err());
    let link = FakeLink::new(200);
    assert!(o.export(&link).is_err());
    assert!(link.posts.lock().unwrap().is_empty());
}

#[test]
fn export_sends_each_record_once_and_keeps_the_cursor_on_failure() {
    let (_d, o) = outbox();
    for i in 0..150u64 {
        o.append(grant_revoked(&format!("g-{i}")), 100 + i).unwrap();
    }
    // The sidecar refuses: nothing advances.
    let bad = FakeLink::new(500);
    assert!(o.export(&bad).unwrap_err().contains("could not be written"));
    assert_eq!(o.exported().unwrap(), 0);
    // No records folder on the sidecar: not configured, nothing advances.
    let off = FakeLink::new(404);
    assert_eq!(o.export(&off).unwrap(), ExportResult::NotConfigured);
    assert_eq!(o.exported().unwrap(), 0);

    let ok = FakeLink::new(200);
    assert_eq!(o.export(&ok).unwrap(), ExportResult::Exported(150));
    assert_eq!(ok.sent_ids(), (1..=150).collect::<Vec<_>>());
    let posts = ok.posts.lock().unwrap().clone();
    assert_eq!(posts.len(), 2, "batches of {EXPORT_BATCH}");
    assert!(posts.iter().all(|(p, _)| p == "/records/core"));
    let first = &posts[0].1["records"][0];
    assert_eq!(first["kind"], "grant.revoked");
    assert_eq!(first["decision"], "approved");
    assert_eq!(first["outcome"], "completed");
    assert_eq!(first["atMs"], 100);
    assert_eq!(first["hash"], o.records().unwrap()[0].hash);
    assert_eq!(o.exported().unwrap(), 150);

    // Nothing new: nothing sent. One more: only it.
    let again = FakeLink::new(200);
    assert_eq!(o.export(&again).unwrap(), ExportResult::Exported(0));
    o.append(grant_reset(), 999).unwrap();
    assert_eq!(o.export(&again).unwrap(), ExportResult::Exported(1));
    assert_eq!(again.sent_ids(), vec![151]);
}

#[test]
fn too_many_unexported_records_fail_closed() {
    let d = tempdir();
    let o = HicOutbox::new(d.path().join(OUTBOX_DIR)).with_bounds(3, 100);
    for i in 0..3 {
        o.append(grant_revoked(&format!("g-{i}")), i).unwrap();
    }
    assert!(o.check_writable().is_err());
    assert!(o.append(grant_reset(), 9).unwrap_err().contains("waiting"));
    o.export(&FakeLink::new(200)).unwrap();
    o.check_writable().unwrap();
    assert_eq!(o.append(grant_reset(), 10).unwrap().record_id, 4);
}

#[test]
fn compaction_keeps_the_chain_verifiable() {
    let d = tempdir();
    let o = HicOutbox::new(d.path().join(OUTBOX_DIR)).with_bounds(1_000, 5);
    for i in 0..12u64 {
        o.append(grant_revoked(&format!("g-{i}")), i).unwrap();
    }
    o.export(&FakeLink::new(200)).unwrap();
    let kept = o.records().unwrap();
    assert_eq!(kept.len(), 5);
    assert_eq!(kept[0].record_id, 8);
    // The chain continues from the base line.
    let next = o.append(grant_reset(), 50).unwrap();
    assert_eq!(next.record_id, 13);
    assert_eq!(next.prev_hash, kept[4].hash);
    assert_eq!(o.records().unwrap().len(), 6);
    let link = FakeLink::new(200);
    o.export(&link).unwrap();
    assert_eq!(link.sent_ids(), vec![13]);
}

#[test]
fn validation_matches_the_sidecar_rules() {
    let mut ev = grant_revoked("g-1");
    ev.decision = "auto_within_budget".into();
    assert!(
        validate(&ev).is_err(),
        "HIC-2 is only an escalation within budget"
    );
    let mut ev = grant_revoked("g-1");
    ev.decision = "denied".into();
    assert!(
        validate(&ev).is_err(),
        "a grant change is the member's approval"
    );
    let mut ev = grant_revoked("g-1");
    ev.kind = "wallet.sweep".into();
    assert!(validate(&ev).is_err());
    let ev = card_event("agent.tool_approval", "denied", "fs_write", "").unwrap();
    assert_eq!(ev.outcome, None);
    let mut bad = ev.clone();
    bad.outcome = Some("completed".into());
    assert!(validate(&bad).is_err(), "a denial has no outcome");
    let mut bad = ev;
    bad.evidence.push(HicEvidence {
        kind: "x".into(),
        uri: "y".into(),
        digest: Some("0x12".into()),
    });
    assert!(validate(&bad).is_err());
    // A refused event is never stored.
    let (_d, o) = outbox();
    let mut ev = grant_reset();
    ev.kind = "nope".into();
    assert!(o.append(ev, 1).is_err());
    assert!(o.records().unwrap().is_empty());
}

fn spend(mode: crate::escalation::Mode, outcome: &str) -> crate::escalation::SpendRecord {
    crate::escalation::SpendRecord {
        escalation_id: "esc-1".into(),
        endpoint_id: "ep-1".into(),
        destination: "api.provider.example".into(),
        quoted_micros: 2_000,
        charged_micros: 1_500,
        mode,
        outcome: outcome.into(),
        usage_reported: true,
        exceeded_quote: false,
        at_ms: 5,
    }
}

#[test]
fn escalation_spend_is_hic2_inside_the_budget_and_hic1_when_confirmed() {
    use crate::escalation::Mode;
    let e = escalation_spent(&spend(Mode::Budget, "answered"));
    assert_eq!(
        (e.decision.as_str(), e.outcome.as_deref()),
        ("auto_within_budget", Some("completed"))
    );
    validate(&e).unwrap();
    let e = escalation_spent(&spend(Mode::Confirmed, "not_sent"));
    assert_eq!(
        (e.decision.as_str(), e.outcome.as_deref()),
        ("approved", Some("failed"))
    );
    let e = escalation_spent(&spend(Mode::Confirmed, "failed"));
    assert_eq!(e.outcome.as_deref(), Some("outcome_unknown"));
    assert!(e.subject.contains("esc-1"));
}

#[test]
fn approval_cards_are_the_only_webview_kinds() {
    for kind in [
        "grant.revoked",
        "escalation.spend",
        "grant.full_access_confirmed",
    ] {
        assert!(card_event(kind, "approved", "x", "y").is_err(), "{kind}");
    }
    assert!(card_event("ceremony.approval", "auto_within_budget", "x", "").is_err());
    let ev = card_event("ceremony.approval", "approved", "Send 1 SALT", "").unwrap();
    assert!(ev.reason.contains("approval card"));
    assert!(card_event("ceremony.approval", "approved", "", "").is_err());
}

#[test]
fn the_record_command_is_async_off_main_and_only_in_the_main_window_acl() {
    let src = include_str!("hic_records.rs");
    assert!(src.contains("pub async fn hic_record_decision"));
    assert!(src.contains("crate::blocking::off_main"));
    let acl = include_str!("../permissions/main-window.toml");
    assert!(acl.contains("\"hic_record_decision\""));
    let lib = include_str!("lib.rs");
    assert!(lib.contains("hic_records::hic_record_decision"));
    let popout = include_str!("../capabilities/popout.json");
    assert!(
        !popout.contains("hic_record_decision"),
        "pop-outs stay least-privilege"
    );
}
