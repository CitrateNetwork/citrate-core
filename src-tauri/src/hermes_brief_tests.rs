// HUP-S1.4 (core half) — the interview card's Rust side: GET /tracks, POST /briefs, POST /briefs/check
// through the same bearer-authed control client as the session calls. Inputs are bounded before they
// reach the sidecar; a 422 from the sidecar surfaces its reason (BRIEF_REFUSED: …) so the card can
// show it inline.

use super::*;
use std::collections::BTreeMap;
use std::sync::Mutex as StdMutex;

#[derive(Default)]
struct Recorder {
    posts: StdMutex<Vec<(String, String)>>,
    gets: StdMutex<Vec<String>>,
    reply: StdMutex<Vec<(u16, String)>>,
}
struct RecControl(std::sync::Arc<Recorder>);
impl HermesControl for RecControl {
    fn get(&self, url: &str, _b: &str) -> std::result::Result<ControlResp, HermesError> {
        self.0.gets.lock().unwrap().push(url.to_string());
        let (status, body) = self.0.reply.lock().unwrap().pop().unwrap_or((200, "[]".into()));
        Ok(ControlResp { status, body })
    }
    fn post(&self, url: &str, _b: &str, body: &str) -> std::result::Result<ControlResp, HermesError> {
        self.0.posts.lock().unwrap().push((url.to_string(), body.to_string()));
        let (status, body) = self.0.reply.lock().unwrap().pop().unwrap_or((200, "{}".into()));
        Ok(ControlResp { status, body })
    }
}

fn mgr(rec: std::sync::Arc<Recorder>) -> HermesManager {
    let dir = std::env::temp_dir().join(format!("hbrief-{}-{:?}", std::process::id(), std::thread::current().id()));
    let _ = std::fs::create_dir_all(&dir);
    let m = HermesManager::new(dir.join("bin"), dir.join("t"), dir.join("c")).with_control(Box::new(RecControl(rec)));
    m.set_token_for_test("deadbeef");
    m
}

const TRACKS: &str = r#"[{"id":"code","title":"Code","summary":"Write or fix code","persona":"Builder",
 "skills":["repo-read"],"workflow":"code-change","workflow_available":false,"ships_in":"0.5.0",
 "gates":["tests pass"],"questions":[
   {"id":"lang","ask":"Language?","choices":["rust","ts"],"default":"rust"},
   {"id":"scope","ask":"Scope?","default":"one crate"},
   {"id":"tests","ask":"Tests?","choices":["yes","no"],"default":"yes"}]}]"#;

fn brief() -> Brief {
    Brief {
        track: "code".into(),
        goal: "fix the parser".into(),
        constraints: vec![BriefConstraint { id: "lang".into(), ask: "Language?".into(), answer: "rust".into(), from_default: true }],
        persona: "Builder".into(),
        skills: vec!["repo-read".into()],
        workflow: "code-change".into(),
        workflow_available: false,
        ships_in: Some("0.5.0".into()),
        gates: vec!["tests pass".into()],
    }
}

#[test]
fn tracks_are_read_from_the_sidecar_with_their_honest_workflow_status() {
    let rec = std::sync::Arc::new(Recorder::default());
    *rec.reply.lock().unwrap() = vec![(200, TRACKS.into())];
    let m = mgr(rec.clone());
    let tracks = m.tracks().unwrap();
    assert_eq!(tracks.len(), 1);
    assert_eq!(tracks[0].questions.len(), 3);
    assert_eq!(tracks[0].questions[1].choices, Vec::<String>::new(), "free-text questions have no choices");
    assert!(!tracks[0].workflow_available);
    assert_eq!(tracks[0].ships_in.as_deref(), Some("0.5.0"));
    assert!(rec.gets.lock().unwrap()[0].ends_with("/tracks"));
}

#[test]
fn a_brief_request_posts_track_goal_and_answers() {
    let rec = std::sync::Arc::new(Recorder::default());
    let reply = serde_json::json!({ "brief": brief(), "markdown": "# Brief" }).to_string();
    *rec.reply.lock().unwrap() = vec![(200, reply)];
    let m = mgr(rec.clone());
    let mut answers = BTreeMap::new();
    answers.insert("lang".to_string(), "ts".to_string());
    let out = m.brief_create(Some("code"), "fix the parser", &answers).unwrap();
    assert_eq!(out.brief.track, "code");
    assert_eq!(out.markdown, "# Brief");
    let posts = rec.posts.lock().unwrap();
    assert!(posts[0].0.ends_with("/briefs"));
    let body: serde_json::Value = serde_json::from_str(&posts[0].1).unwrap();
    assert_eq!(body["track"], "code");
    assert_eq!(body["goal"], "fix the parser");
    assert_eq!(body["answers"]["lang"], "ts");
}

#[test]
fn no_track_lets_the_sidecar_suggest_one() {
    let rec = std::sync::Arc::new(Recorder::default());
    let reply = serde_json::json!({ "brief": brief(), "markdown": "# Brief" }).to_string();
    *rec.reply.lock().unwrap() = vec![(200, reply)];
    let m = mgr(rec.clone());
    m.brief_create(None, "fix the parser", &BTreeMap::new()).unwrap();
    let body: serde_json::Value = serde_json::from_str(&rec.posts.lock().unwrap()[0].1).unwrap();
    assert!(body.get("track").is_none() || body["track"].is_null(), "{body}");
}

#[test]
fn a_422_surfaces_the_sidecar_reason_as_brief_refused() {
    let rec = std::sync::Arc::new(Recorder::default());
    *rec.reply.lock().unwrap() = vec![(422, r#"{"error":"required gate removed: tests pass"}"#.into())];
    let m = mgr(rec);
    let err = m.brief_check(&brief()).unwrap_err().to_string();
    assert_eq!(err, "BRIEF_REFUSED: required gate removed: tests pass");
}

#[test]
fn a_422_with_no_body_still_says_it_was_refused() {
    let rec = std::sync::Arc::new(Recorder::default());
    *rec.reply.lock().unwrap() = vec![(422, String::new())];
    let m = mgr(rec);
    let err = m.brief_create(None, "something vague", &BTreeMap::new()).unwrap_err().to_string();
    assert!(err.starts_with("BRIEF_REFUSED: "), "{err}");
}

#[test]
fn a_checked_brief_returns_the_markdown() {
    let rec = std::sync::Arc::new(Recorder::default());
    *rec.reply.lock().unwrap() = vec![(200, r##"{"ok":true,"markdown":"# Brief\n"}"##.into())];
    let m = mgr(rec.clone());
    let out = m.brief_check(&brief()).unwrap();
    assert!(out.ok);
    assert_eq!(out.markdown, "# Brief\n");
    let posts = rec.posts.lock().unwrap();
    assert!(posts[0].0.ends_with("/briefs/check"));
    let body: serde_json::Value = serde_json::from_str(&posts[0].1).unwrap();
    assert_eq!(body["brief"]["workflow_available"], false, "the sidecar's snake_case wire shape");
    assert_eq!(body["brief"]["constraints"][0]["from_default"], true);
}

#[test]
fn other_failures_are_not_mislabelled_as_refusals() {
    let rec = std::sync::Arc::new(Recorder::default());
    *rec.reply.lock().unwrap() = vec![(401, r#"{"error":"unauthorized"}"#.into())];
    let m = mgr(rec);
    let err = m.tracks().unwrap_err().to_string();
    assert!(err.contains("401") && !err.contains("BRIEF_REFUSED"), "{err}");
}

#[test]
fn oversized_or_malformed_inputs_never_reach_the_sidecar() {
    let rec = std::sync::Arc::new(Recorder::default());
    let m = mgr(rec.clone());
    let none = BTreeMap::new();
    assert!(m.brief_create(Some("../x"), "goal", &none).is_err(), "track id goes in a JSON body but is still bounded");
    assert!(m.brief_create(None, "   ", &none).is_err(), "blank goal");
    assert!(m.brief_create(None, &"g".repeat(2001), &none).is_err(), "goal over 2000 chars");
    let mut long = BTreeMap::new();
    long.insert("lang".to_string(), "x".repeat(501));
    assert!(m.brief_create(Some("code"), "goal", &long).is_err(), "answer over 500 chars");
    let many: BTreeMap<String, String> = (0..17).map(|i| (format!("q{i}"), "a".to_string())).collect();
    assert!(m.brief_create(Some("code"), "goal", &many).is_err(), "more answers than any track asks");
    let mut bad_key = BTreeMap::new();
    bad_key.insert("a b".to_string(), "x".to_string());
    assert!(m.brief_create(Some("code"), "goal", &bad_key).is_err(), "question ids are slugs");
    let mut b = brief();
    b.skills = (0..33).map(|i| format!("s{i}")).collect();
    assert!(m.brief_check(&b).is_err(), "too many skills");
    let mut b = brief();
    b.persona = "p".repeat(201);
    assert!(m.brief_check(&b).is_err(), "persona too long");
    let mut b = brief();
    b.track = "Code Track".into();
    assert!(m.brief_check(&b).is_err(), "track id is a slug");
    assert!(rec.posts.lock().unwrap().is_empty(), "nothing reached the sidecar");
}

#[test]
fn not_running_fails_closed() {
    let dir = std::env::temp_dir().join(format!("hbrief-nr-{}", std::process::id()));
    let m = HermesManager::new(dir.join("bin"), dir.join("t"), dir.join("c"));
    assert!(matches!(m.tracks(), Err(HermesError::NotRunning)));
}

/// The production transport must keep a refusal's body (the sidecar's `{error}` reason); ureq's
/// default turns any non-2xx into a bare status and the card could only say "refused".
#[test]
fn the_ureq_transport_keeps_the_body_of_a_refusal() {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut sock, _) = listener.accept().unwrap();
        let mut buf = [0u8; 4096];
        let _ = sock.read(&mut buf).unwrap();
        let body = r#"{"error":"no track fits that goal; pick one from /tracks"}"#;
        let resp = format!(
            "HTTP/1.1 422 Unprocessable Entity\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        sock.write_all(resp.as_bytes()).unwrap();
    });
    let resp = UreqControl.post(&format!("http://{addr}/briefs"), "tok", "{}").unwrap();
    server.join().unwrap();
    assert_eq!(resp.status, 422);
    assert!(resp.body.contains("no track fits"), "{:?}", resp.body);
}
