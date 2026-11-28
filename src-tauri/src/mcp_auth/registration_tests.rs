// L6-WP06 registration tests (lanes/L6 WP06 RED list: prefers_cimd_over_dcr_over_relay,
// dcr_registers_once_per_issuer), selection driven by the matrix metadata fixtures.

use super::discovery::{discover, parse_as_metadata, AsMetadata};
use super::fixtures::*;
use super::registration::*;
use super::{McpAuthError, CIMD_CLIENT_ID, LOOPBACK_REDIRECT};

fn meta(doc: &str, issuer: &str) -> AsMetadata {
    parse_as_metadata(doc, issuer).unwrap()
}

#[test]
fn prefers_cimd_over_dcr_over_relay() {
    // CIMD advertised (and DCR too): CIMD.
    for (doc, iss) in [
        (LINEAR_AS, "https://mcp.linear.app"),
        (NOTION_AS, "https://mcp.notion.com"),
        (VERCEL_AS, "https://vercel.com"),
        (ATLASSIAN_AS, ATLASSIAN_ISSUER),
    ] {
        assert_eq!(choose(&meta(doc, iss)).unwrap(), Choice::Cimd, "{iss}");
    }
    // Notion has a fixed row, but CIMD wins: the row is used only without CIMD and DCR.
    assert!(fixed_for("https://mcp.notion.com").is_some());
    // DCR only (Stripe), and DCR when CIMD is advertised without a public token method (Hugging
    // Face, quirk 5), and a confidential DCR server (Supabase).
    assert_eq!(
        choose(&meta(STRIPE_AS, "https://access.stripe.com/mcp")).unwrap(),
        Choice::Dcr
    );
    assert_eq!(
        choose(&meta(HF_AS, "https://huggingface.co")).unwrap(),
        Choice::Dcr
    );
    assert_eq!(
        choose(&meta(SUPABASE_AS, "https://api.supabase.com")).unwrap(),
        Choice::Dcr
    );
    // Neither: the fixed row. GitHub's is configured; Slack's names the missing registration.
    let gh = fixed_metadata("https://github.com/login/oauth").unwrap();
    match choose(&gh).unwrap() {
        Choice::Fixed(row) => {
            assert_eq!(row.provider, "github");
            assert_eq!(row.redirect_uri, LOOPBACK_REDIRECT);
        }
        other => panic!("github: {other:?}"),
    }
    match choose(&meta(SLACK_AS, "https://mcp.slack.com")).unwrap_err() {
        McpAuthError::NotConfigured(why) => assert!(why.contains("sl-core"), "{why}"),
        other => panic!("slack: {other:?}"),
    }
    // Google (no CIMD, no DCR, no row yet): no path.
    assert_eq!(
        choose(&meta(GOOGLE_AS, "https://accounts.google.com")).unwrap_err(),
        McpAuthError::NoRegistrationPath
    );
}

#[test]
fn cimd_client_is_the_citrate_metadata_document_url() {
    let v = vault();
    let net = Net::new();
    let c = resolve(
        &NetHttp(net.clone()),
        &v,
        &meta(LINEAR_AS, "https://mcp.linear.app"),
    )
    .unwrap();
    assert_eq!(c.kind(), "cimd");
    assert_eq!(c.client_id(), CIMD_CLIENT_ID);
    assert_eq!(
        c.client_id(),
        "https://citrate.ai/oauth/client/citrate-core.json"
    );
    assert_eq!(c.redirect_uri(), "http://127.0.0.1:8975/oauth/callback");
    assert!(net.sent().is_empty(), "CIMD registers nothing");
}

const STRIPE_REG: &str = "https://access.stripe.com/mcp/oauth2/register";

#[test]
fn dcr_registers_once_per_issuer() {
    let v = vault();
    let net = Net::new();
    net.on(
        STRIPE_REG,
        201,
        None,
        r#"{"client_id":"stripe-dcr-1","token_endpoint_auth_method":"none"}"#,
    );
    let m = meta(STRIPE_AS, "https://access.stripe.com/mcp");
    let first = resolve(&NetHttp(net.clone()), &v, &m).unwrap();
    let second = resolve(&NetHttp(net.clone()), &v, &m).unwrap();
    assert_eq!(first.client_id(), "stripe-dcr-1");
    assert_eq!(second.client_id(), "stripe-dcr-1");
    assert_eq!(first.kind(), "dcr");
    let regs = net.sent_to(STRIPE_REG);
    assert_eq!(regs.len(), 1, "registered once, then reused from the vault");
    let body: serde_json::Value = serde_json::from_str(regs[0].json.as_deref().unwrap()).unwrap();
    assert_eq!(
        body["redirect_uris"],
        serde_json::json!(["http://127.0.0.1:8975/oauth/callback"])
    );
    assert_eq!(body["token_endpoint_auth_method"], "none");
    assert_eq!(body["application_type"], "native");
    // The registration is sealed under the issuer's slot, in the backend-reserved namespace.
    let slot = dcr_slot("https://access.stripe.com/mcp");
    assert!(citrate_core_kit::custody::is_backend_reserved_slot(&slot));
    assert!(v.custody_get(&slot).is_ok());
    // A trailing slash names the same issuer; another issuer gets its own slot.
    assert_eq!(slot, dcr_slot("https://access.stripe.com/mcp/"));
    assert_ne!(slot, dcr_slot("https://api.supabase.com"));
    // Another profile (its own vault) registers for itself.
    let other = vault();
    resolve(&NetHttp(net.clone()), &other, &m).unwrap();
    assert_eq!(net.sent_to(STRIPE_REG).len(), 2);
}

#[test]
fn a_confidential_dcr_client_seals_its_secret_and_uses_the_form_method() {
    let v = vault();
    let net = Net::new();
    let reg = "https://api.supabase.com/platform/oauth/apps/register";
    net.on(reg, 201, None, &format!(r#"{{"client_id":"sb-1","client_secret":"cs-{MARK}","token_endpoint_auth_method":"client_secret_post"}}"#));
    let c = resolve(
        &NetHttp(net.clone()),
        &v,
        &meta(SUPABASE_AS, "https://api.supabase.com"),
    )
    .unwrap();
    match &c {
        Client::Confidential {
            client_id,
            secret,
            basic,
        } => {
            assert_eq!(client_id, "sb-1");
            assert!(secret.expose().contains(MARK));
            assert!(!basic);
        }
        other => panic!("{other:?}"),
    }
    let body: serde_json::Value =
        serde_json::from_str(net.sent_to(reg)[0].json.as_deref().unwrap()).unwrap();
    assert_eq!(
        body["token_endpoint_auth_method"], "client_secret_post",
        "no `none` offered"
    );
    assert!(!format!("{c:?}").contains(MARK), "Debug shows no secret");
    let sealed = sealed_dcr(&v, "https://api.supabase.com").unwrap();
    assert!(sealed.client_secret.unwrap().expose().contains(MARK));
}

#[test]
fn a_refused_registration_is_reported_and_nothing_is_sealed() {
    let v = vault();
    let net = Net::new();
    net.on(STRIPE_REG, 400, None, r#"{"error":"invalid_redirect_uri"}"#);
    let m = meta(STRIPE_AS, "https://access.stripe.com/mcp");
    assert_eq!(
        resolve(&NetHttp(net), &v, &m).unwrap_err(),
        McpAuthError::RegistrationRejected(400)
    );
    assert!(v
        .custody_get(&dcr_slot("https://access.stripe.com/mcp"))
        .is_err());
}

#[test]
fn the_fixed_relay_rows_are_github_slack_and_notion_with_public_ids_only() {
    let names: Vec<&str> = FIXED_CLIENTS.iter().map(|r| r.provider).collect();
    assert_eq!(names, vec!["github", "slack", "notion"]);
    for row in FIXED_CLIENTS {
        assert!(
            row.client_id.is_some() != row.reason.is_some(),
            "{}: id xor reason",
            row.provider
        );
        assert!(row.issuer.starts_with("https://"));
    }
    let v = vault();
    let gh = fixed_metadata("https://github.com/login/oauth").unwrap();
    let c = resolve(&NetHttp(Net::new()), &v, &gh).unwrap();
    assert_eq!(c.kind(), "relay");
    assert_eq!(
        c.client_id(),
        crate::connections::oauth_client::GITHUB_CLIENT_ID
    );
}

#[test]
fn an_issuer_without_published_metadata_uses_only_a_fixed_rows_endpoints() {
    // GitHub's AS metadata was not fetched in the matrix; the fixed row supplies the endpoints.
    let net = Net::new();
    net.on(GITHUB_MCP, 401, Some(GITHUB_WWW), "");
    net.on(GITHUB_PRM_URL, 200, None, GITHUB_PRM);
    let d = discover(&NetHttp(net.clone()), &url(GITHUB_MCP), &fixed_metadata).unwrap();
    assert_eq!(d.resource_id, "https://api.githubcopilot.com/mcp");
    assert_eq!(
        d.auth_server.token_endpoint.as_str(),
        "https://github.com/login/oauth/access_token"
    );
    // The specification URLs were all tried first.
    let gets = net.sent().into_iter().filter(|s| s.kind == "get").count();
    assert_eq!(gets, 1 + 3, "PRM, then the three path-issuer metadata URLs");
    // An issuer with no row and no metadata is refused.
    let net = Net::new();
    net.on(LINEAR_MCP, 401, Some(LINEAR_WWW), "");
    net.on(LINEAR_PRM_URL, 200, None, LINEAR_PRM);
    assert!(matches!(
        discover(&NetHttp(net), &url(LINEAR_MCP), &fixed_metadata).unwrap_err(),
        McpAuthError::BadMetadata(_)
    ));
}
