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
    (
        "Linux x86_64 AppImage",
        ["linux-x86_64-appimage", "linux-x86_64"],
    ),
    (
        "Linux aarch64 AppImage",
        ["linux-aarch64-appimage", "linux-aarch64"],
    ),
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
            let url = entry["url"]
                .as_str()
                .unwrap_or_default()
                .replace("/v0.5.1/", "/v0.5.2/");
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
    assert_eq!(
        eps, ENDPOINTS_V051,
        "0.5.1 installs poll exactly these endpoints"
    );
    assert_eq!(
        c["identifier"].as_str(),
        Some("ai.citrate.core"),
        "bundle id (keychain ACL)"
    );
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
        assert_eq!(
            id, pinned,
            "{label}: signed by key {id}, not the pinned {pinned}"
        );
        assert!(
            alg == "ED" || alg == "Ed",
            "{label}: minisign algorithm {alg}"
        );
        let url = r
            .download_url(hit)
            .unwrap_or_else(|e| panic!("{label}: {e}"));
        assert_eq!(
            url.scheme(),
            "https",
            "{label}: the plugin refuses a non-https download"
        );
    }
}

#[test]
fn a_v052_feed_in_the_same_format_is_offered_to_every_v051_platform() {
    let old = release(FEED_V051);
    let new = release(&feed_v052());
    assert!(
        new.version > old.version,
        "0.5.2 must compare newer than 0.5.1"
    );
    assert!(
        new.pub_date.is_some(),
        "pub_date must be RFC 3339 or the plugin refuses the feed"
    );
    for (label, keys) in LOOKUPS_V051 {
        let hit = keys
            .iter()
            .find(|k| new.download_url(k).is_ok() && new.signature(k).is_ok())
            .unwrap_or_else(|| panic!("{label}: none of {keys:?} in the 0.5.2 feed"));
        let url = new
            .download_url(hit)
            .unwrap_or_else(|e| panic!("{label}: {e}"));
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

// ---------------------------------------------------------------------------------------------
// RC52 G8: the updater's own signature check, against goldens signed with a THROWAWAY key.
//
// fixtures/updater-e2e/ holds two throwaway tauri-signer public keys (A, B), an 8 KiB stand-in
// update artifact, its `.sig` made with A and with B (`tauri signer sign`; the private keys were
// discarded after signing), and a `latest.json` in the 0.5.1 feed shape whose signatures are A's.
// The tests run tauri-plugin-updater's own `check()` and `download()` (which verifies the signature
// against the configured public key before returning a byte) in a mock app, against a loopback HTTP
// server. Nothing is installed. The release keys (v1, v2) are never read or used.
// The same cases run on the packaged Linux app in the K1-L `update-e2e` scenario (docs/UPDATER_E2E.md).
// ---------------------------------------------------------------------------------------------

const E2E_PUB_A: &str = include_str!("../../fixtures/updater-e2e/throwaway-a.pub");
const E2E_PUB_B: &str = include_str!("../../fixtures/updater-e2e/throwaway-b.pub");
const E2E_PAYLOAD: &[u8] = include_bytes!("../../fixtures/updater-e2e/update-payload.bin");
const E2E_SIG_A: &str = include_str!("../../fixtures/updater-e2e/update-payload.bin.sig");
const E2E_SIG_B: &str = include_str!("../../fixtures/updater-e2e/update-payload.bin.b.sig");
const E2E_FEED: &str = include_str!("../../fixtures/updater-e2e/latest.json");

/// The platform key a Linux x86_64 AppImage build looks up first (`{os}-{arch}-{installer}`).
const E2E_TARGET: &str = "linux-x86_64-appimage";

/// A loopback HTTP/1.1 server for one test: answers GET `path` with its body (404 otherwise) and
/// records every requested path. The thread ends with the test process.
struct LoopbackFeed {
    base: String,
    hits: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
}

impl LoopbackFeed {
    fn serve(routes: Vec<(&'static str, Vec<u8>)>) -> Self {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap_or_else(|e| panic!("bind loopback: {e}"));
        let base = format!(
            "http://{}",
            listener
                .local_addr()
                .unwrap_or_else(|e| panic!("local addr: {e}"))
        );
        let hits = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen = hits.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut s) = stream else { continue };
                let mut req = Vec::new();
                let mut buf = [0u8; 1024];
                while !req.windows(4).any(|w| w == b"\r\n\r\n") {
                    match s.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => req.extend_from_slice(&buf[..n]),
                    }
                }
                let line = String::from_utf8_lossy(&req)
                    .lines()
                    .next()
                    .unwrap_or_default()
                    .to_string();
                let path = line
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or_default()
                    .to_string();
                if let Ok(mut h) = seen.lock() {
                    h.push(path.clone());
                }
                let (status, body) = match routes.iter().find(|(p, _)| *p == path) {
                    Some((_, b)) => ("200 OK", b.clone()),
                    None => ("404 Not Found", Vec::new()),
                };
                let head = format!(
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = s
                    .write_all(head.as_bytes())
                    .and_then(|_| s.write_all(&body));
            }
        });
        LoopbackFeed { base, hits }
    }

    fn hits(&self) -> Vec<String> {
        self.hits.lock().map(|h| h.clone()).unwrap_or_default()
    }
}

/// The golden feed with the artifact URL moved to `base`, and optionally another version or
/// another signature in every platform entry.
fn e2e_feed(base: &str, version: Option<&str>, signature: Option<&str>) -> Vec<u8> {
    let mut v: serde_json::Value =
        serde_json::from_str(E2E_FEED).unwrap_or_else(|e| panic!("golden feed: {e}"));
    if let Some(ver) = version {
        v["version"] = ver.into();
    }
    if let Some(p) = v["platforms"].as_object_mut() {
        for (_, entry) in p.iter_mut() {
            entry["url"] = format!("{base}/update-payload.bin").into();
            if let Some(sig) = signature {
                entry["signature"] = sig.into();
            }
        }
    }
    v.to_string().into_bytes()
}

/// The `.sig` text with one bit of the Ed25519 signature flipped (still valid base64 and minisign
/// framing, same key id), as a damaged or forged feed entry would carry.
fn e2e_altered_signature(sig_b64: &str) -> String {
    let text = String::from_utf8(b64(sig_b64)).unwrap_or_else(|e| panic!("sig utf8: {e}"));
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    let mut raw = b64(&lines[1]);
    assert_eq!(
        raw.len(),
        74,
        "minisign signature line: 2 alg + 8 key id + 64 sig"
    );
    raw[40] ^= 0x01;
    lines[1] = base64::engine::general_purpose::STANDARD.encode(raw);
    base64::engine::general_purpose::STANDARD.encode(format!("{}\n", lines.join("\n")))
}

#[derive(Debug, PartialEq)]
enum E2eOutcome {
    /// `check()` found nothing newer.
    NoUpdate,
    /// `download()` returned the bytes: the signature verified against the pinned key.
    Verified { version: String, bytes: Vec<u8> },
    /// `download()` refused the artifact (the plugin's error text).
    Refused(String),
}

/// A mock app at `current` that pins `pubkey` and polls `feed_url`, running the plugin's own
/// check, then download (download verifies; install is never called).
fn e2e_run(pubkey: &str, current: &str, feed_url: &str) -> E2eOutcome {
    use tauri_plugin_updater::UpdaterExt;
    let mut ctx = tauri::test::mock_context(tauri::test::noop_assets());
    ctx.config_mut().plugins.0.insert(
        "updater".into(),
        serde_json::json!({
            "pubkey": pubkey.trim(),
            "endpoints": [feed_url],
            "dangerousInsecureTransportProtocol": true,
        }),
    );
    ctx.package_info_mut().version = current
        .parse()
        .unwrap_or_else(|e| panic!("version {current}: {e}"));
    let app = tauri::test::mock_builder()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .build(ctx)
        .unwrap_or_else(|e| panic!("mock app: {e}"));
    let url: url::Url = feed_url.parse().unwrap_or_else(|e| panic!("{e}"));
    tauri::async_runtime::block_on(async move {
        let updater = app
            .updater_builder()
            .target(E2E_TARGET)
            .no_proxy()
            .endpoints(vec![url])
            .and_then(|b| b.build())
            .unwrap_or_else(|e| panic!("updater: {e}"));
        match updater.check().await {
            Err(e) => panic!("check failed: {e}"),
            Ok(None) => E2eOutcome::NoUpdate,
            Ok(Some(update)) => match update.download(|_, _| {}, || {}).await {
                Ok(bytes) => E2eOutcome::Verified {
                    version: update.version.clone(),
                    bytes,
                },
                Err(e) => E2eOutcome::Refused(e.to_string()),
            },
        }
    })
}

/// Serve `feed` and `payload`, run the mock app at 0.5.1 pinning key A.
fn e2e_case(
    feed_version: Option<&str>,
    signature: Option<&str>,
    payload: Vec<u8>,
) -> (E2eOutcome, Vec<String>) {
    // Two servers: the feed must name the artifact URL, which needs the artifact server's port.
    let artifact = LoopbackFeed::serve(vec![("/update-payload.bin", payload)]);
    let feed = LoopbackFeed::serve(vec![(
        "/latest.json",
        e2e_feed(&artifact.base, feed_version, signature),
    )]);
    let out = e2e_run(E2E_PUB_A, "0.5.1", &format!("{}/latest.json", feed.base));
    (out, artifact.hits())
}

#[test]
fn the_e2e_goldens_are_signed_by_a_throwaway_key_not_a_release_key() {
    let (_, a) = key_id(&b64(E2E_PUB_A));
    let (_, b) = key_id(&b64(E2E_PUB_B));
    let (_, v2) = key_id(&b64(PUBKEY_V051));
    assert_ne!(a, b, "the two throwaway keys must differ");
    for k in [&a, &b] {
        assert_ne!(k, &v2, "a golden uses the v2 release key");
        assert_ne!(k, "F6E74B821EAAEB32", "a golden uses the v1 release key");
    }
    assert_eq!(key_id(&b64(E2E_SIG_A)).1, a);
    assert_eq!(key_id(&b64(E2E_SIG_B)).1, b);
    // The golden feed has the 0.5.1 shape: every platform a 0.5.1 build looks up, signed by A.
    let r = release(E2E_FEED);
    for (label, keys) in LOOKUPS_V051 {
        for k in keys {
            let sig = r
                .signature(k)
                .unwrap_or_else(|e| panic!("{label} {k}: {e}"));
            assert_eq!(key_id(&b64(sig)).1, a, "{label} {k}");
        }
    }
}

#[test]
fn the_golden_signature_verifies_over_the_artifact_as_minisign_v_checks_it() {
    // What the release checklist's `minisign -V` step checks, with the plugin's verifier crate.
    let pk = |p: &str| {
        minisign_verify::PublicKey::decode(&String::from_utf8(b64(p)).unwrap_or_default())
            .unwrap_or_else(|e| panic!("pubkey: {e}"))
    };
    let sig = |s: &str| {
        minisign_verify::Signature::decode(&String::from_utf8(b64(s)).unwrap_or_default())
            .unwrap_or_else(|e| panic!("signature: {e}"))
    };
    assert!(pk(E2E_PUB_A)
        .verify(E2E_PAYLOAD, &sig(E2E_SIG_A), true)
        .is_ok());
    assert!(pk(E2E_PUB_B)
        .verify(E2E_PAYLOAD, &sig(E2E_SIG_B), true)
        .is_ok());
    assert!(matches!(
        pk(E2E_PUB_A).verify(E2E_PAYLOAD, &sig(E2E_SIG_B), true),
        Err(minisign_verify::Error::UnexpectedKeyId)
    ));
    let mut altered = E2E_PAYLOAD.to_vec();
    altered[4096] ^= 0x80;
    assert!(matches!(
        pk(E2E_PUB_A).verify(&altered, &sig(E2E_SIG_A), true),
        Err(minisign_verify::Error::InvalidSignature)
    ));
    assert!(matches!(
        pk(E2E_PUB_A).verify(E2E_PAYLOAD, &sig(&e2e_altered_signature(E2E_SIG_A)), true),
        Err(minisign_verify::Error::InvalidSignature)
    ));
}

#[test]
fn the_updater_offers_and_verifies_a_newer_update_signed_by_the_pinned_key() {
    let (out, hits) = e2e_case(None, None, E2E_PAYLOAD.to_vec());
    assert_eq!(
        out,
        E2eOutcome::Verified {
            version: "0.5.2".into(),
            bytes: E2E_PAYLOAD.to_vec()
        }
    );
    assert_eq!(hits, ["/update-payload.bin"]);
}

#[test]
fn the_updater_refuses_an_update_whose_signature_was_altered() {
    let bad = e2e_altered_signature(E2E_SIG_A);
    let (out, _) = e2e_case(None, Some(&bad), E2E_PAYLOAD.to_vec());
    assert_eq!(
        out,
        E2eOutcome::Refused("The signature verification failed".into())
    );
}

#[test]
fn the_updater_refuses_an_update_signed_by_another_key() {
    let (out, _) = e2e_case(None, Some(E2E_SIG_B.trim()), E2E_PAYLOAD.to_vec());
    assert_eq!(
        out,
        E2eOutcome::Refused(
            "The signature was created with a different key than the one provided".into()
        )
    );
}

#[test]
fn the_updater_refuses_an_artifact_altered_after_signing() {
    let mut altered = E2E_PAYLOAD.to_vec();
    altered[0] ^= 0x01;
    let (out, _) = e2e_case(None, None, altered);
    assert_eq!(
        out,
        E2eOutcome::Refused("The signature verification failed".into())
    );
}

#[test]
fn the_updater_offers_nothing_when_the_feed_is_not_newer() {
    for v in ["0.5.1", "0.5.0", "0.4.2"] {
        let (out, hits) = e2e_case(Some(v), None, E2E_PAYLOAD.to_vec());
        assert_eq!(out, E2eOutcome::NoUpdate, "feed version {v}");
        assert!(
            hits.is_empty(),
            "feed {v}: the artifact was fetched: {hits:?}"
        );
    }
}

#[test]
fn no_release_config_or_release_path_relaxes_the_updater_or_enables_the_e2e_driver() {
    // Tripwire for the e2e test build's two switches. (1) The test overlay allows plain http to a
    // loopback feed; no config that ships may carry any `dangerous*` updater flag or another key.
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut confs = 0;
    for entry in std::fs::read_dir(dir).unwrap_or_else(|e| panic!("{e}")) {
        let path = entry.unwrap_or_else(|e| panic!("{e}")).path();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_string();
        if !(name.starts_with("tauri") && name.ends_with(".conf.json")) {
            continue;
        }
        confs += 1;
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{name}: {e}"));
        let v: serde_json::Value =
            serde_json::from_str(&text).unwrap_or_else(|e| panic!("{name}: {e}"));
        let up = &v["plugins"]["updater"];
        if let Some(obj) = up.as_object() {
            for k in obj.keys() {
                assert!(!k.starts_with("dangerous"), "{name}: plugins.updater.{k}");
            }
            if name != "tauri.conf.json" {
                assert!(
                    up.get("pubkey").is_none(),
                    "{name} overrides the updater pubkey"
                );
                assert!(
                    up.get("endpoints").is_none(),
                    "{name} overrides the updater endpoints"
                );
            }
        }
        assert!(
            !text.contains("updater-e2e"),
            "{name} names the updater-e2e feature"
        );
    }
    assert!(confs >= 2, "no tauri*.conf.json found in {}", dir.display());
    // (2) The `updater-e2e` feature: not on by default, and no release workflow or release script
    // names it (the K1-L harness under scripts/ci/linux-native-acceptance/ is the only user).
    let cargo: toml::Value =
        toml::from_str(include_str!("../Cargo.toml")).unwrap_or_else(|e| panic!("Cargo.toml: {e}"));
    let default = cargo
        .get("features")
        .and_then(|f| f.get("default"))
        .and_then(|d| d.as_array())
        .cloned()
        .unwrap_or_default();
    assert!(
        !default.iter().any(|f| f.as_str() == Some("updater-e2e")),
        "updater-e2e is a default feature"
    );
    let root = dir.parent().unwrap_or_else(|| panic!("no repo root"));
    let mut release_paths = vec![root.join(".github/workflows/release.yml")];
    for entry in std::fs::read_dir(root.join("scripts")).unwrap_or_else(|e| panic!("{e}")) {
        let p = entry.unwrap_or_else(|e| panic!("{e}")).path();
        if p.is_file() {
            release_paths.push(p);
        }
    }
    for p in &release_paths {
        let text = std::fs::read_to_string(p).unwrap_or_default();
        assert!(
            !text.contains("updater-e2e"),
            "{} names the updater-e2e feature",
            p.display()
        );
    }
    assert!(
        release_paths.len() > 5,
        "release paths not found under {}",
        root.display()
    );
}
