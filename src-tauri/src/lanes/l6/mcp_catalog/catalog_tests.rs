//! L6-WP07: the Marketplace lists the verified brand servers before the catalog key exists, and
//! says plainly that the signed catalog is not available yet.
use super::brand::BRAND_SERVERS;
use super::{catalog_view, normalize_url, saved_endpoint_urls, TrustBadge, NO_KEY_MESSAGE};
use citrate_components::catalog::{catalog_trust_root, CatalogError};
use citrate_components::key::TrustRoot;
use std::collections::BTreeSet;
use std::path::PathBuf;

/// No saved servers (the lookup succeeded and found none).
fn none_saved() -> Result<BTreeSet<String>, String> {
    Ok(BTreeSet::new())
}

fn tmp_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let p = std::env::temp_dir().join(format!(
        "citrate-core-mcp-catalog-{tag}-{nanos}-{:?}",
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&p).expect("mk tmpdir");
    p
}

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
    let v = catalog_view(&Err(CatalogError::KeyNotIssued), &none_saved());
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
        assert_eq!(e.installed, Some(false));
        assert!(!e.write_tools_default);
    }
}

#[test]
fn the_shipped_build_says_the_key_is_not_issued() {
    // The key ceremony has not happened: the shipped slot is empty, so a release build shows
    // the pinned list and the honest message, never a signed state.
    let v = catalog_view(&catalog_trust_root(), &none_saved());
    assert!(!v.signed);
    assert_eq!(v.refused.as_ref().map(|r| r.code.as_str()), Some("no_key"));
}

#[test]
fn the_view_serializes_in_the_frozen_shape() {
    let v = catalog_view(&Err(CatalogError::KeyNotIssued), &none_saved());
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
        &none_saved(),
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
    let v = catalog_view(&Ok(test_root()), &none_saved());
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
    let v = catalog_view(&Err(CatalogError::KeyNotIssued), &Ok(saved));
    let installed: Vec<&str> = v
        .entries
        .iter()
        .filter(|e| e.installed == Some(true))
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

#[test]
fn normalize_url_drops_the_default_https_port() {
    // mcp_server_save keeps an explicit :443, which is the same endpoint as the pinned URL.
    assert_eq!(
        normalize_url("https://mcp.notion.com:443/mcp"),
        normalize_url("https://mcp.notion.com/mcp")
    );
    assert_eq!(
        normalize_url("https://MCP.Notion.com:443/mcp/"),
        "https://mcp.notion.com/mcp"
    );
    // A different port, path or query is a different endpoint.
    assert_ne!(
        normalize_url("https://mcp.notion.com:8443/mcp"),
        normalize_url("https://mcp.notion.com/mcp")
    );
    assert_ne!(
        normalize_url("https://mcp.notion.com/mcp?x=1"),
        normalize_url("https://mcp.notion.com/mcp")
    );
    assert_ne!(
        normalize_url("https://mcp.notion.com/sse"),
        normalize_url("https://mcp.notion.com/mcp")
    );
    // Not https: never equal to a pinned https entry.
    assert_ne!(
        normalize_url("http://mcp.notion.com/mcp"),
        normalize_url("https://mcp.notion.com/mcp")
    );
}

#[test]
fn a_saved_endpoint_with_the_default_port_marks_the_entry_installed() {
    let mut saved = BTreeSet::new();
    saved.insert(normalize_url("https://mcp.notion.com:443/mcp"));
    let v = catalog_view(&Err(CatalogError::KeyNotIssued), &Ok(saved));
    let notion = v
        .entries
        .iter()
        .find(|e| e.entry_id == "brand:notion")
        .expect("notion is pinned");
    assert_eq!(notion.installed, Some(true));
}

/// A registry as a version before RC52 wrote it: env values in plain text, and the old
/// allowlist file beside it.
fn write_legacy_registry(dir: &std::path::Path) -> (String, String) {
    let reg = serde_json::json!({ "servers": [
        { "name": "notion", "transport": "http", "url": "https://mcp.notion.com:443/mcp/",
          "env": { "NOTES_TOKEN": "tok-abc-123" }, "enabled": true },
        { "name": "local", "transport": "stdio", "command": "/bin/echo" }
    ]})
    .to_string();
    let allow = serde_json::json!({ "servers": [{"name": "notion", "env": {"NOTES_TOKEN": "tok-abc-123"}}] })
        .to_string();
    std::fs::write(dir.join(crate::mcp_servers::REGISTRY_FILE), &reg).expect("write registry");
    std::fs::write(dir.join(crate::mcp_servers::LEGACY_ALLOWLIST_FILE), &allow)
        .expect("write allowlist");
    (reg, allow)
}

#[test]
fn the_installed_lookup_never_migrates_or_rewrites_a_legacy_registry() {
    // Listing the catalog must not move secrets to the keychain, rewrite the registry or remove
    // the old allowlist: that is McpStore::load's job, not a read-only view's. The lookup takes
    // only a directory, so it has no keychain to write to.
    let dir = tmp_dir("legacy");
    let (reg, allow) = write_legacy_registry(&dir);
    let urls = saved_endpoint_urls(&dir).expect("a legacy registry still parses");
    assert_eq!(
        urls,
        BTreeSet::from([normalize_url("https://mcp.notion.com/mcp")])
    );
    let reg_after = std::fs::read_to_string(dir.join(crate::mcp_servers::REGISTRY_FILE))
        .expect("registry still there");
    assert_eq!(reg_after, reg, "the registry was rewritten");
    let allow_after = std::fs::read_to_string(dir.join(crate::mcp_servers::LEGACY_ALLOWLIST_FILE))
        .expect("the old allowlist was removed");
    assert_eq!(allow_after, allow, "the old allowlist was rewritten");
    let names: Vec<String> = std::fs::read_dir(&dir)
        .expect("ls")
        .filter_map(|e| e.ok().map(|e| e.file_name().to_string_lossy().into_owned()))
        .collect();
    assert_eq!(names.len(), 2, "the lookup wrote a file: {names:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_missing_registry_means_nothing_is_installed() {
    let dir = tmp_dir("missing");
    assert_eq!(saved_endpoint_urls(&dir), Ok(BTreeSet::new()));
    assert!(
        std::fs::read_dir(&dir).expect("ls").next().is_none(),
        "the lookup created a file"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_damaged_registry_keeps_the_pinned_catalog_and_reports_installed_unknown() {
    let dir = tmp_dir("damaged");
    std::fs::write(dir.join(crate::mcp_servers::REGISTRY_FILE), "{ not json").expect("write");
    let lookup = saved_endpoint_urls(&dir);
    assert!(lookup.is_err(), "a damaged file is not silently empty");
    let v = catalog_view(&Err(CatalogError::KeyNotIssued), &lookup);
    // The pinned entries and the key status survive the failed lookup.
    assert_eq!(v.entries.len(), BRAND_SERVERS.len());
    assert_eq!(v.refused.as_ref().map(|r| r.code.as_str()), Some("no_key"));
    assert!(v.entries.iter().all(|e| e.installed.is_none()));
    let why = v.installed_unavailable.as_deref().expect("says why");
    assert!(why.contains("could not be read"), "{why}");
    let j = serde_json::to_value(&v).expect("serializes");
    assert!(j["entries"][0]["installed"].is_null());
    assert!(j["installedUnavailable"].is_string());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_successful_lookup_reports_no_installed_error() {
    let v = catalog_view(&Err(CatalogError::KeyNotIssued), &none_saved());
    assert!(v.installed_unavailable.is_none());
    let j = serde_json::to_value(&v).expect("serializes");
    assert!(j.get("installedUnavailable").is_some());
}
