// L6-WP06 discovery tests (lanes/L6 WP06 RED list: discovery_follows_resource_metadata_then_
// as_metadata, discovery_refuses_http_and_private_addresses, discovery_caps_size_and_time,
// as_issuer_must_match_metadata), plus the matrix normalizations (quirks 1 to 4 and 8).

use super::discovery::*;
use super::fixtures::*;
use super::http::{self, Request};
use super::McpAuthError;
use std::net::{IpAddr, Ipv4Addr};

fn no_fallback(_: &str) -> Option<AsMetadata> {
    None
}

fn discover_on(net: &std::sync::Arc<Net>, endpoint: &str) -> Result<Discovered, McpAuthError> {
    discover(&NetHttp(net.clone()), &url(endpoint), &no_fallback)
}

#[test]
fn parses_bearer_challenges_from_the_matrix() {
    let c = parse_www_authenticate(LINEAR_WWW).unwrap();
    assert_eq!(c.resource_metadata.as_deref(), Some(LINEAR_PRM_URL));
    assert_eq!(c.scope.as_deref(), Some("read write"));
    let g = parse_www_authenticate(GITHUB_WWW).unwrap();
    assert_eq!(g.error.as_deref(), Some("invalid_request"));
    assert_eq!(g.resource_metadata.as_deref(), Some(GITHUB_PRM_URL));
    // Figma: no space after a comma, and an extra `authorization_uri` that is ignored (quirk 7).
    let f = parse_www_authenticate(
        r#"Bearer resource_metadata="https://mcp.figma.com/.well-known/oauth-protected-resource",scope="mcp:connect",authorization_uri="https://api.figma.com/.well-known/oauth-authorization-server""#,
    )
    .unwrap();
    assert_eq!(f.scope.as_deref(), Some("mcp:connect"));
    assert_eq!(
        f.resource_metadata.as_deref(),
        Some("https://mcp.figma.com/.well-known/oauth-protected-resource")
    );
    // Another scheme first, then Bearer; an escaped quote inside a value.
    let m =
        parse_www_authenticate(r#"Basic realm="x", Bearer error="invalid_token", scope="a \"b\"""#)
            .unwrap();
    assert_eq!(m.error.as_deref(), Some("invalid_token"));
    assert_eq!(m.scope.as_deref(), Some(r#"a "b""#));
    assert_eq!(parse_www_authenticate(r#"Basic realm="x""#), None);
    assert_eq!(parse_www_authenticate(""), None);
}

#[test]
fn discovery_follows_resource_metadata_then_as_metadata() {
    let net = Net::new();
    serve_linear(&net);
    let d = discover_on(&net, LINEAR_MCP).unwrap();
    assert_eq!(d.resource.as_str(), "https://mcp.linear.app/mcp");
    assert_eq!(
        d.scopes,
        vec!["read", "write"],
        "the challenge scope is authoritative"
    );
    assert_eq!(d.auth_server.issuer, "https://mcp.linear.app");
    assert!(d.auth_server.cimd_supported && d.auth_server.iss_parameter_supported);
    let order: Vec<(String, &str)> = net.sent().into_iter().map(|s| (s.url, s.kind)).collect();
    assert_eq!(
        order,
        vec![
            (LINEAR_MCP.to_string(), "probe"),
            (LINEAR_PRM_URL.to_string(), "get"),
            (LINEAR_AS_URL.to_string(), "get"),
        ]
    );
}

#[test]
fn a_server_that_answers_without_a_token_is_discovered_through_well_known_path_then_root() {
    // Hugging Face answers an anonymous initialize with 200 (matrix quirk 8).
    let net = Net::new();
    net.on(HF_MCP, 200, None, r#"{"result":{}}"#);
    net.on(HF_PRM_URL, 200, None, HF_PRM);
    net.on(HF_AS_URL, 200, None, HF_AS);
    let d = discover_on(&net, HF_MCP).unwrap();
    assert_eq!(d.resource.as_str(), HF_MCP);
    assert!(
        d.scopes.contains(&"read-mcp".to_string()),
        "no challenge: scopes_supported"
    );
    // No PRM at the path: the root is tried next, in that order.
    let net = Net::new();
    net.on("https://x.example.com/srv/mcp", 401, Some("Bearer"), "");
    let err = discover_on(&net, "https://x.example.com/srv/mcp").unwrap_err();
    assert_eq!(err, McpAuthError::NoResourceMetadata);
    let gets: Vec<String> = net
        .sent()
        .into_iter()
        .filter(|s| s.kind == "get")
        .map(|s| s.url)
        .collect();
    assert_eq!(
        gets,
        vec![
            "https://x.example.com/.well-known/oauth-protected-resource/srv/mcp",
            "https://x.example.com/.well-known/oauth-protected-resource",
        ]
    );
}

#[test]
fn resource_identifiers_that_differ_by_a_trailing_slash_or_are_a_prefix_are_accepted() {
    // Vercel: endpoint without slash, resource with one.
    let net = Net::new();
    serve(
        &net,
        VERCEL_MCP,
        VERCEL_WWW,
        VERCEL_PRM_URL,
        VERCEL_PRM,
        VERCEL_AS_URL,
        VERCEL_AS,
    );
    assert_eq!(
        discover_on(&net, VERCEL_MCP).unwrap().resource_id,
        "https://mcp.vercel.com/"
    );
    // Slack: endpoint /mcp, resource is the origin.
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
    assert_eq!(
        discover_on(&net, SLACK_MCP).unwrap().resource_id,
        "https://mcp.slack.com"
    );
    // The covers rule itself.
    let cases = [
        (
            "https://api.githubcopilot.com/mcp",
            "https://api.githubcopilot.com/mcp/",
            true,
        ),
        (
            "https://api.githubcopilot.com/mcp",
            "https://api.githubcopilot.com/mcp/readonly",
            true,
        ),
        ("https://mcp.hubspot.com", "https://mcp.hubspot.com/", true),
        (
            "https://mcp.linear.app/mcp",
            "https://mcp.linear.app/mcpx",
            false,
        ),
        (
            "https://mcp.linear.app/mcp",
            "https://mcp.linear.app/",
            false,
        ),
        (
            "https://mcp.linear.app/mcp",
            "https://mcp.notion.com/mcp",
            false,
        ),
        (
            "https://mcp.linear.app/mcp",
            "https://mcp.linear.app:8443/mcp",
            false,
        ),
        (
            "https://MCP.Linear.app/mcp",
            "https://mcp.linear.app/mcp",
            true,
        ),
    ];
    for (r, e, want) in cases {
        assert_eq!(resource_covers(&url(r), &url(e)), want, "{r} covers {e}");
    }
}

#[test]
fn prm_naming_another_server_is_a_resource_mismatch() {
    let net = Net::new();
    net.on(LINEAR_MCP, 401, Some(LINEAR_WWW), "");
    net.on(LINEAR_PRM_URL, 200, None, NOTION_PRM);
    assert_eq!(
        discover_on(&net, LINEAR_MCP).unwrap_err(),
        McpAuthError::ResourceMismatch
    );
}

#[test]
fn path_issuers_use_path_inserted_metadata_in_the_spec_order() {
    let got: Vec<String> = as_metadata_candidates(&url(ATLASSIAN_ISSUER))
        .unwrap()
        .into_iter()
        .map(|u| u.to_string())
        .collect();
    assert_eq!(
        got,
        vec![
            ATLASSIAN_AS_URL.to_string(),
            "https://auth.atlassian.com/.well-known/openid-configuration/VCeDsk8ZHncYF1g234fKtc4lNipbBhu3".into(),
            "https://auth.atlassian.com/VCeDsk8ZHncYF1g234fKtc4lNipbBhu3/.well-known/openid-configuration".into(),
        ]
    );
    let root: Vec<String> = as_metadata_candidates(&url("https://mcp.linear.app"))
        .unwrap()
        .into_iter()
        .map(|u| u.to_string())
        .collect();
    assert_eq!(
        root,
        vec![
            LINEAR_AS_URL.to_string(),
            "https://mcp.linear.app/.well-known/openid-configuration".into()
        ]
    );
    // Atlassian and Stripe discover end to end through the path-inserted URL.
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
    assert_eq!(
        discover_on(&net, ATLASSIAN_MCP).unwrap().auth_server.issuer,
        ATLASSIAN_ISSUER
    );
    let net = Net::new();
    serve(
        &net,
        STRIPE_MCP,
        STRIPE_WWW,
        STRIPE_PRM_URL,
        STRIPE_PRM,
        STRIPE_AS_URL,
        STRIPE_AS,
    );
    let d = discover_on(&net, STRIPE_MCP).unwrap();
    assert_eq!(d.auth_server.issuer, "https://access.stripe.com/mcp");
    assert!(d.auth_server.registration_endpoint.is_some() && !d.auth_server.cimd_supported);
}

#[test]
fn as_issuer_must_match_metadata_with_one_trailing_slash_normalization() {
    // Google: the PRM lists `https://accounts.google.com/`, the metadata says no slash (quirk 3).
    assert!(issuers_match(
        "https://accounts.google.com/",
        "https://accounts.google.com"
    ));
    let m = parse_as_metadata(GOOGLE_AS, "https://accounts.google.com/").unwrap();
    assert_eq!(m.issuer, "https://accounts.google.com");
    // Any other difference is refused.
    assert!(!issuers_match(
        "https://accounts.google.com//",
        "https://accounts.google.com"
    ));
    assert_eq!(
        parse_as_metadata(LINEAR_AS, "https://mcp.notion.com").unwrap_err(),
        McpAuthError::IssuerMismatch
    );
    let net = Net::new();
    net.on(LINEAR_MCP, 401, Some(LINEAR_WWW), "");
    net.on(LINEAR_PRM_URL, 200, None, LINEAR_PRM);
    net.on(LINEAR_AS_URL, 200, None, NOTION_AS);
    assert_eq!(
        discover_on(&net, LINEAR_MCP).unwrap_err(),
        McpAuthError::IssuerMismatch
    );
}

#[test]
fn a_server_that_does_not_advertise_s256_is_refused() {
    let no_field = LINEAR_AS.replace(r#""code_challenge_methods_supported":["S256"],"#, "");
    assert_eq!(
        parse_as_metadata(&no_field, "https://mcp.linear.app").unwrap_err(),
        McpAuthError::PkceS256Unsupported
    );
    let plain = LINEAR_AS.replace(r#"["S256"]"#, r#"["plain"]"#);
    assert_eq!(
        parse_as_metadata(&plain, "https://mcp.linear.app").unwrap_err(),
        McpAuthError::PkceS256Unsupported
    );
}

#[test]
fn discovery_refuses_http_and_private_addresses() {
    let net = Net::new();
    for (u, want) in [
        ("http://mcp.linear.app/mcp", McpAuthError::InsecureUrl),
        (
            "https://user:pw@mcp.linear.app/mcp",
            McpAuthError::InsecureUrl,
        ),
        ("https://mcp.linear.app/mcp#frag", McpAuthError::InsecureUrl),
        ("https://127.0.0.1/mcp", McpAuthError::PrivateAddress),
        ("https://10.0.0.8/mcp", McpAuthError::PrivateAddress),
        ("https://192.168.1.4/mcp", McpAuthError::PrivateAddress),
        (
            "https://169.254.169.254/latest/meta-data",
            McpAuthError::PrivateAddress,
        ),
        ("https://[::1]/mcp", McpAuthError::PrivateAddress),
        ("https://[fe80::1]/mcp", McpAuthError::PrivateAddress),
        ("https://localhost/mcp", McpAuthError::PrivateAddress),
        ("https://api.localhost/mcp", McpAuthError::PrivateAddress),
    ] {
        assert_eq!(discover_on(&net, u).unwrap_err(), want, "{u}");
    }
    // A public name that resolves to a private or metadata address (DNS rebinding) is refused
    // before anything is sent.
    net.dns(
        "rebind.example.com",
        vec![IpAddr::V4(Ipv4Addr::new(169, 254, 169, 254))],
    );
    assert_eq!(
        discover_on(&net, "https://rebind.example.com/mcp").unwrap_err(),
        McpAuthError::PrivateAddress
    );
    net.dns(
        "mixed.example.com",
        vec![PUBLIC_IP, IpAddr::V4(Ipv4Addr::new(10, 1, 2, 3))],
    );
    assert_eq!(
        discover_on(&net, "https://mixed.example.com/mcp").unwrap_err(),
        McpAuthError::PrivateAddress
    );
    assert!(
        net.sent().is_empty(),
        "nothing was sent to a refused address"
    );
    // A PRM or AS URL taken from a server document gets the same checks.
    let net = Net::new();
    net.on(
        LINEAR_MCP,
        401,
        Some(r#"Bearer resource_metadata="http://mcp.linear.app/prm""#),
        "",
    );
    assert_eq!(
        discover_on(&net, LINEAR_MCP).unwrap_err(),
        McpAuthError::InsecureUrl
    );
    let net = Net::new();
    net.on(LINEAR_MCP, 401, Some(LINEAR_WWW), "");
    net.on(
        LINEAR_PRM_URL,
        200,
        None,
        &LINEAR_PRM.replace("https://mcp.linear.app\"]", "https://10.0.0.1\"]"),
    );
    assert_eq!(
        discover_on(&net, LINEAR_MCP).unwrap_err(),
        McpAuthError::PrivateAddress
    );
    let net = Net::new();
    serve(
        &net,
        LINEAR_MCP,
        LINEAR_WWW,
        LINEAR_PRM_URL,
        LINEAR_PRM,
        LINEAR_AS_URL,
        &LINEAR_AS.replace(
            "https://mcp.linear.app/token",
            "http://mcp.linear.app/token",
        ),
    );
    assert_eq!(
        discover_on(&net, LINEAR_MCP).unwrap_err(),
        McpAuthError::InsecureUrl
    );
}

/// A seam that fails every request with one error (size cap or deadline hit in the transport).
struct Failing(McpAuthError);
impl super::http::McpHttp for Failing {
    fn resolve(&self, _: &str, _: u16) -> Result<Vec<IpAddr>, McpAuthError> {
        Ok(vec![PUBLIC_IP])
    }
    fn send(
        &self,
        _: &url::Url,
        _: &[IpAddr],
        _: Request<'_>,
    ) -> Result<http::Reply, McpAuthError> {
        Err(self.0.clone())
    }
}

#[test]
fn discovery_caps_size_and_time() {
    // The production transport reads at most 64 KiB and gives up after 10 s (LiveHttp config).
    assert_eq!(http::MAX_BODY, 64 * 1024);
    assert_eq!(http::TIMEOUT, std::time::Duration::from_secs(10));
    // Hitting either aborts discovery with that reason, never a partial result.
    for e in [McpAuthError::TooLarge, McpAuthError::Timeout] {
        let got = discover(&Failing(e.clone()), &url(LINEAR_MCP), &no_fallback).unwrap_err();
        assert_eq!(got, e);
    }
}
