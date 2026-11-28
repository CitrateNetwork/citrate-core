//! The hardened fetch every MCP authorization request goes through (L6-WP06 step 1): https only,
//! no user info or fragment, no redirect followed (a 3xx is returned as a status), DNS resolved
//! once and every address checked public (`fl_collect::is_public_ip`), the connection pinned to
//! those addresses, 10 s, 64 KiB, no proxy from the environment.
//!
//! The server URL comes from a catalog entry or the member, and every later URL (metadata,
//! endpoints) comes from the server's own documents, so each one is checked here, not trusted.

use super::McpAuthError;
use std::net::IpAddr;
use std::time::Duration;
use url::Url;
use zeroize::Zeroizing;

/// Cap on any body read (metadata, token answer, registration answer).
pub(crate) const MAX_BODY: u64 = 64 * 1024;

/// Whole-request deadline.
pub(crate) const TIMEOUT: Duration = Duration::from_secs(10);

/// The MCP protocol version sent on the discovery probe.
pub(crate) const PROBE_PROTOCOL_VERSION: &str = "2025-11-25";

/// The unauthenticated `initialize` the discovery probe sends (it carries no member data).
pub(crate) const PROBE_BODY: &str = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"Citrate Core","version":"0.6.0"}}}"#;

/// One request.
pub(crate) enum Request<'a> {
    Get,
    /// `application/x-www-form-urlencoded`; `basic` is HTTP Basic client authentication.
    PostForm {
        form: &'a [(&'a str, &'a str)],
        basic: Option<(&'a str, &'a str)>,
    },
    PostJson(&'a str),
    /// The MCP `initialize` probe ([`PROBE_BODY`]) with the MCP `Accept` header.
    Probe,
}

/// One answer. The body may hold tokens, so it wipes on drop.
pub(crate) struct Reply {
    pub status: u16,
    pub www_authenticate: Option<String>,
    pub body: Zeroizing<String>,
}

impl std::fmt::Debug for Reply {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Reply")
            .field("status", &self.status)
            .field("body", &"<redacted>")
            .finish()
    }
}

/// The network seam. Production is [`LiveHttp`]; tests script it.
pub(crate) trait McpHttp: Send + Sync {
    /// Resolve `host` once. The answer is vetted by [`send`], then pinned.
    fn resolve(&self, host: &str, port: u16) -> Result<Vec<IpAddr>, McpAuthError>;
    /// Send `req` to `url`, connecting only to `pinned` (never resolving again).
    fn send(&self, url: &Url, pinned: &[IpAddr], req: Request<'_>) -> Result<Reply, McpAuthError>;
}

/// Refuse anything but a plain public https URL. A literal IP host must be public; `localhost`
/// and `*.localhost` are refused by name.
pub(crate) fn check_url(url: &Url) -> Result<(), McpAuthError> {
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(McpAuthError::InsecureUrl);
    }
    match url.host() {
        None => Err(McpAuthError::InsecureUrl),
        Some(url::Host::Ipv4(ip)) if !crate::fl_collect::is_public_ip(IpAddr::V4(ip)) => {
            Err(McpAuthError::PrivateAddress)
        }
        Some(url::Host::Ipv6(ip)) if !crate::fl_collect::is_public_ip(IpAddr::V6(ip)) => {
            Err(McpAuthError::PrivateAddress)
        }
        Some(url::Host::Domain(d)) => {
            let d = d.trim_end_matches('.').to_ascii_lowercase();
            if d == "localhost" || d.ends_with(".localhost") {
                Err(McpAuthError::PrivateAddress)
            } else {
                Ok(())
            }
        }
        Some(_) => Ok(()),
    }
}

/// Parse and [`check_url`] a URL taken from a server document.
pub(crate) fn parse_checked(raw: &str) -> Result<Url, McpAuthError> {
    let url = Url::parse(raw).map_err(|_| McpAuthError::InsecureUrl)?;
    check_url(&url)?;
    Ok(url)
}

/// Check `url`, resolve it once, refuse any non-public answer, then send pinned to that answer.
pub(crate) fn send(http: &dyn McpHttp, url: &Url, req: Request<'_>) -> Result<Reply, McpAuthError> {
    check_url(url)?;
    let addrs = match url.host() {
        Some(url::Host::Ipv4(ip)) => vec![IpAddr::V4(ip)],
        Some(url::Host::Ipv6(ip)) => vec![IpAddr::V6(ip)],
        Some(url::Host::Domain(d)) => {
            http.resolve(d, url.port_or_known_default().unwrap_or(443))?
        }
        None => return Err(McpAuthError::InsecureUrl),
    };
    if addrs.is_empty() {
        return Err(McpAuthError::Network);
    }
    if addrs.iter().any(|ip| !crate::fl_collect::is_public_ip(*ip)) {
        return Err(McpAuthError::PrivateAddress);
    }
    http.send(url, &addrs, req)
}

/// The production network: ureq over rustls, pinned resolver, statuses read not raised.
pub(crate) struct LiveHttp;

impl McpHttp for LiveHttp {
    fn resolve(&self, host: &str, port: u16) -> Result<Vec<IpAddr>, McpAuthError> {
        use std::net::ToSocketAddrs;
        (host, port)
            .to_socket_addrs()
            .map(|a| a.map(|x| x.ip()).collect())
            .map_err(|_| McpAuthError::Network)
    }

    fn send(&self, url: &Url, pinned: &[IpAddr], req: Request<'_>) -> Result<Reply, McpAuthError> {
        let config = ureq::Agent::config_builder()
            .https_only(true)
            .max_redirects(0)
            .max_redirects_will_error(false)
            .proxy(None)
            .timeout_global(Some(TIMEOUT))
            .http_status_as_error(false)
            .build();
        let agent = ureq::Agent::with_parts(
            config,
            ureq::unversioned::transport::DefaultConnector::new(),
            crate::fl_collect::PinnedResolver(crate::fl_collect::pinned_socket_addrs(url, pinned)),
        );
        let sent = match req {
            Request::Get => agent
                .get(url.as_str())
                .header("Accept", "application/json")
                .call(),
            Request::PostForm { form, basic } => {
                let mut r = agent
                    .post(url.as_str())
                    .header("Accept", "application/json");
                let auth;
                if let Some((user, pass)) = basic {
                    use base64::Engine as _;
                    let raw = Zeroizing::new(format!("{user}:{pass}"));
                    auth = Zeroizing::new(format!(
                        "Basic {}",
                        base64::engine::general_purpose::STANDARD.encode(raw.as_bytes())
                    ));
                    r = r.header("Authorization", auth.as_str());
                }
                r.send_form(form.iter().copied())
            }
            Request::PostJson(body) => agent
                .post(url.as_str())
                .header("Content-Type", "application/json")
                .header("Accept", "application/json")
                .send(body),
            Request::Probe => agent
                .post(url.as_str())
                .header("Content-Type", "application/json")
                .header("Accept", "application/json, text/event-stream")
                .header("MCP-Protocol-Version", PROBE_PROTOCOL_VERSION)
                .send(PROBE_BODY),
        };
        let mut resp = sent.map_err(|e| match e {
            ureq::Error::Timeout(_) => McpAuthError::Timeout,
            _ => McpAuthError::Network,
        })?;
        let status = resp.status().as_u16();
        let www_authenticate = resp
            .headers()
            .get_all("www-authenticate")
            .iter()
            .filter_map(|v| v.to_str().ok())
            .collect::<Vec<_>>()
            .join(", ");
        let body = resp
            .body_mut()
            .with_config()
            .limit(MAX_BODY)
            .read_to_string()
            .map(Zeroizing::new)
            .map_err(|e| match e {
                ureq::Error::BodyExceedsLimit(_) => McpAuthError::TooLarge,
                ureq::Error::Timeout(_) => McpAuthError::Timeout,
                _ => McpAuthError::Network,
            });
        // A probe's body is an MCP stream that may not be text; only metadata bodies matter.
        let body = match (body, status) {
            (Ok(b), _) => b,
            (Err(McpAuthError::TooLarge), _) => return Err(McpAuthError::TooLarge),
            (Err(_), 401) => Zeroizing::new(String::new()),
            (Err(e), _) => return Err(e),
        };
        Ok(Reply {
            status,
            www_authenticate: (!www_authenticate.is_empty()).then_some(www_authenticate),
            body,
        })
    }
}
