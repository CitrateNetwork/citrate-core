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
    /// The exact endpoint paths (under `base_url`) an adapter may call, with an optional query.
    /// `{model}` stands for one model id (`[A-Za-z0-9._-]`, 1..=128, not starting with `.`).
    pub paths: &'static [&'static str],
}

/// The endpoints of an OpenAI chat-completions API: xAI, DeepSeek and Meta, and every custom
/// endpoint.
pub const CHAT_PATHS: &[&str] = &["/chat/completions", "/models"];

/// The six governed vendors (ADR P-3). Hosts are the vendors' published API hosts, checked
/// against each vendor's own API docs on 2026-10-26 (sources in each adapter's module docs).
/// Meta: the Meta Model API serving the Muse models is `https://api.meta.ai/v1`
/// (<https://dev.meta.ai/docs/overview>); the earlier Llama API host is not used.
pub const VENDORS: &[VendorSpec] = &[
    VendorSpec {
        vendor: Vendor::OpenAi,
        name: "OpenAI",
        base_url: "https://api.openai.com/v1",
        key_page: "https://platform.openai.com/api-keys",
        revoke_page: "https://platform.openai.com/api-keys",
        paths: &["/responses", "/models"],
    },
    VendorSpec {
        vendor: Vendor::Anthropic,
        name: "Anthropic",
        base_url: "https://api.anthropic.com/v1",
        key_page: "https://console.anthropic.com/settings/keys",
        revoke_page: "https://console.anthropic.com/settings/keys",
        paths: &["/messages", "/models?limit=1000"],
    },
    VendorSpec {
        vendor: Vendor::Gemini,
        name: "Google Gemini",
        base_url: "https://generativelanguage.googleapis.com/v1beta",
        key_page: "https://aistudio.google.com/apikey",
        revoke_page: "https://aistudio.google.com/apikey",
        paths: &[
            "/models/{model}:generateContent",
            "/models/{model}:streamGenerateContent?alt=sse",
            "/models?pageSize=1000",
        ],
    },
    VendorSpec {
        vendor: Vendor::Xai,
        name: "xAI",
        base_url: "https://api.x.ai/v1",
        key_page: "https://console.x.ai",
        revoke_page: "https://console.x.ai",
        paths: CHAT_PATHS,
    },
    VendorSpec {
        vendor: Vendor::DeepSeek,
        name: "DeepSeek",
        // The documented base (<https://api-docs.deepseek.com/>): the bare host.
        base_url: "https://api.deepseek.com",
        key_page: "https://platform.deepseek.com/api_keys",
        revoke_page: "https://platform.deepseek.com/api_keys",
        paths: CHAT_PATHS,
    },
    VendorSpec {
        vendor: Vendor::Meta,
        name: "Meta",
        base_url: "https://api.meta.ai/v1",
        key_page: "https://dev.meta.ai/platform",
        revoke_page: "https://dev.meta.ai/platform",
        paths: CHAT_PATHS,
    },
];

/// A remote-agent delegate (ADR P-8). Not a chat model; listed so the delegate WP shares this
/// egress table, the key store and the usage log.
// Consumed by WP-S1-PROV-AGENT (the Cognition delegate); pinned by the egress tests here.
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DelegateSpec {
    pub id: &'static str,
    pub name: &'static str,
    pub base_url: &'static str,
    pub key_page: &'static str,
}

/// Cognition: API v3 (`https://api.devin.ai/v3`, `Authorization: Bearer cog_…`, sessions at
/// `POST /v3/organizations/{org_id}/sessions`); v1/v2 and `apk_` keys are legacy
/// (<https://docs.devin.ai/api-reference/authentication>).
#[cfg_attr(not(test), allow(dead_code))]
pub const DELEGATES: &[DelegateSpec] = &[DelegateSpec {
    id: "cognition",
    name: "Cognition (Devin)",
    base_url: "https://api.devin.ai/v3",
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
    /// The exact endpoint paths an adapter may call here ([`VendorSpec::paths`]).
    pub paths: &'static [&'static str],
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
    paths: &'static [&'static str],
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
        paths,
    })
}

/// The pinned endpoint of a governed vendor.
pub fn resolve_vendor(v: Vendor) -> Result<ResolvedEndpoint, ProviderError> {
    let spec = vendor_spec(v);
    resolved(
        ProviderId::Vendor(v),
        parse_base_url(spec.base_url)?,
        true,
        spec.paths,
    )
}

/// Whether `path` is one of the endpoint's exact paths (`{model}` matching one model id).
pub fn path_allowed(paths: &[&str], path: &str) -> bool {
    paths.iter().any(|p| match p.split_once("{model}") {
        None => *p == path,
        Some((pre, post)) => path
            .strip_prefix(pre)
            .and_then(|rest| rest.strip_suffix(post))
            .is_some_and(|m| {
                !m.is_empty()
                    && m.len() <= 128
                    && !m.starts_with('.')
                    && m.bytes()
                        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
            }),
    })
}

/// Join an adapter's path to the endpoint. The path must be one of the endpoint's exact
/// endpoint paths ([`VendorSpec::paths`], or [`CHAT_PATHS`] for a custom endpoint), which holds
/// for a bare-host base as much as for one with a path; the joined URL is then re-checked to be
/// on the pinned host and port, over https, with no userinfo or fragment.
pub fn join_checked(ep: &ResolvedEndpoint, path: &str) -> Result<url::Url, ProviderError> {
    let bad = |m| Err(ProviderError::Egress(m));
    if !path.starts_with('/') || path.starts_with("//") {
        return bad("an adapter path must start with a single /");
    }
    if path.len() > 2048
        || path
            .bytes()
            .any(|b| !b.is_ascii_graphic() || matches!(b, b'\\' | b'@' | b'#'))
    {
        return bad("an adapter path has characters that are not allowed");
    }
    if !path_allowed(ep.paths, path) {
        return bad("that is not one of this provider's API endpoints");
    }
    let mut joined = ep.base_url.as_str().trim_end_matches('/').to_string();
    joined.push_str(path);
    let url = url::Url::parse(&joined).map_err(|_| ProviderError::Egress("bad path"))?;
    check_on_endpoint(ep, &url)?;
    Ok(url)
}

/// The URL is https, on the endpoint's exact host and port, with no userinfo or fragment.
pub fn check_on_endpoint(ep: &ResolvedEndpoint, url: &url::Url) -> Result<(), ProviderError> {
    let same_host = url
        .host_str()
        .is_some_and(|h| h.eq_ignore_ascii_case(&ep.host));
    if url.scheme() != "https"
        || !same_host
        || url.port_or_known_default() != Some(ep.port)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(ProviderError::Egress(
            "the request would leave the provider's pinned host",
        ));
    }
    Ok(())
}

/// Headers an adapter may never set: auth (core places the key), routing, framing and cookies.
const FORBIDDEN_HEADERS: &[&str] = &[
    "authorization",
    "proxy-authorization",
    "x-api-key",
    "x-goog-api-key",
    "api-key",
    "cookie",
    "host",
    "content-length",
    "transfer-encoding",
    "connection",
    "upgrade",
    "te",
    "trailer",
    "forwarded",
    "x-forwarded-for",
    "x-forwarded-host",
    "x-forwarded-proto",
];

/// Extra headers an adapter may set: non-secret protocol headers only, with a token name and a
/// printable value (no header injection).
pub fn check_extra_header(name: &str, value: &str) -> Result<(), ProviderError> {
    let lname = name.to_ascii_lowercase();
    if name.is_empty()
        || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        || FORBIDDEN_HEADERS.contains(&lname.as_str())
    {
        return Err(ProviderError::Egress("an adapter may not set that header"));
    }
    if value.len() > 1024 || value.bytes().any(|b| b.is_ascii_control()) {
        return Err(ProviderError::Egress(
            "a header value has characters that are not allowed",
        ));
    }
    Ok(())
}

/// The headers a vendor key may be placed in: the vendors' own auth headers.
pub const AUTH_HEADERS: &[&str] = &["x-api-key", "x-goog-api-key"];

/// The auth header names an adapter may place the key in (besides `Authorization: Bearer`).
pub fn check_auth_header_name(name: &str) -> Result<(), ProviderError> {
    if AUTH_HEADERS.contains(&name) {
        Ok(())
    } else {
        Err(ProviderError::Egress(
            "a key may only go in the vendor's own auth header",
        ))
    }
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

/// The host rules for a custom endpoint, on top of [`parse_base_url`]: a DNS name (no IP literal),
/// not a local-only name, no trailing dot.
pub fn check_custom_host(base_url: &str) -> Result<(), ProviderError> {
    let u = parse_base_url(base_url)?;
    let Some(url::Host::Domain(host)) = u.host() else {
        return Err(ProviderError::Egress(
            "a custom endpoint needs a host name, not an IP address",
        ));
    };
    let host = host.to_ascii_lowercase();
    const LOCAL_SUFFIXES: &[&str] = &[".localhost", ".local", ".internal", ".lan", ".home.arpa"];
    if host.ends_with('.')
        || !host.contains('.')
        || host == "localhost"
        || LOCAL_SUFFIXES.iter().any(|s| host.ends_with(s))
    {
        return Err(ProviderError::Egress(
            "a custom endpoint needs a public host name (no local names or trailing dot)",
        ));
    }
    Ok(())
}

/// The endpoint a custom provider resolves to. Refused until the member confirmed it.
pub fn resolve_custom(c: &CustomEndpoint) -> Result<ResolvedEndpoint, ProviderError> {
    let id = ProviderId::parse(&c.id)
        .filter(ProviderId::is_custom)
        .ok_or(ProviderError::Egress("not a custom endpoint id"))?;
    check_custom_host(&c.base_url)?;
    resolved(id, parse_base_url(&c.base_url)?, false, CHAT_PATHS)
}

// ---------------------------------------------------------------------------
// The production transport
// ---------------------------------------------------------------------------

/// Connect deadline for a vendor call.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// Whole-call deadline (a long streamed answer included).
pub const CALL_TIMEOUT: Duration = Duration::from_secs(600);

/// The transport's configuration: https only; no redirect is followed (a 3xx comes back as an
/// answer and the adapter maps it to [`ProviderError::Redirect`]); no proxy, whatever
/// `HTTPS_PROXY` / `ALL_PROXY` say (ureq reads them by default); statuses are read, not raised.
pub fn pinned_config() -> ureq::config::Config {
    ureq::Agent::config_builder()
        .https_only(true)
        .max_redirects(0)
        .max_redirects_will_error(false)
        .proxy(None)
        .timeout_connect(Some(CONNECT_TIMEOUT))
        .timeout_global(Some(CALL_TIMEOUT))
        .http_status_as_error(false)
        .build()
}

/// https only, no redirects, no proxy from the environment, pinned host re-checked.
pub struct PinnedHttp;

impl ProviderHttp for PinnedHttp {
    fn send(&self, req: WireRequest<'_>) -> Result<InboundResponse, ProviderError> {
        // Defence in depth: the composed URL is re-checked against the endpoint here, at the
        // last point before a socket opens.
        check_on_endpoint(req.endpoint, &req.url)?;
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
