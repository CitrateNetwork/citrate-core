//! HUP-S5.5 @rule8: the production key slot ships EMPTY, so the updater refuses everything
//! until the key ceremony sets it; and the component key must never be the app-updater key.
mod common;

use citrate_components::error::ComponentError;
use citrate_components::key::{TrustRoot, PRODUCTION_COMPONENT_PUBKEY};

#[test]
fn the_production_slot_is_empty_and_refuses() {
    // When the @rule8 ceremony sets the key, this test is updated in the same signed commit.
    assert!(PRODUCTION_COMPONENT_PUBKEY.is_none());
    let err = TrustRoot::production().unwrap_err();
    assert!(matches!(err, ComponentError::KeyNotConfigured), "{err:?}");
    assert!(err.to_string().contains("not configured"), "{err}");
}

#[test]
fn the_component_key_is_not_the_app_updater_key() {
    // Key separation: a signature that installs the app must never install a component, and the
    // reverse. Read the app-updater key from tauri.conf.json (base64 of the minisign .pub file).
    let conf = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../src-tauri/tauri.conf.json"
    ))
    .unwrap();
    let v: serde_json::Value = serde_json::from_str(&conf).unwrap();
    let app_pub = v["plugins"]["updater"]["pubkey"].as_str().unwrap();
    use base64::Engine as _;
    let pub_file = String::from_utf8(
        base64::engine::general_purpose::STANDARD
            .decode(app_pub)
            .unwrap(),
    )
    .unwrap();
    let app_key_line = pub_file.lines().nth(1).unwrap().trim().to_string();
    assert!(
        TrustRoot::from_base64(&app_key_line).is_ok(),
        "the app key parses as a minisign key"
    );
    if let Some(k) = PRODUCTION_COMPONENT_PUBKEY {
        assert_ne!(
            k.trim(),
            app_key_line,
            "the component key must differ from the app-updater key"
        );
    }
    // And the check the updater applies at run time refuses it outright.
    let err = TrustRoot::from_base64_checked(&app_key_line, &[app_key_line.as_str()]).unwrap_err();
    assert!(matches!(err, ComponentError::BadKey(_)), "{err:?}");
}

#[test]
fn a_malformed_key_is_refused() {
    for bad in ["", "not base64!", "RWQ=", "dW50cnVzdGVk"] {
        let err = TrustRoot::from_base64(bad).unwrap_err();
        assert!(matches!(err, ComponentError::BadKey(_)), "{bad:?}: {err:?}");
    }
}

#[test]
fn a_test_key_parses_and_has_a_stable_fingerprint() {
    let k = common::TestKey::generate();
    let a = TrustRoot::from_base64(&k.public_b64()).unwrap();
    let b = TrustRoot::from_base64(&format!("  {}\n", k.public_b64())).unwrap();
    assert_eq!(a.fingerprint(), b.fingerprint());
    assert_eq!(a.fingerprint().len(), 16);
}
