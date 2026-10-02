// HUP-S2.3 — the Budgets command surface (Settings → Budgets) and the budgeted SIWE entry point.

use super::*;
use citrate_core_kit::custody::{CustodyError, CustodyVault, Keyring};
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
const NOW: u64 = 1_790_856_000_000;

fn tmp_dir(tag: &str) -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let mut p = std::env::temp_dir();
    p.push(format!(
        "citrate-web-budgets-app-{tag}-{}-{}",
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
        dir.join(BUDGET_FILE_NAME),
        Box::new(FakeKeyring::default()),
        NOW,
    )
}

fn siwe() -> String {
    citrate_core_kit::siwe::SiweFields {
        scheme: None,
        domain: "app.example.org".into(),
        address: ADDR.into(),
        statement: None,
        uri: "https://app.example.org/".into(),
        version: "1".into(),
        chain_id: "40204".into(),
        nonce: "abcdef0123456789AA".into(),
        issued_at: "2026-10-01T11:59:30Z".into(),
        expiration_time: Some("2026-10-01T12:10:00Z".into()),
        not_before: None,
        request_id: None,
        resources: vec![],
    }
    .to_message()
}

#[test]
fn taint_sources_none_is_unknown_and_empty_is_clean() {
    assert_eq!(taint_from(None), TaskTaint::Unknown);
    assert_eq!(taint_from(Some(vec![])), TaskTaint::Clean);
    assert_eq!(
        taint_from(Some(vec!["ext".into()])),
        TaskTaint::Sources(vec!["ext".into()])
    );
}

#[test]
fn origin_attestation_is_honestly_unavailable_until_the_managed_browser_lands() {
    // No managed browser (HUP-S5.1) yet, so core cannot attest any page origin, whatever the
    // caller claims. Every sign-in therefore asks the member (HIC-1).
    assert_eq!(attest_origin(Some("tab-1")), None);
    assert_eq!(attest_origin(None), None);
    assert!(ATTESTATION_UNAVAILABLE_REASON.contains("managed browser"));
}

#[test]
fn ttl_days_are_bounded_by_the_ceiling() {
    assert_eq!(ttl_days_to_ms(1), Ok(86_400_000));
    assert!(ttl_days_to_ms(0).is_err());
    assert!(ttl_days_to_ms(31).is_err());
}

#[test]
fn status_reports_defaults_as_pending_owner_signoff_and_no_budgets() {
    let d = tmp_dir("status");
    let g = gate(&d);
    let st = status_of(&g, NOW, Some(&ADDR.to_lowercase()));
    assert!(st.snapshot.budgets.is_empty());
    assert!(st.defaults.pending_owner_signoff);
    assert_eq!(
        st.defaults.max_count,
        citrate_core_kit::web_budget::PLACEHOLDER_DEFAULT_MAX_COUNT
    );
    assert_eq!(st.ceilings.max_count, 50);
    assert_eq!(st.ceilings.ttl_days, 30);
    assert!(!st.attestation.available);
    assert_eq!(st.rate.min_gap_seconds, 30);
    assert_eq!(st.rate.window_max, 20);
    let v = serde_json::to_value(&st).expect("json");
    assert_eq!(
        v["defaults"]["pendingOwnerSignoff"],
        serde_json::Value::Bool(true)
    );
    assert_eq!(v["snapshot"]["health"]["state"], "ok");
}

#[test]
fn a_sign_in_request_falls_through_to_a_card_today_even_with_a_budget() {
    let d = tmp_dir("request");
    let v = vault(&d);
    let g = gate(&d);
    g.grant(
        "https://app.example.org",
        DEFAULT_PRINCIPAL,
        5,
        86_400_000,
        &ADDR.to_lowercase(),
        NOW,
    )
    .expect("grant");
    let c = citrate_core_kit::ceremony::SignatureCeremony::new();
    let out = request_inner(
        &c,
        &v,
        &g,
        SigningRequestArgs {
            message: siwe(),
            tab_id: Some("tab-1".into()),
            claimed_origin: "https://app.example.org".into(),
            taint_sources: Some(vec![]),
            hic_required: false,
        },
        &|| NOW,
    );
    match out {
        citrate_core_kit::ceremony::BudgetedOutcome::Pending { ceremony, reason } => {
            assert!(reason.contains("could not confirm which site"), "{reason}");
            assert!(ceremony.origin.contains("not verified"));
            assert!(c.status(&ceremony.id).is_some());
        }
        other => panic!("no auto-sign is possible without origin attestation: {other:?}"),
    }
    assert_eq!(g.snapshot(NOW, None).budgets[0].used_count, 0);
}

#[test]
fn a_hostile_claimed_origin_is_only_ever_display_text() {
    let d = tmp_dir("claimed");
    let v = vault(&d);
    let g = gate(&d);
    let c = citrate_core_kit::ceremony::SignatureCeremony::new();
    let out = request_inner(
        &c,
        &v,
        &g,
        SigningRequestArgs {
            message: siwe(),
            tab_id: None,
            claimed_origin: "x".repeat(5000),
            taint_sources: None,
            hic_required: false,
        },
        &|| NOW,
    );
    if let citrate_core_kit::ceremony::BudgetedOutcome::Pending { ceremony, .. } = out {
        assert!(ceremony.origin.chars().count() <= MAX_CLAIMED_ORIGIN_CHARS + 60);
    } else {
        panic!("must be a card");
    }
}

#[test]
fn grant_requires_a_wallet() {
    let d = tmp_dir("grant");
    let g = gate(&d);
    assert!(grant_inner(&g, None, "https://app.example.org", 5, 7, NOW).is_err());
    let b = grant_inner(
        &g,
        Some(&ADDR.to_lowercase()),
        "https://app.example.org",
        5,
        7,
        NOW,
    )
    .expect("grant");
    assert_eq!(b.max_count, 5);
    assert_eq!(b.expires_at_ms, NOW + 7 * 86_400_000);
}

#[test]
fn budget_commands_are_registered_and_in_the_main_window_acl_only() {
    let lib = include_str!("lib.rs");
    let acl = include_str!("../permissions/main-window.toml");
    let popout = include_str!("../capabilities/popout.json");
    for cmd in [
        "web_budget_status",
        "web_budget_grant",
        "web_budget_revoke",
        "web_budget_revoke_all",
        "web_budget_reset",
        "web_signing_request",
    ] {
        assert!(
            lib.contains(&format!("web_budgets::{cmd}")),
            "{cmd} registered"
        );
        assert!(
            acl.contains(&format!("\"{cmd}\"")),
            "{cmd} in the main-window ACL"
        );
        assert!(
            !popout.contains(cmd),
            "{cmd} must not reach a pop-out window"
        );
    }
}

fn attested(origin: &str) -> OriginAttestation {
    OriginAttestation {
        origin: origin.into(),
        frame: citrate_core_kit::web_budget::FrameKind::Top,
        mode: citrate_core_kit::web_budget::BrowserMode::Managed,
    }
}

#[test]
fn the_window_command_never_trusts_a_caller_claim_of_a_clean_task() {
    // Even once core can attest the origin, the main-window command must not let its caller
    // declare the task clean or HIC not required: any page script could otherwise collect
    // budgeted sign-ins. Taint for an auto-sign has to come from core, not the request body.
    let d = tmp_dir("caller-taint");
    let v = vault(&d);
    let g = gate(&d);
    g.grant(
        "https://app.example.org",
        DEFAULT_PRINCIPAL,
        5,
        86_400_000,
        &ADDR.to_lowercase(),
        NOW,
    )
    .expect("grant");
    let c = citrate_core_kit::ceremony::SignatureCeremony::new();
    let out = request_attested(
        &c,
        &v,
        &g,
        SigningRequestArgs {
            message: siwe(),
            tab_id: Some("tab-1".into()),
            claimed_origin: "https://app.example.org".into(),
            taint_sources: Some(vec![]),
            hic_required: false,
        },
        Some(attested("https://app.example.org")),
        &|| NOW,
    );
    match out {
        citrate_core_kit::ceremony::BudgetedOutcome::Pending { ceremony, .. } => {
            assert!(c.status(&ceremony.id).is_some());
        }
        other => panic!("a caller-asserted clean task must not auto-sign: {other:?}"),
    }
    assert_eq!(g.snapshot(NOW, None).budgets[0].used_count, 0);
}

#[test]
fn caller_taint_sources_can_only_add_taint() {
    assert_eq!(window_taint(None), TaskTaint::Unknown);
    assert_eq!(window_taint(Some(vec![])), TaskTaint::Unknown);
    assert_eq!(
        window_taint(Some(vec!["https://x.example".into()])),
        TaskTaint::Sources(vec!["https://x.example".into()])
    );
}
