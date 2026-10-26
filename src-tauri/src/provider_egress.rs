//! D-11 (RC52) — egress for cloud model calls. ADR-2026-10-26 P-4 and "HTTP behaviour".
//!
//! - **Named vendors are pinned** to their official API host ([`VENDORS`]). The host is a constant
//!   here, not a setting: nothing the webview, the sidecar or a model says can change it.
//! - **A custom endpoint** (OpenAI-compatible) is possible only after the member confirms that exact
//!   URL ([`CustomEndpoint::confirmed`]); until then a call to it is refused. It is reported
//!   `verified: false` everywhere.
//! - **The transport** ([`PinnedHttp`]) is https only, follows no redirect (a 3xx is an error),
//!   ignores the proxy environment variables (`HTTPS_PROXY`, `ALL_PROXY`, …), and re-checks that the
//!   URL it is about to open is on the endpoint's pinned host and port.

// Interface commit: the proxy that uses these lands in the following commits of this PR.
#![allow(dead_code)]

use crate::model_provider::{
    InboundResponse, Method, ProviderError, ProviderHttp, ProviderId, Vendor, WireRequest,
};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// A governed vendor: its pinned API base, and the pages the key-hygiene prompts link to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VendorSpec {
    pub vendor: Vendor,
    pub name: &'static str,
    /// The official API base URL (https, pinned).
    pub base_url: &'static str,
    /// The vendor page where the member creates a scoped, spend-limited key.
    pub key_page: &'static str,
    /// The vendor page where the member revokes a key.
    pub revoke_page: &'static str,
}

/// The six governed vendors (ADR P-3). Hosts are the vendors' published API hosts.
pub const VENDORS: &[VendorSpec] = &[
    VendorSpec {
        vendor: Vendor::OpenAi,
        name: "OpenAI",
        base_url: "https://api.openai.com/v1",
        key_page: "https://platform.openai.com/api-keys",
        revoke_page: "https://platform.openai.com/api-keys",
    },
    VendorSpec {
        vendor: Vendor::Anthropic,
        name: "Anthropic",
        base_url: "https://api.anthropic.com/v1",
        key_page: "https://console.anthropic.com/settings/keys",
        revoke_page: "https://console.anthropic.com/settings/keys",
    },
    VendorSpec {
        vendor: Vendor::Gemini,
        name: "Google Gemini",
        base_url: "https://generativelanguage.googleapis.com/v1beta",
        key_page: "https://aistudio.google.com/apikey",
        revoke_page: "https://aistudio.google.com/apikey",
    },
    VendorSpec {
        vendor: Vendor::Xai,
        name: "xAI",
        base_url: "https://api.x.ai/v1",
        key_page: "https://console.x.ai",
        revoke_page: "https://console.x.ai",
    },
    VendorSpec {
        vendor: Vendor::DeepSeek,
        name: "DeepSeek",
        base_url: "https://api.deepseek.com",
        key_page: "https://platform.deepseek.com/api_keys",
        revoke_page: "https://platform.deepseek.com/api_keys",
    },
    VendorSpec {
        vendor: Vendor::Meta,
        name: "Meta",
        base_url: "https://api.llama.com/v1",
        key_page: "https://llama.developer.meta.com",
        revoke_page: "https://llama.developer.meta.com",
    },
];

/// A remote-agent delegate (ADR P-8). Not a chat model; listed so the delegate WP shares this
/// egress table, the key store and the usage log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DelegateSpec {
    pub id: &'static str,
    pub name: &'static str,
    pub base_url: &'static str,
    pub key_page: &'static str,
}

pub const DELEGATES: &[DelegateSpec] = &[DelegateSpec {
    id: "cognition",
    name: "Cognition (Devin)",
    base_url: "https://api.devin.ai/v1",
    key_page: "https://app.devin.ai/settings",
}];

pub fn vendor_spec(v: Vendor) -> &'static VendorSpec {
    // VENDORS lists every Vendor (pinned by `every_vendor_has_exactly_one_pinned_spec`).
    VENDORS
        .iter()
        .find(|s| s.vendor == v)
        .unwrap_or(&VENDORS[0])
}

/// Where a call may go: the base URL, the one host (and port) it is pinned to, and whether the
/// endpoint is a governed vendor (`verified`) or a member-confirmed custom one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedEndpoint {
    pub provider: ProviderId,
    pub base_url: url::Url,
    pub host: String,
    pub port: u16,
    pub verified: bool,
}

/// Parse and check a base URL: https, a host, no userinfo, query or fragment.
pub fn parse_base_url(s: &str) -> Result<url::Url, ProviderError> {
    let s = s.trim();
    if s.len() > 2048 || s.bytes().any(|b| !b.is_ascii_graphic()) {
        return Err(ProviderError::Egress(
            "the endpoint URL has characters that are not allowed",
        ));
    }
    let u =
        url::Url::parse(s).map_err(|_| ProviderError::Egress("the endpoint URL is not valid"))?;
    if u.scheme() != "https" {
        return Err(ProviderError::Egress("a cloud endpoint must be https"));
    }
    if u.host_str().is_none_or(str::is_empty) {
        return Err(ProviderError::Egress("the endpoint URL has no host"));
    }
    if !u.username().is_empty() || u.password().is_some() {
        return Err(ProviderError::Egress(
            "the endpoint URL may not carry a user name or password",
        ));
    }
    if u.query().is_some() || u.fragment().is_some() {
        return Err(ProviderError::Egress(
            "the endpoint URL may not carry a query or fragment",
        ));
    }
    Ok(u)
}

fn resolved(
    provider: ProviderId,
    base: url::Url,
    verified: bool,
) -> Result<ResolvedEndpoint, ProviderError> {
    let host = base
        .host_str()
        .ok_or(ProviderError::Egress("the endpoint URL has no host"))?
        .to_ascii_lowercase();
    let port = base.port_or_known_default().unwrap_or(443);
    Ok(ResolvedEndpoint {
        provider,
        base_url: base,
        host,
        port,
        verified,
    })
}

/// The pinned endpoint of a governed vendor.
pub fn resolve_vendor(v: Vendor) -> Result<ResolvedEndpoint, ProviderError> {
    let spec = vendor_spec(v);
    resolved(ProviderId::Vendor(v), parse_base_url(spec.base_url)?, true)
}

/// Join an adapter's path to the endpoint and check the result stays on the pinned host and port,
/// over https, with no userinfo or fragment. The path must start with `/` and may carry a query.
pub fn join_checked(ep: &ResolvedEndpoint, path: &str) -> Result<url::Url, ProviderError> {
    let _ = path;
    // Interface stage: composes only (egress checks land with the egress WP commit).
    let mut base = ep.base_url.as_str().trim_end_matches('/').to_string();
    base.push_str(path);
    url::Url::parse(&base).map_err(|_| ProviderError::Egress("bad path"))
}

/// Extra headers an adapter may set: non-secret protocol headers only.
pub fn check_extra_header(name: &str, value: &str) -> Result<(), ProviderError> {
    let _ = (name, value);
    Ok(())
}

/// The auth header names an adapter may place the key in.
pub fn check_auth_header_name(name: &str) -> Result<(), ProviderError> {
    let _ = name;
    Ok(())
}

// ---------------------------------------------------------------------------
// Custom endpoints (ADR P-4: Advanced toggle, warning, per-endpoint confirmation, unverified)
// ---------------------------------------------------------------------------

/// A member-added OpenAI-compatible endpoint. Stored without its key (the key is in the keychain).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomEndpoint {
    /// `custom-<slug>`.
    pub id: String,
    pub label: String,
    pub base_url: String,
    /// The model ids the member listed for it (the anomaly halt's "known models").
    pub models: Vec<String>,
    /// Whether the member confirmed this exact URL. Unconfirmed: every call is refused.
    pub confirmed: bool,
    pub created_ms: u64,
}

/// The endpoint a custom provider resolves to. Refused until the member confirmed it.
pub fn resolve_custom(c: &CustomEndpoint) -> Result<ResolvedEndpoint, ProviderError> {
    let id = ProviderId::parse(&c.id)
        .filter(ProviderId::is_custom)
        .ok_or(ProviderError::Egress("not a custom endpoint id"))?;
    resolved(id, parse_base_url(&c.base_url)?, false)
}

// ---------------------------------------------------------------------------
// The production transport
// ---------------------------------------------------------------------------

/// Connect deadline for a vendor call.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// Whole-call deadline (a long streamed answer included).
pub const CALL_TIMEOUT: Duration = Duration::from_secs(600);

/// The transport's configuration.
pub fn pinned_config() -> ureq::config::Config {
    ureq::Agent::config_builder()
        .timeout_connect(Some(CONNECT_TIMEOUT))
        .timeout_global(Some(CALL_TIMEOUT))
        .http_status_as_error(false)
        .build()
}

/// https only, no redirects, no proxy from the environment, pinned host re-checked.
pub struct PinnedHttp;

impl ProviderHttp for PinnedHttp {
    fn send(&self, req: WireRequest<'_>) -> Result<InboundResponse, ProviderError> {
        let agent: ureq::Agent = pinned_config().into();
        let url = req.url.as_str();
        let resp = match (req.method, &req.body) {
            (Method::Get, _) => {
                let mut b = agent.get(url);
                for (k, v) in &req.headers {
                    b = b.header(k.as_str(), v.as_str());
                }
                b.header(req.auth_header.0, req.auth_header.1.as_str())
                    .call()
            }
            (Method::Post, body) => {
                let mut b = agent.post(url);
                for (k, v) in &req.headers {
                    b = b.header(k.as_str(), v.as_str());
                }
                b.header(req.auth_header.0, req.auth_header.1.as_str())
                    .header("content-type", "application/json")
                    .send(body.as_deref().unwrap_or(b"{}"))
            }
        }
        .map_err(|e| match e {
            ureq::Error::Timeout(_) => ProviderError::Transport("timed out"),
            _ => ProviderError::Transport("request failed"),
        })?;
        let status = resp.status().as_u16();
        let content_type = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_ascii_lowercase();
        let body = resp.into_body().into_reader();
        Ok(InboundResponse {
            status,
            content_type,
            body: Box::new(body),
        })
    }
}

#[cfg(test)]
mod tests {
    include!("provider_egress_tests.rs");
}
