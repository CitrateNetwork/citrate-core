// L7-WP06: the read-only Tailscale LocalAPI client. Fixtures are synthetic (invented host names,
// logins and addresses), shaped like the LocalAPI's /status and /whois answers. Each test talks to
// a fake LocalAPI on loopback (or a unix socket); nothing here reaches a real Tailscale.
// Written red-first: the module body was absent, so these failed to compile, then brought green.

use super::*;
use std::net::TcpListener;
use std::sync::mpsc::Receiver;
use std::time::Instant;

const TOKEN: &str = "t0k3nabc123";

const STATUS: &str = r#"{
  "Version": "1.90.1-t1234",
  "BackendState": "Running",
  "Self": {"HostName": "studio", "OS": "macOS", "TailscaleIPs": ["100.64.1.10"], "Online": true,
           "DNSName": "studio.example.ts.net.", "UserID": 7},
  "User": {"7": {"ID": 7, "LoginName": "member@example.com"}},
  "Peer": {
    "nodekey:aa": {"HostName": "linux-box", "DNSName": "linux-box.example.ts.net.", "UserID": 7,
                   "OS": "linux", "TailscaleIPs": ["100.64.1.11"], "Online": true},
    "nodekey:bb": {"HostName": "old-laptop", "OS": "windows", "TailscaleIPs": ["100.64.1.12"],
                   "Online": false}
  }
}"#;

const WHOIS: &str = r#"{
  "Node": {"ID": 11, "Name": "linux-box.example.ts.net.", "Addresses": ["100.64.1.11/32"]},
  "UserProfile": {"ID": 7, "LoginName": "member@example.com", "DisplayName": "Member"}
}"#;

/// A one-shot fake LocalAPI on loopback: answers `reply` after `delay` and hands back the request.
fn fake_tcp(reply: Vec<u8>, delay: Duration) -> (u16, Receiver<String>) {
    let l = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = l.local_addr().expect("addr").port();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        if let Ok((s, _)) = l.accept() {
            serve_one(s, &reply, delay, &tx);
        }
    });
    (port, rx)
}

fn serve_one<S: Read + Write>(mut s: S, reply: &[u8], delay: Duration, tx: &mpsc::Sender<String>) {
    let mut buf = Vec::new();
    let mut b = [0u8; 1024];
    while find(&buf, b"\r\n\r\n").is_none() {
        match s.read(&mut b) {
            Ok(0) | Err(_) => break,
            Ok(n) => buf.extend_from_slice(&b[..n]),
        }
    }
    let _ = tx.send(String::from_utf8_lossy(&buf).into_owned());
    std::thread::sleep(delay);
    let _ = s.write_all(reply);
}

fn ok(body: &str) -> Vec<u8> {
    format!("HTTP/1.0 200 OK\r\nContent-Type: application/json\r\n\r\n{body}").into_bytes()
}

fn tcp(port: u16) -> Transport {
    Transport::Tcp {
        port,
        token: Token(TOKEN.to_string()),
    }
}

#[test]
fn the_client_only_issues_get_on_status_and_whois() {
    let eps = [
        Endpoint::Status,
        Endpoint::WhoIs("100.64.1.11".parse().expect("ip")),
        Endpoint::WhoIs("fd7a:115c:a1e0::1".parse().expect("ip")),
    ];
    for ep in eps {
        // Exhaustive on purpose: a new Endpoint does not compile until it is listed here.
        let want = match ep {
            Endpoint::Status => "GET /localapi/v0/status HTTP/1.0".to_string(),
            Endpoint::WhoIs(ip) => format!(
                "GET /localapi/v0/whois?addr={} HTTP/1.0",
                ip.to_string().replace(':', "%3A")
            ),
        };
        let req = String::from_utf8(request_bytes(&ep, None)).expect("ascii");
        assert_eq!(req.split("\r\n").next(), Some(want.as_str()));
        assert!(req.contains("\r\nHost: local-tailscaled.sock\r\n"));
        let lower = req.to_ascii_lowercase();
        assert!(!lower.contains("origin:") && !lower.contains("referer:"));
        assert!(req.ends_with("\r\n\r\n"), "no body");
        assert!(!lower.contains("authorization:"), "no token was given");
    }
    // The whois address is escaped, so it cannot add a query parameter or a path.
    let v6 = String::from_utf8(request_bytes(&eps[2], None)).expect("ascii");
    assert!(v6.starts_with("GET /localapi/v0/whois?addr=fd7a%3A115c%3Aa1e0%3A%3A1 HTTP/1.0"));
    // No other method appears anywhere in the client.
    let src = include_str!("tailscale_local.rs");
    for m in [
        "\"POST",
        "\"PUT",
        "\"PATCH",
        "\"DELETE",
        "\"HEAD",
        "\"OPTIONS",
    ] {
        assert!(!src.contains(m), "{m}");
    }
    assert_eq!(src.matches("\"GET ").count(), 1, "one request builder");
}

#[test]
fn status_json_maps_to_the_same_report_as_the_cli() {
    let (port, req) = fake_tcp(ok(STATUS), Duration::ZERO);
    let got = status_via(&tcp(port), TIMEOUT).expect("status");
    // The same parser as `tailscale status --json`, so both paths give the same report.
    assert_eq!(got, parse_status(STATUS));
    assert_eq!(got.state, crate::fleet_tailscale::TsState::Running);
    // The macOS token rides as HTTP Basic auth with an empty user.
    let basic = base64::engine::general_purpose::STANDARD.encode(format!(":{TOKEN}"));
    let req = req.recv_timeout(TIMEOUT).expect("request");
    assert!(
        req.contains(&format!("\r\nAuthorization: Basic {basic}\r\n")),
        "{req}"
    );
}

#[test]
fn magicdns_names_are_read_from_peer_dnsname() {
    let r = parse_status(STATUS);
    let linux = r
        .peers
        .iter()
        .find(|p| p.host_name == "linux-box")
        .expect("peer");
    assert_eq!(linux.dns_name.as_deref(), Some("linux-box.example.ts.net"));
    assert_eq!(linux.user_login.as_deref(), Some("member@example.com"));
    let old = r
        .peers
        .iter()
        .find(|p| p.host_name == "old-laptop")
        .expect("peer");
    assert_eq!(old.dns_name, None);
    assert_eq!(old.user_login, None);
}

#[test]
fn whois_maps_login_and_node() {
    let (port, req) = fake_tcp(ok(WHOIS), Duration::ZERO);
    let ip: IpAddr = "100.64.1.11".parse().expect("ip");
    let got = whois_via(&tcp(port), ip, TIMEOUT).expect("whois");
    assert_eq!(
        got,
        WhoIs {
            login: Some("member@example.com".into()),
            node: Some("linux-box.example.ts.net".into()),
        }
    );
    let req = req.recv_timeout(TIMEOUT).expect("request");
    assert!(req.starts_with("GET /localapi/v0/whois?addr=100.64.1.11 HTTP/1.0\r\n"));
    assert_eq!(
        parse_whois("{}"),
        Ok(WhoIs {
            login: None,
            node: None
        })
    );
    assert_eq!(parse_whois("not json"), Err(LocalApiError::Malformed));
}

#[test]
fn a_slow_localapi_times_out_in_4s() {
    assert_eq!(TIMEOUT, Duration::from_secs(4));
    let (port, _req) = fake_tcp(ok(STATUS), Duration::from_secs(3));
    let started = Instant::now();
    let got = status_via(&tcp(port), Duration::from_millis(300));
    assert_eq!(got, Err(LocalApiError::Timeout));
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "{:?}",
        started.elapsed()
    );
}

#[test]
fn oversized_answers_are_capped() {
    assert_eq!(MAX_BODY, 4 * 1024 * 1024);
    let big = "x".repeat(MAX_BODY + 1);
    assert_eq!(parse_response(&ok(&big)), Err(LocalApiError::TooLarge));
    let chunk = format!(
        "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n",
        MAX_BODY + 1
    );
    assert_eq!(
        parse_response(chunk.as_bytes()),
        Err(LocalApiError::TooLarge)
    );
    // Through a live connection: reading stops at the cap.
    let mut huge = ok("");
    huge.extend(std::iter::repeat_n(b'x', MAX_BODY + MAX_HEAD + 8));
    let (port, _req) = fake_tcp(huge, Duration::ZERO);
    assert_eq!(
        status_via(&tcp(port), TIMEOUT),
        Err(LocalApiError::TooLarge)
    );
}

#[test]
fn the_token_is_never_logged_or_returned() {
    let t = tcp(1);
    assert!(!format!("{t:?}").contains(TOKEN));
    assert!(format!("{t:?}").contains("redacted"));
    let (port, _req) = fake_tcp(
        b"HTTP/1.0 403 Forbidden\r\n\r\nbad password".to_vec(),
        Duration::ZERO,
    );
    let e = status_via(&tcp(port), TIMEOUT).expect_err("refused");
    assert_eq!(e, LocalApiError::Http(403));
    for e in [
        e,
        LocalApiError::NotFound,
        LocalApiError::Connect("connection refused".into()),
        LocalApiError::Timeout,
        LocalApiError::TooLarge,
        LocalApiError::Malformed,
    ] {
        assert!(!format!("{e} {e:?}").contains(TOKEN));
    }
}

#[test]
fn fallback_to_cli_is_reported_as_cli() {
    use crate::fleet_tailscale::{pick, TailscaleVia, TsState};
    let (r, via) = pick(Ok(parse_status(STATUS)), || {
        panic!("the CLI is not run when the LocalAPI answered")
    });
    assert_eq!(via, TailscaleVia::Localapi);
    assert_eq!(r.state, TsState::Running);
    let (r, via) = pick(Err(LocalApiError::NotFound), || parse_status(STATUS));
    assert_eq!(via, TailscaleVia::Cli);
    assert_eq!(r, parse_status(STATUS));
    let (r, via) = pick(Err(LocalApiError::Timeout), TailscaleReport::not_installed);
    assert_eq!(via, TailscaleVia::None);
    assert_eq!(r.state, TsState::NotInstalled);
    assert_eq!(
        serde_json::to_value([
            TailscaleVia::Localapi,
            TailscaleVia::Cli,
            TailscaleVia::None
        ])
        .expect("serializes"),
        serde_json::json!(["localapi", "cli", "none"])
    );
}

#[test]
fn answers_other_than_a_json_200_are_errors() {
    assert_eq!(parse_response(b"garbage"), Err(LocalApiError::Malformed));
    assert_eq!(
        parse_response(b"HTTP/1.1 404 Not Found\r\n\r\nno"),
        Err(LocalApiError::Http(404))
    );
    let chunked = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\n{\"a\r\n2\r\n\":\r\n2\r\n1}\r\n0\r\n\r\n";
    assert_eq!(parse_response(chunked), Ok(b"{\"a\":1}".to_vec()));
    assert_eq!(
        parse_response(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nzz\r\n"),
        Err(LocalApiError::Malformed)
    );
    let (port, _req) = fake_tcp(ok("<html>not json</html>"), Duration::ZERO);
    assert_eq!(
        status_via(&tcp(port), TIMEOUT),
        Err(LocalApiError::Malformed)
    );
}

#[test]
fn the_endpoints_each_os_reads_are_pinned() {
    // Checked against tailscale/tailscale main d486755f (2026-11-28); see the module docs.
    assert_eq!(MACSYS_DIR, "/Library/Tailscale");
    assert_eq!(MAC_SOCKETS, &["/var/run/tailscaled.socket"]);
    assert_eq!(
        LINUX_SOCKETS,
        &[
            "/var/run/tailscale/tailscaled.sock",
            "/run/tailscale/tailscaled.sock"
        ]
    );
    assert_eq!(
        WINDOWS_PIPE,
        r"\\.\pipe\ProtectedPrefix\Administrators\Tailscale\tailscaled"
    );
    assert_eq!(HOST, "local-tailscaled.sock");
}

/// A fresh, empty directory for one test. Short, so a unix socket path inside it stays under the
/// 104-byte limit on macOS.
#[cfg(unix)]
fn temp_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("cts-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("temp dir");
    d
}

#[cfg(unix)]
#[test]
fn macsys_port_and_token_are_read_from_the_shared_dir() {
    let d = temp_dir("macsys");
    assert_eq!(macsys_from(&d), None, "nothing there");
    std::os::unix::fs::symlink("41112", d.join("ipnport")).expect("symlink");
    assert_eq!(macsys_from(&d), None, "no token file");
    std::fs::write(d.join("sameuserproof-41112"), format!("{TOKEN}\n")).expect("write");
    assert_eq!(
        macsys_from(&d),
        Some(Transport::Tcp {
            port: 41112,
            token: Token(TOKEN.to_string())
        })
    );
    // A token that is not plain alphanumerics (header injection) is refused.
    std::fs::write(d.join("sameuserproof-41112"), "abc\r\nX-Evil: 1").expect("write");
    assert_eq!(macsys_from(&d), None);
    let _ = std::fs::remove_dir_all(&d);
}

#[cfg(unix)]
#[test]
fn a_unix_socket_localapi_answers() {
    let d = temp_dir("unix");
    let path = d.join("t.sock");
    let l = std::os::unix::net::UnixListener::bind(&path).expect("bind");
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        if let Ok((s, _)) = l.accept() {
            serve_one(s, &ok(STATUS), Duration::ZERO, &tx);
        }
    });
    let got = status_via(&Transport::Unix(path.clone()), TIMEOUT).expect("status");
    assert_eq!(got, parse_status(STATUS));
    let req = rx.recv_timeout(TIMEOUT).expect("request");
    assert!(
        !req.to_ascii_lowercase().contains("authorization:"),
        "no token on the socket"
    );
    assert!(matches!(
        status_via(&Transport::Unix(d.join("absent.sock")), TIMEOUT),
        Err(LocalApiError::Connect(_))
    ));
    let _ = std::fs::remove_dir_all(&d);
}
