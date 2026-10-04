// HUP-S9.4 (n5) — running the eval gate's two arms inside the app. Written red-first (the module
// body was absent). The llama-server here is a local fixture (std TcpListener on 127.0.0.1) that
// records the request bodies; the real-server run is the ignored test at the bottom.

use super::*;
use rand::RngCore;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::Arc;

fn tmpdir(tag: &str) -> PathBuf {
    let mut d = std::env::temp_dir();
    let mut r = [0u8; 8];
    rand::thread_rng().fill_bytes(&mut r);
    d.push(format!("n5-fl-eval-{tag}-{}", hex::encode(r)));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn sha_hex(b: &[u8]) -> String {
    hex::encode(<sha2::Sha256 as sha2::Digest>::digest(b))
}

const NOW: u64 = 1_790_000_000_000;
const ADAPTER: &[u8] = b"GGUF\x03\x00\x00\x00candidate";

fn card(model: &str, n: u64, rate: f64, adapter: Option<&str>) -> String {
    let mut v = serde_json::json!({
        "model": model,
        "datasetVersion": "toolcall-v1+injection-v1",
        "n": n,
        "validToolCallRate": rate,
        "correctToolRate": rate,
        "argsOkRate": rate,
        "injectionResistRate": 1.0,
        "failures": [],
        "failureReasons": {},
        "startedAt": "2026-10-01T00:00:00Z",
        "finishedAt": "2026-10-01T00:05:00Z",
        "scoring": "deterministic"
    });
    if let Some(a) = adapter {
        v["adapterSha256"] = serde_json::json!(a);
    }
    v.to_string()
}

#[test]
fn staging_copies_the_verified_bytes_under_their_hash() {
    let d = tmpdir("stage");
    let src = d.join("cand.gguf");
    std::fs::write(&src, ADAPTER).unwrap();
    let h = sha_hex(ADAPTER);
    let eval_dir = d.join("eval");
    let copy = stage_candidate(&src, &h, &eval_dir).unwrap();
    assert_eq!(copy, eval_dir.join(format!("{h}.gguf")));
    assert_eq!(std::fs::read(&copy).unwrap(), ADAPTER);
    // wrong hash, not GGUF, bad hex
    assert!(stage_candidate(&src, &"0".repeat(64), &eval_dir).unwrap_err().contains("does not match"));
    std::fs::write(d.join("x.gguf"), b"NOPE").unwrap();
    assert!(stage_candidate(&d.join("x.gguf"), &sha_hex(b"NOPE"), &eval_dir).unwrap_err().contains("GGUF"));
    assert!(stage_candidate(&src, "xyz", &eval_dir).is_err());
}

#[test]
fn the_request_body_sets_the_adapter_scale_per_arm() {
    let msgs = r#"[{"role":"user","content":"hi"}]"#;
    let tools = r#"[{"type":"function","function":{"name":"x","parameters":{}}}]"#;
    let b = completion_body("m", msgs, tools, Arm::Base).unwrap();
    assert_eq!(b["lora"], serde_json::json!([{ "id": 0, "scale": 0.0 }]));
    assert_eq!(b["temperature"], serde_json::json!(0));
    assert_eq!(b["tool_choice"], serde_json::json!("auto"));
    assert_eq!(b["model"], serde_json::json!("m"));
    assert_eq!(b["messages"][0]["content"], serde_json::json!("hi"));
    let c = completion_body("m", msgs, tools, Arm::Candidate).unwrap();
    assert_eq!(c["lora"], serde_json::json!([{ "id": 0, "scale": 1.0 }]));
    // malformed input is refused, never sent
    assert!(completion_body("m", "{", tools, Arm::Base).is_err());
    assert!(completion_body("m", msgs, "{}", Arm::Base).is_err());
}

#[test]
fn the_server_must_report_exactly_the_candidate_at_scale_zero() {
    let h = sha_hex(ADAPTER);
    let file = format!("{h}.gguf");
    let ok = format!(r#"[{{"id":0,"path":"{file}","scale":0.0,"task_name":"","prompt_prefix":""}}]"#);
    assert!(check_lora_adapters(&ok, &file).is_ok());
    let applied = format!(r#"[{{"id":0,"path":"{file}","scale":1.0}}]"#);
    assert!(check_lora_adapters(&applied, &file).unwrap_err().contains("scale"));
    let other = r#"[{"id":0,"path":"other.gguf","scale":0.0}]"#;
    assert!(check_lora_adapters(other, &file).is_err());
    let two = format!(r#"[{{"id":0,"path":"{file}","scale":0.0}},{{"id":1,"path":"b.gguf","scale":0.0}}]"#);
    assert!(check_lora_adapters(&two, &file).is_err());
    assert!(check_lora_adapters("[]", &file).is_err());
    assert!(check_lora_adapters("nope", &file).is_err());
}

fn session(fe: &FlEval, sha: &str) -> EvalSessionInfo {
    fe.begin(sha, "m", PathBuf::from("/x/eval").join(format!("{sha}.gguf")), NOW).unwrap()
}

#[test]
fn one_run_at_a_time_and_calls_are_counted_per_arm() {
    let fe = FlEval::default();
    let h = sha_hex(ADAPTER);
    let s = session(&fe, &h);
    assert_eq!(s.model, "m");
    assert!(fe.begin(&h, "m", PathBuf::from("/y"), NOW).unwrap_err().contains("in progress"));
    fe.note_call(&s.session_id, Arm::Base).unwrap();
    fe.note_call(&s.session_id, Arm::Candidate).unwrap();
    fe.note_call(&s.session_id, Arm::Candidate).unwrap();
    let cur = fe.current().unwrap();
    assert_eq!((cur.base_calls, cur.candidate_calls), (1, 2));
    assert!(fe.note_call("not-the-id", Arm::Base).is_err());
    assert!(fe.end("not-the-id").is_err());
    fe.end(&s.session_id).unwrap();
    assert!(fe.current().is_none());
}

#[test]
fn finishing_decides_from_the_runs_core_counted_and_binds_the_adapter() {
    let fe = FlEval::default();
    let h = sha_hex(ADAPTER);
    let s = session(&fe, &h);
    for _ in 0..3 {
        fe.note_call(&s.session_id, Arm::Base).unwrap();
        fe.note_call(&s.session_id, Arm::Candidate).unwrap();
    }
    // The candidate scorecard is stamped by core (the runner does not have to).
    let rec = fe.finish(&s.session_id, &card("m", 3, 0.5, None), &card("m", 3, 0.9, None), "m.gguf", NOW).unwrap();
    assert_eq!(rec.adapter_sha256, h);
    assert_eq!(rec.decision.verdict, crate::fl_rounds::GateVerdict::Accept);
    assert_eq!(rec.base_model, "m");
    assert!(rec.adapter_path.ends_with(&format!("{h}.gguf")));
    assert!(fe.current().is_none(), "finishing ends the run");
}

#[test]
fn finishing_refuses_scorecards_core_did_not_see_run() {
    let h = sha_hex(ADAPTER);
    let run = |base_calls: u64, cand_calls: u64, base: String, cand: String, served: &str| {
        let fe = FlEval::default();
        let s = session(&fe, &h);
        for _ in 0..base_calls {
            fe.note_call(&s.session_id, Arm::Base).unwrap();
        }
        for _ in 0..cand_calls {
            fe.note_call(&s.session_id, Arm::Candidate).unwrap();
        }
        let r = fe.finish(&s.session_id, &base, &cand, served, NOW);
        // A refused finish leaves the run open so it can be ended cleanly.
        if r.is_err() {
            assert!(fe.current().is_some());
        }
        r
    };
    // fewer calls than the scorecard claims
    let e = run(2, 3, card("m", 3, 0.5, None), card("m", 3, 0.9, None), "m.gguf").unwrap_err();
    assert!(e.contains("base"), "{e}");
    // more candidate calls than the scorecard (a best-of-several run)
    let e = run(3, 6, card("m", 3, 0.5, None), card("m", 3, 0.9, None), "m.gguf").unwrap_err();
    assert!(e.contains("candidate"), "{e}");
    // a base run stamped with an adapter
    let e = run(3, 3, card("m", 3, 0.5, Some(&h)), card("m", 3, 0.9, None), "m.gguf").unwrap_err();
    assert!(e.contains("base"), "{e}");
    // a candidate stamped with another adapter
    let e = run(3, 3, card("m", 3, 0.5, None), card("m", 3, 0.9, Some(&"2".repeat(64))), "m.gguf").unwrap_err();
    assert!(e.contains("adapter"), "{e}");
    // another model's scorecards, or the served model changed during the run
    let e = run(3, 3, card("x", 3, 0.5, None), card("x", 3, 0.9, None), "m.gguf").unwrap_err();
    assert!(e.contains("model"), "{e}");
    let e = run(3, 3, card("m", 3, 0.5, None), card("m", 3, 0.9, None), "other.gguf").unwrap_err();
    assert!(e.contains("model"), "{e}");
}

#[test]
fn a_no_better_candidate_is_rejected_and_recorded_as_such() {
    let fe = FlEval::default();
    let h = sha_hex(ADAPTER);
    let s = session(&fe, &h);
    for _ in 0..3 {
        fe.note_call(&s.session_id, Arm::Base).unwrap();
        fe.note_call(&s.session_id, Arm::Candidate).unwrap();
    }
    let rec = fe.finish(&s.session_id, &card("m", 3, 0.5, None), &card("m", 3, 0.5, None), "m.gguf", NOW).unwrap();
    assert_eq!(rec.decision.verdict, crate::fl_rounds::GateVerdict::Reject);
}

/// A loopback stand-in for llama-server's chat route: checks the bearer and returns a fixed
/// assistant message, recording each body.
fn chat_fixture(key: String) -> (u16, Arc<std::sync::Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let s2 = seen.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut s) = stream else { continue };
            let mut buf = vec![0u8; 65536];
            let mut got = Vec::new();
            // read headers + body (content-length)
            loop {
                let n = s.read(&mut buf).unwrap_or(0);
                if n == 0 {
                    break;
                }
                got.extend_from_slice(&buf[..n]);
                let text = String::from_utf8_lossy(&got).to_string();
                if let Some(i) = text.find("\r\n\r\n") {
                    let cl = text
                        .lines()
                        .find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap_or(0)))
                        .unwrap_or(0);
                    if got.len() >= i + 4 + cl {
                        break;
                    }
                }
            }
            let text = String::from_utf8_lossy(&got).to_string();
            let authed = text.lines().any(|l| l.eq_ignore_ascii_case(&format!("authorization: Bearer {key}")));
            let body = text.split("\r\n\r\n").nth(1).unwrap_or("").to_string();
            s2.lock().unwrap().push(body);
            let (code, out) = if authed {
                (200, r#"{"choices":[{"message":{"role":"assistant","content":"ok","tool_calls":[]}}]}"#)
            } else {
                (401, r#"{"error":"unauthorized"}"#)
            };
            let resp = format!("HTTP/1.1 {code} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{out}", out.len());
            let _ = s.write_all(resp.as_bytes());
        }
    });
    (port, seen)
}

#[test]
fn the_proxy_sends_the_arm_scale_with_the_session_key_and_returns_the_message() {
    let mut k = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut k);
    let key = hex::encode(k);
    let (port, seen) = chat_fixture(key.clone());
    let body = completion_body("m", r#"[{"role":"user","content":"hi"}]"#, "[]", Arm::Candidate).unwrap();
    let msg = post_completion(port, &key, &body).unwrap();
    let v: serde_json::Value = serde_json::from_str(&msg).unwrap();
    assert_eq!(v["content"], serde_json::json!("ok"));
    let sent: serde_json::Value = serde_json::from_str(&seen.lock().unwrap()[0]).unwrap();
    assert_eq!(sent["lora"][0]["scale"], serde_json::json!(1.0));
    // a wrong key is an error, not a model answer
    assert!(post_completion(port, "wrong", &body).unwrap_err().contains("401"));
}

/// Real-server check (manual): the bundled llama-server, a real base model and a real adapter.
/// `CITRATE_LLAMA_BIN=<llama-server> CITRATE_FL_BASE=<base.gguf> CITRATE_FL_ADAPTER=<adapter.gguf>
///  cargo test --lib fl_eval::tests::a_real_llama_server -- --ignored --nocapture`
#[test]
#[ignore = "needs the bundled llama-server, a base model and an adapter on disk"]
fn a_real_llama_server_serves_both_arms_and_chats_stay_on_the_base() {
    let bin = PathBuf::from(std::env::var("CITRATE_LLAMA_BIN").unwrap());
    let base = PathBuf::from(std::env::var("CITRATE_FL_BASE").unwrap());
    let adapter = PathBuf::from(std::env::var("CITRATE_FL_ADAPTER").unwrap());
    let d = tmpdir("real");
    let sha = {
        let bytes = std::fs::read(&adapter).unwrap();
        sha_hex(&bytes)
    };
    let copy = stage_candidate(&adapter, &sha, &d.join("eval")).unwrap();
    let port = 18_391;
    let m = crate::serve::LlamaServerManager::new(bin, base, d.join("crash.jsonl"), port);
    m.set_eval_lora(Some(copy.clone()));
    m.start_if_ready(true).unwrap();
    wait_healthy(port, std::time::Duration::from_secs(180)).unwrap();
    let key = m.api_key();
    let adapters = get_lora_adapters(port, &key).unwrap();
    println!("lora-adapters: {adapters}");
    check_lora_adapters(&adapters, &format!("{sha}.gguf")).unwrap();
    let msgs = r#"[{"role":"user","content":"Name three fruits."}]"#;
    let ask = |arm: Arm| {
        let b = completion_body(&m.current_model_file(), msgs, "[]", arm).unwrap();
        post_completion(port, &key, &b).unwrap()
    };
    let base_msg = ask(Arm::Base);
    let cand_msg = ask(Arm::Candidate);
    // A chat request that names no adapter (what every member chat sends) is the base arm.
    let mut chat = completion_body(&m.current_model_file(), msgs, "[]", Arm::Base).unwrap();
    chat.as_object_mut().unwrap().remove("lora");
    let chat_msg = post_completion(port, &key, &chat).unwrap();
    println!("base:      {base_msg}\ncandidate: {cand_msg}\nchat:      {chat_msg}");
    let content = |s: &str| serde_json::from_str::<serde_json::Value>(s).unwrap()["content"].clone();
    assert_eq!(content(&chat_msg), content(&base_msg), "a chat must be served by the base model alone");
    m.stop();
}

/// Real scorecards through the gate (manual): the two arms of one eval run and the adapter file.
/// `CITRATE_FL_ADAPTER=<adapter.gguf> CITRATE_FL_BASE_CARD=<base.json> CITRATE_FL_CAND_CARD=<cand.json>
///  cargo test --lib fl_eval::tests::real_scorecards -- --ignored --nocapture`
#[test]
#[ignore = "needs a real adapter and the two scorecards of a real eval run"]
fn real_scorecards_through_the_gate() {
    let adapter = std::env::var("CITRATE_FL_ADAPTER").unwrap();
    let sha = sha_hex(&std::fs::read(&adapter).unwrap());
    let req = crate::fl_rounds::AdapterGateRequest {
        adapter_path: adapter,
        expected_sha256: sha,
        base_tools_path: std::env::var("CITRATE_FL_BASE_CARD").unwrap(),
        candidate_tools_path: std::env::var("CITRATE_FL_CAND_CARD").unwrap(),
        base_qa_path: None,
        candidate_qa_path: None,
    };
    let rec = crate::fl_rounds::evaluate_adapter(&req, NOW).unwrap();
    println!("{}", serde_json::to_string_pretty(&rec).unwrap());
}
