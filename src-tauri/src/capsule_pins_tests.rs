// HUP-S2.5 — bundled capsule pins, verified seeding, and bundle configs.
use super::*;
use std::path::PathBuf;

fn src_tauri() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// A unique scratch dir, removed on drop.
struct Tmp(PathBuf);

impl Tmp {
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn tmp() -> Tmp {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let p = std::env::temp_dir().join(format!(
        "citrate-core-capsule-pins-{}-{nanos}-{n}",
        std::process::id()
    ));
    std::fs::create_dir_all(&p).expect("mk tmpdir");
    Tmp(p)
}

/// A bundled dir with one capsule `name` whose `.cps` holds `bytes`.
fn bundle_with(root: &Path, name: &str, bytes: &[u8]) {
    let d = root.join(name);
    std::fs::create_dir_all(&d).expect("mkdir");
    std::fs::write(d.join(format!("{name}.cps")), bytes).expect("write cps");
    std::fs::write(d.join("manifest.toml"), format!("name = \"{name}\"\n")).expect("write");
}

fn pin_of(bytes: &[u8]) -> String {
    sha256_hex(bytes)
}

#[test]
fn pins_match_the_bundled_files() {
    let caps = src_tauri().join("capsules");
    for (name, pin) in BUNDLED_CAPSULES {
        verify_cps(&caps.join(name), name, pin)
            .unwrap_or_else(|e| panic!("pin for {name} is stale: {e}"));
    }
    // Every bundled capsule dir is pinned (none ships unverified).
    let mut on_disk: Vec<String> = std::fs::read_dir(&caps)
        .expect("capsules dir")
        .flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    on_disk.sort();
    let mut pinned: Vec<String> = BUNDLED_CAPSULES
        .iter()
        .map(|(n, _)| n.to_string())
        .collect();
    pinned.sort();
    assert_eq!(
        on_disk, pinned,
        "every capsules/<name> dir must be pinned, and only those"
    );
}

#[test]
fn shipped_capsules_seed_verified() {
    let dest = tmp();
    let report = seed_verified(&src_tauri().join("capsules"), dest.path());
    assert!(report.refused.is_empty(), "{:?}", report.refused);
    assert_eq!(
        report.seeded,
        vec!["echo-chain".to_string(), "hello".to_string()]
    );
    assert_eq!(report.present, 2);
    assert!(dest.path().join("hello").join("hello.cps").exists());
}

#[test]
fn a_tampered_bundled_capsule_is_not_seeded() {
    let root = tmp();
    let bundled = root.path().join("bundled");
    bundle_with(&bundled, "hello", b"TAMPERED");
    let pin = pin_of(b"GENUINE");
    let pins = [("hello", pin.as_str())];
    let report = seed_verified_with(&bundled, &root.path().join("dest"), &pins);
    assert!(report.seeded.is_empty());
    assert_eq!(report.refused.len(), 1);
    assert!(
        report.refused[0].1.contains("does not match"),
        "{:?}",
        report.refused
    );
    assert!(!root.path().join("dest").join("hello").exists());
    assert_eq!(report.present, 0);
}

#[test]
fn an_unpinned_bundled_capsule_is_not_seeded() {
    let root = tmp();
    let bundled = root.path().join("bundled");
    bundle_with(&bundled, "stray", b"ANYTHING");
    let report = seed_verified_with(&bundled, &root.path().join("dest"), &[]);
    assert_eq!(report.refused.len(), 1);
    assert!(report.refused[0].1.contains("not a capsule pinned"));
    assert!(!root.path().join("dest").join("stray").exists());
}

#[test]
fn a_missing_cps_is_not_seeded() {
    let root = tmp();
    let bundled = root.path().join("bundled");
    std::fs::create_dir_all(bundled.join("hello")).expect("mkdir");
    std::fs::write(bundled.join("hello").join("capsule.wasm"), b"\0asm").expect("write");
    let pin = pin_of(b"GENUINE");
    let report = seed_verified_with(
        &bundled,
        &root.path().join("dest"),
        &[("hello", pin.as_str())],
    );
    assert_eq!(report.refused.len(), 1);
    assert!(
        report.refused[0].1.contains("cannot read"),
        "{:?}",
        report.refused
    );
}

#[test]
fn a_matching_seeded_copy_is_kept_and_a_tampered_one_is_replaced() {
    let root = tmp();
    let bundled = root.path().join("bundled");
    let dest = root.path().join("dest");
    bundle_with(&bundled, "hello", b"GENUINE");
    let pin = pin_of(b"GENUINE");
    let pins = [("hello", pin.as_str())];

    let first = seed_verified_with(&bundled, &dest, &pins);
    assert_eq!(first.seeded, vec!["hello".to_string()]);
    let again = seed_verified_with(&bundled, &dest, &pins);
    assert_eq!(again.kept, vec!["hello".to_string()]);

    std::fs::write(dest.join("hello").join("hello.cps"), b"EDITED").expect("tamper");
    std::fs::write(dest.join("hello").join("extra.bin"), b"left behind").expect("write");
    let repaired = seed_verified_with(&bundled, &dest, &pins);
    assert_eq!(repaired.repaired, vec!["hello".to_string()]);
    assert_eq!(
        std::fs::read(dest.join("hello").join("hello.cps")).expect("read"),
        b"GENUINE"
    );
    assert!(
        !dest.join("hello").join("extra.bin").exists(),
        "replaced, not merged"
    );
}

#[test]
fn the_members_own_capsules_are_never_touched() {
    let root = tmp();
    let bundled = root.path().join("bundled");
    let dest = root.path().join("dest");
    bundle_with(&bundled, "hello", b"GENUINE");
    std::fs::create_dir_all(dest.join("mine")).expect("mkdir");
    std::fs::write(dest.join("mine").join("mine.cps"), b"MINE").expect("write");
    let pin = pin_of(b"GENUINE");
    let report = seed_verified_with(&bundled, &dest, &[("hello", pin.as_str())]);
    assert_eq!(report.present, 2);
    assert_eq!(
        std::fs::read(dest.join("mine").join("mine.cps")).expect("read"),
        b"MINE"
    );
}

#[test]
fn a_missing_bundled_dir_seeds_nothing() {
    let root = tmp();
    let report = seed_verified(&root.path().join("nope"), &root.path().join("dest"));
    assert_eq!(report, SeedReport::default());
}

/// US-2.5: capsules are in every bundle config, so no build ships without
/// them. Each config that declares bundle resources must list every pinned
/// capsule dir.
#[test]
fn every_bundle_config_ships_every_pinned_capsule() {
    let mut checked = 0;
    for entry in std::fs::read_dir(src_tauri()).expect("src-tauri").flatten() {
        let file = entry.file_name().to_string_lossy().into_owned();
        if !(file.starts_with("tauri.") && file.ends_with(".conf.json")) {
            continue;
        }
        let text = std::fs::read_to_string(entry.path()).expect("read config");
        let json: serde_json::Value = serde_json::from_str(&text).expect("config is JSON");
        let resources = json
            .pointer("/bundle/resources")
            .and_then(|r| r.as_array())
            .unwrap_or_else(|| panic!("{file} has no bundle.resources list"));
        let listed: Vec<&str> = resources.iter().filter_map(|r| r.as_str()).collect();
        for (name, _) in BUNDLED_CAPSULES {
            let want = format!("capsules/{name}/*");
            assert!(listed.contains(&want.as_str()), "{file} must bundle {want}");
        }
        checked += 1;
    }
    assert!(
        checked >= 6,
        "expected the base config and the overlays, saw {checked}"
    );
}
