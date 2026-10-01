//! HUP-S0.3b — the Hugging Face read token for gated / private model downloads.
//!
//! The member's Hugging Face connection (Settings › Connections, sealed in the custody vault by
//! `connections.rs`) is read IN-PROCESS by the download path and attached as
//! `Authorization: Bearer …` ONLY to requests whose origin is exactly `https://huggingface.co`
//! or `https://hf.co` (default port, no userinfo). A Hugging Face resolve answers with a redirect
//! to a CDN on another host (`cdn-lfs*.hf.co`, `*.xethub.hf.co`, …) whose signed URL needs no
//! token, so redirects are followed HERE, hop by hop, and every hop recomputes the header from
//! its own origin: a cross-origin hop never carries it, a same-origin hop keeps it.
//!
//! ## Secret discipline (I-2)
//! - The token never crosses the invoke boundary: no `#[tauri::command]` here, and no error or
//!   `Debug` output carries it ([`HfToken`]'s `Debug` is redacted; [`FetchError`] has no
//!   token-bearing variant).
//! - The buffers that hold it are `Zeroizing`.
//! - A token with bytes that could break the header line (CR/LF, spaces, controls) is refused.
//!
//! ## Honest gating
//! A `401`/`403` from an in-scope origin is [`FetchError::Gated`], whose message tells the member
//! the model needs a Hugging Face token with access and where to add it. A `401`/`403` from any
//! other origin (an expired CDN signature, say) stays a plain status error.

use zeroize::Zeroizing;

/// The origins the token may be sent to. Exact host match, https, default port.
pub const HF_AUTH_HOSTS: &[&str] = &["huggingface.co", "hf.co"];

/// Redirect hops followed before giving up (ureq's own default is also 10).
pub const MAX_REDIRECTS: u32 = 10;

// ---------------------------------------------------------------------------
// The token
// ---------------------------------------------------------------------------

/// A Hugging Face access token held in-process for request auth. Never serialized, never
/// returned to the webview; `Debug` is redacted and the value is wiped on drop.
pub struct HfToken(Zeroizing<String>);

impl HfToken {
    /// Wrap a token. `None` for an empty value or one containing anything outside visible ASCII
    /// (so a stored value can never inject a header line or split the request).
    #[cfg(test)]
    pub fn new(raw: String) -> Option<HfToken> {
        HfToken::from_zeroizing(Zeroizing::new(raw))
    }

    /// [`HfToken::new`] over a value already in a wiping buffer (no unwiped copy is made).
    pub fn from_zeroizing(raw: Zeroizing<String>) -> Option<HfToken> {
        if raw.is_empty() || !raw.bytes().all(|b| b.is_ascii_graphic()) {
            return None;
        }
        Some(HfToken(raw))
    }

    fn bearer(&self) -> Zeroizing<String> {
        Zeroizing::new(format!("Bearer {}", self.0.as_str()))
    }
}

impl std::fmt::Debug for HfToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("HfToken(<redacted>)")
    }
}

/// The member's connected Hugging Face token, read in-process from the custody vault (Settings ›
/// Connections). `None` when Hugging Face is not connected, the vault is locked, or the token has
/// expired; the download then proceeds without auth (public repos need none).
pub fn connected_token<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Option<HfToken> {
    use tauri::Manager as _;
    let custody = app.try_state::<crate::custody::CustodyState>()?;
    let raw = crate::connections::sealed_access_token(
        crate::connections::Service::HuggingFace,
        &custody.0,
    )?;
    HfToken::from_zeroizing(raw)
}

// ---------------------------------------------------------------------------
// The origin scope
// ---------------------------------------------------------------------------

/// The set of exact origins (scheme, host, port) a token may be attached to.
#[derive(Clone, Debug)]
pub struct AuthScope {
    origins: Vec<(String, String, u16)>,
}

impl AuthScope {
    /// Production scope: `https://huggingface.co` and `https://hf.co` on port 443.
    pub fn huggingface() -> AuthScope {
        AuthScope {
            origins: HF_AUTH_HOSTS
                .iter()
                .map(|h| ("https".to_string(), (*h).to_string(), 443))
                .collect(),
        }
    }

    /// A single exact origin (e.g. a loopback test server `http://127.0.0.1:<port>`).
    #[cfg(test)]
    pub fn exact_for_tests(origin: &str) -> AuthScope {
        AuthScope::exact_origins_for_tests(&[origin])
    }

    /// Several exact origins (loopback test servers).
    #[cfg(test)]
    pub fn exact_origins_for_tests(origins: &[&str]) -> AuthScope {
        AuthScope {
            origins: origins
                .iter()
                .filter_map(|o| url::Url::parse(o).ok())
                .filter_map(|u| {
                    Some((
                        u.scheme().to_string(),
                        u.host_str()?.to_string(),
                        u.port_or_known_default()?,
                    ))
                })
                .collect(),
        }
    }

    /// Whether `url` is exactly one of the scope's origins (and carries no userinfo).
    pub fn allows(&self, url: &url::Url) -> bool {
        if !url.username().is_empty() || url.password().is_some() {
            return false;
        }
        let (Some(host), Some(port)) = (url.host_str(), url.port_or_known_default()) else {
            return false;
        };
        self.origins.iter().any(|(s, h, p)| {
            s.as_str() == url.scheme() && h.eq_ignore_ascii_case(host) && *p == port
        })
    }
}

/// The `Authorization` value for a request to `url`: `Some` only with a token AND an in-scope
/// origin.
pub fn header_for(
    scope: &AuthScope,
    token: Option<&HfToken>,
    url: &url::Url,
) -> Option<Zeroizing<String>> {
    match token {
        Some(t) if scope.allows(url) => Some(t.bearer()),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Fetch with manual redirects
// ---------------------------------------------------------------------------

/// Per-hop timeouts (each `None` leaves ureq's default).
#[derive(Clone, Copy, Debug, Default)]
pub struct Timeouts {
    pub connect: Option<std::time::Duration>,
    pub recv_response: Option<std::time::Duration>,
    pub global: Option<std::time::Duration>,
}

/// A fetch failure. Carries no URL query (signed CDN URLs), no header, and no token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchError {
    /// The URL (or a redirect target) did not parse.
    BadUrl,
    /// A redirect had no usable `Location`, or pointed at a non-http(s) scheme.
    BadRedirect,
    /// More than [`MAX_REDIRECTS`] hops.
    TooManyRedirects,
    /// A `401`/`403` from an in-scope (Hugging Face) origin: the repo is gated or private.
    Gated { token_sent: bool },
    /// Any other non-2xx final status.
    Status(u16),
    /// Connection / TLS / timeout failure.
    Transport(String),
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FetchError::BadUrl => f.write_str("the model URL is not valid"),
            FetchError::BadRedirect => f.write_str("the model server sent an unusable redirect"),
            FetchError::TooManyRedirects => f.write_str("the model server redirected too many times"),
            FetchError::Gated { token_sent: false } => f.write_str(
                "This model needs a Hugging Face token with access. Add it in Settings › Connections \
                 (connect Hugging Face), and accept the model's terms on its Hugging Face page if it asks.",
            ),
            FetchError::Gated { token_sent: true } => f.write_str(
                "This model needs a Hugging Face token with access, and the connected Hugging Face \
                 account was refused. Accept the model's terms on its Hugging Face page, then reconnect \
                 Hugging Face in Settings › Connections.",
            ),
            FetchError::Status(s) => write!(f, "unexpected HTTP status {s}"),
            FetchError::Transport(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for FetchError {}

/// Resolve a redirect `Location` against the current URL. Only http(s) targets are accepted.
fn next_url(current: &url::Url, location: Option<&str>) -> Result<url::Url, FetchError> {
    let loc = location
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .ok_or(FetchError::BadRedirect)?;
    let next = current.join(loc).map_err(|_| FetchError::BadRedirect)?;
    match next.scheme() {
        "http" | "https" => Ok(next),
        _ => Err(FetchError::BadRedirect),
    }
}

/// `GET url` (with an optional `Range`), following up to [`MAX_REDIRECTS`] redirects by hand.
/// Every hop attaches `Authorization` only if [`header_for`] allows that hop's origin. Returns the
/// final 2xx response; any other final status is an error ([`FetchError::Gated`] for a
/// `401`/`403` from an in-scope origin).
pub fn fetch(
    url: &str,
    range: Option<&str>,
    scope: &AuthScope,
    token: Option<&HfToken>,
    timeouts: &Timeouts,
) -> Result<ureq::http::Response<ureq::Body>, FetchError> {
    let mut current = url::Url::parse(url).map_err(|_| FetchError::BadUrl)?;
    let mut hops: u32 = 0;
    loop {
        let mut req = ureq::get(current.as_str())
            .config()
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_connect(timeouts.connect)
            .timeout_recv_response(timeouts.recv_response)
            .timeout_global(timeouts.global)
            .build();
        if let Some(r) = range {
            req = req.header("Range", r);
        }
        let auth = header_for(scope, token, &current);
        let token_sent = auth.is_some();
        if let Some(value) = auth.as_ref() {
            req = req.header("Authorization", value.as_str());
        }
        let resp = req
            .call()
            .map_err(|e| FetchError::Transport(e.to_string()))?;
        drop(auth);
        let status = resp.status().as_u16();
        if resp.status().is_redirection() && status != 304 {
            if hops >= MAX_REDIRECTS {
                return Err(FetchError::TooManyRedirects);
            }
            hops += 1;
            let location = resp
                .headers()
                .get("location")
                .and_then(|v| v.to_str().ok())
                .map(str::to_string);
            current = next_url(&current, location.as_deref())?;
            continue;
        }
        if resp.status().is_success() {
            return Ok(resp);
        }
        if (status == 401 || status == 403) && scope.allows(&current) {
            return Err(FetchError::Gated { token_sent });
        }
        return Err(FetchError::Status(status));
    }
}

#[cfg(test)]
mod tests {
    include!("hf_auth_tests.rs");
}
