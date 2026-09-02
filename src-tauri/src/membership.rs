//! citrate-core — membership checkout (CORE-D3.C). @rule8 · T1 money seam.
//!
//! The S3 onboarding "Pay" step opens the REAL core-membership checkout in the user's SYSTEM BROWSER,
//! navigated to `{coreMembershipUrl}/checkout`. The MONEY and the entitlement grant happen ENTIRELY
//! server-side (core-membership → droplet); this app only opens the URL and then WATCHES ITS OWN
//! `/userinfo` entitlement (the store drives that poll, exactly like `kyc_start`/`pollKyc`).
//!
//! ## Why the browser, not an in-app WebView (the fix)
//! Sign-in uses the system browser (RFC 8252 loopback OIDC, `kit::oidc`), so the `auth.citrate.ai` SSO
//! session lives there. An in-app popup `WebviewWindow` is an isolated cookie jar with no session, so
//! core-membership 401s the checkout ("could not connect to checkout"). Opening the real browser
//! reuses that SSO session, so `/checkout` authenticates silently. See [`open_checkout_in_browser`].
//!
//! ## What this module does NOT do (Rule 1 / @rule8)
//! - No signing, no secrets, no key material — it just hands an HTTPS URL to the OS opener.
//! - It does NOT decide success. It opens the URL and returns; the store polls `auth_userinfo` and
//!   only advances S3 when the REAL entitlement lands. A user who closes the tab without paying simply
//!   never flips the entitlement, so the poll never settles (no fabricated membership — the Rule-1 property).
//!
//! ## Testability
//! The OS opener call is not unit-testable headless (flagged). The command's real logic — resolving
//! the config base URL and deriving `{base}/checkout` — is fully tested via `AppConfig::checkout_url`
//! + `config_read` (see config.rs) and membership_tests.rs.

use tauri_plugin_opener::OpenerExt;

/// Open the membership checkout in the user's SYSTEM BROWSER (not an in-app WebView popup).
///
/// WHY the browser, not an in-app window (the CORE-D3.C fix): the app signs in via the system browser
/// (RFC 8252 loopback OIDC, `kit::oidc`), so the user's `auth.citrate.ai` SSO session cookie lives in
/// that browser. An in-app `WebviewWindow` is an ISOLATED cookie jar with no session — so
/// core-membership can't authenticate it and `/api/checkout` returns 401, which the checkout page
/// surfaces as "could not connect to checkout". Handing the URL to the real browser reuses the
/// existing SSO session, so `/checkout` authenticates silently and the Pay button works. It also
/// matches the button's own label ("Check out in your browser"). The store still drives completion by
/// polling the chain for the grant, so no in-app window is needed; a user who closes the tab is a
/// clean no-op.
fn open_checkout_in_browser<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    url: &str,
) -> std::result::Result<(), String> {
    // Validate it is a well-formed URL before handing it to the OS opener.
    let _ = url
        .parse::<tauri::Url>()
        .map_err(|_| "membership: invalid checkout url".to_string())?;
    app.opener()
        .open_url(url.to_string(), None::<&str>)
        .map_err(|e| format!("membership: could not open the checkout in your browser: {e}"))
}

/// `membership_checkout` — open the REAL core-membership checkout in the user's SYSTEM BROWSER,
/// navigated to `{coreMembershipUrl}/checkout` (from the persisted config). Returns `()` on open; the
/// store drives completion by polling `auth_userinfo` until the entitlement goes active (the money +
/// grant are server-side). Opening in the browser (not an in-app WebView) is what lets the checkout
/// authenticate — see [`open_checkout_in_browser`].
///
/// @rule8 / Rule 1: this NEVER signs, holds a secret, or reports a settled membership. It opens a URL
/// and returns. A user who closes the browser tab without paying just leaves the entitlement
/// unchanged — the poll never settles (no fabrication).
#[tauri::command]
pub async fn membership_checkout<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
) -> std::result::Result<(), String> {
    let cfg = crate::config::config_read(app.clone())?;
    let url = cfg.checkout_url();
    open_checkout_in_browser(&app, &url)
}

/// A qualified enterprise sales lead from the step-3 "Enterprise · Contact us" form. NOT the money
/// path — no grant, charge, or signature. Deserialized from the frontend; serialized to the
/// core-membership `/api/enterprise/lead` endpoint (both use the same field names).
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnterpriseLead {
    pub org: String,
    pub email: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contact: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seats: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workload: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeline: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

/// `membership_enterprise_lead` — POST a qualified enterprise lead to core-membership's
/// `/api/enterprise/lead`. Server-side `reqwest` POST (avoids webview CORS/CSP). @rule8 / Rule 1: this
/// carries NO money, signature, or secret — just the form fields (PII is validated + field-encrypted
/// SERVER-side). Returns `()` on 2xx; an honest error string otherwise (never a fabricated "received").
#[tauri::command]
pub async fn membership_enterprise_lead<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    lead: EnterpriseLead,
) -> std::result::Result<(), String> {
    let cfg = crate::config::config_read(app.clone())?;
    let url = cfg.enterprise_lead_url();
    // ureq is BLOCKING (the crate's chosen light HTTP client — lean tree); run it off the async
    // runtime so it never stalls the event loop.
    tauri::async_runtime::spawn_blocking(move || post_enterprise_lead(&url, &lead))
        .await
        .map_err(|e| format!("membership: contact task failed: {e}"))?
}

fn post_enterprise_lead(url: &str, lead: &EnterpriseLead) -> std::result::Result<(), String> {
    match ureq::post(url)
        .config()
        .timeout_global(Some(std::time::Duration::from_secs(15)))
        .build()
        .send_json(lead)
    {
        Ok(_) => Ok(()),
        Err(ureq::Error::StatusCode(400)) => {
            Err("Please check the form — an organization and a valid work email are required.".to_string())
        }
        Err(ureq::Error::StatusCode(503)) => {
            Err("The contact service isn't available yet — please try again shortly.".to_string())
        }
        Err(ureq::Error::StatusCode(code)) => Err(format!("membership: contact request failed ({code})")),
        Err(_) => Err("membership: could not reach the contact service.".to_string()),
    }
}

#[cfg(test)]
mod tests {
    include!("membership_tests.rs");
}
