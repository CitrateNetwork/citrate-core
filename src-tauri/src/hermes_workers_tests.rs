// HUP-S1.9 (core half) — the sidecar's worker processes (toolchain now, browser reserved) as core
// reads them: GET /workers through the bearer-authed control client, decoded into camelCase rows
// for the Activity monitor. A sidecar that is not running reports no workers rather than an error.

use super::super::*;
use super::*;
use std::sync::Mutex as StdMutex;

#[derive(Default)]
struct Rec {
    gets: StdMutex<Vec<String>>,
    reply: StdMutex<Option<(u16, String)>>,
}
struct RecControl(std::sync::Arc<Rec>);
impl HermesControl for RecControl {
    fn get(&self, url: &str, _b: &str) -> std::result::Result<ControlResp, HermesError> {
        self.0.gets.lock().unwrap().push(url.to_string());
        let (status, body) = self.0.reply.lock().unwrap().clone().unwrap_or((200, "{}".into()));
        Ok(ControlResp { status, body })
    }
    fn post(&self, _url: &str, _b: &str, _body: &str) -> std::result::Result<ControlResp, HermesError> {
        Ok(ControlResp { status: 200, body: "{}".into() })
    }
}

fn mgr(rec: std::sync::Arc<Rec>) -> HermesManager {
    let dir = std::env::temp_dir().join(format!("hworkers-{}-{:?}", std::process::id(), std::thread::current().id()));
    let _ = std::fs::create_dir_all(&dir);
    let m = HermesManager::new(dir.join("bin"), dir.join("t"), dir.join("c")).with_control(Box::new(RecControl(rec)));
    m.set_token_for_test("deadbeef");
    m
}

const REPORT: &str = r#"{"workers":[
  {"kind":"toolchain","state":"running","healthy":true,"pid":4242,"restarts":1,
   "last_exit":"killed by signal 9","last_error":null,"running_since_ms":1700000000000},
  {"kind":"browser","state":"not_built","detail":"the browser worker arrives with HUP-S5.1; no browser tools exist yet"}
]}"#;

#[test]
fn workers_are_read_from_the_sidecar_and_keep_their_restart_history() {
    let rec = std::sync::Arc::new(Rec::default());
    *rec.reply.lock().unwrap() = Some((200, REPORT.into()));
    let m = mgr(rec.clone());
    let w = m.workers().unwrap();
    assert!(rec.gets.lock().unwrap()[0].ends_with("/workers"));
    assert_eq!(w.len(), 2);
    assert_eq!(w[0].kind, "toolchain");
    assert_eq!(w[0].state, "running");
    assert_eq!(w[0].healthy, Some(true));
    assert_eq!(w[0].restarts, Some(1));
    assert_eq!(w[0].last_exit.as_deref(), Some("killed by signal 9"));
    assert_eq!(w[1].state, "not_built");
    assert!(w[1].detail.as_deref().unwrap().contains("HUP-S5.1"));
    assert_eq!(w[1].healthy, None, "absent fields stay absent, never invented");
}

#[test]
fn rows_serialize_camel_case_for_the_webview() {
    let rec = std::sync::Arc::new(Rec::default());
    *rec.reply.lock().unwrap() = Some((200, REPORT.into()));
    let v = serde_json::to_value(mgr(rec).workers().unwrap()).unwrap();
    assert_eq!(v[0]["lastExit"], "killed by signal 9");
    assert_eq!(v[0]["runningSinceMs"], 1_700_000_000_000u64);
    assert!(v[0].get("last_exit").is_none());
}

#[test]
fn a_sidecar_without_the_route_is_an_honest_error_not_an_empty_list() {
    let rec = std::sync::Arc::new(Rec::default());
    *rec.reply.lock().unwrap() = Some((404, String::new()));
    assert!(mgr(rec).workers().is_err());
}

#[test]
fn an_oversized_report_is_capped() {
    let rows: Vec<String> = (0..50).map(|i| format!(r#"{{"kind":"k{i}","state":"running"}}"#)).collect();
    let rec = std::sync::Arc::new(Rec::default());
    *rec.reply.lock().unwrap() = Some((200, format!(r#"{{"workers":[{}]}}"#, rows.join(","))));
    assert_eq!(mgr(rec).workers().unwrap().len(), MAX_WORKER_ROWS);
}
