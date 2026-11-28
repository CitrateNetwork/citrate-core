//! Discovery (L6-WP06 step 1): from a remote MCP server URL to its protected resource metadata
//! (RFC 9728) and its authorization server metadata (RFC 8414 / OpenID Connect Discovery), as
//! the MCP authorization specification 2025-11-25 orders them.
//!
//! Normalizations, each from the L6-WP07 provider matrix (quirks 1 to 4 and 8):
//! - a resource identifier may differ from the endpoint by one trailing slash (Slack, GitHub,
//!   Vercel, HubSpot) or be a same-origin prefix of it (Slack, Figma, X, Asana): see
//!   [`resource_covers`];
//! - an issuer may differ from the metadata `issuer` by one trailing slash (Google): see
//!   [`issuers_match`];
//! - an issuer with a path uses path-inserted metadata URLs (Atlassian, Stripe): see
//!   [`as_metadata_candidates`];
//! - a server that answers `initialize` without a token (Gmail, Drive, Hugging Face) is still
//!   discovered through the well-known URLs.

use super::http::{self, McpHttp, Request};
use super::McpAuthError;
use serde::Deserialize;
use url::Url;

/// The parameters of a `Bearer` challenge that matter here (RFC 6750 section 3, RFC 9728 5.1).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Challenge {
    pub resource_metadata: Option<String>,
    pub scope: Option<String>,
    pub error: Option<String>,
}

/// Parse the `Bearer` challenge out of a `WWW-Authenticate` value. Other schemes and unknown
/// parameters (Figma's `authorization_uri`, quirk 7) are ignored. `None` when there is no
/// `Bearer` challenge.
pub(crate) fn parse_www_authenticate(value: &str) -> Option<Challenge> {
    let mut rest = value.trim();
    let mut in_bearer = false;
    let mut found = false;
    let mut out = Challenge::default();
    while !rest.is_empty() {
        rest = rest.trim_start_matches([',', ' ', '\t']);
        // A token: either a scheme name or a parameter name.
        let end = rest
            .find(|c: char| c == '=' || c == ',' || c.is_whitespace())
            .unwrap_or(rest.len());
        let token = &rest[..end];
        let after = rest[end..].trim_start();
        if let Some(v) = after.strip_prefix('=') {
            let v = v.trim_start();
            let (value, remaining) = if let Some(q) = v.strip_prefix('"') {
                let mut s = String::new();
                let mut chars = q.char_indices();
                let mut close = q.len();
                while let Some((i, c)) = chars.next() {
                    match c {
                        '\\' => {
                            if let Some((_, n)) = chars.next() {
                                s.push(n);
                            }
                        }
                        '"' => {
                            close = i + 1;
                            break;
                        }
                        _ => s.push(c),
                    }
                }
                (s, &q[close.min(q.len())..])
            } else {
                let e = v.find(',').unwrap_or(v.len());
                (v[..e].trim().to_string(), &v[e..])
            };
            if in_bearer {
                match token.to_ascii_lowercase().as_str() {
                    "resource_metadata" => out.resource_metadata = Some(value),
                    "scope" => out.scope = Some(value),
                    "error" => out.error = Some(value),
                    _ => {}
                }
            }
            rest = remaining;
        } else {
            if token.is_empty() {
                break;
            }
            // A new scheme starts.
            in_bearer = token.eq_ignore_ascii_case("bearer");
            found |= in_bearer;
            rest = after;
        }
    }
    found.then_some(out)
}

/// The comparison key of a resource or endpoint URL: lowercase scheme and host (the `url` crate
/// already lowercases them), the port when not the default, and the path with one trailing slash
/// removed (so `https://mcp.vercel.com/` and `https://mcp.vercel.com` are one resource). The query
/// is kept; there is never a fragment ([`http::check_url`]).
pub(crate) fn canonical_key(u: &Url) -> String {
    let mut path = u.path().to_string();
    if path.ends_with('/') {
        path.pop();
    }
    let port = match u.port() {
        Some(p) => format!(":{p}"),
        None => String::new(),
    };
    let query = u.query().map(|q| format!("?{q}")).unwrap_or_default();
    format!(
        "{}://{}{port}{path}{query}",
        u.scheme(),
        u.host_str().unwrap_or("")
    )
}

/// Whether the resource identifier `resource` covers `endpoint`: same origin, and the endpoint
/// path is the resource path or below it (matrix quirks 1 and 2). Nothing else does.
pub(crate) fn resource_covers(resource: &Url, endpoint: &Url) -> bool {
    if resource.origin() != endpoint.origin() || resource.query().is_some() {
        return false;
    }
    let r = canonical_key(resource);
    let mut e = endpoint.clone();
    e.set_query(None);
    let e = canonical_key(&e);
    e == r || e.starts_with(&format!("{r}/"))
}

/// Whether two issuer identifiers name one issuer: equal, or equal once one trailing slash is
/// removed from each (Google lists `https://accounts.google.com/` in its PRM, quirk 3).
pub(crate) fn issuers_match(a: &str, b: &str) -> bool {
    a == b || a.strip_suffix('/').unwrap_or(a) == b.strip_suffix('/').unwrap_or(b)
}

/// The protected resource metadata URLs to try, in the specification's order: the challenge's
/// `resource_metadata` when present (and then only it); otherwise the path form, then the root.
pub(crate) fn prm_candidates(
    endpoint: &Url,
    challenge: Option<&Challenge>,
) -> Result<Vec<Url>, McpAuthError> {
    if let Some(raw) = challenge.and_then(|c| c.resource_metadata.as_deref()) {
        return Ok(vec![http::parse_checked(raw)?]);
    }
    let mut out = Vec::new();
    let path = endpoint.path().trim_end_matches('/');
    let origin = endpoint.origin().ascii_serialization();
    if !path.is_empty() {
        out.push(well_known(&origin, "oauth-protected-resource", path)?);
    }
    out.push(well_known(&origin, "oauth-protected-resource", "")?);
    Ok(out)
}

fn well_known(origin: &str, name: &str, path: &str) -> Result<Url, McpAuthError> {
    Url::parse(&format!("{origin}/.well-known/{name}{path}")).map_err(|_| McpAuthError::InsecureUrl)
}

/// The authorization server metadata URLs to try, in the specification's order.
pub(crate) fn as_metadata_candidates(issuer: &Url) -> Result<Vec<Url>, McpAuthError> {
    let origin = issuer.origin().ascii_serialization();
    let path = issuer.path().trim_end_matches('/');
    if path.is_empty() {
        return Ok(vec![
            well_known(&origin, "oauth-authorization-server", "")?,
            well_known(&origin, "openid-configuration", "")?,
        ]);
    }
    Ok(vec![
        well_known(&origin, "oauth-authorization-server", path)?,
        well_known(&origin, "openid-configuration", path)?,
        Url::parse(&format!("{origin}{path}/.well-known/openid-configuration"))
            .map_err(|_| McpAuthError::InsecureUrl)?,
    ])
}

#[derive(Deserialize)]
struct RawPrm {
    resource: Option<String>,
    #[serde(default)]
    authorization_servers: Vec<String>,
    #[serde(default)]
    scopes_supported: Vec<String>,
}

#[derive(Deserialize)]
struct RawAs {
    issuer: Option<String>,
    authorization_endpoint: Option<String>,
    token_endpoint: Option<String>,
    registration_endpoint: Option<String>,
    revocation_endpoint: Option<String>,
    #[serde(default)]
    code_challenge_methods_supported: Option<Vec<String>>,
    #[serde(default)]
    client_id_metadata_document_supported: bool,
    #[serde(default)]
    authorization_response_iss_parameter_supported: bool,
    #[serde(default)]
    token_endpoint_auth_methods_supported: Option<Vec<String>>,
}

/// The authorization server metadata that the flow uses, every URL checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AsMetadata {
    /// The metadata's `issuer`, exactly as published (the RFC 9207 `iss` is compared to it).
    pub issuer: String,
    pub authorization_endpoint: Url,
    pub token_endpoint: Url,
    pub registration_endpoint: Option<Url>,
    pub revocation_endpoint: Option<Url>,
    pub cimd_supported: bool,
    pub iss_parameter_supported: bool,
    /// `None` when absent (RFC 8414 default: `client_secret_basic`).
    pub token_auth_methods: Option<Vec<String>>,
}

/// Parse and validate one authorization server metadata document for `expected_issuer`.
pub(crate) fn parse_as_metadata(
    body: &str,
    expected_issuer: &str,
) -> Result<AsMetadata, McpAuthError> {
    let raw: RawAs = serde_json::from_str(body).map_err(|_| McpAuthError::BadMetadata("json"))?;
    let issuer = raw.issuer.ok_or(McpAuthError::BadMetadata("issuer"))?;
    if !issuers_match(&issuer, expected_issuer) {
        return Err(McpAuthError::IssuerMismatch);
    }
    let methods = raw.code_challenge_methods_supported.unwrap_or_default();
    if !methods.iter().any(|m| m == "S256") {
        return Err(McpAuthError::PkceS256Unsupported);
    }
    let req = |v: Option<String>, f: &'static str| -> Result<Url, McpAuthError> {
        http::parse_checked(&v.ok_or(McpAuthError::BadMetadata(f))?)
    };
    let opt = |v: Option<String>| -> Result<Option<Url>, McpAuthError> {
        v.map(|s| http::parse_checked(&s)).transpose()
    };
    Ok(AsMetadata {
        issuer,
        authorization_endpoint: req(raw.authorization_endpoint, "authorization_endpoint")?,
        token_endpoint: req(raw.token_endpoint, "token_endpoint")?,
        registration_endpoint: opt(raw.registration_endpoint)?,
        revocation_endpoint: opt(raw.revocation_endpoint)?,
        cimd_supported: raw.client_id_metadata_document_supported,
        iss_parameter_supported: raw.authorization_response_iss_parameter_supported,
        token_auth_methods: raw.token_endpoint_auth_methods_supported,
    })
}

/// Everything discovery found for one server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Discovered {
    pub endpoint: Url,
    /// The PRM `resource`, parsed (for comparisons).
    pub resource: Url,
    /// The PRM `resource`, exactly as published: the RFC 8707 `resource` parameter value (`Url`
    /// would add a slash to an empty path, and Slack's resource has none).
    pub resource_id: String,
    /// The scopes to request (specification "Scope Selection Strategy": the challenge's `scope`,
    /// else the PRM `scopes_supported`, else none).
    pub scopes: Vec<String>,
    pub auth_server: AsMetadata,
}

/// Discover `endpoint`. `fallback_metadata` supplies the metadata for an issuer whose server
/// publishes none at the well-known URLs (a fixed client row, see `registration`); it is asked
/// only after every specification URL failed.
pub(crate) fn discover(
    net: &dyn McpHttp,
    endpoint: &Url,
    fallback_metadata: &dyn Fn(&str) -> Option<AsMetadata>,
) -> Result<Discovered, McpAuthError> {
    http::check_url(endpoint)?;
    let probe = http::send(net, endpoint, Request::Probe)?;
    let challenge = match (probe.status, probe.www_authenticate.as_deref()) {
        (401 | 403, Some(h)) => parse_www_authenticate(h),
        _ => None,
    };
    let mut prm = None;
    for url in prm_candidates(endpoint, challenge.as_ref())? {
        let reply = http::send(net, &url, Request::Get)?;
        if reply.status == 200 {
            prm = Some(
                serde_json::from_str::<RawPrm>(&reply.body)
                    .map_err(|_| McpAuthError::BadMetadata("json"))?,
            );
            break;
        }
    }
    let prm = prm.ok_or(McpAuthError::NoResourceMetadata)?;
    let resource_id = prm.resource.ok_or(McpAuthError::BadMetadata("resource"))?;
    let resource = http::parse_checked(&resource_id)?;
    if !resource_covers(&resource, endpoint) {
        return Err(McpAuthError::ResourceMismatch);
    }
    let as_url = prm
        .authorization_servers
        .first()
        .ok_or(McpAuthError::BadMetadata("authorization_servers"))?;
    let as_parsed = http::parse_checked(as_url)?;
    let mut meta = None;
    for url in as_metadata_candidates(&as_parsed)? {
        let reply = match http::send(net, &url, Request::Get) {
            Ok(r) => r,
            Err(McpAuthError::Network | McpAuthError::Timeout) => continue,
            Err(e) => return Err(e),
        };
        if reply.status == 200 {
            meta = Some(parse_as_metadata(&reply.body, as_url)?);
            break;
        }
    }
    let auth_server = match meta {
        Some(m) => m,
        None => fallback_metadata(as_url.as_str())
            .ok_or(McpAuthError::BadMetadata("authorization server metadata"))?,
    };
    let scopes = match challenge.and_then(|c| c.scope) {
        Some(s) => s.split_whitespace().map(str::to_string).collect(),
        None => prm.scopes_supported,
    };
    Ok(Discovered {
        endpoint: endpoint.clone(),
        resource,
        resource_id,
        scopes,
        auth_server,
    })
}
