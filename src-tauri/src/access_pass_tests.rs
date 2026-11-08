// First-run access check: parsing, the gate's answers, the store, and verify against local fakes
// (a closure and a loopback HTTP server, test-only). No network beyond 127.0.0.1.

use super::*;
use std::cell::RefCell;
use std::io::{Read, Write};
use std::path::PathBuf;

const NOW: u64 = 1_800_000_000;
const GATE: &str = "https://www.citrate.ai/api/access/verify";
const CODE: &str = "AbCdEfGhIjKlMnOpQrStUvWxYz0123456789-_AbCdE";

fn path(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("citrate-access-pass-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("access")).expect("dir");
    d.join("access").join("pass.json")
}

#[test]
fn the_code_constant_is_43_chars() {
    assert_eq!(CODE.len(), CODE_LEN);
}

#[test]
fn parse_accepts_a_bare_code_and_an_access_link() {
    assert_eq!(parse_input(CODE).unwrap(), CODE);
    assert_eq!(parse_input(&format!("  {CODE}\n")).unwrap(), CODE);
    assert_eq!(
        parse_input(&format!("citrate://access?code={CODE}")).unwrap(),
        CODE
    );
}

#[test]
fn parse_refuses_everything_else() {
    for bad in [
        "",
        "short",
        &format!("{CODE}x"),
        &format!("https://www.citrate.ai/download/invite?t={CODE}"),
        &format!("citrate://claim?code={CODE}"),
        &format!("citrate://access?code={CODE}&code={CODE}"),
        "citrate://access?code=",
        &"a".repeat(600),
    ] {
        assert_eq!(parse_input(bad), Err(AccessError::NotACode), "{bad}");
    }
}

#[test]
fn a_group_invite_link_gets_its_own_message_and_is_not_parsed() {
    let link = format!("citrate://invite?g=ab&t={CODE}&k=x&e=1");
    assert_eq!(parse_input(&link), Err(AccessError::GroupInviteLink));
    assert!(AccessError::GroupInviteLink
        .message()
        .contains("group invite"));
}

#[test]
fn gate_url_must_be_https_on_www_citrate_ai() {
    assert!(gate_url_ok(GATE));
    assert!(gate_url_ok("https://www.citrate.ai:443/api/access/verify"));
    for bad in [
        "http://127.0.0.1:8080/api/access/verify",
        "http://localhost/api/access/verify",
        "https://127.0.0.1/api/access/verify",
        "http://www.citrate.ai/api/access/verify",
        "https://citrate.ai/api/access/verify",
        "https://example.com/api/access/verify",
        "https://www.citrate.ai.example.com/api/access/verify",
        "https://www.citrate.ai:8443/api/access/verify",
        "https://user@www.citrate.ai/api/access/verify",
        "https://user:pw@www.citrate.ai/api/access/verify",
        "file:///etc/passwd",
        "",
    ] {
        assert!(!gate_url_ok(bad), "{bad}");
    }
}

#[test]
fn the_build_time_gate_url_check_is_the_same_rule() {
    // `build_gate_url_ok` runs at compile time on CITRATE_ACCESS_GATE_URL (a const assertion), so a
    // build cannot be made with a gate anywhere but https://www.citrate.ai/.
    assert!(build_gate_url_ok(None));
    assert!(build_gate_url_ok(Some(GATE)));
    for bad in [
        "http://localhost/api/access/verify",
        "http://127.0.0.1:8080/api/access/verify",
        "https://www.citrate.ai",
        "https://www.citrate.ai.example.com/api/access/verify",
        "https://www.citrate.ai:8443/api/access/verify",
        "http://www.citrate.ai/api/access/verify",
        "",
    ] {
        assert!(!build_gate_url_ok(Some(bad)), "{bad}");
    }
    assert!(build_gate_url_ok(GATE_URL));
}

#[test]
fn answers_are_read_strictly() {
    assert_eq!(
        read_answer(200, r#"{"ok":true,"platform":"mac"}"#),
        Answer::Recorded {
            platform: "mac".into()
        }
    );
    assert_eq!(
        read_answer(200, r#"{"ok":true,"platform":"windows"}"#),
        Answer::Refused(AccessError::Unavailable)
    );
    assert_eq!(
        read_answer(200, r#"{"ok":true,"platform":"mac","x":1}"#),
        Answer::Refused(AccessError::Unavailable)
    );
    assert_eq!(
        read_answer(200, r#"{"ok":false,"platform":"mac"}"#),
        Answer::Refused(AccessError::Unavailable)
    );
    assert_eq!(
        read_answer(200, "<html>"),
        Answer::Refused(AccessError::Unavailable)
    );
    assert_eq!(
        read_answer(409, r#"{"ok":false,"reason":"used"}"#),
        Answer::Refused(AccessError::Used)
    );
    assert_eq!(
        read_answer(409, r#"{"ok":false,"reason":"expired"}"#),
        Answer::Refused(AccessError::Expired)
    );
    assert_eq!(
        read_answer(409, r#"{"ok":false,"reason":"revoked"}"#),
        Answer::Refused(AccessError::Revoked)
    );
    assert_eq!(
        read_answer(409, r#"{"ok":false,"reason":"other"}"#),
        Answer::Refused(AccessError::Unavailable)
    );
    assert_eq!(read_answer(400, ""), Answer::Refused(AccessError::NotACode));
    assert_eq!(read_answer(404, ""), Answer::Refused(AccessError::GateOff));
    assert_eq!(
        read_answer(429, ""),
        Answer::Refused(AccessError::RateLimited)
    );
    assert_eq!(
        read_answer(503, ""),
        Answer::Refused(AccessError::Unavailable)
    );
}

#[test]
fn a_build_without_a_gate_is_off_and_asks_nothing() {
    let p = path("off");
    let v = status_at(&p, None, false, NOW).unwrap();
    assert_eq!(
        v,
        AccessPassView {
            gate: "off",
            required: false,
            recorded: None,
            platform: None
        }
    );
    assert!(!p.exists(), "nothing written when off");
    let called = RefCell::new(false);
    let r = verify_at(&p, None, CODE, NOW, |_, _| {
        *called.borrow_mut() = true;
        Ok((200, String::new()))
    });
    assert_eq!(r, Err(AccessError::GateOff));
    assert!(!*called.borrow());
}

#[test]
fn a_bad_gate_url_reads_as_off() {
    let p = path("badurl");
    let v = status_at(&p, Some("http://example.com/x"), false, NOW).unwrap();
    assert_eq!(v.gate, "off");
    assert!(!v.required);
}

#[test]
fn a_fresh_install_is_asked() {
    let p = path("fresh");
    let v = status_at(&p, Some(GATE), false, NOW).unwrap();
    assert_eq!(
        v,
        AccessPassView {
            gate: "on",
            required: true,
            recorded: None,
            platform: None
        }
    );
}

#[test]
fn an_updating_install_is_recorded_as_existing_and_never_asked() {
    let p = path("existing");
    let v = status_at(&p, Some(GATE), true, NOW).unwrap();
    assert_eq!(v.recorded.as_deref(), Some("existing"));
    assert!(!v.required);
    // Recorded once, so a later status (saved state or not) stays existing.
    let again = status_at(&p, Some(GATE), false, NOW + 10).unwrap();
    assert_eq!(again.recorded.as_deref(), Some("existing"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
}

/// An app data dir as a fresh install leaves it after one quit at the access check: tauri-plugin-store
/// saves every store it loaded on exit, so `config.json` is there (empty, or with defaults), and the
/// custody envelope is made on every launch.
fn data_dir_after_a_quit_at_the_gate(tag: &str, config: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("citrate-access-data-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("dir");
    std::fs::write(d.join("config.json"), config).expect("config.json");
    std::fs::write(d.join("custody.enc"), b"{}").expect("custody.enc");
    d
}

#[test]
fn a_config_json_saved_at_quit_does_not_make_an_install_existing() {
    let defaults = r#"{"app":{"net":"testnet","rpc":"local","dataDir":"~/.citrate/core","cpuCap":50,"autolock":30,"channel":"stable","telemetry":false,"coreMembershipUrl":"https://membership.citrate.ai"}}"#;
    for (tag, config) in [("empty", "{}"), ("defaults", defaults)] {
        let d = data_dir_after_a_quit_at_the_gate(tag, config);
        let v = status_in(&d, Some(GATE), false, NOW).unwrap();
        assert!(v.required, "{tag}: a fresh install is still asked after a quit");
        assert_eq!(v.recorded, None, "{tag}");
        // And again on the next launch.
        assert!(status_in(&d, Some(GATE), false, NOW + 60).unwrap().required, "{tag}");
    }
}

#[test]
fn a_finished_onboarding_from_an_earlier_build_is_recorded_existing() {
    // `onboarded` is what the app read from its saved state: `stage: "done"`, written by 0.5.0..0.5.2
    // when onboarding ends (src/onboarding/accessPass.ts `finishedOnboardingBefore`).
    let d = data_dir_after_a_quit_at_the_gate("onboarded", "{}");
    let v = status_in(&d, Some(GATE), true, NOW).unwrap();
    assert_eq!(v.recorded.as_deref(), Some("existing"));
    assert!(!v.required);
    // Recorded in pass.json, so it stays existing even if the saved state is later lost.
    let again = status_in(&d, Some(GATE), false, NOW + 60).unwrap();
    assert_eq!(again.recorded.as_deref(), Some("existing"));
    assert!(d.join("access").join("pass.json").is_file());
}

#[test]
fn a_gate_off_build_writes_nothing_to_the_data_dir() {
    let d = data_dir_after_a_quit_at_the_gate("off-dir", "{}");
    let v = status_in(&d, None, true, NOW).unwrap();
    assert_eq!(v.gate, "off");
    assert!(!d.join("access").exists());
}

#[test]
fn verify_records_the_code_and_never_stores_it() {
    let p = path("verify-ok");
    let seen = RefCell::new(String::new());
    let v = verify_at(
        &p,
        Some(GATE),
        &format!("citrate://access?code={CODE}"),
        NOW,
        |url, body| {
            assert_eq!(url, GATE);
            *seen.borrow_mut() = body.to_string();
            Ok((200, r#"{"ok":true,"platform":"mac"}"#.into()))
        },
    )
    .unwrap();
    assert_eq!(
        v,
        AccessPassView {
            gate: "on",
            required: false,
            recorded: Some("invited".into()),
            platform: Some("mac".into())
        }
    );
    let sent: serde_json::Value = serde_json::from_str(&seen.borrow()).unwrap();
    assert_eq!(sent["code"], CODE);
    let stored = std::fs::read_to_string(&p).unwrap();
    assert!(!stored.contains(CODE), "the access code is never stored");
    let f = read_store(&p).unwrap().unwrap();
    assert_eq!(sent["install"], f.install_id.as_str());
    assert_eq!(f.recorded.as_deref(), Some("invited"));
    assert!(!status_at(&p, Some(GATE), false, NOW).unwrap().required);
}

#[test]
fn a_retry_after_a_lost_answer_is_the_same_install() {
    let p = path("retry");
    let first = RefCell::new(String::new());
    let r = verify_at(&p, Some(GATE), CODE, NOW, |_, body| {
        *first.borrow_mut() = body.to_string();
        Err("connection reset".into())
    });
    assert_eq!(r, Err(AccessError::Unavailable));
    let second = RefCell::new(String::new());
    verify_at(&p, Some(GATE), CODE, NOW, |_, body| {
        *second.borrow_mut() = body.to_string();
        Ok((200, r#"{"ok":true,"platform":"linux-x86_64"}"#.into()))
    })
    .unwrap();
    let a: serde_json::Value = serde_json::from_str(&first.borrow()).unwrap();
    let b: serde_json::Value = serde_json::from_str(&second.borrow()).unwrap();
    assert_eq!(a["install"], b["install"]);
}

#[test]
fn a_new_install_id_is_saved_before_the_request_even_over_a_damaged_store() {
    for (tag, damaged) in [
        ("garbage", &b"garbage"[..]),
        ("foreign", &br#"{"format":"other","installId":"x"}"#[..]),
    ] {
        let p = path(&format!("damaged-id-{tag}"));
        std::fs::write(&p, damaged).unwrap();
        let first = RefCell::new(String::new());
        let r = verify_at(&p, Some(GATE), CODE, NOW, |_, body| {
            // At the moment the request goes out, the id it carries is already on disk.
            let sent: serde_json::Value = serde_json::from_str(body).unwrap();
            let on_disk = read_store(&p).expect("readable").expect("saved");
            assert_eq!(sent["install"], on_disk.install_id.as_str(), "{tag}");
            *first.borrow_mut() = body.to_string();
            Err("connection reset".into())
        });
        assert_eq!(r, Err(AccessError::Unavailable), "{tag}");
        let second = RefCell::new(String::new());
        let _ = verify_at(&p, Some(GATE), CODE, NOW, |_, body| {
            *second.borrow_mut() = body.to_string();
            Err("connection reset".into())
        });
        let a: serde_json::Value = serde_json::from_str(&first.borrow()).unwrap();
        let b: serde_json::Value = serde_json::from_str(&second.borrow()).unwrap();
        assert_eq!(a["install"], b["install"], "{tag}: a retry is the same install");
    }
}

#[test]
fn a_refused_code_records_nothing() {
    let p = path("refused");
    let r = verify_at(&p, Some(GATE), CODE, NOW, |_, _| {
        Ok((409, r#"{"ok":false,"reason":"used"}"#.into()))
    });
    assert_eq!(r, Err(AccessError::Used));
    assert_eq!(read_store(&p).unwrap().unwrap().recorded, None);
    assert!(status_at(&p, Some(GATE), false, NOW).unwrap().required);
}

#[test]
fn malformed_input_never_reaches_the_gate() {
    let p = path("malformed");
    let called = RefCell::new(false);
    let r = verify_at(&p, Some(GATE), "not a code", NOW, |_, _| {
        *called.borrow_mut() = true;
        Ok((200, String::new()))
    });
    assert_eq!(r, Err(AccessError::NotACode));
    assert!(!*called.borrow());
}

#[test]
fn a_damaged_store_is_not_read_as_recorded() {
    let p = path("damaged");
    std::fs::write(
        &p,
        br#"{"format":"citrate-access-pass-1","installId":"x","recorded":"invited"}"#,
    )
    .unwrap();
    assert!(read_store(&p).is_err());
    assert!(status_at(&p, Some(GATE), false, NOW).unwrap().required);
    std::fs::write(&p, b"garbage").unwrap();
    assert!(status_at(&p, Some(GATE), false, NOW).unwrap().required);
}

/// A one-request loopback fake. It reads the WHOLE request (headers and the Content-Length body)
/// before it answers, and keeps its end open until the client closes. The earlier fake read once and
/// dropped the socket: when the body had not arrived in that read, closing with unread bytes made the
/// kernel send RST, and macOS then fails the client's next setsockopt (ureq sets the socket timeouts
/// before each read and write) with EINVAL, "Invalid argument (os error 22)".
fn serve_once(listener: std::net::TcpListener, answer: String) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        s.set_read_timeout(Some(std::time::Duration::from_secs(10)))
            .unwrap();
        let mut req = Vec::new();
        let mut buf = [0u8; 4096];
        let head_end = loop {
            let n = s.read(&mut buf).unwrap();
            assert!(n > 0, "client closed before the request ended");
            req.extend_from_slice(&buf[..n]);
            if let Some(i) = req.windows(4).position(|w| w == b"\r\n\r\n") {
                break i + 4;
            }
        };
        let head = String::from_utf8_lossy(&req[..head_end]).to_ascii_lowercase();
        let len: usize = head
            .lines()
            .find_map(|l| l.strip_prefix("content-length:"))
            .map(|v| v.trim().parse().unwrap())
            .unwrap_or(0);
        while req.len() < head_end + len {
            let n = s.read(&mut buf).unwrap();
            assert!(n > 0, "client closed before the body ended");
            req.extend_from_slice(&buf[..n]);
        }
        s.write_all(answer.as_bytes()).unwrap();
        s.flush().unwrap();
        // Wait for the client to close its end (it reads the answer, then drops the connection).
        while matches!(s.read(&mut buf), Ok(n) if n > 0) {}
        String::from_utf8_lossy(&req).to_string()
    })
}

fn answer(status_line: &str, extra_headers: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status_line}\r\nContent-Type: application/json\r\n{extra_headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

#[test]
fn post_json_reads_a_non_2xx_answer_from_a_loopback_fake() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = serve_once(
        listener,
        answer("409 Conflict", "", r#"{"ok":false,"reason":"expired"}"#),
    );
    let url = format!("http://{addr}/api/access/verify");
    // The production agent is https-only; the loopback fake is plain http, so the test agent
    // differs from it in that one setting only.
    let (status, text) = post_json_with(
        &gate_agent(false),
        &url,
        r#"{"code":"x","install":"y"}"#,
    )
    .unwrap();
    assert_eq!(status, 409);
    assert_eq!(
        read_answer(status, &text),
        Answer::Refused(AccessError::Expired)
    );
    let req = server.join().unwrap();
    assert!(req.starts_with("POST /api/access/verify "));
    assert!(req
        .to_ascii_lowercase()
        .contains("content-type: application/json"));
    assert!(req.ends_with(r#"{"code":"x","install":"y"}"#));
}

#[test]
fn post_json_does_not_follow_a_redirect() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let elsewhere = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    elsewhere.set_nonblocking(true).unwrap();
    let to = format!("http://{}/api/access/verify", elsewhere.local_addr().unwrap());
    let server = serve_once(
        listener,
        answer("307 Temporary Redirect", &format!("Location: {to}\r\n"), "{}"),
    );
    let (status, _) = post_json_with(
        &gate_agent(false),
        &format!("http://{addr}/api/access/verify"),
        r#"{"code":"x","install":"y"}"#,
    )
    .unwrap();
    server.join().unwrap();
    assert_eq!(status, 307, "the redirect comes back as an answer");
    assert_eq!(read_answer(status, "{}"), Answer::Refused(AccessError::Unavailable));
    assert!(
        matches!(elsewhere.accept(), Err(e) if e.kind() == std::io::ErrorKind::WouldBlock),
        "the redirect target was never contacted"
    );
}

#[test]
fn post_json_refuses_plain_http_and_never_connects() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}/api/access/verify", listener.local_addr().unwrap());
    let r = post_json(&url, r#"{"code":"x","install":"y"}"#);
    assert!(r.is_err(), "plain http is refused: {r:?}");
    assert!(
        matches!(listener.accept(), Err(e) if e.kind() == std::io::ErrorKind::WouldBlock),
        "no connection was made"
    );
}

#[test]
fn the_commands_are_registered_and_allowed_for_the_main_window() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let lib = std::fs::read_to_string(root.join("src/lib.rs")).unwrap();
    let acl = std::fs::read_to_string(root.join("permissions/main-window.toml")).unwrap();
    for cmd in ["access_pass_status", "access_pass_verify"] {
        assert!(
            lib.contains(&format!("access_pass::{cmd}")),
            "{cmd} in generate_handler"
        );
        assert!(
            acl.contains(&format!("\"{cmd}\"")),
            "{cmd} in main-window.toml"
        );
    }
}

#[test]
fn messages_are_plain_and_name_no_secret() {
    for e in [
        AccessError::NotACode,
        AccessError::Used,
        AccessError::Expired,
        AccessError::Revoked,
        AccessError::GateOff,
        AccessError::RateLimited,
        AccessError::Unavailable,
    ] {
        let m = e.message();
        assert!(!m.is_empty());
        assert!(!m.contains('—'), "no em-dash: {m}");
    }
}
