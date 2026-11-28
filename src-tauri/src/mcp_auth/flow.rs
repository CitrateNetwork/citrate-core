//! The authorization code flow for a remote MCP server (L6-WP06 step 3): PKCE S256 (discovery
//! already refused a server that does not advertise it), a fresh `state`, the client's exact
//! registered redirect, the RFC 8707 `resource` on both the authorization and the token request,
//! the RFC 9207 `iss` check on the callback, and a token sealed bound to that resource.

use super::discovery::Discovered;
use super::http::{self, McpHttp, Request};
use super::registration::{self, Client};
use super::store::{self, ClientRef, TokenRecord};
use super::{oauth_error_code, McpAuthError};
use crate::connections::oauth_client::relay::{RelayOp, RelayTransport};
use crate::connections::{ct_eq, now_unix, pct, random_state, ConnError, Pkce};
use crate::custody::CustodyVault;
use crate::secret::SecretString;
use serde::Deserialize;
use zeroize::Zeroizing;

/// One authorization in flight. `Debug` shows no secret.
pub(crate) struct AuthRequest {
    pub url: String,
    pub state: String,
    verifier: Zeroizing<String>,
    pub client: Client,
    pub discovered: Discovered,
}

impl std::fmt::Debug for AuthRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuthRequest")
            .field("client", &self.client)
            .field("resource", &self.discovered.resource_id)
            .finish()
    }
}

impl AuthRequest {
    /// The PKCE verifier, for tests that check the challenge derivation.
    #[cfg(test)]
    pub(crate) fn verifier(&self) -> &str {
        &self.verifier
    }
}

/// Build the authorization request: fresh state and PKCE pair, the client's exact redirect,
/// `resource` and the selected scopes.
pub(crate) fn begin(discovered: Discovered, client: Client) -> AuthRequest {
    let state = random_state();
    let pkce = Pkce::new();
    let mut q: Vec<(&str, String)> = vec![
        ("response_type", "code".into()),
        ("client_id", client.client_id().into()),
        ("redirect_uri", client.redirect_uri().into()),
        ("state", state.clone()),
        ("code_challenge", pkce.challenge.clone()),
        ("code_challenge_method", "S256".into()),
        ("resource", discovered.resource_id.clone()),
    ];
    // A fixed client row may name narrower scopes and the provider's separator (Slack).
    let row_scopes = match &client {
        Client::Relay { .. } => {
            registration::fixed_for(&discovered.auth_server.issuer).and_then(|row| row.scopes)
        }
        _ => None,
    };
    let mut discovered = discovered;
    let sep = match row_scopes {
        Some((scopes, sep)) => {
            discovered.scopes = scopes.iter().map(|s| s.to_string()).collect();
            sep
        }
        None => " ",
    };
    if !discovered.scopes.is_empty() {
        q.push(("scope", discovered.scopes.join(sep)));
    }
    let query = q
        .iter()
        .map(|(k, v)| format!("{}={}", pct(k), pct(v)))
        .collect::<Vec<_>>()
        .join("&");
    let base = discovered.auth_server.authorization_endpoint.as_str();
    let sep = if base.contains('?') { '&' } else { '?' };
    AuthRequest {
        url: format!("{base}{sep}{query}"),
        state,
        verifier: pkce.verifier,
        client,
        discovered,
    }
}

/// What came back on the loopback. `Debug` redacts the code.
pub(crate) struct Callback {
    pub code: Option<Zeroizing<String>>,
    pub state: Option<String>,
    pub iss: Option<String>,
    pub error: Option<String>,
}

/// Parse a callback target (`/oauth/callback?..`).
pub(crate) fn parse_callback(target: &str) -> Callback {
    let mut cb = Callback {
        code: None,
        state: None,
        iss: None,
        error: None,
    };
    let Ok(parsed) = url::Url::parse("http://127.0.0.1/").and_then(|b| b.join(target)) else {
        return cb;
    };
    for (k, v) in parsed.query_pairs() {
        match k.as_ref() {
            "code" if !v.is_empty() => cb.code = Some(Zeroizing::new(v.into_owned())),
            "state" => cb.state = Some(v.into_owned()),
            "iss" => cb.iss = Some(v.into_owned()),
            "error" => cb.error = Some(v.into_owned()),
            _ => {}
        }
    }
    cb
}

/// Check a callback against its request: `state` (constant-time), the provider's error, then
/// `iss`. When the server advertises RFC 9207 support, `iss` must be present; whenever it is
/// present it must equal the metadata `issuer` exactly (a callback from another provider fails).
pub(crate) fn check_callback(
    req: &AuthRequest,
    cb: Callback,
) -> Result<Zeroizing<String>, McpAuthError> {
    match &cb.state {
        Some(s) if ct_eq(s.as_bytes(), req.state.as_bytes()) => {}
        _ => return Err(McpAuthError::StateMismatch),
    }
    if let Some(e) = cb.error.as_deref() {
        return Err(if e == "access_denied" {
            McpAuthError::Declined
        } else {
            McpAuthError::TokenRejected(super::sanitize_code(e))
        });
    }
    let issuer = &req.discovered.auth_server.issuer;
    match cb.iss.as_deref() {
        Some(iss) if iss != issuer => return Err(McpAuthError::IssMismatch),
        None if req.discovered.auth_server.iss_parameter_supported => {
            return Err(McpAuthError::IssMismatch)
        }
        _ => {}
    }
    cb.code.ok_or(McpAuthError::StateMismatch)
}

/// A token endpoint answer.
#[derive(Deserialize)]
pub(crate) struct Grant {
    pub access_token: SecretString,
    #[serde(default)]
    pub token_type: Option<String>,
    #[serde(default)]
    pub refresh_token: Option<SecretString>,
    #[serde(default)]
    pub expires_in: Option<u64>,
    #[serde(default)]
    pub scope: Option<String>,
}

impl std::fmt::Debug for Grant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Grant")
            .field("access_token", &"<redacted>")
            .field("expires_in", &self.expires_in)
            .finish()
    }
}

/// Parse a token endpoint reply: a non-2xx passes the server's OAuth `error` code through
/// (`invalid_grant` for an expired or replayed code); a 2xx must hold a non-empty bearer token.
pub(crate) fn parse_grant(status: u16, body: &str) -> Result<Grant, McpAuthError> {
    if !(200..300).contains(&status) {
        return Err(McpAuthError::TokenRejected(oauth_error_code(body)));
    }
    let g: Grant = serde_json::from_str(body)
        .map_err(|_| McpAuthError::TokenRejected("invalid_response".into()))?;
    let bearer = g
        .token_type
        .as_deref()
        .is_none_or(|t| t.eq_ignore_ascii_case("bearer"));
    if g.access_token.is_blank() || !bearer {
        return Err(McpAuthError::TokenRejected("invalid_response".into()));
    }
    Ok(g)
}

/// The form fields every token request carries for `client` (client id and, for a confidential
/// DCR client using the form, its secret).
pub(crate) fn client_fields<'a>(client: &'a Client, form: &mut Vec<(&'static str, &'a str)>) {
    form.push(("client_id", client.client_id()));
    if let Client::Confidential {
        secret,
        basic: false,
        ..
    } = client
    {
        form.push(("client_secret", secret.expose()));
    }
}

/// Post a token-endpoint form (direct clients) and parse the grant.
pub(crate) fn post_token_form(
    net: &dyn McpHttp,
    token_endpoint: &url::Url,
    client: &Client,
    form: &[(&str, &str)],
) -> Result<Grant, McpAuthError> {
    let basic = match client {
        Client::Confidential {
            client_id,
            secret,
            basic: true,
        } => Some((client_id.as_str(), secret.expose())),
        _ => None,
    };
    let reply = http::send(net, token_endpoint, Request::PostForm { form, basic })?;
    parse_grant(reply.status, &reply.body)
}

/// Map a relay failure for an MCP flow.
pub(crate) fn relay_err(e: crate::connections::oauth_client::relay::RelayError) -> McpAuthError {
    match e.into_conn(ConnError::TokenExchange) {
        ConnError::NotSignedIn => McpAuthError::NotSignedIn,
        ConnError::RateLimited => McpAuthError::RateLimited,
        ConnError::NotConfigured => {
            McpAuthError::NotConfigured("the Citrate relay has no client for it")
        }
        ConnError::Network => McpAuthError::Network,
        _ => McpAuthError::TokenRejected("relay_rejected".into()),
    }
}

/// Exchange `code` with the request's verifier, the same redirect and the same resource.
pub(crate) fn exchange(
    net: &dyn McpHttp,
    relay: Option<&dyn RelayTransport>,
    req: &AuthRequest,
    code: &str,
) -> Result<Grant, McpAuthError> {
    let resource = req.discovered.resource_id.as_str();
    let redirect = req.client.redirect_uri();
    if let Client::Relay { provider, .. } = &req.client {
        let relay = relay.ok_or(McpAuthError::NotSignedIn)?;
        let body = Zeroizing::new(
            serde_json::json!({
                "code": code,
                "code_verifier": req.verifier.as_str(),
                "redirect_uri": redirect,
                "resource": resource,
            })
            .to_string(),
        );
        let out = relay
            .post(provider, RelayOp::Token, &body)
            .map_err(relay_err)?;
        return parse_grant(200, &out);
    }
    let mut form: Vec<(&str, &str)> = vec![
        ("grant_type", "authorization_code"),
        ("code", code),
        ("redirect_uri", redirect),
        ("code_verifier", req.verifier.as_str()),
        ("resource", resource),
    ];
    client_fields(&req.client, &mut form);
    post_token_form(
        net,
        &req.discovered.auth_server.token_endpoint,
        &req.client,
        &form,
    )
}

/// The outcome of a connect, free of any secret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Connected {
    pub connector_id: String,
    pub resource: String,
    pub client_kind: &'static str,
    pub scope: Option<String>,
    /// Requested scopes the server did not grant (it may grant less, RFC 6749 section 3.3).
    pub missing_scopes: Vec<String>,
    /// Some requested scopes were not granted: the connector is "limited".
    pub limited: bool,
}

/// The scopes in a token answer's `scope`: space-separated per RFC 6749, comma-separated by
/// Slack; both are accepted.
pub(crate) fn split_scopes(s: &str) -> Vec<&str> {
    s.split(|c: char| c == ',' || c.is_whitespace())
        .filter(|x| !x.is_empty())
        .collect()
}

/// The requested scopes missing from `grant`, or `None` when the server did not say (RFC 6749
/// section 5.1: an absent `scope` means the requested scope was granted).
pub(crate) fn missing_from(req: &AuthRequest, grant: &Grant) -> Option<Vec<String>> {
    let granted = split_scopes(grant.scope.as_deref()?);
    Some(
        req.discovered
            .scopes
            .iter()
            .filter(|s| !granted.contains(&s.as_str()))
            .cloned()
            .collect(),
    )
}

/// Best-effort revoke of a token that will not be stored (none of the scopes were granted).
fn revoke_unused(
    net: &dyn McpHttp,
    relay: Option<&dyn RelayTransport>,
    req: &AuthRequest,
    grant: &Grant,
) {
    let token = grant.access_token.expose();
    if let Client::Relay { provider, .. } = &req.client {
        if let Some(relay) = relay {
            let body = Zeroizing::new(serde_json::json!({ "token": token }).to_string());
            let _ = relay.post(provider, RelayOp::Revoke, &body);
        }
        return;
    }
    if let Some(endpoint) = &req.discovered.auth_server.revocation_endpoint {
        let mut form: Vec<(&str, &str)> =
            vec![("token", token), ("token_type_hint", "access_token")];
        client_fields(&req.client, &mut form);
        let basic = match &req.client {
            Client::Confidential {
                client_id,
                secret,
                basic: true,
            } => Some((client_id.as_str(), secret.expose())),
            _ => None,
        };
        let _ = http::send(net, endpoint, Request::PostForm { form: &form, basic });
    }
}

/// Seal `grant` for `connector_id`, bound to the request's resource and issuer.
pub(crate) fn seal_grant(
    vault: &CustodyVault,
    connector_id: &str,
    req: &AuthRequest,
    grant: Grant,
) -> Result<Connected, McpAuthError> {
    let now = now_unix();
    let auth = &req.discovered.auth_server;
    let missing_scopes = missing_from(req, &grant).unwrap_or_default();
    let rec = TokenRecord {
        v: store::RECORD_V,
        connector_id: connector_id.to_string(),
        resource: req.discovered.resource_id.clone(),
        issuer: auth.issuer.clone(),
        token_endpoint: auth.token_endpoint.as_str().to_string(),
        revocation_endpoint: auth
            .revocation_endpoint
            .as_ref()
            .map(|u| u.as_str().to_string()),
        client: ClientRef {
            kind: req.client.kind().to_string(),
            client_id: req.client.client_id().to_string(),
            provider: match &req.client {
                Client::Relay { provider, .. } => Some(provider.to_string()),
                _ => None,
            },
            redirect_uri: req.client.redirect_uri().to_string(),
        },
        access_token: grant.access_token,
        refresh_token: grant.refresh_token.filter(|t| !t.is_blank()),
        scope: grant.scope.clone(),
        expires_at: grant.expires_in.map(|e| now.saturating_add(e)),
        connected_at: now,
        needs_refresh: false,
        missing_scopes: missing_scopes.clone(),
    };
    store::write(vault, &rec)?;
    Ok(Connected {
        connector_id: connector_id.to_string(),
        resource: rec.resource.clone(),
        client_kind: req.client.kind(),
        scope: grant.scope,
        limited: !missing_scopes.is_empty(),
        missing_scopes,
    })
}

/// Complete an authorization from a callback target: check it, exchange, seal.
pub(crate) fn complete(
    net: &dyn McpHttp,
    vault: &CustodyVault,
    relay: Option<&dyn RelayTransport>,
    connector_id: &str,
    req: &AuthRequest,
    cb: Callback,
) -> Result<Connected, McpAuthError> {
    let code = check_callback(req, cb)?;
    let grant = exchange(net, relay, req, &code)?;
    // Lead decision 2026-11-28: a granted subset is stored as "limited"; none granted is refused
    // and the token revoked.
    let none_granted = !req.discovered.scopes.is_empty()
        && missing_from(req, &grant).is_some_and(|m| m.len() == req.discovered.scopes.len());
    if none_granted {
        revoke_unused(net, relay, req, &grant);
        return Err(McpAuthError::ScopesDenied);
    }
    seal_grant(vault, connector_id, req, grant)
}

/// The whole connect: flag, discovery, registration, browser, callback, exchange, seal.
/// Blocking; the caller runs it off the main thread.
pub(crate) fn connect(
    net: &dyn McpHttp,
    vault: &CustodyVault,
    relay: Option<&dyn RelayTransport>,
    connector_id: &str,
    endpoint: &url::Url,
    open: impl Fn(&str) -> Result<(), McpAuthError>,
) -> Result<Connected, McpAuthError> {
    if !super::mcp_remote_auth_enabled() {
        return Err(McpAuthError::Disabled);
    }
    store::token_slot(connector_id)?;
    if !vault.is_unlocked() {
        return Err(McpAuthError::Vault);
    }
    let discovered = super::discovery::discover(net, endpoint, &registration::fixed_metadata)?;
    let client = registration::resolve(net, vault, &discovered.auth_server)?;
    if matches!(client, Client::Relay { .. }) && relay.is_none() {
        return Err(McpAuthError::NotSignedIn);
    }
    let listener =
        crate::connections::ConnectionListener::bind().map_err(|_| McpAuthError::PortInUse)?;
    let req = begin(discovered, client);
    open(&req.url)?;
    let cb = match listener.wait_for_callback(crate::connections::CALLBACK_TIMEOUT, &req.state) {
        Ok(p) => Callback {
            code: Some(Zeroizing::new(p.code)),
            state: Some(p.state),
            iss: p.iss,
            error: None,
        },
        Err(ConnError::Declined) => return Err(McpAuthError::Declined),
        Err(ConnError::Timeout) => return Err(McpAuthError::Timeout),
        Err(ConnError::StateMismatch) => return Err(McpAuthError::StateMismatch),
        Err(_) => return Err(McpAuthError::Network),
    };
    let out = complete(net, vault, relay, connector_id, &req, cb);
    let line = match &out {
        Ok(c) => format!(
            "mcp_auth: {connector_id} connected via {}; token sealed",
            c.client_kind
        ),
        Err(e) => format!(
            "mcp_auth: {connector_id} connect via {:?} failed: {e}",
            req.client
        ),
    };
    eprintln!("{}", crate::secret_shapes::redact(&line));
    out
}
