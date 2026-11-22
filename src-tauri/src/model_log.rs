//! The local-model download log: `<app log dir>/model-download.log` (on macOS
//! `~/Library/Logs/ai.citrate.core/model-download.log`), rotated at [`MAX_LOG_BYTES`] with
//! [`KEEP_ROTATED`] older files, written by the same rotating writer the sidecar logs use.
//!
//! The webview's errors never reached the disk, so a member's failed first-run download left
//! nothing to read afterwards. Every download start, resume, retry, source change, stall, pause,
//! disk refusal, verify result and final error is written here.
//!
//! What a line can hold: file names, byte counts, hosts and error text. Every URL is cut to its
//! scheme, host and path (a signed CDN URL's `Expires` / `Signature` / `Key-Pair-Id` query never
//! lands), and the line then passes the shared secret-shape scrub. No token is ever handled here.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use citrate_core_kit::sidecar_log::{LogFileConfig, RotatingLog};

/// The log file's name in the app log dir.
pub const LOG_FILE: &str = "model-download.log";
/// Rotate before the file passes this size.
pub const MAX_LOG_BYTES: u64 = 1024 * 1024;
/// Older files kept (`model-download.log.1` .. `.3`).
pub const KEEP_ROTATED: usize = 3;

/// One open download log.
pub struct ModelLog {
    inner: RotatingLog,
}

impl ModelLog {
    /// Prepare the log at `path` (nothing is opened until the first line).
    pub fn open(path: PathBuf, max_bytes: u64, keep: usize) -> ModelLog {
        ModelLog {
            inner: RotatingLog::new(LogFileConfig {
                path,
                max_bytes,
                keep,
                filter: Some(std::sync::Arc::new(|l: &str| Some(scrub(l)))),
                queue: 1,
            }),
        }
    }

    /// Append one event line (best effort; a write failure is counted by the writer, never raised).
    pub fn event(&mut self, line: &str) {
        self.inner.append(now_ms(), "model", line);
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

static GLOBAL: OnceLock<Mutex<ModelLog>> = OnceLock::new();

/// Open the app's download log under `app_log_dir`, once, at start-up.
pub fn init(app_log_dir: &Path) {
    let _ = GLOBAL.set(Mutex::new(ModelLog::open(
        app_log_dir.join(LOG_FILE),
        MAX_LOG_BYTES,
        KEEP_ROTATED,
    )));
}

/// Write one event to the app's download log (a no-op before [`init`], e.g. in unit tests).
pub fn event(line: &str) {
    if let Some(log) = GLOBAL.get() {
        if let Ok(mut l) = log.lock() {
            l.event(line);
        }
    }
}

/// The host of `url` (for a short source label), or the URL without its query when it does not
/// parse.
pub fn host_of(url: &str) -> String {
    url::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_string))
        .unwrap_or_else(|| strip_query(url).to_string())
}

fn strip_query(token: &str) -> &str {
    token.split(['?', '#']).next().unwrap_or("")
}

/// Cut every URL in `line` to scheme, host and path, then run the shared secret-shape scrub.
pub fn scrub(line: &str) -> String {
    let cut: Vec<String> = line
        .split(' ')
        .map(|tok| {
            if tok.contains("://") && (tok.contains('?') || tok.contains('#')) {
                format!("{}?…", strip_query(tok))
            } else if tok.contains("Signature=")
                || tok.contains("Expires=")
                || tok.contains("Key-Pair-Id=")
            {
                "[query withheld]".to_string()
            } else {
                tok.to_string()
            }
        })
        .collect();
    crate::secret_shapes::redact(&cut.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_signed_url_keeps_its_host_and_path_only() {
        let s = scrub("GET https://us.aws.cdn.hf.co/xet-bridge-us/abc?Expires=1795376229&Signature=xyz&Key-Pair-Id=K failed");
        assert!(
            s.contains("https://us.aws.cdn.hf.co/xet-bridge-us/abc"),
            "{s}"
        );
        assert!(
            !s.contains("Signature") && !s.contains("xyz") && !s.contains("Expires"),
            "{s}"
        );
        assert_eq!(host_of("https://citrate.ai/download/model"), "citrate.ai");
    }
}
