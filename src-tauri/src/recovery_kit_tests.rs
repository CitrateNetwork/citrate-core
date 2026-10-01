// HUP-S10.5 — device-key recovery kit: round trip and wrong-recovery tests.
// Written red-first (the module body was absent, so these failed to compile), then the
// implementation brought them green.

use super::*;
use std::collections::HashMap;
use std::sync::Mutex;

type KR<T> = std::result::Result<T, citrate_core_kit::custody::CustodyError>;

/// An in-memory keyring so the tests never touch the real OS keychain.
#[derive(Default)]
struct MemKeyring(Mutex<HashMap<String, Vec<u8>>>);

impl crate::custody::Keyring for MemKeyring {
    fn get(&self, account: &str) -> KR<Option<Vec<u8>>> {
        Ok(self.0.lock().unwrap().get(account).cloned())
    }
    fn set(&self, account: &str, secret: &[u8]) -> KR<()> {
        self.0
            .lock()
            .unwrap()
            .insert(account.to_string(), secret.to_vec());
        Ok(())
    }
    fn delete(&self, account: &str) -> KR<()> {
        self.0.lock().unwrap().remove(account);
        Ok(())
    }
}

/// A keyring whose backend is unreachable (fail closed).
struct DeadKeyring;
impl crate::custody::Keyring for DeadKeyring {
    fn get(&self, _a: &str) -> KR<Option<Vec<u8>>> {
        Err(citrate_core_kit::custody::CustodyError::KeyringUnavailable)
    }
    fn set(&self, _a: &str, _s: &[u8]) -> KR<()> {
        Err(citrate_core_kit::custody::CustodyError::KeyringUnavailable)
    }
    fn delete(&self, _a: &str) -> KR<()> {
        Err(citrate_core_kit::custody::CustodyError::KeyringUnavailable)
    }
}

const PASS: &str = "a long recovery passphrase";

fn tmpdir(tag: &str) -> std::path::PathBuf {
    use rand::RngCore;
    let mut d = std::env::temp_dir();
    let mut r = [0u8; 8];
    rand::thread_rng().fill_bytes(&mut r);
    d.push(format!("n4-recovery-test-{tag}-{}", hex::encode(r)));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A keyring holding both device keys, with distinct fixed values.
fn provisioned() -> MemKeyring {
    let k = MemKeyring::default();
    use crate::custody::Keyring;
    k.set("node-storage-key", &[0x11; 32]).unwrap();
    k.set("memory-store-key", &[0x22; 32]).unwrap();
    k
}

fn snapshot(k: &MemKeyring) -> HashMap<String, Vec<u8>> {
    k.0.lock().unwrap().clone()
}

// ---------- the covered key set ----------

#[test]
fn covers_exactly_the_device_minted_keys_and_never_the_wallet() {
    let accounts: Vec<&str> = DEVICE_KEYS.iter().map(|k| k.account).collect();
    assert_eq!(accounts, vec!["node-storage-key", "memory-store-key"]);
    for k in DEVICE_KEYS {
        assert!(
            !k.account.starts_with("custody-"),
            "wallet vault keys are out of scope"
        );
        assert!(
            !k.account.starts_with("comms-"),
            "the comms key is re-derived from the wallet"
        );
        assert_eq!(k.len, 32);
    }
}

#[test]
fn fingerprint_is_stable_short_and_account_bound() {
    let a = fingerprint("node-storage-key", &[7; 32]);
    assert_eq!(a, fingerprint("node-storage-key", &[7; 32]));
    assert_eq!(a.len(), 16);
    assert_ne!(a, fingerprint("memory-store-key", &[7; 32]));
    assert_ne!(a, fingerprint("node-storage-key", &[8; 32]));
    // The fingerprint never contains the key bytes.
    assert!(!a.contains("0707070707"));
}

#[test]
fn collect_refuses_when_a_key_is_missing_or_the_keyring_is_down() {
    let empty = MemKeyring::default();
    assert_eq!(
        collect_keys(&empty).unwrap_err(),
        RecoveryError::NothingToBackUp
    );
    assert_eq!(
        collect_keys(&DeadKeyring).unwrap_err(),
        RecoveryError::KeyringUnavailable
    );
    // One present key is enough to back up.
    let one = MemKeyring::default();
    use crate::custody::Keyring;
    one.set("memory-store-key", &[3; 32]).unwrap();
    let keys = collect_keys(&one).unwrap();
    assert_eq!(keys.len(), 1);
    assert_eq!(keys[0].account, "memory-store-key");
}

// ---------- recovery phrase (sheet) ----------

#[test]
fn phrase_sheet_round_trips_onto_an_empty_keyring() {
    let src = provisioned();
    let keys = collect_keys(&src).unwrap();
    let sheet = render_sheet(&keys, "2026-10-01");
    // 24 words per key, labelled, and no hex key material in the sheet.
    assert!(sheet.contains("[node-storage-key]"));
    assert!(sheet.contains("[memory-store-key]"));
    assert!(!sheet.contains(&hex::encode([0x11u8; 32])));
    assert!(!sheet.to_lowercase().contains("wallet recovery phrase:"));

    let parsed = parse_sheet(&sheet).unwrap();
    let dst = MemKeyring::default();
    let report = restore_keys(&dst, &parsed, None, false).unwrap();
    assert_eq!(
        report.restored,
        vec!["node-storage-key", "memory-store-key"]
    );
    assert!(report.unchanged.is_empty());
    assert_eq!(snapshot(&dst), snapshot(&src));
}

#[test]
fn phrase_sheet_parse_tolerates_case_and_spacing() {
    let keys = collect_keys(&provisioned()).unwrap();
    let sheet = render_sheet(&keys, "2026-10-01");
    let messy: String = sheet
        .lines()
        .map(|l| format!("  {}  ", l.to_uppercase().replace(' ', "   ")))
        .collect::<Vec<_>>()
        .join("\r\n");
    let parsed = parse_sheet(&messy).unwrap();
    assert_eq!(parsed.len(), 2);
    assert_eq!(parsed[0].key.as_slice(), &[0x11; 32]);
}

#[test]
fn phrase_with_a_wrong_word_is_refused_and_writes_nothing() {
    let keys = collect_keys(&provisioned()).unwrap();
    let sheet = render_sheet(&keys, "2026-10-01");
    // Swap the first word of the first phrase for a different valid word.
    let first = keys_first_word(&sheet);
    let other = if first == "zoo" { "abandon" } else { "zoo" };
    let bad = sheet.replacen(&format!("\n{first} "), &format!("\n{other} "), 1);
    assert_ne!(bad, *sheet);
    let err = parse_sheet(&bad).unwrap_err();
    assert_eq!(err, RecoveryError::PhraseInvalid);
}

fn keys_first_word(sheet: &str) -> String {
    let mut after_label = false;
    for l in sheet.lines() {
        if l.starts_with("[node-storage-key]") {
            after_label = true;
            continue;
        }
        if after_label && !l.trim().is_empty() {
            return l.split_whitespace().next().unwrap().to_string();
        }
    }
    panic!("no phrase line");
}

#[test]
fn phrase_with_a_word_that_is_not_in_the_list_is_refused() {
    let keys = collect_keys(&provisioned()).unwrap();
    let sheet = render_sheet(&keys, "2026-10-01");
    let first = keys_first_word(&sheet);
    let bad = sheet.replacen(&format!("\n{first} "), "\ncitrate ", 1);
    assert_eq!(parse_sheet(&bad).unwrap_err(), RecoveryError::PhraseInvalid);
}

#[test]
fn sheet_without_any_phrase_or_with_unknown_label_is_refused() {
    assert_eq!(
        parse_sheet("hello").unwrap_err(),
        RecoveryError::NotARecoveryKit
    );
    let keys = collect_keys(&provisioned()).unwrap();
    let sheet =
        render_sheet(&keys, "2026-10-01").replace("[memory-store-key]", "[custody-master-key]");
    assert_eq!(parse_sheet(&sheet).unwrap_err(), RecoveryError::UnknownKey);
}

// ---------- recovery file (sealed) ----------

#[test]
fn sealed_file_round_trips() {
    let keys = collect_keys(&provisioned()).unwrap();
    let file = seal_kit(PASS.as_bytes(), &keys, "2026-10-01").unwrap();
    assert_eq!(&file[..8], KIT_MAGIC);
    assert!(!file
        .windows(32)
        .any(|w| w == [0x11u8; 32] || w == [0x22u8; 32]));
    let opened = open_kit(PASS.as_bytes(), &file).unwrap();
    let dst = MemKeyring::default();
    restore_keys(&dst, &opened, None, false).unwrap();
    assert_eq!(snapshot(&dst), snapshot(&provisioned()));
}

#[test]
fn sealed_file_with_the_wrong_passphrase_is_refused() {
    let keys = collect_keys(&provisioned()).unwrap();
    let file = seal_kit(PASS.as_bytes(), &keys, "2026-10-01").unwrap();
    assert_eq!(
        open_kit(b"not the passphrase at all", &file).unwrap_err(),
        RecoveryError::WrongPassphraseOrDamaged
    );
}

#[test]
fn sealed_file_tampering_and_foreign_files_are_refused() {
    let keys = collect_keys(&provisioned()).unwrap();
    let mut file = seal_kit(PASS.as_bytes(), &keys, "2026-10-01").unwrap();
    let last = file.len() - 1;
    file[last] ^= 1;
    assert_eq!(
        open_kit(PASS.as_bytes(), &file).unwrap_err(),
        RecoveryError::WrongPassphraseOrDamaged
    );
    // A journal export is not a recovery kit.
    assert_eq!(
        open_kit(PASS.as_bytes(), b"CITJRNL\0\x01rest-of-a-journal-file").unwrap_err(),
        RecoveryError::NotARecoveryKit
    );
}

#[test]
fn file_and_sheet_paths_are_checked() {
    assert!(check_kit_path(
        std::path::Path::new("/tmp/a.citrate-recovery"),
        KitForm::File
    )
    .is_ok());
    assert!(check_kit_path(
        std::path::Path::new("/tmp/a.citrate-recovery.txt"),
        KitForm::Sheet
    )
    .is_ok());
    assert_eq!(
        check_kit_path(
            std::path::Path::new("relative.citrate-recovery"),
            KitForm::File
        )
        .unwrap_err(),
        RecoveryError::UnsafePath
    );
    assert_eq!(
        check_kit_path(std::path::Path::new("/tmp/a.txt"), KitForm::Sheet).unwrap_err(),
        RecoveryError::UnsafePath
    );
    assert_eq!(
        check_kit_path(
            std::path::Path::new("/tmp/a.citrate-recovery"),
            KitForm::Sheet
        )
        .unwrap_err(),
        RecoveryError::UnsafePath
    );
    assert!(check_passphrase("short").is_err());
    assert!(check_passphrase(PASS).is_ok());
}

#[test]
fn writing_both_forms_to_disk_is_owner_only_and_round_trips() {
    let dir = tmpdir("disk");
    let keys = collect_keys(&provisioned()).unwrap();
    let sheet_path = dir.join("kit.citrate-recovery.txt");
    let file_path = dir.join("kit.citrate-recovery");
    write_sheet(&sheet_path, &keys, "2026-10-01").unwrap();
    write_sealed(&file_path, PASS, &keys, "2026-10-01").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for p in [&sheet_path, &file_path] {
            let mode = std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "{p:?}");
        }
    }
    let from_sheet = parse_sheet(&std::fs::read_to_string(&sheet_path).unwrap()).unwrap();
    let from_file = read_sealed(&file_path, PASS).unwrap();
    assert_eq!(from_sheet.len(), 2);
    assert_eq!(from_file.len(), 2);
    for (a, b) in from_sheet.iter().zip(from_file.iter()) {
        assert_eq!(a.account, b.account);
        assert_eq!(a.key.as_slice(), b.key.as_slice());
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

// ---------- wrong recovery ----------

#[test]
fn a_kit_from_another_install_is_refused_against_recorded_fingerprints() {
    // This install recorded its fingerprints when it made its own kit.
    let mine = provisioned();
    let recorded = fingerprints_of(&collect_keys(&mine).unwrap());
    // Another install's kit (different key bytes).
    let theirs = MemKeyring::default();
    use crate::custody::Keyring;
    theirs.set("node-storage-key", &[0x99; 32]).unwrap();
    theirs.set("memory-store-key", &[0x88; 32]).unwrap();
    let foreign = collect_keys(&theirs).unwrap();
    // The keychain was lost: nothing in it now.
    let lost = MemKeyring::default();
    let err = restore_keys(&lost, &foreign, Some(&recorded), false).unwrap_err();
    assert_eq!(err, RecoveryError::WrongRecovery);
    // ... even with replace, a kit for different data is refused.
    let err = restore_keys(&lost, &foreign, Some(&recorded), true).unwrap_err();
    assert_eq!(err, RecoveryError::WrongRecovery);
    assert!(snapshot(&lost).is_empty(), "nothing written on refusal");
}

#[test]
fn restoring_over_a_different_live_key_needs_replace() {
    let kit = collect_keys(&provisioned()).unwrap();
    // After the loss the app minted a fresh key for the node.
    let live = MemKeyring::default();
    use crate::custody::Keyring;
    live.set("node-storage-key", &[0x55; 32]).unwrap();
    let err = restore_keys(&live, &kit, None, false).unwrap_err();
    assert_eq!(
        err,
        RecoveryError::WouldReplaceLiveKey("node-storage-key".into())
    );
    // All-or-nothing: memory-store-key was not written either.
    assert!(live.get("memory-store-key").unwrap().is_none());
    // With replace the kit wins.
    let report = restore_keys(&live, &kit, None, true).unwrap();
    assert_eq!(
        report.restored,
        vec!["node-storage-key", "memory-store-key"]
    );
    assert_eq!(
        live.get("node-storage-key").unwrap().unwrap(),
        vec![0x11; 32]
    );
}

#[test]
fn restoring_the_same_keys_is_a_no_op() {
    let k = provisioned();
    let kit = collect_keys(&k).unwrap();
    let recorded = fingerprints_of(&kit);
    let report = restore_keys(&k, &kit, Some(&recorded), false).unwrap();
    assert!(report.restored.is_empty());
    assert_eq!(
        report.unchanged,
        vec!["node-storage-key", "memory-store-key"]
    );
}

#[test]
fn restore_fails_closed_when_the_keyring_is_down() {
    let kit = collect_keys(&provisioned()).unwrap();
    assert_eq!(
        restore_keys(&DeadKeyring, &kit, None, false).unwrap_err(),
        RecoveryError::KeyringUnavailable
    );
}

#[test]
fn recorded_fingerprints_persist_in_the_data_dir() {
    let dir = tmpdir("fp");
    assert!(read_recorded(&dir).is_none());
    let kit = collect_keys(&provisioned()).unwrap();
    record_fingerprints(&dir, &kit).unwrap();
    let rec = read_recorded(&dir).unwrap();
    assert_eq!(
        rec.get("node-storage-key"),
        Some(&fingerprint("node-storage-key", &[0x11; 32]))
    );
    assert_eq!(rec.len(), 2);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn status_reports_presence_and_fingerprints_but_no_key_bytes() {
    let dir = tmpdir("status");
    let k = provisioned();
    let st = status_of(&k, &dir);
    assert!(st.keyring_reachable);
    assert_eq!(st.keys.len(), 2);
    assert!(st.keys.iter().all(|e| e.present));
    assert!(st.keys.iter().all(|e| e.recorded_fingerprint.is_none()));
    let json = serde_json::to_string(&st).unwrap();
    assert!(!json.contains(&hex::encode([0x11u8; 32])));
    let down = status_of(&DeadKeyring, &dir);
    assert!(!down.keyring_reachable);
    assert!(down.keys.iter().all(|e| !e.present));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn errors_read_as_plain_member_messages() {
    for e in [
        RecoveryError::NothingToBackUp,
        RecoveryError::KeyringUnavailable,
        RecoveryError::PhraseInvalid,
        RecoveryError::NotARecoveryKit,
        RecoveryError::UnknownKey,
        RecoveryError::WrongPassphraseOrDamaged,
        RecoveryError::WrongRecovery,
        RecoveryError::WouldReplaceLiveKey("node-storage-key".into()),
        RecoveryError::PassphraseTooShort,
        RecoveryError::UnsafePath,
        RecoveryError::Io,
    ] {
        let m = e.to_string();
        assert!(!m.is_empty());
        assert!(!m.contains('\u{2014}'), "no em-dash: {m}");
        assert!(!m.to_lowercase().contains("aes"), "no algorithm names: {m}");
    }
}
