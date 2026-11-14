// 0.5.4: the signed minimum version. Policies are signed with a throwaway key made per test, in
// the exact form `tauri signer sign` writes (base64 of the minisign .sig text) and checked against
// a pubkey in the `tauri.conf.json` form (base64 of the minisign .pub text).
use super::*;

/// A minisign-format Ed25519 key (test-only).
struct TestKey {
    sk: ed25519_dalek::SigningKey,
    key_id: [u8; 8],
}

fn b64(b: impl AsRef<[u8]>) -> String {
    base64::engine::general_purpose::STANDARD.encode(b)
}

impl TestKey {
    fn generate() -> Self {
        TestKey {
            sk: ed25519_dalek::SigningKey::from_bytes(&rand::random::<[u8; 32]>()),
            key_id: rand::random(),
        }
    }

    /// The `tauri.conf.json` `plugins.updater.pubkey` form.
    fn conf_pubkey(&self) -> String {
        let mut bin = Vec::with_capacity(42);
        bin.extend_from_slice(b"Ed");
        bin.extend_from_slice(&self.key_id);
        bin.extend_from_slice(self.sk.verifying_key().as_bytes());
        b64(format!("untrusted comment: minisign public key: TEST\n{}\n", b64(bin)))
    }

    /// A prehashed (`ED`) signature, as `tauri signer sign` writes the `.sig` (base64 of the text).
    fn sign(&self, data: &[u8]) -> String {
        use blake2::Digest as _;
        use ed25519_dalek::Signer as _;
        let h = blake2::Blake2b512::digest(data);
        let sig = self.sk.sign(&h).to_bytes();
        let mut bin1 = Vec::with_capacity(74);
        bin1.extend_from_slice(b"ED");
        bin1.extend_from_slice(&self.key_id);
        bin1.extend_from_slice(&sig);
        let tc = "timestamp:1790000000\tfile:update-policy.json";
        let mut global = sig.to_vec();
        global.extend_from_slice(tc.as_bytes());
        let gsig = self.sk.sign(&global).to_bytes();
        b64(format!(
            "untrusted comment: signature from tauri secret key\n{}\ntrusted comment: {tc}\n{}\n",
            b64(bin1),
            b64(gsig)
        ))
    }

    fn policy(&self, min: &str, issued_at: u64) -> StoredPolicy {
        let body = format!(
            r#"{{"domain":"{POLICY_DOMAIN}","minimumVersion":"{min}","issuedAt":{issued_at},"reason":"0.5.4 fixes Hermes"}}"#
        );
        StoredPolicy {
            policy: b64(&body),
            signature: self.sign(body.as_bytes()),
        }
    }
}

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "core-update-policy-{tag}-{}-{}",
        std::process::id(),
        rand::random::<u32>()
    ));
    std::fs::create_dir_all(&d).unwrap();
    d.join(POLICY_FILE)
}

#[test]
fn versions_order_like_semver_with_prereleases_below_the_release() {
    let v = |s| Version::parse(s).unwrap();
    assert!(v("0.5.3") < v("0.5.4"));
    assert!(v("0.5.4-rc.1") < v("0.5.4"));
    assert!(v("0.5.10") > v("0.5.9"));
    assert!(v("1.0.0") > v("0.99.99"));
    assert_eq!(v("v0.5.4"), v("0.5.4"));
    assert_eq!(v("0.5.4+build.7"), v("0.5.4"));
    for bad in ["", "0.5", "0.5.4.1", "a.b.c", "0.5.4-", "-1.0.0", "0.5. 4"] {
        assert!(Version::parse(bad).is_none(), "{bad:?} must not parse");
    }
}

#[test]
fn a_verified_policy_requires_the_update_below_its_minimum_only() {
    let k = TestKey::generate();
    let p = k.policy("0.5.4", 1_790_000_000);
    let doc = verify(&k.conf_pubkey(), &p.policy, &p.signature).expect("verifies");
    assert_eq!(doc.minimum_version, "0.5.4");
    assert!(verdict("0.5.3", Some(&doc), None).update_required);
    assert!(verdict("0.5.4-rc.2", Some(&doc), None).update_required);
    assert!(!verdict("0.5.4", Some(&doc), None).update_required);
    assert!(!verdict("0.6.0", Some(&doc), None).update_required);
    // No policy: never required.
    assert!(!verdict("0.1.0", None, None).update_required);
}

#[test]
fn a_tampered_minimum_or_a_foreign_key_never_gates() {
    let k = TestKey::generate();
    let p = k.policy("0.5.4", 1);
    // The feed host raises the minimum without the key.
    let raised = b64(format!(
        r#"{{"domain":"{POLICY_DOMAIN}","minimumVersion":"9.9.9","issuedAt":1}}"#
    ));
    assert!(verify(&k.conf_pubkey(), &raised, &p.signature).is_err());
    // Signed by some other key.
    let other = TestKey::generate();
    let q = other.policy("0.5.4", 1);
    let e = verify(&k.conf_pubkey(), &q.policy, &q.signature).unwrap_err();
    assert!(e.contains("does not match"), "{e}");
    // Garbage in every field is an error, never a panic.
    for (pol, sig) in [("", ""), ("!!", "!!"), (p.policy.as_str(), "")] {
        assert!(verify(&k.conf_pubkey(), pol, sig).is_err());
    }
}

#[test]
fn a_signed_document_for_another_domain_is_refused() {
    // An artifact signature verifies over bytes that are not this document: refused by the domain.
    let k = TestKey::generate();
    let body = r#"{"domain":"citrate-components-manifest","minimumVersion":"0.5.4","issuedAt":1}"#;
    let e = verify(&k.conf_pubkey(), &b64(body), &k.sign(body.as_bytes())).unwrap_err();
    assert!(e.contains("not citrate-core-update-policy/v1"), "{e}");
    // Signed bytes that are not a policy at all (an update bundle).
    let bundle = b"\x1f\x8b\x08\x00not json";
    assert!(verify(&k.conf_pubkey(), &b64(bundle), &k.sign(bundle)).is_err());
    // Unknown fields are refused too (the signer's document is exactly the schema).
    let extra = format!(r#"{{"domain":"{POLICY_DOMAIN}","minimumVersion":"0.5.4","issuedAt":1,"blockAll":true}}"#);
    assert!(verify(&k.conf_pubkey(), &b64(&extra), &k.sign(extra.as_bytes())).is_err());
}

#[test]
fn apply_keeps_the_verified_policy_and_status_reads_it_back_offline() {
    let k = TestKey::generate();
    let path = tmp("keep");
    let v = apply(&path, &k.conf_pubkey(), "0.5.3", &k.policy("0.5.4", 10), "0.5.4");
    assert!(v.update_required, "{v:?}");
    assert_eq!(v.minimum_version.as_deref(), Some("0.5.4"));
    assert_eq!(v.refused, None);
    // Launch with no network: the kept policy still gates.
    let s = status(&path, &k.conf_pubkey(), "0.5.3");
    assert!(s.update_required);
    // After the update it no longer does.
    assert!(!status(&path, &k.conf_pubkey(), "0.5.4").update_required);
}

#[test]
fn a_replayed_older_policy_does_not_lift_a_kept_higher_minimum() {
    let k = TestKey::generate();
    let path = tmp("replay");
    apply(&path, &k.conf_pubkey(), "0.5.3", &k.policy("0.5.4", 20), "0.5.4");
    let v = apply(&path, &k.conf_pubkey(), "0.5.3", &k.policy("0.5.2", 30), "0.5.4");
    assert_eq!(v.minimum_version.as_deref(), Some("0.5.4"));
    assert!(v.update_required);
    // A higher one replaces it.
    let v = apply(&path, &k.conf_pubkey(), "0.5.3", &k.policy("0.5.5", 40), "0.5.5");
    assert_eq!(v.minimum_version.as_deref(), Some("0.5.5"));
}

#[test]
fn a_minimum_above_what_the_feed_offers_is_refused_so_nobody_is_locked_out() {
    let k = TestKey::generate();
    let path = tmp("ahead");
    let v = apply(&path, &k.conf_pubkey(), "0.5.3", &k.policy("0.5.5", 1), "0.5.4");
    assert!(!v.update_required);
    assert!(v.refused.as_deref().unwrap_or("").contains("feed offers 0.5.4"), "{v:?}");
    assert!(!path.exists(), "nothing kept from a refused policy");
}

#[test]
fn a_kept_file_that_no_longer_verifies_is_ignored_and_said() {
    let k = TestKey::generate();
    let path = tmp("edited");
    apply(&path, &k.conf_pubkey(), "0.5.3", &k.policy("0.5.4", 1), "0.5.4");
    // Someone edits the kept file to raise the minimum.
    let mut kept: StoredPolicy = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    kept.policy = b64(format!(r#"{{"domain":"{POLICY_DOMAIN}","minimumVersion":"9.0.0","issuedAt":2}}"#));
    std::fs::write(&path, serde_json::to_vec(&kept).unwrap()).unwrap();
    let s = status(&path, &k.conf_pubkey(), "0.5.3");
    assert!(!s.update_required);
    assert!(s.refused.as_deref().unwrap_or("").contains("ignored"), "{s:?}");
    // No file at all: nothing required, nothing refused.
    let none = status(&tmp("none"), &k.conf_pubkey(), "0.5.3");
    assert_eq!((none.update_required, none.refused), (false, None));
}

#[test]
fn the_pinned_release_key_parses_in_the_form_verify_reads() {
    // The real tauri.conf.json key must decode the way `verify` decodes it, or every real policy
    // would be refused.
    let conf: serde_json::Value =
        serde_json::from_str(include_str!("../tauri.conf.json")).expect("tauri.conf.json");
    let key = conf["plugins"]["updater"]["pubkey"].as_str().expect("pubkey");
    let text = b64_text(key, "updater key").expect("base64 text");
    minisign_verify::PublicKey::decode(&text).expect("the pinned key parses");
    // And a signature from another key is refused against it.
    let k = TestKey::generate();
    let p = k.policy("0.5.4", 1);
    assert!(verify(key, &p.policy, &p.signature).is_err());
}

#[test]
fn update_policy_commands_are_registered_and_allowed_for_the_main_window() {
    let lib = include_str!("lib.rs");
    let acl = include_str!("../permissions/main-window.toml");
    for cmd in ["update_policy_apply", "update_policy_status"] {
        assert!(lib.contains(&format!("update_policy::{cmd}")), "{cmd} in generate_handler!");
        assert!(acl.contains(&format!("\"{cmd}\"")), "{cmd} in main-window.toml");
    }
}
