//! K5a: the signed Store catalog is refused unless the production key slot holds a key, the
//! signature is ours and made for the catalog, and every listing field checks out. Only
//! first-party listings are accepted in this version.
mod common;

use citrate_components::catalog::{
    catalog_trust_root, verify_catalog, CatalogError, ListingKind, ListingTier, SeenCatalog,
    CATALOG_DOMAIN, PRODUCTION_CATALOG_PUBKEY,
};
use citrate_components::key::TrustRoot;
use common::*;

fn listing(id: &str, kind: &str, extra: &str) -> String {
    format!(
        r#"{{"id":"{id}","kind":"{kind}","name":"Listing {id}","summary":"What it does.","version":"1.0.0","publisher":"Citrate","first_party":true,"tier":"member","artifact":{{"url":"https://store.test/{id}.json","sha256":"{sha}","size":120}},"permissions":[{{"id":"local_model","reason":"Runs on your local model."}}]{extra}}}"#,
        sha = "ab".repeat(32),
    )
}

const BUDGET: &str = r#","budget":{"max_runs_per_day":2,"max_tokens_per_day":8000,"max_tokens_per_run":4000,"max_spend_salt":"0"}"#;

fn catalog(seq: u64, listings: &[String]) -> String {
    format!(
        r#"{{"schema":1,"sequence":{seq},"issued_at":{},"expires_at":{},"listings":[{}]}}"#,
        NOW - 60,
        NOW + 7 * DAY,
        listings.join(",")
    )
}

fn good() -> String {
    catalog(
        4,
        &[
            listing("daily-brief", "daemon", BUDGET),
            listing("gemma-small", "model", ""),
        ],
    )
}

fn sign(key: &TestKey, json: &str) -> String {
    key.sign(json.as_bytes(), &format!("{CATALOG_DOMAIN} seq=4"))
}

fn root(key: &TestKey) -> TrustRoot {
    TrustRoot::from_base64(&key.public_b64()).expect("test key parses")
}

#[test]
fn the_production_catalog_key_slot_is_empty_and_fails_closed() {
    assert!(
        PRODUCTION_CATALOG_PUBKEY.is_none(),
        "the catalog key comes from the genesis key ceremony"
    );
    let err = catalog_trust_root().unwrap_err();
    assert_eq!(err, CatalogError::KeyNotIssued);
    assert_eq!(
        err.to_string(),
        "Store catalog not available yet: signing key not issued"
    );
}

#[test]
fn a_catalog_signed_by_the_test_key_verifies() {
    let key = TestKey::generate();
    let json = good();
    let vc = verify_catalog(json.as_bytes(), &sign(&key, &json), &root(&key), None, NOW)
        .expect("verifies");
    let c = vc.catalog();
    assert_eq!(c.sequence, 4);
    assert_eq!(c.listings.len(), 2);
    assert_eq!(c.listings[0].kind, ListingKind::Daemon);
    assert_eq!(c.listings[0].tier, ListingTier::Member);
    assert!(c.listings[0].first_party);
    assert_eq!(
        c.listings[0].budget.as_ref().map(|b| b.max_runs_per_day),
        Some(2)
    );
    assert_eq!(c.listings[1].kind, ListingKind::Model);
    assert_eq!(vc.digest_hex(), sha256_hex(json.as_bytes()));
}

#[test]
fn every_listing_kind_parses() {
    let key = TestKey::generate();
    let kinds = ["app", "daemon", "widget", "skill", "model", "mcp_server"];
    let ls: Vec<String> = kinds
        .iter()
        .map(|k| {
            listing(
                &format!("l-{}", k.replace('_', "-")),
                k,
                if *k == "daemon" { BUDGET } else { "" },
            )
        })
        .collect();
    let json = catalog(1, &ls);
    let vc = verify_catalog(json.as_bytes(), &sign(&key, &json), &root(&key), None, NOW).unwrap();
    let got: Vec<ListingKind> = vc.catalog().listings.iter().map(|l| l.kind).collect();
    assert_eq!(
        got,
        vec![
            ListingKind::App,
            ListingKind::Daemon,
            ListingKind::Widget,
            ListingKind::Skill,
            ListingKind::Model,
            ListingKind::McpServer
        ]
    );
}

#[test]
fn a_tampered_catalog_is_refused() {
    let key = TestKey::generate();
    let json = good();
    let sig = sign(&key, &json);
    let tampered = json.replace("\"tier\":\"member\"", "\"tier\":\"public\"");
    assert_ne!(tampered, json);
    let err = verify_catalog(tampered.as_bytes(), &sig, &root(&key), None, NOW).unwrap_err();
    assert!(matches!(err, CatalogError::SignatureInvalid(_)), "{err:?}");
}

#[test]
fn a_catalog_signed_by_another_key_is_refused() {
    let key = TestKey::generate();
    let other = TestKey::generate();
    let json = good();
    let err = verify_catalog(
        json.as_bytes(),
        &sign(&other, &json),
        &root(&key),
        None,
        NOW,
    )
    .unwrap_err();
    assert!(matches!(err, CatalogError::SignatureInvalid(_)), "{err:?}");
}

#[test]
fn a_component_manifest_signature_does_not_pass_as_a_catalog_signature() {
    let key = TestKey::generate();
    let json = good();
    let sig = key.sign(json.as_bytes(), "citrate-components-manifest seq=4");
    let err = verify_catalog(json.as_bytes(), &sig, &root(&key), None, NOW).unwrap_err();
    assert_eq!(err, CatalogError::WrongPurpose);
}

#[test]
fn a_legacy_unhashed_signature_is_refused() {
    let key = TestKey::generate();
    let json = good();
    let sig = key.sign_legacy(json.as_bytes(), CATALOG_DOMAIN);
    assert!(verify_catalog(json.as_bytes(), &sig, &root(&key), None, NOW).is_err());
}

#[test]
fn an_expired_or_future_catalog_is_refused() {
    let key = TestKey::generate();
    let json = good();
    let sig = sign(&key, &json);
    assert_eq!(
        verify_catalog(json.as_bytes(), &sig, &root(&key), None, NOW + 8 * DAY).unwrap_err(),
        CatalogError::Expired
    );
    assert_eq!(
        verify_catalog(json.as_bytes(), &sig, &root(&key), None, NOW - DAY).unwrap_err(),
        CatalogError::NotYetValid
    );
}

#[test]
fn an_older_catalog_is_refused_as_a_rollback() {
    let key = TestKey::generate();
    let json = good();
    let sig = sign(&key, &json);
    let seen = SeenCatalog {
        sequence: 9,
        digest_hex: "00".repeat(32),
    };
    let err = verify_catalog(json.as_bytes(), &sig, &root(&key), Some(&seen), NOW).unwrap_err();
    assert_eq!(err, CatalogError::Rollback { seen: 9, got: 4 });
    // The same sequence is accepted again only for the same bytes.
    let same = SeenCatalog {
        sequence: 4,
        digest_hex: sha256_hex(json.as_bytes()),
    };
    assert!(verify_catalog(json.as_bytes(), &sig, &root(&key), Some(&same), NOW).is_ok());
}

fn refused(json: &str) -> CatalogError {
    let key = TestKey::generate();
    verify_catalog(json.as_bytes(), &sign(&key, json), &root(&key), None, NOW).unwrap_err()
}

#[test]
fn third_party_listings_are_refused_in_this_version() {
    let l = listing("outside", "app", "").replace("\"first_party\":true", "\"first_party\":false");
    let err = refused(&catalog(1, &[l]));
    assert!(
        matches!(&err, CatalogError::Malformed(m) if m.contains("first-party")),
        "{err:?}"
    );
}

#[test]
fn a_daemon_listing_must_declare_a_zero_spend_budget() {
    let err = refused(&catalog(1, &[listing("d", "daemon", "")]));
    assert!(
        matches!(&err, CatalogError::Malformed(m) if m.contains("budget")),
        "{err:?}"
    );
    let spend = BUDGET.replace("\"max_spend_salt\":\"0\"", "\"max_spend_salt\":\"5\"");
    let err = refused(&catalog(1, &[listing("d", "daemon", &spend)]));
    assert!(
        matches!(&err, CatalogError::Malformed(m) if m.contains("spend")),
        "{err:?}"
    );
    let per_run = BUDGET.replace("\"max_tokens_per_run\":4000", "\"max_tokens_per_run\":9000");
    let err = refused(&catalog(1, &[listing("d", "daemon", &per_run)]));
    assert!(matches!(&err, CatalogError::Malformed(_)), "{err:?}");
}

#[test]
fn field_rules_are_enforced() {
    // Unknown kind, unknown tier, unknown field.
    assert!(matches!(
        refused(&catalog(1, &[listing("x", "plugin", "")])),
        CatalogError::Malformed(_)
    ));
    let t = listing("x", "app", "").replace("\"tier\":\"member\"", "\"tier\":\"gold\"");
    assert!(matches!(
        refused(&catalog(1, &[t])),
        CatalogError::Malformed(_)
    ));
    let u = listing("x", "app", ",\"extra\":1");
    assert!(matches!(
        refused(&catalog(1, &[u])),
        CatalogError::Malformed(_)
    ));
    // Plain http, a bad hash, a zero size.
    let h = listing("x", "app", "").replace("https://store.test", "http://store.test");
    assert!(
        matches!(refused(&catalog(1, &[h])), CatalogError::Malformed(m) if m.contains("https"))
    );
    let s = listing("x", "app", "").replace(&"ab".repeat(32), "abc");
    assert!(
        matches!(refused(&catalog(1, &[s])), CatalogError::Malformed(m) if m.contains("sha256"))
    );
    let z = listing("x", "app", "").replace("\"size\":120", "\"size\":0");
    assert!(matches!(
        refused(&catalog(1, &[z])),
        CatalogError::Malformed(_)
    ));
    // Bad id, duplicate id, a permission id that is not a plain token.
    assert!(matches!(
        refused(&catalog(1, &[listing("../x", "app", "")])),
        CatalogError::Malformed(_)
    ));
    let dup = listing("same", "app", "");
    assert!(
        matches!(refused(&catalog(1, &[dup.clone(), dup])), CatalogError::Malformed(m) if m.contains("duplicate"))
    );
    let p = listing("x", "app", "").replace("\"id\":\"local_model\"", "\"id\":\"Local Model!\"");
    assert!(
        matches!(refused(&catalog(1, &[p])), CatalogError::Malformed(m) if m.contains("permission"))
    );
    // Lifetime longer than the maximum, wrong schema.
    let long = good().replace(
        &format!("\"expires_at\":{}", NOW + 7 * DAY),
        &format!("\"expires_at\":{}", NOW + 90 * DAY),
    );
    assert!(matches!(refused(&long), CatalogError::Malformed(_)));
    assert!(matches!(
        refused(&good().replace("\"schema\":1", "\"schema\":2")),
        CatalogError::Malformed(_)
    ));
}

#[test]
fn nothing_in_the_json_is_read_before_the_signature_verifies() {
    // Not JSON at all, but unsigned: the signature error comes first.
    let key = TestKey::generate();
    let other = TestKey::generate();
    let junk = "not json";
    let err =
        verify_catalog(junk.as_bytes(), &sign(&other, junk), &root(&key), None, NOW).unwrap_err();
    assert!(matches!(err, CatalogError::SignatureInvalid(_)), "{err:?}");
}

#[test]
fn an_oversized_catalog_is_refused_before_any_check() {
    let key = TestKey::generate();
    let big = "x".repeat((1 << 20) + 1);
    let err =
        verify_catalog(big.as_bytes(), &sign(&key, &big), &root(&key), None, NOW).unwrap_err();
    assert!(matches!(err, CatalogError::Malformed(m) if m.contains("larger")));
}

#[test]
fn the_catalog_key_must_differ_from_the_app_updater_key() {
    let app = citrate_components::key::app_updater_key_line().unwrap();
    let err = citrate_components::catalog::catalog_trust_root_from(Some(&app)).unwrap_err();
    assert!(matches!(err, CatalogError::BadKey(_)), "{err:?}");
    let key = TestKey::generate();
    assert!(citrate_components::catalog::catalog_trust_root_from(Some(&key.public_b64())).is_ok());
}

#[test]
fn the_catalog_key_is_refused_when_it_is_the_component_key_under_any_key_id() {
    use citrate_components::catalog::catalog_trust_root_with;
    let component = TestKey::generate();
    let line = component.public_b64();
    // Exactly the component key line.
    let err = catalog_trust_root_with(Some(&line), &[line.as_str()]).unwrap_err();
    assert!(matches!(err, CatalogError::BadKey(_)), "{err:?}");
    // The same public key with a different key id: decoded comparison still refuses it.
    let reid = component.public_b64_with_key_id([7u8; 8]);
    let err = catalog_trust_root_with(Some(&reid), &[line.as_str()]).unwrap_err();
    assert!(matches!(err, CatalogError::BadKey(_)), "{err:?}");
    // The app-updater key under another key id is refused by the production rule too.
    let app = citrate_components::key::app_updater_key_line().unwrap();
    use base64::Engine as _;
    let b64 = base64::engine::general_purpose::STANDARD;
    let mut raw = b64.decode(&app).unwrap();
    raw[2..10].copy_from_slice(&[3u8; 8]);
    let err =
        citrate_components::catalog::catalog_trust_root_from(Some(&b64.encode(raw))).unwrap_err();
    assert!(matches!(err, CatalogError::BadKey(_)), "{err:?}");
    // An empty slot is still KeyNotIssued first.
    assert_eq!(
        catalog_trust_root_with(None, &[line.as_str()]).unwrap_err(),
        CatalogError::KeyNotIssued
    );
}

#[test]
fn clearing_the_recorded_sequence_leaves_only_the_lifetime_cap_as_the_rollback_bound() {
    // Documented in docs/STORE_CATALOG.md: the high-water mark lives in app data. Without it
    // (a cleared app data folder), an older catalog is accepted again while it is unexpired,
    // and refused once its lifetime (at most MAX_CATALOG_LIFETIME_SECS) runs out.
    use citrate_components::catalog::MAX_CATALOG_LIFETIME_SECS;
    let key = TestKey::generate();
    let json = good();
    let sig = sign(&key, &json);
    assert!(verify_catalog(json.as_bytes(), &sig, &root(&key), None, NOW).is_ok());
    assert_eq!(
        verify_catalog(
            json.as_bytes(),
            &sig,
            &root(&key),
            None,
            NOW - 60 + MAX_CATALOG_LIFETIME_SECS
        )
        .unwrap_err(),
        CatalogError::Expired
    );
}
