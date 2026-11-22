// Onboarding model download: failure-mode tests (owner requirement 2026-11-22, "the user can
// access a decent local model on the way into the app ... it works flawlessly").
//
// Every network test here runs the REAL production transport (`UreqModelTransport`, which follows
// redirects hop by hop through `hf_auth::fetch`) against a loopback HTTP server that imitates the
// real chain: an ORIGIN that answers 302 to a SIGNED, EXPIRING CDN URL, the way the Hugging Face
// Xet CDN does (a signed `Expires=` URL that answers 403 once it has expired). The server can expire
// signed URLs, cut a response at expiry, drop a connection mid-body, stall, ignore Range, shift
// Content-Range, report a different file size, answer 404, and serve wrong bytes.

use super::*;
use std::collections::VecDeque;
use std::io::{BufRead as _, BufReader, Write as _};
use std::net::TcpListener;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

fn body_of(len: usize) -> Vec<u8> {
    let mut v = GGUF_MAGIC.to_vec();
    let mut b: u8 = 3;
    while v.len() < len {
        v.push(b);
        b = b.wrapping_mul(31).wrapping_add(17);
    }
    v
}

fn sha_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(bytes))
}

fn tmp_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let p = std::env::temp_dir().join(format!(
        "citrate-core-mdl-{tag}-{nanos}-{:?}",
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&p).expect("mk tmp dir");
    p
}

fn part_of(dir: &std::path::Path) -> PathBuf {
    dir.join(format!("{MODEL_FILE}.part"))
}

fn plenty_of_disk(_: &std::path::Path) -> Option<u64> {
    Some(u64::MAX / 4)
}

fn zero_backoff(_: u32) -> Duration {
    Duration::ZERO
}

// ---------------------------------------------------------------------------
// The loopback model server
// ---------------------------------------------------------------------------

/// One fault, applied to one CDN response (in order). After the queue is empty every response is
/// served normally, unless `sticky` is set.
#[derive(Clone, Debug)]
enum Fault {
    /// Send this many body bytes of a full Content-Length response, then close the socket.
    DropAfter(usize),
    /// Send this many body bytes, then hold the socket open, sending nothing.
    StallAfter(usize),
    /// Answer with this status and no body.
    Status(u16),
    /// Answer 200 with the whole file, ignoring Range.
    IgnoreRange,
    /// Answer 206, but with a Content-Range that starts somewhere else than asked.
    ShiftRange,
}

#[derive(Default)]
struct Cfg {
    /// Signed CDN URL lifetime.
    expiry: Option<Duration>,
    /// Cut an in-flight CDN response once its signed URL expires.
    cut_at_expiry: bool,
    /// Bytes per 10 ms sent on CDN responses (`None` = unthrottled).
    trickle: Option<usize>,
    faults: VecDeque<Fault>,
    sticky: Option<Fault>,
    /// The origin answers 404 (the object is not there).
    origin_404: bool,
    /// The origin answers 200 with a web page.
    origin_html: bool,
    /// The total size the CDN reports in Content-Range (a different file).
    report_total: Option<u64>,
    /// Serve these bytes instead of the real body (same length).
    wrong_body: Option<Arc<Vec<u8>>>,
}

#[derive(Clone, Debug)]
struct Seen {
    path: String,
    range: Option<String>,
    status: u16,
    /// For a CDN request: whether its signed URL had already expired when it arrived.
    expired: bool,
}

struct ModelServer {
    port: u16,
    cfg: Arc<StdMutex<Cfg>>,
    seen: Arc<StdMutex<Vec<Seen>>>,
}

impl ModelServer {
    fn start(body: Vec<u8>) -> ModelServer {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let cfg = Arc::new(StdMutex::new(Cfg::default()));
        let seen = Arc::new(StdMutex::new(Vec::new()));
        let body = Arc::new(body);
        let (cfg2, seen2) = (cfg.clone(), seen.clone());
        std::thread::spawn(move || {
            for conn in listener.incoming() {
                let Ok(stream) = conn else { return };
                let (cfg, seen, body) = (cfg2.clone(), seen2.clone(), body.clone());
                std::thread::spawn(move || serve_one(stream, port, &cfg, &seen, &body));
            }
        });
        ModelServer { port, cfg, seen }
    }

    fn url(&self) -> String {
        format!("http://127.0.0.1:{}/model", self.port)
    }

    fn set(&self, f: impl FnOnce(&mut Cfg)) {
        f(&mut self.cfg.lock().expect("cfg"));
    }

    fn seen(&self) -> Vec<Seen> {
        self.seen.lock().expect("seen").clone()
    }

    fn cdn(&self) -> Vec<Seen> {
        self.seen().into_iter().filter(|s| s.path.starts_with("/cdn")).collect()
    }

    fn origin_hits(&self) -> usize {
        self.seen().iter().filter(|s| s.path == "/model").count()
    }

    fn transport(&self, total: u64) -> UreqModelTransport {
        UreqModelTransport::new(self.url()).with_expected_total(total)
    }
}

fn now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

fn serve_one(
    stream: std::net::TcpStream,
    port: u16,
    cfg: &StdMutex<Cfg>,
    seen: &StdMutex<Vec<Seen>>,
    body: &Arc<Vec<u8>>,
) {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    if reader.read_line(&mut line).unwrap_or(0) == 0 {
        return;
    }
    let path = line.split_whitespace().nth(1).unwrap_or("/").to_string();
    let mut range = None;
    loop {
        let mut h = String::new();
        if reader.read_line(&mut h).unwrap_or(0) == 0 || h == "\r\n" {
            break;
        }
        if let Some(v) = h.strip_prefix("Range: ").or_else(|| h.strip_prefix("range: ")) {
            range = Some(v.trim().to_string());
        }
    }
    let mut out = reader.into_inner();
    let record = |status: u16, expired: bool| {
        seen.lock().expect("seen").push(Seen {
            path: path.clone(),
            range: range.clone(),
            status,
            expired,
        });
    };
    let head = |out: &mut std::net::TcpStream, status: &str, extra: &str, len: usize| {
        let _ = write!(
            out,
            "HTTP/1.1 {status}\r\nContent-Length: {len}\r\nConnection: close\r\n{extra}\r\n"
        );
    };

    if path == "/model" {
        let (c404, html, expiry) = {
            let c = cfg.lock().expect("cfg");
            (c.origin_404, c.origin_html, c.expiry.unwrap_or(Duration::from_secs(3600)))
        };
        if c404 {
            record(404, false);
            head(&mut out, "404 Not Found", "", 0);
            return;
        }
        if html {
            let page = b"<!DOCTYPE html><html><body>join the waitlist</body></html>";
            record(200, false);
            head(&mut out, "200 OK", "Content-Type: text/html\r\n", page.len());
            let _ = out.write_all(page);
            return;
        }
        let exp = now_ms() + expiry.as_millis();
        record(302, false);
        head(
            &mut out,
            "302 Found",
            &format!("Location: http://127.0.0.1:{port}/cdn/blob?Expires={exp}&Signature=sig{exp}\r\n"),
            0,
        );
        return;
    }

    let exp: u128 = path
        .split("Expires=")
        .nth(1)
        .and_then(|s| s.split('&').next())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    if now_ms() > exp {
        record(403, true);
        let msg = b"<Error><Code>AccessDenied</Code><Message>Request has expired</Message></Error>";
        head(&mut out, "403 Forbidden", "Content-Type: application/xml\r\n", msg.len());
        let _ = out.write_all(msg);
        return;
    }
    let (fault, trickle, cut, report_total, wrong) = {
        let mut c = cfg.lock().expect("cfg");
        let f = c.faults.pop_front().or_else(|| c.sticky.clone());
        (f, c.trickle, c.cut_at_expiry, c.report_total, c.wrong_body.clone())
    };
    let bytes: &[u8] = wrong.as_deref().map(|v| v.as_slice()).unwrap_or(body.as_slice());
    let size = bytes.len();
    if let Some(Fault::Status(s)) = fault {
        record(s, false);
        head(&mut out, &format!("{s} Fault"), "", 0);
        return;
    }
    let (mut start, mut end) = (0usize, size.saturating_sub(1));
    let mut partial = false;
    if let Some(r) = range.as_deref().and_then(|r| r.strip_prefix("bytes=")) {
        if !matches!(fault, Some(Fault::IgnoreRange)) {
            let mut it = r.splitn(2, '-');
            start = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
            if let Some(e) = it.next().and_then(|s| s.parse::<usize>().ok()) {
                end = e.min(size - 1);
            }
            partial = true;
        }
    }
    let total = report_total.map(|t| t.to_string()).unwrap_or_else(|| size.to_string());
    let cr_start = if matches!(fault, Some(Fault::ShiftRange)) { start.saturating_sub(1) } else { start };
    let n = end + 1 - start;
    if partial {
        record(206, false);
        head(
            &mut out,
            "206 Partial Content",
            &format!("Content-Range: bytes {cr_start}-{}/{total}\r\n", cr_start + n - 1),
            n,
        );
    } else {
        record(200, false);
        head(&mut out, "200 OK", "", n);
    }
    let slice = &bytes[start..=end];
    let limit = match fault {
        Some(Fault::DropAfter(k)) | Some(Fault::StallAfter(k)) => k.min(n),
        _ => n,
    };
    let step = trickle.unwrap_or(64 * 1024).max(1);
    let mut sent = 0;
    while sent < limit {
        if cut && now_ms() > exp {
            return;
        }
        let k = step.min(limit - sent);
        if out.write_all(&slice[sent..sent + k]).is_err() {
            return;
        }
        sent += k;
        if trickle.is_some() {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    if let Some(Fault::StallAfter(_)) = fault {
        let _ = out.flush();
        std::thread::sleep(Duration::from_secs(30));
    }
    // DropAfter: returning drops the socket before Content-Length bytes were sent.
}

/// A manager over the loopback server with fast test timings.
fn net_manager(dir: &std::path::Path, srv: &ModelServer, body: &[u8]) -> ModelManager {
    ModelManager::new(
        dir.to_path_buf(),
        Box::new(srv.transport(body.len() as u64)),
        sha_hex(body),
        body.len() as u64,
    )
    .with_segment_bytes(1 << 20)
    .with_retry_backoff(zero_backoff)
    .with_stall_timeout(Duration::from_millis(800))
    .with_free_space_probe(plenty_of_disk)
}

// ===========================================================================
// 1. Every attempt re-resolves from the ORIGIN; a signed CDN URL is never reused.
// ===========================================================================

#[test]
fn a_download_longer_than_the_signed_url_lifetime_re_resolves_and_finishes() {
    let body = body_of(600 * 1024);
    let srv = ModelServer::start(body.clone());
    // Signed URLs live 150 ms and are cut in flight when they expire; the transfer takes longer.
    srv.set(|c| {
        c.expiry = Some(Duration::from_millis(150));
        c.cut_at_expiry = true;
        c.trickle = Some(8 * 1024);
    });
    let dir = tmp_dir("expiry");
    let mgr = net_manager(&dir, &srv, &body).with_segment_bytes(64 * 1024);
    let t0 = Instant::now();
    mgr.download().expect("finishes across many signed-URL lifetimes");
    assert!(t0.elapsed() > Duration::from_millis(150), "the transfer outlasted one signed URL");
    mgr.verify().expect("byte-identical");
    let cdn = srv.cdn();
    assert!(cdn.iter().all(|s| !s.expired), "no request ever reused an expired signed URL: {cdn:?}");
    assert_eq!(srv.origin_hits(), cdn.len(), "every CDN request came from a fresh origin redirect");
}

#[test]
fn a_403_from_an_expired_cdn_hop_is_re_resolved_and_resumed_with_range() {
    let body = body_of(300 * 1024);
    let srv = ModelServer::start(body.clone());
    srv.set(|c| c.faults = VecDeque::from(vec![Fault::DropAfter(100 * 1024), Fault::Status(403), Fault::Status(410), Fault::Status(401)]));
    let dir = tmp_dir("cdn403");
    let mgr = net_manager(&dir, &srv, &body);
    mgr.download().expect("a 401/403/410 from the CDN hop is re-resolved, not final");
    mgr.verify().expect("byte-identical");
    let cdn = srv.cdn();
    let resumed = cdn.iter().skip(4).find_map(|s| s.range.clone()).unwrap_or_default();
    assert!(resumed.starts_with("bytes=102400-"), "resumed with Range from the bytes on disk: {cdn:?}");
}

#[test]
fn the_part_sidecar_records_the_pin_and_origin_never_a_signed_url() {
    let body = body_of(300 * 1024);
    let srv = ModelServer::start(body.clone());
    srv.set(|c| c.sticky = Some(Fault::Status(503)));
    srv.set(|c| c.faults = VecDeque::from(vec![Fault::DropAfter(64 * 1024)]));
    let dir = tmp_dir("sidecar");
    let mgr = net_manager(&dir, &srv, &body);
    assert!(mgr.download().is_err(), "the server keeps failing");
    let meta = read_part_meta(&dir, MODEL_FILE).expect("a sidecar beside the .part");
    assert_eq!(meta.size, body.len() as u64);
    assert_eq!(meta.sha256, sha_hex(&body));
    assert_eq!(meta.file, MODEL_FILE);
    let raw = std::fs::read_to_string(dir.join(format!("{MODEL_FILE}.part.json"))).expect("sidecar");
    assert!(!raw.contains("Expires") && !raw.contains("Signature") && !raw.contains("/cdn/"), "no signed URL persisted: {raw}");
    assert_eq!(std::fs::metadata(part_of(&dir)).expect("part").len(), 64 * 1024);
}

// ===========================================================================
// 2. Resume across retries and app restarts; Range must be honoured.
// ===========================================================================

#[test]
fn a_new_process_resumes_the_part_from_its_exact_size_and_verifies() {
    let body = body_of(400 * 1024);
    let srv = ModelServer::start(body.clone());
    let dir = tmp_dir("restart");
    // "Process 1" gets 128 KiB and then the network goes away for good: it gives up.
    srv.set(|c| {
        c.faults = VecDeque::from(vec![Fault::DropAfter(128 * 1024)]);
        c.sticky = Some(Fault::Status(503));
    });
    {
        let mgr = net_manager(&dir, &srv, &body);
        assert!(mgr.download().is_err());
    }
    assert_eq!(std::fs::metadata(part_of(&dir)).expect("part kept").len(), 128 * 1024);
    // "Process 2": a fresh manager (fresh process state) over the same dir, network back.
    srv.set(|c| c.sticky = None);
    let before = srv.cdn().len();
    let mgr = net_manager(&dir, &srv, &body);
    assert!(matches!(mgr.status(), ModelStatus::Downloading { downloaded_bytes: 131072, .. }));
    mgr.download().expect("resumes");
    let first = srv.cdn()[before].range.clone().unwrap_or_default();
    assert!(first.starts_with("bytes=131072-"), "first request after restart resumes at the part size: {first}");
    mgr.verify().expect("byte-identical after a restart");
    assert!(read_part_meta(&dir, MODEL_FILE).is_none(), "the sidecar goes with the .part");
}

#[test]
fn a_part_left_for_a_different_pin_is_discarded_and_restarted_from_zero() {
    let body = body_of(200 * 1024);
    let srv = ModelServer::start(body.clone());
    let dir = tmp_dir("stale-pin");
    std::fs::write(part_of(&dir), vec![7u8; 50 * 1024]).expect("stale part");
    write_part_meta(
        &dir,
        &PartMeta::new(MODEL_FILE, 999_999, "00".repeat(32), vec!["https://old.example/model".into()]),
    )
    .expect("stale sidecar");
    let mgr = net_manager(&dir, &srv, &body);
    mgr.download().expect("a fresh download");
    let first = srv.cdn()[0].range.clone().unwrap_or_default();
    assert!(first.starts_with("bytes=0-"), "restarted from zero, not resumed onto another file: {first}");
    mgr.verify().expect("the new file verifies");
}

#[test]
fn a_legacy_part_without_a_sidecar_is_resumed() {
    let body = body_of(200 * 1024);
    let srv = ModelServer::start(body.clone());
    let dir = tmp_dir("legacy-part");
    std::fs::write(part_of(&dir), &body[..70 * 1024]).expect("legacy part");
    let mgr = net_manager(&dir, &srv, &body);
    mgr.download().expect("resumes a part written by an older app");
    assert!(srv.cdn()[0].range.as_deref().unwrap_or("").starts_with("bytes=71680-"));
    mgr.verify().expect("verifies");
}

#[test]
fn a_206_whose_content_range_starts_elsewhere_is_refused_before_appending() {
    let body = body_of(200 * 1024);
    let srv = ModelServer::start(body.clone());
    srv.set(|c| c.sticky = Some(Fault::ShiftRange));
    let dir = tmp_dir("shift");
    std::fs::write(part_of(&dir), &body[..1000]).expect("part");
    let mgr = net_manager(&dir, &srv, &body);
    let r = mgr.download();
    assert!(matches!(r, Err(ModelError::RangeIgnored)), "got {r:?}");
    assert_eq!(std::fs::metadata(part_of(&dir)).expect("part").len(), 1000, "nothing appended");
}

#[test]
fn a_resume_answered_200_is_refused_and_the_next_source_resumes() {
    let body = body_of(200 * 1024);
    let bad = ModelServer::start(body.clone());
    bad.set(|c| c.sticky = Some(Fault::IgnoreRange));
    let good = ModelServer::start(body.clone());
    let dir = tmp_dir("ignore-range");
    std::fs::write(part_of(&dir), &body[..1000]).expect("part");
    let sources: Vec<Box<dyn ModelTransport>> = vec![
        Box::new(bad.transport(body.len() as u64)),
        Box::new(good.transport(body.len() as u64)),
    ];
    download_with(&dir, sources, &test_opts(&body)).expect("the second source resumes");
    assert_eq!(std::fs::read(dir.join(MODEL_FILE)).expect("file"), body);
}

#[test]
fn a_source_that_serves_a_different_size_is_skipped() {
    let body = body_of(100 * 1024);
    let other = ModelServer::start(body.clone());
    other.set(|c| c.report_total = Some(5));
    let good = ModelServer::start(body.clone());
    let dir = tmp_dir("wrong-size");
    let sources: Vec<Box<dyn ModelTransport>> = vec![
        Box::new(other.transport(body.len() as u64)),
        Box::new(good.transport(body.len() as u64)),
    ];
    download_with(&dir, sources, &test_opts(&body)).expect("the right file comes from the next source");
    assert_eq!(other.cdn().len(), 1, "the wrong-size source is asked once, not retried");
}

#[test]
fn a_source_that_answers_404_is_skipped_at_once() {
    let body = body_of(100 * 1024);
    let missing = ModelServer::start(body.clone());
    missing.set(|c| c.origin_404 = true);
    let good = ModelServer::start(body.clone());
    let dir = tmp_dir("404");
    let sources: Vec<Box<dyn ModelTransport>> = vec![
        Box::new(missing.transport(body.len() as u64)),
        Box::new(good.transport(body.len() as u64)),
    ];
    let t0 = Instant::now();
    download_with(&dir, sources, &ModelDownloadOptions { retry_backoff: |_| Duration::from_secs(2), ..test_opts(&body) })
        .expect("the next source serves it");
    assert_eq!(missing.origin_hits(), 1, "a missing object is not retried");
    assert!(t0.elapsed() < Duration::from_secs(2), "no backoff was spent on it");
}

// ===========================================================================
// 3. Stall detection; no wall-clock limit on a download that is moving.
// ===========================================================================

#[test]
fn a_stalled_body_is_abandoned_after_the_stall_timeout_and_resumed() {
    let body = body_of(300 * 1024);
    let srv = ModelServer::start(body.clone());
    srv.set(|c| c.faults = VecDeque::from(vec![Fault::StallAfter(20 * 1024)]));
    let dir = tmp_dir("stall");
    let mgr = net_manager(&dir, &srv, &body);
    let t0 = Instant::now();
    mgr.download().expect("resumes after the stall");
    assert!(t0.elapsed() < Duration::from_secs(10), "the stall cost about one stall timeout, took {:?}", t0.elapsed());
    let second = srv.cdn()[1].range.clone().unwrap_or_default();
    assert!(second.starts_with("bytes=20480-"), "resumed at the byte the stall stopped at: {second}");
    mgr.verify().expect("byte-identical");
}

#[test]
fn a_slow_download_that_keeps_moving_is_never_cut() {
    let body = body_of(64 * 1024);
    let srv = ModelServer::start(body.clone());
    // 512 bytes every 10 ms: the one segment takes over a second, longer than the stall timeout,
    // but bytes keep arriving, so it is never abandoned.
    srv.set(|c| c.trickle = Some(512));
    let dir = tmp_dir("slow");
    let mgr = net_manager(&dir, &srv, &body).with_segment_bytes(1 << 20);
    mgr.download().expect("a slow but moving download finishes");
    assert_eq!(srv.cdn().len(), 1, "one request, never cut and restarted");
}

#[test]
fn a_connection_dropped_mid_body_resumes_at_the_exact_byte() {
    let body = body_of(300 * 1024);
    let srv = ModelServer::start(body.clone());
    srv.set(|c| c.faults = VecDeque::from(vec![Fault::DropAfter(10_000), Fault::DropAfter(5), Fault::DropAfter(0)]));
    let dir = tmp_dir("drop");
    let mgr = net_manager(&dir, &srv, &body);
    mgr.download().expect("drops are retried");
    mgr.verify().expect("byte-identical");
    assert!(srv.cdn()[1].range.as_deref().unwrap_or("").starts_with("bytes=10000-"));
}

// ===========================================================================
// Retries: bounded, but an outage longer than one source's retries still finishes.
// ===========================================================================

#[test]
fn an_outage_longer_than_one_pass_over_the_sources_still_finishes() {
    let body = body_of(100 * 1024);
    let a = ModelServer::start(body.clone());
    let b = ModelServer::start(body.clone());
    // Each source fails more times in a row than one source's retry budget, then recovers.
    let fails = MAX_SEGMENT_RETRIES as usize + 3;
    for s in [&a, &b] {
        s.set(|c| c.faults = (0..fails).map(|_| Fault::Status(503)).collect());
    }
    let dir = tmp_dir("long-outage");
    let sources: Vec<Box<dyn ModelTransport>> = vec![
        Box::new(a.transport(body.len() as u64)),
        Box::new(b.transport(body.len() as u64)),
    ];
    download_with(&dir, sources, &test_opts(&body)).expect("the download outlasts the outage");
    assert_eq!(std::fs::read(dir.join(MODEL_FILE)).expect("file"), body);
}

#[test]
fn an_outage_that_never_ends_gives_up_after_bounded_passes_and_keeps_the_part() {
    let body = body_of(100 * 1024);
    let a = ModelServer::start(body.clone());
    a.set(|c| {
        c.faults = VecDeque::from(vec![Fault::DropAfter(4096)]);
        c.sticky = Some(Fault::Status(503));
    });
    let dir = tmp_dir("endless-outage");
    let sources: Vec<Box<dyn ModelTransport>> = vec![Box::new(a.transport(body.len() as u64))];
    let r = download_with(&dir, sources, &ModelDownloadOptions { max_passes: 3, ..test_opts(&body) });
    assert!(matches!(r, Err(ModelError::Transport(_))), "got {r:?}");
    // The first pass gains 4096 bytes (its drop, then its retries); three passes in a row then
    // gain nothing, and the download stops. Each pass is 1 + MAX_SEGMENT_RETRIES attempts.
    assert_eq!(a.cdn().len(), 4 * (1 + MAX_SEGMENT_RETRIES as usize));
    assert_eq!(std::fs::metadata(part_of(&dir)).expect("part").len(), 4096);
}

#[test]
fn the_apps_own_download_never_gives_up_on_a_lost_connection_and_resumes_when_it_returns() {
    let body = body_of(100 * 1024);
    let a = ModelServer::start(body.clone());
    a.set(|c| c.sticky = Some(Fault::Status(503)));
    let dir = tmp_dir("unbounded");
    let opts = ModelDownloadOptions { max_passes: u32::MAX, pass_backoff: |_| Duration::from_millis(50), ..test_opts(&body) };
    assert_eq!(ModelDownloadOptions::for_pin(&DEFAULT_PIN, vec![]).max_passes, u32::MAX, "production has no bound");
    let srv = Arc::new(a);
    let s2 = srv.clone();
    // The "network" comes back after many passes.
    let healer = std::thread::spawn(move || {
        let t0 = Instant::now();
        while s2.cdn().len() < 5 * (1 + MAX_SEGMENT_RETRIES as usize) {
            assert!(t0.elapsed() < Duration::from_secs(30));
            std::thread::sleep(Duration::from_millis(5));
        }
        s2.set(|c| c.sticky = None);
    });
    let sources: Vec<Box<dyn ModelTransport>> = vec![Box::new(srv.transport(body.len() as u64))];
    download_with(&dir, sources, &opts).expect("keeps waiting, then finishes");
    healer.join().expect("healer");
    assert_eq!(std::fs::read(dir.join(MODEL_FILE)).expect("file"), body);
}

#[test]
fn while_waiting_for_the_connection_status_says_so_and_a_pause_is_taken_and_kept() {
    let body = body_of(100 * 1024);
    let a = ModelServer::start(body.clone());
    a.set(|c| {
        c.faults = VecDeque::from(vec![Fault::DropAfter(2048)]);
        c.sticky = Some(Fault::Status(503));
    });
    let dir = tmp_dir("wait-pause");
    let part = part_of(&dir);
    let opts = ModelDownloadOptions { max_passes: u32::MAX, pass_backoff: |_| Duration::from_secs(60), ..test_opts(&body) };
    let d2 = dir.clone();
    let tr = a.transport(body.len() as u64);
    let h = std::thread::spawn(move || {
        let sources: Vec<Box<dyn ModelTransport>> = vec![Box::new(tr)];
        download_with(&d2, sources, &opts)
    });
    let mgr = net_manager(&dir, &a, &body);
    let t0 = Instant::now();
    loop {
        if let ModelStatus::Downloading { retry_in_secs: Some(n), last_error: Some(_), .. } = mgr.status() {
            if n > 10 {
                break; // the 60 s wait between passes
            }
        }
        assert!(t0.elapsed() < Duration::from_secs(20), "no waiting status: {:?}", mgr.status());
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(request_pause(&part), "a waiting download takes the pause");
    let r = h.join().expect("join");
    assert!(matches!(r, Err(ModelError::Paused)), "got {r:?}");
    assert!(matches!(mgr.status(), ModelStatus::Paused { downloaded_bytes: 2048, .. }), "{:?}", mgr.status());
}

// ===========================================================================
// 4. Disk space.
// ===========================================================================

fn one_mib_free(_: &std::path::Path) -> Option<u64> {
    Some(1 << 20)
}

#[test]
fn a_disk_without_room_is_refused_before_any_byte_with_a_clear_message() {
    let body = body_of(64 * 1024);
    let srv = ModelServer::start(body.clone());
    let dir = tmp_dir("nodisk");
    let mgr = ModelManager::new(dir.clone(), Box::new(srv.transport(body.len() as u64)), sha_hex(&body), 5 << 30)
        .with_free_space_probe(one_mib_free);
    let r = mgr.download();
    let Err(ModelError::DiskFull { need, free }) = r else { panic!("expected DiskFull, got {r:?}") };
    assert_eq!(free, 1 << 20);
    assert!(need >= 5 << 30, "needs the file plus a margin: {need}");
    assert!(srv.seen().is_empty(), "nothing was requested");
    assert!(!part_of(&dir).exists(), "no .part created");
    let msg = ModelError::DiskFull { need, free }.user_message();
    assert!(msg.contains("free disk space") && msg.contains("GB"), "{msg}");
    assert!(msg.contains("smaller model"), "points at the smaller model: {msg}");
}

#[test]
fn the_disk_check_counts_the_bytes_already_downloaded() {
    let body = body_of(4 << 20);
    let srv = ModelServer::start(body.clone());
    let dir = tmp_dir("disk-partial");
    std::fs::write(part_of(&dir), &body[..(3 << 20) + 512]).expect("part");
    // 1 MiB free + the margin would not fit the whole file, but it fits what is left.
    fn left_plus_margin(_: &std::path::Path) -> Option<u64> {
        Some((1 << 20) + DISK_MARGIN_BYTES)
    }
    let mgr = net_manager(&dir, &srv, &body).with_free_space_probe(left_plus_margin);
    mgr.download().expect("what is left fits");
}

#[test]
fn a_full_disk_while_writing_is_reported_as_disk_full_and_the_part_is_kept() {
    let e = io_to_model_error(std::io::Error::from_raw_os_error(libc::ENOSPC), 10, Some(3));
    assert!(matches!(e, ModelError::DiskFull { need: 10, free: 3 }), "got {e:?}");
    let other = io_to_model_error(std::io::Error::other("boom"), 10, Some(3));
    assert!(matches!(other, ModelError::Io(_)));
}

// ===========================================================================
// 6. Pause / resume; live progress with speed and ETA.
// ===========================================================================

#[test]
fn pause_stops_the_download_keeps_the_part_and_resume_continues() {
    let body = body_of(400 * 1024);
    let srv = ModelServer::start(body.clone());
    srv.set(|c| c.trickle = Some(4096));
    let dir = tmp_dir("pause");
    let part = part_of(&dir);
    let mgr = net_manager(&dir, &srv, &body);
    let p2 = part.clone();
    let pauser = std::thread::spawn(move || {
        let t0 = Instant::now();
        while std::fs::metadata(&p2).map(|m| m.len()).unwrap_or(0) < 32 * 1024 {
            assert!(t0.elapsed() < Duration::from_secs(20), "download never started");
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(request_pause(&p2), "a running download takes the pause");
    });
    let r = mgr.download();
    pauser.join().expect("pauser");
    assert!(matches!(r, Err(ModelError::Paused)), "got {r:?}");
    let have = std::fs::metadata(&part).expect("part kept").len();
    assert!(have >= 32 * 1024 && have < body.len() as u64);
    assert!(matches!(mgr.status(), ModelStatus::Paused { downloaded_bytes, .. } if downloaded_bytes == have));
    assert!(!request_pause(&part), "nothing to pause once stopped");
    srv.set(|c| c.trickle = None);
    mgr.download().expect("resume continues");
    mgr.verify().expect("byte-identical");
}

#[test]
fn status_while_downloading_reports_speed_and_eta() {
    let body = body_of(600 * 1024);
    let srv = ModelServer::start(body.clone());
    srv.set(|c| c.trickle = Some(4096));
    let dir = tmp_dir("speed");
    let mgr = Arc::new(net_manager(&dir, &srv, &body));
    let m2 = mgr.clone();
    let h = std::thread::spawn(move || m2.download());
    let t0 = Instant::now();
    let mut seen = None;
    while t0.elapsed() < Duration::from_secs(20) {
        if let ModelStatus::Downloading { bytes_per_sec: Some(bps), eta_secs: Some(eta), .. } = mgr.status() {
            seen = Some((bps, eta));
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = request_pause(&part_of(&dir));
    let _ = h.join();
    let (bps, _eta) = seen.expect("a speed and an ETA while bytes flow");
    assert!(bps > 0.0);
}

#[test]
fn status_while_waiting_to_retry_says_why() {
    let body = body_of(64 * 1024);
    let srv = ModelServer::start(body.clone());
    srv.set(|c| c.faults = VecDeque::from(vec![Fault::Status(503)]));
    let dir = tmp_dir("retry-why");
    let mgr = Arc::new(net_manager(&dir, &srv, &body).with_retry_backoff(|_| Duration::from_secs(3)));
    let m2 = mgr.clone();
    let h = std::thread::spawn(move || m2.download());
    let t0 = Instant::now();
    let mut why = None;
    while t0.elapsed() < Duration::from_secs(10) {
        if let ModelStatus::Downloading { retry_in_secs: Some(_), last_error: Some(e), .. } = mgr.status() {
            why = Some(e);
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    h.join().expect("join").expect("finishes after the wait");
    assert!(why.expect("a retry notice").contains("503"));
}

// ===========================================================================
// Integrity: kept from #757, plus one clean retry from zero.
// ===========================================================================

#[test]
fn wrong_bytes_from_a_source_fail_verify_and_one_clean_retry_from_zero_heals_it() {
    let body = body_of(100 * 1024);
    let mut wrong = body.clone();
    for b in wrong.iter_mut().skip(4) {
        *b ^= 0x5A;
    }
    let srv = ModelServer::start(body.clone());
    srv.set(|c| c.wrong_body = Some(Arc::new(wrong)));
    let dir = tmp_dir("wrong-bytes");
    let srv2 = &srv;
    let mut attempt = 0;
    let r = download_and_verify_with_clean_retry(&dir, MODEL_FILE, &sha_hex(&body), body.len() as u64, || {
        attempt += 1;
        if attempt == 2 {
            srv2.set(|c| c.wrong_body = None);
        }
        let sources: Vec<Box<dyn ModelTransport>> = vec![Box::new(srv2.transport(body.len() as u64))];
        download_with(&dir, sources, &test_opts(&body))
    });
    r.expect("the second, clean download verifies");
    assert_eq!(attempt, 2, "exactly one clean retry");
    assert!(is_file_ready(&dir, MODEL_FILE));
}

#[test]
fn a_second_checksum_mismatch_is_final_and_nothing_is_ready() {
    let body = body_of(64 * 1024);
    let mut wrong = body.clone();
    wrong[100] ^= 1;
    let srv = ModelServer::start(body.clone());
    srv.set(|c| c.wrong_body = Some(Arc::new(wrong)));
    let dir = tmp_dir("wrong-twice");
    let mut attempts = 0;
    let r = download_and_verify_with_clean_retry(&dir, MODEL_FILE, &sha_hex(&body), body.len() as u64, || {
        attempts += 1;
        let sources: Vec<Box<dyn ModelTransport>> = vec![Box::new(srv.transport(body.len() as u64))];
        download_with(&dir, sources, &test_opts(&body))
    });
    assert!(matches!(r, Err(ModelError::ChecksumMismatch)), "got {r:?}");
    assert_eq!(attempts, 2);
    assert!(!dir.join(MODEL_FILE).exists() && !is_file_ready(&dir, MODEL_FILE));
}

#[test]
fn a_web_page_from_the_origin_hands_over_to_the_next_source() {
    let body = body_of(64 * 1024);
    let page = ModelServer::start(body.clone());
    page.set(|c| c.origin_html = true);
    let good = ModelServer::start(body.clone());
    let dir = tmp_dir("html");
    let sources: Vec<Box<dyn ModelTransport>> = vec![
        Box::new(page.transport(body.len() as u64)),
        Box::new(good.transport(body.len() as u64)),
    ];
    download_with(&dir, sources, &test_opts(&body)).expect("next source");
    assert_eq!(std::fs::read(dir.join(MODEL_FILE)).expect("file"), body);
}

fn test_opts(body: &[u8]) -> ModelDownloadOptions {
    ModelDownloadOptions {
        file: MODEL_FILE.to_string(),
        expected_hash: sha_hex(body),
        expected_size: body.len() as u64,
        segment_bytes: 64 * 1024,
        retry_backoff: zero_backoff,
        stall_timeout: Duration::from_millis(800),
        free_space: plenty_of_disk,
        max_passes: MAX_SOURCE_PASSES,
        pass_backoff: zero_backoff,
        source_labels: Vec::new(),
    }
}

// ===========================================================================
// 5. Sources in order: Citrate mirror, citrate.ai, Hugging Face (pinned revision). Smaller model.
// ===========================================================================

#[test]
fn sources_are_the_mirror_then_the_vanity_redirect_then_hugging_face_at_a_pinned_revision() {
    let with_mirror = PinnedModel { mirror_urls: &["https://mirror.example/m.gguf"], ..DEFAULT_PIN };
    assert_eq!(
        with_mirror.source_urls(),
        vec![
            "https://mirror.example/m.gguf".to_string(),
            DEFAULT_MODEL_URL.to_string(),
            "https://huggingface.co/ggml-org/gemma-4-E4B-it-GGUF/resolve/8903cb408c7df39cde2ed15fac397e6d7be92951/gemma-4-E4B-it-Q4_0.gguf".to_string(),
        ]
    );
    // Today no Citrate mirror is published (it needs the owner's sign-off), so the default list is
    // the vanity redirect and Hugging Face.
    assert_eq!(DEFAULT_PIN.source_urls().len(), DEFAULT_PIN.mirror_urls.len() + 2);
    assert_eq!(model_source_urls(None), DEFAULT_PIN.source_urls());
    for u in DEFAULT_PIN.source_urls().iter().chain(SMALL_PIN.source_urls().iter()) {
        assert!(u.starts_with("https://"), "{u}");
        assert!(!u.contains("Expires=") && !u.contains("Signature="), "never a signed URL: {u}");
    }
}

#[test]
fn the_smaller_model_is_pinned_and_is_not_the_default() {
    assert_eq!(SMALL_PIN.file, "gemma-4-E2B-it-Q4_0.gguf");
    assert_eq!(SMALL_PIN.size_bytes, 2_841_481_184);
    assert_eq!(SMALL_PIN.sha256, "8e30dff3ac4c8434c49a7036fa15564bdbb6044e42bf04550bf1a096ad7e6a52");
    assert_eq!(
        SMALL_PIN.source_urls().last().map(String::as_str),
        Some("https://huggingface.co/ggml-org/gemma-4-E2B-it-GGUF/resolve/2a8538c2e2d4687953a86c422c7c623cfd022880/gemma-4-E2B-it-Q4_0.gguf")
    );
    assert_eq!(LocalModel::default(), LocalModel::Default);
    assert_eq!(LocalModel::Default.pin().file, MODEL_FILE);
    assert_eq!(LocalModel::Default.pin().sha256, MODEL_SHA256);
    assert_eq!(LocalModel::Small.pin().file, SMALL_PIN.file);
}

#[test]
fn the_local_model_choice_persists_and_picks_the_file_status_reads() {
    let dir = tmp_dir("choice");
    assert_eq!(read_local_choice(&dir), LocalModel::Default, "no choice file means the default");
    write_local_choice(&dir, LocalModel::Small).expect("write");
    assert_eq!(read_local_choice(&dir), LocalModel::Small);
    let st = ModelState::new(dir.clone());
    assert_eq!(st.choice(), LocalModel::Small);
    std::fs::write(dir.join(format!("{}.part", SMALL_PIN.file)), [0u8; 10]).expect("part");
    assert!(matches!(st.status(), ModelStatus::Downloading { downloaded_bytes: 10, total_bytes: 2_841_481_184, .. }));
    st.set_choice(LocalModel::Default).expect("switch back");
    assert!(matches!(st.status(), ModelStatus::NotPresent));
    assert_eq!(read_local_choice(&dir), LocalModel::Default);
}

// ===========================================================================
// 7. The download log: rotated, and never a signed URL.
// ===========================================================================

#[test]
fn the_download_log_rotates_and_never_holds_a_signed_url_or_query() {
    let dir = tmp_dir("log");
    let path = dir.join("model-download.log");
    let mut log = crate::model_log::ModelLog::open(path.clone(), 64 * 1024, 3);
    log.event("attempt failed at https://us.aws.cdn.hf.co/xet-bridge-us/abc?Expires=1&Signature=s3cr3t&Key-Pair-Id=K");
    let text = std::fs::read_to_string(&path).expect("log");
    assert!(text.contains("us.aws.cdn.hf.co"), "{text}");
    assert!(!text.contains("Signature") && !text.contains("s3cr3t") && !text.contains("Expires"), "{text}");
    for i in 0..20_000 {
        log.event(&format!("line {i} {}", "x".repeat(40)));
    }
    assert!(path.exists());
    assert!(dir.join("model-download.log.3").exists(), "rotated files kept");
    assert!(!dir.join("model-download.log.4").exists(), "at most three rotated files");
    assert!(std::fs::metadata(&path).expect("log").len() <= 64 * 1024);
}

#[test]
fn user_messages_are_plain_words() {
    for e in [
        ModelError::Transport("connection refused".into()),
        ModelError::Stalled { secs: 45 },
        ModelError::ChecksumMismatch,
        ModelError::BadMagic,
        ModelError::RangeIgnored,
        ModelError::DiskFull { need: 5 << 30, free: 1 << 30 },
        ModelError::Paused,
    ] {
        let m = e.user_message();
        assert!(!m.contains('\u{2014}'), "no em-dash: {m}");
        assert!(!m.to_lowercase().contains("magic") && !m.contains("206"), "plain words: {m}");
        assert!(m.ends_with('.'), "a sentence: {m}");
    }
}
