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

/// The symbol and minor-unit scale of each currency the app can state (ISO 4217). Any other
/// currency is refused, and the app shows the pricing link instead.
fn currency_format(currency: &str) -> Option<(&'static str, u32)> {
    match currency {
        "usd" => Some(("$", 2)),
        "eur" => Some(("€", 2)),
        "gbp" => Some(("£", 2)),
        "jpy" => Some(("¥", 0)),
        _ => None,
    }
}

/// The amount in minor units that `display` states for `currency`, or None unless the WHOLE
/// string is that currency's symbol followed by the amount: whole units with no leading zero,
/// optionally grouped in threes by commas, then (for a 2-decimal currency) optionally "." and
/// exactly two digits. A 0-decimal currency takes no decimal part.
fn display_minor_units(currency: &str, display: &str) -> Option<u64> {
    let (symbol, scale) = currency_format(currency)?;
    let rest = display.strip_prefix(symbol)?;
    let (whole, frac) = match rest.split_once('.') {
        Some((w, f)) => (w, Some(f)),
        None => (rest, None),
    };
    // Whole units: plain digits, or digit groups of three after a 1–3 digit lead.
    let groups: Vec<&str> = whole.split(',').collect();
    let well_grouped = groups.len() == 1
        || (!groups[0].is_empty()
            && groups[0].len() <= 3
            && groups[1..].iter().all(|g| g.len() == 3));
    if whole.is_empty()
        || !well_grouped
        || !groups
            .iter()
            .all(|g| !g.is_empty() && g.bytes().all(|b| b.is_ascii_digit()))
    {
        return None;
    }
    let digits: String = groups.concat();
    if digits.len() > 1 && digits.starts_with('0') {
        return None;
    }
    let whole: u64 = digits.parse().ok()?;
    let minor: u64 = match (scale, frac) {
        (0, None) => 0,
        (2, None) => 0,
        (2, Some(f)) if f.len() == 2 && f.bytes().all(|b| b.is_ascii_digit()) => f.parse().ok()?,
        _ => return None,
    };
    whole.checked_mul(10u64.pow(scale))?.checked_add(minor)
}

/// Decide the price from the route's HTTP status and body.
pub fn parse_price(status: u16, body: &[u8]) -> Result<MembershipPrice, String> {
    if status != 200 {
        return Err(format!("membership price unavailable (HTTP {status})"));
    }
    let v: serde_json::Value = serde_json::from_slice(body)
        .map_err(|_| "membership price: unreadable response".to_string())?;
    let currency = v
        .get("currency")
        .and_then(|c| c.as_str())
        .filter(|c| c.len() == 3 && c.bytes().all(|b| b.is_ascii_lowercase()))
        .ok_or("membership price: missing or invalid currency")?
        .to_string();
    let unit_amount = v
        .get("unitAmount")
        .and_then(|u| u.as_u64())
        .filter(|u| *u > 0)
        .ok_or("membership price: missing or invalid unitAmount")?;
    let interval = v
        .get("interval")
        .and_then(|i| i.as_str())
        .filter(|i| matches!(*i, "year" | "month"))
        .ok_or("membership price: missing or invalid interval")?
        .to_string();
    let display_amount = v
        .get("displayAmount")
        .and_then(|d| d.as_str())
        .filter(|d| {
            !d.is_empty()
                && d.len() <= 24
                && d.chars()
                    .all(|c| c.is_alphanumeric() || " .,$€£¥".contains(c))
        })
        .ok_or("membership price: missing or invalid displayAmount")?
        .to_string();
    if display_minor_units(&currency, &display_amount) != Some(unit_amount) {
        return Err(
            "membership price: displayAmount does not state unitAmount in the declared currency"
                .to_string(),
        );
    }
    Ok(MembershipPrice {
        currency,
        unit_amount,
        interval,
        display_amount,
    })
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
