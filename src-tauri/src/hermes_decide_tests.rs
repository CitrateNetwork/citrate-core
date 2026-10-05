// HUP-S5.3 (core half): the decide() slot's per-backend metering as core reads it: GET
// /decide/stats through the bearer-authed control client, decoded into bounded camelCase rows for
// the Activity monitor. The fixture follows the sidecar's own report shape (DecideStatus with a
// DecisionReport inside); its numbers are illustrative, not a recorded run.

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
    let dir = std::env::temp_dir().join(format!("hdecide-{}-{:?}", std::process::id(), std::thread::current().id()));
    let _ = std::fs::create_dir_all(&dir);
    let m = HermesManager::new(dir.join("bin"), dir.join("t"), dir.join("c")).with_control(Box::new(RecControl(rec)));
    m.set_token_for_test("deadbeef");
    m
}

const REPORT: &str = r#"{"jevEnabled":false,"jevOrigins":0,"jevNonWeb":false,"logging":true,
 "report":{"schema":1,"backends":{
  "local":{"decisions":38,"errors":0,"errors_by_kind":{},"latency_ms":{"p50":812,"p95":2210,"max":3100},
           "mean_confidence":null,"egress_bytes":0,"tasks_attempted":10,"tasks_succeeded":9,"task_success_bps":9000},
  "jev":{"decisions":2,"errors":1,"errors_by_kind":{"timeout":1},"latency_ms":{"p50":140,"p95":140,"max":140},
         "mean_confidence":0.82,"egress_bytes":5120,"tasks_attempted":0,"tasks_succeeded":0,"task_success_bps":null}
 }}}"#;

#[test]
fn the_per_backend_report_is_read_from_the_sidecar() {
    let rec = std::sync::Arc::new(Rec::default());
    *rec.reply.lock().unwrap() = Some((200, REPORT.into()));
    let m = mgr(rec.clone()).decide_stats().unwrap();
    assert!(rec.gets.lock().unwrap()[0].ends_with("/decide/stats"));
    assert!(m.logging);
    assert!(!m.jev_enabled);
    assert_eq!(m.backends.len(), 2);
    let jev = &m.backends[0];
    let local = &m.backends[1];
    assert_eq!(local.backend, "local");
    assert_eq!(local.decisions, 38);
    assert_eq!(local.p50_ms, Some(812));
    assert_eq!(local.p95_ms, Some(2210));
    assert_eq!(local.tasks_attempted, 10);
    assert_eq!(local.tasks_succeeded, 9);
    assert_eq!(local.task_success_bps, Some(9000));
    assert_eq!(local.mean_confidence, None, "absent numbers stay absent");
    assert_eq!(jev.backend, "jev");
    assert_eq!(jev.errors, 1);
    assert_eq!(jev.egress_bytes, 5120);
    assert_eq!(jev.task_success_bps, None, "no task outcome recorded: no rate");
}

#[test]
fn rows_serialize_camel_case_for_the_webview() {
    let rec = std::sync::Arc::new(Rec::default());
    *rec.reply.lock().unwrap() = Some((200, REPORT.into()));
    let v = serde_json::to_value(mgr(rec).decide_stats().unwrap()).unwrap();
    assert_eq!(v["backends"][1]["tasksSucceeded"], 9);
    assert_eq!(v["backends"][1]["p95Ms"], 2210);
    assert_eq!(v["backends"][0]["egressBytes"], 5120);
    assert_eq!(v["jevEnabled"], false);
    assert!(v["backends"][1].get("tasks_succeeded").is_none());
}

#[test]
fn a_sidecar_without_the_route_is_an_honest_error() {
    let rec = std::sync::Arc::new(Rec::default());
    *rec.reply.lock().unwrap() = Some((404, String::new()));
    assert!(mgr(rec).decide_stats().is_err());
}

#[test]
fn a_bad_report_is_bounded() {
    let rows: Vec<String> = (0..40)
        .map(|i| format!(r#""b{i:02}{}":{{"decisions":1,"tasks_attempted":1,"tasks_succeeded":7,"task_success_bps":70000,"mean_confidence":null}}"#, "x".repeat(80)))
        .collect();
    let rec = std::sync::Arc::new(Rec::default());
    *rec.reply.lock().unwrap() = Some((200, format!(r#"{{"report":{{"backends":{{{}}}}}}}"#, rows.join(","))));
    let m = mgr(rec).decide_stats().unwrap();
    assert_eq!(m.backends.len(), MAX_DECIDE_BACKENDS);
    assert!(m.backends.iter().all(|b| b.backend.chars().count() <= 32));
    assert!(m.backends.iter().all(|b| b.tasks_succeeded <= b.tasks_attempted));
    assert!(m.backends.iter().all(|b| b.task_success_bps.unwrap_or(0) <= 10_000));
}

#[test]
fn the_command_is_async_registered_and_in_the_main_window_acl() {
    let lib = include_str!("lib.rs");
    let acl = include_str!("../permissions/main-window.toml");
    let src = include_str!("hermes_decide.rs");
    let invoke = include_str!("../../src/bridge/tauri/invoke.ts");
    let cmd = "hermes_decide_stats";
    assert!(lib.contains(&format!("hermes::decide::{cmd},")), "{cmd} registered");
    assert!(acl.contains(&format!("\"{cmd}\"")), "{cmd} in main-window.toml");
    assert!(src.contains(&format!("pub async fn {cmd}(")), "{cmd} is async");
    assert!(invoke.contains(&format!("{cmd}: ")), "{cmd} has an invoke timeout");
}
