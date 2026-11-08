// CORE-N3 follow-up: "Remove old network data". Only a `chain.<tag>.bak[-n]` directory directly in
// the node data dir, holding only chain database files, at the size the member confirmed, is
// removed. Nothing else (wallet, keys, the current chain, node.toml, the marker, a path outside)
// can be touched through it.

use crate::chain_backup::{list_chain_backups, remove_chain_backup, BackupError};
use std::path::{Path, PathBuf};

fn tmp(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let p = std::env::temp_dir().join(format!(
        "citrate-core-chain-backup-{tag}-{nanos}-{:?}",
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&p).unwrap();
    p
}

/// A node data dir after a 0.5.3 switch: keys, the current chain, node.toml, the marker, and the
/// r1006 chain set aside. Returns the bytes in the backup.
fn switched_node_dir(d: &Path) -> u64 {
    for (f, b) in [
        ("proposer.key", &b"proposer"[..]),
        ("noise.key", b"noise"),
        ("encryption.meta", b"meta"),
        ("node.toml", b"[chain]"),
        ("chain-genesis", b"0x7e57\n"),
        ("CURRENT", b"current chain"),
        ("000001.sst", b"current chain sst"),
    ] {
        std::fs::write(d.join(f), b).unwrap();
    }
    let bak = d.join("chain.r1006.bak");
    std::fs::create_dir(&bak).unwrap();
    let mut n = 0u64;
    for (f, b) in [
        ("CURRENT", &b"old"[..]),
        ("MANIFEST-000005", b"old manifest"),
        ("000624.sst", b"old sst bytes"),
        ("LOCK", b""),
        ("000623.log.dup-1", b"dup"),
    ] {
        std::fs::write(bak.join(f), b).unwrap();
        n += b.len() as u64;
    }
    n
}

fn snapshot(d: &Path) -> Vec<(String, Vec<u8>)> {
    let mut v: Vec<(String, Vec<u8>)> = std::fs::read_dir(d)
        .unwrap()
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
        .map(|e| {
            (
                e.file_name().to_string_lossy().to_string(),
                std::fs::read(e.path()).unwrap(),
            )
        })
        .collect();
    v.sort();
    v
}

#[test]
fn lists_each_backup_with_its_size_and_nothing_else() {
    let d = tmp("list");
    let bytes = switched_node_dir(&d);
    std::fs::create_dir(d.join("chain-aside")).unwrap(); // the chain's own cold-resync dir: not ours
    std::fs::create_dir(d.join("keys")).unwrap();
    std::fs::write(d.join("chain.r1005.bak"), b"a file, not a backup").unwrap();
    let list = list_chain_backups(&d).unwrap();
    assert_eq!(list.len(), 1, "{list:?}");
    assert_eq!(list[0].name, "chain.r1006.bak");
    assert_eq!(list[0].bytes, bytes);
    assert_eq!(list[0].files, 5);
    assert_eq!(list_chain_backups(&d.join("absent")).unwrap(), vec![]);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn removes_the_backup_and_touches_nothing_else() {
    let d = tmp("remove");
    let bytes = switched_node_dir(&d);
    let before = snapshot(&d);
    assert_eq!(remove_chain_backup(&d, "chain.r1006.bak", bytes), Ok(bytes));
    assert!(!d.join("chain.r1006.bak").exists());
    assert_eq!(snapshot(&d), before, "keys, current chain, node.toml and marker unchanged");
    assert_eq!(list_chain_backups(&d).unwrap(), vec![]);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn refuses_any_name_that_is_not_a_backup_directly_in_the_node_dir() {
    let d = tmp("names");
    let bytes = switched_node_dir(&d);
    let outside = tmp("outside");
    std::fs::write(outside.join("CURRENT"), b"must survive").unwrap();
    let outside_s = outside.to_string_lossy().to_string();
    let traversal = "chain.r1006.bak/../..".to_string();
    let parent = "../chain.r1006.bak".to_string();
    let before = snapshot(&d);
    for name in [
        "",
        ".",
        "..",
        "node.toml",
        "proposer.key",
        "CURRENT",
        "keys",
        "chain-aside",
        "chain..bak",
        "chain.r1006.bak/",
        "chain.r1006.bak/CURRENT",
        traversal.as_str(),
        parent.as_str(),
        outside_s.as_str(),
        "/",
        "chain.r1006.bak\0",
        "chain.r1007.bak", // well formed, but absent
    ] {
        let r = remove_chain_backup(&d, name, bytes);
        assert!(
            matches!(r, Err(BackupError::NotABackup(_)) | Err(BackupError::Absent(_))),
            "{name:?}: {r:?}"
        );
    }
    assert!(d.join("chain.r1006.bak").is_dir(), "the backup is still there");
    assert_eq!(snapshot(&d), before);
    assert_eq!(std::fs::read(outside.join("CURRENT")).unwrap(), b"must survive");
    let _ = std::fs::remove_dir_all(&d);
    let _ = std::fs::remove_dir_all(&outside);
}

#[cfg(unix)]
#[test]
fn refuses_a_backup_that_is_a_symlink_or_holds_one() {
    let d = tmp("symlink");
    switched_node_dir(&d);
    let outside = tmp("symlink-target");
    std::fs::write(outside.join("CURRENT"), b"must survive").unwrap();
    // The backup name points outside the node dir.
    std::os::unix::fs::symlink(&outside, d.join("chain.r1005.bak")).unwrap();
    assert!(matches!(
        remove_chain_backup(&d, "chain.r1005.bak", 12),
        Err(BackupError::NotABackup(_))
    ));
    assert!(!list_chain_backups(&d)
        .unwrap()
        .iter()
        .any(|b| b.name == "chain.r1005.bak"));
    // A backup holding a symlink (to the wallet, say) is not removed.
    std::os::unix::fs::symlink(outside.join("CURRENT"), d.join("chain.r1006.bak/000700.sst")).unwrap();
    let r = remove_chain_backup(&d, "chain.r1006.bak", 0);
    assert!(matches!(r, Err(BackupError::Unexpected(_))), "{r:?}");
    assert!(d.join("chain.r1006.bak/CURRENT").exists());
    assert_eq!(std::fs::read(outside.join("CURRENT")).unwrap(), b"must survive");
    let _ = std::fs::remove_dir_all(&d);
    let _ = std::fs::remove_dir_all(&outside);
}

#[test]
fn refuses_a_backup_holding_anything_but_chain_database_files() {
    for (what, name) in [
        ("a key", "proposer.key"),
        ("a wallet file", "custody.enc"),
        ("a config", "node.toml"),
        ("a subdirectory", "000001.sst.d"),
    ] {
        let d = tmp("foreign");
        let bytes = switched_node_dir(&d);
        let p = d.join("chain.r1006.bak").join(name);
        if what == "a subdirectory" {
            std::fs::create_dir(&p).unwrap();
        } else {
            std::fs::write(&p, b"keep me").unwrap();
        }
        let r = remove_chain_backup(&d, "chain.r1006.bak", bytes + 7);
        assert!(matches!(r, Err(BackupError::Unexpected(_))), "{what}: {r:?}");
        assert!(p.exists(), "{what} kept");
        assert!(d.join("chain.r1006.bak/CURRENT").exists(), "{what}: nothing removed");
        let _ = std::fs::remove_dir_all(&d);
    }
}

#[test]
fn refuses_when_the_size_is_not_the_one_the_member_confirmed() {
    let d = tmp("size");
    let bytes = switched_node_dir(&d);
    let r = remove_chain_backup(&d, "chain.r1006.bak", bytes + 1);
    assert_eq!(r, Err(BackupError::Changed { confirmed: bytes + 1, now: bytes }));
    assert!(d.join("chain.r1006.bak/CURRENT").exists());
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn refuses_while_a_genesis_switch_is_not_finished() {
    let d = tmp("intent");
    let bytes = switched_node_dir(&d);
    std::fs::write(
        d.join(crate::node_genesis::MIGRATION_INTENT_FILE),
        "nonce 00\nfrom unrecorded\nto unrecorded\naside chain.r1006.bak\n",
    )
    .unwrap();
    let r = remove_chain_backup(&d, "chain.r1006.bak", bytes);
    assert!(matches!(r, Err(BackupError::SwitchInProgress)), "{r:?}");
    assert!(d.join("chain.r1006.bak/CURRENT").exists());
    let _ = std::fs::remove_dir_all(&d);
}
