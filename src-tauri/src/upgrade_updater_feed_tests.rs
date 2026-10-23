//! 0.5.1 -> 0.5.2 updater path (UPGRADE_051_052 §Updater).
//!
//! A 0.5.1 install updates only through its own updater build: tauri-plugin-updater 2.10.1, the
//! endpoints and the minisign public key pinned in v0.5.1's tauri.conf.json. These tests hold the
//! 0.5.2 release to that contract:
//! - this tree still pins the SAME public key and the SAME endpoints as v0.5.1 (a rotated key or a
//!   moved feed strands every 0.5.1 install, which has no other way to learn about it);
//! - the published 0.5.1 feed (fixture: the `latest.json` asset of the v0.5.1 GitHub release,
//!   byte-identical to the CDN copy on 2026-10-23) parses with the plugin's own deserializer, and
//!   every signature in it names the pinned key;
//! - a 0.5.2 feed in the same format parses, is newer than 0.5.1 under the plugin's comparison,
//!   and resolves for every platform key a 0.5.1 build looks up.
//!
//! The private key is never touched here: these checks read only public material.

use base64::Engine;

const TAURI_CONF: &str = include_str!("../tauri.conf.json");
const FEED_V051: &str = include_str!("../../fixtures/upgrade-v0.5.1/updater/latest.v0.5.1.json");

/// The minisign public key v0.5.1 pins (tauri.conf.json at b521d98e, plugins.updater.pubkey).
const PUBKEY_V051: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDhFQzM4MkREMTA3RjA4QzUKUldURkNIOFEzWUxEanZsN096Vk1WWm9WMllUUE5GNXVUYVFYeGRQNFluRnVlVG5vWVZnV0F6eXAK";

/// The endpoints v0.5.1 polls, in order. The plugin stops at the first endpoint that answers with a
/// parseable release, so the CDN copy is the one 0.5.1 installs actually read.
const ENDPOINTS_V051: [&str; 2] = [
    "https://citrate-cdn.nyc3.cdn.digitaloceanspaces.com/downloads/updater/latest.json",
    "https://github.com/CitrateNetwork/citrate-core/releases/latest/download/latest.json",
];

/// The platform keys a 0.5.1 build looks up, in the plugin's order (`{os}-{arch}-{installer}`, then
/// `{os}-{arch}`; updater.rs get_urls). 0.5.1 shipped macOS arm64 (.app), and Linux x86_64 and
/// aarch64 as AppImage and .deb. Windows 0.5.1 has no updater artifacts (its feed entry is absent,
/// which the app reads as "no update", src/shell/updater.ts isNoPlatformInFeed).
const LOOKUPS_V051: [(&str, [&str; 2]); 5] = [
    ("macOS arm64 .app", ["darwin-aarch64-app", "darwin-aarch64"]),
    ("Linux x86_64 AppImage", ["linux-x86_64-appimage", "linux-x86_64"]),
    ("Linux aarch64 AppImage", ["linux-aarch64-appimage", "linux-aarch64"]),
    ("Linux x86_64 .deb", ["linux-x86_64-deb", "linux-x86_64"]),
    ("Linux aarch64 .deb", ["linux-aarch64-deb", "linux-aarch64"]),
];

fn conf() -> serde_json::Value {
    serde_json::from_str(TAURI_CONF).unwrap_or_else(|e| panic!("tauri.conf.json: {e}"))
}

fn b64(s: &str) -> Vec<u8> {
    base64::engine::general_purpose::STANDARD
        .decode(s.trim())
        .unwrap_or_else(|e| panic!("base64: {e}"))
}

/// The 8-byte key id inside a minisign public key or signature file (the second line, base64,
/// bytes 2..10 after the 2-byte algorithm tag), as the hex the untrusted comment prints.
fn key_id(minisign_file: &[u8]) -> (String, String) {
    let text = String::from_utf8(minisign_file.to_vec()).unwrap_or_else(|e| panic!("utf8: {e}"));
    let line = text
        .lines()
        .nth(1)
        .unwrap_or_else(|| panic!("no key/signature line in {text:?}"));
    let raw = b64(line);
    assert!(raw.len() >= 10, "short minisign line");
    let alg = String::from_utf8_lossy(&raw[..2]).to_string();
    let mut id = raw[2..10].to_vec();
    id.reverse(); // minisign prints the little-endian id as big-endian hex
    (alg, hex::encode_upper(id))
}

fn release(json: &str) -> tauri_plugin_updater::RemoteRelease {
    serde_json::from_str(json).unwrap_or_else(|e| panic!("the plugin refuses the feed: {e}"))
}

/// A 0.5.2 feed in the exact shape the 0.5.1 release used (same platform keys, CDN URLs, the
/// signature fields carried over byte for byte so their key id is the real one).
fn feed_v052() -> String {
    let mut v: serde_json::Value =
        serde_json::from_str(FEED_V051).unwrap_or_else(|e| panic!("fixture: {e}"));
    v["version"] = "0.5.2".into();
    v["pub_date"] = "2026-11-01T18:00:00Z".into();
    v["notes"] = "Citrate Core 0.5.2".into();
    if let Some(p) = v["platforms"].as_object_mut() {
        for (_, entry) in p.iter_mut() {
            let url = entry["url"].as_str().unwrap_or_default().replace("/v0.5.1/", "/v0.5.2/");
            entry["url"] = url.into();
        }
    }
    v.to_string()
}

#[test]
fn this_build_pins_the_v051_updater_key_and_endpoints() {
    let c = conf();
    let up = &c["plugins"]["updater"];
    assert_eq!(
        up["pubkey"].as_str(),
        Some(PUBKEY_V051),
        "the updater public key changed: every 0.5.1 install would refuse this release's updates"
    );
    let eps: Vec<&str> = up["endpoints"]
        .as_array()
        .unwrap_or_else(|| panic!("no updater endpoints"))
        .iter()
        .filter_map(|e| e.as_str())
        .collect();
    assert_eq!(eps, ENDPOINTS_V051, "0.5.1 installs poll exactly these endpoints");
    assert_eq!(c["identifier"].as_str(), Some("ai.citrate.core"), "bundle id (keychain ACL)");
    assert_eq!(c["bundle"]["createUpdaterArtifacts"].as_bool(), Some(true));
    let (alg, id) = key_id(&b64(PUBKEY_V051));
    assert_eq!((alg.as_str(), id.as_str()), ("Ed", "8EC382DD107F08C5"));
}

#[test]
fn the_published_v051_feed_parses_and_every_signature_names_the_pinned_key() {
    let r = release(FEED_V051);
    let (_, pinned) = key_id(&b64(PUBKEY_V051));
    for (label, keys) in LOOKUPS_V051 {
        let hit = keys
            .iter()
            .find(|k| r.download_url(k).is_ok() && r.signature(k).is_ok())
            .unwrap_or_else(|| panic!("{label}: none of {keys:?} in the 0.5.1 feed"));
        let sig = r.signature(hit).unwrap_or_else(|e| panic!("{label}: {e}"));
        let (alg, id) = key_id(&b64(sig));
        assert_eq!(id, pinned, "{label}: signed by key {id}, not the pinned {pinned}");
        assert!(alg == "ED" || alg == "Ed", "{label}: minisign algorithm {alg}");
        let url = r.download_url(hit).unwrap_or_else(|e| panic!("{label}: {e}"));
        assert_eq!(url.scheme(), "https", "{label}: the plugin refuses a non-https download");
    }
}

#[test]
fn a_v052_feed_in_the_same_format_is_offered_to_every_v051_platform() {
    let old = release(FEED_V051);
    let new = release(&feed_v052());
    assert!(new.version > old.version, "0.5.2 must compare newer than 0.5.1");
    assert!(new.pub_date.is_some(), "pub_date must be RFC 3339 or the plugin refuses the feed");
    for (label, keys) in LOOKUPS_V051 {
        let hit = keys
            .iter()
            .find(|k| new.download_url(k).is_ok() && new.signature(k).is_ok())
            .unwrap_or_else(|| panic!("{label}: none of {keys:?} in the 0.5.2 feed"));
        let url = new.download_url(hit).unwrap_or_else(|e| panic!("{label}: {e}"));
        assert!(url.as_str().contains("/v0.5.2/"), "{label}: {url}");
    }
}

#[test]
fn a_feed_without_pub_date_rfc3339_or_with_a_bad_version_is_refused() {
    // Fail-closed edges of the plugin's parser that a hand-edited feed can hit.
    let mut v: serde_json::Value =
        serde_json::from_str(&feed_v052()).unwrap_or_else(|e| panic!("{e}"));
    v["pub_date"] = "2026-11-01 18:00".into();
    assert!(serde_json::from_value::<tauri_plugin_updater::RemoteRelease>(v.clone()).is_err());
    v["pub_date"] = "2026-11-01T18:00:00Z".into();
    v["version"] = "0.5".into();
    assert!(serde_json::from_value::<tauri_plugin_updater::RemoteRelease>(v.clone()).is_err());
    // A leading "v" is accepted (parse_version trims it), so a tag-style version still works.
    v["version"] = "v0.5.2".into();
    let r = serde_json::from_value::<tauri_plugin_updater::RemoteRelease>(v)
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(r.version.to_string(), "0.5.2");
}

#[test]
fn a_prerelease_version_is_newer_than_v051_so_it_must_never_reach_the_production_feed() {
    // semver orders 0.5.2-rc.1 above 0.5.1: an rc manifest uploaded to the production endpoint
    // is offered to every 0.5.1 install. The release checklist keeps rc feeds off it.
    let old = release(FEED_V051);
    let mut v: serde_json::Value =
        serde_json::from_str(&feed_v052()).unwrap_or_else(|e| panic!("{e}"));
    v["version"] = "0.5.2-rc.1".into();
    let rc = release(&v.to_string());
    assert!(rc.version > old.version);
}
