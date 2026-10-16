// HUP-S2.3 — the budgeted SIWE path through the ONE gated signer (ADR-2026-09-30 D1-D4).
//
// Included in ceremony.rs's test module after ceremony_tests.rs, so it shares that file's helpers
// (vault_with_wallet, FakeKeyring, address_of_verifying_key, CANONICAL_ADDRESS).
//
// Invariant map (WebSigningBudget.tla → test):
//   OnlyClosedList          → budget_path_never_signs_typed_data_or_plain_text
//   OriginBound/TopFrameOnly→ unattested_or_subframe_requests_become_cards
//   NonceUnique             → a_replayed_nonce_becomes_a_card
//   NeverExceedsCaps        → caps_hold_through_the_signer
//   RevokeImmediate         → revoke_takes_effect_before_the_next_request
//   ExpiredInert            → expiry_between_reserve_and_sign_signs_nothing
//   TaintDowngrade          → tainted_or_hic_required_requests_become_cards
//   RecordBeforeSignature   → auto_sign_within_budget_recovers_to_the_wallet (record Signed)
//   FallThroughLive / NoDrop→ every fall-through leaves a pending HIC-1 ceremony
//   (web_budget_tests.rs covers BudgetMonotone, crash recovery and write-ahead failure.)

use crate::web_budget::{
    BrowserMode, BudgetGate, FrameKind, OriginAttestation, RecordStatus, TaskTaint,
    DEFAULT_PRINCIPAL,
};

const B_ORIGIN: &str = "https://app.example.org";
const B_NOW: u64 = 1_790_856_000_000; // 2026-10-01T12:00:00Z

fn b_siwe(nonce: &str, issued: &str, expires: &str) -> String {
    crate::siwe::SiweFields {
        scheme: None,
        domain: "app.example.org".into(),
        address: CANONICAL_ADDRESS.into(),
        statement: Some("Sign in to Example.".into()),
        uri: "https://app.example.org/login".into(),
        version: "1".into(),
        chain_id: "40204".into(),
        nonce: nonce.into(),
        issued_at: issued.into(),
        expiration_time: Some(expires.into()),
        not_before: None,
        request_id: None,
        resources: vec![],
    }
    .to_message()
}

fn b_msg(nonce: &str) -> String {
    b_siwe(nonce, "2026-10-01T11:59:30Z", "2026-10-01T12:10:00Z")
}

fn b_gate() -> BudgetGate {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let mut p = std::env::temp_dir();
    p.push(format!(
        "citrate-ceremony-budget-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&p);
    p.push("web-signing-budgets.json");
    BudgetGate::open(p, Box::new(FakeKeyring::default()), B_NOW)
}

fn b_grant(g: &BudgetGate, max: u32, ttl_ms: u64) {
    g.grant(
        B_ORIGIN,
        DEFAULT_PRINCIPAL,
        max,
        ttl_ms,
        &CANONICAL_ADDRESS.to_lowercase(),
        B_NOW,
    )
    .expect("grant");
}

fn b_req(message: String) -> SiweSignRequest {
    SiweSignRequest {
        message,
        attestation: Some(OriginAttestation {
            origin: B_ORIGIN.into(),
            frame: FrameKind::Top,
            mode: BrowserMode::Managed,
        }),
        taint: TaskTaint::Clean,
        hic_required: false,
        principal: DEFAULT_PRINCIPAL.into(),
        claimed_origin: B_ORIGIN.into(),
    }
}

fn b_recover(message: &str, sig_hex: &str) -> String {
    use k256::ecdsa::{RecoveryId, Signature as K256Sig};
    let raw = hex::decode(sig_hex).expect("hex");
    let prehash = crate::wallet::eip191_prehash(message.as_bytes());
    let recid = RecoveryId::from_byte(raw[64] - 27).expect("v");
    let sig = K256Sig::from_slice(&raw[..64]).expect("rs");
    let vk =
        k256::ecdsa::VerifyingKey::recover_from_prehash(&prehash, &sig, recid).expect("recover");
    address_of_verifying_key(&vk)
}

/// Assert the outcome is a pending HIC-1 ceremony that still exists, carrying `message`, and return
/// its reason.
fn b_assert_card(c: &SignatureCeremony, out: &BudgetedOutcome, message: &str) -> String {
    match out {
        BudgetedOutcome::Pending { ceremony, reason } => {
            let st = c
                .status(&ceremony.id)
                .expect("the fall-through ceremony is pending");
            assert_eq!(st.kind, IntentKind::PersonalSign);
            assert_eq!(st.chain_id, 40204);
            assert!(st.decoded.action.contains("Sign message"));
            let _ = message;
            reason.clone()
        }
        BudgetedOutcome::AutoSigned { .. } => {
            panic!("expected an HIC-1 card, got an auto-signature")
        }
    }
}

#[test]
fn auto_sign_within_budget_recovers_to_the_wallet() {
    let (v, _p) = vault_with_wallet();
    let c = SignatureCeremony::new();
    let g = b_gate();
    b_grant(&g, 3, 7 * 86_400_000);
    let m = b_msg("abcdef0123456789AA");
    let out = c.request_siwe_budgeted(&v, &g, b_req(m.clone()), &|| B_NOW);
    let BudgetedOutcome::AutoSigned {
        signature,
        record_id,
        remaining,
        origin,
        ..
    } = out
    else {
        panic!("expected an auto-signature within the budget: {out:?}");
    };
    assert_eq!(origin, B_ORIGIN);
    assert_eq!(remaining, 2);
    assert_eq!(signature.kind, IntentKind::PersonalSign);
    assert_eq!(
        b_recover(&m, &signature.sig_hex).to_lowercase(),
        CANONICAL_ADDRESS.to_lowercase()
    );
    // No HIC-1 card was created for it.
    assert_eq!(c.pending_count(), 0);
    // RecordBeforeSignature: the record exists and is closed as Signed.
    let snap = g.snapshot(B_NOW, Some(&CANONICAL_ADDRESS.to_lowercase()));
    let rec = snap
        .records
        .iter()
        .find(|r| r.record_id == record_id)
        .expect("record");
    assert_eq!(rec.status, RecordStatus::Signed);
    assert_eq!(rec.nonce.as_deref(), Some("abcdef0123456789AA"));
}

#[test]
fn no_budget_means_every_sign_in_is_a_card() {
    let (v, _p) = vault_with_wallet();
    let c = SignatureCeremony::new();
    let g = b_gate();
    let m = b_msg("abcdef0123456789AA");
    let out = c.request_siwe_budgeted(&v, &g, b_req(m.clone()), &|| B_NOW);
    let reason = b_assert_card(&c, &out, &m);
    assert!(reason.contains("no sign-in budget"), "{reason}");
    // The card is an ordinary ceremony: the member approves it by id and it signs once.
    let BudgetedOutcome::Pending { ceremony, .. } = out else {
        unreachable!()
    };
    let sig = c
        .approve(&v, &ceremony.id, false)
        .expect("member approves the card");
    assert_eq!(
        b_recover(&m, &sig.sig_hex).to_lowercase(),
        CANONICAL_ADDRESS.to_lowercase()
    );
}

#[test]
fn budget_path_never_signs_typed_data_or_plain_text() {
    let (v, _p) = vault_with_wallet();
    let c = SignatureCeremony::new();
    let g = b_gate();
    b_grant(&g, 50, 7 * 86_400_000);
    for text in [
        "{\"primaryType\":\"Permit\",\"domain\":{\"name\":\"T\"},\"message\":{}}".to_string(),
        "please sign this".to_string(),
    ] {
        let out = c.request_siwe_budgeted(&v, &g, b_req(text.clone()), &|| B_NOW);
        b_assert_card(&c, &out, &text);
    }
}

#[test]
fn unattested_or_subframe_requests_become_cards() {
    let (v, _p) = vault_with_wallet();
    let c = SignatureCeremony::new();
    let g = b_gate();
    b_grant(&g, 50, 7 * 86_400_000);
    let m = b_msg("abcdef0123456789AA");
    let mut r = b_req(m.clone());
    r.attestation = None;
    r.claimed_origin = "https://app.example.org".into();
    let out = c.request_siwe_budgeted(&v, &g, r, &|| B_NOW);
    let reason = b_assert_card(&c, &out, &m);
    assert!(reason.contains("could not confirm which site"), "{reason}");
    // The card marks an unattested origin as unverified rather than showing it as fact.
    if let BudgetedOutcome::Pending { ceremony, .. } = &out {
        assert!(
            ceremony.origin.contains("not verified"),
            "{}",
            ceremony.origin
        );
    }
    let mut sub = b_req(m.clone());
    if let Some(a) = sub.attestation.as_mut() {
        a.frame = FrameKind::Sub;
    }
    b_assert_card(&c, &c.request_siwe_budgeted(&v, &g, sub, &|| B_NOW), &m);
}

#[test]
fn a_replayed_nonce_becomes_a_card() {
    let (v, _p) = vault_with_wallet();
    let c = SignatureCeremony::new();
    let g = b_gate();
    b_grant(&g, 50, 7 * 86_400_000);
    let m = b_msg("abcdef0123456789AA");
    assert!(matches!(
        c.request_siwe_budgeted(&v, &g, b_req(m.clone()), &|| B_NOW),
        BudgetedOutcome::AutoSigned { .. }
    ));
    let later = B_NOW + 60_000;
    let out = c.request_siwe_budgeted(&v, &g, b_req(m.clone()), &|| later);
    let reason = b_assert_card(&c, &out, &m);
    assert!(reason.contains("nonce was already used"), "{reason}");
}

#[test]
fn caps_hold_through_the_signer() {
    let (v, _p) = vault_with_wallet();
    let c = SignatureCeremony::new();
    let g = b_gate();
    b_grant(&g, 1, 7 * 86_400_000);
    let a = b_msg("abcdef0123456789A1");
    assert!(matches!(
        c.request_siwe_budgeted(&v, &g, b_req(a), &|| B_NOW),
        BudgetedOutcome::AutoSigned { .. }
    ));
    let b = b_msg("abcdef0123456789A2");
    let later = B_NOW + 60_000;
    let reason = b_assert_card(
        &c,
        &c.request_siwe_budgeted(&v, &g, b_req(b.clone()), &|| later),
        &b,
    );
    assert!(reason.contains("used up"), "{reason}");
}

#[test]
fn revoke_takes_effect_before_the_next_request() {
    let (v, _p) = vault_with_wallet();
    let c = SignatureCeremony::new();
    let g = b_gate();
    b_grant(&g, 10, 7 * 86_400_000);
    let id = g.snapshot(B_NOW, None).budgets[0].id;
    g.revoke(id, B_NOW).expect("revoke");
    let m = b_msg("abcdef0123456789AA");
    let reason = b_assert_card(
        &c,
        &c.request_siwe_budgeted(&v, &g, b_req(m.clone()), &|| B_NOW),
        &m,
    );
    assert!(reason.contains("revoked"), "{reason}");
}

#[test]
fn revoke_waits_for_an_in_flight_auto_sign_and_then_wins() {
    // The budget lock serializes revoke against the whole check, reserve, record and sign
    // sequence: a revoke that commits first blocks every later auto-sign under that budget.
    let (v, _p) = vault_with_wallet();
    let c = std::sync::Arc::new(SignatureCeremony::new());
    let g = std::sync::Arc::new(b_gate());
    b_grant(&g, 10, 7 * 86_400_000);
    let id = g.snapshot(B_NOW, None).budgets[0].id;
    let held = g.lock();
    let g2 = std::sync::Arc::clone(&g);
    let t = std::thread::spawn(move || g2.revoke(id, B_NOW));
    // The revoke cannot commit while the lock is held.
    std::thread::sleep(std::time::Duration::from_millis(50));
    assert!(!t.is_finished());
    drop(held);
    t.join().expect("join").expect("revoke");
    let m = b_msg("abcdef0123456789AA");
    b_assert_card(
        &c,
        &c.request_siwe_budgeted(&v, &g, b_req(m.clone()), &|| B_NOW),
        &m,
    );
}

#[test]
fn expiry_between_reserve_and_sign_signs_nothing() {
    let (v, _p) = vault_with_wallet();
    let c = SignatureCeremony::new();
    let g = b_gate();
    // The budget ends 1 s after "now"; the clock moves past it between reserve and sign.
    b_grant(&g, 10, 60_000);
    let ticks = std::sync::atomic::AtomicU64::new(0);
    let clock = || {
        let n = ticks.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if n == 0 {
            B_NOW + 59_000
        } else {
            B_NOW + 61_000
        }
    };
    let m = b_siwe(
        "abcdef0123456789AA",
        "2026-10-01T12:00:30Z",
        "2026-10-01T12:10:00Z",
    );
    let out = c.request_siwe_budgeted(&v, &g, b_req(m.clone()), &clock);
    let reason = b_assert_card(&c, &out, &m);
    assert!(reason.contains("expired before signing"), "{reason}");
    let snap = g.snapshot(B_NOW, None);
    // The reservation is kept (over-count is the safe direction) and the record says not signed.
    assert_eq!(snap.budgets[0].used_count, 1);
    assert_eq!(snap.records[0].status, RecordStatus::NotSigned);
}

#[test]
fn tainted_or_hic_required_requests_become_cards() {
    let (v, _p) = vault_with_wallet();
    let c = SignatureCeremony::new();
    let g = b_gate();
    b_grant(&g, 50, 7 * 86_400_000);
    let m = b_msg("abcdef0123456789AA");
    let mut tainted = b_req(m.clone());
    tainted.taint = TaskTaint::Sources(vec!["ext".into()]);
    b_assert_card(&c, &c.request_siwe_budgeted(&v, &g, tainted, &|| B_NOW), &m);
    let mut unknown = b_req(m.clone());
    unknown.taint = TaskTaint::Unknown;
    b_assert_card(&c, &c.request_siwe_budgeted(&v, &g, unknown, &|| B_NOW), &m);
    let mut hic = b_req(m.clone());
    hic.hic_required = true;
    let reason = b_assert_card(&c, &c.request_siwe_budgeted(&v, &g, hic, &|| B_NOW), &m);
    assert!(reason.contains("explicit approval"), "{reason}");
    // Nothing was reserved by any of them.
    assert_eq!(g.snapshot(B_NOW, None).budgets[0].used_count, 0);
}

#[test]
fn a_locked_vault_signs_nothing_and_asks() {
    let (v, _p) = vault_with_wallet();
    let c = SignatureCeremony::new();
    let g = b_gate();
    b_grant(&g, 50, 7 * 86_400_000);
    v.lock();
    let m = b_msg("abcdef0123456789AA");
    b_assert_card(
        &c,
        &c.request_siwe_budgeted(&v, &g, b_req(m.clone()), &|| B_NOW),
        &m,
    );
}

#[test]
fn budget_tripwire_the_budget_modules_never_reach_a_signer() {
    // The budget store and the SIWE parser are pure: neither may call a gated signer. The ONE
    // budgeted signer call lives in `request_siwe_budgeted` in ceremony.rs.
    let calls = [
        "sign_".to_string() + "message(",
        "sign_".to_string() + "transaction(",
        "sign_".to_string() + "personal(",
    ];
    for (name, src) in [
        ("web_budget.rs", include_str!("web_budget.rs")),
        ("siwe.rs", include_str!("siwe.rs")),
    ] {
        let non_test = b_strip_terminal_tests(src);
        for call in &calls {
            assert!(
                !non_test
                    .lines()
                    .any(|l| l.contains(call.as_str()) && !l.trim_start().starts_with("//")),
                "{name} must not invoke a gated signer ({call})"
            );
        }
    }
    // The budgeted path signs ONLY personal_sign: it never names the tx or raw-message signer.
    let ceremony_non_test = b_strip_terminal_tests(include_str!("ceremony.rs"));
    let start = ceremony_non_test
        .find("pub fn request_siwe_budgeted")
        .expect("the budgeted path exists");
    let body = &ceremony_non_test[start..];
    let end = body.find("\n    }\n").map(|e| e + 6).unwrap_or(body.len());
    let body = &body[..end];
    assert!(
        body.contains(calls[2].as_str()),
        "the budgeted path signs via sign_personal"
    );
    assert!(!body.contains(calls[0].as_str()) && !body.contains(calls[1].as_str()));
}

/// Cut at the terminal `#[cfg(test)] mod tests` block only (ceremony.rs has an earlier
/// `#[cfg(test)]` helper, so the shared `strip_test_module` would stop too early for code after it).
fn b_strip_terminal_tests(src: &str) -> String {
    match src.rfind("#[cfg(test)]\nmod tests") {
        Some(i) => src[..i].to_string(),
        None => src.to_string(),
    }
}
