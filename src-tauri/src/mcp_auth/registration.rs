//! Client registration (L6-WP06 step 2). Order (lanes/L6 WP06, owner brief 2026-11-28):
//!
//! 1. CIMD, only when the authorization server advertises `client_id_metadata_document_supported`
//!    and accepts a public client at its token endpoint (`none`, or no list). The client id is
//!    [`super::CIMD_CLIENT_ID`]. Hugging Face advertises CIMD but lists only secret-based methods
//!    (matrix quirk 5), so it falls to DCR.
//! 2. Dynamic client registration (RFC 7591), only when `registration_endpoint` is present.
//!    Registered once per issuer, sealed in `connection-dcr-<issuer hash>`, reused after.
//! 3. A fixed pre-registered client whose secret is in the Citrate relay (rows below).
//!
//! The MCP specification 2025-11-25 lists pre-registered first; Citrate puts it last because a
//! fixed row costs an owner registration and a relay secret, and CIMD or DCR need neither. A row
//! is used only for an issuer that offers neither (matrix: Slack, GitHub, Google, Asana, HubSpot).

use super::discovery::{issuers_match, AsMetadata};
use super::http::{self, McpHttp, Request};
use super::{store, McpAuthError, CIMD_CLIENT_ID, LOOPBACK_REDIRECT};
use crate::custody::CustodyVault;
use crate::secret::SecretString;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

/// A fixed client: a provider whose authorization server offers no CIMD and no DCR. The secret
/// is in the relay deployment (`<PROVIDER>_CLIENT_SECRET`); only the public id is compiled in.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct FixedClient {
    /// The relay provider name (`/oauth/relay/v1/{provider}/..`).
    pub provider: &'static str,
    /// The issuer this row serves (compared with [`issuers_match`]).
    pub issuer: &'static str,
    pub client_id: Option<&'static str>,
    /// The redirect registered on the provider's app.
    pub redirect_uri: &'static str,
    /// Why `client_id` is `None`, naming the missing registration.
    pub reason: Option<&'static str>,
    /// Endpoints for an issuer that publishes no metadata (GitHub: matrix row, from GitHub's docs;
    /// PKCE S256 per GitHub's OAuth docs). `None` when the issuer publishes its metadata.
    pub static_endpoints: Option<(&'static str, &'static str)>,
}

/// The fixed rows (provider matrix 2026-11-28; lanes/L6 WP13 for GitHub).
pub(crate) const FIXED_CLIENTS: &[FixedClient] = &[
    FixedClient {
        provider: "github",
        issuer: "https://github.com/login/oauth",
        client_id: Some(crate::connections::oauth_client::GITHUB_CLIENT_ID),
        redirect_uri: LOOPBACK_REDIRECT,
        reason: None,
        static_endpoints: Some((
            "https://github.com/login/oauth/authorize",
            "https://github.com/login/oauth/access_token",
        )),
    },
    FixedClient {
        provider: "slack",
        issuer: "https://mcp.slack.com",
        client_id: None,
        redirect_uri: crate::connections::HOSTED_REDIRECT_URI,
        reason: Some(
            "the Slack app (A0CM5JALP3R) client id is not compiled in yet and its secret is not \
             in the relay (runbook card sl-core)",
        ),
        static_endpoints: None,
    },
    FixedClient {
        provider: "notion",
        issuer: "https://mcp.notion.com",
        client_id: None,
        redirect_uri: crate::connections::HOSTED_REDIRECT_URI,
        reason: Some(
            "the Notion public integration's client id is not registered for release builds \
             (Notion's MCP server offers CIMD, which is used instead)",
        ),
        static_endpoints: None,
    },
];

/// The fixed row for `issuer`, if any.
pub(crate) fn fixed_for(issuer: &str) -> Option<&'static FixedClient> {
    FIXED_CLIENTS
        .iter()
        .find(|r| issuers_match(r.issuer, issuer))
}

/// Discovery's fallback: metadata for a fixed-row issuer that publishes none.
pub(crate) fn fixed_metadata(issuer: &str) -> Option<AsMetadata> {
    let row = fixed_for(issuer)?;
    let (authorize, token) = row.static_endpoints?;
    Some(AsMetadata {
        issuer: row.issuer.to_string(),
        authorization_endpoint: http::parse_checked(authorize).ok()?,
        token_endpoint: http::parse_checked(token).ok()?,
        registration_endpoint: None,
        revocation_endpoint: None,
        cimd_supported: false,
        iss_parameter_supported: false,
        token_auth_methods: None,
    })
}

/// Which registration path the metadata allows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Choice {
    Cimd,
    Dcr,
    Fixed(&'static FixedClient),
}

/// Pick the path from the metadata alone (no network).
pub(crate) fn choose(meta: &AsMetadata) -> Result<Choice, McpAuthError> {
    let public_ok = meta
        .token_auth_methods
        .as_ref()
        .is_none_or(|m| m.iter().any(|x| x == "none"));
    if meta.cimd_supported && public_ok {
        return Ok(Choice::Cimd);
    }
    if meta.registration_endpoint.is_some() {
        return Ok(Choice::Dcr);
    }
    if meta.cimd_supported {
        return Ok(Choice::Cimd);
    }
    match fixed_for(&meta.issuer) {
        Some(row) if row.client_id.is_some() => Ok(Choice::Fixed(row)),
        Some(row) => Err(McpAuthError::NotConfigured(
            row.reason.unwrap_or("no client id"),
        )),
        None => Err(McpAuthError::NoRegistrationPath),
    }
}

/// How the flow authenticates as a client.
#[derive(Clone)]
pub(crate) enum Client {
    /// A public client (CIMD, or DCR with `none`).
    Public {
        kind: &'static str,
        client_id: String,
    },
    /// A DCR client the server made confidential; `basic` selects HTTP Basic over the form.
    Confidential {
        client_id: String,
        secret: SecretString,
        basic: bool,
    },
    /// A fixed client through the relay.
    Relay {
        provider: &'static str,
        client_id: &'static str,
        redirect_uri: &'static str,
    },
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.kind())
    }
}

impl Client {
    pub(crate) fn kind(&self) -> &'static str {
        match self {
            Client::Public { kind, .. } => kind,
            Client::Confidential { .. } => "dcr",
            Client::Relay { .. } => "relay",
        }
    }
    pub(crate) fn client_id(&self) -> &str {
        match self {
            Client::Public { client_id, .. } | Client::Confidential { client_id, .. } => client_id,
            Client::Relay { client_id, .. } => client_id,
        }
    }
    /// The exact redirect this client registered.
    pub(crate) fn redirect_uri(&self) -> &'static str {
        match self {
            Client::Relay { redirect_uri, .. } => redirect_uri,
            _ => LOOPBACK_REDIRECT,
        }
    }
}

/// The DCR record sealed per issuer.
#[derive(Serialize, Deserialize)]
pub(crate) struct DcrRecord {
    pub issuer: String,
    pub client_id: String,
    #[serde(
        default,
        serialize_with = "crate::secret::serialize_exposed_opt",
        skip_serializing_if = "Option::is_none"
    )]
    pub client_secret: Option<SecretString>,
    pub auth_method: String,
    pub registered_at: u64,
}

/// `connection-dcr-<first 16 bytes of sha256(issuer without trailing slash), hex>`.
pub(crate) fn dcr_slot(issuer: &str) -> String {
    let norm = issuer.strip_suffix('/').unwrap_or(issuer);
    let digest = Sha256::digest(norm.as_bytes());
    format!("connection-dcr-{}", hex::encode(&digest[..16]))
}

/// The client for `meta`: CIMD, a reused or new DCR registration, or a fixed relay row.
pub(crate) fn resolve(
    net: &dyn McpHttp,
    vault: &CustodyVault,
    meta: &AsMetadata,
) -> Result<Client, McpAuthError> {
    match choose(meta)? {
        Choice::Cimd => Ok(Client::Public {
            kind: "cimd",
            client_id: CIMD_CLIENT_ID.to_string(),
        }),
        Choice::Fixed(row) => Ok(Client::Relay {
            provider: row.provider,
            client_id: row
                .client_id
                .ok_or(McpAuthError::NotConfigured("no client id"))?,
            redirect_uri: row.redirect_uri,
        }),
        Choice::Dcr => {
            let rec = register_or_reuse(net, vault, meta)?;
            Ok(dcr_client(rec))
        }
    }
}

/// The client for a sealed DCR record.
pub(crate) fn dcr_client(rec: DcrRecord) -> Client {
    match rec.client_secret {
        Some(secret) if !secret.is_blank() => Client::Confidential {
            client_id: rec.client_id,
            secret,
            basic: rec.auth_method == "client_secret_basic",
        },
        _ => Client::Public {
            kind: "dcr",
            client_id: rec.client_id,
        },
    }
}

/// The token endpoint auth method to ask for: `none` when allowed, else `client_secret_post`,
/// else `client_secret_basic` (RFC 8414 default when the list is absent is `client_secret_basic`,
/// but a native client asks for `none` first and lets the server say otherwise).
fn wanted_auth_method(meta: &AsMetadata) -> &'static str {
    match &meta.token_auth_methods {
        None => "none",
        Some(m) if m.iter().any(|x| x == "none") => "none",
        Some(m) if m.iter().any(|x| x == "client_secret_post") => "client_secret_post",
        Some(_) => "client_secret_basic",
    }
}

/// The sealed registration for `meta.issuer`, or a new one (registered once, then reused).
pub(crate) fn register_or_reuse(
    net: &dyn McpHttp,
    vault: &CustodyVault,
    meta: &AsMetadata,
) -> Result<DcrRecord, McpAuthError> {
    let slot = dcr_slot(&meta.issuer);
    if !vault.is_unlocked() {
        return Err(McpAuthError::Vault);
    }
    if let Ok(bytes) = vault.custody_get(&slot) {
        if let Ok(rec) = serde_json::from_slice::<DcrRecord>(&bytes) {
            if issuers_match(&rec.issuer, &meta.issuer) && !rec.client_id.is_empty() {
                return Ok(rec);
            }
        }
    }
    let endpoint = meta
        .registration_endpoint
        .as_ref()
        .ok_or(McpAuthError::NoRegistrationPath)?;
    let method = wanted_auth_method(meta);
    let body = serde_json::json!({
        "client_name": "Citrate Core",
        "client_uri": "https://citrate.ai",
        "redirect_uris": [LOOPBACK_REDIRECT],
        "grant_types": ["authorization_code", "refresh_token"],
        "response_types": ["code"],
        "token_endpoint_auth_method": method,
        "application_type": "native",
    })
    .to_string();
    let reply = http::send(net, endpoint, Request::PostJson(&body))?;
    if !(200..300).contains(&reply.status) {
        return Err(McpAuthError::RegistrationRejected(reply.status));
    }
    #[derive(Deserialize)]
    struct Answer {
        client_id: String,
        #[serde(default)]
        client_secret: Option<SecretString>,
        #[serde(default)]
        token_endpoint_auth_method: Option<String>,
    }
    let ans: Answer =
        serde_json::from_str(&reply.body).map_err(|_| McpAuthError::BadMetadata("registration"))?;
    if ans.client_id.is_empty() {
        return Err(McpAuthError::BadMetadata("client_id"));
    }
    let rec = DcrRecord {
        issuer: meta.issuer.clone(),
        client_id: ans.client_id,
        client_secret: ans.client_secret.filter(|s| !s.is_blank()),
        auth_method: ans
            .token_endpoint_auth_method
            .unwrap_or_else(|| method.to_string()),
        registered_at: crate::connections::now_unix(),
    };
    let bytes = serde_json::to_vec(&rec).map_err(|_| McpAuthError::Vault)?;
    store::seal(vault, &slot, bytes)?;
    Ok(rec)
}

/// The sealed DCR record for `issuer` (refresh and revoke of a DCR connection need it).
pub(crate) fn sealed_dcr(vault: &CustodyVault, issuer: &str) -> Option<DcrRecord> {
    let bytes = vault.custody_get(&dcr_slot(issuer)).ok()?;
    serde_json::from_slice(&bytes).ok()
}
