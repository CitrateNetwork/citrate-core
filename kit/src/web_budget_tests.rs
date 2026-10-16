// HUP-S2.3 — the WebSigningBudget store (ADR-2026-09-30 D2 #19-21, D3 inert x402, D4).
//
// These drive the store directly (grant / revoke / evaluate / reserve / recover). The end-to-end
// path through the gated signer is in ceremony_budget_tests.rs. Each test names the
// WebSigningBudget.tla invariant or ADR clause it pins.

use super::*;
use crate::custody::{CustodyError, Keyring};
use crate::siwe::SiweFields;
use std::sync::{Arc, Mutex as StdMutex};

#[derive(Default, Clone)]
struct MemKeyring {
    store: Arc<StdMutex<std::collections::HashMap<String, Vec<u8>>>>,
    broken: bool,
}

impl Keyring for MemKeyring {
    fn get(&self, account: &str) -> std::result::Result<Option<Vec<u8>>, CustodyError> {
        if self.broken {
            return Err(CustodyError::KeyringUnavailable);
        }
        Ok(self.store.lock().unwrap().get(account).cloned())
    }
    fn set(&self, account: &str, secret: &[u8]) -> std::result::Result<(), CustodyError> {
        if self.broken {
            return Err(CustodyError::KeyringUnavailable);
        }
        self.store
            .lock()
            .unwrap()
            .insert(account.into(), secret.to_vec());
        Ok(())
    }
    fn delete(&self, account: &str) -> std::result::Result<(), CustodyError> {
        self.store.lock().unwrap().remove(account);
        Ok(())
    }
}

const WALLET: &str = "0x9858effd232b4033e47d90003d41ec34ecaeda94";
const ADDR: &str = "0x9858EfFD232B4033E47d90003D41EC34EcaEda94";
const ORIGIN: &str = "https://app.example.org";
const NOW: u64 = 1_790_856_000_000; // 2026-10-01T12:00:00Z
const MIN: u64 = 60_000;
const HOUR: u64 = 60 * MIN;
const DAY: u64 = 24 * HOUR;

fn tmp_path(tag: &str) -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let mut p = std::env::temp_dir();
    p.push(format!(
        "citrate-web-budget-{tag}-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p.push("web-signing-budgets.json");
    p
}

fn open(tag: &str) -> (BudgetGate, std::path::PathBuf, MemKeyring) {
    let p = tmp_path(tag);
    let k = MemKeyring::default();
    (BudgetGate::open(p.clone(), Box::new(k.clone()), NOW), p, k)
}

/// A SIWE message for ORIGIN issued `at` (epoch ms), expiring 10 min later, with `nonce`.
fn msg_at(nonce: &str, at: u64) -> String {
    SiweFields {
        scheme: None,
        domain: "app.example.org".into(),
        address: ADDR.into(),
        statement: Some("Sign in to Example.".into()),
        uri: "https://app.example.org/login".into(),
        version: "1".into(),
        chain_id: "40204".into(),
        nonce: nonce.into(),
        issued_at: rfc3339(at),
        expiration_time: Some(rfc3339(at + 10 * MIN)),
        not_before: None,
        request_id: None,
        resources: vec![],
    }
    .to_message()
}

fn rfc3339(ms: u64) -> String {
    // Inverse of days_from_civil for test fixtures (UTC, whole seconds).
    let secs = (ms / 1000) as i64;
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

fn top() -> OriginAttestation {
    OriginAttestation {
        origin: ORIGIN.into(),
        frame: FrameKind::Top,
        mode: BrowserMode::Managed,
    }
}

fn input<'a>(
    message: &'a str,
    att: Option<&'a OriginAttestation>,
    taint: &'a TaskTaint,
) -> SiweEvalInput<'a> {
    SiweEvalInput {
        message,
        attestation: att,
        taint,
        hic_required: false,
        principal: DEFAULT_PRINCIPAL,
        wallet_address: Some(WALLET),
    }
}

fn grant(g: &BudgetGate, max: u32) -> WebSigningBudget {
    g.grant(ORIGIN, DEFAULT_PRINCIPAL, max, 7 * DAY, WALLET, NOW)
        .expect("grant")
}

/// Evaluate + reserve under one lock acquisition (what the ceremony does before signing).
fn auto(
    g: &BudgetGate,
    message: &str,
    att: Option<&OriginAttestation>,
    taint: &TaskTaint,
    now: u64,
) -> Result<u64, FallThrough> {
    let mut guard = g.lock();
    let plan = guard.evaluate_siwe(&input(message, att, taint), now)?;
    guard.reserve(&plan, now)
}

#[test]
fn default_is_no_budget_so_every_request_asks() {
    let (g, _, _) = open("default");
    assert!(g.snapshot(NOW, Some(WALLET)).budgets.is_empty());
    let m = msg_at("abcdef0123456789AA", NOW);
    assert_eq!(
        auto(&g, &m, Some(&top()), &TaskTaint::Clean, NOW),
        Err(FallThrough::NoBudget)
    );
}

#[test]
fn placeholder_defaults_are_conservative_and_inside_the_o2_ceilings() {
    let (count, ttl_days, ceiling) = (
        std::hint::black_box(PLACEHOLDER_DEFAULT_MAX_COUNT),
        std::hint::black_box(PLACEHOLDER_DEFAULT_TTL_DAYS),
        std::hint::black_box(MAX_COUNT_CEILING),
    );
    assert!((1..=ceiling).contains(&count));
    assert!(u64::from(ttl_days) * DAY <= MAX_TTL_MS_CEILING);
    assert_eq!(MAX_COUNT_CEILING, 50);
    assert_eq!(MAX_TTL_MS_CEILING, 30 * DAY);
    assert!(std::hint::black_box(DEFAULTS_PENDING_OWNER_SIGNOFF));
}

#[test]
fn happy_path_reserves_and_writes_the_record_first() {
    let (g, _, _) = open("happy");
    let b = grant(&g, 2);
    let m = msg_at("abcdef0123456789AA", NOW);
    let rid = auto(&g, &m, Some(&top()), &TaskTaint::Clean, NOW).expect("eligible");
    let snap = g.snapshot(NOW, Some(WALLET));
    let rec = snap
        .records
        .iter()
        .find(|r| r.record_id == rid)
        .expect("record");
    // RecordBeforeSignature: the reservation record exists before any signature.
    assert_eq!(rec.kind, RecordKind::AutoSign);
    assert_eq!(rec.status, RecordStatus::Reserved);
    assert_eq!(rec.budget_id, Some(b.id));
    assert_eq!(rec.nonce.as_deref(), Some("abcdef0123456789AA"));
    assert_eq!(rec.statement.as_deref(), Some("Sign in to Example."));
    assert_eq!(rec.origin, ORIGIN);
    assert_eq!(rec.remaining_after, Some(1));
    assert!(rec.payload_digest.as_deref().is_some_and(|d| d.len() == 66));
    assert_eq!(snap.budgets[0].used_count, 1);
}

#[test]
fn origin_bound_and_top_frame_only() {
    let (g, _, _) = open("prov");
    grant(&g, 5);
    let m = msg_at("abcdef0123456789AA", NOW);
    // No attestation at all (the managed browser is not running): HIC-1.
    assert!(matches!(
        auto(&g, &m, None, &TaskTaint::Clean, NOW),
        Err(FallThrough::NotAttested)
    ));
    let mut sub = top();
    sub.frame = FrameKind::Sub;
    assert_eq!(
        auto(&g, &m, Some(&sub), &TaskTaint::Clean, NOW),
        Err(FallThrough::NotTopFrame)
    );
    let mut attach = top();
    attach.mode = BrowserMode::Attach;
    assert_eq!(
        auto(&g, &m, Some(&attach), &TaskTaint::Clean, NOW),
        Err(FallThrough::AttachMode)
    );
    // Attested on another origin: no budget there, and the message's domain would not bind.
    let other = OriginAttestation {
        origin: "https://other.example.org".into(),
        ..top()
    };
    assert_eq!(
        auto(&g, &m, Some(&other), &TaskTaint::Clean, NOW),
        Err(FallThrough::NoBudget)
    );
}

#[test]
fn message_checks_fall_through_with_the_reason() {
    let (g, _, _) = open("msg");
    grant(&g, 5);
    let m = msg_at("short", NOW);
    assert_eq!(
        auto(&g, &m, Some(&top()), &TaskTaint::Clean, NOW),
        Err(FallThrough::Message(crate::siwe::SiweReject::WeakNonce))
    );
}

#[test]
fn nonce_unique_per_origin() {
    let (g, _, _) = open("nonce");
    grant(&g, 5);
    let m = msg_at("abcdef0123456789AA", NOW);
    auto(&g, &m, Some(&top()), &TaskTaint::Clean, NOW).expect("first");
    // Same nonce later (past the burst gap): the ledger refuses it.
    let m2 = msg_at("abcdef0123456789AA", NOW + MIN);
    assert_eq!(
        auto(&g, &m2, Some(&top()), &TaskTaint::Clean, NOW + MIN),
        Err(FallThrough::NonceReused)
    );
}

#[test]
fn never_exceeds_caps_count_gap_and_window() {
    let (g, _, _) = open("caps");
    grant(&g, 2);
    let a = msg_at("abcdef0123456789A1", NOW);
    auto(&g, &a, Some(&top()), &TaskTaint::Clean, NOW).expect("1");
    // Burst gap: a second sign-in within 30 s goes to HIC-1.
    let b = msg_at("abcdef0123456789A2", NOW + 10_000);
    assert_eq!(
        auto(&g, &b, Some(&top()), &TaskTaint::Clean, NOW + 10_000),
        Err(FallThrough::RateGap)
    );
    let b2 = msg_at("abcdef0123456789A2", NOW + 31_000);
    auto(&g, &b2, Some(&top()), &TaskTaint::Clean, NOW + 31_000).expect("2");
    // max_count = 2 reached.
    let c = msg_at("abcdef0123456789A3", NOW + 2 * MIN);
    assert_eq!(
        auto(&g, &c, Some(&top()), &TaskTaint::Clean, NOW + 2 * MIN),
        Err(FallThrough::BudgetExhausted)
    );
    let snap = g.snapshot(NOW + 2 * MIN, Some(WALLET));
    assert_eq!(snap.budgets[0].used_count, 2);
    assert_eq!(snap.budgets[0].remaining, 0);
}

#[test]
fn rolling_window_caps_at_twenty_per_origin() {
    let (g, _, _) = open("window");
    g.grant(
        ORIGIN,
        DEFAULT_PRINCIPAL,
        MAX_COUNT_CEILING,
        7 * DAY,
        WALLET,
        NOW,
    )
    .expect("grant");
    for i in 0..SIWE_WINDOW_MAX as u64 {
        let t = NOW + i * MIN;
        let m = msg_at(&format!("abcdef0123456789{i:04}"), t);
        auto(&g, &m, Some(&top()), &TaskTaint::Clean, t).expect("inside window");
    }
    let t = NOW + 30 * MIN;
    let m = msg_at("abcdef0123456789ZZZZ", t);
    assert_eq!(
        auto(&g, &m, Some(&top()), &TaskTaint::Clean, t),
        Err(FallThrough::RateWindow)
    );
    // A rolling window, not a calendar day: 24 h after the first one, one slot frees up.
    let t2 = NOW + DAY + 30_000;
    let m2 = msg_at("abcdef0123456789YYYY", t2);
    auto(&g, &m2, Some(&top()), &TaskTaint::Clean, t2).expect("window rolled");
}

#[test]
fn taint_downgrade_with_the_o3_same_origin_exemption() {
    let (g, _, _) = open("taint");
    grant(&g, 10);
    let m = msg_at("abcdef0123456789AA", NOW);
    let other = TaskTaint::Sources(vec!["https://evil.example.com".into()]);
    assert_eq!(
        auto(&g, &m, Some(&top()), &other, NOW),
        Err(FallThrough::Tainted)
    );
    let ext = TaskTaint::Sources(vec!["ext".into()]);
    assert_eq!(
        auto(&g, &m, Some(&top()), &ext, NOW),
        Err(FallThrough::Tainted)
    );
    assert_eq!(
        auto(&g, &m, Some(&top()), &TaskTaint::Unknown, NOW),
        Err(FallThrough::TaintUnknown)
    );
    // O-3 accepted: content from the same allowlisted origin does not taint a sign-in to it.
    assert!(std::hint::black_box(O3_SAME_ORIGIN_EXEMPT));
    let same = TaskTaint::Sources(vec![ORIGIN.into()]);
    auto(&g, &m, Some(&top()), &same, NOW).expect("same-origin content is exempt");
}

#[test]
fn hic_required_calls_never_take_the_budget_path() {
    let (g, _, _) = open("hic");
    grant(&g, 10);
    let m = msg_at("abcdef0123456789AA", NOW);
    let guard = g.lock();
    let mut inp = input(&m, None, &TaskTaint::Clean);
    let t = top();
    inp.attestation = Some(&t);
    inp.hic_required = true;
    assert_eq!(
        guard.evaluate_siwe(&inp, NOW).err(),
        Some(FallThrough::HicRequired)
    );
}

#[test]
fn budgets_are_scoped_to_their_principal_and_wallet() {
    let (g, _, _) = open("principal");
    grant(&g, 10);
    let m = msg_at("abcdef0123456789AA", NOW);
    let t = top();
    let guard = g.lock();
    let mut inp = input(&m, Some(&t), &TaskTaint::Clean);
    inp.principal = "mcp:external-client";
    assert_eq!(
        guard.evaluate_siwe(&inp, NOW).err(),
        Some(FallThrough::NoBudget)
    );
    let mut inp2 = input(&m, Some(&t), &TaskTaint::Clean);
    inp2.wallet_address = Some("0x0000000000000000000000000000000000000001");
    assert_eq!(
        guard.evaluate_siwe(&inp2, NOW).err(),
        Some(FallThrough::WalletChanged)
    );
    let mut inp3 = input(&m, Some(&t), &TaskTaint::Clean);
    inp3.wallet_address = None;
    assert_eq!(
        guard.evaluate_siwe(&inp3, NOW).err(),
        Some(FallThrough::NoWallet)
    );
}

#[test]
fn expired_inert() {
    let (g, _, _) = open("expiry");
    g.grant(ORIGIN, DEFAULT_PRINCIPAL, 5, HOUR, WALLET, NOW)
        .expect("grant");
    let t = NOW + HOUR;
    let m = msg_at("abcdef0123456789AA", t);
    assert_eq!(
        auto(&g, &m, Some(&top()), &TaskTaint::Clean, t),
        Err(FallThrough::BudgetExpired)
    );
}

#[test]
fn still_signable_rechecks_expiry_after_the_reservation() {
    let (g, _, _) = open("recheck");
    g.grant(ORIGIN, DEFAULT_PRINCIPAL, 5, HOUR, WALLET, NOW)
        .expect("grant");
    let t = NOW + HOUR - 1000;
    let m = msg_at("abcdef0123456789AA", t);
    let mut guard = g.lock();
    let plan = guard
        .evaluate_siwe(&input(&m, Some(&top()), &TaskTaint::Clean), t)
        .expect("eligible");
    guard.reserve(&plan, t).expect("reserved");
    assert_eq!(guard.still_signable(&plan, t + 500), Ok(()));
    assert_eq!(
        guard.still_signable(&plan, NOW + HOUR),
        Err(FallThrough::ExpiredBeforeSigning)
    );
}

#[test]
fn revoke_immediate_and_revoke_all() {
    let (g, _, _) = open("revoke");
    let b = grant(&g, 10);
    g.revoke(b.id, NOW).expect("revoke");
    let m = msg_at("abcdef0123456789AA", NOW);
    assert_eq!(
        auto(&g, &m, Some(&top()), &TaskTaint::Clean, NOW),
        Err(FallThrough::BudgetRevoked)
    );
    // A revoked slot can be granted again (a new generation), never by widening the old one.
    let b2 = grant(&g, 3);
    assert_ne!(b2.id, b.id);
    let other = g
        .grant(
            "https://second.example.org",
            DEFAULT_PRINCIPAL,
            3,
            DAY,
            WALLET,
            NOW,
        )
        .expect("second origin");
    assert_eq!(
        g.revoke_all("member pressed Stop all autonomy", NOW)
            .expect("all"),
        2
    );
    let snap = g.snapshot(NOW, Some(WALLET));
    assert!(snap.budgets.iter().all(|v| v.revoked_at_ms.is_some()));
    assert!(snap.budgets.iter().any(|v| v.id == other.id));
    assert!(snap
        .records
        .iter()
        .any(|r| r.kind == RecordKind::AllBudgetsRevoked));
}

#[test]
fn grant_never_widens_a_live_budget_and_respects_ceilings() {
    let (g, _, _) = open("grant");
    grant(&g, 5);
    assert_eq!(
        g.grant(ORIGIN, DEFAULT_PRINCIPAL, 6, DAY, WALLET, NOW)
            .err(),
        Some(BudgetError::LiveBudgetExists)
    );
    assert_eq!(
        g.grant(
            "https://b.example.org",
            DEFAULT_PRINCIPAL,
            MAX_COUNT_CEILING + 1,
            DAY,
            WALLET,
            NOW
        )
        .err(),
        Some(BudgetError::OverCeiling)
    );
    assert_eq!(
        g.grant(
            "https://b.example.org",
            DEFAULT_PRINCIPAL,
            0,
            DAY,
            WALLET,
            NOW
        )
        .err(),
        Some(BudgetError::OverCeiling)
    );
    assert_eq!(
        g.grant(
            "https://b.example.org",
            DEFAULT_PRINCIPAL,
            5,
            MAX_TTL_MS_CEILING + 1,
            WALLET,
            NOW
        )
        .err(),
        Some(BudgetError::OverCeiling)
    );
    assert!(matches!(
        g.grant(
            "http://b.example.org",
            DEFAULT_PRINCIPAL,
            5,
            DAY,
            WALLET,
            NOW
        )
        .err(),
        Some(BudgetError::BadOrigin(_))
    ));
}

#[test]
fn budget_monotone_across_restart() {
    let p = tmp_path("restart");
    let k = MemKeyring::default();
    {
        let g = BudgetGate::open(p.clone(), Box::new(k.clone()), NOW);
        grant(&g, 3);
        let m = msg_at("abcdef0123456789AA", NOW);
        auto(&g, &m, Some(&top()), &TaskTaint::Clean, NOW).expect("reserve");
    }
    // A restart never resets a cap or the nonce ledger.
    let g = BudgetGate::open(p.clone(), Box::new(k.clone()), NOW + MIN);
    let snap = g.snapshot(NOW + MIN, Some(WALLET));
    assert_eq!(snap.budgets[0].used_count, 1);
    let m = msg_at("abcdef0123456789AA", NOW + MIN);
    assert_eq!(
        auto(&g, &m, Some(&top()), &TaskTaint::Clean, NOW + MIN),
        Err(FallThrough::NonceReused)
    );
}

#[test]
fn crash_recovery_marks_open_reservations_outcome_unknown() {
    let p = tmp_path("crash");
    let k = MemKeyring::default();
    let rid;
    {
        let g = BudgetGate::open(p.clone(), Box::new(k.clone()), NOW);
        grant(&g, 3);
        let m = msg_at("abcdef0123456789AA", NOW);
        // Reserved, then the process "crashes" before the signer result is recorded.
        rid = auto(&g, &m, Some(&top()), &TaskTaint::Clean, NOW).expect("reserve");
    }
    let g = BudgetGate::open(p, Box::new(k), NOW + MIN);
    let snap = g.snapshot(NOW + MIN, Some(WALLET));
    let r = snap
        .records
        .iter()
        .find(|r| r.record_id == rid)
        .expect("record");
    // NoFalseNegative: never "not signed" when the outcome is unknown.
    assert_eq!(r.status, RecordStatus::OutcomeUnknown);
}

#[test]
fn write_ahead_failure_signs_nothing_and_reserves_nothing() {
    let (g, p, _) = open("wal");
    grant(&g, 3);
    // Make the store's directory unwritable by replacing the file's parent with a file path.
    let dir = p.parent().unwrap().to_path_buf();
    std::fs::remove_dir_all(&dir).unwrap();
    std::fs::write(&dir, b"not a directory").unwrap();
    let m = msg_at("abcdef0123456789AA", NOW);
    assert_eq!(
        auto(&g, &m, Some(&top()), &TaskTaint::Clean, NOW),
        Err(FallThrough::WriteAheadFailed)
    );
    // In-memory state is unchanged: nothing reserved.
    assert_eq!(g.snapshot(NOW, Some(WALLET)).budgets[0].used_count, 0);
    let _ = std::fs::remove_file(&dir);
}

#[test]
fn tampered_store_fails_closed_until_reset() {
    let p = tmp_path("tamper");
    let k = MemKeyring::default();
    {
        let g = BudgetGate::open(p.clone(), Box::new(k.clone()), NOW);
        grant(&g, 3);
    }
    let raw = std::fs::read_to_string(&p).unwrap();
    std::fs::write(
        &p,
        raw.replace("\\\"max_count\\\":3", "\\\"max_count\\\":50"),
    )
    .unwrap();
    let g = BudgetGate::open(p.clone(), Box::new(k.clone()), NOW);
    assert!(matches!(g.health(), StoreHealth::Failed(_)));
    let m = msg_at("abcdef0123456789AA", NOW);
    assert_eq!(
        auto(&g, &m, Some(&top()), &TaskTaint::Clean, NOW),
        Err(FallThrough::StoreUnavailable)
    );
    assert_eq!(
        g.grant(ORIGIN, DEFAULT_PRINCIPAL, 3, DAY, WALLET, NOW)
            .err(),
        Some(BudgetError::StoreUnavailable)
    );
    // The member resets the store: the old file is kept aside, budgets start empty.
    g.reset_after_integrity_failure(NOW).expect("reset");
    assert_eq!(g.health(), StoreHealth::Ok);
    assert!(g.snapshot(NOW, Some(WALLET)).budgets.is_empty());
    let kept_aside = std::fs::read_dir(p.parent().unwrap())
        .unwrap()
        .filter_map(|e| e.ok())
        .any(|e| e.file_name().to_string_lossy().contains(".corrupt-"));
    assert!(kept_aside);
}

#[test]
fn reset_refuses_a_healthy_store_and_keeps_its_caps() {
    // Reset exists only to recover from an integrity failure. On a healthy store it must not
    // wipe counters, the nonce ledger or the records, whoever calls it.
    let (g, p, _k) = open("reset-healthy");
    let b = grant(&g, 3);
    assert_eq!(
        g.reset_after_integrity_failure(NOW),
        Err(BudgetError::StoreHealthy)
    );
    let snap = g.snapshot(NOW, Some(WALLET));
    assert_eq!(snap.budgets.len(), 1);
    assert_eq!(snap.budgets[0].id, b.id);
    assert!(p.exists());
    let aside = std::fs::read_dir(p.parent().unwrap())
        .unwrap()
        .filter_map(|e| e.ok())
        .any(|e| e.file_name().to_string_lossy().contains(".corrupt-"));
    assert!(!aside, "a healthy file is never moved aside");
}

#[test]
fn keychain_unavailable_fails_closed() {
    let p = tmp_path("nokeychain");
    let k = MemKeyring {
        broken: true,
        ..Default::default()
    };
    let g = BudgetGate::open(p, Box::new(k), NOW);
    assert!(matches!(g.health(), StoreHealth::Failed(_)));
    assert_eq!(
        g.grant(ORIGIN, DEFAULT_PRINCIPAL, 3, DAY, WALLET, NOW)
            .err(),
        Some(BudgetError::StoreUnavailable)
    );
}

#[test]
fn records_are_hash_chained() {
    let (g, _, _) = open("chain");
    let b = grant(&g, 3);
    let m = msg_at("abcdef0123456789AA", NOW);
    auto(&g, &m, Some(&top()), &TaskTaint::Clean, NOW).expect("reserve");
    g.revoke(b.id, NOW + 1).expect("revoke");
    let snap = g.snapshot(NOW, Some(WALLET));
    let mut recs = snap.records.clone();
    recs.sort_by_key(|r| r.record_id);
    assert_eq!(recs.len(), 3);
    assert_eq!(recs[0].prev_hash, GENESIS_HASH);
    for w in recs.windows(2) {
        assert_eq!(w[1].prev_hash, w[0].hash);
    }
    assert_eq!(snap.head_hash, recs[2].hash);
    assert!(verify_chain(&recs));
    let mut broken = recs.clone();
    broken[1].nonce = Some("tampered000000000".into());
    assert!(!verify_chain(&broken));
}

#[test]
fn x402_is_typed_separately_and_inert_until_an_asset_is_allowlisted() {
    // O-1: no wrapped-SALT asset exists yet, so the allowlist is empty and B-2 never auto-signs.
    assert!(X402_ASSET_ALLOWLIST.is_empty());
    let req = X402Request {
        recipient: "0x00000000000000000000000000000000000000aa".into(),
        asset: "0x00000000000000000000000000000000000000bb".into(),
        amount: "1".into(),
    };
    assert_eq!(
        x402_budgetable(&req, &TaskTaint::Clean),
        Err(FallThrough::X402Inert)
    );
}

#[test]
fn every_fall_through_reason_is_plain_language() {
    for f in [
        FallThrough::StoreUnavailable,
        FallThrough::HicRequired,
        FallThrough::NotAttested,
        FallThrough::NotTopFrame,
        FallThrough::AttachMode,
        FallThrough::NoBudget,
        FallThrough::BudgetRevoked,
        FallThrough::BudgetExpired,
        FallThrough::BudgetExhausted,
        FallThrough::WalletChanged,
        FallThrough::NoWallet,
        FallThrough::Tainted,
        FallThrough::TaintUnknown,
        FallThrough::Message(crate::siwe::SiweReject::Malformed),
        FallThrough::NonceReused,
        FallThrough::RateGap,
        FallThrough::RateWindow,
        FallThrough::WriteAheadFailed,
        FallThrough::ExpiredBeforeSigning,
        FallThrough::SignerUnavailable,
        FallThrough::X402Inert,
    ] {
        let s = f.to_string();
        assert!(
            !s.is_empty() && !s.contains('\u{2014}') && !s.to_lowercase().contains("hitl"),
            "{s}"
        );
    }
}

// ---------------------------------------------------------------------------------------------
// HUP-S2.3 follow-ups: rollback protection, the anchor export cursor, address sharing.
// ---------------------------------------------------------------------------------------------

#[test]
fn a_rolled_back_budget_file_fails_closed() {
    // BudgetMonotone across an attacker with file access: an older, validly MACed copy of the
    // file must not bring back spent counts, nonces or rate slots. The keychain holds the
    // generation of the newest file this device saved.
    let p = tmp_path("rollback");
    let k = MemKeyring::default();
    let old;
    {
        let g = BudgetGate::open(p.clone(), Box::new(k.clone()), NOW);
        grant(&g, 3);
        old = std::fs::read(&p).unwrap();
        let m = msg_at("abcdef0123456789AA", NOW);
        auto(&g, &m, Some(&top()), &TaskTaint::Clean, NOW).expect("reserve");
    }
    std::fs::write(&p, &old).unwrap();
    let g = BudgetGate::open(p.clone(), Box::new(k.clone()), NOW + MIN);
    assert!(
        matches!(g.health(), StoreHealth::Failed(ref r) if r.contains("older")),
        "{:?}",
        g.health()
    );
    let m = msg_at("abcdef0123456789AA", NOW + MIN);
    assert_eq!(
        auto(&g, &m, Some(&top()), &TaskTaint::Clean, NOW + MIN),
        Err(FallThrough::StoreUnavailable),
        "the replayed nonce is not signed again"
    );
    g.reset_after_integrity_failure(NOW + MIN).expect("reset");
    assert_eq!(g.health(), StoreHealth::Ok);
    drop(g);
    let g = BudgetGate::open(p, Box::new(k), NOW + 2 * MIN);
    assert_eq!(g.health(), StoreHealth::Ok, "a reset store reopens cleanly");
}

#[test]
fn a_deleted_budget_file_after_a_save_fails_closed() {
    let p = tmp_path("deleted");
    let k = MemKeyring::default();
    {
        let g = BudgetGate::open(p.clone(), Box::new(k.clone()), NOW);
        grant(&g, 3);
    }
    std::fs::remove_file(&p).unwrap();
    let g = BudgetGate::open(p, Box::new(k), NOW);
    assert!(matches!(g.health(), StoreHealth::Failed(_)), "{:?}", g.health());
}

#[test]
fn a_crash_between_the_file_and_the_keychain_generation_is_accepted() {
    // The file is written first, then the keychain generation. A crash in between leaves the
    // file one generation ahead, which is the newer state and must open.
    let p = tmp_path("gen-crash");
    let k = MemKeyring::default();
    {
        let g = BudgetGate::open(p.clone(), Box::new(k.clone()), NOW);
        grant(&g, 3);
        grant_other(&g);
    }
    let gen: u64 = String::from_utf8(k.get(GENERATION_ACCOUNT).unwrap().unwrap())
        .unwrap()
        .parse()
        .unwrap();
    k.set(GENERATION_ACCOUNT, (gen - 1).to_string().as_bytes())
        .unwrap();
    let g = BudgetGate::open(p, Box::new(k.clone()), NOW);
    assert_eq!(g.health(), StoreHealth::Ok);
}

fn grant_other(g: &BudgetGate) {
    g.grant(
        "https://other.example.org",
        DEFAULT_PRINCIPAL,
        2,
        DAY,
        WALLET,
        NOW,
    )
    .expect("grant");
}

#[test]
fn a_store_from_before_generations_opens_and_starts_counting() {
    // Files written before rollback protection have no generation and the keychain has none:
    // they open, and the next save seals generation 1 or more.
    let p = tmp_path("legacy");
    let k = MemKeyring::default();
    {
        let g = BudgetGate::open(p.clone(), Box::new(k.clone()), NOW);
        grant(&g, 3);
    }
    k.delete(GENERATION_ACCOUNT).unwrap();
    let g = BudgetGate::open(p.clone(), Box::new(k.clone()), NOW);
    assert_eq!(g.health(), StoreHealth::Ok);
    grant_other(&g);
    assert!(k.get(GENERATION_ACCOUNT).unwrap().is_some());
}

#[test]
fn records_export_in_order_and_stop_at_an_open_reservation() {
    // US-2.3 AC3: core sends its records to the decision log the nightly anchor covers. Only
    // closed records go, oldest first, and never past a reservation whose outcome is not known
    // yet (it would otherwise be exported before it is final).
    let (g, p, k) = open("export");
    let b = grant(&g, 3);
    let first = g.records_to_export(10);
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].kind, RecordKind::BudgetGranted);
    let rid = {
        let mut guard = g.lock();
        let m = msg_at("abcdef0123456789AA", NOW);
        let plan = guard
            .evaluate_siwe(&input(&m, Some(&top()), &TaskTaint::Clean), NOW)
            .expect("plan");
        guard.reserve(&plan, NOW).expect("reserve")
    };
    g.revoke(b.id, NOW + 1).expect("revoke");
    let pending = g.records_to_export(10);
    assert_eq!(
        pending.iter().map(|r| r.record_id).collect::<Vec<_>>(),
        vec![1],
        "the open reservation holds back itself and everything after it"
    );
    g.lock().close(rid, RecordStatus::Signed);
    let all = g.records_to_export(10);
    assert_eq!(all.iter().map(|r| r.record_id).collect::<Vec<_>>(), vec![1, 2, 3]);
    assert_eq!(g.records_to_export(2).len(), 2, "bounded");
    g.mark_exported(2).expect("mark");
    assert_eq!(
        g.records_to_export(10)
            .iter()
            .map(|r| r.record_id)
            .collect::<Vec<_>>(),
        vec![3]
    );
    assert!(g.mark_exported(1).is_ok(), "never moves backwards, never fails");
    assert_eq!(g.records_to_export(10).len(), 1);
    // The cursor is part of the MACed file: it survives a restart.
    drop(g);
    let g = BudgetGate::open(p, Box::new(k), NOW);
    assert_eq!(g.records_to_export(10).len(), 1);
    assert!(g.mark_exported(99).is_err(), "past the last record");
}

#[test]
fn a_live_budget_is_found_only_for_its_origin_principal_and_wallet() {
    // Sharing the member's address with a page (eth_requestAccounts) is allowed only for a site
    // with a live budget for the active wallet.
    let (g, _, _) = open("live");
    assert_eq!(g.live_budget_for(ORIGIN, DEFAULT_PRINCIPAL, Some(WALLET), NOW), None);
    let b = grant(&g, 1);
    assert_eq!(
        g.live_budget_for("https://APP.example.org/", DEFAULT_PRINCIPAL, Some(WALLET), NOW),
        Some(b.id)
    );
    assert_eq!(g.live_budget_for(ORIGIN, "other", Some(WALLET), NOW), None);
    assert_eq!(
        g.live_budget_for(ORIGIN, DEFAULT_PRINCIPAL, Some("0x0000000000000000000000000000000000000001"), NOW),
        None
    );
    assert_eq!(g.live_budget_for(ORIGIN, DEFAULT_PRINCIPAL, None, NOW), None);
    assert_eq!(g.live_budget_for(ORIGIN, DEFAULT_PRINCIPAL, Some(WALLET), NOW + 8 * DAY), None);
    assert_eq!(g.live_budget_for("http://app.example.org", DEFAULT_PRINCIPAL, Some(WALLET), NOW), None);
    g.revoke(b.id, NOW).expect("revoke");
    assert_eq!(g.live_budget_for(ORIGIN, DEFAULT_PRINCIPAL, Some(WALLET), NOW), None);
}

// ---------------------------------------------------------------------------------------------
// HUP-S1.5: the pinned x402 authorization template (D3 "core builds the bytes it signs").
// ---------------------------------------------------------------------------------------------

const TEST_ASSET: X402Asset = X402Asset {
    chain_id: 40204,
    verifying_contract: "0x1111111111111111111111111111111111111111",
    name: "Wrapped SALT",
    version: "1",
};
const TEST_PAYEE: &str = "0x3333333333333333333333333333333333333333";
const TEST_FROM: &str = "0x2222222222222222222222222222222222222222";

fn x402_req(amount: &str) -> X402Request {
    X402Request {
        recipient: TEST_PAYEE.to_string(),
        asset: TEST_ASSET.verifying_contract.to_uppercase().replacen("0X", "0x", 1),
        amount: amount.to_string(),
    }
}

#[test]
fn x402_template_binds_asset_payee_wallet_window_and_core_nonce() {
    let nonce = [0xab; 32];
    let a = build_x402_authorization(
        &TEST_ASSET,
        TEST_PAYEE,
        &x402_req("1500000000000000000"),
        TEST_FROM,
        1_000_000,
        300,
        nonce,
    )
    .unwrap_or_else(|e| panic!("build: {e:?}"));
    assert_eq!(a.from, [0x22; 20]);
    assert_eq!(a.to, [0x33; 20]);
    assert_eq!(a.nonce, nonce);
    assert_eq!(a.valid_after, 1_000_000 - X402_VALID_AFTER_SKEW_S);
    assert_eq!(a.valid_before, 1_000_300);
    assert_eq!(crate::eip712::u256_to_dec(&a.value), "1500000000000000000");
    let d = TEST_ASSET.domain().unwrap_or_else(|e| panic!("domain: {e:?}"));
    assert_eq!(d.chain_id, 40204);
    assert_eq!(a.view(&d, 18).amount, "1.5");
}

#[test]
fn x402_template_refuses_every_binding_mismatch() {
    let b = |asset: &X402Asset, payee: &str, req: &X402Request, from: &str, validity: u64| {
        build_x402_authorization(asset, payee, req, from, 1_000_000, validity, [1; 32]).err()
    };
    let ok = x402_req("1");
    let other_asset = X402Request {
        asset: "0x4444444444444444444444444444444444444444".into(),
        ..ok.clone()
    };
    assert_eq!(
        b(&TEST_ASSET, TEST_PAYEE, &other_asset, TEST_FROM, 60),
        Some(X402BuildError::AssetMismatch)
    );
    assert_eq!(
        b(&TEST_ASSET, "0x5555555555555555555555555555555555555555", &ok, TEST_FROM, 60),
        Some(X402BuildError::RecipientMismatch)
    );
    assert_eq!(b(&TEST_ASSET, TEST_PAYEE, &ok, TEST_FROM, 0), Some(X402BuildError::Validity));
    assert_eq!(
        b(&TEST_ASSET, TEST_PAYEE, &ok, TEST_FROM, X402_VALIDITY_MAX_S + 1),
        Some(X402BuildError::Validity)
    );
    assert_eq!(b(&TEST_ASSET, TEST_PAYEE, &ok, TEST_FROM, X402_VALIDITY_MAX_S), None);
    assert_eq!(
        b(&TEST_ASSET, TEST_PAYEE, &x402_req("0"), TEST_FROM, 60),
        Some(X402BuildError::ZeroAmount)
    );
    assert!(matches!(
        b(&TEST_ASSET, TEST_PAYEE, &x402_req("1.5"), TEST_FROM, 60),
        Some(X402BuildError::Malformed(_))
    ));
    assert!(matches!(
        b(&TEST_ASSET, TEST_PAYEE, &ok, "0x22", 60),
        Some(X402BuildError::Malformed(_))
    ));
}

#[test]
fn x402_nonces_come_from_the_os_csprng_and_do_not_repeat() {
    let a = fresh_x402_nonce();
    let b = fresh_x402_nonce();
    assert_ne!(a, b);
    assert_ne!(a, [0u8; 32]);
}

#[test]
fn the_x402_allowlist_stays_empty_so_b2_stays_inert() {
    // O-1 is open: no allowlisted asset, so even a well-formed request falls through to HIC-1.
    assert!(X402_ASSET_ALLOWLIST.is_empty());
    assert_eq!(
        x402_budgetable(
            &X402Request {
                recipient: TEST_PAYEE.into(),
                asset: TEST_ASSET.verifying_contract.into(),
                amount: "1".into()
            },
            &TaskTaint::Clean
        ),
        Err(FallThrough::X402Inert)
    );
}
