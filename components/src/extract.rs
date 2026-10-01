//! Unpacking a verified artifact into a fresh directory, with path checks.
//!
//! Rules: every entry path is a relative path of normal components (no `..`, no absolute or
//! drive paths, no backslashes); only regular files, directories and symlinks are accepted
//! (hard links, devices and FIFOs are refused); a path may appear once; set-id bits and
//! group/other write bits are dropped. Symlinks are created after every file and directory, so
//! no file is ever written through an archive symlink; each symlink target must stay inside
//! the tree both lexically and after resolution, and must exist; no link is placed under
//! another archive link.
use std::fs;
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};

use crate::error::{io, ComponentError};
use crate::manifest::ArchiveFormat;

/// The most entries one archive may have (a Chromium build has tens of thousands of files).
pub const MAX_ENTRIES: u64 = 200_000;
/// The most bytes one archive may unpack to.
pub const MAX_UNPACKED_BYTES: u64 = 8 << 30;
const MAX_DEPTH: usize = 32;
const MAX_PATH_CHARS: usize = 1024;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExtractReport {
    pub files: u64,
    pub dirs: u64,
    pub symlinks: u64,
    pub bytes: u64,
}

fn unsafe_entry(m: impl Into<String>) -> ComponentError {
    ComponentError::UnsafeArchiveEntry(m.into())
}

/// A relative path made only of normal components. `.` components and a trailing `/` are
/// ignored; anything else unusual is refused.
pub fn safe_relative_path(p: &str) -> Result<PathBuf, ComponentError> {
    if p.is_empty()
        || p.len() > MAX_PATH_CHARS
        || p.contains(['\0', '\\', ':'])
        || p.starts_with('/')
        || p.starts_with('~')
    {
        return Err(unsafe_entry(format!("path {p:?}")));
    }
    let mut out = PathBuf::new();
    let mut depth = 0usize;
    for part in p.split('/') {
        match part {
            "" | "." => continue,
            ".." => return Err(unsafe_entry(format!("path {p:?} has a parent component"))),
            normal => {
                depth += 1;
                out.push(normal);
            }
        }
    }
    if depth == 0 || depth > MAX_DEPTH {
        return Err(unsafe_entry(format!("path {p:?}")));
    }
    Ok(out)
}

/// Unpacks `archive` (already verified) into `dest`, which must not exist yet. A raw artifact
/// becomes the single executable file `dest/raw_name`. On error `dest` is removed.
pub fn extract(
    format: ArchiveFormat,
    archive: &Path,
    dest: &Path,
    raw_name: &str,
) -> Result<ExtractReport, ComponentError> {
    fs::create_dir(dest).map_err(io)?;
    let r = extract_into(format, archive, dest, raw_name);
    if r.is_err() {
        let _ = fs::remove_dir_all(dest);
    }
    r
}

fn extract_into(
    format: ArchiveFormat,
    archive: &Path,
    dest: &Path,
    raw_name: &str,
) -> Result<ExtractReport, ComponentError> {
    match format {
        ArchiveFormat::Raw => {
            let rel = safe_relative_path(raw_name)?;
            let mut src = fs::File::open(archive).map_err(io)?;
            let mut out = create_new(&dest.join(rel))?;
            let bytes = std::io::copy(&mut src, &mut out).map_err(io)?;
            out.flush().map_err(io)?;
            set_mode(&out, true)?;
            Ok(ExtractReport {
                files: 1,
                bytes,
                ..ExtractReport::default()
            })
        }
        ArchiveFormat::TarGz => {
            let f = fs::File::open(archive).map_err(io)?;
            unpack_tar(flate2::read::GzDecoder::new(BufReader::new(f)), dest)
        }
        ArchiveFormat::TarXz => {
            // lzma-rs decodes into a writer, so the tar goes to a capped temporary file next to
            // the archive (inside the staging directory) and is read back from there.
            let tmp = archive.with_extension("unxz.tar");
            let r = (|| {
                let f = fs::File::open(archive).map_err(io)?;
                let mut out = CappedWriter {
                    inner: create_new(&tmp)?,
                    left: MAX_UNPACKED_BYTES,
                };
                lzma_rs::xz_decompress(&mut BufReader::new(f), &mut out)
                    .map_err(|e| unsafe_entry(format!("xz: {e}")))?;
                out.inner.flush().map_err(io)?;
                unpack_tar(fs::File::open(&tmp).map_err(io)?, dest)
            })();
            let _ = fs::remove_file(&tmp);
            r
        }
        ArchiveFormat::Zip => Err(ComponentError::UnsupportedFormat(
            "zip archives are not unpacked in this version (Windows toolchain spike)".into(),
        )),
    }
}

struct CappedWriter<W> {
    inner: W,
    left: u64,
}

impl<W: Write> Write for CappedWriter<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if buf.len() as u64 > self.left {
            return Err(std::io::Error::other(
                "the archive unpacks to more than the limit",
            ));
        }
        let n = self.inner.write(buf)?;
        self.left -= n as u64;
        Ok(n)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

fn create_new(p: &Path) -> Result<fs::File, ComponentError> {
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent).map_err(io)?;
    }
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(p)
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                unsafe_entry(format!("{} appears twice", p.display()))
            } else {
                io(e)
            }
        })
}

#[cfg(unix)]
fn set_mode(f: &fs::File, exec: bool) -> Result<(), ComponentError> {
    use std::os::unix::fs::PermissionsExt as _;
    let mode = if exec { 0o755 } else { 0o644 };
    f.set_permissions(fs::Permissions::from_mode(mode))
        .map_err(io)
}

#[cfg(not(unix))]
fn set_mode(_f: &fs::File, _exec: bool) -> Result<(), ComponentError> {
    Ok(())
}

fn utf8(bytes: &[u8], what: &str) -> Result<String, ComponentError> {
    String::from_utf8(bytes.to_vec()).map_err(|_| unsafe_entry(format!("{what} is not UTF-8")))
}

fn unpack_tar(reader: impl Read, dest: &Path) -> Result<ExtractReport, ComponentError> {
    let mut ar = tar::Archive::new(reader);
    let mut rep = ExtractReport::default();
    let mut links: Vec<(PathBuf, String)> = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    let mut count: u64 = 0;
    for entry in ar
        .entries()
        .map_err(|e| unsafe_entry(format!("tar: {e}")))?
    {
        let mut e = entry.map_err(|e| unsafe_entry(format!("tar: {e}")))?;
        count += 1;
        if count > MAX_ENTRIES {
            return Err(unsafe_entry("too many entries"));
        }
        let kind = e.header().entry_type();
        // Metadata records (pax headers, GNU long names) carry no file of their own; the tar
        // crate folds the long names into the next entry's path.
        if matches!(
            kind,
            tar::EntryType::XGlobalHeader
                | tar::EntryType::XHeader
                | tar::EntryType::GNULongName
                | tar::EntryType::GNULongLink
        ) {
            continue;
        }
        let path = utf8(&e.path_bytes(), "a path")?;
        let rel = safe_relative_path(&path)?;
        if kind.is_dir() {
            fs::create_dir_all(dest.join(&rel)).map_err(io)?;
            rep.dirs += 1;
            continue;
        }
        if !seen.insert(rel.clone()) {
            return Err(unsafe_entry(format!("{path} appears twice")));
        }
        if kind.is_file() || kind == tar::EntryType::Continuous {
            let exec = e.header().mode().map(|m| m & 0o111 != 0).unwrap_or(false);
            let left = MAX_UNPACKED_BYTES.saturating_sub(rep.bytes);
            let mut out = create_new(&dest.join(&rel))?;
            let n = std::io::copy(&mut (&mut e).take(left.saturating_add(1)), &mut out)
                .map_err(|e| unsafe_entry(format!("tar: {e}")))?;
            if n > left {
                return Err(unsafe_entry("the archive unpacks to more than the limit"));
            }
            out.flush().map_err(io)?;
            set_mode(&out, exec)?;
            rep.bytes += n;
            rep.files += 1;
        } else if kind.is_symlink() {
            let target = e
                .link_name_bytes()
                .ok_or_else(|| unsafe_entry(format!("{path}: symlink without a target")))?;
            links.push((rel, utf8(&target, "a symlink target")?));
        } else {
            return Err(unsafe_entry(format!(
                "{path}: entry type {kind:?} is not allowed"
            )));
        }
    }
    make_symlinks(dest, &links)?;
    rep.symlinks = links.len() as u64;
    Ok(rep)
}

/// Where `target` lands, relative to the tree root, when the link sits at `link`; `None` if
/// it climbs above the root.
fn lexical_target(link: &Path, target: &str) -> Option<PathBuf> {
    if target.is_empty() || target.starts_with('/') || target.contains(['\0', '\\', ':']) {
        return None;
    }
    let mut parts: Vec<std::ffi::OsString> = link
        .parent()
        .map(|p| p.iter().map(|s| s.to_os_string()).collect())
        .unwrap_or_default();
    for part in target.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            normal => parts.push(normal.into()),
        }
    }
    Some(parts.iter().collect())
}

#[cfg(unix)]
fn make_symlinks(dest: &Path, links: &[(PathBuf, String)]) -> Result<(), ComponentError> {
    for (rel, target) in links {
        if lexical_target(rel, target).is_none() {
            return Err(unsafe_entry(format!(
                "{} -> {target} points outside the tree",
                rel.display()
            )));
        }
        // Every link is placed under real directories only: a parent path that passes through
        // an earlier archive symlink could place this link (or create directories) outside the
        // tree before the resolution check below runs.
        parent_has_no_symlink(dest, rel)?;
        let at = dest.join(rel);
        if let Some(parent) = at.parent() {
            fs::create_dir_all(parent).map_err(io)?;
        }
        std::os::unix::fs::symlink(target, &at).map_err(|e| {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                unsafe_entry(format!("{} appears twice", rel.display()))
            } else {
                io(e)
            }
        })?;
    }
    // Resolution check: a chain of individually harmless links can still leave the tree.
    let root = fs::canonicalize(dest).map_err(io)?;
    for (rel, target) in links {
        let resolved = fs::canonicalize(dest.join(rel))
            .map_err(|_| unsafe_entry(format!("{} -> {target} does not resolve", rel.display())))?;
        if !resolved.starts_with(&root) {
            return Err(unsafe_entry(format!(
                "{} -> {target} resolves outside the tree",
                rel.display()
            )));
        }
    }
    Ok(())
}

/// Refuses `rel` when any existing ancestor of it inside `dest` is a symlink.
#[cfg(unix)]
fn parent_has_no_symlink(dest: &Path, rel: &Path) -> Result<(), ComponentError> {
    let mut at = dest.to_path_buf();
    let Some(parent) = rel.parent() else {
        return Ok(());
    };
    for part in parent.iter() {
        at.push(part);
        match fs::symlink_metadata(&at) {
            Ok(m) if m.file_type().is_symlink() => {
                return Err(unsafe_entry(format!(
                    "{}: a link may not be placed under another link",
                    rel.display()
                )));
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(io(e)),
        }
    }
    Ok(())
}

#[cfg(not(unix))]
fn make_symlinks(_dest: &Path, links: &[(PathBuf, String)]) -> Result<(), ComponentError> {
    match links.first() {
        None => Ok(()),
        Some((rel, _)) => Err(unsafe_entry(format!(
            "{}: symlinks are not unpacked on this platform",
            rel.display()
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::lexical_target;
    use std::path::{Path, PathBuf};

    #[test]
    fn lexical_target_resolves_relative_to_the_link() {
        let t = |l: &str, x: &str| lexical_target(Path::new(l), x);
        assert_eq!(
            t("bin/npm", "../lib/cli.js"),
            Some(PathBuf::from("lib/cli.js"))
        );
        assert_eq!(t("a", "b"), Some(PathBuf::from("b")));
        assert_eq!(t("a", "."), Some(PathBuf::new()));
        assert_eq!(t("a", ".."), None);
        assert_eq!(t("bin/x", "../../y"), None);
        assert_eq!(t("x", "/etc"), None);
        assert_eq!(t("x", ""), None);
    }
}
