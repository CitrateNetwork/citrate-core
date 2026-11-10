//! CORE-D5 (0.5.3) — a sidecar's captured output, kept on disk in rotating files.
//!
//! Until 0.5.3 every sidecar's stdout/stderr lived only in the supervisor's 500-line ring, so a
//! crash, a quit or an update erased what led up to it. [`RotatingLog`] appends each captured line
//! to `<dir>/<name>.log` as `<unix ms> <out|err> <line>`. When the next line would take the file
//! past [`LogFileConfig::max_bytes`], the file becomes `<name>.log.1`, `.1` becomes `.2`, and so
//! on up to [`LogFileConfig::keep`]; the oldest is removed. So one sidecar never holds more than
//! `max_bytes × (keep + 1)` on disk.
//!
//! - Every line passes through the configured filter before it is written (the app passes its
//!   key-shape redaction, so a vendor key a sidecar prints never reaches the disk).
//! - A line longer than [`MAX_LINE_BYTES`] is cut at a character boundary and marked.
//! - The folder is owner-only (`0700`), each file `0600` (unix).
//! - Writing is best effort: a failed write never stops capture into the ring or the child. The
//!   failures are counted and the last one is kept, so the health read can say the file is behind.
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// A line longer than this is cut before it is written.
pub const MAX_LINE_BYTES: usize = 16 * 1024;
/// The smallest rotation size accepted (a smaller value is raised to it).
pub const MIN_MAX_BYTES: u64 = 4 * 1024;

/// A filter applied to each line before it reaches the disk.
pub type LineFilter = Arc<dyn Fn(&str) -> String + Send + Sync>;

/// Where and how a sidecar's output is kept on disk.
#[derive(Clone)]
pub struct LogFileConfig {
    /// The current file, e.g. `<app log dir>/sidecars/node.log`. Rotated files sit beside it.
    pub path: PathBuf,
    /// Rotate before the file would exceed this many bytes.
    pub max_bytes: u64,
    /// How many rotated files to keep (`<name>.log.1` .. `<name>.log.<keep>`).
    pub keep: usize,
    /// Applied to each line before it is written.
    pub filter: Option<LineFilter>,
}

impl std::fmt::Debug for LogFileConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LogFileConfig")
            .field("path", &self.path)
            .field("max_bytes", &self.max_bytes)
            .field("keep", &self.keep)
            .field("filter", &self.filter.is_some())
            .finish()
    }
}

/// What the health read reports about a sidecar's log file.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogFileStatus {
    pub path: String,
    /// Lines that could not be written since this supervisor started.
    pub write_failures: u64,
    /// The last write failure, if any.
    pub last_error: Option<String>,
}

/// The rotated file `n` (1-based) of `path`: `<name>.log.<n>`.
pub fn rotated_path(path: &Path, n: usize) -> PathBuf {
    let mut s = path.as_os_str().to_os_string();
    s.push(format!(".{n}"));
    PathBuf::from(s)
}

/// Every file of this log that exists, oldest first (`.<keep>` .. `.1`, then the current file).
pub fn existing_files(path: &Path, keep: usize) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = (1..=keep)
        .rev()
        .map(|n| rotated_path(path, n))
        .filter(|p| std::fs::symlink_metadata(p).is_ok_and(|m| m.is_file()))
        .collect();
    if std::fs::symlink_metadata(path).is_ok_and(|m| m.is_file()) {
        out.push(path.to_path_buf());
    }
    out
}

/// Cut `line` to at most [`MAX_LINE_BYTES`] bytes at a character boundary.
fn bounded(line: &str) -> std::borrow::Cow<'_, str> {
    if line.len() <= MAX_LINE_BYTES {
        return std::borrow::Cow::Borrowed(line);
    }
    let mut end = MAX_LINE_BYTES;
    while end > 0 && !line.is_char_boundary(end) {
        end -= 1;
    }
    std::borrow::Cow::Owned(format!("{} [line cut]", &line[..end]))
}

fn open_append(path: &Path) -> std::io::Result<std::fs::File> {
    let mut o = std::fs::OpenOptions::new();
    o.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
        // Never append through a link someone left at the log's name.
        o.custom_flags(libc::O_NOFOLLOW);
    }
    let f = o.open(path)?;
    if !f.metadata()?.is_file() {
        return Err(std::io::Error::other("the log path is not a regular file"));
    }
    Ok(f)
}

/// The open log of one sidecar.
pub struct RotatingLog {
    config: LogFileConfig,
    file: Option<std::fs::File>,
    size: u64,
    write_failures: u64,
    last_error: Option<String>,
}

impl RotatingLog {
    /// Prepare the log. Nothing is opened until the first line; a folder that cannot be created
    /// is reported by the first write.
    pub fn new(mut config: LogFileConfig) -> Self {
        config.max_bytes = config.max_bytes.max(MIN_MAX_BYTES);
        RotatingLog {
            config,
            file: None,
            size: 0,
            write_failures: 0,
            last_error: None,
        }
    }

    pub fn status(&self) -> LogFileStatus {
        LogFileStatus {
            path: self.config.path.display().to_string(),
            write_failures: self.write_failures,
            last_error: self.last_error.clone(),
        }
    }

    fn ensure_open(&mut self) -> std::io::Result<()> {
        if self.file.is_some() {
            return Ok(());
        }
        if let Some(dir) = self.config.path.parent() {
            crate::fsutil::ensure_private_dir(dir)?;
        }
        let f = open_append(&self.config.path)?;
        self.size = f.metadata()?.len();
        self.file = Some(f);
        Ok(())
    }

    /// Shift `.1..` up by one (dropping the oldest) and move the current file to `.1`.
    fn rotate(&mut self) -> std::io::Result<()> {
        self.file = None;
        let path = self.config.path.clone();
        let keep = self.config.keep;
        if keep == 0 {
            match std::fs::remove_file(&path) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e),
            }
        } else {
            match std::fs::remove_file(rotated_path(&path, keep)) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e),
            }
            for n in (1..keep).rev() {
                let from = rotated_path(&path, n);
                if std::fs::symlink_metadata(&from).is_ok() {
                    std::fs::rename(&from, rotated_path(&path, n + 1))?;
                }
            }
            if std::fs::symlink_metadata(&path).is_ok() {
                std::fs::rename(&path, rotated_path(&path, 1))?;
            }
        }
        self.size = 0;
        Ok(())
    }

    fn try_append(&mut self, text: &str) -> std::io::Result<()> {
        self.ensure_open()?;
        let len = text.len() as u64;
        if self.size > 0 && self.size.saturating_add(len) > self.config.max_bytes {
            self.rotate()?;
            self.ensure_open()?;
        }
        let f = self
            .file
            .as_mut()
            .ok_or_else(|| std::io::Error::other("the log file is not open"))?;
        f.write_all(text.as_bytes())?;
        self.size = self.size.saturating_add(len);
        Ok(())
    }

    /// Append one captured line. Best effort: a failure is counted, never raised.
    pub fn append(&mut self, ts_unix_ms: u64, stream: &str, line: &str) {
        let filtered = match &self.config.filter {
            Some(f) => f(line),
            None => line.to_string(),
        };
        let text = format!("{ts_unix_ms} {stream} {}\n", bounded(&filtered));
        if let Err(e) = self.try_append(&text) {
            // Drop the handle so the next line reopens (the folder may have been removed).
            self.file = None;
            self.write_failures = self.write_failures.saturating_add(1);
            self.last_error = Some(e.to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    include!("sidecar_log_tests.rs");
}
