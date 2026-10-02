// HUP-S3.4 (core half): Hermes learns only what is proven.
// - The sidecar's learn routes are reached with validated ids only.
// - An accepted memory is kept in the learned-memory ledger (keyed by proposal id, so a second
//   accept is not a duplicate) and stored in the member's memory graph; a contradiction is Belnap
//   `both` on both sides and linked with a quarantined `contradicts` edge, never merged.
// - Publishing to the SkillRegistry is an HIC-1 ceremony action, disabled until it is signed off.

use super::*;
use crate::hermes::{ControlResp, HermesControl, HermesError, HermesManager};
use std::sync::Mutex as StdMutex;

#[derive(Default)]
struct Recorder {
    calls: StdMutex<Vec<(String, String, String)>>,
    reply: StdMutex<Vec<(u16, String)>>,
}
struct RecControl(std::sync::Arc<Recorder>);
impl RecControl {
    fn answer(&self, method: &str, url: &str, body: &str) -> ControlResp {
        self.0
            .calls
            .lock()
            .unwrap()
            .push((method.into(), url.into(), body.into()));
        let (status, body) = self.0.reply.lock().unwrap().pop().unwrap_or((200, "{}".into()));
        ControlResp { status, body }
    }
}
impl HermesControl for RecControl {
    fn get(&self, url: &str, _b: &str) -> std::result::Result<ControlResp, HermesError> {
        Ok(self.answer("GET", url, ""))
    }
    fn post(&self, url: &str, _b: &str, body: &str) -> std::result::Result<ControlResp, HermesError> {
        Ok(self.answer("POST", url, body))
    }
}

fn mgr(rec: std::sync::Arc<Recorder>) -> HermesManager {
    let dir = std::env::temp_dir().join(format!("hlearn-{}-{:?}", std::process::id(), std::thread::current().id()));
    let _ = std::fs::create_dir_all(&dir);
    let m = HermesManager::new(dir.join("bin"), dir.join("t"), dir.join("c")).with_control(Box::new(RecControl(rec)));
    m.set_token_for_test("deadbeef");
    m
}

const PID: &str = "lp-0123456789abcdef01234567";
const P1: &str = "lp-000000000000000000000001";
const P2: &str = "lp-000000000000000000000002";
const P9: &str = "lp-000000000000000000000009";

// ---- ids and routes ---------------------------------------------------------------------------

#[test]
fn proposal_and_run_ids_are_validated_before_they_reach_a_url() {
    assert!(valid_proposal_id(PID).is_ok());
    for bad in ["", "lp-", "lp-0123456789ABCDEF01234567", "lp-0123456789abcdef0123456", "../x", "lp-0123456789abcdef01234567/accept"] {
        assert!(valid_proposal_id(bad).is_err(), "{bad}");
    }
    assert!(valid_run_id("wr-12").is_ok());
    for bad in ["", "wr-", "wr-x", "wr-1/2", "12"] {
        assert!(valid_run_id(bad).is_err(), "{bad}");
    }
}

#[test]
fn the_learn_calls_use_the_sidecar_routes() {
    let rec = std::sync::Arc::new(Recorder::default());
    let m = mgr(rec.clone());
    rec.reply.lock().unwrap().push((202, r#"{"run_id":"wr-1"}"#.into()));
    let run = workflow_run(&m, "s1-ab", r#"{"id":"w","steps":[]}"#).unwrap();
    assert_eq!(run, "wr-1");
    workflow_status(&m, "s1-ab", "wr-1").unwrap();
    learn_list(&m, true).unwrap();
    learn_propose(&m, "s1-ab", "wr-1", &serde_json::json!({"kind": "memory", "key": "k", "value": "v"})).unwrap();
    learn_reject(&m, PID, "0xmember", "not right").unwrap();
    let calls = rec.calls.lock().unwrap().clone();
    let urls: Vec<String> = calls.iter().map(|(m, u, _)| format!("{m} {}", u.trim_start_matches("http://127.0.0.1:19700"))).collect();
    assert_eq!(
        urls,
        vec![
            "POST /sessions/s1-ab/workflows",
            "GET /sessions/s1-ab/workflows/wr-1",
            "GET /learn/proposals?all=true",
            "POST /learn/proposals",
            &format!("POST /learn/proposals/{PID}/reject"),
        ]
    );
    let propose: serde_json::Value = serde_json::from_str(&calls[3].2).unwrap();
    assert_eq!(propose["session_id"], "s1-ab");
    assert_eq!(propose["run_id"], "wr-1");
    assert_eq!(propose["content"]["kind"], "memory");
    let reject: serde_json::Value = serde_json::from_str(&calls[4].2).unwrap();
    assert_eq!(reject["member"], "0xmember");
}

#[test]
fn a_sidecar_refusal_carries_its_reason() {
    let rec = std::sync::Arc::new(Recorder::default());
    let m = mgr(rec.clone());
    rec.reply.lock().unwrap().push((409, r#"{"error":"conflicts not acknowledged by the member","conflicts":[]}"#.into()));
    let e = learn_accept_raw(&m, PID, "0xm", &[]).unwrap_err();
    assert!(e.starts_with("LEARN_REFUSED: conflicts not acknowledged"), "{e}");
    rec.reply.lock().unwrap().push((503, r#"{"error":"learning is off (no learn folder configured)"}"#.into()));
    let e = learn_list(&m, false).unwrap_err();
    assert!(e.contains("learning is off"), "{e}");
}

#[test]
fn bad_ids_never_reach_the_sidecar() {
    let rec = std::sync::Arc::new(Recorder::default());
    let m = mgr(rec.clone());
    assert!(learn_reject(&m, "../../stop", "m", "").is_err());
    assert!(workflow_status(&m, "s1", "wr-1/../../stop").is_err());
    assert!(workflow_run(&m, "s 1", "{}").is_err());
    assert!(learn_propose(&m, "s1", "nope", &serde_json::json!({})).is_err());
    assert!(rec.calls.lock().unwrap().is_empty());
}

// ---- the learned-memory ledger ----------------------------------------------------------------

fn record(pid: &str, key: &str, value: &str, belnap: &str, contradicts: &[&str]) -> serde_json::Value {
    serde_json::json!({
        "schema": "citrate.learn.memory.v1",
        "proposal_id": pid,
        "key": key,
        "value": value,
        "belnap": belnap,
        "contradicts": contradicts,
        "content_sha256": "ab".repeat(32),
        "evidence": {"workflow_id": "check", "steps": ["a"], "verdicts": [{"step": "a", "name": "v", "passed": true, "detail": ""}], "attempts": 1,
                     "trajectory": {"session_id": "s1", "workflow_id": "check", "messages": 2, "sha256": "cd".repeat(32)}},
        "provenance": {"session_id": "s1", "agent": "hermes", "model": "gemma"},
        "accepted_by": "0xm",
        "accepted_at_ms": 1,
        "decision_seq": 3
    })
}

#[derive(Default)]
struct FakeGraph {
    running: bool,
    fail_assert: bool,
    asserts: StdMutex<Vec<(String, String)>>,
    edges: StdMutex<Vec<(String, String, String)>>,
    supersedes: StdMutex<Vec<(String, String)>>,
    fail_supersede: bool,
    next: StdMutex<u32>,
}
impl MemoryGraph for FakeGraph {
    fn running(&self) -> bool {
        self.running
    }
    fn assert_claim(&self, tenant: &str, content: &str) -> std::result::Result<String, String> {
        if self.fail_assert {
            return Err("memory tool error: server has no signing identity".into());
        }
        self.asserts.lock().unwrap().push((tenant.into(), content.into()));
        let mut n = self.next.lock().unwrap();
        *n += 1;
        Ok(format!("asserted {:012x} [Claim] in {tenant} by me", *n))
    }
    fn propose_contradicts(&self, from: &str, to: &str, evidence: &str) -> std::result::Result<String, String> {
        self.edges.lock().unwrap().push((from.into(), to.into(), evidence.into()));
        Ok("proposed (quarantined)".into())
    }
    fn supersede(&self, from: &str, to: &str, _evidence: &str) -> std::result::Result<String, String> {
        if self.fail_supersede {
            return Err("memory tool error: no write scope".into());
        }
        self.supersedes.lock().unwrap().push((from.into(), to.into()));
        Ok("confirmed".into())
    }
}

#[test]
fn an_accepted_memory_is_stored_once_per_proposal() {
    let g = FakeGraph { running: true, ..Default::default() };
    let mut ledger = Ledger::default();
    let e = ledger.accept_record(&record(P1, "deploy chain", "40204", "true", &[]), &g).unwrap();
    assert_eq!(e.belnap, "true");
    assert_eq!(e.graph.state, "stored");
    assert_eq!(e.graph.node_id.as_deref(), Some("000000000001"));
    let (tenant, text) = g.asserts.lock().unwrap()[0].clone();
    assert_eq!(tenant, "personal");
    assert!(text.contains("deploy chain: 40204"), "{text}");
    // The same proposal accepted again (after a restart) is not stored twice.
    ledger.accept_record(&record(P1, "deploy chain", "40204", "true", &[]), &g).unwrap();
    assert_eq!(ledger.entries.len(), 1);
    assert_eq!(g.asserts.lock().unwrap().len(), 1);
}

#[test]
fn a_contradiction_is_both_on_both_sides_and_linked_not_merged() {
    let g = FakeGraph { running: true, ..Default::default() };
    let mut ledger = Ledger::default();
    ledger.accept_record(&record(P1, "deploy chain", "1", "true", &[]), &g).unwrap();
    let e = ledger
        .accept_record(&record(P2, "deploy chain", "40204", "both", &[&format!("proposal:{P1}")]), &g)
        .unwrap();
    assert_eq!(e.belnap, "both");
    assert_eq!(e.contradicts, vec![P1.to_string()]);
    let old = ledger.entries.iter().find(|x| x.proposal_id == P1).unwrap();
    assert_eq!(old.belnap, "both", "the earlier memory is marked unresolved too");
    assert!(old.contradicts.contains(&P2.to_string()));
    assert_eq!(ledger.entries.len(), 2, "both claims are kept");
    let edges = g.edges.lock().unwrap().clone();
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].0, "000000000002");
    assert_eq!(edges[0].1, "000000000001");
    let texts: Vec<String> = g.asserts.lock().unwrap().iter().map(|(_, t)| t.clone()).collect();
    assert!(texts[1].contains("contradicts"), "{texts:?}");
}

#[test]
fn when_the_memory_store_is_down_the_memory_waits_and_is_stored_later() {
    let down = FakeGraph::default();
    let mut ledger = Ledger::default();
    let e = ledger.accept_record(&record(P1, "k", "v", "true", &[]), &down).unwrap();
    assert_eq!(e.graph.state, "pending");
    assert!(e.graph.node_id.is_none());
    let up = FakeGraph { running: true, ..Default::default() };
    let n = ledger.store_pending(&up);
    assert_eq!(n, 1);
    assert_eq!(ledger.entries[0].graph.state, "stored");
    let failing = FakeGraph { running: true, fail_assert: true, ..Default::default() };
    let mut l2 = Ledger::default();
    let e = l2.accept_record(&record(P9, "k", "v", "true", &[]), &failing).unwrap();
    assert_eq!(e.graph.state, "failed");
    assert!(e.graph.detail.as_deref().unwrap_or("").contains("signing identity"));
}

#[test]
fn a_record_that_is_not_a_learn_memory_is_refused() {
    let g = FakeGraph { running: true, ..Default::default() };
    let mut ledger = Ledger::default();
    let mut bad = record(P1, "k", "v", "true", &[]);
    bad["schema"] = serde_json::json!("something.else");
    assert!(ledger.accept_record(&bad, &g).is_err());
    let mut bad = record(P1, "k", "v", "maybe", &[]);
    bad["schema"] = serde_json::json!("citrate.learn.memory.v1");
    assert!(ledger.accept_record(&bad, &g).is_err());
    assert!(ledger.entries.is_empty());
}

#[test]
fn the_ledger_round_trips_through_its_file() {
    let dir = std::env::temp_dir().join(format!("hlearn-ledger-{}-{:?}", std::process::id(), std::thread::current().id()));
    let _ = std::fs::remove_dir_all(&dir);
    let path = dir.join("learned-memories.json");
    let g = FakeGraph { running: true, ..Default::default() };
    let mut ledger = Ledger::load(&path).unwrap();
    assert!(ledger.entries.is_empty());
    ledger.accept_record(&record(P1, "k", "v", "true", &[]), &g).unwrap();
    ledger.save(&path).unwrap();
    let back = Ledger::load(&path).unwrap();
    assert_eq!(back.entries, ledger.entries);
    std::fs::write(&path, b"{ nope").unwrap();
    assert!(Ledger::load(&path).is_err(), "a damaged ledger is an error, never silently emptied");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_assert_reply_names_the_new_node() {
    assert_eq!(node_id_from_assert("asserted 0a1b2c3d4e5f [Claim] in personal by x").as_deref(), Some("0a1b2c3d4e5f"));
    assert_eq!(node_id_from_assert("asserted 0a1b2c3d4e5f [Claim] in personal by x — WARNING: not embedded").as_deref(), Some("0a1b2c3d4e5f"));
    assert_eq!(node_id_from_assert("server has no signing identity"), None);
    assert_eq!(node_id_from_assert("asserted zz [Claim]"), None);
}

// ---- publishing -------------------------------------------------------------------------------

#[test]
fn publishing_is_off_until_signed_off_and_says_why() {
    let p = publish_availability(false, Some("0x60806040632a99614500"));
    assert!(!p.enabled);
    assert!(p.note.contains("pending owner sign-off"), "{}", p.note);
    let p = publish_availability(true, Some("0x"));
    assert!(!p.enabled);
    assert!(p.note.contains("not deployed"), "{}", p.note);
    let p = publish_availability(true, None);
    assert!(!p.enabled);
    let p = publish_availability(true, Some("0x6080604052600436106100"));
    assert!(!p.enabled, "code without registerSkill is not the registry");
    let p = publish_availability(true, Some("0x608060405263632a996145"));
    assert!(p.enabled, "{}", p.note);
}

fn payload(to: &str, owner: &str) -> serde_json::Value {
    serde_json::json!({
        "chain_id": 40204, "to": to, "value": "0x0",
        "data": "0x2a996145", "function": "registerSkill(string,string,string,string,string[])",
        "name": "deploy-checklist", "version": "1.0.0", "manifest_cid": "", "description": "d",
        "tags": ["hermes-learned"], "owner": owner, "content_sha256": "ab", "expected_skill_hash": "0x00",
        "hic": "hic-1", "broadcast": false, "proposal_id": PID
    })
}

#[test]
fn a_publish_payload_becomes_a_ceremony_intent_only_when_it_matches() {
    let reg = "0x2b687899ef4af05a18f4f36ce1fe9d51c017a97c";
    let me = "0x1111111111111111111111111111111111111111";
    let intent = publish_intent(&payload(reg, me), reg, me).unwrap();
    assert_eq!(intent.origin, "agent:hermes");
    assert_eq!(intent.chain_id, 40204);
    let tx: serde_json::Value = serde_json::from_str(&intent.raw).unwrap();
    assert_eq!(tx["to"], reg);
    assert_eq!(tx["from"], me);
    assert_eq!(tx["value"], "0x0");
    assert!(tx["data"].as_str().unwrap().starts_with("0x2a996145"));
    assert!(tx["gas"].as_str().unwrap().starts_with("0x"));
    // Another target, another owner, a broadcast flag, another function or chain: refused.
    assert!(publish_intent(&payload("0x2222222222222222222222222222222222222222", me), reg, me).is_err());
    assert!(publish_intent(&payload(reg, "0x3333333333333333333333333333333333333333"), reg, me).is_err());
    let mut p = payload(reg, me);
    p["broadcast"] = serde_json::json!(true);
    assert!(publish_intent(&p, reg, me).is_err());
    let mut p = payload(reg, me);
    p["data"] = serde_json::json!("0xa9059cbb");
    assert!(publish_intent(&p, reg, me).is_err());
    let mut p = payload(reg, me);
    p["chain_id"] = serde_json::json!(1);
    assert!(publish_intent(&p, reg, me).is_err());
    let mut p = payload(reg, me);
    p["value"] = serde_json::json!("0x1");
    assert!(publish_intent(&p, reg, me).is_err());
}

// ---- the sidecar env ----------------------------------------------------------------------------

#[test]
fn the_sidecar_gets_the_learn_and_skills_folders() {
    let dir = std::env::temp_dir().join(format!("hlearn-env-{}", std::process::id()));
    let m = HermesManager::new(dir.join("bin"), dir.join("t"), dir.join("c"))
        .with_learn_dirs(dir.join("learn"), dir.join("skills"));
    let env = m.spec_env_for_test();
    let get = |k: &str| env.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
    assert_eq!(get("CITRATE_HERMES_LEARN_DIR"), Some(dir.join("learn").to_string_lossy().to_string()));
    assert_eq!(get("CITRATE_HERMES_LEARN_SKILLS_DIR"), Some(dir.join("skills").to_string_lossy().to_string()));
    assert_eq!(get("CITRATE_HERMES_SKILLS"), Some(dir.join("skills").to_string_lossy().to_string()), "accepted skills load in later sessions");
    let plain = HermesManager::new(dir.join("bin"), dir.join("t"), dir.join("c"));
    assert!(plain.spec_env_for_test().iter().all(|(n, _)| !n.contains("LEARN")), "no folders, no learning");
}

// ---- resolving a contradiction (US-3.4 AC4; formal/ContradictionResolve.tla in the runtime) ----

const P3: &str = "lp-000000000000000000000003";

fn resolution(kept: &str, retracted: &str) -> serde_json::Value {
    serde_json::json!({
        "schema": "citrate.learn.resolve.v1",
        "kept": kept,
        "retracted": retracted,
        "key": "deploy chain",
        "kept_value": "40204",
        "retracted_value": "1",
        "decided_by": "0xm",
        "decided_at_ms": 5,
        "decision_seq": 9
    })
}

fn entry<'a>(l: &'a Ledger, pid: &str) -> &'a LearnedMemory {
    l.entries.iter().find(|e| e.proposal_id == pid).unwrap()
}

/// P1 then P2 on the same key with different values: both `both`, both stored.
fn two_both(g: &FakeGraph) -> Ledger {
    let mut l = Ledger::default();
    l.accept_record(&record(P1, "deploy chain", "1", "true", &[]), g).unwrap();
    l.accept_record(&record(P2, "deploy chain", "40204", "both", &[&format!("proposal:{P1}")]), g).unwrap();
    l
}

#[test]
fn the_resolve_call_uses_the_sidecar_route_with_validated_ids() {
    let rec = std::sync::Arc::new(Recorder::default());
    let m = mgr(rec.clone());
    rec.reply.lock().unwrap().push((200, serde_json::json!({"ok": true, "resolution": resolution(P2, P1)}).to_string()));
    let r = learn_resolve(&m, P2, P1, "0xmember").unwrap();
    assert_eq!(r["kept"], P2);
    let calls = rec.calls.lock().unwrap().clone();
    assert_eq!(calls.len(), 1);
    assert!(calls[0].1.ends_with("/learn/memories/resolve"), "{}", calls[0].1);
    let body: serde_json::Value = serde_json::from_str(&calls[0].2).unwrap();
    assert_eq!(body, serde_json::json!({"member": "0xmember", "keep": P2, "retract": P1}));
    assert!(learn_resolve(&m, "../x", P1, "m").is_err());
    assert!(learn_resolve(&m, P2, "lp-zz", "m").is_err());
    assert!(learn_resolve(&m, P2, P2, "m").is_err(), "a memory cannot be kept and retracted at once");
    assert_eq!(rec.calls.lock().unwrap().len(), 1, "bad ids never reach the sidecar");
}

#[test]
fn a_resolution_retracts_one_side_and_settles_the_kept_one() {
    let g = FakeGraph { running: true, ..Default::default() };
    let mut l = two_both(&g);
    let old_p1 = entry(&l, P1).graph.node_id.clone().unwrap();
    let old_p2 = entry(&l, P2).graph.node_id.clone().unwrap();
    assert!(l.apply_resolution(&resolution(P2, P1), &g).unwrap());
    let d = entry(&l, P1);
    assert_eq!(d.belnap, "false", "retracted, kept for the record");
    assert_eq!(d.retracted_for.as_deref(), Some(P2));
    assert_eq!(d.resolved_seq, Some(9));
    assert!(d.contradicts.is_empty());
    let k = entry(&l, P2);
    assert_eq!(k.belnap, "true");
    assert!(k.contradicts.is_empty());
    assert_eq!(k.graph.state, "stored");
    // The kept memory is stored again as settled, and the new node supersedes both old ones.
    let new_node = k.graph.node_id.clone().unwrap();
    assert_ne!(new_node, old_p2);
    assert!(k.supersede_nodes.is_empty(), "{:?}", k.supersede_nodes);
    let texts: Vec<String> = g.asserts.lock().unwrap().iter().map(|(_, t)| t.clone()).collect();
    let last = texts.last().unwrap();
    assert!(last.contains("deploy chain: 40204") && !last.contains("unresolved"), "{last}");
    let mut sup = g.supersedes.lock().unwrap().clone();
    sup.sort();
    let mut want = vec![(new_node.clone(), old_p1), (new_node, old_p2)];
    want.sort();
    assert_eq!(sup, want);
    // Applying the same resolution again changes nothing.
    assert!(!l.apply_resolution(&resolution(P2, P1), &g).unwrap());
    assert_eq!(g.supersedes.lock().unwrap().len(), 2);
}

#[test]
fn a_resolution_that_is_not_one_is_refused() {
    let g = FakeGraph { running: true, ..Default::default() };
    let mut l = two_both(&g);
    let mut bad = resolution(P2, P1);
    bad["schema"] = serde_json::json!("citrate.learn.memory.v1");
    assert!(l.apply_resolution(&bad, &g).is_err());
    assert!(l.apply_resolution(&resolution("lp-x", P1), &g).is_err());
    assert!(l.apply_resolution(&resolution(P1, P1), &g).is_err());
    assert_eq!(entry(&l, P1).belnap, "both");
    // A resolution for memories this ledger never received changes nothing (honestly).
    assert!(!l.apply_resolution(&resolution(P9, P3), &g).unwrap());
}

#[test]
fn with_three_memories_the_kept_one_stays_both_until_every_contradiction_is_resolved() {
    let g = FakeGraph { running: true, ..Default::default() };
    let mut l = two_both(&g);
    l.accept_record(&record(P3, "deploy chain", "7", "both", &[&format!("proposal:{P1}"), &format!("proposal:{P2}")]), &g).unwrap();
    assert!(l.apply_resolution(&resolution(P2, P1), &g).unwrap());
    let k = entry(&l, P2);
    assert_eq!(k.belnap, "both", "P3 still disagrees with P2");
    assert_eq!(k.contradicts, vec![P3.to_string()]);
    // Not settled, so not stored again: the existing node supersedes the retracted one directly.
    assert_eq!(g.supersedes.lock().unwrap().len(), 1);
    assert_eq!(entry(&l, P3).contradicts, vec![P2.to_string()]);
    assert!(l.apply_resolution(&resolution(P2, P3), &g).unwrap());
    assert_eq!(entry(&l, P2).belnap, "true");
    assert_eq!(entry(&l, P3).belnap, "false");
}

#[test]
fn a_resolution_applied_out_of_order_never_settles_a_memory_that_is_itself_retracted() {
    // ContradictionResolve.tla finding 2. The sidecar recorded (keep P2, retract P1) and then
    // (keep P3, retract P2); core lost the first answer and sees the second first.
    let g = FakeGraph { running: true, ..Default::default() };
    let mut l = two_both(&g);
    l.accept_record(&record(P3, "deploy chain", "7", "both", &[&format!("proposal:{P1}"), &format!("proposal:{P2}")]), &g).unwrap();
    assert!(l.apply_resolution(&resolution(P3, P2), &g).unwrap());
    assert_eq!(entry(&l, P2).belnap, "false");
    assert_eq!(entry(&l, P3).belnap, "both", "P1 still disagrees with P3");
    let p1 = entry(&l, P1);
    assert!(p1.contradicts == vec![P3.to_string()]);
    assert_eq!(p1.belnap, "both", "P1 is not the kept one here; core must not promote it");
    // The first resolution arrives late: P1 retracted; P2 (kept there) is already false and
    // stays false; P3 has nothing left to contradict but was not kept here.
    assert!(l.apply_resolution(&resolution(P2, P1), &g).unwrap());
    assert_eq!(entry(&l, P1).belnap, "false");
    assert_eq!(entry(&l, P2).belnap, "false", "a retracted memory is never settled");
    assert!(entry(&l, P2).supersede_nodes.is_empty(), "a retracted memory takes over nothing in the graph");
    assert_eq!(entry(&l, P3).belnap, "both");
    // A sync whose list does not show P3 standing (pruned, or not in the list) leaves it alone.
    let partial = serde_json::json!({"proposals": [
        {"id": P1, "kind": "memory", "state": {"state": "retracted", "by": "0xm", "kept": P2}},
        {"id": P3, "kind": "memory", "state": {"state": "proposed"}}
    ]});
    assert_eq!(l.sync_with_sidecar(&partial, &g), 0);
    assert_eq!(entry(&l, P3).belnap, "both");
    // The sync settles P3, because the sidecar shows it standing.
    let list = serde_json::json!({"proposals": [
        {"id": P1, "kind": "memory", "state": {"state": "retracted", "by": "0xm", "kept": P2}},
        {"id": P2, "kind": "memory", "state": {"state": "retracted", "by": "0xm", "kept": P3}},
        {"id": P3, "kind": "memory", "state": {"state": "persisted"}}
    ]});
    assert_eq!(l.sync_with_sidecar(&list, &g), 1);
    assert_eq!(entry(&l, P3).belnap, "true");
    assert_eq!(entry(&l, P2).belnap, "false");
}

#[test]
fn the_sync_applies_a_resolution_whose_answer_was_lost() {
    let g = FakeGraph { running: true, ..Default::default() };
    let mut l = two_both(&g);
    let list = serde_json::json!({"proposals": [
        {"id": P1, "kind": "memory", "state": {"state": "retracted", "by": "0xm", "kept": P2}},
        {"id": P2, "kind": "memory", "state": {"state": "persisted"}},
        {"id": P9, "kind": "skill", "state": {"state": "persisted"}}
    ]});
    assert_eq!(l.sync_with_sidecar(&list, &g), 1, "one resolution applied, which settles the kept memory");
    assert_eq!(entry(&l, P1).belnap, "false");
    assert_eq!(entry(&l, P2).belnap, "true");
    assert_eq!(l.sync_with_sidecar(&list, &g), 0, "idempotent");
    // A memory the sidecar no longer lists (pruned) or shows undecided is never settled by sync.
    let mut l2 = two_both(&g);
    let none = serde_json::json!({"proposals": []});
    assert_eq!(l2.sync_with_sidecar(&none, &g), 0);
    assert_eq!(entry(&l2, P2).belnap, "both");
    let bogus = serde_json::json!({"proposals": [{"id": P1, "kind": "memory", "state": {"state": "retracted", "kept": "../x"}}]});
    assert_eq!(l2.sync_with_sidecar(&bogus, &g), 0);
    assert_eq!(entry(&l2, P1).belnap, "both");
}

#[test]
fn when_the_memory_store_is_down_the_graph_part_of_a_resolution_waits() {
    let up = FakeGraph { running: true, ..Default::default() };
    let mut l = two_both(&up);
    let down = FakeGraph::default();
    assert!(l.apply_resolution(&resolution(P2, P1), &down).unwrap());
    assert_eq!(entry(&l, P1).belnap, "false", "the ledger decision does not wait for the graph");
    let k = entry(&l, P2);
    assert_eq!(k.belnap, "true");
    assert_eq!(k.graph.state, "pending");
    assert_eq!(k.supersede_nodes.len(), 2);
    assert_eq!(l.store_pending(&up), 1);
    let k = entry(&l, P2);
    assert_eq!(k.graph.state, "stored");
    assert!(k.supersede_nodes.is_empty());
    assert_eq!(up.supersedes.lock().unwrap().len(), 2);
    // A failed supersede keeps the node to retry and says why.
    let mut l = two_both(&up);
    let flaky = FakeGraph { running: true, fail_supersede: true, ..Default::default() };
    l.apply_resolution(&resolution(P2, P1), &flaky).unwrap();
    let k = entry(&l, P2);
    assert_eq!(k.graph.state, "stored");
    assert_eq!(k.supersede_nodes.len(), 2);
    assert!(k.graph.detail.as_deref().unwrap_or("").contains("no write scope"), "{:?}", k.graph.detail);
}

#[test]
fn a_retracted_memory_that_never_reached_the_graph_is_never_stored() {
    let down = FakeGraph::default();
    let mut l = Ledger::default();
    l.accept_record(&record(P1, "deploy chain", "1", "true", &[]), &down).unwrap();
    l.accept_record(&record(P2, "deploy chain", "40204", "both", &[&format!("proposal:{P1}")]), &down).unwrap();
    l.apply_resolution(&resolution(P2, P1), &down).unwrap();
    assert_eq!(entry(&l, P1).graph.state, "retracted");
    let up = FakeGraph { running: true, ..Default::default() };
    // The sidecar handing the same record over again (an accept after a lost save) does not
    // bring a retracted memory into the graph.
    let again = l.accept_record(&record(P1, "deploy chain", "1", "true", &[]), &up).unwrap();
    assert_eq!(again.belnap, "false");
    assert_eq!(again.graph.state, "retracted");
    assert!(up.asserts.lock().unwrap().is_empty());
    assert_eq!(l.store_pending(&up), 1, "only the kept memory is stored");
    let texts: Vec<String> = up.asserts.lock().unwrap().iter().map(|(_, t)| t.clone()).collect();
    assert_eq!(texts.len(), 1);
    assert!(texts[0].contains("40204"));
    assert!(up.supersedes.lock().unwrap().is_empty(), "nothing old to supersede");
}

// ---- pinning a skill before publishing --------------------------------------------------------

#[derive(Default)]
struct FakeKubo {
    added: StdMutex<Vec<(String, Vec<u8>)>>,
    pinned: StdMutex<Vec<String>>,
    serve: StdMutex<Option<Vec<u8>>>,
    down: bool,
}
impl crate::storage::KuboTransport for FakeKubo {
    fn add(&self, filename: &str, bytes: &[u8]) -> std::result::Result<crate::storage::AddOutcome, crate::storage::StorageError> {
        if self.down {
            return Err(crate::storage::StorageError::Transport("connection refused".into()));
        }
        self.added.lock().unwrap().push((filename.into(), bytes.to_vec()));
        Ok(crate::storage::AddOutcome { cid: "bafkreiexampleskillcid".into(), size_bytes: bytes.len() as u64 })
    }
    fn pin_add(&self, cid: &str) -> std::result::Result<(), crate::storage::StorageError> {
        self.pinned.lock().unwrap().push(cid.into());
        Ok(())
    }
    fn pin_rm(&self, _cid: &str) -> std::result::Result<(), crate::storage::StorageError> {
        Ok(())
    }
    fn pin_ls(&self) -> std::result::Result<Vec<String>, crate::storage::StorageError> {
        Ok(self.pinned.lock().unwrap().clone())
    }
    fn cat(&self, _cid: &str) -> std::result::Result<Vec<u8>, crate::storage::StorageError> {
        let served = self.serve.lock().unwrap().clone();
        Ok(served.unwrap_or_else(|| self.added.lock().unwrap().last().map(|(_, b)| b.clone()).unwrap_or_default()))
    }
}

const SKILL_MD: &str = "---\nname: deploy-checklist\ndescription: Checks a contract before deploy\n---\n\n1. Run the tests.\n";

fn sha(s: &str) -> String {
    use sha2::Digest as _;
    hex::encode(sha2::Sha256::digest(s.as_bytes()))
}

#[test]
fn a_skill_is_pinned_and_read_back_before_its_cid_is_used() {
    let k = FakeKubo::default();
    let cid = pin_skill(&k, SKILL_MD, &sha(SKILL_MD)).unwrap();
    assert_eq!(cid, "bafkreiexampleskillcid");
    assert_eq!(k.added.lock().unwrap()[0].0, "SKILL.md");
    assert_eq!(k.added.lock().unwrap()[0].1, SKILL_MD.as_bytes());
    assert_eq!(k.pinned.lock().unwrap().clone(), vec![cid]);
}

#[test]
fn a_skill_pin_is_refused_when_the_bytes_do_not_match() {
    let k = FakeKubo::default();
    let e = pin_skill(&k, SKILL_MD, &"00".repeat(32)).unwrap_err();
    assert!(e.contains("does not match"), "{e}");
    assert!(k.added.lock().unwrap().is_empty(), "nothing is added for a mismatched skill");
    let k = FakeKubo { serve: StdMutex::new(Some(b"something else".to_vec())), ..Default::default() };
    let e = pin_skill(&k, SKILL_MD, &sha(SKILL_MD)).unwrap_err();
    assert!(e.contains("read back"), "{e}");
    let k = FakeKubo { down: true, ..Default::default() };
    let e = pin_skill(&k, SKILL_MD, &sha(SKILL_MD)).unwrap_err();
    assert!(e.contains("IPFS"), "{e}");
}

#[test]
fn the_publish_payload_must_carry_the_pinned_cid() {
    let reg = "0x2b687899ef4af05a18f4f36ce1fe9d51c017a97c";
    let me = "0x1111111111111111111111111111111111111111";
    let mut p = payload(reg, me);
    p["manifest_cid"] = serde_json::json!("bafkreiexampleskillcid");
    assert!(check_manifest_cid(&p, "bafkreiexampleskillcid").is_ok());
    assert!(check_manifest_cid(&p, "bafkreiother").is_err());
    p["manifest_cid"] = serde_json::json!("");
    assert!(check_manifest_cid(&p, "bafkreiexampleskillcid").is_err());
}
