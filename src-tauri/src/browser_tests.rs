// HUP-S5.1 + S5.6 — citrate-core's side of Hermes's browser. Every member control is checked
// here before it reaches the sidecar (attach needs consent and an unprivileged loopback port,
// origins must be http(s), action ids are the sidecar's shape), the sidecar's refusal reasons
// reach the member verbatim, and an older sidecar without the browser routes reads as "off".

use super::*;
use crate::hermes::{ControlResp, HermesControl, HermesError, HermesManager};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct Recorder {
    posts: Mutex<Vec<(String, String)>>,
    gets: Mutex<Vec<String>>,
    reply: Mutex<Vec<(u16, String)>>,
}
struct RecControl(Arc<Recorder>);
impl HermesControl for RecControl {
    fn get(&self, url: &str, _b: &str) -> std::result::Result<ControlResp, HermesError> {
        self.0.gets.lock().unwrap().push(url.to_string());
        let (status, body) = self
            .0
            .reply
            .lock()
            .unwrap()
            .pop()
            .unwrap_or((200, "{}".into()));
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

fn mgr(rec: Arc<Recorder>, running: bool) -> HermesManager {
    let dir = std::env::temp_dir().join(format!(
        "hbrowser-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::create_dir_all(&dir);
    let m = HermesManager::new(dir.join("bin"), dir.join("t"), dir.join("c"))
        .with_control(Box::new(RecControl(rec)));
    if running {
        m.set_token_for_test("deadbeef");
    }
    m
}

fn reply(rec: &Recorder, status: u16, body: &str) {
    rec.reply.lock().unwrap().push((status, body.to_string()));
}

#[test]
fn status_passes_the_sidecars_view_through() {
    let rec = Arc::new(Recorder::default());
    reply(
        &rec,
        200,
        r#"{"enabled":true,"mode":"managed","chromium":{"state":"system","path":"/x"}}"#,
    );
    let v = browser_status(&mgr(rec.clone(), true)).unwrap();
    assert_eq!(v["enabled"], true);
    assert_eq!(v["mode"], "managed");
    assert!(rec.gets.lock().unwrap()[0].ends_with("/browser/status"));
}

#[test]
fn an_older_sidecar_or_a_stopped_one_reads_as_off() {
    let rec = Arc::new(Recorder::default());
    reply(&rec, 404, "");
    let v = browser_status(&mgr(rec, true)).unwrap();
    assert_eq!(v["enabled"], false);

    let rec = Arc::new(Recorder::default());
    let v = browser_status(&mgr(rec.clone(), false)).unwrap();
    assert_eq!(v["enabled"], false);
    assert_eq!(v["running"], false);
    assert!(
        rec.gets.lock().unwrap().is_empty(),
        "nothing is contacted when Hermes is not running"
    );
}

#[test]
fn frames_are_none_until_there_is_something_newer() {
    let rec = Arc::new(Recorder::default());
    reply(&rec, 204, "");
    assert!(browser_frame(&mgr(rec.clone(), true), 7).unwrap().is_none());
    assert!(rec.gets.lock().unwrap()[0].ends_with("/browser/frame?after=7"));

    let rec = Arc::new(Recorder::default());
    reply(
        &rec,
        200,
        r#"{"version":8,"mime":"image/jpeg","data":"/9j/","withheld":false}"#,
    );
    let f = browser_frame(&mgr(rec, true), 7).unwrap().expect("a frame");
    assert_eq!(f["version"], 8);
}

#[test]
fn attach_needs_consent_and_an_unprivileged_port_before_anything_is_sent() {
    let rec = Arc::new(Recorder::default());
    let m = mgr(rec.clone(), true);
    let e = browser_attach(&m, 9222, false).unwrap_err();
    assert!(e.contains("consent"), "{e}");
    let e = browser_attach(&m, 80, true).unwrap_err();
    assert!(e.contains("1024"), "{e}");
    assert!(rec.posts.lock().unwrap().is_empty());

    browser_attach(&m, 9222, true).unwrap();
    let posts = rec.posts.lock().unwrap();
    assert!(posts[0].0.ends_with("/browser/attach"));
    let body: serde_json::Value = serde_json::from_str(&posts[0].1).unwrap();
    assert_eq!(body, serde_json::json!({"port": 9222, "consent": true}));
}

#[test]
fn origins_must_be_web_origins_and_the_sidecars_reason_is_shown() {
    let rec = Arc::new(Recorder::default());
    let m = mgr(rec.clone(), true);
    for bad in [
        "file:///etc",
        "javascript:alert(1)",
        "",
        "chrome://settings",
        "not a url",
    ] {
        assert!(browser_origin(&m, bad, true, false).is_err(), "{bad}");
    }
    let long = format!("https://example.com/{}", "a".repeat(3000));
    assert!(browser_origin(&m, &long, true, false).is_err());
    assert!(rec.posts.lock().unwrap().is_empty());

    reply(
        &rec,
        200,
        r#"{"ok":true,"origin":"https://docs.example.org"}"#,
    );
    let shown = browser_origin(&m, "https://docs.example.org/x", true, false).unwrap();
    assert_eq!(shown, "https://docs.example.org");
    let body: serde_json::Value = serde_json::from_str(&rec.posts.lock().unwrap()[0].1).unwrap();
    assert_eq!(
        body,
        serde_json::json!({"origin": "https://docs.example.org/x", "allow": true, "includeSensitive": false})
    );

    reply(
        &rec,
        400,
        r#"{"error":"https://www.chase.com is in the excluded category \"Banking, payments and exchanges\""}"#,
    );
    let e = browser_origin(&m, "https://www.chase.com", true, false).unwrap_err();
    assert!(e.contains("excluded category"), "{e}");
}

#[test]
fn decisions_name_a_waiting_action_by_its_id() {
    let rec = Arc::new(Recorder::default());
    let m = mgr(rec.clone(), true);
    for bad in ["", "x1", "b", "b1;", "../b1", "b12345678901234567890"] {
        assert!(browser_decide(&m, bad, true).is_err(), "{bad}");
    }
    assert!(rec.posts.lock().unwrap().is_empty());
    browser_decide(&m, "b12", false).unwrap();
    let posts = rec.posts.lock().unwrap();
    assert!(posts[0].0.ends_with("/browser/actions/decide"));
    let body: serde_json::Value = serde_json::from_str(&posts[0].1).unwrap();
    assert_eq!(body, serde_json::json!({"id": "b12", "allow": false}));
}

#[test]
fn stop_resume_and_detach_post_to_their_routes() {
    let rec = Arc::new(Recorder::default());
    let m = mgr(rec.clone(), true);
    browser_simple(&m, BrowserControl::Stop).unwrap();
    browser_simple(&m, BrowserControl::Resume).unwrap();
    browser_simple(&m, BrowserControl::Detach).unwrap();
    let paths: Vec<String> = rec
        .posts
        .lock()
        .unwrap()
        .iter()
        .map(|(u, _)| u.rsplit('/').next().unwrap_or_default().to_string())
        .collect();
    assert_eq!(paths, ["stop", "resume", "detach"]);
}

#[test]
fn controls_fail_honestly_when_hermes_is_not_running() {
    let rec = Arc::new(Recorder::default());
    let m = mgr(rec.clone(), false);
    let e = browser_simple(&m, BrowserControl::Stop).unwrap_err();
    assert!(e.to_lowercase().contains("not running"), "{e}");
    assert!(rec.posts.lock().unwrap().is_empty());
}

#[test]
fn a_sidecar_error_is_reported_with_its_reason() {
    let rec = Arc::new(Recorder::default());
    let m = mgr(rec.clone(), true);
    reply(
        &rec,
        502,
        r#"{"error":"no Chrome is listening for remote debugging on 127.0.0.1:9222"}"#,
    );
    let e = browser_attach(&m, 9222, true).unwrap_err();
    assert!(e.contains("no Chrome is listening"), "{e}");
    reply(&rec, 404, r#"{"error":"the browser is off"}"#);
    let e = browser_simple(&m, BrowserControl::Stop).unwrap_err();
    assert!(e.contains("the browser is off"), "{e}");
}
