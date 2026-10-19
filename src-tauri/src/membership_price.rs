//! citrate-core — the membership price (C1-P; UI truth contract §5.3).
//!
//! Data source (Rule 7): core-membership's public price route,
//! `{coreMembershipUrl}/api/membership/price` (live at https://membership.citrate.ai), which returns
//! `{currency, unitAmount, interval, displayAmount, …}` from Stripe, or a 503 with no number when it
//! can't read the price. It sends no CORS headers, so the read happens here, not in the webview.
//!
//! Rule 1: a price is returned only when the route answered 200 with a well-formed body whose
//! `displayAmount` states the same amount as `unitAmount`. Anything else (a 503, any other status,
//! a malformed body, a mismatch) is an error, and the app keeps its "See membership pricing" link.
//! This module never supplies a number of its own.

use serde::Serialize;

/// The membership price as core-membership served it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MembershipPrice {
    /// ISO 4217 code, lower case (e.g. "usd").
    pub currency: String,
    /// Minor units (cents).
    pub unit_amount: u64,
    /// "year" or "month".
    pub interval: String,
    /// The route's own display string (e.g. "$48"), checked against `unit_amount`.
    pub display_amount: String,
}

/// `{base}/api/membership/price`.
pub fn price_url(base: &str) -> String {
    format!("{}/api/membership/price", base.trim_end_matches('/'))
}

/// Decide the price from the route's HTTP status and body.
pub fn parse_price(status: u16, body: &[u8]) -> Result<MembershipPrice, String> {
    let _ = (status, body);
    Err("membership price: not read yet".to_string())
}

/// GET the price route: status + body, or a transport error.
fn fetch(url: &str) -> Result<(u16, Vec<u8>), String> {
    let resp = ureq::get(url)
        .config()
        .timeout_global(Some(std::time::Duration::from_secs(10)))
        .max_redirects(0)
        .http_status_as_error(false)
        .build()
        .call()
        .map_err(|_| "membership price: could not reach core-membership".to_string())?;
    let status = resp.status().as_u16();
    let mut resp = resp;
    let body = resp
        .body_mut()
        .with_config()
        .limit(16 * 1024)
        .read_to_vec()
        .map_err(|_| "membership price: unreadable response".to_string())?;
    Ok((status, body))
}

/// `membership_price` — read the membership price from core-membership (see the module docs).
/// A pure public read: no auth, no signing, no secrets.
#[tauri::command]
pub async fn membership_price<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
) -> std::result::Result<MembershipPrice, String> {
    let cfg = crate::config::config_read_sync(app)?;
    let url = price_url(&cfg.core_membership_url);
    tauri::async_runtime::spawn_blocking(move || {
        let (status, body) = fetch(&url)?;
        parse_price(status, &body)
    })
    .await
    .map_err(|e| format!("membership price: task failed: {e}"))?
}

#[cfg(test)]
mod tests {
    include!("membership_price_tests.rs");
}
