//! HUP-S5.5 end to end, the way the release ceremony and the app use the updater, against a
//! throwaway minisign-format TEST key generated in the test (never the production key):
//!
//! bundle entry (measured zip with app-bundle symlinks) -> `manifest_from_bundle` with one
//! artifact signature per hash -> manifest signed -> `citrate-components verify-manifest` (the
//! release check, as a process) -> verify -> download to staging -> size, SHA-256 and artifact
//! signature -> unpack -> health check -> swap; then an update whose download does not match
//! its hash is refused with the installed version untouched; then the good update installs and
//! an explicit rollback returns to the first version.
//!
//! `tests/install.rs` covers each refusal on its own (tar.gz, hand-written manifests); this test
//! is the one path through the release tooling and the zip unpacker together.
mod common;

use citrate_components::bundle::{manifest_from_bundle, Bundle, DepsLock};
use citrate_components::error::ComponentError;
use citrate_components::install::{EntrypointsPresent, InstallOutcome, Store};
use citrate_components::key::TrustRoot;
use citrate_components::manifest::{verify_manifest, VerifiedManifest};
use citrate_components::platform::Platform;
use common::*;
use std::collections::BTreeMap;
use std::path::Path;

const NAME: &str = "chromium";
const EXE: &str = "chrome-mac-arm64/Test.app/Contents/MacOS/Test";
const FW: &str = "chrome-mac-arm64/Test.app/Contents/Frameworks/Test Framework.framework";

/// A small archive shaped like the Chrome for Testing app: an executable and a versioned
/// framework reached through `Versions/Current`.
fn app_zip(version: &str) -> Vec<u8> {
    let fw_bin = format!("{FW}/Versions/{version}/Test Framework");
    zip_bytes(&[
        ZEntry::File(EXE, version.as_bytes(), 0o755),
        ZEntry::File(&fw_bin, b"MH_DYLIB", 0o755),
        ZEntry::Symlink(&format!("{FW}/Versions/Current"), version),
        ZEntry::Symlink(
            &format!("{FW}/Test Framework"),
            "Versions/Current/Test Framework",
        ),
    ])
}

/// A one-tool bundle whose macOS arm64 artifact is measured at `url`.
fn bundle(version: &str, url: &str, bytes: &[u8]) -> Bundle {
    let measured = serde_json::json!({
        "url": url,
        "format": "zip",
        "status": "measured",
        "sha256": sha256_hex(bytes),
        "size": bytes.len(),
        "entrypoints": [EXE],
    });
    Bundle::parse(
        &serde_json::json!({
            "schema": 1,
            "wp": "HUP-S5.5 e2e test",
            "note": "test bundle",
            "measured_on": "test",
            "platforms": ["macos-arm64"],
            "tools": [{
                "name": NAME,
                "version": version,
                "kind": "browser",
                "license": "LicenseRef-Test",
                "homepage": "https://example.test/",
                "entrypoints": [EXE],
                "artifacts": {"macos-arm64": measured},
            }],
            "libraries": {"lock": "templates/deps.lock.json", "note": "none", "archives": {}},
        })
        .to_string(),
    )
    .unwrap()
}

struct Release {
    json: String,
    sig: String,
}

/// What the ceremony produces: sign the artifact, build the manifest from the bundle, sign it.
fn release(key: &TestKey, b: &Bundle, bytes: &[u8], sequence: u64) -> Release {
    let deps = DepsLock::parse(r#"{"solc":"0.8.36","deps":{}}"#).unwrap();
    let t = &b.tools[0];
    let mut sigs = BTreeMap::new();
    sigs.insert(
        sha256_hex(bytes),
        key.sign(
            bytes,
            &format!("citrate-components-artifact {} {}", t.name, t.version),
        ),
    );
    let m = manifest_from_bundle(b, &deps, &sigs, sequence, NOW - DAY, NOW + 13 * DAY).unwrap();
    let json = serde_json::to_string_pretty(&m).unwrap();
    let sig = key.sign(
        json.as_bytes(),
        &format!("citrate-components-manifest {sequence}"),
    );
    Release { json, sig }
}

/// Step 6 of the ceremony, run as the release tool itself.
fn cli_verify(dir: &Path, key: &TestKey, r: &Release) -> String {
    let m = dir.join("manifest.json");
    let s = dir.join("manifest.json.minisig");
    std::fs::write(&m, &r.json).unwrap();
    std::fs::write(&s, &r.sig).unwrap();
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_citrate-components"))
        .args(["verify-manifest", "--manifest"])
        .arg(&m)
        .arg("--sig")
        .arg(&s)
        .args(["--pubkey", &key.public_b64(), "--now", &NOW.to_string()])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "verify-manifest: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

fn verified(store: &Store, root: &TrustRoot, r: &Release) -> VerifiedManifest {
    let seen = store.state().unwrap().last_manifest;
    verify_manifest(r.json.as_bytes(), &r.sig, root, seen.as_ref(), NOW).unwrap()
}

fn current_exe(store: &Store) -> Vec<u8> {
    let dir = store.current_dir(NAME).unwrap().unwrap();
    std::fs::read(dir.join(EXE)).unwrap()
}

#[test]
fn a_zip_component_goes_from_the_bundle_to_install_refuses_a_bad_hash_and_rolls_back() {
    let t = TempDir::new("e2e");
    let key = TestKey::generate();
    let root = TrustRoot::from_base64(&key.public_b64()).unwrap();
    let f = DirFetcher::new(&t.path().join("srv"));
    let store = Store::open(&t.path().join("components")).unwrap();
    let install = |vm: &VerifiedManifest| {
        store.install(
            vm,
            NAME,
            Platform::MacosArm64,
            &root,
            &f,
            &EntrypointsPresent,
            NOW,
        )
    };

    // 1. First release, first install.
    let v1 = app_zip("154.0.8037.92");
    let url1 = f.put("chrome-v1.zip", &v1);
    let r1 = release(&key, &bundle("154.0.8037.92", &url1, &v1), &v1, 1);
    let checked = cli_verify(t.path(), &key, &r1);
    assert!(checked.contains("manifest ok: sequence 1"), "{checked}");
    assert!(checked.contains("chromium 154.0.8037.92"), "{checked}");
    let vm1 = verified(&store, &root, &r1);
    assert_eq!(
        install(&vm1).unwrap(),
        InstallOutcome::Installed {
            version: "154.0.8037.92".into(),
            previous: None
        }
    );
    assert_eq!(current_exe(&store), b"154.0.8037.92");
    let dir1 = store.current_dir(NAME).unwrap().unwrap();
    assert_eq!(
        std::fs::read(dir1.join(FW).join("Test Framework")).unwrap(),
        b"MH_DYLIB",
        "the framework link resolves through Versions/Current"
    );

    // 2. Second release; the mirror serves bytes of the same size that do not match the hash.
    let v2 = app_zip("154.0.8037.93");
    let url2 = f.put("chrome-v2.zip", &v2);
    let r2 = release(&key, &bundle("154.0.8037.93", &url2, &v2), &v2, 2);
    cli_verify(t.path(), &key, &r2);
    let vm2 = verified(&store, &root, &r2);
    let mut tampered = v2.clone();
    let at = tampered
        .windows(13)
        .position(|w| w == b"154.0.8037.93")
        .unwrap();
    tampered[at] = b'9';
    assert_eq!(tampered.len(), v2.len());
    f.put("chrome-v2.zip", &tampered);
    let err = install(&vm2).unwrap_err();
    assert!(matches!(err, ComponentError::HashMismatch), "{err:?}");
    assert_eq!(
        current_exe(&store),
        b"154.0.8037.92",
        "current is untouched"
    );
    let staging = t.path().join("components/.staging");
    assert!(!staging.exists() || std::fs::read_dir(&staging).unwrap().next().is_none());

    // 3. The mirror is fixed: the same signed manifest now installs.
    f.put("chrome-v2.zip", &v2);
    assert_eq!(
        install(&vm2).unwrap(),
        InstallOutcome::Installed {
            version: "154.0.8037.93".into(),
            previous: Some("154.0.8037.92".into())
        }
    );
    assert_eq!(current_exe(&store), b"154.0.8037.93");

    // 4. Explicit rollback to the first version.
    assert_eq!(store.rollback(NAME).unwrap().version, "154.0.8037.92");
    assert_eq!(current_exe(&store), b"154.0.8037.92");
    assert_eq!(store.state().unwrap().last_manifest.unwrap().sequence, 2);

    // 5. The first manifest can no longer be replayed.
    let seen = store.state().unwrap().last_manifest;
    let replay = verify_manifest(r1.json.as_bytes(), &r1.sig, &root, seen.as_ref(), NOW);
    assert!(
        matches!(replay, Err(ComponentError::ManifestRollback { .. })),
        "{replay:?}"
    );
}
