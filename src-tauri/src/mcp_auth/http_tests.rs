// L6-WP06 hardened fetch tests: https only, public addresses only (checked after DNS, then
// pinned), no user info or fragment.

use super::fixtures::*;
use super::http::{self, Request};
use super::McpAuthError;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

#[test]
fn only_plain_public_https_urls_pass() {
    for ok in [
        "https://mcp.linear.app/mcp",
        "https://93.184.215.14/x",
        "https://mcp.stripe.com",
    ] {
        assert_eq!(http::check_url(&url(ok)), Ok(()), "{ok}");
    }
    for (bad, want) in [
        ("http://mcp.linear.app/mcp", McpAuthError::InsecureUrl),
        ("ftp://mcp.linear.app/", McpAuthError::InsecureUrl),
        ("https://u@mcp.linear.app/", McpAuthError::InsecureUrl),
        ("https://mcp.linear.app/#x", McpAuthError::InsecureUrl),
        ("https://127.0.0.1/", McpAuthError::PrivateAddress),
        ("https://172.16.0.1/", McpAuthError::PrivateAddress),
        ("https://100.64.0.1/", McpAuthError::PrivateAddress),
        ("https://169.254.169.254/", McpAuthError::PrivateAddress),
        ("https://[::1]/", McpAuthError::PrivateAddress),
        ("https://[fd00::1]/", McpAuthError::PrivateAddress),
        ("https://[::ffff:10.0.0.1]/", McpAuthError::PrivateAddress),
        ("https://LOCALHOST./", McpAuthError::PrivateAddress),
    ] {
        assert_eq!(http::check_url(&url(bad)), Err(want), "{bad}");
    }
    assert_eq!(
        http::parse_checked("not a url"),
        Err(McpAuthError::InsecureUrl)
    );
}

#[test]
fn a_name_that_resolves_to_any_private_address_is_never_contacted() {
    let net = Net::new();
    for ips in [
        vec![IpAddr::V4(Ipv4Addr::LOCALHOST)],
        vec![PUBLIC_IP, IpAddr::V4(Ipv4Addr::new(192, 168, 0, 2))],
        vec![IpAddr::V6(Ipv6Addr::LOCALHOST)],
        vec![],
    ] {
        net.dns("evil.example.com", ips);
        assert!(http::send(
            &NetHttp(net.clone()),
            &url("https://evil.example.com/x"),
            Request::Get
        )
        .is_err());
    }
    assert!(net.sent().is_empty());
    // A public answer is sent, pinned to that answer (the seam asserts a non-empty pin).
    net.dns("ok.example.com", vec![PUBLIC_IP]);
    net.on("https://ok.example.com/x", 200, None, "{}");
    let r = http::send(
        &NetHttp(net.clone()),
        &url("https://ok.example.com/x"),
        Request::Get,
    )
    .unwrap();
    assert_eq!(r.status, 200);
    assert_eq!(net.sent().len(), 1);
}

#[test]
fn the_probe_is_an_unauthenticated_initialize_with_no_member_data() {
    let v: serde_json::Value = serde_json::from_str(http::PROBE_BODY).unwrap();
    assert_eq!(v["method"], "initialize");
    assert_eq!(v["params"]["protocolVersion"], http::PROBE_PROTOCOL_VERSION);
    assert_eq!(v["params"]["clientInfo"]["name"], "Citrate Core");
}
