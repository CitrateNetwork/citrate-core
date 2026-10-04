//! HUP-S5.5: archives are unpacked only inside the staging tree. Traversal, absolute paths,
//! escaping or dangling symlinks, hard links and duplicates are refused.
mod common;

use citrate_components::error::ComponentError;
use citrate_components::extract::{extract, safe_relative_path};
use citrate_components::manifest::ArchiveFormat;
use common::*;

fn run(
    tag: &str,
    format: ArchiveFormat,
    bytes: &[u8],
) -> (
    TempDir,
    Result<citrate_components::extract::ExtractReport, ComponentError>,
) {
    let t = TempDir::new(tag);
    let archive = t.path().join("download.bin");
    std::fs::write(&archive, bytes).unwrap();
    let dest = t.path().join("tree");
    let r = extract(format, &archive, &dest, "solc");
    (t, r)
}

#[test]
fn safe_relative_path_rules() {
    for ok in ["a", "a/b", "./a/b", "a/./b", "bin/forge"] {
        assert!(safe_relative_path(ok).is_ok(), "{ok}");
    }
    for bad in [
        "",
        ".",
        "/etc/passwd",
        "../x",
        "a/../../x",
        "a/..",
        "a\\b",
        "C:x",
        "a\0b",
        "~/x",
    ] {
        assert!(safe_relative_path(bad).is_err(), "{bad:?}");
    }
    let deep = vec!["d"; 40].join("/");
    assert!(safe_relative_path(&deep).is_err(), "too deep");
}

#[test]
fn a_raw_artifact_becomes_one_executable_file() {
    let (t, r) = run("raw", ArchiveFormat::Raw, b"#!/bin/sh\necho solc\n");
    let rep = r.unwrap();
    assert_eq!(rep.files, 1);
    let p = t.path().join("tree/solc");
    assert_eq!(std::fs::read(&p).unwrap(), b"#!/bin/sh\necho solc\n");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        assert_eq!(
            std::fs::metadata(&p).unwrap().permissions().mode() & 0o777,
            0o755
        );
    }
}

#[test]
fn a_tar_gz_unpacks_files_dirs_and_inside_symlinks() {
    let bytes = tar_gz(&[
        Entry::Dir("node/"),
        Entry::Dir("node/bin/"),
        Entry::File("node/lib/cli.js", b"console.log(1)", 0o644),
        Entry::File("node/bin/node", b"ELF", 0o4755), // setuid is dropped
        Entry::Symlink("node/bin/npm", "../lib/cli.js"),
    ]);
    let (t, r) = run("tgz", ArchiveFormat::TarGz, &bytes);
    let rep = r.unwrap();
    assert_eq!(rep.files, 2);
    assert_eq!(rep.symlinks, 1);
    let tree = t.path().join("tree");
    assert_eq!(
        std::fs::read(tree.join("node/bin/npm")).unwrap(),
        b"console.log(1)"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = std::fs::metadata(tree.join("node/bin/node"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(
            mode & 0o7777,
            0o755,
            "setuid and group/other write are dropped"
        );
        let mode = std::fs::metadata(tree.join("node/lib/cli.js"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o7777, 0o644);
    }
}

#[test]
fn a_tar_xz_unpacks() {
    let bytes = tar_xz(&[
        Entry::Dir("aderyn-aarch64-apple-darwin/"),
        Entry::File("aderyn-aarch64-apple-darwin/aderyn", b"bin", 0o755),
        Entry::File("aderyn-aarch64-apple-darwin/LICENSE", b"GPL", 0o644),
    ]);
    let (t, r) = run("txz", ArchiveFormat::TarXz, &bytes);
    assert_eq!(r.unwrap().files, 2);
    assert!(t
        .path()
        .join("tree/aderyn-aarch64-apple-darwin/aderyn")
        .is_file());
}

#[test]
fn traversal_and_absolute_paths_are_refused() {
    for p in ["../evil", "a/../../evil", "/tmp/evil"] {
        let bytes = tar_gz(&[Entry::RawPath(p, b"x")]);
        let (t, r) = run("trav", ArchiveFormat::TarGz, &bytes);
        let err = r.unwrap_err();
        assert!(
            matches!(err, ComponentError::UnsafeArchiveEntry(_)),
            "{p}: {err:?}"
        );
        assert!(!t.path().join("evil").exists());
    }
}

#[test]
fn escaping_symlinks_are_refused() {
    for (link, target) in [
        ("bin/x", "../../outside"),
        ("x", "/etc/passwd"),
        ("bin/y", "../.."),
    ] {
        let bytes = tar_gz(&[Entry::Dir("bin/"), Entry::Symlink(link, target)]);
        let (_t, r) = run("symesc", ArchiveFormat::TarGz, &bytes);
        let err = r.unwrap_err();
        assert!(
            matches!(err, ComponentError::UnsafeArchiveEntry(_)),
            "{link} -> {target}: {err:?}"
        );
    }
}

#[test]
fn a_symlink_chain_that_escapes_after_resolution_is_refused() {
    // `s -> .` is inside; `t -> s/../x` looks inside lexically but resolves to the parent of
    // the tree. The canonical check after unpacking catches it.
    let bytes = tar_gz(&[Entry::Symlink("s", "."), Entry::Symlink("t", "s/../x")]);
    let (_t, r) = run("chain", ArchiveFormat::TarGz, &bytes);
    let err = r.unwrap_err();
    assert!(
        matches!(err, ComponentError::UnsafeArchiveEntry(_)),
        "{err:?}"
    );
}

#[test]
fn a_file_written_through_an_archive_symlink_is_refused() {
    // The symlink is created last, so a later file entry under it lands in a real directory and
    // the symlink then collides with it.
    let bytes = tar_gz(&[
        Entry::Symlink("lib", "/tmp"),
        Entry::File("lib/evil", b"x", 0o644),
    ]);
    let (_t, r) = run("through", ArchiveFormat::TarGz, &bytes);
    assert!(r.is_err());
}

#[test]
fn dangling_symlinks_hard_links_and_duplicates_are_refused() {
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("dangling", tar_gz(&[Entry::Symlink("x", "missing")])),
        (
            "hardlink",
            tar_gz(&[Entry::File("a", b"1", 0o644), Entry::Hardlink("b", "a")]),
        ),
        (
            "duplicate",
            tar_gz(&[Entry::File("a", b"1", 0o644), Entry::File("a", b"2", 0o644)]),
        ),
    ];
    for (label, bytes) in cases {
        let (_t, r) = run(label, ArchiveFormat::TarGz, &bytes);
        let err = r.unwrap_err();
        assert!(
            matches!(err, ComponentError::UnsafeArchiveEntry(_)),
            "{label}: {err:?}"
        );
    }
}

#[test]
fn a_truncated_zip_is_an_error_not_a_partial_success() {
    // Was `zip_is_refused_honestly` while zip was unsupported. A bare local-header signature
    // is not an archive: the error is about the archive, never "unsupported".
    let (t, r) = run("zip", ArchiveFormat::Zip, b"PK\x03\x04");
    let err = r.unwrap_err();
    assert!(
        matches!(err, ComponentError::UnsafeArchiveEntry(_)),
        "{err:?}"
    );
    assert!(
        !t.path().join("tree").exists(),
        "a failed unpack leaves no tree"
    );
}

#[test]
fn a_corrupt_archive_is_an_error_not_a_partial_success() {
    let mut bytes = tar_gz(&[Entry::File("a", &[7u8; 4096], 0o644)]);
    let n = bytes.len();
    bytes.truncate(n / 2);
    let (_t, r) = run("corrupt", ArchiveFormat::TarGz, &bytes);
    assert!(r.is_err());
}

#[test]
fn the_destination_must_not_exist_yet() {
    let t = TempDir::new("exists");
    let archive = t.path().join("download.bin");
    std::fs::write(&archive, b"x").unwrap();
    let dest = t.path().join("tree");
    std::fs::create_dir_all(&dest).unwrap();
    assert!(extract(ArchiveFormat::Raw, &archive, &dest, "solc").is_err());
}

#[test]
fn a_symlink_chain_to_an_existing_file_outside_the_tree_is_refused() {
    // `s -> .` then `t -> s/../download.bin`: lexically inside, but it resolves to the
    // downloaded archive next to the tree, which exists. Only the containment check after
    // resolution catches this one.
    let bytes = tar_gz(&[
        Entry::Symlink("s", "."),
        Entry::Symlink("t", "s/../download.bin"),
    ]);
    let (_t, r) = run("chainout", ArchiveFormat::TarGz, &bytes);
    let err = r.unwrap_err();
    assert!(
        matches!(err, ComponentError::UnsafeArchiveEntry(_)),
        "{err:?}"
    );
}

#[test]
fn a_symlink_that_climbs_out_and_back_in_is_refused() {
    // `a -> ../tree/x` resolves inside today, but the tree is renamed into place after
    // unpacking, and then it would point somewhere else. Targets may not climb out at all.
    let bytes = tar_gz(&[
        Entry::File("x", b"1", 0o644),
        Entry::Symlink("a", "../tree/x"),
    ]);
    let (_t, r) = run("outin", ArchiveFormat::TarGz, &bytes);
    let err = r.unwrap_err();
    assert!(
        matches!(err, ComponentError::UnsafeArchiveEntry(_)),
        "{err:?}"
    );
}

#[test]
fn no_symlink_is_ever_created_through_another_archive_symlink() {
    // `q -> .` and `p -> q/..` are each lexically inside the tree, but `p` resolves to the
    // directory holding the tree. A later link under `p/` would then be created outside the
    // tree before any check after unpacking could run. A link whose parent path passes
    // through an archive symlink is refused before anything is created.
    let bytes = tar_gz(&[
        Entry::Symlink("q", "."),
        Entry::Symlink("p", "q/.."),
        Entry::Symlink("p/planted", "q"),
    ]);
    let (t, r) = run("linkparent", ArchiveFormat::TarGz, &bytes);
    let err = r.unwrap_err();
    assert!(
        matches!(err, ComponentError::UnsafeArchiveEntry(_)),
        "{err:?}"
    );
    assert!(
        std::fs::symlink_metadata(t.path().join("planted")).is_err(),
        "a link was created outside the tree"
    );
}

// ---- zip (HUP-S5.1 Chrome for Testing, HUP-S6.1 Windows foundry and node) ----

fn assert_unsafe(
    label: &str,
    r: Result<citrate_components::extract::ExtractReport, ComponentError>,
) {
    let err = r.unwrap_err();
    assert!(
        matches!(err, ComponentError::UnsafeArchiveEntry(_)),
        "{label}: {err:?}"
    );
}

#[test]
fn a_zip_unpacks_files_dirs_exec_bits_and_inside_symlinks() {
    let bytes = zip_bytes(&[
        ZEntry::Dir("node/"),
        ZEntry::File("node/lib/cli.js", b"console.log(1)", 0o644),
        ZEntry::Stored("node/bin/node", b"ELF", 0o755),
        ZEntry::Symlink("node/bin/npm", "../lib/cli.js"),
    ]);
    let (t, r) = run("zipok", ArchiveFormat::Zip, &bytes);
    let rep = r.unwrap();
    assert_eq!(rep.files, 2);
    assert_eq!(rep.symlinks, 1);
    assert_eq!(rep.dirs, 1);
    assert_eq!(rep.bytes, 17);
    let tree = t.path().join("tree");
    assert_eq!(
        std::fs::read(tree.join("node/bin/npm")).unwrap(),
        b"console.log(1)"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = |p: &str| {
            std::fs::metadata(tree.join(p))
                .unwrap()
                .permissions()
                .mode()
                & 0o7777
        };
        assert_eq!(mode("node/bin/node"), 0o755);
        assert_eq!(mode("node/lib/cli.js"), 0o644);
        assert!(std::fs::symlink_metadata(tree.join("node/bin/npm"))
            .unwrap()
            .file_type()
            .is_symlink());
    }
}

#[test]
fn zip_set_id_and_group_write_bits_are_dropped() {
    let bytes = zip_bytes(&[ZEntry::File("bin/forge", b"ELF", 0o777)]);
    let bytes = zip_set_mode(bytes, "bin/forge", 0o106_777); // regular file, setuid, 0777
    let (t, r) = run("zipsuid", ArchiveFormat::Zip, &bytes);
    r.unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = std::fs::metadata(t.path().join("tree/bin/forge"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o7777, 0o755);
    }
    #[cfg(not(unix))]
    let _ = t;
}

/// The Chrome for Testing 154 macOS archive's five links, with the exact targets the real
/// archive carries (`Versions/Current` first points at the version directory, and the four
/// framework-root links go through it).
const CFT_FW: &str =
    "chrome-mac-arm64/Google Chrome for Testing.app/Contents/Frameworks/Google Chrome for Testing Framework.framework";

fn cft_shaped_zip(version_tag: &[u8]) -> Vec<u8> {
    let v = format!("{CFT_FW}/Versions/154.0.8037.92");
    let exe =
        "chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing";
    let fw_bin = format!("{v}/Google Chrome for Testing Framework");
    let helper = format!("{v}/Helpers/chrome_crashpad_handler");
    let lib = format!("{v}/Libraries/libEGL.dylib");
    let res = format!("{v}/Resources/Info.plist");
    let l_res = format!("{CFT_FW}/Resources");
    let l_cur = format!("{CFT_FW}/Versions/Current");
    let l_lib = format!("{CFT_FW}/Libraries");
    let l_bin = format!("{CFT_FW}/Google Chrome for Testing Framework");
    let l_hlp = format!("{CFT_FW}/Helpers");
    // Order as in the real archive: Resources comes before Versions/Current, so a link can be
    // written before the link it resolves through.
    zip_bytes(&[
        ZEntry::File(
            "chrome-mac-arm64/ABOUT",
            b"Google Chrome for Testing",
            0o644,
        ),
        ZEntry::Dir("chrome-mac-arm64/Google Chrome for Testing.app/"),
        ZEntry::File(exe, version_tag, 0o755),
        ZEntry::File(&fw_bin, b"MH_DYLIB", 0o755),
        ZEntry::File(&helper, b"MH_EXECUTE", 0o755),
        ZEntry::File(&lib, b"MH_DYLIB", 0o644),
        ZEntry::File(&res, b"<plist/>", 0o644),
        ZEntry::Symlink(&l_res, "Versions/Current/Resources"),
        ZEntry::Symlink(&l_cur, "154.0.8037.92"),
        ZEntry::Symlink(&l_lib, "Versions/Current/Libraries"),
        ZEntry::Symlink(
            &l_bin,
            "Versions/Current/Google Chrome for Testing Framework",
        ),
        ZEntry::Symlink(&l_hlp, "Versions/Current/Helpers"),
    ])
}

#[test]
fn the_five_chrome_for_testing_app_symlinks_unpack_and_resolve_inside_the_tree() {
    let (t, r) = run("cft", ArchiveFormat::Zip, &cft_shaped_zip(b"chrome 154"));
    let rep = r.unwrap();
    assert_eq!(rep.symlinks, 5);
    assert_eq!(rep.files, 6);
    let tree = t.path().join("tree");
    let root = std::fs::canonicalize(&tree).unwrap();
    for (link, want) in [
        ("Resources", "Versions/Current/Resources"),
        ("Versions/Current", "154.0.8037.92"),
        ("Libraries", "Versions/Current/Libraries"),
        (
            "Google Chrome for Testing Framework",
            "Versions/Current/Google Chrome for Testing Framework",
        ),
        ("Helpers", "Versions/Current/Helpers"),
    ] {
        let at = tree.join(CFT_FW).join(link);
        #[cfg(unix)]
        assert_eq!(
            std::fs::read_link(&at).unwrap(),
            std::path::PathBuf::from(want),
            "{link}"
        );
        let resolved = std::fs::canonicalize(&at).unwrap();
        assert!(
            resolved.starts_with(&root),
            "{link} -> {}",
            resolved.display()
        );
    }
    assert_eq!(
        std::fs::read(tree.join(CFT_FW).join("Helpers/chrome_crashpad_handler")).unwrap(),
        b"MH_EXECUTE"
    );
    let exe = tree.join(
        "chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing",
    );
    assert_eq!(std::fs::read(exe).unwrap(), b"chrome 154");
}

#[test]
fn zip_traversal_absolute_drive_and_backslash_paths_are_refused() {
    // The writer refuses none of these names, but each is smuggled in by renaming a harmless
    // entry of the same length, so the test does not depend on the writer's own checks.
    for (i, bad) in [
        "../evil",
        "a/../../ev",
        "/tmp/evil",
        "a\\..\\evil",
        "C:/evil.ex",
        "~/evil.txt",
    ]
    .into_iter()
    .enumerate()
    {
        let placeholder: String = "p".repeat(bad.len());
        let bytes = zip_bytes(&[ZEntry::File(&placeholder, b"x", 0o644)]);
        let bytes = zip_rename(bytes, &placeholder, bad);
        let (t, r) = run(&format!("ziptrav{i}"), ArchiveFormat::Zip, &bytes);
        assert_unsafe(bad, r);
        assert!(!t.path().join("evil").exists());
        assert!(!t.path().join("tree").exists());
    }
}

#[test]
fn zip_symlinks_that_leave_the_tree_are_refused() {
    let cases: Vec<(&str, Vec<u8>)> = vec![
        (
            "absolute",
            zip_bytes(&[ZEntry::Symlink("x", "/etc/passwd")]),
        ),
        (
            "climbs out",
            zip_bytes(&[
                ZEntry::Dir("bin/"),
                ZEntry::Symlink("bin/x", "../../outside"),
            ]),
        ),
        ("parent of root", zip_bytes(&[ZEntry::Symlink("y", "..")])),
        (
            "out and back in",
            zip_bytes(&[
                ZEntry::File("x", b"1", 0o644),
                ZEntry::Symlink("a", "../tree/x"),
            ]),
        ),
        (
            "chain to an existing file outside",
            zip_bytes(&[
                ZEntry::Symlink("s", "."),
                ZEntry::Symlink("t", "s/../download.bin"),
            ]),
        ),
        (
            "backslash target",
            zip_bytes(&[ZEntry::Symlink("w", "..\\..\\x")]),
        ),
        ("dangling", zip_bytes(&[ZEntry::Symlink("d", "missing")])),
    ];
    for (label, bytes) in cases {
        let (_t, r) = run("zipsym", ArchiveFormat::Zip, &bytes);
        assert_unsafe(label, r);
    }
}

#[test]
fn no_zip_symlink_is_created_through_another_archive_symlink() {
    let bytes = zip_bytes(&[
        ZEntry::Symlink("q", "."),
        ZEntry::Symlink("p", "q/.."),
        ZEntry::Symlink("p/planted", "q"),
    ]);
    let (t, r) = run("ziplinkparent", ArchiveFormat::Zip, &bytes);
    assert_unsafe("link under link", r);
    assert!(
        std::fs::symlink_metadata(t.path().join("planted")).is_err(),
        "a link was created outside the tree"
    );
}

#[test]
fn a_zip_file_written_through_an_archive_symlink_is_refused() {
    let bytes = zip_bytes(&[
        ZEntry::Symlink("lib", "/tmp"),
        ZEntry::File("lib/evil", b"x", 0o644),
    ]);
    let (_t, r) = run("zipthrough", ArchiveFormat::Zip, &bytes);
    assert!(r.is_err());
}

#[test]
fn zip_devices_fifos_and_sockets_are_refused() {
    for (label, mode) in [
        ("fifo", 0o010_644u32),
        ("char device", 0o020_644),
        ("block device", 0o060_644),
        ("socket", 0o140_644),
    ] {
        let bytes = zip_bytes(&[ZEntry::File("dev", b"", 0o644)]);
        let bytes = zip_set_mode(bytes, "dev", mode);
        let (_t, r) = run("zipdev", ArchiveFormat::Zip, &bytes);
        assert_unsafe(label, r);
    }
}

#[test]
fn a_zip_directory_entry_with_a_file_type_is_refused() {
    let bytes = zip_bytes(&[ZEntry::Dir("d/")]);
    let bytes = zip_set_mode(bytes, "d/", 0o120_777); // a "directory" that claims to be a link
    let (_t, r) = run("zipdirlink", ArchiveFormat::Zip, &bytes);
    assert_unsafe("dir with link mode", r);
}

#[test]
fn zip_duplicate_entries_are_refused() {
    let bytes = zip_bytes(&[
        ZEntry::File("dup_a", b"1", 0o644),
        ZEntry::File("dup_b", b"2", 0o644),
    ]);
    let bytes = zip_rename(bytes, "dup_b", "dup_a");
    let (_t, r) = run("zipdup", ArchiveFormat::Zip, &bytes);
    assert!(r.is_err(), "a duplicate name must not unpack");
    let bytes = zip_bytes(&[
        ZEntry::File("dup_a", b"1", 0o644),
        ZEntry::Symlink("dup_b", "dup_a"),
    ]);
    let bytes = zip_rename(bytes, "dup_b", "dup_a");
    let (_t, r) = run("zipdup2", ArchiveFormat::Zip, &bytes);
    assert!(r.is_err(), "a link may not replace a file of the same name");
}

#[test]
fn an_encrypted_zip_entry_is_refused() {
    let bytes = zip_bytes(&[ZEntry::Stored("secret", b"x", 0o644)]);
    let bytes = zip_mark_encrypted(bytes, "secret");
    let (t, r) = run("zipenc", ArchiveFormat::Zip, &bytes);
    assert_unsafe("encrypted", r);
    assert!(!t.path().join("tree").exists());
}

#[test]
fn a_zip_entry_whose_data_does_not_match_its_crc_is_refused() {
    let bytes = zip_bytes(&[ZEntry::Stored("f", b"good data", 0o644)]);
    let at = bytes.windows(9).position(|w| w == b"good data").unwrap();
    let mut bytes = bytes;
    bytes[at] = b'G';
    let (t, r) = run("zipcrc", ArchiveFormat::Zip, &bytes);
    assert_unsafe("crc", r);
    assert!(!t.path().join("tree").exists());
}

/// The real archive the bundle pins (191 MB), unpacked by the same code the app uses. Run with
/// `CITRATE_CFT_ZIP=<path to chrome-mac-arm64.zip> cargo test -p citrate-components --test
/// extract -- --ignored`.
#[test]
#[ignore = "needs the pinned Chrome for Testing archive; set CITRATE_CFT_ZIP"]
fn the_pinned_chrome_for_testing_zip_unpacks_with_its_five_links() {
    let Some(zip) = std::env::var_os("CITRATE_CFT_ZIP") else {
        panic!("set CITRATE_CFT_ZIP to the downloaded chrome-mac-arm64.zip");
    };
    let bytes = std::fs::read(&zip).unwrap();
    assert_eq!(
        sha256_hex(&bytes),
        "b62e904b6571c5ff5108ed7812cf93ac6d1c4027f10ae47ac34d8e229ed88001",
        "not the archive components/toolchain-bundle.json pins"
    );
    let t = TempDir::new("cftreal");
    let dest = t.path().join("tree");
    let rep = extract(ArchiveFormat::Zip, std::path::Path::new(&zip), &dest, "x").unwrap();
    assert_eq!(rep.symlinks, 5, "{rep:?}");
    let exe = dest.join(
        "chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing",
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = std::fs::metadata(&exe).unwrap().permissions().mode();
        assert_eq!(mode & 0o111, 0o111, "the browser is executable");
    }
    let root = std::fs::canonicalize(&dest).unwrap();
    let fw = dest.join(CFT_FW);
    for link in [
        "Resources",
        "Versions/Current",
        "Libraries",
        "Google Chrome for Testing Framework",
        "Helpers",
    ] {
        assert!(std::fs::canonicalize(fw.join(link))
            .unwrap()
            .starts_with(&root));
    }
    eprintln!("unpacked: {rep:?}");
}
