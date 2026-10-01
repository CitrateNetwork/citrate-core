//! HUP-S5.5: download to staging, verify, then swap; a failure anywhere before the commit
//! leaves the installed version exactly as it was; the previous version can be rolled back to.
mod common;

use citrate_components::error::ComponentError;
use citrate_components::install::{EntrypointsPresent, HealthCheck, InstallOutcome, Store};
use citrate_components::key::TrustRoot;
use citrate_components::manifest::{verify_manifest, Component, VerifiedManifest};
use citrate_components::platform::Platform;
use common::*;
use std::path::Path;

struct Env {
    t: TempDir,
    key: TestKey,
    f: DirFetcher,
    store: Store,
}

fn env(tag: &str) -> Env {
    let t = TempDir::new(tag);
    let f = DirFetcher::new(&t.path().join("srv"));
    let store = Store::open(&t.path().join("components")).unwrap();
    Env {
        t,
        key: TestKey::generate(),
        f,
        store,
    }
}

fn root(e: &Env) -> TrustRoot {
    TrustRoot::from_base64(&e.key.public_b64()).unwrap()
}

fn verified(e: &Env, json: &str) -> VerifiedManifest {
    let seen = e.store.state().unwrap().last_manifest;
    verify_manifest(
        json.as_bytes(),
        &sign_manifest(&e.key, json),
        &root(e),
        seen.as_ref(),
        NOW,
    )
    .unwrap()
}

fn foundry_tgz(tag: &[u8]) -> Vec<u8> {
    tar_gz(&[
        Entry::File("forge", tag, 0o755),
        Entry::File("anvil", tag, 0o755),
    ])
}

fn install(e: &Env, vm: &VerifiedManifest, name: &str) -> Result<InstallOutcome, ComponentError> {
    e.store.install(
        vm,
        name,
        Platform::MacosArm64,
        &root(e),
        &e.f,
        &EntrypointsPresent,
        NOW,
    )
}

fn current_file(e: &Env, name: &str, file: &str) -> Vec<u8> {
    let dir = e.store.current_dir(name).unwrap().unwrap();
    std::fs::read(dir.join(file)).unwrap()
}

fn staging_is_empty(e: &Env) -> bool {
    let s = e.t.path().join("components/.staging");
    !s.exists() || std::fs::read_dir(&s).unwrap().next().is_none()
}

#[test]
fn install_then_update_then_rollback() {
    let e = env("cycle");
    let m1 = manifest_json(
        &e.key,
        &e.f,
        1,
        "foundry",
        "1.5.0",
        "macos-arm64",
        "tar.gz",
        &foundry_tgz(b"v150"),
        &["forge", "anvil"],
    );
    let vm1 = verified(&e, &m1);
    let out = install(&e, &vm1, "foundry").unwrap();
    assert_eq!(
        out,
        InstallOutcome::Installed {
            version: "1.5.0".into(),
            previous: None
        }
    );
    assert_eq!(current_file(&e, "foundry", "forge"), b"v150");

    let m2 = manifest_json(
        &e.key,
        &e.f,
        2,
        "foundry",
        "1.5.1",
        "macos-arm64",
        "tar.gz",
        &foundry_tgz(b"v151"),
        &["forge", "anvil"],
    );
    let vm2 = verified(&e, &m2);
    let out = install(&e, &vm2, "foundry").unwrap();
    assert_eq!(
        out,
        InstallOutcome::Installed {
            version: "1.5.1".into(),
            previous: Some("1.5.0".into())
        }
    );
    assert_eq!(current_file(&e, "foundry", "forge"), b"v151");
    assert!(staging_is_empty(&e));

    // Installing the same thing again is a no-op that downloads nothing.
    let before = e.f.served.borrow().len();
    assert_eq!(
        install(&e, &vm2, "foundry").unwrap(),
        InstallOutcome::AlreadyCurrent {
            version: "1.5.1".into()
        }
    );
    assert_eq!(e.f.served.borrow().len(), before);

    let back = e.store.rollback("foundry").unwrap();
    assert_eq!(back.version, "1.5.0");
    assert_eq!(current_file(&e, "foundry", "forge"), b"v150");
    // And forward again (rollback swaps current and previous).
    assert_eq!(e.store.rollback("foundry").unwrap().version, "1.5.1");

    let st = e.store.state().unwrap();
    assert_eq!(st.last_manifest.unwrap().sequence, 2);
}

#[test]
fn only_two_versions_are_kept_on_disk() {
    let e = env("prune");
    for (seq, v) in [(1u64, "1.0.0"), (2, "1.0.1"), (3, "1.0.2")] {
        let m = manifest_json(
            &e.key,
            &e.f,
            seq,
            "medusa",
            v,
            "macos-arm64",
            "raw",
            v.as_bytes(),
            &["medusa"],
        );
        install(&e, &verified(&e, &m), "medusa").unwrap();
    }
    let dirs: Vec<_> = std::fs::read_dir(e.t.path().join("components/medusa"))
        .unwrap()
        .collect();
    assert_eq!(dirs.len(), 2, "current + previous only");
}

/// Installs 1.0.0, then tries 1.0.1 with `break_it` applied; the install must fail with an
/// error matching `expect`, and 1.0.0 must still be current and intact.
fn failed_update_leaves_current_untouched(
    tag: &str,
    break_it: impl Fn(&mut serde_json::Value, &Env),
    expect: fn(&ComponentError) -> bool,
) {
    let e = env(tag);
    let m1 = manifest_json(
        &e.key,
        &e.f,
        1,
        "solc",
        "0.8.35",
        "macos-arm64",
        "raw",
        b"old-solc",
        &["solc"],
    );
    install(&e, &verified(&e, &m1), "solc").unwrap();
    let installed_before = e.store.state().unwrap().components;

    let m2 = manifest_json(
        &e.key,
        &e.f,
        2,
        "solc",
        "0.8.36",
        "macos-arm64",
        "raw",
        b"new-solc",
        &["solc"],
    );
    let mut v: serde_json::Value = serde_json::from_str(&m2).unwrap();
    break_it(&mut v, &e);
    let m2 = v.to_string();
    let vm2 = verified(&e, &m2);
    let err = install(&e, &vm2, "solc").unwrap_err();
    assert!(expect(&err), "{tag}: {err:?}");

    assert_eq!(current_file(&e, "solc", "solc"), b"old-solc", "{tag}");
    let after = e.store.state().unwrap();
    assert_eq!(
        after.components, installed_before,
        "{tag}: installed versions unchanged"
    );
    // The newer manifest is still recorded, so an older one cannot be replayed afterwards.
    assert_eq!(after.last_manifest.map(|m| m.sequence), Some(2), "{tag}");
    assert!(staging_is_empty(&e), "{tag}: staging cleaned");
    let dirs: Vec<_> = std::fs::read_dir(e.t.path().join("components/solc"))
        .unwrap()
        .collect();
    assert_eq!(dirs.len(), 1, "{tag}: no half-installed version dir");
}

fn art(v: &mut serde_json::Value) -> &mut serde_json::Value {
    &mut v["components"][0]["artifacts"]["macos-arm64"]
}

#[test]
fn a_hash_mismatch_is_refused_and_current_is_kept() {
    failed_update_leaves_current_untouched(
        "hash",
        |v, _| art(v)["sha256"] = sha256_hex(b"something else").into(),
        |e| matches!(e, ComponentError::HashMismatch),
    );
}

#[test]
fn a_size_mismatch_is_refused_and_current_is_kept() {
    failed_update_leaves_current_untouched(
        "size-short",
        |v, _| art(v)["size"] = 100.into(),
        |e| matches!(e, ComponentError::SizeMismatch { .. }),
    );
    failed_update_leaves_current_untouched(
        "size-long",
        |v, _| art(v)["size"] = 3.into(),
        |e| matches!(e, ComponentError::ArtifactTooLarge { .. }),
    );
}

#[test]
fn an_artifact_signed_by_another_key_is_refused_and_current_is_kept() {
    failed_update_leaves_current_untouched(
        "artsig",
        |v, _| {
            let other = TestKey::generate();
            art(v)["signature"] = other
                .sign(b"new-solc", "citrate-components-artifact solc 0.8.36")
                .into();
        },
        |e| matches!(e, ComponentError::SignatureInvalid(_)),
    );
}

#[test]
fn an_artifact_signature_with_the_manifest_comment_is_refused() {
    failed_update_leaves_current_untouched(
        "artcomment",
        |v, e| {
            art(v)["signature"] = e
                .key
                .sign(b"new-solc", "citrate-components-manifest")
                .into()
        },
        |e| matches!(e, ComponentError::WrongTrustedComment),
    );
}

#[test]
fn a_swapped_artifact_is_refused_and_current_is_kept() {
    // The server returns different bytes than the manifest names (a compromised mirror).
    failed_update_leaves_current_untouched(
        "swap",
        |v, e| {
            let url = art(v)["url"].as_str().unwrap().to_string();
            let name = url.rsplit('/').next().unwrap().to_string();
            std::fs::write(e.f.dir.join(name), b"evil-sol").unwrap();
        },
        |e| matches!(e, ComponentError::HashMismatch),
    );
}

#[test]
fn a_failed_health_check_is_refused_and_current_is_kept() {
    failed_update_leaves_current_untouched(
        "health",
        |v, _| v["components"][0]["entrypoints"] = serde_json::json!(["bin/solc"]),
        |e| matches!(e, ComponentError::HealthCheck(_)),
    );
}

#[test]
fn a_fetch_failure_is_refused_and_current_is_kept() {
    failed_update_leaves_current_untouched(
        "fetch",
        |v, _| art(v)["url"] = "https://fixture.test/missing".into(),
        |e| matches!(e, ComponentError::Fetch(_)),
    );
}

struct AlwaysFails;
impl HealthCheck for AlwaysFails {
    fn check(&self, _tree: &Path, _c: &Component) -> Result<(), String> {
        Err("the probe failed".into())
    }
}

#[test]
fn a_custom_health_check_failure_aborts_the_swap() {
    let e = env("probe");
    let m = manifest_json(
        &e.key,
        &e.f,
        1,
        "node",
        "24.21.0",
        "macos-arm64",
        "raw",
        b"node",
        &["node"],
    );
    let vm = verified(&e, &m);
    let err = e
        .store
        .install(
            &vm,
            "node",
            Platform::MacosArm64,
            &root(&e),
            &e.f,
            &AlwaysFails,
            NOW,
        )
        .unwrap_err();
    assert!(matches!(err, ComponentError::HealthCheck(_)), "{err:?}");
    assert!(e.store.current_dir("node").unwrap().is_none());
    assert!(staging_is_empty(&e));
}

#[test]
fn install_refuses_a_manifest_older_than_the_one_recorded() {
    let e = env("older");
    let m2 = manifest_json(
        &e.key,
        &e.f,
        2,
        "solc",
        "0.8.36",
        "macos-arm64",
        "raw",
        b"a",
        &["solc"],
    );
    let vm2 = verified(&e, &m2);
    let m1 = manifest_json(
        &e.key,
        &e.f,
        1,
        "solc",
        "0.8.35",
        "macos-arm64",
        "raw",
        b"b",
        &["solc"],
    );
    let vm1 = verified(&e, &m1); // verified before 2 was recorded
    install(&e, &vm2, "solc").unwrap();
    let err = install(&e, &vm1, "solc").unwrap_err();
    assert!(
        matches!(err, ComponentError::ManifestRollback { seen: 2, got: 1 }),
        "{err:?}"
    );
    assert_eq!(current_file(&e, "solc", "solc"), b"a");
}

#[test]
fn unknown_component_or_platform_is_refused() {
    let e = env("unknown");
    let m = manifest_json(
        &e.key,
        &e.f,
        1,
        "solc",
        "0.8.36",
        "linux-x64",
        "raw",
        b"a",
        &["solc"],
    );
    let vm = verified(&e, &m);
    let err = install(&e, &vm, "solc").unwrap_err();
    assert!(
        matches!(err, ComponentError::NoArtifactForPlatform { .. }),
        "{err:?}"
    );
    let err = install(&e, &vm, "chromium").unwrap_err();
    assert!(
        matches!(err, ComponentError::UnknownComponent(_)),
        "{err:?}"
    );
}

#[test]
fn an_any_platform_artifact_is_used_when_no_specific_one_exists() {
    let e = env("any");
    let lib = tar_gz(&[Entry::File(
        "contracts/ERC721.sol",
        b"// SPDX-License-Identifier: MIT",
        0o644,
    )]);
    let m = manifest_json(
        &e.key,
        &e.f,
        1,
        "openzeppelin-contracts",
        "5.7.0",
        "any",
        "tar.gz",
        &lib,
        &["contracts/ERC721.sol"],
    );
    install(&e, &verified(&e, &m), "openzeppelin-contracts").unwrap();
    assert!(e
        .store
        .current_dir("openzeppelin-contracts")
        .unwrap()
        .is_some());
}

#[test]
fn rollback_without_a_previous_version_is_refused() {
    let e = env("noprev");
    let err = e.store.rollback("solc").unwrap_err();
    assert!(matches!(err, ComponentError::NoPrevious(_)), "{err:?}");
    let m = manifest_json(
        &e.key,
        &e.f,
        1,
        "solc",
        "0.8.36",
        "macos-arm64",
        "raw",
        b"a",
        &["solc"],
    );
    install(&e, &verified(&e, &m), "solc").unwrap();
    let err = e.store.rollback("solc").unwrap_err();
    assert!(matches!(err, ComponentError::NoPrevious(_)), "{err:?}");
}

#[test]
fn recover_removes_leftover_staging_and_unreferenced_versions() {
    let e = env("recover");
    let m = manifest_json(
        &e.key,
        &e.f,
        1,
        "solc",
        "0.8.36",
        "macos-arm64",
        "raw",
        b"a",
        &["solc"],
    );
    install(&e, &verified(&e, &m), "solc").unwrap();
    // Simulate a crash: a staging dir and a renamed-but-uncommitted version dir.
    let root_dir = e.t.path().join("components");
    std::fs::create_dir_all(root_dir.join(".staging/solc-crashed/tree")).unwrap();
    std::fs::create_dir_all(root_dir.join("solc/0.8.37-deadbeefdead")).unwrap();
    let rep = e.store.recover().unwrap();
    assert_eq!(rep.removed_staging, 1);
    assert_eq!(rep.removed_versions, 1);
    assert!(staging_is_empty(&e));
    assert_eq!(current_file(&e, "solc", "solc"), b"a");
}

#[test]
fn a_corrupt_state_file_is_an_error_not_a_silent_reset() {
    let e = env("corrupt");
    std::fs::write(e.t.path().join("components/state.json"), b"{not json").unwrap();
    assert!(matches!(
        e.store.state().unwrap_err(),
        ComponentError::Io(_) | ComponentError::State(_)
    ));
}

#[cfg(unix)]
#[test]
fn the_store_root_is_owner_only() {
    use std::os::unix::fs::PermissionsExt as _;
    let e = env("perm");
    let mode = std::fs::metadata(e.t.path().join("components"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o700);
}
