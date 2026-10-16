// HUP-S2.9 — core's side of undo for agent file changes: the sidecar's `/checkpoints` routes are
// called with the session bearer, ids are validated before they reach a URL, and every refusal the
// sidecar gives (a conflict, a pruned step, undo not enabled) comes back as an honest outcome the UI
// shows as is, never as success.

use super::super::*;
use super::*;
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
        let (status, body) = self.0.reply.lock().unwrap().pop().unwrap_or((200, "{}".into()));
        Ok(ControlResp { status, body })
    }
    fn post(&self, url: &str, _b: &str, body: &str) -> std::result::Result<ControlResp, HermesError> {
        self.0.posts.lock().unwrap().push((url.to_string(), body.to_string()));
        let (status, body) = self.0.reply.lock().unwrap().pop().unwrap_or((200, "{}".into()));
        Ok(ControlResp { status, body })
    }
}

fn mgr(rec: std::sync::Arc<Recorder>) -> HermesManager {
    let dir = std::env::temp_dir().join(format!("hundo-{}-{:?}", std::process::id(), std::thread::current().id()));
    let _ = std::fs::create_dir_all(&dir);
    let m = HermesManager::new(dir.join("bin"), dir.join("t"), dir.join("c")).with_control(Box::new(RecControl(rec)));
    m.set_token_for_test("deadbeef");
    m
}

fn with_reply(status: u16, body: &str) -> (std::sync::Arc<Recorder>, HermesManager) {
    let rec = std::sync::Arc::new(Recorder::default());
    rec.reply.lock().unwrap().push((status, body.to_string()));
    let m = mgr(rec.clone());
    (rec, m)
}

#[test]
fn listing_reads_the_sessions_steps_from_the_sidecar() {
    let (rec, m) = with_reply(
        200,
        r#"{"session":"s3-ab","steps":[{"seq":2,"status":"committed","paths":["src/a.rs"],"root":"/w/proj"},{"seq":1,"status":"undone","paths":["b.md","c.md"],"root":"/w/proj"}]}"#,
    );
    let list = m.checkpoints_list("s3-ab").unwrap();
    assert_eq!(rec.gets.lock().unwrap()[0], format!("http://{HERMES_CONTROL_ADDR}/checkpoints/s3-ab"));
    assert!(list.enabled);
    assert_eq!(list.session, "s3-ab");
    assert_eq!(list.steps.len(), 2);
    assert_eq!(list.steps[0].seq, 2);
    assert_eq!(list.steps[0].status, "committed");
    assert_eq!(list.steps[0].paths, vec!["src/a.rs".to_string()]);
    assert_eq!(list.steps[1].root, "/w/proj");
}

#[test]
fn listing_says_honestly_when_undo_is_not_enabled_or_not_supported() {
    let (_rec, m) = with_reply(503, r#"{"error":"undo checkpoints are not enabled in this agent sidecar","kind":"disabled"}"#);
    let list = m.checkpoints_list("s3-ab").unwrap();
    assert!(!list.enabled);
    assert!(list.steps.is_empty());
    assert!(list.note.as_deref().unwrap_or("").contains("not enabled"), "{:?}", list.note);

    // An older sidecar has no such route: 404 with no `kind`.
    let (_rec, m) = with_reply(404, "");
    let list = m.checkpoints_list("s3-ab").unwrap();
    assert!(!list.enabled);
    assert!(list.note.as_deref().unwrap_or("").contains("does not support undo"), "{:?}", list.note);
}

#[test]
fn undoing_a_step_posts_to_its_route_and_reports_what_was_restored() {
    let (rec, m) = with_reply(200, r#"{"undone":[4],"restored":["src/a.rs"],"prunedThrough":null}"#);
    let out = m.undo_step("s3-ab", 4).unwrap();
    assert_eq!(rec.posts.lock().unwrap()[0].0, format!("http://{HERMES_CONTROL_ADDR}/checkpoints/s3-ab/steps/4/undo"));
    assert!(out.ok);
    assert_eq!(out.undone, vec![4]);
    assert_eq!(out.restored, vec!["src/a.rs".to_string()]);
    assert!(out.reason.is_none());
}

#[test]
fn a_conflict_is_an_honest_refusal_with_every_changed_path() {
    let body = r#"{"error":"undo refused, nothing was changed: 1 path(s) changed since the step (step 4 src/a.rs: now file sha256:0123456789ab)","kind":"conflict","conflicts":[{"seq":4,"path":"src/a.rs","found":"file sha256:0123456789ab"}]}"#;
    let (_rec, m) = with_reply(409, body);
    let out = m.undo_step("s3-ab", 4).unwrap();
    assert!(!out.ok);
    assert_eq!(out.kind.as_deref(), Some("conflict"));
    assert!(out.reason.as_deref().unwrap().contains("nothing was changed"));
    assert_eq!(out.conflicts.len(), 1);
    assert_eq!(out.conflicts[0].path, "src/a.rs");
    assert_eq!(out.conflicts[0].seq, 4);
    assert!(out.undone.is_empty());
}

#[test]
fn other_refusals_keep_their_kind_and_reason() {
    for (status, kind) in [(410, "pruned"), (409, "already_undone"), (409, "busy"), (404, "not_found"), (503, "disabled")] {
        let body = format!(r#"{{"error":"why {kind}","kind":"{kind}"}}"#);
        let (_rec, m) = with_reply(status, &body);
        let out = m.undo_step("s3-ab", 1).unwrap();
        assert!(!out.ok, "{kind}");
        assert_eq!(out.kind.as_deref(), Some(kind));
        assert_eq!(out.reason.as_deref(), Some(format!("why {kind}").as_str()));
    }
}

#[test]
fn an_unexpected_failure_is_an_error_not_a_refusal() {
    let (_rec, m) = with_reply(500, "boom");
    assert!(m.undo_step("s3-ab", 1).is_err());
    let (_rec, m) = with_reply(200, "not json");
    assert!(m.undo_step("s3-ab", 1).is_err());
}

#[test]
fn undoing_a_session_posts_to_the_session_route() {
    let (rec, m) = with_reply(200, r#"{"undone":[3,2,1],"restored":["a","b"],"prunedThrough":null}"#);
    let out = m.undo_session("s3-ab").unwrap();
    assert_eq!(rec.posts.lock().unwrap()[0].0, format!("http://{HERMES_CONTROL_ADDR}/checkpoints/s3-ab/undo"));
    assert!(out.ok);
    assert_eq!(out.undone, vec![3, 2, 1]);
}

#[test]
fn bad_ids_and_steps_never_reach_the_sidecar() {
    let rec = std::sync::Arc::new(Recorder::default());
    let m = mgr(rec.clone());
    assert!(m.checkpoints_list("../x").is_err());
    assert!(m.undo_step("s3 ab", 1).is_err());
    assert!(m.undo_step("s3-ab", 0).is_err());
    assert!(m.undo_session("s3-ab/../../stop").is_err());
    assert!(rec.gets.lock().unwrap().is_empty());
    assert!(rec.posts.lock().unwrap().is_empty());
}

#[test]
fn undo_needs_a_running_sidecar() {
    let dir = std::env::temp_dir().join(format!("hundo-nr-{}", std::process::id()));
    let m = HermesManager::new(dir.join("bin"), dir.join("t"), dir.join("c"));
    assert!(matches!(m.checkpoints_list("s1-a"), Err(HermesError::NotRunning)));
    assert!(matches!(m.undo_step("s1-a", 1), Err(HermesError::NotRunning)));
}

#[test]
fn the_child_gets_the_checkpoint_dir_but_never_the_file_tools_switch() {
    let dir = std::env::temp_dir().join(format!("hundo-env-{}", std::process::id()));
    let m = HermesManager::new(dir.join("bin"), dir.join("t"), dir.join("c"));
    let env: std::collections::BTreeMap<String, String> = m.spec_env_for_test().into_iter().collect();
    assert!(!env.contains_key(HERMES_CHECKPOINTS_ENV), "no store unless configured");
    let store = dir.join("hermes").join("checkpoints");
    let m = m.with_checkpoints_dir(store.clone());
    let env: std::collections::BTreeMap<String, String> = m.spec_env_for_test().into_iter().collect();
    assert_eq!(env.get(HERMES_CHECKPOINTS_ENV).map(String::as_str), Some(store.to_string_lossy().as_ref()));
    // The file tools stay off by default: core sets neither switch nor a grants file (no Grants UI
    // yet, pending owner sign-off on enabling agent writes).
    assert!(!env.contains_key("CITRATE_HERMES_FILES"));
    assert!(!env.contains_key("CITRATE_HERMES_GRANTS"));
}

// HUP-S5.4: one step's diff for the Code and diff pop-out.
#[test]
fn a_step_diff_is_read_from_its_route_and_passed_on_as_is() {
    let (rec, m) = with_reply(
        200,
        r#"{"session":"s3-ab","seq":2,"status":"committed","files":[
            {"path":"src/a.rs","before":{"kind":"text","text":"fn a() {}\n"},"after":{"kind":"text","text":"fn a() { 1 }\n"}},
            {"path":"img.png","before":{"kind":"absent"},"after":{"kind":"binary","size":12}},
            {"path":"big.log","before":{"kind":"too_large","size":900000},"after":{"kind":"unavailable","reason":"the file changed after this step (now absent)"}}]}"#,
    );
    let d = m.checkpoint_diff("s3-ab", 2).unwrap();
    assert_eq!(
        rec.gets.lock().unwrap()[0],
        format!("http://{HERMES_CONTROL_ADDR}/checkpoints/s3-ab/steps/2/diff")
    );
    assert!(d.ok);
    assert_eq!(d.status, "committed");
    assert_eq!(d.files.len(), 3);
    assert_eq!(
        d.files[0].before,
        DiffSide::Text {
            text: "fn a() {}\n".into()
        }
    );
    assert_eq!(d.files[1].after, DiffSide::Binary { size: 12 });
    assert_eq!(d.files[2].before, DiffSide::TooLarge { size: 900_000 });
    let json = serde_json::to_value(&d).unwrap();
    assert_eq!(json["files"][2]["after"]["kind"], "unavailable");
    assert_eq!(json["files"][0]["before"]["kind"], "text");
}

#[test]
fn a_step_diff_refusal_is_an_honest_outcome_and_bad_input_reaches_no_url() {
    let (_rec, m) = with_reply(410, r#"{"error":"step 1 of s3-ab was pruned","kind":"pruned"}"#);
    let d = m.checkpoint_diff("s3-ab", 1).unwrap();
    assert!(!d.ok);
    assert_eq!(d.kind.as_deref(), Some("pruned"));
    assert!(d.reason.as_deref().unwrap_or("").contains("pruned"));
    assert!(d.files.is_empty());
    let (_rec, m) = with_reply(404, "");
    let d = m.checkpoint_diff("s3-ab", 1).unwrap();
    assert_eq!(d.kind.as_deref(), Some("unsupported"));
    assert!(d.reason.as_deref().unwrap_or("").contains("cannot show diffs"));
    let rec = std::sync::Arc::new(Recorder::default());
    let m = mgr(rec.clone());
    assert!(m.checkpoint_diff("s3-ab", 0).is_err());
    assert!(m.checkpoint_diff("../x", 1).is_err());
    assert!(rec.gets.lock().unwrap().is_empty(), "nothing was called");
}

#[test]
fn a_diff_for_another_step_is_refused() {
    let (_rec, m) = with_reply(
        200,
        r#"{"session":"s3-ab","seq":9,"status":"committed","files":[]}"#,
    );
    assert!(m.checkpoint_diff("s3-ab", 2).is_err());
}

#[test]
fn a_step_diff_passes_on_at_most_the_file_cap() {
    let files: Vec<String> = (0..MAX_DIFF_FILES + 5)
        .map(|i| format!(r#"{{"path":"f{i}.txt","before":{{"kind":"absent"}},"after":{{"kind":"text","text":"x"}}}}"#))
        .collect();
    let body = format!(
        r#"{{"session":"s3-ab","seq":4,"status":"committed","files":[{}]}}"#,
        files.join(",")
    );
    let (_rec, m) = with_reply(200, &body);
    let d = m.checkpoint_diff("s3-ab", 4).unwrap();
    assert!(d.ok);
    assert_eq!(d.files.len(), MAX_DIFF_FILES);
    assert_eq!(d.files[0].path, "f0.txt");
}
