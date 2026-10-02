// HUP-S1.5 — the escalation router's core half: member endpoints (key in the keyring, never in a
// record or a view), the daily spend budget (SpendBudget.tla invariants as unit tests), quotes that
// must be shown before a run, HIC-1 confirmation over budget or with untrusted context, settlement
// from the sidecar's answer, persistence that fails closed, and the disabled registry route.

use super::*;
use std::collections::HashMap;
use std::sync::Mutex;

const KEY: &str = "sk-escalation-secret-0123456789";
const DAY: u64 = DAY_MS;
const T0: u64 = 20_000 * DAY + 1_000; // some moment inside day 20000

#[derive(Default)]
struct MemKeyring {
    map: Mutex<HashMap<String, String>>,
    fail: bool,
}

impl crate::ai::AiKeyring for MemKeyring {
    fn get(&self, account: &str) -> std::result::Result<Option<String>, crate::ai::AiError> {
        if self.fail {
            return Err(crate::ai::AiError::KeyringUnavailable);
        }
        Ok(self
            .map
            .lock()
            .map(|m| m.get(account).cloned())
            .unwrap_or(None))
    }
    fn set(&self, account: &str, value: &str) -> std::result::Result<(), crate::ai::AiError> {
        if self.fail {
            return Err(crate::ai::AiError::KeyringUnavailable);
        }
        if let Ok(mut m) = self.map.lock() {
            m.insert(account.to_string(), value.to_string());
        }
        Ok(())
    }
    fn delete(&self, account: &str) -> std::result::Result<(), crate::ai::AiError> {
        if let Ok(mut m) = self.map.lock() {
            m.remove(account);
        }
        Ok(())
    }
}

fn input() -> EndpointInput {
    EndpointInput {
        label: "My planner".into(),
        base_url: "https://api.example.com/v1".into(),
        model: "planner-large".into(),
        input_micros_per_mtok: 3_000_000,
        output_micros_per_mtok: 15_000_000,
    }
}

/// A book with one endpoint and a daily cap of `cap` micro-USD.
fn book(cap: u64) -> (Book, String) {
    let mut b = Book::new(Ledger::new(cap, T0));
    let ep = b.add_endpoint(&input(), "ep1".into(), T0).expect("add");
    (b, ep.id)
}

fn quote(b: &mut Book, ep: &str, id: &str, max_tokens: u32, now: u64) -> QuoteView {
    b.quote(ep, "Plan the mint page.", None, max_tokens, now, id.into())
        .expect("quote")
}

// ---------------------------------------------------------------------------
// Endpoint input
// ---------------------------------------------------------------------------

#[test]
fn endpoint_urls_must_be_https_or_loopback_http() {
    for ok in [
        "https://api.example.com/v1",
        "http://127.0.0.1:1234/v1",
        "http://localhost:11434/v1",
    ] {
        assert!(validate_base_url(ok).is_ok(), "{ok}");
    }
    for bad in [
        "http://api.example.com/v1",
        "https://u:p@api.example.com/v1",
        "https://api.example.com/v1?k=1",
        "https://api.example.com/v1#x",
        "file:///etc/passwd",
        "https://",
        "https://api.example.com/ v1",
    ] {
        assert!(validate_base_url(bad).is_err(), "{bad}");
    }
}

#[test]
fn endpoint_input_is_bounded() {
    assert!(validate_endpoint_input(&input()).is_ok());
    let mut i = input();
    i.label = String::new();
    assert!(validate_endpoint_input(&i).is_err());
    i = input();
    i.label = "x".repeat(81);
    assert!(validate_endpoint_input(&i).is_err());
    i = input();
    i.model = String::new();
    assert!(validate_endpoint_input(&i).is_err());
    i = input();
    i.output_micros_per_mtok = MAX_PRICE_MICROS_PER_MTOK + 1;
    assert!(validate_endpoint_input(&i).is_err());
}

#[test]
fn at_most_max_endpoints_can_be_added() {
    let mut b = Book::new(Ledger::new(0, T0));
    for n in 0..MAX_ENDPOINTS {
        b.add_endpoint(&input(), format!("e{n}"), T0).expect("add");
    }
    assert!(b.add_endpoint(&input(), "one-more".into(), T0).is_err());
}

#[test]
fn the_destination_names_the_label_and_host_only() {
    let (b, _) = book(0);
    let d = b.endpoints[0].destination();
    assert_eq!(d, "My planner · api.example.com");
}

#[test]
fn adding_seals_the_key_in_the_keyring_and_no_record_or_view_carries_it() {
    let kr = MemKeyring::default();
    let mut b = Book::new(Ledger::new(0, T0));
    let ep = add_endpoint_with_key(&mut b, &kr, &input(), KEY, "ep9".into(), T0).expect("add");
    let stored = kr
        .map
        .lock()
        .expect("lock")
        .get(&key_account("ep9"))
        .cloned();
    assert_eq!(stored.as_deref(), Some(KEY));
    let rec = serde_json::to_string(&b.endpoints).expect("ser");
    assert!(!rec.contains(KEY));
    let view = serde_json::to_string(&ep).expect("ser");
    assert!(!view.contains(KEY));
}

#[test]
fn a_keyring_failure_adds_nothing() {
    let kr = MemKeyring {
        fail: true,
        ..Default::default()
    };
    let mut b = Book::new(Ledger::new(0, T0));
    assert!(add_endpoint_with_key(&mut b, &kr, &input(), KEY, "ep9".into(), T0).is_err());
    assert!(b.endpoints.is_empty());
}

#[test]
fn a_key_with_header_breaking_bytes_is_refused() {
    let kr = MemKeyring::default();
    let mut b = Book::new(Ledger::new(0, T0));
    assert!(add_endpoint_with_key(&mut b, &kr, &input(), "sk bad\r\n", "e".into(), T0).is_err());
    assert!(add_endpoint_with_key(&mut b, &kr, &input(), "", "e".into(), T0).is_err());
}

#[test]
fn removing_an_endpoint_deletes_its_key_and_voids_its_quotes() {
    let kr = MemKeyring::default();
    let mut b = Book::new(Ledger::new(1_000_000, T0));
    add_endpoint_with_key(&mut b, &kr, &input(), KEY, "ep1".into(), T0).expect("add");
    let q = quote(&mut b, "ep1", "q1", 64, T0);
    remove_endpoint_with_key(&mut b, &kr, "ep1").expect("remove");
    assert!(kr.map.lock().expect("lock").is_empty());
    let e = b
        .authorize("q1", q.cost_micros, false, false, T0, "x1".into())
        .expect_err("voided");
    assert!(matches!(e, EscError::UnknownQuote));
}

// ---------------------------------------------------------------------------
// Quotes: the price is computed and shown before anything runs
// ---------------------------------------------------------------------------

#[test]
fn a_quote_prices_the_worst_case_and_names_the_destination() {
    let (mut b, ep) = book(1_000_000);
    let q = quote(&mut b, &ep, "q1", 1000, T0);
    // input bound = bytes + 16 per message; one user message, no system prompt.
    let input_tokens = "Plan the mint page.".len() as u64 + 16;
    let expect = (input_tokens * 3_000_000 + 1000 * 15_000_000).div_ceil(1_000_000);
    assert_eq!(q.cost_micros, expect);
    assert_eq!(q.destination, "My planner · api.example.com");
    assert!(q.within_budget);
    assert_eq!(q.expires_ms, T0 + QUOTE_TTL_MS);
}

#[test]
fn a_quote_for_an_unknown_endpoint_is_refused() {
    let (mut b, _) = book(1_000_000);
    assert!(b.quote("nope", "x", None, 10, T0, "q".into()).is_err());
}

#[test]
fn a_quote_needs_a_bounded_prompt_and_token_count() {
    let (mut b, ep) = book(1_000_000);
    assert!(b.quote(&ep, "  ", None, 10, T0, "q".into()).is_err());
    assert!(b.quote(&ep, "x", None, 0, T0, "q".into()).is_err());
    assert!(b
        .quote(&ep, "x", None, MAX_ESCALATION_TOKENS + 1, T0, "q".into())
        .is_err());
    assert!(b
        .quote(
            &ep,
            &"x".repeat(MAX_PROMPT_BYTES + 1),
            None,
            10,
            T0,
            "q".into()
        )
        .is_err());
}

#[test]
fn outstanding_quotes_are_bounded() {
    let (mut b, ep) = book(1_000_000);
    for n in 0..(MAX_QUOTES + 5) {
        quote(&mut b, &ep, &format!("q{n}"), 8, T0 + n as u64);
    }
    assert!(b.quotes.len() <= MAX_QUOTES);
}

#[test]
fn no_escalation_without_the_shown_price() {
    let (mut b, ep) = book(1_000_000);
    let q = quote(&mut b, &ep, "q1", 64, T0);
    let e = b
        .authorize("q1", q.cost_micros + 1, false, false, T0, "x1".into())
        .expect_err("mismatch");
    assert!(matches!(e, EscError::PriceNotShown { .. }));
    // An unknown quote id never runs either.
    assert!(matches!(
        b.authorize("q-never", 1, true, false, T0, "x2".into()),
        Err(EscError::UnknownQuote)
    ));
}

#[test]
fn an_expired_quote_does_not_run() {
    let (mut b, ep) = book(1_000_000);
    let q = quote(&mut b, &ep, "q1", 64, T0);
    let e = b
        .authorize(
            "q1",
            q.cost_micros,
            true,
            false,
            T0 + QUOTE_TTL_MS + 1,
            "x".into(),
        )
        .expect_err("expired");
    assert!(matches!(e, EscError::QuoteExpired));
}

#[test]
fn a_quote_runs_at_most_once() {
    let (mut b, ep) = book(1_000_000);
    let q = quote(&mut b, &ep, "q1", 64, T0);
    b.authorize("q1", q.cost_micros, false, false, T0, "x1".into())
        .expect("first");
    assert!(matches!(
        b.authorize("q1", q.cost_micros, false, false, T0, "x2".into()),
        Err(EscError::UnknownQuote)
    ));
}

// ---------------------------------------------------------------------------
// The budget: SpendWithinCap, HIC-1 over budget, taint, period reset
// ---------------------------------------------------------------------------

#[test]
fn within_budget_runs_without_asking_and_reserves_write_ahead() {
    let (mut b, ep) = book(1_000_000);
    let q = quote(&mut b, &ep, "q1", 64, T0);
    let a = b
        .authorize("q1", q.cost_micros, false, false, T0, "x1".into())
        .expect("within budget");
    assert_eq!(a.mode, Mode::Budget);
    assert_eq!(b.ledger.reserved_micros, q.cost_micros);
    assert_eq!(b.ledger.used(), q.cost_micros);
}

#[test]
fn over_budget_asks_and_keeps_the_quote_for_the_confirmation() {
    let (mut b, ep) = book(10);
    let q = quote(&mut b, &ep, "q1", 64, T0);
    assert!(!q.within_budget);
    match b.authorize("q1", q.cost_micros, false, false, T0, "x1".into()) {
        Err(EscError::NeedsConfirmation { cost_micros, .. }) => {
            assert_eq!(cost_micros, q.cost_micros)
        }
        other => panic!("expected NeedsConfirmation, got {other:?}"),
    }
    assert_eq!(b.ledger.used(), 0, "nothing reserved without the member");
    let a = b
        .authorize("q1", q.cost_micros, true, false, T0, "x1".into())
        .expect("confirmed");
    assert_eq!(a.mode, Mode::Confirmed);
    // A confirmed escalation is the member's one-off decision: it never counts against the cap.
    assert_eq!(b.ledger.used(), 0);
    assert!(b.ledger.used() <= b.ledger.cap_micros);
}

#[test]
fn the_default_cap_is_zero_so_every_escalation_asks() {
    assert_eq!(DEFAULT_DAILY_CAP_MICROS, 0);
    let (mut b, ep) = book(DEFAULT_DAILY_CAP_MICROS);
    let q = quote(&mut b, &ep, "q1", 8, T0);
    assert!(!q.within_budget);
    assert!(matches!(
        b.authorize("q1", q.cost_micros, false, false, T0, "x".into()),
        Err(EscError::NeedsConfirmation { .. })
    ));
}

#[test]
fn untrusted_context_always_asks_even_within_budget() {
    let (mut b, ep) = book(1_000_000_000);
    let q = quote(&mut b, &ep, "q1", 8, T0);
    assert!(q.within_budget);
    match b.authorize("q1", q.cost_micros, false, true, T0, "x".into()) {
        Err(EscError::NeedsConfirmation { reason, .. }) => assert!(reason.contains("untrusted")),
        other => panic!("expected NeedsConfirmation, got {other:?}"),
    }
    let a = b
        .authorize("q1", q.cost_micros, true, true, T0, "x".into())
        .expect("confirmed");
    assert_eq!(a.mode, Mode::Confirmed);
}

#[test]
fn spend_never_exceeds_the_cap_across_many_escalations() {
    let (mut b, ep) = book(5_000);
    let mut n = 0;
    loop {
        let id = format!("q{n}");
        let q = quote(&mut b, &ep, &id, 64, T0);
        match b.authorize(&id, q.cost_micros, false, false, T0, format!("x{n}")) {
            Ok(a) => {
                b.settle(&a.escalation_id, Settlement::MaybeSent, T0);
            }
            Err(EscError::NeedsConfirmation { .. }) => break,
            Err(e) => panic!("{e}"),
        }
        assert!(b.ledger.used() <= b.ledger.cap_micros);
        n += 1;
        assert!(n < 1000);
    }
    assert!(n > 0, "at least one escalation fit");
}

#[test]
fn settlement_charges_at_most_the_reservation_and_releases_the_rest() {
    let (mut b, ep) = book(1_000_000);
    let q = quote(&mut b, &ep, "q1", 64, T0);
    let a = b
        .authorize("q1", q.cost_micros, false, false, T0, "x1".into())
        .expect("ok");
    let rec = b.settle(
        &a.escalation_id,
        Settlement::Answered {
            charged_micros: q.cost_micros + 999,
            usage_reported: true,
            exceeded_quote: false,
        },
        T0,
    );
    assert_eq!(
        rec.charged_micros, q.cost_micros,
        "capped at the reservation"
    );
    assert_eq!(b.ledger.reserved_micros, 0);
    assert_eq!(b.ledger.committed_micros, q.cost_micros);
}

#[test]
fn settlement_of_a_request_that_never_left_charges_nothing() {
    let (mut b, ep) = book(1_000_000);
    let q = quote(&mut b, &ep, "q1", 64, T0);
    let a = b
        .authorize("q1", q.cost_micros, false, false, T0, "x1".into())
        .expect("ok");
    let rec = b.settle(&a.escalation_id, Settlement::NotSent, T0);
    assert_eq!(rec.charged_micros, 0);
    assert_eq!(b.ledger.used(), 0);
}

#[test]
fn settlement_of_a_request_that_may_have_been_sent_keeps_the_full_charge() {
    let (mut b, ep) = book(1_000_000);
    let q = quote(&mut b, &ep, "q1", 64, T0);
    let a = b
        .authorize("q1", q.cost_micros, false, false, T0, "x1".into())
        .expect("ok");
    let rec = b.settle(&a.escalation_id, Settlement::MaybeSent, T0);
    assert_eq!(rec.charged_micros, q.cost_micros);
    assert_eq!(b.ledger.committed_micros, q.cost_micros);
}

#[test]
fn settling_an_unknown_escalation_changes_nothing() {
    let (mut b, _) = book(1_000_000);
    let before = b.ledger.clone();
    b.settle("ghost", Settlement::MaybeSent, T0);
    assert_eq!(b.ledger.committed_micros, before.committed_micros);
    assert_eq!(b.ledger.reserved_micros, before.reserved_micros);
}

#[test]
fn the_budget_resets_only_at_the_utc_day_boundary() {
    let (mut b, ep) = book(1_000_000);
    let q = quote(&mut b, &ep, "q1", 64, T0);
    let a = b
        .authorize("q1", q.cost_micros, false, false, T0, "x1".into())
        .expect("ok");
    b.settle(&a.escalation_id, Settlement::MaybeSent, T0);
    let spent = b.ledger.used();
    // Later the same day: no reset.
    b.ledger.roll(T0 + DAY / 2);
    assert_eq!(b.ledger.used(), spent);
    // A clock that moves backwards never resets either.
    b.ledger.roll(T0 - 5 * DAY);
    assert_eq!(b.ledger.used(), spent);
    // The next UTC day: reset.
    b.ledger.roll((T0 / DAY + 1) * DAY);
    assert_eq!(b.ledger.used(), 0);
}

#[test]
fn a_reservation_from_yesterday_settles_without_touching_todays_budget() {
    let (mut b, ep) = book(1_000_000);
    let q = quote(&mut b, &ep, "q1", 64, T0);
    let a = b
        .authorize("q1", q.cost_micros, false, false, T0, "x1".into())
        .expect("ok");
    let tomorrow = (T0 / DAY + 1) * DAY + 10;
    b.ledger.roll(tomorrow);
    b.settle(&a.escalation_id, Settlement::MaybeSent, tomorrow);
    assert_eq!(b.ledger.used(), 0);
}

#[test]
fn the_cap_cannot_be_lowered_below_what_today_already_used() {
    let (mut b, ep) = book(1_000_000);
    let q = quote(&mut b, &ep, "q1", 64, T0);
    b.authorize("q1", q.cost_micros, false, false, T0, "x1".into())
        .expect("ok");
    assert!(b.ledger.set_cap(q.cost_micros - 1, T0).is_err());
    assert!(b.ledger.set_cap(q.cost_micros, T0).is_ok());
    assert!(b.ledger.set_cap(MAX_DAILY_CAP_MICROS + 1, T0).is_err());
}

#[test]
fn an_unreadable_ledger_fails_closed_to_asking() {
    let mut l = Ledger::new(1_000_000_000, T0);
    l.unreadable = true;
    let mut b = Book::new(l);
    b.add_endpoint(&input(), "ep1".into(), T0).expect("add");
    let q = quote(&mut b, "ep1", "q1", 8, T0);
    assert!(!q.within_budget);
    assert!(matches!(
        b.authorize("q1", q.cost_micros, false, false, T0, "x".into()),
        Err(EscError::NeedsConfirmation { .. })
    ));
}

#[test]
fn history_is_bounded_and_records_mode_and_destination() {
    let (mut b, ep) = book(1_000_000_000);
    for n in 0..(HISTORY_CAP + 3) {
        let id = format!("q{n}");
        let q = quote(&mut b, &ep, &id, 1, T0);
        let a = b
            .authorize(&id, q.cost_micros, false, false, T0, format!("x{n}"))
            .expect("ok");
        b.settle(&a.escalation_id, Settlement::NotSent, T0);
    }
    assert_eq!(b.ledger.history.len(), HISTORY_CAP);
    let last = b.ledger.history.back().expect("last");
    assert_eq!(last.mode, Mode::Budget);
    assert_eq!(last.destination, "My planner · api.example.com");
}

// ---------------------------------------------------------------------------
// The sidecar request and its answer
// ---------------------------------------------------------------------------

#[test]
fn the_sidecar_body_carries_the_key_and_exact_quoted_text_and_debug_redacts() {
    let (mut b, ep) = book(1_000_000);
    let q = quote(&mut b, &ep, "q1", 64, T0);
    let a = b
        .authorize("q1", q.cost_micros, false, false, T0, "x1".into())
        .expect("ok");
    let body = sidecar_body(&a, KEY);
    let v: serde_json::Value = serde_json::from_str(body.as_str()).expect("json");
    assert_eq!(v["apiKey"], KEY);
    assert_eq!(v["prompt"], "Plan the mint page.");
    assert_eq!(v["reservedMicros"], q.cost_micros);
    assert_eq!(v["baseUrl"], "https://api.example.com/v1");
    assert_eq!(v["maxTokens"], 64);
    assert_eq!(v["price"]["inputMicrosPerMtok"], 3_000_000);
    assert!(!format!("{a:?}").contains(KEY));
}

#[test]
fn a_sidecar_answer_is_interpreted_for_settlement() {
    let ok = serde_json::json!({"escalationId": "x1", "content": "the plan", "chargedMicros": 42, "usageReported": true, "exceededQuote": false}).to_string();
    match interpret_sidecar(200, &ok) {
        Ok((
            Settlement::Answered {
                charged_micros,
                usage_reported,
                ..
            },
            text,
        )) => {
            assert_eq!(charged_micros, 42);
            assert!(usage_reported);
            assert_eq!(text, "the plan");
        }
        other => panic!("{other:?}"),
    }
    match interpret_sidecar(422, r#"{"error":"bad","sent":false}"#) {
        Err((Settlement::NotSent, m)) => assert!(m.contains("bad")),
        other => panic!("{other:?}"),
    }
    match interpret_sidecar(502, r#"{"error":"HTTP 500","sent":true}"#) {
        Err((Settlement::MaybeSent, _)) => {}
        other => panic!("{other:?}"),
    }
    // An answer core cannot read is treated as possibly sent (over-count is the safe side).
    assert!(matches!(
        interpret_sidecar(500, "garbage"),
        Err((Settlement::MaybeSent, _))
    ));
    assert!(matches!(
        interpret_sidecar(200, "garbage"),
        Err((Settlement::MaybeSent, _))
    ));
}

// ---------------------------------------------------------------------------
// Persistence
// ---------------------------------------------------------------------------

fn tmp_dir(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!(
        "citrate-escalation-test-{tag}-{}-{}",
        std::process::id(),
        rand::random::<u32>()
    ));
    let _ = std::fs::create_dir_all(&d);
    d
}

#[test]
fn the_ledger_and_endpoints_round_trip_and_a_restart_never_resets_the_cap() {
    let dir = tmp_dir("rt");
    let (mut b, ep) = book(1_000_000);
    let q = quote(&mut b, &ep, "q1", 64, T0);
    b.authorize("q1", q.cost_micros, false, false, T0, "x1".into())
        .expect("ok");
    save_book(&dir, &b).expect("save");
    let b2 = load_book(&dir, T0);
    assert_eq!(b2.ledger.used(), q.cost_micros);
    assert_eq!(b2.ledger.cap_micros, 1_000_000);
    assert_eq!(b2.endpoints.len(), 1);
    assert!(b2.quotes.is_empty(), "quotes are never persisted");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_fresh_install_starts_with_the_default_cap() {
    let dir = tmp_dir("fresh");
    let b = load_book(&dir, T0);
    assert_eq!(b.ledger.cap_micros, DEFAULT_DAILY_CAP_MICROS);
    assert!(!b.ledger.unreadable);
    assert!(b.endpoints.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_corrupt_ledger_file_fails_closed_and_is_kept_aside() {
    let dir = tmp_dir("corrupt");
    std::fs::write(dir.join(LEDGER_FILE), b"{ not json").expect("write");
    let b = load_book(&dir, T0);
    assert!(b.ledger.unreadable);
    assert_eq!(b.ledger.cap_micros, 0);
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// Registry route: disabled, honestly
// ---------------------------------------------------------------------------

#[test]
fn the_registry_route_is_disabled_and_names_what_is_missing() {
    let s = registry_status(None, &[]);
    assert!(!s.enabled);
    assert!(s.missing.iter().any(|m| m.contains("InferenceRouter")));
    assert!(s.missing.iter().any(|m| m.contains("x402")));
    // Even with an address and an asset, the EIP-712 precondition keeps it off in this build.
    let s = registry_status(
        Some("0x1111111111111111111111111111111111111111"),
        &["0x2222222222222222222222222222222222222222"],
    );
    assert!(!s.enabled);
    assert!(!s.missing.iter().any(|m| m.contains("InferenceRouter")));
    assert!(s.missing.iter().any(|m| m.contains("EIP-712")));
}

#[test]
fn the_x402_asset_allowlist_is_empty_in_this_build() {
    assert!(X402_ASSET_ALLOWLIST.is_empty());
}

#[test]
fn the_address_book_has_no_inference_router_pinned_yet() {
    // Federation F-4: the post-reroll redeploy has not pinned it. When it does, this test is the
    // reminder to revisit the registry route.
    assert_eq!(crate::addresses::inference_router(), None);
}

/// Cross-repo golden: the sidecar (citrate-agent-runtime `agent-escalation`
/// `worst_case_golden_shared_with_core`) refuses a reservation below its own worst case, so core's
/// quote for the same request must be exactly this.
#[test]
fn the_quote_matches_the_sidecars_worst_case_golden() {
    let mut b = Book::new(Ledger::new(1_000_000, T0));
    let mut i = input();
    i.input_micros_per_mtok = 1_000_000;
    i.output_micros_per_mtok = 2_000_000;
    b.add_endpoint(&i, "g".into(), T0).expect("add");
    let q = b
        .quote("g", "Plan it.", None, 64, T0, "qg".into())
        .expect("quote");
    assert_eq!(q.cost_micros, 152);
    let q = b
        .quote("g", "Plan it.", Some(""), 64, T0, "qg2".into())
        .expect("quote");
    assert_eq!(q.cost_micros, 152);
}
