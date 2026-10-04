// HUP-S2.3 — the Budgets command surface (Settings → Budgets) and the budgeted SIWE entry point.

use super::*;
use citrate_core_kit::custody::{CustodyError, Keyring};
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

fn gate(dir: &std::path::Path) -> BudgetGate {
    BudgetGate::open(
        dir.join(BUDGET_FILE_NAME),
        Box::new(FakeKeyring::default()),
        NOW,
    )
}

#[test]
fn the_webview_can_only_name_a_request_id() {
    // M-11 class: the webview supplies no taint, no hic flag, no message and no origin. The only
    // argument is the id the sidecar minted.
    assert!(valid_request_id("signin-3-123456").is_ok());
    for bad in ["", "signin-", "signin-x", "b12", "signin-1;drop", &"signin-1".repeat(10)] {
        assert!(valid_request_id(bad).is_err(), "{bad}");
    }
    let src = include_str!("web_budgets.rs");
    let cmd = &src[src.find("pub async fn web_signing_request(").expect("command")..];
    let sig = &cmd[..cmd.find(") ->").expect("signature end")];
    assert!(sig.contains("request_id: String"));
    for forbidden in ["taint", "hic", "message", "origin"] {
        assert!(!sig.contains(forbidden), "the command must not take `{forbidden}`");
    }
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
    assert!(st.attestation.available);
    assert!(st.attestation.reason.contains("managed browser"));
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
        "web_signing_approve",
        "web_signing_reject",
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
