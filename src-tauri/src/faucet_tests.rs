// HUP-S6.5 (core half): faucet_request tests. Included from `faucet.rs` (`mod tests`).

use super::*;
use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex};

const WALLET: &str = "0x00000000000000000000000000000000000000aa";
const OTHER: &str = "0x00000000000000000000000000000000000000bb";
const BASE: &str = "https://faucet.example";
const HOUR_MS: u64 = 3_600_000;

fn hash(b: u8) -> String {
    format!("0x{}", hex::encode([b; 32]))
}

/// Scripted HTTP: canned GET answers by URL, one canned POST answer, and a log of every POST.
#[derive(Default)]
struct Script {
    gets: HashMap<String, Result<(u16, String), String>>,
    post_reply: Option<Result<(u16, String), String>>,
    posts: StdMutex<Vec<(String, Value)>>,
}

impl FaucetHttp for Script {
    fn get(&self, url: &str) -> Result<(u16, String), String> {
        self.gets
            .get(url)
            .cloned()
            .unwrap_or_else(|| Err(format!("no route {url}")))
    }
    fn post_json(&self, url: &str, body: &Value) -> Result<(u16, String), String> {
        self.posts
            .lock()
            .expect("posts lock")
            .push((url.to_string(), body.clone()));
        self.post_reply
            .clone()
            .unwrap_or_else(|| Err("no post reply".to_string()))
    }
}

impl Script {
    fn posting(reply: Result<(u16, String), String>) -> Self {
        Script {
            post_reply: Some(reply),
            ..Default::default()
        }
    }
    fn post_count(&self) -> usize {
        self.posts.lock().expect("posts lock").len()
    }
}

struct Chain {
    balance: u128,
    gas_price: u128,
}

impl ChainReads for Chain {
    fn balance_wei(&self, _address: &str) -> Result<u128, String> {
        Ok(self.balance)
    }
    fn gas_price_wei(&self) -> Result<u128, String> {
        Ok(self.gas_price)
    }
}

const POOR: Chain = Chain {
    balance: 0,
    gas_price: 1_000_000_000,
};

fn sent_body() -> String {
    json!({"success": true, "tx_hash": hash(0x5e), "message": "Successfully sent 10 SALT", "amount": "10000000000000000000"}).to_string()
}

/// A scratch dir removed on drop (no extra dev-dependency).
struct TempDir(std::path::PathBuf);

impl TempDir {
    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn temp_store() -> (TempDir, FaucetStore) {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "citrate-faucet-test-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&dir).expect("tempdir");
    let store = FaucetStore::new(Some(dir.join(STATE_FILE_NAME)));
    (TempDir(dir), store)
}

/// A real HIC outbox on a scratch folder, shared by the tests that do not inspect records.
fn rec() -> &'static crate::hic_records::HicOutbox {
    static OUTBOX: std::sync::OnceLock<crate::hic_records::HicOutbox> = std::sync::OnceLock::new();
    OUTBOX.get_or_init(|| {
        crate::hic_records::HicOutbox::new(std::env::temp_dir().join(format!(
            "citrate-faucet-test-records-{}",
            std::process::id()
        )))
    })
}

fn granted(store: &FaucetStore) {
    grant(store, rec(), WALLET, 1_000).expect("grant");
}

fn ctx<'a>(initcode_hash: &'a str, now_ms: u64) -> RequestCtx<'a> {
    RequestCtx {
        wallet: WALLET,
        origin: "hermes",
        initcode_hash,
        deploy_ready: true,
        base: BASE,
        now_ms,
    }
}

// ------------------------------------------------------------------ pure rules

#[test]
fn defaults_change_nothing_the_book_starts_off() {
    let book = FaucetBook::default();
    assert!(book.budget.is_none());
    assert_eq!(gate(&book, WALLET, true, 0, 1, 0), Gate::Disabled);
    let parsed: FaucetBook = serde_json::from_str("{}").expect("empty file parses");
    assert_eq!(parsed, FaucetBook::default());
}

#[test]
fn placeholders_are_marked_pending_owner_sign_off() {
    assert_eq!(MEMBER_WINDOW_MS, 24 * HOUR_MS);
    assert_eq!(MAX_PER_WINDOW, 1);
    assert_eq!(DEPLOY_GAS_LIMIT, crate::contract_deploy::DEFAULT_DEPLOY_GAS);
    assert_eq!(PENDING_OWNER_SIGN_OFF.len(), 5);
    for line in PENDING_OWNER_SIGN_OFF {
        assert!(line.contains("Pending owner sign-off"), "{line}");
        assert!(!line.contains('\u{2014}'), "no em-dashes in member prose: {line}");
    }
}

#[test]
fn faucet_url_override_is_https_or_loopback_only() {
    assert_eq!(faucet_base_url(None), DEFAULT_FAUCET_URL);
    assert_eq!(faucet_base_url(Some("  ")), DEFAULT_FAUCET_URL);
    assert_eq!(faucet_base_url(Some("https://f.example/")), "https://f.example");
    assert_eq!(faucet_base_url(Some("http://127.0.0.1:3002")), "http://127.0.0.1:3002");
    assert_eq!(faucet_base_url(Some("http://localhost:3002")), "http://localhost:3002");
    for bad in [
        "http://faucet.example",
        "ftp://x.example",
        "https://u:p@x.example",
        "https://x.example/path",
        "https://x.example/?q=1",
        "not a url",
    ] {
        assert_eq!(faucet_base_url(Some(bad)), DEFAULT_FAUCET_URL, "{bad}");
    }
}

#[test]
fn addresses_and_hashes_are_validated() {
    assert_eq!(
        normalize_address("0x00000000000000000000000000000000000000AA").expect("ok"),
        WALLET
    );
    assert!(normalize_address("00000000000000000000000000000000000000aa").is_err());
    assert!(normalize_address("0x1234").is_err());
    assert_eq!(normalize_hash(&hash(0xAB).to_uppercase().replacen("0X", "0x", 1)).expect("ok"), hash(0xab));
    assert!(normalize_hash("0x12").is_err());
    assert!(normalize_hash(&hash(1)[2..]).is_err());
}

#[test]
fn need_is_gas_limit_times_price_and_saturates() {
    assert_eq!(need_wei(2_000_000, 1_000_000_000), 2_000_000_000_000_000);
    assert_eq!(need_wei(u64::MAX, u128::MAX), u128::MAX);
}

#[test]
fn gate_order_disabled_wallet_deploy_need_window() {
    let mut book = FaucetBook::default();
    assert_eq!(gate(&book, WALLET, true, 0, 10, 0), Gate::Disabled);
    book.budget = Some(FaucetBudget {
        wallet: OTHER.to_string(),
        granted_at_ms: 0,
        window_ms: MEMBER_WINDOW_MS,
        max_per_window: 1,
    });
    assert_eq!(
        gate(&book, WALLET, true, 0, 10, 0),
        Gate::WalletChanged {
            granted_for: OTHER.to_string()
        }
    );
    book.budget = Some(FaucetBudget {
        wallet: WALLET.to_string(),
        granted_at_ms: 0,
        window_ms: MEMBER_WINDOW_MS,
        max_per_window: 1,
    });
    assert_eq!(gate(&book, WALLET, false, 0, 10, 0), Gate::NoPendingDeploy);
    assert_eq!(
        gate(&book, WALLET, true, 10, 10, 0),
        Gate::NotNeeded {
            balance_wei: "10".into(),
            need_wei: "10".into()
        }
    );
    assert_eq!(gate(&book, WALLET, true, 9, 10, 0), Gate::Go);
}

fn entry(at_ms: u64, outcome: Outcome, next: Option<u64>) -> LedgerEntry {
    LedgerEntry {
        at_ms,
        wallet: WALLET.to_string(),
        origin: "local-user".into(),
        initcode_hash: None,
        need_wei: None,
        balance_wei: None,
        outcome,
        tx_hash: None,
        message: String::new(),
        next_eligible_at_ms: next,
    }
}

#[test]
fn only_sent_or_unknown_use_the_window() {
    for (o, consumes) in [
        (Outcome::Sent, true),
        (Outcome::Unknown, true),
        (Outcome::RateLimited, false),
        (Outcome::ChallengeRequired, false),
        (Outcome::Refused, false),
        (Outcome::Unreachable, false),
    ] {
        assert_eq!(o.consumes_window(), consumes, "{o:?}");
        let book = FaucetBook {
            budget: None,
            ledger: vec![entry(100, o, None)],
        };
        assert_eq!(next_eligible_ms(&book, WALLET, 200).is_some(), consumes, "{o:?}");
    }
}

#[test]
fn the_window_reopens_after_24_hours_and_is_per_wallet() {
    let book = FaucetBook {
        budget: None,
        ledger: vec![entry(1_000, Outcome::Sent, None)],
    };
    let (t, _) = next_eligible_ms(&book, WALLET, 2_000).expect("waiting");
    assert_eq!(t, 1_000 + MEMBER_WINDOW_MS);
    assert!(next_eligible_ms(&book, WALLET, 1_000 + MEMBER_WINDOW_MS).is_none());
    assert!(next_eligible_ms(&book, OTHER, 2_000).is_none(), "another wallet is not affected");
}

#[test]
fn the_faucets_next_time_is_respected_even_without_a_drip() {
    let book = FaucetBook {
        budget: None,
        ledger: vec![entry(1_000, Outcome::RateLimited, Some(50_000))],
    };
    let (t, reason) = next_eligible_ms(&book, WALLET, 2_000).expect("waiting");
    assert_eq!(t, 50_000);
    assert!(reason.contains("faucet asked"));
    assert!(next_eligible_ms(&book, WALLET, 50_000).is_none());
}

// ------------------------------------------------------------------ reply interpretation

#[test]
fn a_success_needs_a_real_tx_hash() {
    let ok = interpret_reply(200, &sent_body(), 0);
    assert_eq!(ok.outcome, Outcome::Sent);
    assert_eq!(ok.tx_hash, Some(hash(0x5e)));
    let no_hash = interpret_reply(200, r#"{"success":true,"tx_hash":null,"message":"x"}"#, 0);
    assert_eq!(no_hash.outcome, Outcome::Unknown);
    let bad_hash = interpret_reply(200, r#"{"success":true,"tx_hash":"0x12"}"#, 0);
    assert_eq!(bad_hash.outcome, Outcome::Unknown);
}

#[test]
fn rate_limits_new_and_old_faucets() {
    let new = json!({"success": false, "message": "Rate limited: address cooldown: 23h 5m remaining",
        "amount": "0", "code": "rate_limited", "limit": "address", "retry_after_secs": 83_100,
        "next_eligible_at": 1_900_000_000u64});
    let r = interpret_reply(200, &new.to_string(), 5);
    assert_eq!(r.outcome, Outcome::RateLimited);
    assert_eq!(r.next_eligible_at_ms, Some(1_900_000_000_000));

    let old = json!({"success": false, "tx_hash": null, "amount": "0",
        "message": "Rate limited: address cooldown: 23h 5m remaining"});
    let r = interpret_reply(200, &old.to_string(), 1_000);
    assert_eq!(r.outcome, Outcome::RateLimited);
    assert_eq!(r.next_eligible_at_ms, Some(1_000 + (23 * 3600 + 5 * 60) * 1000));

    let ip_old = json!({"success": false, "message": "Rate limited: ip cooldown: 0h 40m remaining"});
    let r = interpret_reply(200, &ip_old.to_string(), 0);
    assert_eq!(r.next_eligible_at_ms, Some(40 * 60 * 1000));
}

#[test]
fn captcha_unknown_refused_and_transport_shapes() {
    let c = interpret_reply(200, r#"{"success":false,"code":"captcha_required","message":"CAPTCHA required: turnstile_token missing"}"#, 0);
    assert_eq!(c.outcome, Outcome::ChallengeRequired);
    let c_old = interpret_reply(200, r#"{"success":false,"message":"CAPTCHA required: turnstile_token missing"}"#, 0);
    assert_eq!(c_old.outcome, Outcome::ChallengeRequired);
    let u = interpret_reply(200, r#"{"success":false,"message":"Unknown RPC response"}"#, 0);
    assert_eq!(u.outcome, Outcome::Unknown);
    let refused = interpret_reply(200, "{\"success\":false,\"code\":\"not_member\",\"message\":\"no token\u{202E}evil\"}", 0);
    assert_eq!(refused.outcome, Outcome::Refused);
    assert_eq!(refused.message, "no tokenevil", "bidi overrides are stripped");
    assert_eq!(interpret_reply(502, "", 0).outcome, Outcome::Unreachable);
    assert_eq!(interpret_reply(200, "<html>", 0).outcome, Outcome::Unreachable);
    assert_eq!(interpret_reply(200, r#"{"hello":1}"#, 0).outcome, Outcome::Unreachable);
    let long = interpret_reply(200, &json!({"success": false, "message": "x".repeat(5000)}).to_string(), 0);
    assert!(long.message.chars().count() <= 300);
}

// ------------------------------------------------------------------ the request

#[test]
fn off_by_default_sends_nothing_and_points_at_the_page() {
    let (_d, store) = temp_store();
    let http = Script::posting(Ok((200, sent_body())));
    let r = request(&store, rec(), &http, &POOR, &ctx(&hash(1), 5_000)).expect("request");
    assert_eq!(r.state, "disabled");
    assert_eq!(r.gate, Gate::Disabled);
    assert_eq!(http.post_count(), 0);
    assert!(r.message.contains("not turned on"));
    assert_eq!(r.faucet_page, format!("{BASE}/?address={WALLET}"));
    assert!(store.load().expect("load").ledger.is_empty());
}

#[test]
fn a_granted_request_posts_once_for_the_members_own_wallet() {
    let (_d, store) = temp_store();
    granted(&store);
    let http = Script::posting(Ok((200, sent_body())));
    let r = request(&store, rec(), &http, &POOR, &ctx(&hash(2), 5_000)).expect("request");
    assert_eq!(r.outcome, Some(Outcome::Sent));
    assert_eq!(r.tx_hash, Some(hash(0x5e)));
    assert_eq!(r.need_wei.as_deref(), Some("2000000000000000"));
    let posts = http.posts.lock().expect("posts").clone();
    assert_eq!(posts.len(), 1);
    assert_eq!(posts[0].0, format!("{BASE}/faucet"));
    assert_eq!(posts[0].1, json!({"address": WALLET}), "the recipient is the member's wallet and nothing else");
    let book = store.load().expect("load");
    assert_eq!(book.ledger.len(), 1);
    assert_eq!(book.ledger[0].origin, "hermes");
    assert_eq!(book.ledger[0].initcode_hash, Some(hash(2)));

    // Same member, an hour later: inside the window, no second POST.
    let again = request(&store, rec(), &http, &POOR, &ctx(&hash(2), 5_000 + HOUR_MS)).expect("request");
    assert_eq!(again.state, "waiting");
    assert_eq!(again.next_eligible_at_ms, Some(5_000 + MEMBER_WINDOW_MS));
    assert_eq!(http.post_count(), 1);
}

#[test]
fn no_ready_deploy_or_enough_balance_means_no_request() {
    let (_d, store) = temp_store();
    granted(&store);
    let http = Script::posting(Ok((200, sent_body())));
    let h3 = hash(3);
    let mut c = ctx(&h3, 5_000);
    c.deploy_ready = false;
    let r = request(&store, rec(), &http, &POOR, &c).expect("request");
    assert_eq!(r.state, "no_pending_deploy");
    let rich = Chain {
        balance: 2_000_000_000_000_000,
        gas_price: 1_000_000_000,
    };
    let r = request(&store, rec(), &http, &rich, &ctx(&hash(3), 5_000)).expect("request");
    assert_eq!(r.state, "not_needed");
    assert_eq!(http.post_count(), 0);
}

#[test]
fn a_changed_wallet_does_not_inherit_the_grant() {
    let (_d, store) = temp_store();
    grant(&store, rec(), OTHER, 1).expect("grant");
    let http = Script::posting(Ok((200, sent_body())));
    let r = request(&store, rec(), &http, &POOR, &ctx(&hash(4), 5_000)).expect("request");
    assert_eq!(r.state, "wallet_changed");
    assert_eq!(http.post_count(), 0);
}

#[test]
fn an_unreachable_faucet_is_recorded_and_does_not_use_the_window() {
    let (_d, store) = temp_store();
    granted(&store);
    let http = Script::posting(Err("connection refused".into()));
    let r = request(&store, rec(), &http, &POOR, &ctx(&hash(5), 5_000)).expect("request");
    assert_eq!(r.outcome, Some(Outcome::Unreachable));
    assert!(r.message.contains("Fund the deploy from your own SALT"));
    assert!(next_eligible_ms(&store.load().expect("load"), WALLET, 6_000).is_none());
}

#[test]
fn a_rate_limit_from_the_faucet_blocks_until_its_time_without_retrying() {
    let (_d, store) = temp_store();
    granted(&store);
    let body = json!({"success": false, "code": "rate_limited", "limit": "ip",
        "message": "Rate limited: ip cooldown: 0h 30m remaining", "next_eligible_at": 7_000u64});
    let http = Script::posting(Ok((200, body.to_string())));
    let r = request(&store, rec(), &http, &POOR, &ctx(&hash(6), 5_000)).expect("request");
    assert_eq!(r.outcome, Some(Outcome::RateLimited));
    assert_eq!(r.next_eligible_at_ms, Some(7_000_000));
    let again = request(&store, rec(), &http, &POOR, &ctx(&hash(6), 6_000)).expect("request");
    assert_eq!(again.state, "waiting");
    assert_eq!(http.post_count(), 1, "no retry loop");
}

#[test]
fn a_captcha_answer_says_so_and_sends_nothing_more() {
    let (_d, store) = temp_store();
    granted(&store);
    let http = Script::posting(Ok((200, r#"{"success":false,"code":"captcha_required","message":"CAPTCHA required: turnstile_token missing"}"#.into())));
    let r = request(&store, rec(), &http, &POOR, &ctx(&hash(7), 5_000)).expect("request");
    assert_eq!(r.outcome, Some(Outcome::ChallengeRequired));
    assert!(r.message.contains("CAPTCHA"));
    assert!(r.tx_hash.is_none());
}

#[test]
fn concurrent_callers_get_one_post() {
    let (_d, store) = temp_store();
    granted(&store);
    let store = Arc::new(store);
    let http = Arc::new(Script::posting(Ok((200, sent_body()))));
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let store = store.clone();
            let http = http.clone();
            std::thread::spawn(move || {
                let h = hash(8);
                request(&store, rec(), &*http, &POOR, &ctx(&h, 5_000)).expect("request").state
            })
        })
        .collect();
    let states: Vec<String> = handles.into_iter().map(|h| h.join().expect("thread")).collect();
    assert_eq!(http.post_count(), 1);
    assert_eq!(states.iter().filter(|s| *s == "requested").count(), 1);
    assert_eq!(states.iter().filter(|s| *s == "waiting").count(), 7);
}

#[test]
fn a_damaged_state_file_is_an_error_not_a_silent_reset() {
    let (dir, store) = temp_store();
    std::fs::write(dir.path().join(STATE_FILE_NAME), b"not json").expect("write");
    assert!(store.load().is_err());
    let http = Script::posting(Ok((200, sent_body())));
    assert!(request(&store, rec(), &http, &POOR, &ctx(&hash(9), 1)).is_err());
    assert_eq!(http.post_count(), 0);
}

#[test]
fn grant_and_revoke_persist_privately_and_revoke_keeps_history() {
    let (dir, store) = temp_store();
    let b = grant(&store, rec(), "0x00000000000000000000000000000000000000AA", 42).expect("grant");
    assert_eq!(b.wallet, WALLET);
    assert_eq!(b.window_ms, MEMBER_WINDOW_MS);
    let http = Script::posting(Ok((200, sent_body())));
    request(&store, rec(), &http, &POOR, &ctx(&hash(10), 50)).expect("request");
    revoke(&store, rec()).expect("revoke");
    let book = store.load().expect("load");
    assert!(book.budget.is_none());
    assert_eq!(book.ledger.len(), 1);
    let off = request(&store, rec(), &http, &POOR, &ctx(&hash(10), 60)).expect("request");
    assert_eq!(off.state, "disabled");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(dir.path().join(STATE_FILE_NAME))
            .expect("meta")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }
    let unavailable = FaucetStore::new(None);
    assert!(grant(&unavailable, rec(), WALLET, 1).is_err(), "no data dir: cannot turn on");
}

#[test]
fn the_ledger_is_bounded() {
    let mut book = FaucetBook::default();
    for i in 0..(MAX_LEDGER as u64 + 5) {
        book.append_entry(entry(i, Outcome::Refused, None));
    }
    assert_eq!(book.ledger.len(), MAX_LEDGER);
    assert_eq!(book.ledger[0].at_ms, 5);
}

// ------------------------------------------------------------------ status + health

/// One scripted HTTP answer.
type Reply = Result<(u16, String), String>;

fn with_gets(pairs: &[(&str, Reply)]) -> Script {
    Script {
        gets: pairs
            .iter()
            .map(|(k, v)| (format!("{BASE}{k}"), v.clone()))
            .collect(),
        ..Default::default()
    }
}

#[test]
fn health_reads_ready_and_falls_back_for_an_older_faucet() {
    let ready = with_gets(&[("/ready", Ok((200, r#"{"ready":true}"#.into())))]);
    assert_eq!(probe_health(&ready, BASE).ready, Some(true));
    let not = with_gets(&[(
        "/ready",
        Ok((503, r#"{"ready":false,"reason":"the faucet account cannot cover another drip"}"#.into())),
    )]);
    let h = probe_health(&not, BASE);
    assert!(h.reachable);
    assert_eq!(h.ready, Some(false));
    assert!(h.detail.contains("cannot cover"));
    let old = with_gets(&[
        ("/ready", Ok((404, String::new()))),
        ("/health", Ok((200, r#"{"status":"ok"}"#.into()))),
    ]);
    let h = probe_health(&old, BASE);
    assert!(h.reachable);
    assert_eq!(h.ready, None);
    let down = with_gets(&[]);
    let h = probe_health(&down, BASE);
    assert!(!h.reachable);
    assert_eq!(h.ready, None);
}

#[test]
fn eligibility_from_the_faucet() {
    let url = format!("/eligibility?address={WALLET}");
    let yes = with_gets(&[(url.as_str(), Ok((200, r#"{"eligible":true}"#.into())))]);
    assert_eq!(faucet_eligibility(&yes, BASE, WALLET, 0), Some(None));
    let no = with_gets(&[(
        url.as_str(),
        Ok((200, r#"{"eligible":false,"next_eligible_at":100}"#.into())),
    )]);
    assert_eq!(faucet_eligibility(&no, BASE, WALLET, 0), Some(Some(100_000)));
    let old = with_gets(&[(url.as_str(), Ok((404, String::new())))]);
    assert_eq!(faucet_eligibility(&old, BASE, WALLET, 0), None);
}

#[test]
fn status_reports_off_by_default_with_honest_health() {
    let (_d, store) = temp_store();
    let http = with_gets(&[("/ready", Err("connection refused".into()))]);
    let s = status(&store, &http, BASE, Ok(WALLET.to_string()), 9);
    assert!(!s.enabled);
    assert!(!s.wallet_matches);
    assert!(!s.health.reachable);
    assert!(!s.faucet_eligibility_known);
    assert_eq!(s.window_hours, 24);
    assert_eq!(s.max_per_window, 1);
    assert_eq!(s.pending_owner_sign_off.len(), PENDING_OWNER_SIGN_OFF.len());
    granted(&store);
    let s = status(&store, &http, BASE, Ok(WALLET.to_string()), 9);
    assert!(s.enabled && s.wallet_matches);
    let s = status(&store, &http, BASE, Err("locked".into()), 9);
    assert!(s.wallet.is_none());
    assert_eq!(s.wallet_error.as_deref(), Some("locked"));
}

// ------------------------------------------------------------------ real HTTP + window rules

/// One HTTP exchange on a loopback socket: returns the raw request the client sent.
fn serve_once(response: String) -> (String, std::thread::JoinHandle<String>) {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let h = std::thread::spawn(move || {
        let (mut s, _) = listener.accept().expect("accept");
        s.set_read_timeout(Some(std::time::Duration::from_secs(5))).expect("timeout");
        let mut buf = Vec::new();
        let mut chunk = [0u8; 4096];
        loop {
            let n = s.read(&mut chunk).unwrap_or(0);
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..n]);
            let text = String::from_utf8_lossy(&buf).to_string();
            if let Some(i) = text.find("\r\n\r\n") {
                if text[..i].to_ascii_lowercase().contains("transfer-encoding: chunked") {
                    if text.ends_with("0\r\n\r\n") {
                        break;
                    }
                    continue;
                }
                let len = text
                    .lines()
                    .find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap_or(0)))
                    .unwrap_or(0);
                if buf.len() >= i + 4 + len {
                    break;
                }
            }
        }
        s.write_all(response.as_bytes()).expect("write");
        String::from_utf8_lossy(&buf).to_string()
    });
    (format!("http://{addr}"), h)
}

#[test]
fn the_production_http_client_posts_json_to_a_real_socket() {
    let body = r#"{"success":false,"code":"not_member","message":"x"}"#;
    let resp = format!(
        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    );
    let (base, h) = serve_once(resp);
    let (status, body) = UreqFaucet
        .post_json(&format!("{base}/faucet"), &json!({"address": WALLET}))
        .expect("post");
    let raw = h.join().expect("server");
    assert_eq!(status, 200);
    assert_eq!(interpret_reply(status, &body, 0).outcome, Outcome::Refused);
    assert!(raw.starts_with("POST /faucet HTTP/1.1"));
    let sent: Value = raw
        .split_once("\r\n\r\n")
        .and_then(|(_, b)| serde_json::from_str(b).ok())
        .expect("a JSON body");
    assert_eq!(sent, json!({"address": WALLET}));
}

#[test]
fn the_challenge_window_stays_on_the_faucets_origin() {
    let u = |s: &str| url::Url::parse(s).expect("url");
    assert!(same_origin(&u("https://faucet.citrate.ai/?address=0x1"), "https://faucet.citrate.ai"));
    assert!(same_origin(&u("https://faucet.citrate.ai/faucet.js"), "https://faucet.citrate.ai"));
    assert!(!same_origin(&u("https://evil.example/"), "https://faucet.citrate.ai"));
    assert!(!same_origin(&u("http://faucet.citrate.ai/"), "https://faucet.citrate.ai"));
    assert!(!same_origin(&u("https://faucet.citrate.ai.evil.example/"), "https://faucet.citrate.ai"));
}

#[test]
fn the_challenge_window_lets_the_captcha_frame_load_and_nothing_else() {
    // The webview asks the navigation handler about every frame. The faucet page embeds its
    // CAPTCHA as an iframe from the provider, so a same-origin-only rule blocks the challenge.
    let u = |s: &str| url::Url::parse(s).expect("url");
    let base = "https://faucet.citrate.ai";
    for ok in [
        "https://faucet.citrate.ai/?address=0x1",
        "https://challenges.cloudflare.com/cdn-cgi/challenge-platform/h/b/turnstile/if/ov2/av0/rcv0/0/abc/light/normal",
        "about:blank",
        "about:srcdoc",
    ] {
        assert!(challenge_window_may_load(&u(ok), base), "{ok} must load");
    }
    for no in [
        "https://evil.example/",
        "http://challenges.cloudflare.com/",
        "https://challenges.cloudflare.com.evil.example/",
        "https://cloudflare.com/",
        "https://challenges.cloudflare.com:8443/",
        "about:config",
        "file:///etc/passwd",
        "data:text/html,hi",
        "javascript:alert(1)",
        "tauri://localhost/",
    ] {
        assert!(!challenge_window_may_load(&u(no), base), "{no} must not load");
    }
    assert_eq!(CHALLENGE_FRAME_ORIGIN, "https://challenges.cloudflare.com");
}

// ------------------------------------------------------------------ decision records (ADR D4.3)

fn scratch_outbox() -> (TempDir, crate::hic_records::HicOutbox) {
    let (dir, _store) = temp_store();
    let o = crate::hic_records::HicOutbox::new(dir.path().join("hic-outbox"));
    (dir, o)
}

#[test]
fn every_grant_call_and_revoke_leaves_a_decision_record() {
    let (_d, store) = temp_store();
    let (_o, outbox) = scratch_outbox();
    grant(&store, &outbox, WALLET, 1_000).expect("grant");
    let http = Script::posting(Ok((200, sent_body())));
    // Hermes asks inside the budget: HIC-2.
    request(&store, &outbox, &http, &POOR, &ctx(&hash(0x21), 5_000)).expect("request");
    // The member asks from the app for another deploy a day later: HIC-1.
    let h = hash(0x22);
    let mut c = ctx(&h, 5_000 + MEMBER_WINDOW_MS + 1);
    c.origin = "local-user";
    let refused = Script::posting(Ok((200, r#"{"success":false,"code":"not_member","message":"no membership token"}"#.into())));
    request(&store, &outbox, &refused, &POOR, &c).expect("request");
    revoke(&store, &outbox).expect("revoke");

    let recs = outbox.records().expect("records");
    let kinds: Vec<(&str, &str, Option<&str>)> = recs
        .iter()
        .map(|r| (r.event.kind.as_str(), r.event.decision.as_str(), r.event.outcome.as_deref()))
        .collect();
    assert_eq!(
        kinds,
        vec![
            ("faucet.budget_granted", "approved", Some("completed")),
            ("faucet.topup", "auto_within_budget", Some("completed")),
            ("faucet.topup", "approved", Some("failed")),
            ("faucet.budget_revoked", "approved", Some("completed")),
        ]
    );
    let sent = &recs[1].event;
    assert!(sent.subject.contains(WALLET) && sent.subject.contains(&hash(0x21)));
    assert!(sent.reason.contains("hermes") && sent.reason.contains("HIC-2"));
    assert_eq!(sent.evidence.len(), 1);
    assert_eq!(sent.evidence[0].digest.as_deref(), Some(hash(0x5e).as_str()));
    assert!(recs[2].event.outcome_detail.as_deref().unwrap_or("").starts_with("refused"));
    for r in &recs {
        crate::hic_records::validate(&r.event).expect("every record passes the sidecar's rules");
    }
}

#[test]
fn a_gate_refusal_sends_nothing_and_records_nothing() {
    let (_d, store) = temp_store();
    let (_o, outbox) = scratch_outbox();
    let http = Script::posting(Ok((200, sent_body())));
    request(&store, &outbox, &http, &POOR, &ctx(&hash(0x23), 5_000)).expect("off");
    grant(&store, &outbox, WALLET, 1_000).expect("grant");
    let h = hash(0x24);
    let mut c = ctx(&h, 5_000);
    c.deploy_ready = false;
    request(&store, &outbox, &http, &POOR, &c).expect("no deploy");
    assert_eq!(http.post_count(), 0);
    assert_eq!(outbox.records().expect("records").len(), 1, "only the grant");
}

#[test]
fn no_faucet_call_and_no_grant_when_the_record_cannot_be_written() {
    let (_d, store) = temp_store();
    let (_o, full) = scratch_outbox();
    let full = full.with_bounds(0, 10);
    let err = grant(&store, &full, WALLET, 1_000).expect_err("refused");
    assert!(err.contains("not turned on"), "{err}");
    assert!(store.load().expect("load").budget.is_none(), "the grant is not left in place");

    granted(&store);
    let http = Script::posting(Ok((200, sent_body())));
    let err = request(&store, &full, &http, &POOR, &ctx(&hash(0x25), 5_000)).expect_err("refused");
    assert!(err.contains("was not asked"), "{err}");
    assert_eq!(http.post_count(), 0, "fail closed: nothing is sent");
    assert!(store.load().expect("load").ledger.is_empty());
}

/// A sink that accepts the pre-check and then fails the write (a disk that fills in between).
struct WriteFails;

impl DecisionSink for WriteFails {
    fn check_writable(&self) -> Result<(), String> {
        Ok(())
    }
    fn record(&self, _ev: crate::hic_records::HicEvent) -> Result<u64, String> {
        Err("disk full".to_string())
    }
}

#[test]
fn a_record_that_fails_after_the_check_undoes_a_grant_and_is_reported_after_a_call() {
    let (_d, store) = temp_store();
    assert!(grant(&store, &WriteFails, WALLET, 1_000).is_err());
    assert!(store.load().expect("load").budget.is_none(), "undone");

    granted(&store);
    let http = Script::posting(Ok((200, sent_body())));
    let r = request(&store, &WriteFails, &http, &POOR, &ctx(&hash(0x26), 5_000)).expect("request");
    assert_eq!(r.outcome, Some(Outcome::Sent), "the drip happened and is not hidden");
    assert!(r.message.contains("decision record could not be written"), "{}", r.message);
    assert_eq!(store.load().expect("load").ledger.len(), 1, "the window is still used");

    // Turning the faucet off is never blocked by the record.
    let err = revoke(&store, &WriteFails).expect_err("reported");
    assert!(err.contains("the faucet is off"), "{err}");
    assert!(store.load().expect("load").budget.is_none());
}

#[test]
fn faucet_record_kinds_follow_the_sidecar_rules() {
    let b = FaucetBudget {
        wallet: WALLET.to_string(),
        granted_at_ms: 1,
        window_ms: MEMBER_WINDOW_MS,
        max_per_window: 1,
    };
    let mut ev = budget_granted_event(&b);
    crate::hic_records::validate(&ev).expect("grant");
    assert!(ev.subject.contains("1 top-up per 24 h"));
    ev.decision = "auto_within_budget".into();
    assert!(crate::hic_records::validate(&ev).is_err(), "a grant is the member's own decision");
    let mut ev = budget_revoked_event(WALLET);
    crate::hic_records::validate(&ev).expect("revoke");
    ev.decision = "denied".into();
    ev.outcome = None;
    assert!(crate::hic_records::validate(&ev).is_err());
    let mut e = entry(1, Outcome::Unknown, None);
    e.origin = "mcp:claude-desktop".into();
    let ev = topup_event(&e);
    assert_eq!(ev.decision, "auto_within_budget");
    assert_eq!(ev.outcome.as_deref(), Some("outcome_unknown"));
    crate::hic_records::validate(&ev).expect("topup");
    let mut denied = ev;
    denied.decision = "denied".into();
    denied.outcome = None;
    assert!(crate::hic_records::validate(&denied).is_err(), "a top-up is never a denial");
}

#[test]
fn no_capability_names_the_challenge_window() {
    for cap in [
        include_str!("../capabilities/default.json"),
        include_str!("../capabilities/popout.json"),
    ] {
        assert!(!cap.contains(CHALLENGE_WINDOW_LABEL));
        assert!(!cap.contains("\"windows\": [\"*\"]"));
    }
}

#[test]
fn the_commands_are_registered_and_allowed_for_the_main_window_only() {
    let lib = include_str!("lib.rs");
    let acl = include_str!("../permissions/main-window.toml");
    for cmd in [
        "faucet_status",
        "faucet_grant",
        "faucet_revoke",
        "faucet_request",
        "faucet_open_challenge",
    ] {
        assert!(lib.contains(&format!("faucet::{cmd},")), "{cmd} registered");
        assert!(acl.contains(&format!("\"{cmd}\"")), "{cmd} in main-window.toml");
    }
}

#[test]
fn the_node_mcp_tool_takes_no_recipient() {
    let t = crate::node_mcp_tools::tool("faucet_request").expect("tool listed");
    let schema = (t.input_schema)();
    let props = schema["properties"].as_object().expect("props");
    assert_eq!(props.keys().collect::<Vec<_>>(), vec!["initcode_hash"]);
    assert_eq!(schema["required"], json!(["initcode_hash"]));
    assert!(crate::node_mcp_tools::reject_unknown(&json!({"initcode_hash": hash(1), "address": OTHER}), &schema).is_err());
    let listed = crate::node_mcp_tools::tool_json(t);
    assert_eq!(listed["annotations"]["readOnlyHint"], false);
    assert_eq!(listed["annotations"]["destructiveHint"], false);
}

// ------------------------------------------------------------------ the MCP route

/// A backend that only answers the faucet tool and records what it was asked.
#[derive(Default)]
struct FaucetOnlyBackend {
    asked: StdMutex<Vec<(String, String)>>,
}

impl crate::node_mcp_protocol::NodeBackend for FaucetOnlyBackend {
    fn node_status(&self) -> Result<Value, String> {
        Err("not in this test".into())
    }
    fn rpc_read(&self, _m: &str, _p: Value) -> Result<crate::node_mcp_protocol::RpcRead, String> {
        Err("not in this test".into())
    }
    fn wallet_address(&self) -> Result<String, String> {
        Err("not in this test".into())
    }
    fn memory_search(&self, _t: &str, _q: &str, _l: usize) -> Result<Value, String> {
        Err("not in this test".into())
    }
    fn groups(&self) -> Result<Value, String> {
        Err("not in this test".into())
    }
    fn cluster_status(&self, _g: &str) -> Result<Value, String> {
        Err("not in this test".into())
    }
    fn cluster_peers(&self, _g: &str) -> Result<Value, String> {
        Err("not in this test".into())
    }
    fn invites(&self, _g: &str) -> Result<Value, String> {
        Err("not in this test".into())
    }
    fn propose_transaction(
        &self,
        _o: &str,
        _t: &str,
        _v: u128,
        _d: &str,
    ) -> Result<crate::node_mcp_protocol::ProposedSignature, String> {
        Err("not in this test".into())
    }
    fn close_ceremony(&self, _id: &str) {}
    fn devices(&self) -> Result<Value, String> {
        Err("absent".into())
    }
    fn pins(&self) -> Result<Value, String> {
        Err("absent".into())
    }
    fn propose_deploy(
        &self,
        _o: &str,
        _b: &str,
        _c: &str,
        _v: u128,
        _g: Option<u64>,
    ) -> Result<crate::node_mcp_protocol::ProposedSignature, String> {
        Err("absent".into())
    }
    fn anchor_ready(&self) -> Result<(), String> {
        Err("absent".into())
    }
    fn contract_abi(&self, _a: &str) -> Result<Value, String> {
        Err("absent".into())
    }
    fn faucet_request(&self, origin: &str, initcode_hash: &str) -> Result<Value, String> {
        self.asked
            .lock()
            .expect("asked lock")
            .push((origin.to_string(), initcode_hash.to_string()));
        Ok(json!({"state": "disabled"}))
    }
}

fn mcp_call(core: &crate::node_mcp_protocol::McpCore, args: Value) -> Value {
    let c = crate::node_mcp_protocol::CallerCtx {
        token_id: "t1".into(),
        token_label: "Hermes".into(),
        client_name: Some("hermes".into()),
        read_only: false,
    };
    core.dispatch(
        &c,
        &json!({"jsonrpc": "2.0", "id": 9, "method": "tools/call", "params": {"name": "faucet_request", "arguments": args}}),
    )
    .expect("reply")
}

#[test]
fn the_mcp_tool_runs_at_once_with_the_callers_origin_and_no_recipient() {
    let backend = Arc::new(FaucetOnlyBackend::default());
    let core = crate::node_mcp_protocol::McpCore::new(
        backend.clone(),
        Arc::new(crate::node_mcp_approvals::ApprovalInbox::new()),
    );
    let ok = mcp_call(&core, json!({"initcode_hash": hash(0xAB).to_uppercase().replacen("0X", "0x", 1)}));
    assert_eq!(ok["result"]["isError"], json!(false), "{ok}");
    assert_eq!(ok["result"]["structuredContent"]["state"], "disabled");
    let asked = backend.asked.lock().expect("asked").clone();
    assert_eq!(asked, vec![("mcp:Hermes via hermes".to_string(), hash(0xab))]);

    for bad in [
        json!({"initcode_hash": "0x12"}),
        json!({}),
        json!({"initcode_hash": hash(1), "address": OTHER}),
        json!({"initcode_hash": hash(1), "amount": "1"}),
    ] {
        let r = mcp_call(&core, bad.clone());
        assert_eq!(r["result"]["isError"], json!(true), "{bad}");
    }
    assert_eq!(backend.asked.lock().expect("asked").len(), 1, "bad calls never reach core");
}

#[test]
fn a_backend_without_a_faucet_says_so() {
    struct Plain;
    impl crate::node_mcp_protocol::NodeBackend for Plain {
        fn node_status(&self) -> Result<Value, String> {
            Err("x".into())
        }
        fn rpc_read(&self, _m: &str, _p: Value) -> Result<crate::node_mcp_protocol::RpcRead, String> {
            Err("x".into())
        }
        fn wallet_address(&self) -> Result<String, String> {
            Err("x".into())
        }
        fn memory_search(&self, _t: &str, _q: &str, _l: usize) -> Result<Value, String> {
            Err("x".into())
        }
        fn groups(&self) -> Result<Value, String> {
            Err("x".into())
        }
        fn cluster_status(&self, _g: &str) -> Result<Value, String> {
            Err("x".into())
        }
        fn cluster_peers(&self, _g: &str) -> Result<Value, String> {
            Err("x".into())
        }
        fn invites(&self, _g: &str) -> Result<Value, String> {
            Err("x".into())
        }
        fn propose_transaction(
            &self,
            _o: &str,
            _t: &str,
            _v: u128,
            _d: &str,
        ) -> Result<crate::node_mcp_protocol::ProposedSignature, String> {
            Err("x".into())
        }
        fn close_ceremony(&self, _id: &str) {}
        fn devices(&self) -> Result<Value, String> {
            Err("absent".into())
        }
        fn pins(&self) -> Result<Value, String> {
            Err("absent".into())
        }
        fn propose_deploy(
            &self,
            _o: &str,
            _b: &str,
            _c: &str,
            _v: u128,
            _g: Option<u64>,
        ) -> Result<crate::node_mcp_protocol::ProposedSignature, String> {
            Err("absent".into())
        }
        fn anchor_ready(&self) -> Result<(), String> {
            Err("absent".into())
        }
        fn contract_abi(&self, _a: &str) -> Result<Value, String> {
            Err("absent".into())
        }
    }
    let core = crate::node_mcp_protocol::McpCore::new(
        Arc::new(Plain),
        Arc::new(crate::node_mcp_approvals::ApprovalInbox::new()),
    );
    let r = mcp_call(&core, json!({"initcode_hash": hash(2)}));
    assert_eq!(r["result"]["isError"], json!(true));
    assert!(r.to_string().contains("not available"));
}

// ------------------------------------------------------------------ end to end (real faucet)

/// Chain reads against the throwaway anvil the e2e script starts.
struct LoopbackReads(crate::rpc::RpcClient<crate::rpc::HttpTransport>);

impl ChainReads for LoopbackReads {
    fn balance_wei(&self, address: &str) -> Result<u128, String> {
        self.0.get_balance(address).map_err(|e| e.to_string())
    }
    fn gas_price_wei(&self) -> Result<u128, String> {
        self.0.gas_price().map(u128::from).map_err(|e| e.to_string())
    }
}

/// HUP-S6.5 end to end: core's production HTTP client against a real `citrate-faucet` binary
/// (citrate-chain `faucet/`) that signs real transfers on a throwaway anvil with chain id 40204.
/// Run by `scripts/e2e-faucet.sh`; skipped unless `CITRATE_E2E_FAUCET_URL` and
/// `CITRATE_E2E_RPC` are set. Nothing touches chain 40204.
#[test]
fn e2e_faucet_binary_drips_once_then_both_sides_report_the_next_time() {
    let (Ok(base), Ok(rpc)) = (
        std::env::var("CITRATE_E2E_FAUCET_URL"),
        std::env::var("CITRATE_E2E_RPC"),
    ) else {
        eprintln!("skipped: CITRATE_E2E_FAUCET_URL is not set (run scripts/e2e-faucet.sh)");
        return;
    };
    let base = faucet_base_url(Some(&base));
    let reads = LoopbackReads(crate::rpc::RpcClient::with_transport(
        crate::rpc::HttpTransport::new(rpc),
    ));
    let member = "0x00000000000000000000000000000000000fa0e1";
    let now = crate::hic_records::now_ms();
    assert_eq!(reads.balance_wei(member).expect("balance"), 0, "a fresh member wallet");

    // The faucet is up and funded.
    let health = probe_health(&UreqFaucet, &base);
    assert_eq!(health.ready, Some(true), "{}", health.detail);
    assert_eq!(faucet_eligibility(&UreqFaucet, &base, member, now), Some(None));

    // 1. Hermes asks inside the member's budget: one real drip to the member's own wallet.
    let (_d, store) = temp_store();
    let (_o, outbox) = scratch_outbox();
    grant(&store, &outbox, member, now).expect("grant");
    let h = hash(0x31);
    let ctx = RequestCtx {
        wallet: member,
        origin: "hermes",
        initcode_hash: &h,
        deploy_ready: true,
        base: &base,
        now_ms: now,
    };
    let r = request(&store, &outbox, &UreqFaucet, &reads, &ctx).expect("request");
    assert_eq!(r.outcome, Some(Outcome::Sent), "{}", r.message);
    assert!(r.tx_hash.as_deref().is_some_and(is_tx_hash));
    let mut balance = 0;
    for _ in 0..50 {
        balance = reads.balance_wei(member).expect("balance");
        if balance > 0 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert_eq!(balance, DRIP_WEI, "the drip landed on the member's wallet");

    // 2. Now the balance covers the deploy: a second ask sends nothing.
    let again = request(&store, &outbox, &UreqFaucet, &reads, &ctx).expect("request");
    assert_eq!(again.state, "not_needed");

    // 3. The faucet's own view: not eligible for about 24 hours.
    let next = faucet_eligibility(&UreqFaucet, &base, member, now)
        .expect("known")
        .expect("a next time");
    assert!(next > now + 23 * HOUR_MS && next <= now + 25 * HOUR_MS, "{next}");

    // 4. Another device with its own app history (the app window cannot see it) asks for a
    //    second member with no balance from the same IP: the faucet refuses with its next time
    //    and the app records it and does not retry.
    let second = "0x00000000000000000000000000000000000fa0e2";
    let (_d2, store2) = temp_store();
    grant(&store2, &outbox, second, now).expect("grant");
    let ctx2 = RequestCtx {
        wallet: second,
        ..ctx
    };
    let refused = request(&store2, &outbox, &UreqFaucet, &reads, &ctx2).expect("request");
    assert_eq!(refused.outcome, Some(Outcome::RateLimited), "{}", refused.message);
    assert!(refused.next_eligible_at_ms.is_some_and(|t| t > now));
    let waiting = request(&store2, &outbox, &UreqFaucet, &reads, &ctx2).expect("request");
    assert_eq!(waiting.state, "waiting");
    assert_eq!(reads.balance_wei(second).expect("balance"), 0);

    // 5. Every call left a decision record.
    let topups: Vec<_> = outbox
        .records()
        .expect("records")
        .into_iter()
        .filter(|r| r.event.kind == "faucet.topup")
        .map(|r| r.event.outcome.unwrap_or_default())
        .collect();
    assert_eq!(topups, vec!["completed".to_string(), "failed".to_string()]);
}
