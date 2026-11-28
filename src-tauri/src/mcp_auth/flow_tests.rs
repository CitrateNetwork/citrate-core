// L6-WP06 flow tests (lanes/L6 WP06 RED list: authorize_carries_resource_and_s256,
// callback_iss_checked_when_advertised, token_bound_to_resource; owner brief: state, exact
// redirect, cross-provider callback, denied consent, expired or replayed code, missing scopes,
// long tokens, redaction).

use super::discovery::{discover, Discovered};
use super::fixtures::*;
use super::flow::*;
use super::registration::{fixed_metadata, resolve, Client};
use super::{store, McpAuthError};
use base64::Engine as _;
use sha2::{Digest as _, Sha256};
use std::collections::HashMap;
use std::sync::Arc;

const LINEAR_TOKEN: &str = "https://mcp.linear.app/token";

fn linear() -> (Arc<Net>, Discovered) {
    let net = Net::new();
    serve_linear(&net);
    let d = discover(&NetHttp(net.clone()), &url(LINEAR_MCP), &fixed_metadata).unwrap();
    (net, d)
}

fn cimd() -> Client {
    Client::Public {
        kind: "cimd",
        client_id: super::CIMD_CLIENT_ID.to_string(),
    }
}

fn query(u: &str) -> HashMap<String, String> {
    url(u)
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect()
}

fn cb(req: &AuthRequest, extra: &str) -> Callback {
    parse_callback(&format!(
        "/oauth/callback?code=code-{MARK}&state={}{extra}",
        req.state
    ))
}

#[test]
fn authorize_carries_resource_and_s256() {
    let (_, d) = linear();
    let req = begin(d, cimd());
    assert!(req.url.starts_with("https://mcp.linear.app/authorize?"));
    let q = query(&req.url);
    assert_eq!(q["response_type"], "code");
    assert_eq!(
        q["client_id"],
        "https://citrate.ai/oauth/client/citrate-core.json"
    );
    assert_eq!(q["redirect_uri"], "http://127.0.0.1:8975/oauth/callback");
    assert_eq!(q["resource"], "https://mcp.linear.app/mcp");
    assert_eq!(q["scope"], "read write");
    assert_eq!(q["code_challenge_method"], "S256");
    let want = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(Sha256::digest(req.verifier().as_bytes()));
    assert_eq!(
        q["code_challenge"], want,
        "challenge = BASE64URL(SHA256(verifier))"
    );
    assert!(
        req.verifier().len() >= 43,
        "RFC 7636 minimum verifier length"
    );
    assert!(
        !req.url.contains(req.verifier()),
        "the verifier never leaves the device"
    );
}

#[test]
fn every_authorization_has_a_fresh_high_entropy_state() {
    let (_, d) = linear();
    let a = begin(d.clone(), cimd());
    let b = begin(d, cimd());
    assert_ne!(a.state, b.state);
    assert!(a.state.len() >= 43);
    assert_eq!(query(&a.url)["state"], a.state);
}

#[test]
fn a_wrong_or_missing_state_fails_closed() {
    let (_, d) = linear();
    let req = begin(d, cimd());
    let bad = parse_callback(&format!("/oauth/callback?code=c&state=x{}", req.state));
    assert_eq!(
        check_callback(&req, bad).unwrap_err(),
        McpAuthError::StateMismatch
    );
    let none = parse_callback("/oauth/callback?code=c");
    assert_eq!(
        check_callback(&req, none).unwrap_err(),
        McpAuthError::StateMismatch
    );
}

#[test]
fn callback_iss_checked_when_advertised() {
    let (_, d) = linear();
    let req = begin(d, cimd());
    // Linear advertises RFC 9207: a callback without `iss` is refused.
    assert_eq!(
        check_callback(&req, cb(&req, "")).unwrap_err(),
        McpAuthError::IssMismatch
    );
    let ok = cb(&req, "&iss=https%3A%2F%2Fmcp.linear.app");
    assert!(check_callback(&req, ok).unwrap().contains(MARK));
}

#[test]
fn a_callback_from_another_provider_is_rejected() {
    let (_, d) = linear();
    let req = begin(d, cimd());
    let notion = cb(&req, "&iss=https%3A%2F%2Fmcp.notion.com");
    assert_eq!(
        check_callback(&req, notion).unwrap_err(),
        McpAuthError::IssMismatch
    );
    // A server without RFC 9207 support (Atlassian) still has a present `iss` checked.
    let net = Net::new();
    serve(
        &net,
        ATLASSIAN_MCP,
        ATLASSIAN_WWW,
        ATLASSIAN_PRM_URL,
        ATLASSIAN_PRM,
        ATLASSIAN_AS_URL,
        ATLASSIAN_AS,
    );
    let d = discover(&NetHttp(net), &url(ATLASSIAN_MCP), &fixed_metadata).unwrap();
    assert!(!d.auth_server.iss_parameter_supported);
    let req = begin(d, cimd());
    assert!(
        check_callback(&req, cb(&req, "")).is_ok(),
        "absent iss is allowed when not advertised"
    );
    let foreign = cb(&req, "&iss=https%3A%2F%2Fmcp.linear.app");
    assert_eq!(
        check_callback(&req, foreign).unwrap_err(),
        McpAuthError::IssMismatch
    );
}

#[test]
fn denied_consent_ends_the_flow_and_stores_nothing() {
    let (net, d) = linear();
    let v = vault();
    let req = begin(d, cimd());
    let denied = parse_callback(&format!(
        "/oauth/callback?error=access_denied&state={}",
        req.state
    ));
    assert_eq!(
        complete(&NetHttp(net.clone()), &v, None, "linear", &req, denied).unwrap_err(),
        McpAuthError::Declined
    );
    assert!(net.sent_to(LINEAR_TOKEN).is_empty(), "nothing exchanged");
    assert_eq!(
        store::read(&v, "linear").unwrap_err(),
        McpAuthError::NotConnected
    );
    // A denial that does not carry the flow's state is a drive-by: it cannot end the flow.
    let driveby = parse_callback("/oauth/callback?error=access_denied&state=other");
    assert_eq!(
        check_callback(&req, driveby).unwrap_err(),
        McpAuthError::StateMismatch
    );
}

#[test]
fn an_expired_or_replayed_code_passes_the_servers_error_through() {
    let (net, d) = linear();
    let v = vault();
    net.on(
        LINEAR_TOKEN,
        400,
        None,
        &format!(
            r#"{{"error":"invalid_grant","error_description":"code code-{MARK} already used"}}"#
        ),
    );
    let req = begin(d, cimd());
    let err = complete(
        &NetHttp(net.clone()),
        &v,
        None,
        "linear",
        &req,
        cb(&req, "&iss=https://mcp.linear.app"),
    )
    .unwrap_err();
    assert_eq!(err, McpAuthError::TokenRejected("invalid_grant".into()));
    assert!(
        !err.to_string().contains(MARK),
        "the description (which echoes the code) is dropped"
    );
    assert_eq!(
        store::read(&v, "linear").unwrap_err(),
        McpAuthError::NotConnected
    );
    // A code that is not a plain OAuth error code is not passed through.
    assert_eq!(
        parse_grant(400, r#"{"error":"<script>"}"#).unwrap_err(),
        McpAuthError::TokenRejected("invalid_response".into())
    );
    assert_eq!(
        parse_grant(500, "oops").unwrap_err(),
        McpAuthError::TokenRejected("invalid_response".into())
    );
}

#[test]
fn the_token_request_carries_verifier_same_redirect_and_resource_and_no_secret() {
    let (net, d) = linear();
    let v = vault();
    net.on(
        LINEAR_TOKEN,
        200,
        None,
        &token_json("1", true, Some(3600), Some("read write")),
    );
    let req = begin(d, cimd());
    let done = complete(
        &NetHttp(net.clone()),
        &v,
        None,
        "linear",
        &req,
        cb(&req, "&iss=https://mcp.linear.app"),
    )
    .unwrap();
    assert_eq!(done.client_kind, "cimd");
    assert!(done.missing_scopes.is_empty());
    let post = &net.sent_to(LINEAR_TOKEN)[0];
    assert_eq!(post.field("grant_type"), Some("authorization_code"));
    assert_eq!(post.field("code_verifier"), Some(req.verifier()));
    assert_eq!(
        post.field("redirect_uri"),
        Some(query(&req.url)["redirect_uri"].as_str())
    );
    assert_eq!(post.field("resource"), Some("https://mcp.linear.app/mcp"));
    assert_eq!(post.field("client_id"), Some(super::CIMD_CLIENT_ID));
    assert_eq!(post.field("client_secret"), None);
    assert!(post.basic.is_none());
}

#[test]
fn token_bound_to_resource() {
    let (net, d) = linear();
    let v = vault();
    net.on(
        LINEAR_TOKEN,
        200,
        None,
        &token_json("1", true, Some(3600), None),
    );
    let req = begin(d, cimd());
    complete(
        &NetHttp(net),
        &v,
        None,
        "linear",
        &req,
        cb(&req, "&iss=https://mcp.linear.app"),
    )
    .unwrap();
    let rec = store::read(&v, "linear").unwrap();
    assert_eq!(rec.resource, "https://mcp.linear.app/mcp");
    assert_eq!(rec.issuer, "https://mcp.linear.app");
    assert!(rec.serves(&url("https://mcp.linear.app/mcp")));
    assert!(!rec.serves(&url("https://mcp.notion.com/mcp")));
    assert!(rec.expires_at.is_some());
}

#[test]
fn missing_scopes_are_reported_without_failing_the_connect() {
    let (net, d) = linear();
    let v = vault();
    net.on(
        LINEAR_TOKEN,
        200,
        None,
        &token_json("1", false, None, Some("read")),
    );
    let req = begin(d, cimd());
    let done = complete(
        &NetHttp(net),
        &v,
        None,
        "linear",
        &req,
        cb(&req, "&iss=https://mcp.linear.app"),
    )
    .unwrap();
    assert_eq!(done.missing_scopes, vec!["write".to_string()]);
    assert_eq!(done.scope.as_deref(), Some("read"));
}

#[test]
fn a_non_bearer_or_empty_token_is_refused() {
    assert!(parse_grant(200, r#"{"access_token":"x","token_type":"mac"}"#).is_err());
    assert!(parse_grant(200, r#"{"access_token":"  "}"#).is_err());
    assert!(parse_grant(200, r#"{"token_type":"bearer"}"#).is_err());
    assert!(parse_grant(200, r#"{"access_token":"x","token_type":"bearer"}"#).is_ok());
}

#[test]
fn a_fixed_client_exchanges_through_the_relay_with_resource_and_no_secret() {
    let net = Net::new();
    net.on(GITHUB_MCP, 401, Some(GITHUB_WWW), "");
    net.on(GITHUB_PRM_URL, 200, None, GITHUB_PRM);
    let v = vault();
    let d = discover(&NetHttp(net.clone()), &url(GITHUB_MCP), &fixed_metadata).unwrap();
    let client = resolve(&NetHttp(net.clone()), &v, &d.auth_server).unwrap();
    let req = begin(d, client);
    let q = query(&req.url);
    assert_eq!(
        q["client_id"],
        crate::connections::oauth_client::GITHUB_CLIENT_ID
    );
    assert_eq!(q["redirect_uri"], "http://127.0.0.1:8975/oauth/callback");
    assert_eq!(q["resource"], "https://api.githubcopilot.com/mcp");
    let relay = Relay::answering(vec![Ok(token_json("gh", false, None, Some("repo")))]);
    let done = complete(
        &NetHttp(net.clone()),
        &v,
        Some(&relay),
        "github",
        &req,
        cb(&req, ""),
    )
    .unwrap();
    assert_eq!(done.client_kind, "relay");
    let calls = relay.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "github");
    let body: serde_json::Value = serde_json::from_str(&calls[0].2).unwrap();
    assert_eq!(body["resource"], "https://api.githubcopilot.com/mcp");
    assert_eq!(body["redirect_uri"], "http://127.0.0.1:8975/oauth/callback");
    assert_eq!(body["code_verifier"], req.verifier());
    assert!(body.get("client_secret").is_none());
    assert!(
        net.sent().iter().all(|s| s.kind != "form"),
        "nothing posted to GitHub from the device"
    );
    // Without a Citrate session the relay cannot be used.
    let req2 = begin(
        discover(&NetHttp(net.clone()), &url(GITHUB_MCP), &fixed_metadata).unwrap(),
        resolve(
            &NetHttp(net.clone()),
            &v,
            &fixed_metadata("https://github.com/login/oauth").unwrap(),
        )
        .unwrap(),
    );
    assert_eq!(
        complete(&NetHttp(net), &v, None, "github2", &req2, cb(&req2, "")).unwrap_err(),
        McpAuthError::NotSignedIn
    );
}

#[test]
fn long_tokens_seal_and_read_back_unchanged() {
    let (net, d) = linear();
    let v = vault();
    let at = format!("at-{MARK}-{}", "a".repeat(2500));
    let rt = format!("rt-{MARK}-{}", "r".repeat(4000));
    net.on(
        LINEAR_TOKEN,
        200,
        None,
        &serde_json::json!({"access_token": at, "refresh_token": rt, "token_type": "bearer"})
            .to_string(),
    );
    let req = begin(d, cimd());
    complete(
        &NetHttp(net),
        &v,
        None,
        "linear",
        &req,
        cb(&req, "&iss=https://mcp.linear.app"),
    )
    .unwrap();
    let rec = store::read(&v, "linear").unwrap();
    assert_eq!(rec.access_token.expose(), at);
    assert_eq!(rec.refresh_token.as_ref().unwrap().expose(), rt);
}

#[test]
fn nothing_secret_appears_in_debug_output_or_errors() {
    let (net, d) = linear();
    let v = vault();
    net.on(
        LINEAR_TOKEN,
        200,
        None,
        &token_json("1", true, Some(3600), None),
    );
    let req = begin(d, cimd());
    let dbg_req = format!("{req:?}");
    assert!(!dbg_req.contains(req.verifier()) && !dbg_req.contains(&req.state));
    let done = complete(
        &NetHttp(net),
        &v,
        None,
        "linear",
        &req,
        cb(&req, "&iss=https://mcp.linear.app"),
    )
    .unwrap();
    assert!(!format!("{done:?}").contains(MARK));
    let rec = store::read(&v, "linear").unwrap();
    assert!(!format!("{rec:?}").contains(MARK));
    let reply = super::http::Reply {
        status: 200,
        www_authenticate: None,
        body: zeroize::Zeroizing::new(MARK.into()),
    };
    assert!(!format!("{reply:?}").contains(MARK));
}

#[test]
#[allow(clippy::assertions_on_constants)] // pins the compiled default
fn connect_is_refused_while_the_flag_is_off() {
    assert!(!super::MCP_REMOTE_AUTH, "default off");
    if super::mcp_remote_auth_enabled() {
        return; // a developer turned it on for this run
    }
    let net = Net::new();
    serve_linear(&net);
    let v = vault();
    let err = connect(
        &NetHttp(net.clone()),
        &v,
        None,
        "linear",
        &url(LINEAR_MCP),
        |_| panic!("no browser"),
    )
    .unwrap_err();
    assert_eq!(err, McpAuthError::Disabled);
    assert!(
        net.sent().is_empty(),
        "no network call before the flag check"
    );
}

#[test]
fn the_loopback_parser_keeps_iss_and_callbacks_without_it_still_parse() {
    let p = crate::connections::parse_callback_target(
        "/oauth/callback?code=c&state=s&iss=https%3A%2F%2Fmcp.linear.app",
    )
    .unwrap();
    assert_eq!(p.iss.as_deref(), Some("https://mcp.linear.app"));
    let old = crate::connections::parse_callback_target("/oauth/callback?code=c&state=s").unwrap();
    assert_eq!(old.iss, None);
    assert_eq!(old.code, "c");
}

#[test]
fn slack_v2_user_takes_the_user_scopes_in_scope_comma_joined() {
    // Slack's fixed row is not configured yet (no compiled client id), so the client is built here
    // with a fixture id to pin the authorize parameters the row produces.
    let net = Net::new();
    serve(
        &net,
        SLACK_MCP,
        SLACK_WWW,
        SLACK_PRM_URL,
        SLACK_PRM,
        SLACK_AS_URL,
        SLACK_AS,
    );
    let d = discover(&NetHttp(net), &url(SLACK_MCP), &fixed_metadata).unwrap();
    let client = Client::Relay {
        provider: "slack",
        client_id: "fixture-slack-client-id",
        redirect_uri: crate::connections::HOSTED_REDIRECT_URI,
    };
    let req = begin(d, client);
    assert!(req
        .url
        .starts_with("https://slack.com/oauth/v2_user/authorize?"));
    let q = query(&req.url);
    assert_eq!(
        q["scope"],
        "search:read.public,channels:read,channels:history,users:read"
    );
    assert!(!q.contains_key("user_scope"), "v2_user rejects user_scope");
    assert_eq!(q["redirect_uri"], "https://auth.citrate.ai/oauth/callback");
    assert_eq!(q["code_challenge_method"], "S256");
    assert_eq!(q["resource"], "https://mcp.slack.com");
    assert_eq!(
        req.discovered.scopes.len(),
        4,
        "the row's scopes are the requested set"
    );
}
