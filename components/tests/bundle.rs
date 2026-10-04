//! HUP-S6.1: the toolchain bundle definitions are complete, honest about what was measured,
//! agree with the template pins (one source of truth), and turn into a manifest the updater
//! accepts once signed.
mod common;

use citrate_components::bundle::{
    check_bundle, manifest_from_bundle, ArtifactStatus, Bundle, DepsLock,
};
use citrate_components::key::TrustRoot;
use citrate_components::manifest::verify_manifest;
use common::*;
use std::collections::BTreeMap;

const BUNDLE: &str = include_str!("../toolchain-bundle.json");
const DEPS: &str = include_str!("../../templates/deps.lock.json");

fn load() -> (Bundle, DepsLock) {
    (
        Bundle::parse(BUNDLE).unwrap(),
        DepsLock::parse(DEPS).unwrap(),
    )
}

#[test]
fn the_shipped_bundle_passes_its_own_check() {
    let (b, d) = load();
    let problems = check_bundle(&b, &d, &repo_root());
    assert!(problems.is_empty(), "{problems:#?}");
}

#[test]
fn every_tool_the_planset_names_is_present() {
    let (b, _) = load();
    let names: Vec<&str> = b.tools.iter().map(|t| t.name.as_str()).collect();
    for want in [
        "solc", "foundry", "slither", "aderyn", "medusa", "node", "python",
    ] {
        assert!(names.contains(&want), "{want} missing from {names:?}");
    }
    let v = |n: &str| {
        b.tools
            .iter()
            .find(|t| t.name == n)
            .map(|t| t.version.clone())
            .unwrap()
    };
    assert_eq!(v("solc"), "0.8.36");
    assert_eq!(v("foundry"), "1.5.1");
}

#[test]
fn macos_arm64_is_measured_for_every_binary_tool() {
    let (b, _) = load();
    for t in &b.tools {
        let a = t
            .artifacts
            .get("macos-arm64")
            .unwrap_or_else(|| panic!("{} has no macos-arm64 entry", t.name));
        if t.name == "slither" {
            assert_eq!(a.status, ArtifactStatus::ToBeBuilt);
            continue;
        }
        assert_eq!(a.status, ArtifactStatus::Measured, "{}", t.name);
        assert_eq!(a.sha256.as_deref().map(str::len), Some(64), "{}", t.name);
        assert!(a.size.unwrap_or(0) > 0, "{}", t.name);
    }
}

#[test]
fn unmeasured_entries_carry_no_hash() {
    let (b, _) = load();
    let mut to_measure = 0;
    for t in &b.tools {
        for (plat, a) in &t.artifacts {
            match a.status {
                ArtifactStatus::Measured => {
                    assert!(a.sha256.is_some() && a.url.is_some(), "{} {plat}", t.name)
                }
                _ => {
                    to_measure += 1;
                    assert!(
                        a.sha256.is_none() && a.size.is_none(),
                        "{} {plat}: an unmeasured entry must not carry a hash",
                        t.name
                    );
                }
            }
        }
    }
    assert!(to_measure > 0);
}

#[test]
fn the_solc_pin_and_library_commits_come_from_deps_lock() {
    let (b, d) = load();
    let solc = b.tools.iter().find(|t| t.name == "solc").unwrap();
    assert_eq!(solc.version, d.solc);
    for (name, dep) in &d.deps {
        let a = b
            .libraries
            .archives
            .get(name)
            .unwrap_or_else(|| panic!("{name} has no archive"));
        assert!(
            a.url.contains(&dep.commit),
            "{name}: archive url must name the pinned commit"
        );
    }
}

#[test]
fn the_check_catches_each_kind_of_problem() {
    let (good, d) = load();
    let mut cases: Vec<(&str, Bundle)> = Vec::new();
    let mut b = good.clone();
    b.tools
        .iter_mut()
        .find(|t| t.name == "solc")
        .unwrap()
        .version = "0.8.35".into();
    cases.push(("solc differs from deps.lock", b));
    let mut b = good.clone();
    b.tools[0].artifacts.get_mut("macos-arm64").unwrap().url =
        Some("http://binaries.soliditylang.org/x".into());
    cases.push(("http url", b));
    let mut b = good.clone();
    b.tools[0].artifacts.get_mut("linux-x64").unwrap().sha256 = Some("ab".repeat(32));
    cases.push(("hash on an unmeasured entry", b));
    let mut b = good.clone();
    b.tools[0].artifacts.get_mut("macos-arm64").unwrap().sha256 = None;
    cases.push(("measured without a hash", b));
    let mut b = good.clone();
    b.tools[0].license = String::new();
    cases.push(("no licence", b));
    let mut b = good.clone();
    let t = b.tools[0].clone();
    b.tools.push(t);
    cases.push(("duplicate tool", b));
    let mut b = good.clone();
    b.libraries.archives.remove("solady");
    cases.push(("library without an archive", b));
    let mut b = good.clone();
    b.libraries.archives.get_mut("solady").unwrap().url =
        "https://api.github.com/repos/Vectorized/solady/tarball/main".into();
    cases.push(("library url not at the pinned commit", b));
    let mut b = good.clone();
    b.libraries
        .archives
        .get_mut("solady")
        .unwrap()
        .license_files = vec!["components/licenses/missing.txt".into()];
    cases.push(("missing licence file", b));
    let mut b = good.clone();
    b.tools[0].artifacts.remove("windows-x64");
    cases.push(("a platform with no entry", b));
    for (label, b) in cases {
        assert!(
            !check_bundle(&b, &d, &repo_root()).is_empty(),
            "{label} was not caught"
        );
    }
}

#[test]
fn manifest_from_bundle_contains_only_measured_artifacts_and_verifies_once_signed() {
    let (b, d) = load();
    let key = TestKey::generate();
    // The release step signs each measured artifact's bytes; the bytes are not in the repo, so
    // a stand-in signature per hash (the manifest check only decodes artifact signatures).
    let mut sigs = BTreeMap::new();
    for t in &b.tools {
        for a in t.artifacts.values() {
            if let (ArtifactStatus::Measured, Some(h)) = (&a.status, &a.sha256) {
                sigs.insert(
                    h.clone(),
                    key.sign(h.as_bytes(), "citrate-components-artifact"),
                );
            }
        }
    }
    for a in b.libraries.archives.values() {
        sigs.insert(
            a.sha256.clone(),
            key.sign(a.sha256.as_bytes(), "citrate-components-artifact"),
        );
    }
    let m = manifest_from_bundle(&b, &d, &sigs, 7, NOW - DAY, NOW + 13 * DAY).unwrap();
    let json = serde_json::to_string_pretty(&m).unwrap();
    let vm = verify_manifest(
        json.as_bytes(),
        &sign_manifest(&key, &json),
        &TrustRoot::from_base64(&key.public_b64()).unwrap(),
        None,
        NOW,
    )
    .unwrap();
    let names: Vec<&str> = vm
        .manifest()
        .components
        .iter()
        .map(|c| c.name.as_str())
        .collect();
    assert!(
        names.contains(&"foundry")
            && names.contains(&"solady")
            && names.contains(&"openzeppelin-contracts")
    );
    assert!(
        !names.contains(&"slither"),
        "slither is not built yet, so it is not in the manifest"
    );
    let foundry = vm
        .manifest()
        .components
        .iter()
        .find(|c| c.name == "foundry")
        .unwrap();
    assert_eq!(
        foundry.artifacts.keys().collect::<Vec<_>>(),
        vec!["macos-arm64"]
    );
    let solc = vm
        .manifest()
        .components
        .iter()
        .find(|c| c.name == "solc")
        .unwrap();
    assert_eq!(
        solc.artifacts.len(),
        2,
        "macos-arm64 and macos-x64 (the same universal file)"
    );
    let oz = vm
        .manifest()
        .components
        .iter()
        .find(|c| c.name == "openzeppelin-contracts")
        .unwrap();
    assert_eq!(oz.version, "5.7.0");
    assert!(oz.artifacts.contains_key("any"));
}

#[test]
fn manifest_from_bundle_refuses_a_measured_artifact_without_a_signature() {
    let (b, d) = load();
    let err = manifest_from_bundle(&b, &d, &BTreeMap::new(), 1, NOW, NOW + DAY).unwrap_err();
    assert!(err.to_string().contains("signature"), "{err}");
}

fn repo_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}
