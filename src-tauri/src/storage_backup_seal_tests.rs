// STO-2 (v0.6.0) — encrypted backups. Included into `storage_backup::tests` after the K4 tests,
// so it shares their fakes (in-memory kubo, on-disk memory dir, recording daemon).
//
// Known-answer vectors were computed independently of this crate with argon2-cffi and
// pyca/cryptography (AESGCM), building the STREAM-BE32 nonce by hand
// (prefix ‖ counter BE32 ‖ last flag). The script is in the PR description.

use super::seal::{self, OpenReader, Protection, SealWriter};

const PASS: &str = "correct horse battery staple";

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

struct Sealed {
    report: ExportReport,
    kubo: Arc<FakeKubo>,
    mem: PathBuf,
    root: PathBuf,
    staging: PathBuf,
    dest: PathBuf,
    cids: Vec<String>,
}

/// Seed a root with canary content, then export both parts sealed under [`PASS`].
fn export_sealed_both(tag: &str) -> Sealed {
    let root = tmp_dir(tag);
    let dest = root.join("dest");
    std::fs::create_dir_all(&dest).unwrap();
    let staging = root.join("app-data").join("backup-staging");
    let kubo = FakeKubo::new();
    let cids = vec![
        kubo.put_pinned(b"CANARY-FILE-sto2-a first pinned file"),
        kubo.put_pinned(b"CANARY-FILE-sto2-a second pinned file"),
    ];
    let mem = seed_memory_dir(&root);
    std::fs::write(mem.join("docs-corpus.seeded"), b"CANARY-MEMORY-sto2 private note").unwrap();
    let mgr = files_manager(&root, kubo.clone());
    let daemon = FakeDaemon::new(true, &mem.join("store.bge.memdag"));
    let src = MemorySource {
        dir: mem.clone(),
        store_path: mem.join("store.bge.memdag"),
        daemon: &daemon,
        registry: open_registry(),
    };
    let report = export_sealed(
        &dest,
        &[Part::Files, Part::Memory],
        Some(&mgr),
        Some(&src),
        1_760_000_000,
        PASS,
        &staging,
        &mut no_progress(),
    )
    .expect("sealed export");
    Sealed {
        report,
        kubo,
        mem,
        root,
        staging,
        dest,
        cids,
    }
}

fn staging_is_empty(staging: &Path) -> bool {
    !staging.exists() || std::fs::read_dir(staging).unwrap().next().is_none()
}

// ---------------------------------------------------------------- export

#[test]
fn backup_has_no_plaintext() {
    let s = export_sealed_both("sto2-no-plaintext");
    assert!(s.report.encrypted);
    // The destination holds exactly one thing: the sealed file. No folder, no manifest.
    let entries: Vec<PathBuf> = std::fs::read_dir(&s.dest).unwrap().map(|e| e.unwrap().path()).collect();
    assert_eq!(entries, vec![PathBuf::from(&s.report.folder)], "only the sealed file");
    let file = PathBuf::from(&s.report.folder);
    assert!(file.is_file());
    assert_eq!(
        file.file_name().unwrap().to_string_lossy(),
        "citrate-backup-20251009-085320.citrate-backup"
    );
    let bytes = std::fs::read(&file).unwrap();
    // The versioned header.
    assert_eq!(&bytes[..8], seal::MAGIC);
    assert_eq!(bytes[8], 1, "format version 1");
    // No plaintext of any included file, and no names either.
    for needle in [
        b"CANARY-FILE-sto2".as_slice(),
        b"CANARY-MEMORY-sto2",
        b"MANIFEST-000001",
        b"[Version]",
        MANIFEST_NAME.as_bytes(),
        FORMAT.as_bytes(),
        b"memory/",
        b"files/",
        b"docs-corpus",
        b"store.bge.memdag",
    ] {
        assert!(!contains(&bytes, needle), "plaintext {:?} in the sealed file", String::from_utf8_lossy(needle));
    }
    for cid in &s.cids {
        assert!(!contains(&bytes, cid.as_bytes()), "CID {cid} in the sealed file");
    }
    // The 4096-byte run of 7s in the fake store must not survive either.
    assert!(!contains(&bytes, &[7u8; 64]), "store bytes in the sealed file");
    // Owner-only from creation.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&file).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "sealed file mode {mode:o}");
    }
    // The plain staging copy is gone.
    assert!(staging_is_empty(&s.staging), "staging removed after export");
}

#[test]
fn a_plain_folder_is_written_only_when_plaintext_is_chosen() {
    assert!(matches!(Protection::choose(None, true), Ok(Protection::Plaintext)));
    assert!(matches!(Protection::choose(Some(PASS), false), Ok(Protection::Passphrase(_))));
    let neither = Protection::choose(None, false).expect_err("no choice is refused");
    assert!(neither.to_string().contains("passphrase"), "{neither}");
    let both = Protection::choose(Some(PASS), true).expect_err("both is refused");
    assert!(both.to_string().contains("not both"), "{both}");
    let dbg = format!("{:?}", Protection::choose(Some(PASS), false));
    assert!(!dbg.contains(PASS) && dbg.contains("redacted"), "Debug leaks: {dbg}");
    let short = Protection::choose(Some("short"), false).expect_err("short passphrase");
    assert!(short.to_string().contains("12"), "{short}");
}

#[test]
fn the_command_defaults_to_refusing_an_unprotected_export() {
    // Source tripwire: the command routes through `Protection::choose` before it touches any store,
    // and an absent `plaintext` flag means "not chosen".
    let src = include_str!("storage_backup.rs");
    let start = src.find("pub fn storage_backup_export_sync(").expect("export body");
    let body = &src[start..start + src[start..].find("\n}\n").expect("ends")];
    let choose = body.find("Protection::choose(").expect("protection chosen");
    let files = body.find("files_manager(").expect("files manager");
    assert!(choose < files, "the protection check comes first");
    let cmd = &src[src.find("pub async fn storage_backup_export(").expect("cmd")..];
    assert!(cmd[..cmd.find("\n}\n").unwrap()].contains("plaintext.unwrap_or(false)"));
}

// ---------------------------------------------------------------- restore and verify

#[test]
fn wrong_passphrase_refuses() {
    let s = export_sealed_both("sto2-wrong-pass");
    move_memory_on(&s.mem);
    let daemon = FakeDaemon::new(true, &s.mem.join("store.bge.memdag"));
    let target = MemorySource {
        dir: s.mem.clone(),
        store_path: s.mem.join("store.bge.memdag"),
        daemon: &daemon,
        registry: open_registry(),
    };
    let fresh = FakeKubo::new();
    let mgr = files_manager(&s.root, fresh.clone());
    let err = restore_sealed(
        Path::new(&s.report.folder),
        "not the right passphrase",
        &s.staging,
        &[Part::Files, Part::Memory],
        Some(&mgr),
        Some(&target),
        42,
        &mut no_progress(),
    )
    .expect_err("a wrong passphrase refuses");
    assert!(err.to_string().contains("passphrase does not open"), "{err}");
    assert_memory_untouched(&s.mem, &daemon);
    assert!(fresh.blobs.lock().unwrap().is_empty(), "nothing was added to kubo");
    assert!(staging_is_empty(&s.staging), "no plaintext left in staging");
    // Verify reports the same refusal as a problem, never ok.
    let v = verify_sealed_report(Path::new(&s.report.folder), "not the right passphrase", &s.staging);
    assert!(!v.ok);
    assert!(v.problem.unwrap_or_default().contains("passphrase"));
}

#[test]
fn a_sealed_backup_verifies_and_restores_both_parts() {
    let s = export_sealed_both("sto2-roundtrip");
    let v = verify_sealed_report(Path::new(&s.report.folder), PASS, &s.staging);
    assert!(v.ok, "{v:?}");
    assert_eq!(v.folder, s.report.folder, "the report names the sealed file, not staging");
    assert_eq!(v.parts, vec![Part::Files, Part::Memory]);
    assert_eq!(v.file_count, 7);
    assert!(staging_is_empty(&s.staging));

    move_memory_on(&s.mem);
    let daemon = FakeDaemon::new(true, &s.mem.join("store.bge.memdag"));
    let target = MemorySource {
        dir: s.mem.clone(),
        store_path: s.mem.join("store.bge.memdag"),
        daemon: &daemon,
        registry: open_registry(),
    };
    let fresh = FakeKubo::new();
    let mgr = files_manager(&s.root.join("other"), fresh.clone());
    let r = restore_sealed(
        Path::new(&s.report.folder),
        PASS,
        &s.staging,
        &[Part::Files, Part::Memory],
        Some(&mgr),
        Some(&target),
        42,
        &mut no_progress(),
    )
    .expect("restore");
    assert_eq!(r.folder, s.report.folder);
    assert_eq!(r.files_restored, 2);
    assert_eq!(r.memory_files_restored, 5);
    assert_eq!(std::fs::read(s.mem.join("store.bge.memdag/000123.sst")).unwrap(), vec![7u8; 4096]);
    assert_eq!(std::fs::read(s.mem.join("docs-corpus.seeded")).unwrap(), b"CANARY-MEMORY-sto2 private note");
    for cid in &s.cids {
        assert_eq!(
            fresh.blobs.lock().unwrap().get(cid).cloned(),
            s.kubo.blobs.lock().unwrap().get(cid).cloned(),
            "{cid} restored byte for byte"
        );
    }
    assert!(staging_is_empty(&s.staging), "staging removed after restore");
}

#[test]
fn a_damaged_sealed_backup_is_refused() {
    let s = export_sealed_both("sto2-damaged");
    let file = PathBuf::from(&s.report.folder);
    let good = std::fs::read(&file).unwrap();
    let damaged = s.root.join("damaged.citrate-backup");
    let mut cases: Vec<(&str, Vec<u8>)> = Vec::new();
    let mut flip_body = good.clone();
    let last = flip_body.len() - 1;
    flip_body[last] ^= 1;
    cases.push(("a flipped tag byte", flip_body));
    let mut flip_mid = good.clone();
    flip_mid[seal::HEADER_LEN + 3] ^= 0x80;
    cases.push(("a flipped body byte", flip_mid));
    let mut flip_salt = good.clone();
    flip_salt[23] ^= 1;
    cases.push(("a changed salt", flip_salt));
    let mut flip_prefix = good.clone();
    flip_prefix[seal::HEADER_LEN - 6] ^= 1;
    cases.push(("a changed nonce prefix", flip_prefix));
    cases.push(("a truncated body", good[..good.len() - 1].to_vec()));
    cases.push(("only the header", good[..seal::HEADER_LEN].to_vec()));
    let mut appended = good.clone();
    appended.extend_from_slice(b"extra");
    cases.push(("bytes appended", appended));
    let mut v2 = good.clone();
    v2[8] = 2;
    cases.push(("an unknown version", v2));
    let mut kdf = good.clone();
    kdf[13] ^= 1; // m_cost low byte
    cases.push(("other KDF costs", kdf));
    for (what, bytes) in cases {
        let _ = std::fs::remove_file(&damaged);
        std::fs::write(&damaged, &bytes).unwrap();
        let v = verify_sealed_report(&damaged, PASS, &s.staging);
        assert!(!v.ok, "{what}: must not verify");
        assert!(v.problem.is_some(), "{what}: a problem is named");
        assert!(staging_is_empty(&s.staging), "{what}: staging cleaned");
    }
    // Not a sealed backup at all.
    std::fs::write(&damaged, b"just some text").unwrap();
    assert!(!seal::is_sealed_file(&damaged));
    assert!(seal::is_sealed_file(&file));
}

#[test]
fn a_sealed_record_with_an_unsafe_path_is_refused_and_nothing_escapes() {
    let root = tmp_dir("sto2-unsafe");
    let file = root.join("evil.citrate-backup");
    for evil in ["../escape", "memory/../../escape", "/etc/escape", "notallowed/x", "files/not/a/cid"] {
        let out = std::fs::File::create(&file).unwrap();
        let mut w = SealWriter::start(out, PASS.as_bytes(), [1u8; 16], [2u8; 7]).unwrap();
        let mut rec = Vec::new();
        // A real manifest record first, then the hostile one.
        rec.push(b'F');
        rec.extend_from_slice(&(MANIFEST_NAME.len() as u16).to_be_bytes());
        rec.extend_from_slice(MANIFEST_NAME.as_bytes());
        rec.extend_from_slice(&2u64.to_be_bytes());
        rec.extend_from_slice(b"{}");
        rec.push(b'F');
        rec.extend_from_slice(&(evil.len() as u16).to_be_bytes());
        rec.extend_from_slice(evil.as_bytes());
        rec.extend_from_slice(&4u64.to_be_bytes());
        rec.extend_from_slice(b"pwnd");
        rec.push(b'E');
        w.feed(&rec).unwrap();
        w.finish().unwrap();
        let into = root.join("into");
        let _ = std::fs::remove_dir_all(&into);
        let err = seal::open_into(&file, PASS.as_bytes(), &into).expect_err(evil);
        assert!(err.to_string().contains("unsafe path"), "{evil}: {err}");
        assert!(!root.join("escape").exists(), "{evil}: nothing written outside");
        let _ = std::fs::remove_dir_all(&into);
    }
}

// ---------------------------------------------------------------- the chunked AEAD

/// Seal `pt` with fixed salt and nonce prefix (test-only; production draws both at random).
fn seal_fixed(pt: &[u8]) -> Vec<u8> {
    let mut w = SealWriter::start(Vec::new(), PASS.as_bytes(), [0x11; 16], [0x22; 7]).unwrap();
    w.feed(pt).unwrap();
    w.finish().unwrap()
}

fn open_all(file: &[u8], len: usize) -> std::result::Result<Vec<u8>, BackupError> {
    let mut r = OpenReader::start(file, PASS.as_bytes())?;
    let mut out = vec![0u8; len];
    r.take_exact(&mut out)?;
    if !r.at_end() {
        return Err(BackupError::Manifest("trailing data".into()));
    }
    Ok(out)
}

#[test]
fn sealed_format_matches_the_independent_known_answer_vector() {
    // One chunk: header ‖ AES-256-GCM(key = Argon2id(PASS, salt), nonce = prefix ‖ 0u32 ‖ 0x01,
    // aad = header, pt).
    let got = seal_fixed(b"citrate STO-2 test vector\n");
    assert_eq!(
        hex::encode(&got),
        "4349545242414b000101000100000000000300000001101111111111111111111111111111111101072222222222222200100000caed0c4f6f463ac2dac09d4841479a6c8c3d3f622092959cae3df6b316cd3c64860baba7a3e8dbde611b"
    );
    assert_eq!(open_all(&got, 26).unwrap(), b"citrate STO-2 test vector\n");
}

#[test]
fn a_two_chunk_seal_matches_the_vector_and_refuses_reorder_and_truncation() {
    let pt: Vec<u8> = (0..seal::CHUNK_LEN + 10).map(|i| (i % 251) as u8).collect();
    let got = seal_fixed(&pt);
    assert_eq!(got.len(), 1_048_670);
    assert_eq!(
        hex::encode(Sha256::digest(&got)),
        "7883e536e5a6ffc872600d178a3ecaf0ff92134c92922057be126cc4e66fe273"
    );
    assert_eq!(open_all(&got, pt.len()).unwrap(), pt);
    // Dropping the last chunk leaves a non-last chunk at the end: refused, not a short success.
    let first_end = seal::HEADER_LEN + seal::CHUNK_LEN + 16;
    let truncated = got[..first_end].to_vec();
    assert!(open_all(&truncated, seal::CHUNK_LEN).is_err(), "truncation at a chunk boundary");
    // Swapping the chunks fails authentication (the counter is in the nonce).
    let mut swapped = got[..seal::HEADER_LEN].to_vec();
    swapped.extend_from_slice(&got[first_end..]);
    swapped.extend_from_slice(&got[seal::HEADER_LEN..first_end]);
    assert!(open_all(&swapped, pt.len()).is_err(), "reordered chunks");
    // An exact multiple of the chunk size ends with a full last chunk and still opens.
    let exact: Vec<u8> = vec![3u8; seal::CHUNK_LEN];
    let sealed = seal_fixed(&exact);
    assert_eq!(sealed.len(), seal::HEADER_LEN + seal::CHUNK_LEN + 16);
    assert_eq!(open_all(&sealed, exact.len()).unwrap(), exact);
}

#[test]
fn every_export_draws_a_fresh_salt_and_nonce_prefix() {
    let a = export_sealed_both("sto2-fresh-a");
    let b = export_sealed_both("sto2-fresh-b");
    let fa = std::fs::read(&a.report.folder).unwrap();
    let fb = std::fs::read(&b.report.folder).unwrap();
    assert_ne!(fa[23..39], fb[23..39], "salt");
    assert_ne!(fa[41..48], fb[41..48], "nonce prefix");
}

#[test]
fn the_sealed_module_uses_only_the_vault_kdf_and_the_existing_aead() {
    // No new primitive: the KDF is the custody vault's, the AEAD is aes-gcm's STREAM.
    let src = include_str!("storage_backup_seal.rs");
    assert!(src.contains("custody::derive_passphrase_key("));
    assert!(src.contains("aead::stream::{DecryptorBE32, EncryptorBE32}"));
    assert!(!src.contains("Argon2::new("), "no second KDF");
}

// ---------------------------------------------------------------- start-up sweep (P3)

#[test]
fn start_up_sweep_removes_leftover_staging_and_nothing_else() {
    let app = tmp_dir("sto2-sweep");
    let root = app.join(STAGING_DIR_NAME);
    std::fs::create_dir_all(root.join("export-0011223344556677/citrate-backup-x/memory")).unwrap();
    std::fs::write(root.join("export-0011223344556677/citrate-backup-x/memory/a"), b"PLAINTEXT").unwrap();
    std::fs::create_dir_all(root.join("open-8899aabbccddeeff")).unwrap();
    // Not ours: a differently named dir, a plain file, and a symlink to a dir outside.
    std::fs::create_dir_all(root.join("keep-me")).unwrap();
    std::fs::write(root.join("export-not-a-dir"), b"x").unwrap();
    let outside = app.join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("precious"), b"keep").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, root.join("export-link")).unwrap();

    let removed = sweep_staging(&app).expect("sweep");
    assert_eq!(removed, 2, "the two staging dirs");
    assert!(!root.join("export-0011223344556677").exists());
    assert!(!root.join("open-8899aabbccddeeff").exists());
    assert!(root.join("keep-me").exists());
    assert!(root.join("export-not-a-dir").exists());
    assert_eq!(std::fs::read(outside.join("precious")).unwrap(), b"keep", "a symlink is never followed");
    // No staging root at all is fine.
    assert_eq!(sweep_staging(&app.join("nowhere")).unwrap(), 0);
}

#[test]
fn the_start_up_sweep_runs_in_app_setup_and_uses_the_command_staging_root() {
    let lib = include_str!("lib.rs");
    let setup = &lib[lib.find(".setup(|app| {").expect("setup")..];
    assert!(setup.contains("storage_backup::sweep_staging("), "sweep wired into setup");
    let module = include_str!("storage_backup.rs");
    let root_fn = &module[module.find("fn staging_root<").expect("staging_root")..];
    assert!(root_fn[..root_fn.find("\n}\n").unwrap()].contains("STAGING_DIR_NAME"));
}
