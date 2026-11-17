//! citrate-core — the membership grant signer's readiness, as core-membership reports it.
//!
//! Why (the 2026-11-12..11-17 grant outage): the treasury signer was stopped for five days, so no
//! membership grant could be signed, and S3 could only say "Waiting for checkout…" because the
//! chain, its one source, had nothing to report. This read lets S3 say the grant is delayed.
//!
//! Data source (Rule 7): core-membership's PUBLIC health route, `{coreMembershipUrl}/api/health`
//! (live at https://membership.citrate.ai). Since core-membership#86 it carries
//! `chain.signerReadiness`, a probed verdict: `ready`, or a named not-ready state. The route answers
//! 200 when the money path is ready and 503 when it is degraded; both carry the body. It sends no
//! CORS headers, so the read happens here, not in the webview.
//!
//! Rule 1: `signer_ready` is `Some(true)` only for `ready` and `Some(false)` only for a named
//! not-ready state. An absent field (a deploy older than #86), the dev-only `local-env`, or any
//! other value is `None`: no claim either way. Any other status, a body that is not
//! core-membership's health, or a transport failure is an error. This never settles a grant; the
//! chain does.

use serde::Serialize;

/// core-membership's report of its grant signer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MembershipSignerHealth {
    /// `Some(true)` ready, `Some(false)` a named not-ready state, `None` not reported.
    pub signer_ready: Option<bool>,
    /// The state as reported, when it is one this app knows.
    pub signer_state: Option<String>,
}

/// The not-ready states core-membership#86 names (`SignerReadinessState` minus `ready` and the
/// dev-only `local-env`).
const NOT_READY: &[&str] = &[
    "unconfigured",
    "unreachable",
    "wrong_contracts",
    "wrong_signer",
    "underfunded",
    "unknown",
];

/// Decide the signer report from the route's HTTP status and body.
pub fn parse_signer_health(status: u16, body: &[u8]) -> Result<MembershipSignerHealth, String> {
    if status != 200 && status != 503 {
        return Err(format!("membership health unavailable (HTTP {status})"));
    }
    let v: serde_json::Value = serde_json::from_slice(body)
        .map_err(|_| "membership health: unreadable response".to_string())?;
    if v.get("service").and_then(|s| s.as_str()) != Some("core-membership") {
        return Err("membership health: not a core-membership health response".to_string());
    }
    let state = v
        .get("chain")
        .and_then(|c| c.get("signerReadiness"))
        .and_then(|s| s.as_str());
    Ok(match state {
        Some("ready") => MembershipSignerHealth {
            signer_ready: Some(true),
            signer_state: Some("ready".to_string()),
        },
        Some(s) if NOT_READY.contains(&s) => MembershipSignerHealth {
            signer_ready: Some(false),
            signer_state: Some(s.to_string()),
        },
        _ => MembershipSignerHealth {
            signer_ready: None,
            signer_state: None,
        },
    })
}

/// GET the health route: status + body, or a transport error.
fn fetch(url: &str) -> Result<(u16, Vec<u8>), String> {
    let resp = ureq::get(url)
        .config()
        .timeout_global(Some(std::time::Duration::from_secs(10)))
        .max_redirects(0)
        .http_status_as_error(false)
        .build()
        .call()
        .map_err(|_| "membership health: could not reach core-membership".to_string())?;
    let status = resp.status().as_u16();
    let mut resp = resp;
    let body = resp
        .body_mut()
        .with_config()
        .limit(32 * 1024)
        .read_to_vec()
        .map_err(|_| "membership health: unreadable response".to_string())?;
    Ok((status, body))
}

/// `membership_signer_health` — read core-membership's grant-signer readiness (see the module
/// docs). A pure public read: no auth, no signing, no secrets.
#[tauri::command]
pub async fn membership_signer_health<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
) -> std::result::Result<MembershipSignerHealth, String> {
    let cfg = crate::config::config_read_sync(app)?;
    // The pinned membership base (kit config): never the stored field directly.
    let url = cfg.membership_health_url();
    tauri::async_runtime::spawn_blocking(move || {
        let (status, body) = fetch(&url)?;
        parse_signer_health(status, &body)
    })
    .await
    .map_err(|e| format!("membership health: task failed: {e}"))?
}

#[cfg(test)]
mod tests {
    include!("membership_health_tests.rs");
}
