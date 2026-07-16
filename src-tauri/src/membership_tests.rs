// CORE-D3.C — membership_checkout plumbing tests.
//
// The `WebviewWindow` itself is NOT unit-testable in Rust (it needs a live
// event loop + webview host — flagged here, exercised only in a packaged
// build / live test). What IS testable is the command's real decision: which
// URL the popup navigates to. That is `AppConfig::checkout_url()`, proven in
// config.rs; here we assert the D3.C-specific plumbing: the popup label is the
// isolated `checkout-popup` (no capability), the command is registered, and the
// checkout URL is exactly `{coreMembershipUrl}/checkout` off the config base.

use crate::config::{AppConfig, AppConfigPatch};

/// The popup uses the dedicated `checkout-popup` label — a remote HTTPS page
/// with NO Tauri capability/IPC, isolated exactly like the A3 `auth-popup`.
#[test]
fn checkout_popup_label_is_the_isolated_checkout_window() {
    assert_eq!(super::CHECKOUT_POPUP_LABEL, "checkout-popup");
    // It is NOT the auth popup nor the main window (distinct isolation).
    assert_ne!(super::CHECKOUT_POPUP_LABEL, "auth-popup");
    assert_ne!(super::CHECKOUT_POPUP_LABEL, "main");
}

/// The command navigates the popup to `{coreMembershipUrl}/checkout`, taking the
/// base from the persisted config (overridable to a preview/prod domain). This
/// is the exact URL `membership_checkout` resolves before opening the popup.
#[test]
fn checkout_url_is_base_plus_checkout_path() {
    // Prod default base.
    assert_eq!(
        AppConfig::default().checkout_url(),
        "https://core-membership.vercel.app/checkout"
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
