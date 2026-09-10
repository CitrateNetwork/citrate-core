// CORE-D3.C — membership_checkout plumbing tests.
//
// The OS opener call itself is NOT unit-testable in Rust (it hands off to the
// system browser — flagged here, exercised only in a packaged build / live
// test). What IS testable is the command's real decision: which URL it opens.
// That is `AppConfig::checkout_url()`, proven in config.rs; here we assert the
// D3.C-specific plumbing: the checkout URL is exactly `{coreMembershipUrl}/checkout`
// off the config base (opened in the SYSTEM BROWSER, so the OIDC SSO session
// authenticates it — the CORE-D3.C fix).

use crate::config::{AppConfig, AppConfigPatch};

/// The command navigates the checkout to `{coreMembershipUrl}/checkout`, taking the
/// base from the persisted config (overridable to a preview/prod domain). This
/// is the exact URL `membership_checkout` resolves before opening the popup.
#[test]
fn checkout_url_is_base_plus_checkout_path() {
    // Prod default base — the canonical membership host (OIDC + Stripe + session share it).
    assert_eq!(
        AppConfig::default().checkout_url(),
        "https://membership.citrate.ai/checkout"
    );
    // Override to a preview domain — the popup would navigate there instead.
    let preview = AppConfig::default().apply(AppConfigPatch {
        core_membership_url: Some("https://core-membership-git-preview.vercel.app".into()),
        ..Default::default()
    });
    assert_eq!(
        preview.checkout_url(),
        "https://core-membership-git-preview.vercel.app/checkout"
    );
}

/// The checkout URL is HTTPS (a remote page) — NOT a loopback literal — so
/// there is no ATS/cleartext concern (unlike the A3 auth loopback redirect).
#[test]
fn checkout_url_is_https_not_loopback() {
    let url = AppConfig::default().checkout_url();
    assert!(url.starts_with("https://"), "checkout must be HTTPS: {url}");
    assert!(!url.contains("127.0.0.1"), "checkout is remote, not loopback");
    // Parses as a valid absolute URL (the popup builder parses it too).
    let parsed = url::Url::parse(&url).expect("checkout url must parse");
    assert_eq!(parsed.scheme(), "https");
    assert_eq!(parsed.path(), "/checkout");
}
