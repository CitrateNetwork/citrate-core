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
fn zip_is_refused_honestly() {
    let (_t, r) = run("zip", ArchiveFormat::Zip, b"PK\x03\x04");
    let err = r.unwrap_err();
    assert!(
        matches!(err, ComponentError::UnsupportedFormat(_)),
        "{err:?}"
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
