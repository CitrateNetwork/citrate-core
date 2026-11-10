//! 0.5.1 -> 0.5.2 upgrade: the app data dir (UPGRADE_051_052 §2).
//!
//! Every fixture under fixtures/upgrade-v0.5.1/app-data was written by the v0.5.1 code that
//! writes it in the app (b521d98e; generator in fixtures/upgrade-v0.5.1/GENERATOR.md). These tests
//! load each one with this build's loader and assert what a 0.5.1 member keeps: no data loss, and a
//! refusal (never a silent empty state) where a file can't be read.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/upgrade-v0.5.1/app-data")
}

/// A private copy of the 0.5.1 app data dir for one test.
fn copy_of_v051(tag: &str) -> PathBuf {
    let dst = std::env::temp_dir().join(format!(
        "citrate-upgrade-v051-app-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dst);
    copy_dir(&fixtures(), &dst);
    dst
}

pub(crate) fn copy_dir(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap_or_else(|e| panic!("mkdir {}: {e}", dst.display()));
    let entries = std::fs::read_dir(src).unwrap_or_else(|e| panic!("read {}: {e}", src.display()));
    for entry in entries.flatten() {
        let p = entry.path();
        let to = dst.join(entry.file_name());
        if p.is_dir() {
            copy_dir(&p, &to);
        } else {
            std::fs::copy(&p, &to).unwrap_or_else(|e| panic!("copy {}: {e}", p.display()));
        }
    }
}

pub(crate) fn read(p: &Path) -> Vec<u8> {
    std::fs::read(p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()))
}

/// An in-memory kubo holding the pins the 0.5.1 node holds.
#[derive(Clone, Default)]
struct Kubo(Arc<Mutex<std::collections::BTreeSet<String>>>);
impl crate::storage::KuboTransport for Kubo {
    fn add(
        &self,
        _filename: &str,
        bytes: &[u8],
    ) -> Result<crate::storage::AddOutcome, crate::storage::StorageError> {
        let cid = format!("bafkreiupgradeadded{}", bytes.len());
        Ok(crate::storage::AddOutcome {
            cid,
            size_bytes: bytes.len() as u64,
        })
    }
    fn pin_add(&self, cid: &str) -> Result<(), crate::storage::StorageError> {
        self.0
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(cid.to_string());
        Ok(())
    }
    fn pin_rm(&self, cid: &str) -> Result<(), crate::storage::StorageError> {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).remove(cid);
        Ok(())
    }
    fn pin_ls(&self) -> Result<Vec<String>, crate::storage::StorageError> {
        Ok(self
            .0
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .iter()
            .cloned()
            .collect())
    }
    fn cat(&self, cid: &str) -> Result<Vec<u8>, crate::storage::StorageError> {
        Err(crate::storage::StorageError::Transport(format!(
            "no such object: {cid}"
        )))
    }
}

fn pins_of_v051(dir: &Path) -> Vec<String> {
    let idx: serde_json::Value = serde_json::from_slice(&read(&dir.join("ipfs/pins.json")))
        .unwrap_or_else(|e| panic!("pins.json: {e}"));
    idx["records"]
        .as_object()
        .unwrap_or_else(|| panic!("no records"))
        .keys()
        .cloned()
        .collect()
}

#[test]
fn v051_pins_keep_their_size_bond_and_date() {
    let dir = copy_of_v051("pins");
    let cids = pins_of_v051(&dir);
    assert_eq!(cids.len(), 2, "the fixture holds two local pins");
    let kubo = Kubo::default();
    for c in &cids {
        kubo.0
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(c.clone());
    }
    let mgr = crate::storage::StorageManager::new(Box::new(kubo), dir.join("ipfs"));
    let rows = mgr.list().unwrap_or_else(|e| panic!("list: {e}"));
    assert_eq!(rows.len(), 2);
    for r in &rows {
        assert_eq!(r.bond_salt, crate::storage::LOCAL_PIN_MARKER, "{}", r.cid);
        assert!(r.added_at > 0, "{}: the 0.5.1 date is kept", r.cid);
        assert_eq!(r.pin_state, crate::storage::PinState::Pinned);
    }
    // A pin made after the upgrade keeps the 0.5.1 records beside it.
    mgr.pin_local("bafkreiupgradeafterupgradeafterupgradeafterupgradeaf")
        .unwrap_or_else(|e| panic!("pin: {e}"));
    let after = pins_of_v051(&dir);
    for c in &cids {
        assert!(after.contains(c), "{c} survived the first 0.5.2 write");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_damaged_pin_index_is_kept_not_overwritten_by_the_next_pin() {
    // 0.5.1 wrote pins.json with a plain truncating write, so a crash mid-write leaves a cut file.
    // The first 0.5.2 pin must not silently replace it with a one-entry index: the bytes are kept
    // beside it for recovery, and the new pin is still recorded.
    let dir = copy_of_v051("damaged-pins");
    let path = dir.join("ipfs/pins.json");
    let full = read(&path);
    let cut = &full[..full.len() / 2];
    std::fs::write(&path, cut).unwrap_or_else(|e| panic!("cut: {e}"));
    let mgr = crate::storage::StorageManager::new(Box::new(Kubo::default()), dir.join("ipfs"));
    mgr.pin_local("bafkreiupgradeafterupgradeafterupgradeafterupgradeaf")
        .unwrap_or_else(|e| panic!("pin: {e}"));
    let kept: Vec<PathBuf> = std::fs::read_dir(dir.join("ipfs"))
        .unwrap_or_else(|e| panic!("read ipfs: {e}"))
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("pins.json.damaged-"))
        })
        .collect();
    assert_eq!(
        kept.len(),
        1,
        "the damaged index is set aside, not overwritten"
    );
    assert_eq!(read(&kept[0]), cut, "byte for byte");
    let now: serde_json::Value =
        serde_json::from_slice(&read(&path)).unwrap_or_else(|e| panic!("new index: {e}"));
    assert!(now["records"]
        .get("bafkreiupgradeafterupgradeafterupgradeafterupgradeaf")
        .is_some());
    let _ = std::fs::remove_dir_all(&dir);
}

/// D-12: 0.5.2 has one invite flow (request and approve) and does not read 0.5.1 invites. A 0.5.1
/// data dir opens cleanly: the first read sets the old `pending.json` aside, owner-only with its
/// bytes (and keys) untouched, and reads as no invites; the next invite starts the new format next
/// to it. `referral-log.json` is no longer read or written and is left as it was.
#[test]
fn v051_invites_are_set_aside_kept_and_a_new_store_starts() {
    let dir = copy_of_v051("invites");
    let path = dir.join("invites/pending.json");
    let old = read(&path);
    let referrals = read(&dir.join("invites/referral-log.json"));
    let group = "abababababababababababababababab";
    let now = 1_800_000_000;
    assert_eq!(
        crate::invites::invite_views_at(&path, group, now),
        Ok(Vec::new()),
        "a 0.5.1 store opens as no invites, not an error"
    );
    let aside: Vec<PathBuf> = std::fs::read_dir(dir.join("invites"))
        .unwrap_or_else(|e| panic!("{e}"))
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("pending.json.earlier-"))
        })
        .collect();
    assert_eq!(aside.len(), 1, "{aside:?}");
    assert_eq!(read(&aside[0]), old, "kept byte for byte");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&aside[0])
            .unwrap_or_else(|e| panic!("{e}"))
            .permissions()
            .mode();
        assert_eq!(mode & 0o077, 0, "owner-only: {mode:o}");
    }
    let made = crate::invites::create_at(&path, group, "", "", "Garden club", now)
        .unwrap_or_else(|e| panic!("a new invite on the upgraded dir: {e}"));
    assert_eq!(
        crate::invites::invite_views_at(&path, group, now).map(|v| v.len()),
        Ok(1)
    );
    assert!(made.link.contains("&e="), "a link of this version");
    assert_eq!(read(&aside[0]), old, "still kept");
    assert_eq!(read(&dir.join("invites/referral-log.json")), referrals);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn v051_daemons_load_with_schedule_and_budget() {
    let dir = copy_of_v051("daemons");
    let book = crate::daemons::load_book(&dir.join("daemons.json"))
        .unwrap_or_else(|e| panic!("a 0.5.1 daemons.json must load: {e}"));
    let list = book.list(1_792_300_000_000, -420);
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].name, "Morning brief");
    assert_eq!(list[0].schedule, "0 8 * * *");
    assert_eq!(list[0].budget.max_runs_per_day, 2);
    // Written back, it loads again unchanged (the first 0.5.2 write keeps the member's daemon).
    crate::daemons::store_book(&dir.join("daemons.json"), &book)
        .unwrap_or_else(|e| panic!("store: {e}"));
    let again = crate::daemons::load_book(&dir.join("daemons.json"))
        .unwrap_or_else(|e| panic!("reload: {e}"));
    assert_eq!(again.list(1_792_300_000_000, -420)[0].id, list[0].id);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_damaged_v051_daemons_file_is_refused_not_emptied() {
    let dir = copy_of_v051("daemons-damaged");
    let p = dir.join("daemons.json");
    let full = read(&p);
    std::fs::write(&p, &full[..full.len() / 2]).unwrap_or_else(|e| panic!("cut: {e}"));
    assert!(crate::daemons::load_book(&p).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_model_v051_verified_is_still_ready() {
    let dir = copy_of_v051("model");
    let models = dir.join("models");
    let body = read(&models.join(crate::model::MODEL_FILE));
    use sha2::Digest;
    let hash = hex::encode(sha2::Sha256::digest(&body));
    struct NoNet;
    impl crate::model::ModelTransport for NoNet {
        fn total_size(&self) -> Result<u64, crate::model::ModelError> {
            Err(crate::model::ModelError::NotPresent)
        }
        fn get_from(
            &self,
            _o: u64,
        ) -> Result<Box<dyn std::io::Read + Send>, crate::model::ModelError> {
            Err(crate::model::ModelError::NotPresent)
        }
    }
    let mm = crate::model::ModelManager::new(models, Box::new(NoNet), hash, body.len() as u64);
    assert_eq!(mm.status(), crate::model::ModelStatus::Ready);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn v051_terminal_choice_off_stays_off() {
    let dir = copy_of_v051("terminal");
    let s = crate::hermes_terminal::load(&dir.join("hermes"))
        .unwrap_or_else(|e| panic!("a 0.5.1 terminal-settings.json must load: {e}"));
    assert!(!s.enabled);
    assert!(!crate::hermes_terminal::enabled(&dir.join("hermes")));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_v051_node_data_dir_on_the_same_genesis_is_kept() {
    let dir = copy_of_v051("node");
    let node = dir.join("node");
    let before: Vec<String> = {
        let mut v: Vec<String> = std::fs::read_dir(&node)
            .unwrap_or_else(|e| panic!("{e}"))
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        v.sort();
        v
    };
    // The 0.5.1 fixture is an r1005 node; this build may target another network (r1006 since the
    // RC52 fill, whose switch upgrade_xgenesis_tests.rs covers), so "same genesis" is r1005's.
    let r1005 = crate::network_profile::profile("alpha-r1005")
        .unwrap_or_else(|e| panic!("{e:?}"))
        .genesis_hash
        .clone()
        .unwrap_or_else(|| panic!("r1005 genesis"));
    let out = crate::node_genesis::reconcile_genesis(&node, &r1005, || {
        panic!("a kept data dir never needs the running-node probe")
    })
    .unwrap_or_else(|e| panic!("reconcile: {e:?}"));
    assert_eq!(out, crate::node_genesis::GenesisOutcome::Kept);
    let mut after: Vec<String> = std::fs::read_dir(&node)
        .unwrap_or_else(|e| panic!("{e}"))
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    after.sort();
    assert_eq!(after, before, "nothing in the 0.5.1 node dir is touched");
    let _ = std::fs::remove_dir_all(&dir);
}

/// D-9 (2026-10-26): the liquid staking pool is retired. A 0.5.1 install holds no pool state in its
/// app data (no file names or carries a pool address, selector, event or field), so the only pool
/// value an upgrade meets is the webview's saved self-stake, which loadState drops
/// (src/shell/upgradeV051.test.ts).
#[test]
fn a_v051_install_holds_no_pool_state_in_its_app_data() {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for e in std::fs::read_dir(dir)
            .unwrap_or_else(|e| panic!("read {}: {e}", dir.display()))
            .flatten()
        {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else {
                out.push(p);
            }
        }
    }
    let mut all = Vec::new();
    walk(&fixtures(), &mut all);
    assert!(all.len() > 10, "the 0.5.1 app data fixture is missing");
    let mut found = Vec::new();
    for p in all {
        let name = p.to_string_lossy().to_string();
        let body = String::from_utf8_lossy(&std::fs::read(&p).unwrap_or_default()).to_string();
        for h in crate::lsp_retired_tests::retired_hits(&format!("{name}\n{body}")) {
            found.push(format!("{name}: {h}"));
        }
    }
    assert!(
        found.is_empty(),
        "0.5.1 app data with pool state: {found:?}"
    );
}

#[test]
fn pin_index_writers_never_load_the_index_leniently() {
    // Tripwire for the bug class above: only the read-only `list` may read the index leniently
    // (a damaged file shown as empty). A writer that did the same would save over the damaged
    // file again, and an index written in place could be cut by a crash.
    let src = include_str!("storage.rs");
    let body = src
        .split("#[cfg(test)]\nmod tests")
        .next()
        .unwrap_or_else(|| panic!("storage.rs has no body"));
    let lenient: Vec<&str> = body
        .lines()
        .filter(|l| l.contains("PinIndex::load_for_display("))
        .collect();
    assert_eq!(
        lenient.len(),
        1,
        "only list reads the index leniently: {lenient:?}"
    );
    let list_at = body
        .find("pub fn list(&self)")
        .unwrap_or_else(|| panic!("no list"));
    let lenient_at = body
        .find("PinIndex::load_for_display(")
        .unwrap_or_else(|| panic!("no lenient read"));
    assert!(lenient_at > list_at, "the lenient read is inside list");
    for needle in ["PinIndex::load(", "serde_json::from_str(&s).ok()"] {
        assert!(!body.contains(needle), "{needle} reads the index leniently");
    }
    let impl_at = body
        .find("impl PinIndex {")
        .unwrap_or_else(|| panic!("no impl PinIndex"));
    let impl_end = body[impl_at..]
        .find("\n}\n")
        .map(|i| impl_at + i)
        .unwrap_or(body.len());
    let imp = &body[impl_at..impl_end];
    assert!(
        !imp.contains("std::fs::write("),
        "the index is saved through a temp file"
    );
    assert!(imp.contains("sync_all()") && imp.contains("std::fs::rename("));
}
