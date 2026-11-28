//! L6-WP07: the Marketplace lists the verified brand servers before the catalog key exists, and
//! says plainly that the signed catalog is not available yet.
use super::brand::BRAND_SERVERS;
use super::{catalog_view, normalize_url, TrustBadge, NO_KEY_MESSAGE};
use citrate_components::catalog::{catalog_trust_root, CatalogError};
use citrate_components::key::TrustRoot;
use std::collections::BTreeSet;

fn test_root() -> TrustRoot {
    // A throwaway key made inside the test (minisign line: "Ed" + key id + Ed25519 key).
    use base64::Engine as _;
    let sk = ed25519_dalek::SigningKey::from_bytes(&rand::random::<[u8; 32]>());
    let mut line = b"Ed".to_vec();
    line.extend_from_slice(&rand::random::<[u8; 8]>());
    line.extend_from_slice(sk.verifying_key().as_bytes());
    TrustRoot::from_base64(&base64::engine::general_purpose::STANDARD.encode(line))
        .expect("test key parses")
}

#[test]
fn no_key_lists_the_pinned_brand_servers_and_says_so() {
    let v = catalog_view(&Err(CatalogError::KeyNotIssued), &BTreeSet::new());
    assert_eq!(v.source, "pinned");
    assert!(!v.signed);
    let r = v.refused.as_ref().expect("the signed section is refused");
    assert_eq!(r.code, "no_key");
    assert_eq!(
        r.message,
        "Signed catalog not available yet: signing key not issued"
    );
    assert_eq!(r.message, NO_KEY_MESSAGE);
    assert_eq!(v.entries.len(), BRAND_SERVERS.len());
    for (e, s) in v.entries.iter().zip(BRAND_SERVERS) {
        assert_eq!(e.entry_id, format!("brand:{}", s.id));
        assert_eq!(e.trust, TrustBadge::VerifiedBrand);
        assert_eq!(e.remote_url.as_deref(), Some(s.url));
        assert_eq!(e.reviewed_at.as_deref(), Some(s.checked));
        assert!(e.packages.is_empty());
        assert!(!e.installed);
        assert!(!e.write_tools_default);
    }
}

#[test]
fn the_shipped_build_says_the_key_is_not_issued() {
    // The key ceremony has not happened: the shipped slot is empty, so a release build shows
    // the pinned list and the honest message, never a signed state.
    let v = catalog_view(&catalog_trust_root(), &BTreeSet::new());
    assert!(!v.signed);
    assert_eq!(v.refused.as_ref().map(|r| r.code.as_str()), Some("no_key"));
}

#[test]
fn the_view_serializes_in_the_frozen_shape() {
    let v = catalog_view(&Err(CatalogError::KeyNotIssued), &BTreeSet::new());
    let j = serde_json::to_value(&v).expect("serializes");
    assert_eq!(j["source"], "pinned");
    assert_eq!(j["signed"], false);
    assert_eq!(j["refused"]["code"], "no_key");
    let first = &j["entries"][0];
    for k in [
        "entryId",
        "name",
        "summary",
        "publisher",
        "trust",
        "remoteUrl",
        "packages",
        "auth",
        "installed",
        "reviewedAt",
        "sourceUrl",
        "registration",
        "writeToolsDefault",
    ] {
        assert!(first.get(k).is_some(), "entry lacks {k}: {first}");
    }
    assert_eq!(first["trust"], "verified_brand");
    let auths: BTreeSet<String> = j["entries"]
        .as_array()
        .expect("entries")
        .iter()
        .filter_map(|e| e["auth"].as_str().map(String::from))
        .collect();
    assert!(
        auths.iter().all(|a| a == "oauth" || a == "none"),
        "{auths:?}"
    );
}

#[test]
fn a_bad_key_is_reported_not_hidden() {
    let v = catalog_view(
        &Err(CatalogError::BadKey("does not parse".into())),
        &BTreeSet::new(),
    );
    assert!(!v.signed);
    let r = v.refused.expect("refused");
    assert_eq!(r.code, "bad_key");
    assert!(r.message.contains("does not parse"), "{}", r.message);
    assert_eq!(v.entries.len(), BRAND_SERVERS.len());
}

#[test]
fn a_key_alone_does_not_claim_a_signed_catalog() {
    // With a key but no verified document (the fetch client is L6-WP08c), nothing is signed.
    let v = catalog_view(&Ok(test_root()), &BTreeSet::new());
    assert_eq!(v.source, "pinned");
    assert!(!v.signed);
    assert_eq!(
        v.refused.as_ref().map(|r| r.code.as_str()),
        Some("not_fetched")
    );
}

#[test]
fn installed_follows_the_saved_servers() {
    let mut saved = BTreeSet::new();
    saved.insert(normalize_url("https://mcp.notion.com/mcp/"));
    saved.insert(normalize_url("https://unrelated.example/mcp"));
    let v = catalog_view(&Err(CatalogError::KeyNotIssued), &saved);
    let installed: Vec<&str> = v
        .entries
        .iter()
        .filter(|e| e.installed)
        .map(|e| e.entry_id.as_str())
        .collect();
    assert_eq!(installed, ["brand:notion"]);
}

#[test]
fn normalize_url_drops_one_trailing_slash_and_lowercases_the_host() {
    assert_eq!(
        normalize_url("https://MCP.Notion.com/mcp/"),
        "https://mcp.notion.com/mcp"
    );
    assert_eq!(
        normalize_url("https://mcp.vercel.com/"),
        "https://mcp.vercel.com"
    );
    // The path keeps its case.
    assert_eq!(
        normalize_url("https://a.example/Mcp"),
        "https://a.example/Mcp"
    );
}
