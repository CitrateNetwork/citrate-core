// HUP-S3.3 + S3.7 (core half): personas and track workflows read from the sidecar, and a custom
// persona checked by it. Core bounds the input first; a 422 surfaces as PERSONA_REFUSED with the
// sidecar's reason.

use super::*;
use crate::hermes::{ControlResp, HermesControl, HermesError, HermesManager};
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
        let (status, body) = self
            .0
            .reply
            .lock()
            .unwrap()
            .pop()
            .unwrap_or((200, "[]".into()));
        Ok(ControlResp { status, body })
    }
    fn post(
        &self,
        url: &str,
        _b: &str,
        body: &str,
    ) -> std::result::Result<ControlResp, HermesError> {
        self.0
            .posts
            .lock()
            .unwrap()
            .push((url.to_string(), body.to_string()));
        let (status, body) = self
            .0
            .reply
            .lock()
            .unwrap()
            .pop()
            .unwrap_or((200, "{}".into()));
        Ok(ControlResp { status, body })
    }
}

fn mgr(rec: std::sync::Arc<Recorder>) -> HermesManager {
    let dir = std::env::temp_dir().join(format!(
        "hpersona-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::create_dir_all(&dir);
    let m = HermesManager::new(dir.join("bin"), dir.join("t"), dir.join("c"))
        .with_control(Box::new(RecControl(rec)));
    m.set_token_for_test("deadbeef");
    m
}

const PERSONA: &str = r#"{"id":"builder","role":"Builder","name":"Graft",
 "name_status":"placeholder, pending owner sign-off","summary":"Ships code and dApps.",
 "voice":"Direct.","tone":"Practical.","style_rules":["Lead with the change."],
 "default_track":"full-project","default_workflow":"hello-mint",
 "tool_emphasis":["forge_test"],"skills":["solidity"],"tts_voice":null,
 "prompt_fragment":"Persona: Graft\n","name_pending_sign_off":true,"custom":false}"#;

fn custom() -> CustomPersonaInput {
    CustomPersonaInput {
        id: "custom-night-owl".into(),
        name: "Night Owl".into(),
        summary: "Late-night pair programmer.".into(),
        voice: "Quiet.".into(),
        tone: "Dry.".into(),
        style_rules: vec!["Lead with the answer.".into()],
        default_track: "code".into(),
        tool_emphasis: vec![],
        skills: vec![],
        tts_voice: None,
    }
}

#[test]
fn personas_are_read_from_the_sidecar_with_their_fragment_and_pending_flag() {
    let rec = std::sync::Arc::new(Recorder::default());
    *rec.reply.lock().unwrap() = vec![(200, format!("[{PERSONA}]"))];
    let m = mgr(rec.clone());
    let ps = personas(&m).unwrap();
    assert_eq!(ps.len(), 1);
    assert_eq!(ps[0].name, "Graft");
    assert!(ps[0].name_pending_sign_off);
    assert_eq!(ps[0].default_workflow, "hello-mint");
    assert!(ps[0].prompt_fragment.starts_with("Persona: Graft"));
    assert!(rec.gets.lock().unwrap()[0].ends_with("/personas"));
}

#[test]
fn workflows_are_read_from_the_sidecar() {
    let rec = std::sync::Arc::new(Recorder::default());
    let body = r#"[{"id":"contract-build","track":"smart-contract","title":"Contract build",
      "summary":"s","is_default":true,"evidence":"tool-report","tools":["forge_test"],
      "verifier_names":["forge_test: all tests pass"],
      "steps":[{"id":"tests","instruction":"i","max_attempts":3,"verifier_names":["forge_test: all tests pass"]}]}]"#;
    *rec.reply.lock().unwrap() = vec![(200, body.into())];
    let m = mgr(rec.clone());
    let ws = workflows(&m).unwrap();
    assert_eq!(ws[0].evidence, "tool-report");
    assert_eq!(ws[0].steps[0].max_attempts, 3);
    assert!(rec.gets.lock().unwrap()[0].ends_with("/workflows"));
}

#[test]
fn a_custom_persona_is_posted_for_the_sidecar_to_check() {
    let rec = std::sync::Arc::new(Recorder::default());
    let reply = PERSONA
        .replace("\"builder\"", "\"custom-night-owl\"")
        .replace("\"custom\":false", "\"custom\":true");
    *rec.reply.lock().unwrap() = vec![(200, reply)];
    let m = mgr(rec.clone());
    let v = persona_check(&m, &custom()).unwrap();
    assert!(v.custom);
    let posts = rec.posts.lock().unwrap();
    assert!(posts[0].0.ends_with("/personas/check"));
    let sent: serde_json::Value = serde_json::from_str(&posts[0].1).unwrap();
    assert_eq!(sent["persona"]["id"], "custom-night-owl");
    assert_eq!(sent["persona"]["style_rules"][0], "Lead with the answer.");
}

#[test]
fn a_sidecar_refusal_reads_persona_refused_with_its_reason() {
    let rec = std::sync::Arc::new(Recorder::default());
    *rec.reply.lock().unwrap() = vec![(
        422,
        r#"{"error":"\"Graft\" is a shipped persona's name; pick another"}"#.into(),
    )];
    let m = mgr(rec);
    let e = persona_check(&m, &custom()).unwrap_err();
    assert!(e.starts_with("PERSONA_REFUSED: "), "{e}");
    assert!(e.contains("shipped persona"), "{e}");
}

#[test]
fn core_bounds_a_custom_persona_before_it_reaches_the_sidecar() {
    let rec = std::sync::Arc::new(Recorder::default());
    let m = mgr(rec.clone());
    let mut c = custom();
    c.id = "night-owl".into();
    assert!(
        persona_check(&m, &c).is_err(),
        "custom ids start with custom-"
    );
    let mut c = custom();
    c.style_rules = vec!["x".repeat(301)];
    assert!(persona_check(&m, &c).is_err(), "rules are bounded");
    let mut c = custom();
    c.style_rules = vec!["r".into(); 13];
    assert!(persona_check(&m, &c).is_err(), "at most 12 rules");
    let mut c = custom();
    c.name = "n".repeat(41);
    assert!(persona_check(&m, &c).is_err(), "name is bounded");
    let mut c = custom();
    c.default_track = "Not A Slug".into();
    assert!(persona_check(&m, &c).is_err());
    let mut c = custom();
    c.tts_voice = Some("bad voice; x".into());
    assert!(persona_check(&m, &c).is_err());
    assert!(
        rec.posts.lock().unwrap().is_empty(),
        "nothing out-of-bounds is sent"
    );
}

#[test]
fn not_running_is_an_honest_error() {
    let dir = std::env::temp_dir().join(format!("hpersona-nr-{}", std::process::id()));
    let m = HermesManager::new(dir.join("bin"), dir.join("t"), dir.join("c"));
    assert!(personas(&m).is_err());
}

#[test]
fn every_core_bound_refuses_before_the_sidecar_sees_it() {
    // Reviewer mutation check: each bound in `validate_custom_input` is exercised on its own.
    type Break = fn(&mut CustomPersonaInput);
    let cases: [(&str, Break); 8] = [
        ("summary", |c| c.summary = "s".repeat(201)),
        ("voice", |c| c.voice = "v".repeat(301)),
        ("tone", |c| c.tone = "t".repeat(301)),
        ("blank voice", |c| c.voice = "  ".into()),
        ("no rules", |c| c.style_rules.clear()),
        ("too many tools", |c| {
            c.tool_emphasis = vec!["forge_test".into(); 13]
        }),
        ("bad tool name", |c| {
            c.tool_emphasis = vec!["forge test".into()]
        }),
        ("too many skills", |c| {
            c.skills = vec!["solidity".into(); 25]
        }),
    ];
    for (what, brk) in cases {
        let mut c = custom();
        brk(&mut c);
        assert!(validate_custom_input(&c).is_err(), "{what} must be refused");
    }
    let mut c = custom();
    c.skills = vec!["Not A Slug".into()];
    assert!(validate_custom_input(&c).is_err(), "skills are slugs");
    assert!(
        validate_custom_input(&custom()).is_ok(),
        "the baseline is valid"
    );
}

// ---- HUP-S3.3 rest: a persona in the session body, track workflows from a session ----------

const BODY: &str = r#"{"model":"m","systemPrompt":"p","tools":[]}"#;

#[test]
fn no_persona_leaves_the_session_body_byte_for_byte_unchanged() {
    assert_eq!(with_session_persona(BODY, None, None).unwrap(), BODY);
}

#[test]
fn a_shipped_persona_id_rides_in_the_session_body() {
    let out = with_session_persona(BODY, Some("auditor"), None).unwrap();
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["persona"], "auditor");
    assert!(v.get("customPersona").is_none());
    assert_eq!(v["systemPrompt"], "p", "the rest of the body is kept");
}

#[test]
fn a_custom_persona_rides_in_the_session_body_after_core_bounds_it() {
    let out = with_session_persona(BODY, None, Some(&custom())).unwrap();
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["customPersona"]["id"], "custom-night-owl");
    assert!(v.get("persona").is_none());
    let mut bad = custom();
    bad.style_rules.clear();
    assert!(with_session_persona(BODY, None, Some(&bad)).is_err());
}

#[test]
fn a_bad_persona_id_or_both_kinds_is_refused_before_the_sidecar() {
    assert!(with_session_persona(BODY, Some("Not A Slug"), None).is_err());
    assert!(with_session_persona(BODY, Some("custom-x"), None).is_err());
    assert!(with_session_persona(BODY, Some(""), None).is_err());
    assert!(with_session_persona(BODY, Some("auditor"), Some(&custom())).is_err());
    assert!(with_session_persona("not json", Some("auditor"), None).is_err());
}

#[test]
fn a_track_workflow_is_started_by_catalog_id_in_a_session() {
    let rec = std::sync::Arc::new(Recorder::default());
    *rec.reply.lock().unwrap() = vec![(
        202,
        r#"{"run_id":"wr-3","workflow_id":"status-note","track":"project-management","evidence":"answer-shape"}"#
            .into(),
    )];
    let m = mgr(rec.clone());
    let v = track_workflow_run(&m, "s1-ab", "status-note").unwrap();
    assert_eq!(v["run_id"], "wr-3");
    let posts = rec.posts.lock().unwrap();
    assert!(
        posts[0].0.ends_with("/sessions/s1-ab/track_workflows"),
        "{}",
        posts[0].0
    );
    let sent: serde_json::Value = serde_json::from_str(&posts[0].1).unwrap();
    assert_eq!(sent, serde_json::json!({"workflow": "status-note"}));
}

#[test]
fn a_refused_track_workflow_reads_workflow_refused_with_the_reason() {
    let rec = std::sync::Arc::new(Recorder::default());
    *rec.reply.lock().unwrap() = vec![(
        422,
        r#"{"error":"this workflow needs the contract toolchain (forge_test), which is off in this app","missing_tools":["forge_test"]}"#.into(),
    )];
    let m = mgr(rec);
    let e = track_workflow_run(&m, "s1-ab", "contract-build").unwrap_err();
    assert!(e.starts_with("WORKFLOW_REFUSED: "), "{e}");
    assert!(e.contains("toolchain"), "{e}");
}

#[test]
fn bad_ids_never_reach_a_url() {
    let rec = std::sync::Arc::new(Recorder::default());
    let m = mgr(rec.clone());
    assert!(track_workflow_run(&m, "s1/../stop", "status-note").is_err());
    assert!(track_workflow_run(&m, "s1-ab", "../stop").is_err());
    assert!(track_workflow_run(&m, "s1-ab", "").is_err());
    assert!(track_workflow_run(&m, "s1-ab", "Status Note").is_err());
    assert!(rec.posts.lock().unwrap().is_empty());
}

#[test]
fn a_run_id_the_sidecar_answers_must_be_well_formed() {
    let rec = std::sync::Arc::new(Recorder::default());
    *rec.reply.lock().unwrap() = vec![(202, r#"{"run_id":"wr-1/../x"}"#.into())];
    let m = mgr(rec);
    assert!(track_workflow_run(&m, "s1-ab", "status-note").is_err());
}

#[test]
fn the_new_commands_are_in_the_main_window_acl() {
    let acl = include_str!("../permissions/main-window.toml");
    assert!(acl.contains("\"hermes_track_workflow_run\""));
}
