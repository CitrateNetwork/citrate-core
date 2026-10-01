// HUP-S10.4 — journal encrypted export: format + crypto + file-path tests.
// Written red-first (module bodies absent → these failed to compile, then failed
// on behaviour), then the implementation brought them green.

use super::*;

const PASS: &str = "a long journal passphrase";
const BUNDLE: &[u8] = br#"{"format":"citrate-journal-bundle","version":1,"pages":[]}"#;

fn tmpdir(tag: &str) -> std::path::PathBuf {
    let mut d = std::env::temp_dir();
    let mut r = [0u8; 8];
    rand::thread_rng().fill_bytes(&mut r);
    d.push(format!("n3-journal-test-{tag}-{}", hex::encode(r)));
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn seal_then_open_round_trips() {
    let file = seal_journal(PASS.as_bytes(), BUNDLE).unwrap();
    let out = open_journal(PASS.as_bytes(), &file).unwrap();
    assert_eq!(&out[..], BUNDLE);
}

#[test]
fn header_is_versioned_and_self_describing() {
    let file = seal_journal(PASS.as_bytes(), BUNDLE).unwrap();
    assert_eq!(&file[..8], MAGIC);
    assert_eq!(file[8], FORMAT_VERSION);
    assert_eq!(file[9], KDF_ARGON2ID_V13);
    let (m, t, p) = citrate_core_kit::custody::PASSPHRASE_KDF_PARAMS;
    assert_eq!(u32::from_be_bytes(file[10..14].try_into().unwrap()), m);
    assert_eq!(u32::from_be_bytes(file[14..18].try_into().unwrap()), t);
    assert_eq!(u32::from_be_bytes(file[18..22].try_into().unwrap()), p);
    assert_eq!(file[22] as usize, SALT_LEN);
    assert_eq!(file[23 + SALT_LEN], AEAD_AES256GCM);
    assert_eq!(file[24 + SALT_LEN] as usize, NONCE_LEN);
    // ciphertext = plaintext + 16-byte tag; no plaintext appears in the file
    assert_eq!(file.len(), HEADER_LEN + BUNDLE.len() + 16);
    assert!(!file.windows(16).any(|w| w == &BUNDLE[..16]));
}

#[test]
fn each_export_uses_a_fresh_salt_and_nonce() {
    let a = seal_journal(PASS.as_bytes(), BUNDLE).unwrap();
    let b = seal_journal(PASS.as_bytes(), BUNDLE).unwrap();
    assert_ne!(a[23..23 + SALT_LEN], b[23..23 + SALT_LEN]);
    assert_ne!(a[25 + SALT_LEN..HEADER_LEN], b[25 + SALT_LEN..HEADER_LEN]);
    assert_ne!(a, b);
}

#[test]
fn wrong_passphrase_fails_cleanly() {
    let file = seal_journal(PASS.as_bytes(), BUNDLE).unwrap();
    let err = open_journal(b"not the passphrase", &file).unwrap_err();
    assert_eq!(err, JournalCryptoError::WrongPassphraseOrDamaged);
}

#[test]
fn any_tampered_byte_fails_closed() {
    let file = seal_journal(PASS.as_bytes(), BUNDLE).unwrap();
    // Every fixed header byte is checked before derivation (cheap: flip them all).
    // Salt, nonce and ciphertext are bound by the AEAD (the header is the AAD);
    // each Argon2id trial costs ~1 s in a debug build, so sample those regions at
    // both ends plus the middle of the ciphertext and the tag.
    let last = file.len() - 1;
    let sampled = [
        23,
        23 + SALT_LEN - 1,
        25 + SALT_LEN,
        HEADER_LEN - 1,
        HEADER_LEN,
        HEADER_LEN + BUNDLE.len() / 2,
        last - 8,
        last,
    ];
    for i in (0..23).chain(23 + SALT_LEN..25 + SALT_LEN).chain(sampled) {
        let mut t = file.clone();
        t[i] ^= 0x01;
        assert!(
            open_journal(PASS.as_bytes(), &t).is_err(),
            "flip at byte {i} was accepted"
        );
    }
}

#[test]
fn foreign_truncated_and_unsupported_files_are_named() {
    assert_eq!(
        open_journal(PASS.as_bytes(), b"hello").unwrap_err(),
        JournalCryptoError::NotAJournalFile
    );
    let file = seal_journal(PASS.as_bytes(), BUNDLE).unwrap();
    assert_eq!(
        open_journal(PASS.as_bytes(), &file[..HEADER_LEN]).unwrap_err(),
        JournalCryptoError::NotAJournalFile
    );
    let mut v = file.clone();
    v[8] = 2;
    assert_eq!(
        open_journal(PASS.as_bytes(), &v).unwrap_err(),
        JournalCryptoError::UnsupportedVersion
    );
    // KDF params other than the published ones are refused before any derivation
    // (a crafted file cannot make the app spend unbounded memory).
    let mut m = file.clone();
    m[10..14].copy_from_slice(&u32::MAX.to_be_bytes());
    assert_eq!(
        open_journal(PASS.as_bytes(), &m).unwrap_err(),
        JournalCryptoError::UnsupportedParameters
    );
    let mut a = file.clone();
    a[23 + SALT_LEN] = 9;
    assert_eq!(
        open_journal(PASS.as_bytes(), &a).unwrap_err(),
        JournalCryptoError::UnsupportedParameters
    );
}

#[test]
fn export_passphrase_policy() {
    assert_eq!(
        check_export_passphrase("short").unwrap_err(),
        JournalCryptoError::PassphraseTooShort
    );
    // counted in characters, not bytes
    assert!(check_export_passphrase("ééééééééééé").is_err());
    assert!(check_export_passphrase("éééééééééééé").is_ok());
    assert!(check_export_passphrase(PASS).is_ok());
}

#[test]
fn export_path_must_be_absolute_with_the_journal_extension() {
    assert!(check_export_path(std::path::Path::new("relative.citrate-journal")).is_err());
    assert!(check_export_path(std::path::Path::new("/tmp/x.txt")).is_err());
    assert!(check_export_path(std::path::Path::new("/tmp/x")).is_err());
    let d = tmpdir("path");
    assert!(check_export_path(&d.join("ok.citrate-journal")).is_ok());
    std::fs::remove_dir_all(&d).unwrap();
}

#[cfg(unix)]
#[test]
fn export_refuses_to_write_through_a_symlink() {
    let d = tmpdir("link");
    let target = d.join("target.txt");
    std::fs::write(&target, b"keep me").unwrap();
    let link = d.join("evil.citrate-journal");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    let err = export_to_path(&link, PASS, BUNDLE).unwrap_err();
    assert_eq!(err, JournalCryptoError::UnsafePath);
    assert_eq!(std::fs::read(&target).unwrap(), b"keep me");
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn export_then_import_through_files_round_trips_and_writes_only_ciphertext() {
    let d = tmpdir("rt");
    let path = d.join("journal-2026-10-01.citrate-journal");
    let n = export_to_path(&path, PASS, BUNDLE).unwrap();
    let on_disk = std::fs::read(&path).unwrap();
    assert_eq!(n as usize, on_disk.len());
    assert!(
        !on_disk.windows(8).any(|w| w == b"citrate-"),
        "plaintext bundle marker found on disk"
    );
    // the directory holds exactly the one sealed file: no plaintext temp file left behind
    let entries: Vec<_> = std::fs::read_dir(&d).unwrap().collect();
    assert_eq!(entries.len(), 1);
    let back = import_from_path(&path, PASS).unwrap();
    assert_eq!(back.as_bytes(), BUNDLE);
    assert_eq!(
        import_from_path(&path, "wrong passphrase!!").unwrap_err(),
        JournalCryptoError::WrongPassphraseOrDamaged
    );
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn import_refuses_an_oversized_file_before_reading_it() {
    let d = tmpdir("big");
    let path = d.join("big.citrate-journal");
    let f = std::fs::File::create(&path).unwrap();
    f.set_len(MAX_FILE_BYTES + 1).unwrap();
    assert_eq!(
        import_from_path(&path, PASS).unwrap_err(),
        JournalCryptoError::TooLarge
    );
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn error_messages_are_plain_and_name_no_internals() {
    for e in [
        JournalCryptoError::WrongPassphraseOrDamaged,
        JournalCryptoError::NotAJournalFile,
        JournalCryptoError::UnsupportedVersion,
        JournalCryptoError::UnsupportedParameters,
        JournalCryptoError::PassphraseTooShort,
        JournalCryptoError::UnsafePath,
        JournalCryptoError::TooLarge,
        JournalCryptoError::Io,
    ] {
        let m = e.to_string();
        assert!(!m.is_empty());
        assert!(!m.contains("aes") && !m.contains("argon"), "{m}");
    }
}

// --- review hardening (HUP-S10.4 adversarial review) ---------------------------

#[cfg(unix)]
#[test]
fn export_over_an_existing_loose_file_leaves_it_owner_only() {
    use std::os::unix::fs::PermissionsExt;
    let d = tmpdir("mode");
    let path = d.join("old.citrate-journal");
    std::fs::write(&path, b"older export").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    export_to_path(&path, PASS, BUNDLE).unwrap();
    let mode = std::fs::metadata(&path).unwrap().permissions().mode();
    assert_eq!(
        mode & 0o077,
        0,
        "a replaced export must be owner-only, got {mode:o}"
    );
    std::fs::remove_dir_all(&d).unwrap();
}

#[cfg(unix)]
#[test]
fn the_export_opener_itself_does_not_follow_a_symlink() {
    // The pre-check in export_to_path can race with a symlink swap; the open
    // call must refuse a symlink on its own.
    let d = tmpdir("nofollow");
    let target = d.join("target.txt");
    std::fs::write(&target, b"keep me").unwrap();
    let link = d.join("swap.citrate-journal");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    assert!(open_export_file(&link).is_err());
    assert_eq!(std::fs::read(&target).unwrap(), b"keep me");
    std::fs::remove_dir_all(&d).unwrap();
}

#[cfg(unix)]
#[test]
fn import_refuses_a_non_regular_file_without_blocking() {
    let d = tmpdir("fifo");
    let path = d.join("pipe.citrate-journal");
    let c = std::ffi::CString::new(path.to_str().unwrap()).unwrap();
    // SAFETY: c is a valid NUL-terminated path for the duration of the call.
    assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
    let (tx, rx) = std::sync::mpsc::channel();
    let p = path.clone();
    std::thread::spawn(move || {
        let _ = tx.send(import_from_path(&p, PASS));
    });
    let r = rx
        .recv_timeout(std::time::Duration::from_secs(10))
        .expect("import blocked on a FIFO");
    assert_eq!(r.unwrap_err(), JournalCryptoError::NotAJournalFile);
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn import_refuses_a_directory_as_not_a_journal_file() {
    let d = tmpdir("dir");
    let sub = d.join("folder.citrate-journal");
    std::fs::create_dir_all(&sub).unwrap();
    assert_eq!(
        import_from_path(&sub, PASS).unwrap_err(),
        JournalCryptoError::NotAJournalFile
    );
    std::fs::remove_dir_all(&d).unwrap();
}
