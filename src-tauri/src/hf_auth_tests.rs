// HUP-S0.3b — hf_auth tests. RED-FIRST.
//
// The origin predicate is proven as a pure function over URL shapes (including the CDN hosts a
// Hugging Face resolve redirects to). The redirect behavior is proven over REAL loopback sockets:
// a tiny one-shot HTTP server records the raw request headers each hop received, so the
// "a cross-origin redirect carries no Authorization" claim is checked on the wire, not on a mock.

use super::*;
use std::io::{BufRead as _, BufReader, Write as _};
use std::net::TcpListener;
use std::sync::{Arc, Mutex as StdMutex};

const TOK: &str = "hf_test_token_value";

fn token() -> HfToken {
    HfToken::new(TOK.to_string()).expect("valid token")
}

fn url(s: &str) -> url::Url {
    url::Url::parse(s).expect("url")
}

// ---------------------------------------------------------------------------
// One-shot loopback HTTP server: answers `responses` in order (one per connection) and records
// every request's raw head (request line + headers, lowercased header names kept as sent).
// ---------------------------------------------------------------------------

struct TestServer {
    port: u16,
    seen: Arc<StdMutex<Vec<String>>>,
}

impl TestServer {
    fn start(responses: Vec<String>) -> TestServer {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let seen = Arc::new(StdMutex::new(Vec::new()));
        let seen2 = seen.clone();
        std::thread::spawn(move || {
            for resp in responses {
                let Ok((stream, _)) = listener.accept() else {
                    return;
                };
                let mut reader = BufReader::new(stream);
                let mut head = String::new();
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 {
                        break;
                    }
                    if line == "\r\n" {
                        break;
                    }
                    head.push_str(&line);
                }
                seen2.lock().expect("lock").push(head);
                let mut stream = reader.into_inner();
                let _ = stream.write_all(resp.as_bytes());
                let _ = stream.flush();
            }
        });
        TestServer { port, seen }
    }

    fn origin(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    fn requests(&self) -> Vec<String> {
        self.seen.lock().expect("lock").clone()
    }
}

fn redirect_to(location: &str) -> String {
    format!(
        "HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    )
}

fn ok_body(status: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nContent-Range: bytes 0-{}/{}\r\nConnection: close\r\n\r\n{body}",
        body.len(),
        body.len().saturating_sub(1),
        body.len()
    )
}

fn status_only(status: &str) -> String {
    format!("HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
}

fn has_auth(head: &str) -> bool {
    head.lines()
        .any(|l| l.to_ascii_lowercase().starts_with("authorization:"))
}

fn timeouts() -> Timeouts {
    Timeouts {
        connect: Some(std::time::Duration::from_secs(5)),
        recv_response: Some(std::time::Duration::from_secs(5)),
        global: Some(std::time::Duration::from_secs(10)),
    }
}

fn read_all(resp: ureq::http::Response<ureq::Body>) -> String {
    resp.into_body().read_to_string().expect("body")
}

// ---------------------------------------------------------------------------
// Pure origin predicate
// ---------------------------------------------------------------------------

#[test]
fn hf_scope_allows_only_the_exact_hf_origins_over_https() {
    let s = AuthScope::huggingface();
    assert!(s.allows(&url("https://huggingface.co/org/repo/resolve/main/m.gguf")));
    assert!(s.allows(&url("https://hf.co/org/repo/resolve/main/m.gguf")));
    assert!(s.allows(&url("https://HUGGINGFACE.CO/x")), "host compare is case-insensitive");
    assert!(s.allows(&url("https://huggingface.co:443/x")), "explicit default port");
}

#[test]
fn hf_scope_rejects_cdn_lookalike_downgrade_port_and_userinfo() {
    let s = AuthScope::huggingface();
    for bad in [
        "https://cdn-lfs.hf.co/repos/aa/bb/blob",
        "https://cdn-lfs-us-1.hf.co/repos/aa/bb/blob",
        "https://us.aws.cdn.hf.co/xet/blob",
        "https://cas-bridge.xethub.hf.co/xet-bridge-us/blob",
        "https://transfer.xethub.hf.co/blob",
        "https://huggingface.co.evil.example/x",
        "https://evilhuggingface.co/x",
        "https://xhf.co/x",
        "http://huggingface.co/x",
        "https://huggingface.co:8443/x",
        "https://user@huggingface.co/x",
        "https://user:pw@hf.co/x",
        "https://citrate.ai/download/model",
    ] {
        assert!(!s.allows(&url(bad)), "must not attach the token to {bad}");
    }
}

#[test]
fn header_for_is_none_off_scope_and_bearer_on_scope() {
    let t = token();
    let s = AuthScope::huggingface();
    assert_eq!(
        header_for(&s, Some(&t), &url("https://huggingface.co/x")).as_deref().map(String::as_str),
        Some("Bearer hf_test_token_value")
    );
    assert!(header_for(&s, Some(&t), &url("https://cdn-lfs.hf.co/x")).is_none());
    assert!(header_for(&s, None, &url("https://huggingface.co/x")).is_none());
}

#[test]
fn token_rejects_empty_whitespace_and_header_breaking_bytes() {
    assert!(HfToken::new(String::new()).is_none());
    assert!(HfToken::new("   ".to_string()).is_none());
    assert!(HfToken::new("abc\r\nX-Evil: 1".to_string()).is_none());
    assert!(HfToken::new("abc def".to_string()).is_none());
    assert!(HfToken::new("abc\u{7f}".to_string()).is_none());
    assert!(HfToken::new("hf_AbC-123._~+/=".to_string()).is_some());
}

#[test]
fn token_debug_never_prints_the_value() {
    let dbg = format!("{:?}", token());
    assert!(!dbg.contains(TOK), "Debug leaked the token: {dbg}");
}

#[test]
fn gated_message_points_at_settings_connections_and_has_no_em_dash() {
    for sent in [false, true] {
        let msg = FetchError::Gated { token_sent: sent }.to_string();
        assert!(msg.contains("Hugging Face token"), "{msg}");
        assert!(msg.contains("Settings › Connections"), "{msg}");
        assert!(!msg.contains('\u{2014}'), "no em-dash in user-facing text: {msg}");
        assert!(!msg.contains(TOK));
    }
}

// ---------------------------------------------------------------------------
// Redirect handling over real sockets
// ---------------------------------------------------------------------------

#[test]
fn cross_origin_redirect_strips_authorization() {
    // B is the "CDN": a different origin (different port on loopback).
    let cdn = TestServer::start(vec![ok_body("206 Partial Content", "GGUFdata")]);
    let hub = TestServer::start(vec![redirect_to(&format!("{}/blob?sig=1", cdn.origin()))]);
    let scope = AuthScope::exact_for_tests(&hub.origin());

    let resp = fetch(
        &format!("{}/org/repo/resolve/main/m.gguf", hub.origin()),
        Some("bytes=0-7"),
        &scope,
        Some(&token()),
        &timeouts(),
    )
    .expect("fetch");
    assert_eq!(resp.status().as_u16(), 206);
    assert_eq!(read_all(resp), "GGUFdata");

    let hub_reqs = hub.requests();
    let cdn_reqs = cdn.requests();
    assert_eq!(hub_reqs.len(), 1);
    assert_eq!(cdn_reqs.len(), 1);
    assert!(
        hub_reqs[0].contains("Bearer hf_test_token_value"),
        "the hub hop carries the token: {}",
        hub_reqs[0]
    );
    assert!(
        !has_auth(&cdn_reqs[0]),
        "the cross-origin hop must carry NO Authorization: {}",
        cdn_reqs[0]
    );
    assert!(!cdn_reqs[0].contains(TOK));
    assert!(
        cdn_reqs[0].to_ascii_lowercase().contains("range: bytes=0-7"),
        "the Range header survives the redirect: {}",
        cdn_reqs[0]
    );
}

#[test]
fn same_origin_relative_redirect_keeps_authorization() {
    let hub = TestServer::start(vec![
        redirect_to("/api/resolve-cache/models/org/repo/abc/m.gguf"),
        ok_body("200 OK", "GGUF"),
    ]);
    let scope = AuthScope::exact_for_tests(&hub.origin());
    let resp = fetch(
        &format!("{}/org/repo/resolve/main/m.gguf", hub.origin()),
        None,
        &scope,
        Some(&token()),
        &timeouts(),
    )
    .expect("fetch");
    assert_eq!(read_all(resp), "GGUF");
    let reqs = hub.requests();
    assert_eq!(reqs.len(), 2);
    assert!(reqs[1].starts_with("GET /api/resolve-cache/models/org/repo/abc/m.gguf"));
    assert!(reqs.iter().all(|h| h.contains("Bearer hf_test_token_value")));
}

#[test]
fn redirect_back_onto_the_hub_after_leaving_it_re_attaches_only_on_the_hub() {
    // hub1 -> other -> hub2 (both hubs in scope): the token rides the in-scope hops only.
    // Two in-scope servers stand in for "the same hub" because each test server's port must be
    // known before the server that redirects to it is started.
    let hub2 = TestServer::start(vec![ok_body("200 OK", "GGUF")]);
    let other = TestServer::start(vec![redirect_to(&format!("{}/final", hub2.origin()))]);
    let hub1 = TestServer::start(vec![redirect_to(&format!("{}/hop", other.origin()))]);
    let scope = AuthScope::exact_origins_for_tests(&[&hub1.origin(), &hub2.origin()]);
    let resp = fetch(
        &format!("{}/start", hub1.origin()),
        None,
        &scope,
        Some(&token()),
        &timeouts(),
    )
    .expect("fetch");
    assert_eq!(read_all(resp), "GGUF");
    assert!(has_auth(&hub1.requests()[0]));
    assert!(!has_auth(&other.requests()[0]));
    assert!(has_auth(&hub2.requests()[0]));
}

#[test]
fn no_token_sends_no_authorization_anywhere() {
    let hub = TestServer::start(vec![ok_body("200 OK", "GGUF")]);
    let scope = AuthScope::exact_for_tests(&hub.origin());
    let resp = fetch(&format!("{}/x", hub.origin()), None, &scope, None, &timeouts())
        .expect("fetch");
    assert_eq!(read_all(resp), "GGUF");
    assert!(!has_auth(&hub.requests()[0]));
}

#[test]
fn gated_401_without_token_is_the_honest_gated_error() {
    let hub = TestServer::start(vec![status_only("401 Unauthorized")]);
    let scope = AuthScope::exact_for_tests(&hub.origin());
    let err = fetch(&format!("{}/x", hub.origin()), None, &scope, None, &timeouts())
        .expect_err("401 must fail");
    assert_eq!(err, FetchError::Gated { token_sent: false });
}

#[test]
fn gated_403_with_token_is_the_honest_gated_error() {
    let hub = TestServer::start(vec![status_only("403 Forbidden")]);
    let scope = AuthScope::exact_for_tests(&hub.origin());
    let err = fetch(
        &format!("{}/x", hub.origin()),
        None,
        &scope,
        Some(&token()),
        &timeouts(),
    )
    .expect_err("403 must fail");
    assert_eq!(err, FetchError::Gated { token_sent: true });
}

#[test]
fn a_403_from_the_cdn_is_not_reported_as_gated() {
    let cdn = TestServer::start(vec![status_only("403 Forbidden")]);
    let hub = TestServer::start(vec![redirect_to(&format!("{}/blob", cdn.origin()))]);
    let scope = AuthScope::exact_for_tests(&hub.origin());
    let err = fetch(
        &format!("{}/x", hub.origin()),
        None,
        &scope,
        Some(&token()),
        &timeouts(),
    )
    .expect_err("403 must fail");
    assert_eq!(err, FetchError::Status(403));
}

#[test]
fn non_2xx_final_status_is_an_error() {
    let hub = TestServer::start(vec![status_only("500 Internal Server Error")]);
    let scope = AuthScope::exact_for_tests(&hub.origin());
    let err = fetch(&format!("{}/x", hub.origin()), None, &scope, None, &timeouts())
        .expect_err("500 must fail");
    assert_eq!(err, FetchError::Status(500));
}

#[test]
fn redirect_loop_is_bounded() {
    let responses: Vec<String> = (0..=MAX_REDIRECTS).map(|_| redirect_to("/again")).collect();
    let hub = TestServer::start(responses);
    let scope = AuthScope::exact_for_tests(&hub.origin());
    let err = fetch(&format!("{}/x", hub.origin()), None, &scope, None, &timeouts())
        .expect_err("loop must fail");
    assert_eq!(err, FetchError::TooManyRedirects);
    assert_eq!(hub.requests().len() as u32, MAX_REDIRECTS + 1);
}

#[test]
fn redirect_to_a_non_http_scheme_is_refused() {
    let hub = TestServer::start(vec![redirect_to("file:///etc/passwd")]);
    let scope = AuthScope::exact_for_tests(&hub.origin());
    let err = fetch(&format!("{}/x", hub.origin()), None, &scope, None, &timeouts())
        .expect_err("file: redirect must fail");
    assert_eq!(err, FetchError::BadRedirect);
}

#[test]
fn redirect_without_location_is_refused() {
    let hub = TestServer::start(vec![status_only("302 Found")]);
    let scope = AuthScope::exact_for_tests(&hub.origin());
    let err = fetch(&format!("{}/x", hub.origin()), None, &scope, None, &timeouts())
        .expect_err("no Location must fail");
    assert_eq!(err, FetchError::BadRedirect);
}

// ---------------------------------------------------------------------------
// The model download transport rides the same redirect handling
// ---------------------------------------------------------------------------

use crate::model::{ModelError, ModelTransport, UreqModelTransport};

fn transport_for(hub: &TestServer, path: &str, tok: Option<HfToken>) -> UreqModelTransport {
    UreqModelTransport::new(format!("{}{path}", hub.origin()))
        .with_hf_token(tok)
        .with_auth_scope(AuthScope::exact_for_tests(&hub.origin()))
}

#[test]
fn transport_segment_strips_authorization_on_the_cdn_hop_and_keeps_the_206_gate() {
    let cdn = TestServer::start(vec![ok_body("206 Partial Content", "GGUF")]);
    let hub = TestServer::start(vec![redirect_to(&format!("{}/xet/blob?sig=abc", cdn.origin()))]);
    let t = transport_for(&hub, "/org/repo/resolve/main/m.gguf", Some(token()));
    let mut r = t.get_range(0, 4).expect("segment");
    let mut got = String::new();
    std::io::Read::read_to_string(&mut r, &mut got).expect("read");
    assert_eq!(got, "GGUF");
    assert!(has_auth(&hub.requests()[0]));
    assert!(!has_auth(&cdn.requests()[0]), "{}", cdn.requests()[0]);
    assert!(cdn.requests()[0].to_ascii_lowercase().contains("range: bytes=0-3"));
}

#[test]
fn transport_resume_that_gets_200_is_still_range_ignored() {
    let cdn = TestServer::start(vec![ok_body("200 OK", "GGUFGGUF")]);
    let hub = TestServer::start(vec![redirect_to(&format!("{}/blob", cdn.origin()))]);
    let t = transport_for(&hub, "/m.gguf", Some(token()));
    let err = t.get_range(4, 8).err().expect("must refuse");
    assert!(matches!(err, ModelError::RangeIgnored), "{err}");
}

#[test]
fn transport_total_size_follows_the_redirect_without_leaking_the_token() {
    let cdn = TestServer::start(vec![ok_body("206 Partial Content", "G")]);
    let hub = TestServer::start(vec![redirect_to(&format!("{}/blob", cdn.origin()))]);
    let t = transport_for(&hub, "/m.gguf", Some(token()));
    // ok_body("G") answers Content-Range: bytes 0-0/1.
    assert_eq!(t.total_size().expect("size"), 1);
    assert!(has_auth(&hub.requests()[0]));
    assert!(!has_auth(&cdn.requests()[0]));
}

#[test]
fn transport_gated_repo_surfaces_the_honest_message_and_is_not_retried() {
    let hub = TestServer::start(vec![status_only("401 Unauthorized")]);
    let t = transport_for(&hub, "/org/gated/resolve/main/m.gguf", None);
    let err = t.get_range(0, 4).err().expect("gated");
    assert!(matches!(err, ModelError::Gated { token_sent: false }), "{err}");
    assert!(err.to_string().contains("Settings › Connections"));

    // The download loop returns a gated error at once (no backoff retries against a 401).
    let dir = std::env::temp_dir().join(format!(
        "citrate-core-hfauth-gated-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let hub2 = TestServer::start(vec![status_only("403 Forbidden"), status_only("403 Forbidden")]);
    let mgr = crate::model::ModelManager::new(
        dir.clone(),
        Box::new(transport_for(&hub2, "/m.gguf", Some(token()))),
        "00".repeat(32),
        4,
    )
    .with_retry_backoff(|_| std::time::Duration::ZERO);
    let err = mgr.download().expect_err("gated");
    assert!(matches!(err, ModelError::Gated { token_sent: true }), "{err}");
    assert_eq!(hub2.requests().len(), 1, "a gated answer is final, not retried");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn transport_without_a_token_on_a_public_repo_sends_no_authorization() {
    let hub = TestServer::start(vec![ok_body("206 Partial Content", "GGUF")]);
    let t = transport_for(&hub, "/m.gguf", None);
    let mut r = t.get_range(0, 4).expect("segment");
    let mut got = Vec::new();
    std::io::Read::read_to_end(&mut r, &mut got).expect("read");
    assert_eq!(got, b"GGUF");
    assert!(!has_auth(&hub.requests()[0]));
}
