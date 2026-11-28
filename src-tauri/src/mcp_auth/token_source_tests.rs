// L6-WP06 token source tests (lanes/L6 WP06 RED list: bearer_for_other_resource_is_mismatch,
// on_unauthorized_emits_auth_required_once, vault_locked_returns_vault_locked_not_network; owner
// brief: refresh rotation, concurrent refresh single-flight, revoke and reconnect, cross-profile
// isolation, redaction) and the GitHub part of L6-WP13 (migration).

use super::fixtures::*;
use super::flow::Connected;
use super::migration::*;
use super::store::{self, ClientRef, TokenRecord};
use super::token_source::*;
use super::McpAuthError;
use crate::connections::{now_unix, slot, Service};
use crate::custody::CustodyVault;
use crate::secret::SecretString;
use std::sync::{Arc, Mutex};

const LINEAR_TOKEN: &str = "https://mcp.linear.app/token";
const NOTION_TOKEN: &str = "https://mcp.notion.com/token";

type Notices = Arc<Mutex<Vec<(String, String)>>>;

fn tokens(net: &Arc<Net>) -> (McpTokens, Notices) {
    let seen: Notices = Arc::default();
    let s = seen.clone();
    let sink: AuthRequiredSink = Box::new(move |id: &str, why: &str| {
        s.lock().unwrap().push((id.to_string(), why.to_string()));
    });
    (McpTokens::new(Box::new(NetHttp(net.clone())), sink), seen)
}

/// Seal a CIMD record for Linear (or another connector) directly.
fn seed(
    v: &CustodyVault,
    id: &str,
    resource: &str,
    token_endpoint: &str,
    tag: &str,
    expires_at: Option<u64>,
    refresh: bool,
) {
    let rec = TokenRecord {
        v: store::RECORD_V,
        connector_id: id.to_string(),
        resource: resource.to_string(),
        issuer: "https://mcp.linear.app".into(),
        token_endpoint: token_endpoint.into(),
        revocation_endpoint: None,
        client: ClientRef {
            kind: "cimd".into(),
            client_id: super::CIMD_CLIENT_ID.into(),
            provider: None,
            redirect_uri: super::LOOPBACK_REDIRECT.into(),
        },
        access_token: SecretString::new(format!("at-{MARK}-{tag}")),
        refresh_token: refresh.then(|| SecretString::new(format!("rt-{MARK}-{tag}"))),
        scope: Some("read".into()),
        expires_at,
        connected_at: 1,
        needs_refresh: false,
    };
    store::write(v, &rec).unwrap();
}

fn seed_linear(v: &CustodyVault, expires_at: Option<u64>, refresh: bool) {
    seed(
        v,
        "linear",
        "https://mcp.linear.app/mcp",
        LINEAR_TOKEN,
        "1",
        expires_at,
        refresh,
    );
}

fn linear_url() -> url::Url {
    url(LINEAR_MCP)
}

#[test]
fn bearer_for_the_bound_resource_and_only_it() {
    let net = Net::new();
    let (t, _) = tokens(&net);
    let v = vault();
    seed_linear(&v, None, false);
    assert_eq!(
        t.bearer(&v, None, "linear", &linear_url())
            .unwrap()
            .as_str(),
        format!("at-{MARK}-1")
    );
    assert!(t
        .bearer(
            &v,
            None,
            "linear",
            &url("https://mcp.linear.app/mcp/readonly")
        )
        .is_ok());
    for other in [
        "https://mcp.notion.com/mcp",
        "https://mcp.linear.app/other",
        "https://mcp.linear.app/mcpx",
        "https://evil.example.com/mcp",
    ] {
        assert_eq!(
            t.bearer(&v, None, "linear", &url(other)).unwrap_err(),
            ConnectorAuthError::ResourceMismatch,
            "{other}"
        );
    }
    assert!(
        net.sent().is_empty(),
        "nothing is sent while choosing a bearer"
    );
}

#[test]
fn vault_locked_returns_vault_locked_not_network() {
    let net = Net::new();
    let (t, _) = tokens(&net);
    let v = vault();
    seed_linear(&v, None, false);
    v.lock();
    assert_eq!(
        t.bearer(&v, None, "linear", &linear_url()).unwrap_err(),
        ConnectorAuthError::VaultLocked
    );
    let fresh = vault();
    assert_eq!(
        t.bearer(&fresh, None, "linear", &linear_url()).unwrap_err(),
        ConnectorAuthError::NotConnected
    );
}

#[test]
fn tokens_are_isolated_per_profile_and_per_connector() {
    let net = Net::new();
    let (t, _) = tokens(&net);
    let alice = vault();
    let bob = vault();
    seed_linear(&alice, None, false);
    assert_eq!(
        t.bearer(&bob, None, "linear", &linear_url()).unwrap_err(),
        ConnectorAuthError::NotConnected
    );
    // Another connector id never reads this slot, even asking for the same resource.
    assert_eq!(
        t.bearer(&alice, None, "linear-2", &linear_url())
            .unwrap_err(),
        ConnectorAuthError::NotConnected
    );
    // A record copied into another connector's slot is refused (bound to its connector id).
    let bytes = alice.custody_get("connection-mcp-linear").unwrap();
    store::seal(&alice, "connection-mcp-notion", bytes.to_vec()).unwrap();
    assert_eq!(
        t.bearer(&alice, None, "notion", &linear_url()).unwrap_err(),
        ConnectorAuthError::NotConnected
    );
    // Ids that could escape the namespace are refused; the slots are backend-reserved.
    let long = "a".repeat(65);
    for bad in ["", "../oidc-refresh", "Linear", "-x", long.as_str()] {
        assert!(store::token_slot(bad).is_err(), "{bad}");
    }
    assert!(citrate_core_kit::custody::is_backend_reserved_slot(
        &store::token_slot("linear").unwrap()
    ));
    assert_eq!(
        store::seal(&alice, "oidc-refresh", vec![1]).unwrap_err(),
        McpAuthError::Vault
    );
}

#[test]
fn refresh_rotates_and_never_reuses_the_old_refresh_token() {
    let net = Net::new();
    let (t, _) = tokens(&net);
    let v = vault();
    seed_linear(&v, Some(now_unix().saturating_sub(5)), true);
    net.on(
        LINEAR_TOKEN,
        200,
        None,
        &token_json("2", true, Some(1), None),
    );
    net.on(
        LINEAR_TOKEN,
        200,
        None,
        &token_json("3", true, Some(3600), None),
    );
    // First use: refresh with rt-1, get at-2 / rt-2 (expiring at once, inside the skew).
    let b = t.bearer(&v, None, "linear", &linear_url());
    assert_eq!(
        b.unwrap_err(),
        ConnectorAuthError::ReauthRequired,
        "refreshed once, still not usable: no loop"
    );
    let rec = store::read(&v, "linear").unwrap();
    assert_eq!(
        rec.refresh_token.as_ref().unwrap().expose(),
        format!("rt-{MARK}-2")
    );
    // Next use refreshes with rt-2, never rt-1.
    assert_eq!(
        t.bearer(&v, None, "linear", &linear_url())
            .unwrap()
            .as_str(),
        format!("at-{MARK}-3")
    );
    let posts = net.sent_to(LINEAR_TOKEN);
    assert_eq!(posts.len(), 2);
    assert_eq!(
        posts[0].field("refresh_token"),
        Some(format!("rt-{MARK}-1").as_str())
    );
    assert_eq!(
        posts[1].field("refresh_token"),
        Some(format!("rt-{MARK}-2").as_str())
    );
    for p in &posts {
        assert_eq!(p.field("grant_type"), Some("refresh_token"));
        assert_eq!(p.field("resource"), Some("https://mcp.linear.app/mcp"));
        assert_eq!(p.field("client_id"), Some(super::CIMD_CLIENT_ID));
    }
    assert_eq!(
        store::read(&v, "linear").unwrap().connected_at,
        1,
        "first connect time kept"
    );
}

#[test]
fn a_refresh_without_rotation_keeps_the_sealed_refresh_token() {
    let net = Net::new();
    let (t, _) = tokens(&net);
    let v = vault();
    seed_linear(&v, Some(0), true);
    net.on(
        LINEAR_TOKEN,
        200,
        None,
        &token_json("2", false, Some(3600), None),
    );
    assert!(t.bearer(&v, None, "linear", &linear_url()).is_ok());
    let rec = store::read(&v, "linear").unwrap();
    assert_eq!(rec.refresh_token.unwrap().expose(), format!("rt-{MARK}-1"));
}

#[test]
fn concurrent_refreshes_are_single_flight() {
    let net = Net::new();
    *net.post_delay_ms.lock().unwrap() = 150;
    net.on(
        LINEAR_TOKEN,
        200,
        None,
        &token_json("2", true, Some(3600), None),
    );
    let (t, _) = tokens(&net);
    let v = vault();
    seed_linear(&v, Some(0), true);
    let t = Arc::new(t);
    let v = Arc::new(v);
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let (t, v) = (t.clone(), v.clone());
            std::thread::spawn(move || {
                t.bearer(&v, None, "linear", &linear_url())
                    .map(|z| z.to_string())
            })
        })
        .collect();
    for h in handles {
        assert_eq!(h.join().unwrap().unwrap(), format!("at-{MARK}-2"));
    }
    assert_eq!(
        net.sent_to(LINEAR_TOKEN).len(),
        1,
        "one refresh for eight callers"
    );
}

#[test]
fn a_rejected_refresh_asks_the_member_to_reconnect_once() {
    let net = Net::new();
    net.on(LINEAR_TOKEN, 400, None, r#"{"error":"invalid_grant"}"#);
    let (t, seen) = tokens(&net);
    let v = vault();
    seed_linear(&v, Some(0), true);
    for _ in 0..3 {
        assert_eq!(
            t.bearer(&v, None, "linear", &linear_url()).unwrap_err(),
            ConnectorAuthError::ReauthRequired
        );
    }
    assert_eq!(
        *seen.lock().unwrap(),
        vec![("linear".to_string(), "refresh_rejected".to_string())]
    );
    // An expired token with nothing to refresh with is `Expired`.
    let v2 = vault();
    seed_linear(&v2, Some(0), false);
    assert_eq!(
        t.bearer(&v2, None, "linear", &linear_url()).unwrap_err(),
        ConnectorAuthError::Expired
    );
}

#[test]
fn a_network_failure_during_refresh_is_network_not_reauth() {
    struct Down;
    impl super::http::McpHttp for Down {
        fn resolve(&self, _: &str, _: u16) -> Result<Vec<std::net::IpAddr>, McpAuthError> {
            Err(McpAuthError::Network)
        }
        fn send(
            &self,
            _: &url::Url,
            _: &[std::net::IpAddr],
            _: super::http::Request<'_>,
        ) -> Result<super::http::Reply, McpAuthError> {
            unreachable!()
        }
    }
    let t = McpTokens::new(
        Box::new(Down),
        Box::new(|_: &str, _: &str| panic!("no notice")),
    );
    let v = vault();
    seed_linear(&v, Some(0), true);
    assert_eq!(
        t.bearer(&v, None, "linear", &linear_url()).unwrap_err(),
        ConnectorAuthError::Network
    );
    assert!(
        store::read(&v, "linear").unwrap().refresh_token.is_some(),
        "nothing lost"
    );
}

#[test]
fn on_unauthorized_emits_auth_required_once() {
    let net = Net::new();
    let (t, seen) = tokens(&net);
    let v = vault();
    seed_linear(&v, None, false);
    let www = Some(r#"Bearer error="invalid_token""#);
    t.unauthorized(&v, "linear", www);
    t.unauthorized(&v, "linear", www);
    assert_eq!(
        *seen.lock().unwrap(),
        vec![("linear".to_string(), "invalid_token".to_string())]
    );
    assert_eq!(
        t.bearer(&v, None, "linear", &linear_url()).unwrap_err(),
        ConnectorAuthError::ReauthRequired
    );
    // insufficient_scope needs consent for more scope (step-up), not a refresh.
    let v2 = vault();
    seed(
        &v2,
        "notion",
        "https://mcp.notion.com/mcp",
        NOTION_TOKEN,
        "n",
        None,
        true,
    );
    t.unauthorized(
        &v2,
        "notion",
        Some(r#"Bearer error="insufficient_scope", scope="default write""#),
    );
    assert_eq!(
        seen.lock().unwrap()[1],
        ("notion".to_string(), "insufficient_scope".to_string())
    );
}

#[test]
fn invalid_token_with_a_refresh_token_reconnects_silently_on_next_use() {
    let net = Net::new();
    net.on(
        LINEAR_TOKEN,
        200,
        None,
        &token_json("2", true, Some(3600), None),
    );
    let (t, seen) = tokens(&net);
    let v = vault();
    seed_linear(&v, None, true);
    let src = BoundTokenSource {
        tokens: &t,
        vault: &v,
        relay: None,
    };
    src.on_unauthorized("linear", Some(r#"Bearer error="invalid_token""#));
    assert!(
        seen.lock().unwrap().is_empty(),
        "a refresh may still fix it"
    );
    assert_eq!(
        src.bearer_for("linear", &linear_url()).unwrap().as_str(),
        format!("at-{MARK}-2")
    );
    assert_eq!(net.sent_to(LINEAR_TOKEN).len(), 1);
    assert!(!store::read(&v, "linear").unwrap().needs_refresh);
}

#[test]
fn revoke_clears_the_slot_and_a_new_connect_works_again() {
    let net = Net::new();
    let revoke_url = "https://mcp.notion.com/token";
    net.on(revoke_url, 200, None, "{}");
    let (t, seen) = tokens(&net);
    let v = vault();
    seed(
        &v,
        "notion",
        "https://mcp.notion.com/mcp",
        NOTION_TOKEN,
        "n",
        Some(0),
        false,
    );
    let mut rec = store::read(&v, "notion").unwrap();
    rec.revocation_endpoint = Some(revoke_url.into());
    store::write(&v, &rec).unwrap();
    // Expired, no refresh: the member is asked once.
    let notion = url(NOTION_MCP);
    assert!(t.bearer(&v, None, "notion", &notion).is_err());
    assert_eq!(seen.lock().unwrap().len(), 1);
    assert_eq!(t.revoke(&v, None, "notion").unwrap(), Revoked::Confirmed);
    let post = &net.sent_to(revoke_url)[0];
    assert_eq!(post.field("token"), Some(format!("at-{MARK}-n").as_str()));
    assert_eq!(post.field("token_type_hint"), Some("access_token"));
    assert_eq!(
        t.bearer(&v, None, "notion", &notion).unwrap_err(),
        ConnectorAuthError::NotConnected
    );
    assert_eq!(
        t.revoke(&v, None, "notion").unwrap(),
        Revoked::NothingStored
    );
    // Reconnect: a new record works, and a later failure may notify again.
    seed(
        &v,
        "notion",
        "https://mcp.notion.com/mcp",
        NOTION_TOKEN,
        "n2",
        Some(0),
        false,
    );
    assert!(t.bearer(&v, None, "notion", &notion).is_err());
    assert_eq!(seen.lock().unwrap().len(), 2);
}

#[test]
fn a_failed_or_unsupported_revoke_still_clears_the_slot() {
    let net = Net::new();
    net.on("https://mcp.linear.app/revoke", 500, None, "down");
    let (t, _) = tokens(&net);
    let v = vault();
    seed_linear(&v, None, true);
    assert_eq!(
        t.revoke(&v, None, "linear").unwrap(),
        Revoked::NotSupported,
        "no revocation endpoint"
    );
    assert!(store::read(&v, "linear").is_err());
    seed_linear(&v, None, true);
    let mut rec = store::read(&v, "linear").unwrap();
    rec.revocation_endpoint = Some("https://mcp.linear.app/revoke".into());
    store::write(&v, &rec).unwrap();
    assert_eq!(t.revoke(&v, None, "linear").unwrap(), Revoked::Failed(500));
    assert!(store::read(&v, "linear").is_err());
    assert_eq!(
        net.sent_to("https://mcp.linear.app/revoke")[0].field("token_type_hint"),
        Some("refresh_token")
    );
}

fn seed_github(v: &CustodyVault, expires_at: Option<u64>) {
    let rec = TokenRecord {
        v: store::RECORD_V,
        connector_id: GITHUB_CONNECTOR.into(),
        resource: "https://api.githubcopilot.com/mcp".into(),
        issuer: "https://github.com/login/oauth".into(),
        token_endpoint: "https://github.com/login/oauth/access_token".into(),
        revocation_endpoint: None,
        client: ClientRef {
            kind: "relay".into(),
            client_id: crate::connections::oauth_client::GITHUB_CLIENT_ID.into(),
            provider: Some("github".into()),
            redirect_uri: super::LOOPBACK_REDIRECT.into(),
        },
        access_token: SecretString::new(format!("gho-{MARK}")),
        refresh_token: Some(SecretString::new(format!("ghr-{MARK}"))),
        scope: Some("repo".into()),
        expires_at,
        connected_at: 1,
        needs_refresh: false,
    };
    store::write(v, &rec).unwrap();
}

#[test]
fn relay_refresh_and_revoke_carry_the_resource_and_no_secret() {
    let net = Net::new();
    let (t, _) = tokens(&net);
    let v = vault();
    seed_github(&v, Some(0));
    let relay = Relay::answering(vec![
        Ok(token_json("gh2", true, Some(3600), None)),
        Ok("{\"revoked\":true}".into()),
    ]);
    let gh = url(GITHUB_MCP_READONLY);
    assert_eq!(
        t.bearer(&v, Some(&relay), GITHUB_CONNECTOR, &gh)
            .unwrap()
            .as_str(),
        format!("at-{MARK}-gh2")
    );
    assert_eq!(
        t.revoke(&v, Some(&relay), GITHUB_CONNECTOR).unwrap(),
        Revoked::Confirmed
    );
    let calls = relay.calls();
    assert_eq!(
        calls[0].1,
        crate::connections::oauth_client::relay::RelayOp::Refresh
    );
    let body: serde_json::Value = serde_json::from_str(&calls[0].2).unwrap();
    assert_eq!(body["resource"], "https://api.githubcopilot.com/mcp");
    assert_eq!(body["refresh_token"], format!("ghr-{MARK}"));
    assert_eq!(
        calls[1].1,
        crate::connections::oauth_client::relay::RelayOp::Revoke
    );
    assert!(calls.iter().all(|c| !c.2.contains("client_secret")));
    assert!(
        net.sent().is_empty(),
        "nothing went to GitHub from the device"
    );
}

#[test]
fn notices_and_errors_carry_no_credential() {
    let net = Net::new();
    net.on(
        LINEAR_TOKEN,
        400,
        None,
        &format!(r#"{{"error":"invalid_grant","error_description":"rt-{MARK}-1"}}"#),
    );
    let (t, seen) = tokens(&net);
    let v = vault();
    seed_linear(&v, Some(0), true);
    let e = t.bearer(&v, None, "linear", &linear_url()).unwrap_err();
    assert!(!format!("{e:?}").contains(MARK));
    for (id, why) in seen.lock().unwrap().iter() {
        assert!(!id.contains(MARK) && !why.contains(MARK));
    }
}

// ------------------------------------------------------------- L6-WP13 (GitHub part)

fn seed_old_github(v: &CustodyVault) {
    let mut b = serde_json::to_vec(
        &serde_json::json!({"access_token": format!("old-{MARK}"), "connected_at": 1}),
    )
    .unwrap();
    v.put(&slot(Service::GitHub), &mut b).unwrap();
}

fn connected(id: &str) -> Connected {
    Connected {
        connector_id: id.into(),
        resource: "https://api.githubcopilot.com/mcp".into(),
        client_kind: "relay",
        scope: None,
        missing_scopes: vec![],
    }
}

#[test]
fn old_github_slot_kept_until_new_connection_succeeds() {
    let v = vault();
    seed_old_github(&v);
    assert!(github_needs_migration(&v));
    // A failed or declined sign-in leaves the old token and the prompt in place.
    for failed in [
        Err(McpAuthError::Declined),
        Err(McpAuthError::TokenRejected("invalid_grant".into())),
    ] {
        assert!(!finish_github_migration(&v, &failed).unwrap());
        assert!(v.custody_get(&slot(Service::GitHub)).is_ok());
        assert!(github_needs_migration(&v));
    }
    // A reported success without a readable new record does not clear it either.
    assert!(finish_github_migration(&v, &Ok(connected(GITHUB_CONNECTOR))).is_err());
    assert!(v.custody_get(&slot(Service::GitHub)).is_ok());
}

#[test]
fn old_github_slot_cleared_after_success() {
    let v = vault();
    seed_old_github(&v);
    seed_github(&v, None);
    assert!(
        !github_needs_migration(&v),
        "already connected to the MCP server"
    );
    assert!(finish_github_migration(&v, &Ok(connected(GITHUB_CONNECTOR))).unwrap());
    assert!(v.custody_get(&slot(Service::GitHub)).is_err());
    assert!(store::read(&v, GITHUB_CONNECTOR).is_ok());
}

#[test]
fn hf_and_google_are_not_migrated() {
    let v = vault();
    for svc in [
        Service::HuggingFace,
        Service::GoogleSheets,
        Service::GoogleCalendar,
    ] {
        let mut b = br#"{"access_token":"x","connected_at":1}"#.to_vec();
        v.put(&slot(svc), &mut b).unwrap();
    }
    seed_github(&v, None);
    assert!(!github_needs_migration(&v));
    assert!(!finish_github_migration(&v, &Ok(connected("huggingface"))).unwrap());
    finish_github_migration(&v, &Ok(connected(GITHUB_CONNECTOR))).unwrap();
    for svc in [
        Service::HuggingFace,
        Service::GoogleSheets,
        Service::GoogleCalendar,
    ] {
        assert!(v.custody_get(&slot(svc)).is_ok(), "{svc:?} untouched");
    }
    assert!(
        !github_needs_migration(&vault()),
        "nothing to migrate on a fresh profile"
    );
}
