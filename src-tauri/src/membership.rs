//! citrate-core — membership checkout popup (CORE-D3.C). @rule8 · T1 money seam.
//!
//! The S3 onboarding "Pay" step opens the REAL core-membership checkout in an
//! in-app popup `WebviewWindow` (REUSING the WP-A / CORE-D3.0 popup mechanism),
//! navigated to `{coreMembershipUrl}/checkout`. The MONEY and the entitlement
//! grant happen ENTIRELY server-side (core-membership → droplet); this app only
//! opens the URL and then WATCHES ITS OWN `/userinfo` entitlement (the store
//! drives that poll, exactly like `kyc_start`/`pollKyc`).
//!
//! ## What this module does NOT do (Rule 1 / @rule8)
//! - No signing, no secrets, no key material — the popup just loads an HTTPS URL.
//! - It does NOT decide success. It opens the popup and returns; the store polls
//!   `auth_userinfo` and only advances S3 when the REAL entitlement lands. A user
//!   who closes the popup without paying simply never flips the entitlement, so
//!   the poll never settles (no fabricated membership — the Rule-1 property).
//!
//! ## Capability isolation (mirrors the A3 `auth-popup`)
//! The `checkout-popup` window loads a REMOTE page (core-membership over HTTPS)
//! and is granted NO Tauri capability / IPC — it cannot call any `invoke`
//! command. It is a plain external-URL webview, identical in isolation to the
//! A3 `auth-popup`. Because the URL is HTTPS (not a loopback literal) there is
//! no ATS / cleartext concern here (unlike the auth loopback redirect).
//!
//! ## Testability
//! A `WebviewWindow` cannot be built headless, so the popup open itself is not
//! unit-testable (flagged). The command's real logic — resolving the config
//! base URL and deriving `{base}/checkout` — is fully tested via
//! `AppConfig::checkout_url` + `config_read` (see config.rs) and
//! membership_tests.rs.

use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

/// The dedicated in-app checkout popup window label (CORE-D3.C). A single,
/// well-known label so a stale popup from a prior attempt is reused/closed
/// rather than leaking a second window (same discipline as `auth-popup`).
pub const CHECKOUT_POPUP_LABEL: &str = "checkout-popup";

/// Build + show the in-app membership checkout popup navigated to `url` (the
/// `{coreMembershipUrl}/checkout` page). A DEDICATED window (label
/// `checkout-popup`), NOT the main window: 520×760, centered, focused. It gets
/// NO Tauri capability/IPC — it is a remote HTTPS page, isolated exactly like
/// the A3 `auth-popup`.
///
/// Unlike the auth popup, this wires NO cancellation on close: the store drives
/// completion by polling `/userinfo`, so a user closing the window is a clean
/// no-op (the poll simply keeps running / the store stops it). The money and
/// grant are server-side; this only opens a URL.
///
/// Generic over `R: Runtime` (matching the `config_read` command style). If the
/// window cannot be created (headless / CI), returns an honest error string
/// rather than panicking — mirroring the auth popup's fallback.
fn open_checkout_popup<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    url: &str,
) -> std::result::Result<(), String> {
    // Close any stale popup left by a prior attempt (defensive; the flow's own
    // lifecycle closes it, but a crash could orphan one).
    if let Some(existing) = app.get_webview_window(CHECKOUT_POPUP_LABEL) {
        let _ = existing.close();
    }
    let external = url
        .parse::<tauri::Url>()
        .map_err(|_| "membership: invalid checkout url".to_string())?;
    WebviewWindowBuilder::new(app, CHECKOUT_POPUP_LABEL, WebviewUrl::External(external))
        .title("Citrate Membership")
        .inner_size(520.0, 760.0)
        .center()
        .focused(true)
        .build()
        .map_err(|_| "membership: could not open the checkout window".to_string())?;
    Ok(())
}

/// `membership_checkout` — open the REAL core-membership checkout in an in-app
/// popup navigated to `{coreMembershipUrl}/checkout` (from the persisted config).
/// Returns `()` on open; the store drives completion by polling `auth_userinfo`
/// until the entitlement goes active (the money + grant are server-side).
///
/// @rule8 / Rule 1: this NEVER signs, holds a secret, or reports a settled
/// membership. It opens a URL and returns. The `checkout-popup` window has NO
/// capability/IPC (a remote page). A user who closes it without paying just
/// leaves the entitlement unchanged — the poll never settles (no fabrication).
#[tauri::command]
pub fn membership_checkout<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
) -> std::result::Result<(), String> {
    // The base URL is the persisted config value (overridable to a preview/prod
    // domain); the checkout URL is `{base}/checkout`.
    let cfg = crate::config::config_read(app.clone())?;
    let url = cfg.checkout_url();
    open_checkout_popup(&app, &url)
}

#[cfg(test)]
mod tests {
    include!("membership_tests.rs");
}
