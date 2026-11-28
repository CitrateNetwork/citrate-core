// L6-WP06 test fixtures. Rule 1: these are TEST seams only (a scripted network and relay, a
// headless vault over an in-memory keyring), never evidence that a real provider works. The
// metadata documents below are rebuilt from the probe lines recorded in the L6-WP07 provider
// matrix (evidence/L6-WP07_MCP_PROVIDER_MATRIX_2026-11-28.md, probes 2026-11-28T04:35Z-04:40Z):
// the challenge headers, PRM documents, issuers, registration endpoints, CIMD flags, PKCE methods,
// token auth methods, `iss` support and revocation endpoints are as recorded (scope lists the
// matrix cut are cut here too). Where the matrix recorded no authorize or token URL, the fixture
// uses a path on the issuer's host; those two values are fixture data, not probe evidence.
// Real-provider runtime proof is pending (needs QA accounts, L6-WP10).

use super::http::{McpHttp, Reply, Request};
use super::McpAuthError;
use crate::connections::oauth_client::relay::{RelayError, RelayOp, RelayTransport};
use crate::custody::{CustodyError, CustodyVault, Keyring};
use std::collections::{HashMap, VecDeque};
use std::net::{IpAddr, Ipv4Addr};
use std::sync::{Arc, Mutex};
use zeroize::Zeroizing;

pub(super) const PASS: &[u8] = b"correct horse battery staple";

/// Every fixture credential contains this, so one substring check covers them all.
pub(super) const MARK: &str = "FIXTURE-WP06";

#[derive(Default)]
struct MemKeyring {
    store: Mutex<HashMap<String, Vec<u8>>>,
}
impl Keyring for MemKeyring {
    fn get(&self, a: &str) -> Result<Option<Vec<u8>>, CustodyError> {
        Ok(self.store.lock().unwrap().get(a).cloned())
    }
    fn set(&self, a: &str, s: &[u8]) -> Result<(), CustodyError> {
        self.store.lock().unwrap().insert(a.to_string(), s.to_vec());
        Ok(())
    }
    fn delete(&self, a: &str) -> Result<(), CustodyError> {
        self.store.lock().unwrap().remove(a);
        Ok(())
    }
}

/// A fresh, unlocked vault in its own file (one member profile).
pub(super) fn vault() -> CustodyVault {
    let n: u64 = rand::random();
    let p = std::env::temp_dir().join(format!("citrate-core-wp06-{}-{n:016x}", std::process::id()));
    let v = CustodyVault::new(Box::new(MemKeyring::default()), p, 30);
    v.init(&mut PASS.to_vec()).unwrap();
    v.unlock(&mut PASS.to_vec()).unwrap();
    v
}

/// One recorded request.
#[derive(Debug, Clone)]
pub(super) struct Sent {
    pub url: String,
    pub kind: &'static str,
    pub form: Vec<(String, String)>,
    pub basic: Option<(String, String)>,
    pub json: Option<String>,
}

impl Sent {
    pub fn field(&self, k: &str) -> Option<&str> {
        self.form
            .iter()
            .find(|(a, _)| a == k)
            .map(|(_, v)| v.as_str())
    }
}

type Answer = (u16, Option<String>, String);

/// A scripted network: answers by URL (a queue per URL; the last answer repeats), a DNS table,
/// and a record of every request. Unknown URLs answer 404.
#[derive(Default)]
pub(super) struct Net {
    pub answers: Mutex<HashMap<String, VecDeque<Answer>>>,
    pub dns: Mutex<HashMap<String, Vec<IpAddr>>>,
    pub sent: Mutex<Vec<Sent>>,
    /// Delay each token-endpoint POST (concurrency tests).
    pub post_delay_ms: Mutex<u64>,
}

pub(super) const PUBLIC_IP: IpAddr = IpAddr::V4(Ipv4Addr::new(93, 184, 215, 14));

impl Net {
    pub fn new() -> Arc<Net> {
        Arc::new(Net::default())
    }
    pub fn on(&self, url: &str, status: u16, www: Option<&str>, body: &str) {
        self.answers
            .lock()
            .unwrap()
            .entry(url.to_string())
            .or_default()
            .push_back((status, www.map(str::to_string), body.to_string()));
    }
    pub fn dns(&self, host: &str, ips: Vec<IpAddr>) {
        self.dns.lock().unwrap().insert(host.to_string(), ips);
    }
    pub fn sent(&self) -> Vec<Sent> {
        self.sent.lock().unwrap().clone()
    }
    pub fn sent_to(&self, url: &str) -> Vec<Sent> {
        self.sent().into_iter().filter(|s| s.url == url).collect()
    }
}

/// The seam over a shared [`Net`].
pub(super) struct NetHttp(pub Arc<Net>);

impl McpHttp for NetHttp {
    fn resolve(&self, host: &str, _port: u16) -> Result<Vec<IpAddr>, McpAuthError> {
        Ok(self
            .0
            .dns
            .lock()
            .unwrap()
            .get(host)
            .cloned()
            .unwrap_or_else(|| vec![PUBLIC_IP]))
    }
    fn send(
        &self,
        url: &url::Url,
        pinned: &[IpAddr],
        req: Request<'_>,
    ) -> Result<Reply, McpAuthError> {
        assert!(!pinned.is_empty(), "a request is always pinned");
        let mut s = Sent {
            url: url.to_string(),
            kind: "get",
            form: vec![],
            basic: None,
            json: None,
        };
        match req {
            Request::Get => {}
            Request::Probe => s.kind = "probe",
            Request::PostJson(b) => {
                s.kind = "json";
                s.json = Some(b.to_string());
            }
            Request::PostForm { form, basic } => {
                s.kind = "form";
                s.form = form
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect();
                s.basic = basic.map(|(a, b)| (a.to_string(), b.to_string()));
                let ms = *self.0.post_delay_ms.lock().unwrap();
                if ms > 0 {
                    std::thread::sleep(std::time::Duration::from_millis(ms));
                }
            }
        }
        self.0.sent.lock().unwrap().push(s);
        let mut answers = self.0.answers.lock().unwrap();
        let (status, www, body) = match answers.get_mut(url.as_str()) {
            Some(q) if q.len() > 1 => q.pop_front().unwrap(),
            Some(q) if q.len() == 1 => q.front().cloned().unwrap(),
            _ => (404, None, "not found".to_string()),
        };
        Ok(Reply {
            status,
            www_authenticate: www,
            body: Zeroizing::new(body),
        })
    }
}

/// A scripted relay.
#[derive(Default)]
pub(super) struct Relay {
    pub calls: Mutex<Vec<(String, RelayOp, String)>>,
    pub answers: Mutex<VecDeque<Result<String, RelayError>>>,
}
impl Relay {
    pub fn answering(a: Vec<Result<String, RelayError>>) -> Relay {
        Relay {
            calls: Mutex::new(vec![]),
            answers: Mutex::new(a.into()),
        }
    }
    pub fn calls(&self) -> Vec<(String, RelayOp, String)> {
        self.calls.lock().unwrap().clone()
    }
}
impl RelayTransport for Relay {
    fn post(
        &self,
        provider: &str,
        op: RelayOp,
        body: &str,
    ) -> Result<Zeroizing<String>, RelayError> {
        self.calls
            .lock()
            .unwrap()
            .push((provider.to_string(), op, body.to_string()));
        self.answers
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(Err(RelayError::Network))
            .map(Zeroizing::new)
    }
}

/// A token answer whose tokens carry [`MARK`].
pub(super) fn token_json(
    tag: &str,
    refresh: bool,
    expires_in: Option<u64>,
    scope: Option<&str>,
) -> String {
    let mut v = serde_json::json!({
        "access_token": format!("at-{MARK}-{tag}"),
        "token_type": "Bearer",
    });
    if refresh {
        v["refresh_token"] = format!("rt-{MARK}-{tag}").into();
    }
    if let Some(e) = expires_in {
        v["expires_in"] = e.into();
    }
    if let Some(s) = scope {
        v["scope"] = s.into();
    }
    v.to_string()
}

// ----------------------------------------------------------------- matrix-derived metadata

pub(super) const LINEAR_MCP: &str = "https://mcp.linear.app/mcp";
pub(super) const LINEAR_WWW: &str = r#"Bearer realm="OAuth", resource_metadata="https://mcp.linear.app/.well-known/oauth-protected-resource/mcp", scope="read write""#;
pub(super) const LINEAR_PRM_URL: &str =
    "https://mcp.linear.app/.well-known/oauth-protected-resource/mcp";
pub(super) const LINEAR_PRM: &str = r#"{"resource":"https://mcp.linear.app/mcp","authorization_servers":["https://mcp.linear.app"],"scopes_supported":["read","write"]}"#;
pub(super) const LINEAR_AS_URL: &str =
    "https://mcp.linear.app/.well-known/oauth-authorization-server";
pub(super) const LINEAR_AS: &str = r#"{"issuer":"https://mcp.linear.app","authorization_endpoint":"https://mcp.linear.app/authorize","token_endpoint":"https://mcp.linear.app/token","registration_endpoint":"https://mcp.linear.app/register","client_id_metadata_document_supported":true,"code_challenge_methods_supported":["S256"],"token_endpoint_auth_methods_supported":["client_secret_basic","client_secret_post","none"],"scopes_supported":["read","write","openid","email"],"authorization_response_iss_parameter_supported":true}"#;

pub(super) const NOTION_MCP: &str = "https://mcp.notion.com/mcp";
pub(super) const NOTION_WWW: &str = r#"Bearer realm="OAuth", resource_metadata="https://mcp.notion.com/.well-known/oauth-protected-resource/mcp", scope="default""#;
pub(super) const NOTION_PRM_URL: &str =
    "https://mcp.notion.com/.well-known/oauth-protected-resource/mcp";
pub(super) const NOTION_PRM: &str = r#"{"resource":"https://mcp.notion.com/mcp","authorization_servers":["https://mcp.notion.com"],"scopes_supported":["default"],"bearer_methods_supported":["header"],"resource_name":"Notion MCP (Beta)"}"#;
pub(super) const NOTION_AS_URL: &str =
    "https://mcp.notion.com/.well-known/oauth-authorization-server";
pub(super) const NOTION_AS: &str = r#"{"issuer":"https://mcp.notion.com","authorization_endpoint":"https://mcp.notion.com/authorize","token_endpoint":"https://mcp.notion.com/token","registration_endpoint":"https://mcp.notion.com/register","client_id_metadata_document_supported":true,"code_challenge_methods_supported":["S256"],"grant_types_supported":["authorization_code","refresh_token","urn:ietf:params:oauth:grant-type:jwt-bearer"],"token_endpoint_auth_methods_supported":["client_secret_basic","client_secret_post","none","private_key_jwt"],"authorization_response_iss_parameter_supported":true,"revocation_endpoint":"https://mcp.notion.com/token"}"#;

pub(super) const VERCEL_MCP: &str = "https://mcp.vercel.com";
pub(super) const VERCEL_WWW: &str = r#"Bearer error="invalid_token", error_description="No authorization provided", resource_metadata="https://mcp.vercel.com/.well-known/oauth-protected-resource""#;
pub(super) const VERCEL_PRM_URL: &str =
    "https://mcp.vercel.com/.well-known/oauth-protected-resource";
pub(super) const VERCEL_PRM: &str = r#"{"resource":"https://mcp.vercel.com/","authorization_servers":["https://vercel.com"],"scopes_supported":["openid"]}"#;
pub(super) const VERCEL_AS_URL: &str = "https://vercel.com/.well-known/oauth-authorization-server";
pub(super) const VERCEL_AS: &str = r#"{"issuer":"https://vercel.com","authorization_endpoint":"https://vercel.com/oauth/authorize","token_endpoint":"https://api.vercel.com/login/oauth/token","registration_endpoint":"https://api.vercel.com/login/oauth/register","client_id_metadata_document_supported":true,"code_challenge_methods_supported":["S256"],"token_endpoint_auth_methods_supported":["none","client_secret_basic","client_secret_post"],"authorization_response_iss_parameter_supported":true}"#;

pub(super) const ATLASSIAN_MCP: &str = "https://mcp.atlassian.com/v2/mcp";
pub(super) const ATLASSIAN_ISSUER: &str =
    "https://auth.atlassian.com/VCeDsk8ZHncYF1g234fKtc4lNipbBhu3";
pub(super) const ATLASSIAN_WWW: &str = r#"Bearer resource_metadata="https://mcp.atlassian.com/.well-known/oauth-protected-resource/v2/mcp""#;
pub(super) const ATLASSIAN_PRM_URL: &str =
    "https://mcp.atlassian.com/.well-known/oauth-protected-resource/v2/mcp";
pub(super) const ATLASSIAN_PRM: &str = r#"{"resource":"https://mcp.atlassian.com/v2/mcp","authorization_servers":["https://auth.atlassian.com/VCeDsk8ZHncYF1g234fKtc4lNipbBhu3"],"scopes_supported":["read:me","read:account","offline_access","email","read:jira:agent-interface","search:jira:agent-interface","read:confluence:agent-interface"]}"#;
pub(super) const ATLASSIAN_AS_URL: &str = "https://auth.atlassian.com/.well-known/oauth-authorization-server/VCeDsk8ZHncYF1g234fKtc4lNipbBhu3";
pub(super) const ATLASSIAN_AS: &str = r#"{"issuer":"https://auth.atlassian.com/VCeDsk8ZHncYF1g234fKtc4lNipbBhu3","authorization_endpoint":"https://auth.atlassian.com/VCeDsk8ZHncYF1g234fKtc4lNipbBhu3/authorize","token_endpoint":"https://auth.atlassian.com/VCeDsk8ZHncYF1g234fKtc4lNipbBhu3/token","registration_endpoint":"https://auth.atlassian.com/VCeDsk8ZHncYF1g234fKtc4lNipbBhu3/dcr/register","client_id_metadata_document_supported":true,"code_challenge_methods_supported":["S256"],"token_endpoint_auth_methods_supported":["none","client_secret_post"],"grant_types_supported":["authorization_code","refresh_token","urn:ietf:params:oauth:grant-type:token-exchange"],"revocation_endpoint":"https://auth.atlassian.com/oauth/revoke"}"#;

pub(super) const STRIPE_MCP: &str = "https://mcp.stripe.com";
pub(super) const STRIPE_WWW: &str =
    r#"Bearer resource_metadata="https://mcp.stripe.com/.well-known/oauth-protected-resource""#;
pub(super) const STRIPE_PRM_URL: &str =
    "https://mcp.stripe.com/.well-known/oauth-protected-resource";
pub(super) const STRIPE_PRM: &str = r#"{"resource":"https://mcp.stripe.com","scopes_supported":["mcp"],"authorization_servers":["https://access.stripe.com/mcp"]}"#;
pub(super) const STRIPE_AS_URL: &str =
    "https://access.stripe.com/.well-known/oauth-authorization-server/mcp";
pub(super) const STRIPE_AS: &str = r#"{"issuer":"https://access.stripe.com/mcp","authorization_endpoint":"https://access.stripe.com/mcp/oauth2/authorize","token_endpoint":"https://access.stripe.com/mcp/oauth2/token","registration_endpoint":"https://access.stripe.com/mcp/oauth2/register","code_challenge_methods_supported":["S256"],"token_endpoint_auth_methods_supported":["none"],"revocation_endpoint":"https://access.stripe.com/mcp/oauth2/revoke"}"#;

pub(super) const HF_MCP: &str = "https://huggingface.co/mcp";
pub(super) const HF_PRM_URL: &str =
    "https://huggingface.co/.well-known/oauth-protected-resource/mcp";
pub(super) const HF_PRM: &str = r#"{"resource":"https://huggingface.co/mcp","authorization_servers":["https://huggingface.co"],"scopes_supported":["openid","profile","read-mcp","read-repos","jobs","contribute-repos","inference-api"]}"#;
pub(super) const HF_AS_URL: &str = "https://huggingface.co/.well-known/oauth-authorization-server";
pub(super) const HF_AS: &str = r#"{"issuer":"https://huggingface.co","authorization_endpoint":"https://huggingface.co/oauth/authorize","token_endpoint":"https://huggingface.co/oauth/token","registration_endpoint":"https://huggingface.co/oauth/register","client_id_metadata_document_supported":true,"code_challenge_methods_supported":["S256"],"token_endpoint_auth_methods_supported":["client_secret_basic","client_secret_post"]}"#;

pub(super) const SUPABASE_AS: &str = r#"{"issuer":"https://api.supabase.com","authorization_endpoint":"https://api.supabase.com/v1/oauth/authorize","token_endpoint":"https://api.supabase.com/v1/oauth/token","registration_endpoint":"https://api.supabase.com/platform/oauth/apps/register","code_challenge_methods_supported":["S256","plain"],"token_endpoint_auth_methods_supported":["client_secret_basic","client_secret_post"],"authorization_response_iss_parameter_supported":true}"#;

pub(super) const SLACK_MCP: &str = "https://mcp.slack.com/mcp";
pub(super) const SLACK_WWW: &str =
    r#"Bearer resource_metadata="https://mcp.slack.com/.well-known/oauth-protected-resource""#;
pub(super) const SLACK_PRM_URL: &str = "https://mcp.slack.com/.well-known/oauth-protected-resource";
pub(super) const SLACK_PRM: &str = r#"{"resource":"https://mcp.slack.com","authorization_servers":["https://mcp.slack.com"],"bearer_methods_supported":["header","form"],"scopes_supported":["channels:history","channels:read","search:read.public","users:read","users:read.email"],"resource_name":"Slack API"}"#;
pub(super) const SLACK_AS_URL: &str =
    "https://mcp.slack.com/.well-known/oauth-authorization-server";
pub(super) const SLACK_AS: &str = r#"{"issuer":"https://mcp.slack.com","authorization_endpoint":"https://slack.com/oauth/v2_user/authorize","token_endpoint":"https://slack.com/api/oauth.v2.user.access","code_challenge_methods_supported":["S256"],"grant_types_supported":["authorization_code","refresh_token","urn:ietf:params:oauth:grant-type:jwt-bearer"],"token_endpoint_auth_methods_supported":["client_secret_post"]}"#;

pub(super) const GITHUB_MCP: &str = "https://api.githubcopilot.com/mcp/";
pub(super) const GITHUB_WWW: &str = r#"Bearer error="invalid_request", error_description="No access token was provided in this request", resource_metadata="https://api.githubcopilot.com/.well-known/oauth-protected-resource/mcp/""#;
pub(super) const GITHUB_PRM_URL: &str =
    "https://api.githubcopilot.com/.well-known/oauth-protected-resource/mcp/";
pub(super) const GITHUB_PRM: &str = r#"{"resource":"https://api.githubcopilot.com/mcp","authorization_servers":["https://github.com/login/oauth"],"scopes_supported":["repo","read:org","read:user","user:email","read:packages","write:packages","read:project","project","gist","notifications"],"bearer_methods_supported":["header"],"resource_name":"GitHub MCP Server"}"#;

pub(super) const GOOGLE_AS_URL: &str =
    "https://accounts.google.com/.well-known/oauth-authorization-server";
pub(super) const GOOGLE_AS: &str = r#"{"issuer":"https://accounts.google.com","authorization_endpoint":"https://accounts.google.com/o/oauth2/v2/auth","token_endpoint":"https://oauth2.googleapis.com/token","code_challenge_methods_supported":["plain","S256"],"token_endpoint_auth_methods_supported":["client_secret_post","client_secret_basic"],"authorization_response_iss_parameter_supported":true,"revocation_endpoint":"https://oauth2.googleapis.com/revoke"}"#;

/// Script a server: the 401 probe, its PRM and its AS metadata.
pub(super) fn serve(
    net: &Net,
    mcp: &str,
    www: &str,
    prm_url: &str,
    prm: &str,
    as_url: &str,
    as_doc: &str,
) {
    net.on(mcp, 401, Some(www), "");
    net.on(prm_url, 200, None, prm);
    net.on(as_url, 200, None, as_doc);
}

pub(super) fn serve_linear(net: &Net) {
    serve(
        net,
        LINEAR_MCP,
        LINEAR_WWW,
        LINEAR_PRM_URL,
        LINEAR_PRM,
        LINEAR_AS_URL,
        LINEAR_AS,
    );
}

pub(super) fn url(s: &str) -> url::Url {
    url::Url::parse(s).unwrap()
}
