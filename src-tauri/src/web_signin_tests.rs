// HUP-S2.3: the live sign-in path (core's side of the managed browser's bridge).
//
// The sidecar and the browser's DevTools list are test doubles here (a recorded `GET
// /browser/sign-in` body and a `/json/list` answer); the ceremony, the budget store, the vault and
// the EIP-191 signer are the real ones. Each test names the WebSigningBudget.tla property or the
// ADR clause it pins. `LoopbackDevtools` is exercised against a real local HTTP server.

use super::*;
use citrate_core_kit::custody::{CustodyError, Keyring};
use citrate_core_kit::web_budget::BudgetGate;
use std::sync::Mutex as StdMutex;

#[derive(Default)]
struct FakeKeyring {
    store: StdMutex<std::collections::HashMap<String, Vec<u8>>>,
}

impl Keyring for FakeKeyring {
    fn get(&self, account: &str) -> Result<Option<Vec<u8>>, CustodyError> {
        Ok(self.store.lock().unwrap().get(account).cloned())
    }
    fn set(&self, account: &str, secret: &[u8]) -> Result<(), CustodyError> {
        self.store
            .lock()
            .unwrap()
            .insert(account.into(), secret.to_vec());
        Ok(())
    }
    fn delete(&self, account: &str) -> Result<(), CustodyError> {
        self.store.lock().unwrap().remove(account);
        Ok(())
    }
}

const MNEMONIC: &str =
    "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
const ADDR: &str = "0x9858EfFD232B4033E47d90003D41EC34EcaEda94";
const ORIGIN: &str = "https://app.example.org";
const NOW: u64 = 1_790_856_000_000; // 2026-10-01T12:00:00Z
const DAY: u64 = 86_400_000;

fn tmp_dir(tag: &str) -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let mut p = std::env::temp_dir();
    p.push(format!(
        "citrate-web-signin-{tag}-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn vault(dir: &std::path::Path) -> CustodyVault {
    let v = CustodyVault::new(Box::new(FakeKeyring::default()), dir.join("custody.enc"), 0);
    v.init(&mut b"pw-for-tests".to_vec()).expect("init");
    v.unlock(&mut b"pw-for-tests".to_vec()).expect("unlock");
    citrate_core_kit::wallet::import(&v, MNEMONIC).expect("import");
    v
}

fn gate(dir: &std::path::Path) -> BudgetGate {
    BudgetGate::open(
        dir.join("web-signing-budgets.json"),
        Box::new(FakeKeyring::default()),
        NOW,
    )
}

fn grant(g: &BudgetGate) -> u64 {
    g.grant(ORIGIN, DEFAULT_PRINCIPAL, 5, DAY, &ADDR.to_lowercase(), NOW)
        .expect("grant")
        .id
}

fn siwe(nonce: &str) -> String {
    citrate_core_kit::siwe::SiweFields {
        scheme: None,
        domain: "app.example.org".into(),
        address: ADDR.into(),
        statement: Some("Sign in to Example.".into()),
        uri: "https://app.example.org/".into(),
        version: "1".into(),
        chain_id: "40204".into(),
        nonce: nonce.into(),
        issued_at: "2026-10-01T11:59:30Z".into(),
        expiration_time: Some("2026-10-01T12:10:00Z".into()),
        not_before: None,
        request_id: None,
        resources: vec![],
    }
    .to_message()
}

/// The sidecar: serves a recorded `/browser/sign-in` body and records every POST.
struct FakeLink {
    snapshot: StdMutex<serde_json::Value>,
    posts: StdMutex<Vec<(String, serde_json::Value)>>,
    post_status: StdMutex<u16>,
}

impl FakeLink {
    fn new(snapshot: serde_json::Value) -> Self {
        FakeLink {
            snapshot: StdMutex::new(snapshot),
            posts: StdMutex::new(vec![]),
            post_status: StdMutex::new(200),
        }
    }
    fn posts(&self) -> Vec<(String, serde_json::Value)> {
        self.posts.lock().unwrap().clone()
    }
}

impl SidecarLink for FakeLink {
    fn get(&self, path: &str) -> Result<(u16, String), String> {
        assert_eq!(path, "/browser/sign-in");
        Ok((200, self.snapshot.lock().unwrap().to_string()))
    }
    fn post(&self, path: &str, body: &str) -> Result<(u16, String), String> {
        self.posts
            .lock()
            .unwrap()
            .push((path.to_string(), serde_json::from_str(body).unwrap()));
        let s = *self.post_status.lock().unwrap();
        Ok((s, "{}".to_string()))
    }
}

/// The browser's `/json/list`, as core reads it.
struct FakeDevtools(Result<Vec<DevtoolsTarget>, String>);

impl DevtoolsReader for FakeDevtools {
    fn targets(&self, port: u16) -> Result<Vec<DevtoolsTarget>, String> {
        assert_eq!(port, 41_234, "core reads the port the sidecar reported");
        self.0.clone()
    }
}

fn tab_on(url: &str) -> FakeDevtools {
    FakeDevtools(Ok(vec![
        DevtoolsTarget {
            id: "OTHER".into(),
            kind: "page".into(),
            url: "https://evil.example.net/".into(),
        },
        DevtoolsTarget {
            id: "T1".into(),
            kind: "page".into(),
            url: url.into(),
        },
    ]))
}

fn request(kind: &str, raise: &str, top: bool, message: Option<&str>) -> serde_json::Value {
    serde_json::json!({
        "id": "signin-1-1",
        "kind": kind,
        "raiseOrigin": raise,
        "topFrame": top,
        "messageHex": message.map(|m| hex::encode(m.as_bytes())),
        "address": ADDR,
        "createdMs": NOW,
    })
}

fn snapshot(req: serde_json::Value, taint: serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "mode": "managed",
        "devtoolsPort": 41_234,
        "targetId": "T1",
        "url": "https://app.example.org/login",
        "requests": [req],
        "taint": taint,
    })
}

fn clean() -> serde_json::Value {
    serde_json::json!({"state": "clean"})
}

struct World {
    _dir: std::path::PathBuf,
    vault: CustodyVault,
    gate: BudgetGate,
    ceremony: SignatureCeremony,
    state: SignInState,
}

fn world(tag: &str) -> World {
    let d = tmp_dir(tag);
    World {
        vault: vault(&d),
        gate: gate(&d),
        ceremony: SignatureCeremony::new(),
        state: SignInState::default(),
        _dir: d,
    }
}

fn run(w: &World, link: &FakeLink, dt: &FakeDevtools) -> Result<SignInOutcome, String> {
    handle(
        &SignInCtx {
            link,
            devtools: dt,
            ceremony: &w.ceremony,
            vault: &w.vault,
            gate: &w.gate,
            state: &w.state,
            clock: &|| NOW,
        },
        "signin-1-1",
    )
}

fn answers(link: &FakeLink) -> Vec<serde_json::Value> {
    link.posts()
        .into_iter()
        .filter(|(p, _)| p == "/browser/sign-in/answer")
        .map(|(_, b)| b)
        .collect()
}

// ---- the budgeted path, end to end ----

#[test]
fn a_sign_in_inside_a_budget_is_signed_recorded_and_delivered() {
    let w = world("auto");
    let budget = grant(&w.gate);
    let msg = siwe("abcdef0123456789AA");
    let link = FakeLink::new(snapshot(
        request("personal_sign", ORIGIN, true, Some(&msg)),
        clean(),
    ));
    let out = run(&w, &link, &tab_on("https://app.example.org/login")).expect("handled");
    match out {
        SignInOutcome::AutoSigned {
            origin,
            remaining,
            budget_id,
            delivered,
            ..
        } => {
            assert_eq!(origin, ORIGIN);
            assert_eq!(remaining, 4);
            assert_eq!(budget_id, budget);
            assert!(delivered);
        }
        other => panic!("expected an automatic sign-in: {other:?}"),
    }
    let a = answers(&link);
    assert_eq!(a.len(), 1);
    let sig = a[0]["signature"].as_str().expect("a signature");
    assert_eq!(sig.len(), 132);
    let signer =
        citrate_core_kit::wallet::recover_personal_hex(msg.as_bytes(), sig).expect("recovers");
    assert!(
        signer.eq_ignore_ascii_case(ADDR),
        "EIP-191 over the exact message, by the member's wallet"
    );
    assert_eq!(w.gate.snapshot(NOW, None).budgets[0].used_count, 1);
}

#[test]
fn taint_comes_from_the_sidecar_and_other_sites_force_a_card() {
    // TaintDowngrade (D2 #19): content from another site in any live session means a card.
    let w = world("taint");
    grant(&w.gate);
    let msg = siwe("abcdef0123456789AB");
    let link = FakeLink::new(snapshot(
        request("personal_sign", ORIGIN, true, Some(&msg)),
        serde_json::json!({"state": "sources", "sources": [ORIGIN, "https://news.example.com"]}),
    ));
    match run(&w, &link, &tab_on("https://app.example.org/")).expect("handled") {
        SignInOutcome::Pending { reason, ceremony } => {
            assert!(reason.contains("another source"), "{reason}");
            assert!(w.state.is_sign_in_card(&ceremony.id));
        }
        other => panic!("{other:?}"),
    }
    assert!(
        answers(&link).is_empty(),
        "nothing is sent to the page while the member decides"
    );
    assert_eq!(w.gate.snapshot(NOW, None).budgets[0].used_count, 0);
}

#[test]
fn same_site_content_does_not_taint_a_sign_in_to_that_site() {
    // O-3 (accepted): the page Hermes read is the page asking.
    let w = world("o3");
    grant(&w.gate);
    let msg = siwe("abcdef0123456789AC");
    let link = FakeLink::new(snapshot(
        request("personal_sign", ORIGIN, true, Some(&msg)),
        serde_json::json!({"state": "sources", "sources": [ORIGIN]}),
    ));
    assert!(matches!(
        run(&w, &link, &tab_on("https://app.example.org/")).expect("handled"),
        SignInOutcome::AutoSigned { .. }
    ));
}

#[test]
fn unknown_taint_or_an_empty_source_list_is_a_card() {
    for taint in [
        serde_json::json!({"state": "unknown"}),
        serde_json::json!({"state": "sources", "sources": []}),
    ] {
        let w = world("unknown");
        grant(&w.gate);
        let msg = siwe("abcdef0123456789AD");
        let link = FakeLink::new(snapshot(
            request("personal_sign", ORIGIN, true, Some(&msg)),
            taint,
        ));
        assert!(matches!(
            run(&w, &link, &tab_on("https://app.example.org/")).expect("handled"),
            SignInOutcome::Pending { .. }
        ));
    }
}

// ---- origin attestation (D2 #1-3) ----

#[test]
fn a_page_that_moved_before_core_looked_is_not_attested() {
    // OriginBound: the asking context was on another site; the tab is now on the budgeted one.
    let w = world("moved");
    grant(&w.gate);
    let msg = siwe("abcdef0123456789AE");
    let link = FakeLink::new(snapshot(
        request(
            "personal_sign",
            "https://evil.example.net",
            true,
            Some(&msg),
        ),
        clean(),
    ));
    match run(&w, &link, &tab_on("https://app.example.org/")).expect("handled") {
        SignInOutcome::Pending { reason, ceremony } => {
            assert!(reason.contains("could not confirm"), "{reason}");
            assert!(
                ceremony.origin.contains("not verified"),
                "{}",
                ceremony.origin
            );
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(w.gate.snapshot(NOW, None).budgets[0].used_count, 0);
}

#[test]
fn attach_mode_a_subframe_or_an_unreadable_browser_is_never_budgeted() {
    let msg = siwe("abcdef0123456789AF");
    // Attach mode (D2 #3).
    let w = world("attach");
    grant(&w.gate);
    let mut snap = snapshot(request("personal_sign", ORIGIN, true, Some(&msg)), clean());
    snap["mode"] = serde_json::json!("attached");
    let link = FakeLink::new(snap);
    assert!(matches!(
        run(&w, &link, &tab_on("https://app.example.org/")).expect("handled"),
        SignInOutcome::Pending { .. }
    ));
    // An embedded frame (D2 #2, TopFrameOnly).
    let w = world("sub");
    grant(&w.gate);
    let link = FakeLink::new(snapshot(
        request("personal_sign", ORIGIN, false, Some(&msg)),
        clean(),
    ));
    match run(&w, &link, &tab_on("https://app.example.org/")).expect("handled") {
        SignInOutcome::Pending { reason, .. } => {
            assert!(reason.contains("embedded frame"), "{reason}")
        }
        other => panic!("{other:?}"),
    }
    // DevTools unreadable, or Hermes's tab missing (D8: CDP lost means HIC-1).
    for dt in [
        FakeDevtools(Err("refused".into())),
        FakeDevtools(Ok(vec![])),
    ] {
        let w = world("nodevtools");
        grant(&w.gate);
        let link = FakeLink::new(snapshot(
            request("personal_sign", ORIGIN, true, Some(&msg)),
            clean(),
        ));
        assert!(matches!(
            run(&w, &link, &dt).expect("handled"),
            SignInOutcome::Pending { .. }
        ));
        assert_eq!(w.gate.snapshot(NOW, None).budgets[0].used_count, 0);
    }
}

#[test]
fn attest_takes_the_origin_from_the_browser_not_the_request() {
    let snap: BridgeSnapshot =
        serde_json::from_value(snapshot(request("accounts", ORIGIN, true, None), clean())).unwrap();
    let a = attest(
        &snap,
        &snap.requests[0],
        &tab_on("https://APP.example.org:443/x?y#z"),
    )
    .expect("attested");
    assert_eq!(a.origin, ORIGIN);
    assert_eq!(a.frame, FrameKind::Top);
    assert_eq!(a.mode, BrowserMode::Managed);
    assert!(attest(&snap, &snap.requests[0], &tab_on("about:blank")).is_err());
    let mut no_port = snap.clone();
    no_port.devtools_port = None;
    assert!(attest(&no_port, &snap.requests[0], &tab_on(ORIGIN)).is_err());
    let not_a_page = FakeDevtools(Ok(vec![DevtoolsTarget {
        id: "T1".into(),
        kind: "service_worker".into(),
        url: ORIGIN.into(),
    }]));
    assert!(attest(&snap, &snap.requests[0], &not_a_page).is_err());
}

// ---- the address (eth_requestAccounts) ----

#[test]
fn the_address_is_shared_only_with_a_budgeted_attested_top_frame() {
    let w = world("accounts");
    let link = FakeLink::new(snapshot(request("accounts", ORIGIN, true, None), clean()));
    match run(&w, &link, &tab_on("https://app.example.org/")).expect("handled") {
        SignInOutcome::Refused { reason } => assert!(reason.contains("sign-in budget"), "{reason}"),
        other => panic!("no budget: {other:?}"),
    }
    assert_eq!(answers(&link)[0]["refused"]["code"], 4100);

    grant(&w.gate);
    let link = FakeLink::new(snapshot(request("accounts", ORIGIN, true, None), clean()));
    assert_eq!(
        run(&w, &link, &tab_on("https://app.example.org/")).expect("handled"),
        SignInOutcome::AddressShared {
            origin: ORIGIN.into()
        }
    );
    assert_eq!(
        answers(&link)[0]["accounts"],
        serde_json::json!([ADDR]),
        "EIP-55, so the site's sign-in message passes D2 #9"
    );

    let link = FakeLink::new(snapshot(request("accounts", ORIGIN, false, None), clean()));
    assert!(
        matches!(
            run(&w, &link, &tab_on("https://app.example.org/")).expect("handled"),
            SignInOutcome::Refused { .. }
        ),
        "an embedded frame never learns the address"
    );
}

// ---- other messages and malformed requests ----

#[test]
fn a_non_text_message_is_an_ordinary_card() {
    let w = world("binary");
    grant(&w.gate);
    let mut req = request("personal_sign", ORIGIN, true, None);
    req["messageHex"] = serde_json::json!("ff00fe");
    let link = FakeLink::new(snapshot(req, clean()));
    match run(&w, &link, &tab_on("https://app.example.org/")).expect("handled") {
        SignInOutcome::Pending { reason, ceremony } => {
            assert!(reason.contains("not a sign-in message"), "{reason}");
            assert_eq!(ceremony.origin, ORIGIN, "attested, so shown as the site");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_missing_or_oversized_message_or_unknown_request_is_refused() {
    let w = world("bad");
    let mut req = request("personal_sign", ORIGIN, true, None);
    req["messageHex"] = serde_json::json!("ab".repeat(MAX_MESSAGE_BYTES + 1));
    let link = FakeLink::new(snapshot(req, clean()));
    assert!(matches!(
        run(&w, &link, &tab_on(ORIGIN)).expect("handled"),
        SignInOutcome::Refused { .. }
    ));
    let mut other = request("personal_sign", ORIGIN, true, Some("hi"));
    other["id"] = serde_json::json!("signin-9-9");
    let link = FakeLink::new(snapshot(other, clean()));
    assert!(
        run(&w, &link, &tab_on(ORIGIN)).is_err(),
        "no such request waiting"
    );
}

// ---- cards: approve and reject ----

#[test]
fn an_approved_card_is_signed_once_and_delivered_to_the_page() {
    let w = world("approve");
    let msg = siwe("abcdef0123456789B0");
    let link = FakeLink::new(snapshot(
        request("personal_sign", ORIGIN, true, Some(&msg)),
        clean(),
    ));
    let id = match run(&w, &link, &tab_on(ORIGIN)).expect("handled") {
        SignInOutcome::Pending { ceremony, .. } => ceremony.id,
        other => panic!("no budget, so a card: {other:?}"),
    };
    assert!(approve(&link, &w.ceremony, &w.vault, &w.state, &id, false).expect("approved"));
    let a = answers(&link);
    assert_eq!(a.len(), 1);
    assert_eq!(a[0]["id"], "signin-1-1");
    let sig = a[0]["signature"].as_str().expect("signature");
    let signer =
        citrate_core_kit::wallet::recover_personal_hex(msg.as_bytes(), sig).expect("recovers");
    assert!(signer.eq_ignore_ascii_case(ADDR));
    assert!(
        approve(&link, &w.ceremony, &w.vault, &w.state, &id, false).is_err(),
        "one approval, one signature"
    );
}

#[test]
fn only_sign_in_cards_are_approved_here_and_a_decline_tells_the_page() {
    let w = world("reject");
    let other = w.ceremony.request(SignatureIntent {
        origin: "local-user".into(),
        kind: IntentKind::PersonalSign,
        chain_id: 40204,
        raw: hex::encode(b"hello"),
    });
    let link = FakeLink::new(snapshot(
        request("personal_sign", ORIGIN, true, Some("hi")),
        clean(),
    ));
    assert!(approve(&link, &w.ceremony, &w.vault, &w.state, &other.id, false).is_err());
    assert!(
        w.ceremony.status(&other.id).is_some(),
        "an unrelated ceremony is untouched"
    );

    let id = match run(&w, &link, &tab_on(ORIGIN)).expect("handled") {
        SignInOutcome::Pending { ceremony, .. } => ceremony.id,
        other => panic!("{other:?}"),
    };
    reject(&link, &w.ceremony, &w.state, &id).expect("declined");
    assert!(w.ceremony.status(&id).is_none(), "nothing left to approve");
    let a = answers(&link);
    assert_eq!(a.last().unwrap()["refused"]["code"], 4001);
    assert!(reject(&link, &w.ceremony, &w.state, &id).is_err());
}

// ---- US-2.3 AC3: records into the anchored decision log ----

#[test]
fn records_are_exported_in_batches_and_the_cursor_moves_only_on_success() {
    let w = world("export");
    grant(&w.gate);
    let msg = siwe("abcdef0123456789B1");
    let link = FakeLink::new(snapshot(
        request("personal_sign", ORIGIN, true, Some(&msg)),
        clean(),
    ));
    assert!(matches!(
        run(&w, &link, &tab_on(ORIGIN)).expect("handled"),
        SignInOutcome::AutoSigned { .. }
    ));
    *link.post_status.lock().unwrap() = 500;
    assert!(export_records(&link, &w.gate).is_err());
    assert_eq!(
        w.gate.records_to_export(10).len(),
        2,
        "nothing marked after a failure"
    );
    *link.post_status.lock().unwrap() = 404;
    assert_eq!(
        export_records(&link, &w.gate),
        Ok(ExportResult::NotConfigured)
    );
    assert_eq!(w.gate.records_to_export(10).len(), 2);
    *link.post_status.lock().unwrap() = 200;
    assert_eq!(
        export_records(&link, &w.gate),
        Ok(ExportResult::Exported(2))
    );
    assert!(w.gate.records_to_export(10).is_empty());
    let sent: Vec<serde_json::Value> = link
        .posts()
        .into_iter()
        .filter(|(p, _)| p == "/records/web-signing")
        .map(|(_, b)| b)
        .collect();
    let last = sent.last().unwrap()["records"].as_array().unwrap().clone();
    assert_eq!(last[0]["kind"], "budget_granted");
    assert_eq!(last[0]["status"], "final");
    assert_eq!(last[1]["kind"], "auto_sign");
    assert_eq!(last[1]["status"], "signed");
    assert!(last[1]["payloadDigest"].as_str().unwrap().starts_with("0x"));
    assert_eq!(last[1]["hash"].as_str().unwrap().len(), 66);
    assert_eq!(
        export_records(&link, &w.gate),
        Ok(ExportResult::Exported(0))
    );
}

#[test]
fn a_revoke_all_record_has_a_display_origin() {
    let w = world("revoke-all");
    w.gate.revoke_all("test", NOW).expect("revoked");
    let r = w.gate.records_to_export(10);
    assert_eq!(record_wire(&r[0])["origin"], "(all sites)");
}

// ---- the real DevTools reader, against a local HTTP server ----

fn serve_once(status: &'static str, body: &'static str) -> u16 {
    use std::io::{BufRead, BufReader, Write};
    let l = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = l.local_addr().expect("addr").port();
    std::thread::spawn(move || {
        if let Ok((s, _)) = l.accept() {
            let mut r = BufReader::new(&s);
            let mut line = String::new();
            while r.read_line(&mut line).map(|n| n > 2).unwrap_or(false) {
                line.clear();
            }
            let resp = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let mut w = &s;
            let _ = w.write_all(resp.as_bytes());
        }
    });
    port
}

#[test]
fn the_loopback_reader_parses_chromes_target_list() {
    let port = serve_once(
        "200 OK",
        r#"[{"description":"","devtoolsFrontendUrl":"/devtools/inspector.html","id":"T1","title":"x","type":"page","url":"https://app.example.org/login","webSocketDebuggerUrl":"ws://127.0.0.1/devtools/page/T1"}]"#,
    );
    let t = LoopbackDevtools.targets(port).expect("read");
    assert_eq!(
        t,
        vec![DevtoolsTarget {
            id: "T1".into(),
            kind: "page".into(),
            url: "https://app.example.org/login".into()
        }]
    );
    let port = serve_once("404 Not Found", "{}");
    assert!(LoopbackDevtools.targets(port).is_err());
    let port = serve_once("200 OK", "not json");
    assert!(LoopbackDevtools.targets(port).is_err());
    assert!(
        LoopbackDevtools.targets(80).is_err(),
        "never a privileged port"
    );
}

#[test]
fn sidecar_taint_maps_to_the_kit_taint() {
    assert_eq!(TaintWire::Clean.to_task_taint(), TaskTaint::Clean);
    assert_eq!(TaintWire::Unknown.to_task_taint(), TaskTaint::Unknown);
    assert_eq!(
        TaintWire::Sources { sources: vec![] }.to_task_taint(),
        TaskTaint::Unknown
    );
    assert_eq!(
        TaintWire::Sources {
            sources: vec!["ext:x".into()]
        }
        .to_task_taint(),
        TaskTaint::Sources(vec!["ext:x".into()])
    );
}
