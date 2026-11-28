//! L6-WP08a: the signed MCP subregistry (the Citrate curated mirror of the official MCP Registry).
//!
//! The document is refused unless the catalog key slot holds a key, the signature is ours and made
//! for the subregistry (trusted-comment domain `citrate-mcp-subregistry-v1`), the bytes fit the cap,
//! the document is fresh, it is not older than the one already seen, and every
//! entry carries a known Citrate trust value and only https remotes or pinned packages.
//!
//! Every signature here comes from a throwaway key generated inside the test. No production key
//! exists yet: the catalog key comes from the owner's key ceremony.
mod common;

use citrate_components::catalog::{
    verify_catalog, CatalogError, CATALOG_DOMAIN, PRODUCTION_CATALOG_PUBKEY,
};
use citrate_components::key::{app_updater_key_line, ed25519_public_key, TrustRoot};
use citrate_components::mcp_subregistry::{
    subregistry_trust_root, subregistry_trust_root_from, verify_subregistry, RegistryType,
    SeenSubregistry, SubregistryError, Transport, Trust, MAX_SUBREGISTRY_BYTES, SUBREGISTRY_DOMAIN,
};
use common::*;

const DESC: &str = "Search and edit pages in a workspace.";

fn meta(trust: &str) -> String {
    format!(
        r#""_meta":{{"ai.citrate/curation":{{"trust":"{trust}","reviewedAt":"2026-11-28","toolSnapshotSha256":"{}"}}}}"#,
        "cd".repeat(32)
    )
}

fn remote_entry(name: &str, url: &str, trust: &str) -> String {
    format!(
        r#"{{"name":"{name}","title":"Server {name}","description":"{DESC}","version":"1.0.0","remotes":[{{"type":"streamable-http","url":"{url}"}}],{}}}"#,
        meta(trust)
    )
}

fn package_entry(name: &str, registry: &str, ident: &str, version: &str) -> String {
    format!(
        r#"{{"name":"{name}","title":"Package {name}","description":"{DESC}","version":"2.1.0","packages":[{{"registryType":"{registry}","identifier":"{ident}","version":"{version}"}}],{}}}"#,
        meta("curated")
    )
}

fn doc(seq: u64, entries: &[String]) -> String {
    format!(
        r#"{{"schema":1,"sequence":{seq},"issued_at":{},"expires_at":{},"servers":[{}]}}"#,
        NOW - 60,
        NOW + 7 * DAY,
        entries.join(",")
    )
}

fn good(seq: u64) -> String {
    doc(
        seq,
        &[
            remote_entry(
                "com.notion/mcp",
                "https://mcp.notion.com/mcp",
                "verified_brand",
            ),
            package_entry(
                "io.github.example/files",
                "npm",
                "@example/files-mcp",
                "0.4.2",
            ),
        ],
    )
}

fn sign(key: &TestKey, json: &str) -> String {
    key.sign(json.as_bytes(), &format!("{SUBREGISTRY_DOMAIN} seq=4"))
}

fn root(key: &TestKey) -> TrustRoot {
    subregistry_trust_root_from(Some(&key.public_b64())).expect("test key is usable")
}

fn refused(json: &str) -> SubregistryError {
    let key = TestKey::generate();
    verify_subregistry(json.as_bytes(), &sign(&key, json), &root(&key), None, NOW).unwrap_err()
}

fn assert_malformed(err: SubregistryError, needle: &str) {
    match err {
        SubregistryError::Malformed(m) => assert!(m.contains(needle), "{m:?} lacks {needle:?}"),
        other => panic!("expected Malformed({needle}), got {other:?}"),
    }
}

// --- The key: pending the ceremony, nothing verifies. ---

#[test]
fn key_not_issued_first() {
    assert!(
        PRODUCTION_CATALOG_PUBKEY.is_none(),
        "the catalog key comes from the owner's key ceremony, which has not happened"
    );
    let err = subregistry_trust_root().unwrap_err();
    assert_eq!(err, SubregistryError::KeyNotIssued);
    assert_eq!(
        err.to_string(),
        "Signed catalog not available yet: signing key not issued"
    );
    // An empty slot refuses before any key material is read.
    assert_eq!(
        subregistry_trust_root_from(None).unwrap_err(),
        SubregistryError::KeyNotIssued
    );
}

#[test]
fn no_key_line_is_compiled_into_the_verifier() {
    // A placeholder key in the source would make release builds trust something. Every
    // base64-looking token in the module must fail to decode as a minisign public key.
    let src = include_str!("../src/mcp_subregistry.rs");
    for tok in src.split(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '='))) {
        assert!(
            ed25519_public_key(tok).is_none(),
            "a minisign public key line is compiled into mcp_subregistry.rs: {tok}"
        );
    }
}

#[test]
fn the_app_updater_key_is_refused_as_the_catalog_key() {
    let app = app_updater_key_line().expect("tauri.conf.json has the updater key");
    assert!(matches!(
        subregistry_trust_root_from(Some(&app)).unwrap_err(),
        SubregistryError::BadKey(_)
    ));
}

// --- A good document signed with the test key. ---

#[test]
fn a_subregistry_signed_by_the_test_key_verifies() {
    let key = TestKey::generate();
    let json = good(4);
    let v = verify_subregistry(json.as_bytes(), &sign(&key, &json), &root(&key), None, NOW)
        .expect("verifies");
    let d = v.document();
    assert_eq!(d.sequence, 4);
    assert_eq!(d.servers.len(), 2);
    let notion = &d.servers[0];
    assert_eq!(notion.name, "com.notion/mcp");
    assert_eq!(notion.remotes[0].transport, Transport::StreamableHttp);
    assert_eq!(notion.remotes[0].url, "https://mcp.notion.com/mcp");
    assert_eq!(notion.meta.curation.trust, Trust::VerifiedBrand);
    assert_eq!(notion.meta.curation.reviewed_at, "2026-11-28");
    let pkg = &d.servers[1];
    assert_eq!(pkg.packages[0].registry_type, RegistryType::Npm);
    assert_eq!(pkg.packages[0].version, "0.4.2");
    assert_eq!(pkg.meta.curation.trust, Trust::Curated);
    assert_eq!(v.digest_hex(), sha256_hex(json.as_bytes()));
    assert_eq!(v.seen().sequence, 4);
}

// --- Signature, purpose and size. ---

#[test]
fn a_tampered_subregistry_is_refused() {
    let key = TestKey::generate();
    let json = good(4);
    let sig = sign(&key, &json);
    let tampered = json.replace("https://mcp.notion.com/mcp", "https://mcp.notion.evil/mcp");
    assert_ne!(tampered, json);
    let err = verify_subregistry(tampered.as_bytes(), &sig, &root(&key), None, NOW).unwrap_err();
    assert!(
        matches!(err, SubregistryError::SignatureInvalid(_)),
        "{err:?}"
    );
}

#[test]
fn a_subregistry_signed_by_another_key_is_refused() {
    let key = TestKey::generate();
    let other = TestKey::generate();
    let json = good(4);
    let err = verify_subregistry(
        json.as_bytes(),
        &sign(&other, &json),
        &root(&key),
        None,
        NOW,
    )
    .unwrap_err();
    assert!(
        matches!(err, SubregistryError::SignatureInvalid(_)),
        "{err:?}"
    );
}

#[test]
fn a_garbage_signature_is_refused() {
    let key = TestKey::generate();
    let json = good(4);
    let err =
        verify_subregistry(json.as_bytes(), "not a signature", &root(&key), None, NOW).unwrap_err();
    assert!(
        matches!(err, SubregistryError::SignatureInvalid(_)),
        "{err:?}"
    );
}

#[test]
fn a_legacy_unhashed_signature_is_refused() {
    let key = TestKey::generate();
    let json = good(4);
    let sig = key.sign_legacy(json.as_bytes(), SUBREGISTRY_DOMAIN);
    assert!(verify_subregistry(json.as_bytes(), &sig, &root(&key), None, NOW).is_err());
}

#[test]
fn store_catalog_signature_refused_for_subregistry() {
    let key = TestKey::generate();
    let json = good(4);
    let sig = key.sign(json.as_bytes(), &format!("{CATALOG_DOMAIN} seq=4"));
    let err = verify_subregistry(json.as_bytes(), &sig, &root(&key), None, NOW).unwrap_err();
    assert_eq!(err, SubregistryError::WrongPurpose);
}

#[test]
fn a_domain_that_only_starts_with_ours_is_refused() {
    let key = TestKey::generate();
    let json = good(4);
    for tc in [
        format!("{SUBREGISTRY_DOMAIN}x seq=4"),
        "citrate-mcp-subregistry-v2 seq=4".to_string(),
        format!("seq=4 {SUBREGISTRY_DOMAIN}"),
        "citrate-components-manifest seq=4".to_string(),
    ] {
        let sig = key.sign(json.as_bytes(), &tc);
        assert_eq!(
            verify_subregistry(json.as_bytes(), &sig, &root(&key), None, NOW).unwrap_err(),
            SubregistryError::WrongPurpose,
            "trusted comment {tc:?}"
        );
    }
}

#[test]
fn subregistry_signature_refused_for_store_catalog() {
    let key = TestKey::generate();
    let catalog = format!(
        r#"{{"schema":1,"sequence":1,"issued_at":{},"expires_at":{},"listings":[]}}"#,
        NOW - 60,
        NOW + DAY
    );
    let sig = key.sign(catalog.as_bytes(), &format!("{SUBREGISTRY_DOMAIN} seq=1"));
    let tr = TrustRoot::from_base64(&key.public_b64()).expect("test key parses");
    let err = verify_catalog(catalog.as_bytes(), &sig, &tr, None, NOW).unwrap_err();
    assert_eq!(err, CatalogError::WrongPurpose);
}

#[test]
fn size_cap_before_parse() {
    let key = TestKey::generate();
    // Over the cap with a signature that would not verify and bytes that are not JSON: the size
    // refusal comes first, so neither the signature nor the parser ever saw them.
    let big = vec![b'{'; MAX_SUBREGISTRY_BYTES + 1];
    assert_eq!(
        verify_subregistry(&big, "garbage", &root(&key), None, NOW).unwrap_err(),
        SubregistryError::TooLarge {
            limit: MAX_SUBREGISTRY_BYTES
        }
    );
    // Over the cap even with a good signature.
    let mut json = good(4);
    json.push_str(&" ".repeat(MAX_SUBREGISTRY_BYTES));
    let sig = sign(&key, &json);
    assert_eq!(
        verify_subregistry(json.as_bytes(), &sig, &root(&key), None, NOW).unwrap_err(),
        SubregistryError::TooLarge {
            limit: MAX_SUBREGISTRY_BYTES
        }
    );
}

// --- Freshness and rollback. ---

#[test]
fn expired_refused() {
    let key = TestKey::generate();
    let json = good(4);
    let sig = sign(&key, &json);
    assert_eq!(
        verify_subregistry(json.as_bytes(), &sig, &root(&key), None, NOW + 8 * DAY).unwrap_err(),
        SubregistryError::Expired
    );
    assert_eq!(
        verify_subregistry(json.as_bytes(), &sig, &root(&key), None, NOW - DAY).unwrap_err(),
        SubregistryError::NotYetValid
    );
}

#[test]
fn a_lifetime_over_the_maximum_is_refused() {
    let json = good(4).replace(
        &format!("\"expires_at\":{}", NOW + 7 * DAY),
        &format!("\"expires_at\":{}", NOW + 60 * DAY),
    );
    assert_malformed(refused(&json), "lifetime");
}

#[test]
fn rollback_refused() {
    let key = TestKey::generate();
    let json = good(4);
    let sig = sign(&key, &json);
    let seen = SeenSubregistry {
        sequence: 9,
        digest_hex: "00".repeat(32),
    };
    assert_eq!(
        verify_subregistry(json.as_bytes(), &sig, &root(&key), Some(&seen), NOW).unwrap_err(),
        SubregistryError::Rollback { seen: 9, got: 4 }
    );
    // The same sequence is accepted again only for the same bytes.
    let other_bytes = SeenSubregistry {
        sequence: 4,
        digest_hex: "00".repeat(32),
    };
    assert_eq!(
        verify_subregistry(json.as_bytes(), &sig, &root(&key), Some(&other_bytes), NOW)
            .unwrap_err(),
        SubregistryError::Rollback { seen: 4, got: 4 }
    );
    let same = SeenSubregistry {
        sequence: 4,
        digest_hex: sha256_hex(json.as_bytes()),
    };
    assert!(verify_subregistry(json.as_bytes(), &sig, &root(&key), Some(&same), NOW).is_ok());
}

// --- Parsing and entry rules. ---

#[test]
fn malformed_json_is_refused_after_the_signature() {
    assert_malformed(refused("{\"schema\":1,"), "json");
    assert_malformed(refused("[]"), "json");
    assert_malformed(refused(""), "json");
}

#[test]
fn an_unknown_schema_is_refused() {
    assert_malformed(
        refused(&good(4).replace("\"schema\":1", "\"schema\":2")),
        "schema",
    );
}

#[test]
fn unknown_fields_are_refused() {
    let json = good(4).replace("\"servers\":", "\"extra\":true,\"servers\":");
    assert_malformed(refused(&json), "json");
}

#[test]
fn unknown_trust_value_refused() {
    for t in [
        "registry_unreviewed",
        "member_added",
        "verified",
        "VerifiedBrand",
        "",
    ] {
        let json = doc(
            1,
            &[remote_entry(
                "com.notion/mcp",
                "https://mcp.notion.com/mcp",
                t,
            )],
        );
        assert_malformed(refused(&json), "json");
    }
}

#[test]
fn third_party_entries_allowed_only_with_trust_field() {
    // A third-party entry (not Citrate's, unlike the Store catalog) is accepted with its trust
    // field: the curated package entry in `good` is one.
    let key = TestKey::generate();
    let json = good(4);
    assert!(
        verify_subregistry(json.as_bytes(), &sign(&key, &json), &root(&key), None, NOW).is_ok()
    );
    // Without `_meta`, or with `_meta` but no Citrate curation record, it is refused.
    let no_meta = format!(
        r#"{{"name":"io.github.example/files","title":"Files","description":"{DESC}","version":"1.0.0","remotes":[{{"type":"streamable-http","url":"https://files.example.com/mcp"}}]}}"#
    );
    assert_malformed(refused(&doc(1, &[no_meta])), "json");
    let other_meta = format!(
        r#"{{"name":"io.github.example/files","title":"Files","description":"{DESC}","version":"1.0.0","remotes":[{{"type":"streamable-http","url":"https://files.example.com/mcp"}}],"_meta":{{"io.modelcontextprotocol.registry/official":{{"status":"active"}}}}}}"#
    );
    assert_malformed(refused(&doc(1, &[other_meta])), "json");
}

#[test]
fn non_https_endpoints_are_refused() {
    for url in [
        "http://mcp.notion.com/mcp",
        "ws://mcp.notion.com/mcp",
        "file:///etc/passwd",
        "https://",
        "https://user@mcp.notion.com/mcp",
        "https://mcp.notion.com/mcp with space",
        "HTTPS://mcp.notion.com/mcp",
        "",
    ] {
        let json = doc(1, &[remote_entry("com.notion/mcp", url, "verified_brand")]);
        assert_malformed(refused(&json), "https");
    }
}

#[test]
fn an_entry_needs_a_remote_or_a_pinned_package() {
    let bare = format!(
        r#"{{"name":"com.example/bare","title":"Bare","description":"{DESC}","version":"1.0.0",{}}}"#,
        meta("curated")
    );
    assert_malformed(refused(&doc(1, &[bare])), "remote or a package");
}

#[test]
fn packages_must_be_pinned() {
    for v in ["latest", "^1.2.0", "~1.2", ">=1", "*", "1.x || 2", ""] {
        let json = doc(
            1,
            &[package_entry("com.example/pkg", "npm", "@example/pkg", v)],
        );
        assert_malformed(refused(&json), "pinned");
    }
    let unknown_registry = doc(
        1,
        &[package_entry(
            "com.example/pkg",
            "cargo",
            "example",
            "1.0.0",
        )],
    );
    assert_malformed(refused(&unknown_registry), "json");
    let bad_ident = doc(
        1,
        &[package_entry(
            "com.example/pkg",
            "npm",
            "pkg; rm -rf ~",
            "1.0.0",
        )],
    );
    assert_malformed(refused(&bad_ident), "identifier");
}

#[test]
fn names_are_registry_names_and_unique() {
    for name in [
        "notion",
        "com.notion/",
        "/mcp",
        "com notion/mcp",
        "com.notion/mcp/x",
        "-a/b",
    ] {
        let json = doc(
            1,
            &[remote_entry(name, "https://mcp.notion.com/mcp", "curated")],
        );
        assert_malformed(refused(&json), "name");
    }
    let dup = doc(
        1,
        &[
            remote_entry("com.notion/mcp", "https://mcp.notion.com/mcp", "curated"),
            remote_entry("com.notion/mcp", "https://mcp.notion.com/mcp2", "curated"),
        ],
    );
    assert_malformed(refused(&dup), "duplicate");
}

#[test]
fn curation_fields_are_checked() {
    let bad_date = good(4).replace(
        "\"reviewedAt\":\"2026-11-28\"",
        "\"reviewedAt\":\"yesterday\"",
    );
    assert_malformed(refused(&bad_date), "reviewedAt");
    let bad_sha = good(4).replace(&"cd".repeat(32), &"CD".repeat(32));
    assert_malformed(refused(&bad_sha), "toolSnapshotSha256");
}

#[test]
fn text_fields_are_bounded_and_free_of_control_characters() {
    let long = good(4).replace(DESC, &"x".repeat(2000));
    assert_malformed(refused(&long), "description");
    let ctl = good(4).replace("Server com.notion/mcp", "Server\\u0007");
    assert_malformed(refused(&ctl), "title");
}

#[test]
fn expires_must_follow_issued() {
    let json = good(4).replace(
        &format!("\"expires_at\":{}", NOW + 7 * DAY),
        &format!("\"expires_at\":{}", NOW - 60),
    );
    assert_malformed(refused(&json), "expires_at");
}
