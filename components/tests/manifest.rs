//! HUP-S5.5: the signed component manifest is refused unless its signature, kind of signature,
//! trusted comment, freshness, sequence and every field check out.
mod common;

use citrate_components::error::ComponentError;
use citrate_components::key::TrustRoot;
use citrate_components::manifest::{verify_manifest, SeenManifest};
use common::*;

fn fixture(tag: &str) -> (TempDir, TestKey, DirFetcher) {
    let t = TempDir::new(tag);
    let f = DirFetcher::new(&t.path().join("srv"));
    (t, TestKey::generate(), f)
}

fn one(key: &TestKey, f: &DirFetcher, seq: u64) -> String {
    manifest_json(
        key,
        f,
        seq,
        "solc",
        "0.8.36",
        "macos-arm64",
        "raw",
        b"solc-binary",
        &["solc"],
    )
}

fn root(key: &TestKey) -> TrustRoot {
    TrustRoot::from_base64(&key.public_b64()).unwrap()
}

#[test]
fn a_correctly_signed_manifest_verifies() {
    let (_t, key, f) = fixture("ok");
    let json = one(&key, &f, 3);
    let vm = verify_manifest(
        json.as_bytes(),
        &sign_manifest(&key, &json),
        &root(&key),
        None,
        NOW,
    )
    .unwrap();
    assert_eq!(vm.manifest().sequence, 3);
    assert_eq!(vm.manifest().components[0].name, "solc");
    assert_eq!(vm.digest_hex(), sha256_hex(json.as_bytes()));
}

#[test]
fn a_manifest_signed_by_another_key_is_refused() {
    let (_t, key, f) = fixture("otherkey");
    let other = TestKey::generate();
    let json = one(&key, &f, 1);
    let err = verify_manifest(
        json.as_bytes(),
        &sign_manifest(&other, &json),
        &root(&key),
        None,
        NOW,
    )
    .unwrap_err();
    assert!(
        matches!(err, ComponentError::SignatureInvalid(_)),
        "{err:?}"
    );
}

#[test]
fn one_flipped_byte_breaks_the_signature() {
    let (_t, key, f) = fixture("flip");
    let json = one(&key, &f, 1);
    let sig = sign_manifest(&key, &json);
    let tampered = json.replace("\"sequence\":1", "\"sequence\":9");
    assert_ne!(tampered, json);
    let err = verify_manifest(tampered.as_bytes(), &sig, &root(&key), None, NOW).unwrap_err();
    assert!(
        matches!(err, ComponentError::SignatureInvalid(_)),
        "{err:?}"
    );
}

#[test]
fn a_legacy_non_prehashed_signature_is_refused() {
    let (_t, key, f) = fixture("legacy");
    let json = one(&key, &f, 1);
    let sig = key.sign_legacy(json.as_bytes(), "citrate-components-manifest seq");
    let err = verify_manifest(json.as_bytes(), &sig, &root(&key), None, NOW).unwrap_err();
    assert!(
        matches!(err, ComponentError::SignatureInvalid(_)),
        "{err:?}"
    );
}

#[test]
fn a_valid_signature_with_the_wrong_trusted_comment_is_refused() {
    // An artifact signature (or an app-updater signature) must not pass as a manifest signature.
    let (_t, key, f) = fixture("tc");
    let json = one(&key, &f, 1);
    for tc in [
        "citrate-components-artifact solc 0.8.36",
        "timestamp:1790000000\tfile:latest.json",
        "citrate-components-manifestX",
        "",
    ] {
        let sig = key.sign(json.as_bytes(), tc);
        let err = verify_manifest(json.as_bytes(), &sig, &root(&key), None, NOW).unwrap_err();
        assert!(
            matches!(err, ComponentError::WrongTrustedComment),
            "{tc:?}: {err:?}"
        );
    }
    // The bare prefix alone is accepted.
    let sig = key.sign(json.as_bytes(), "citrate-components-manifest");
    verify_manifest(json.as_bytes(), &sig, &root(&key), None, NOW).unwrap();
}

#[test]
fn an_older_sequence_is_a_rollback_and_is_refused() {
    let (_t, key, f) = fixture("rollback");
    let json = one(&key, &f, 4);
    let sig = sign_manifest(&key, &json);
    let seen = SeenManifest {
        sequence: 5,
        digest_hex: "00".repeat(32),
        verified_at: NOW,
        expires_at: NOW + DAY,
    };
    let err = verify_manifest(json.as_bytes(), &sig, &root(&key), Some(&seen), NOW).unwrap_err();
    assert!(
        matches!(err, ComponentError::ManifestRollback { seen: 5, got: 4 }),
        "{err:?}"
    );
}

#[test]
fn the_same_sequence_is_accepted_only_for_the_same_bytes() {
    let (_t, key, f) = fixture("sameseq");
    let json = one(&key, &f, 5);
    let sig = sign_manifest(&key, &json);
    let same = SeenManifest {
        sequence: 5,
        digest_hex: sha256_hex(json.as_bytes()),
        verified_at: NOW,
        expires_at: NOW + DAY,
    };
    verify_manifest(json.as_bytes(), &sig, &root(&key), Some(&same), NOW).unwrap();
    let different = SeenManifest {
        digest_hex: "11".repeat(32),
        ..same
    };
    let err =
        verify_manifest(json.as_bytes(), &sig, &root(&key), Some(&different), NOW).unwrap_err();
    assert!(
        matches!(err, ComponentError::ManifestRollback { .. }),
        "{err:?}"
    );
}

#[test]
fn an_expired_or_future_manifest_is_refused() {
    let (_t, key, f) = fixture("time");
    let json = one(&key, &f, 1);
    let sig = sign_manifest(&key, &json);
    let r = root(&key);
    // expires_at = NOW + 6 days
    let err = verify_manifest(json.as_bytes(), &sig, &r, None, NOW + 6 * DAY).unwrap_err();
    assert!(matches!(err, ComponentError::ManifestExpired), "{err:?}");
    verify_manifest(json.as_bytes(), &sig, &r, None, NOW + 6 * DAY - 1).unwrap();
    // issued_at = NOW - 1 day; more than the clock skew before that is "not yet valid".
    let err = verify_manifest(json.as_bytes(), &sig, &r, None, NOW - DAY - 3600).unwrap_err();
    assert!(
        matches!(err, ComponentError::ManifestNotYetValid),
        "{err:?}"
    );
}

fn resign(key: &TestKey, v: serde_json::Value) -> (String, String) {
    let json = v.to_string();
    let sig = sign_manifest(key, &json);
    (json, sig)
}

#[test]
fn a_manifest_valid_for_too_long_is_refused() {
    let (_t, key, f) = fixture("lifetime");
    let mut v: serde_json::Value = serde_json::from_str(&one(&key, &f, 1)).unwrap();
    v["expires_at"] = serde_json::json!(NOW + 400 * DAY);
    let (json, sig) = resign(&key, v);
    let err = verify_manifest(json.as_bytes(), &sig, &root(&key), None, NOW).unwrap_err();
    assert!(
        matches!(err, ComponentError::ManifestMalformed(_)),
        "{err:?}"
    );
}

/// Every field rule, each on an otherwise valid, correctly signed manifest.
#[test]
fn signed_but_malformed_fields_are_refused() {
    let (_t, key, f) = fixture("fields");
    let base: serde_json::Value = serde_json::from_str(&one(&key, &f, 1)).unwrap();
    type Edit = Box<dyn Fn(&mut serde_json::Value)>;
    let edits: Vec<(&str, Edit)> = vec![
        ("schema 2", Box::new(|v| v["schema"] = 2.into())),
        (
            "unknown top-level field",
            Box::new(|v| v["extra"] = 1.into()),
        ),
        (
            "no components",
            Box::new(|v| v["components"] = serde_json::json!([])),
        ),
        ("bad channel", Box::new(|v| v["channel"] = "Stable!".into())),
        (
            "name with a slash",
            Box::new(|v| v["components"][0]["name"] = "../solc".into()),
        ),
        (
            "name upper case",
            Box::new(|v| v["components"][0]["name"] = "Solc".into()),
        ),
        (
            "version dot-dot",
            Box::new(|v| v["components"][0]["version"] = "..".into()),
        ),
        (
            "version with a slash",
            Box::new(|v| v["components"][0]["version"] = "1/2".into()),
        ),
        (
            "empty licence",
            Box::new(|v| v["components"][0]["license"] = "".into()),
        ),
        (
            "unknown kind",
            Box::new(|v| v["components"][0]["kind"] = "kernel".into()),
        ),
        (
            "entrypoint escapes",
            Box::new(|v| v["components"][0]["entrypoints"] = serde_json::json!(["../x"])),
        ),
        (
            "entrypoint absolute",
            Box::new(|v| v["components"][0]["entrypoints"] = serde_json::json!(["/bin/sh"])),
        ),
        (
            "unknown platform",
            Box::new(|v| {
                let a = v["components"][0]["artifacts"]["macos-arm64"].clone();
                v["components"][0]["artifacts"] = serde_json::json!({ "plan9-x64": a });
            }),
        ),
        (
            "no artifacts",
            Box::new(|v| v["components"][0]["artifacts"] = serde_json::json!({})),
        ),
        (
            "http url",
            Box::new(|v| {
                v["components"][0]["artifacts"]["macos-arm64"]["url"] =
                    "http://fixture.test/x".into()
            }),
        ),
        (
            "url with a space",
            Box::new(|v| {
                v["components"][0]["artifacts"]["macos-arm64"]["url"] =
                    "https://fixture.test/a b".into()
            }),
        ),
        (
            "url without a host",
            Box::new(|v| {
                v["components"][0]["artifacts"]["macos-arm64"]["url"] = "https:///x".into()
            }),
        ),
        (
            "short sha",
            Box::new(|v| v["components"][0]["artifacts"]["macos-arm64"]["sha256"] = "abcd".into()),
        ),
        (
            "upper-case sha",
            Box::new(|v| {
                let s = v["components"][0]["artifacts"]["macos-arm64"]["sha256"]
                    .as_str()
                    .unwrap()
                    .to_uppercase();
                v["components"][0]["artifacts"]["macos-arm64"]["sha256"] = s.into();
            }),
        ),
        (
            "zero size",
            Box::new(|v| v["components"][0]["artifacts"]["macos-arm64"]["size"] = 0.into()),
        ),
        (
            "huge size",
            Box::new(|v| {
                v["components"][0]["artifacts"]["macos-arm64"]["size"] = (5u64 << 30).into()
            }),
        ),
        (
            "unknown format",
            Box::new(|v| v["components"][0]["artifacts"]["macos-arm64"]["format"] = "rar".into()),
        ),
        (
            "garbage signature",
            Box::new(|v| {
                v["components"][0]["artifacts"]["macos-arm64"]["signature"] = "nope".into()
            }),
        ),
        (
            "duplicate name",
            Box::new(|v| {
                let c = v["components"][0].clone();
                v["components"].as_array_mut().unwrap().push(c);
            }),
        ),
    ];
    for (label, edit) in edits {
        let mut v = base.clone();
        edit(&mut v);
        let (json, sig) = resign(&key, v);
        let err = verify_manifest(json.as_bytes(), &sig, &root(&key), None, NOW).expect_err(label);
        assert!(
            matches!(err, ComponentError::ManifestMalformed(_)),
            "{label}: {err:?}"
        );
    }
    // The unedited base passes, so each refusal above is due to its one edit.
    let (json, sig) = resign(&key, base);
    verify_manifest(json.as_bytes(), &sig, &root(&key), None, NOW).unwrap();
}

#[test]
fn an_oversized_manifest_is_refused_before_parsing() {
    let (_t, key, _f) = fixture("big");
    let big = vec![b' '; (1 << 20) + 1];
    let sig = key.sign(&big, "citrate-components-manifest");
    let err = verify_manifest(&big, &sig, &root(&key), None, NOW).unwrap_err();
    assert!(
        matches!(err, ComponentError::ManifestMalformed(_)),
        "{err:?}"
    );
}
