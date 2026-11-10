//! CORE-D5 (0.5.3) — sidecar log files and the diagnostics bundle.
//!
//! **Log files.** Every registry owner gives its supervisor a rotating on-disk log
//! ([`citrate_core_kit::sidecar_log`]) under the OS app log folder:
//! `<app log dir>/sidecars/<sidecar>.log`, rotated at [`MAX_LOG_BYTES`] with [`KEEP_ROTATED`]
//! older files, so one sidecar holds at most 4 × 4 MiB and the nine at most 144 MiB. Each line is
//! passed through [`disk_filter`] first: key shapes (the D-11 vendor prefixes, the gateway and proxy
//! tokens), bearer tokens, JWTs and emails never reach the disk. Block hashes and addresses are
//! kept, since they are what a node log is read for. "Delete my local data" removes the folder with
//! the rest of the app log dir (`local_data.rs`, `RootKind::Logs`).
//!
//! **Diagnostics bundle.** [`diagnostics_logs_export`] writes one plain-text file the member picks
//! with the save dialog, to review before sending it to anyone. It holds the app version and OS,
//! every sidecar's health row (state, phase, restarts, last crash; from `sidecars_health`), the
//! newest lines of each sidecar's log files, the crash records and the app's last panic. All of it
//! is scrubbed with [`crate::telemetry::scrub`] (home folder, addresses and hashes, emails, tokens,
//! key shapes) plus bare 64-hex runs (a private key printed without `0x`). Then the key tripwire
//! runs on the whole text: if anything key-shaped is left, nothing is written. Nothing is sent.
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use citrate_core_kit::sidecar_log::{existing_files, LogFileConfig};

/// The folder under the OS app log dir that holds the sidecar logs.
pub const LOGS_SUBDIR: &str = "sidecars";
/// Rotate a sidecar's log before it passes this size.
pub const MAX_LOG_BYTES: u64 = 4 * 1024 * 1024;
/// Older files kept per sidecar.
pub const KEEP_ROTATED: usize = 3;
/// Newest lines of each sidecar's logs in a bundle.
pub const BUNDLE_LINES_PER_SIDECAR: usize = 2_000;
/// Newest bytes of each sidecar's logs read for a bundle.
const BUNDLE_BYTES_PER_SIDECAR: u64 = 1024 * 1024;
/// The bundle is refused past this size (it is meant to be read by a person).
pub const MAX_BUNDLE_BYTES: usize = 24 * 1024 * 1024;
/// Newest lines of each crash-record file in a bundle.
const CRASH_LINES: usize = 50;

static LOG_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Set the sidecar log folder once, at start-up, before any sidecar starts.
pub fn init(app_log_dir: &Path) {
    let _ = LOG_DIR.set(app_log_dir.join(LOGS_SUBDIR));
}

/// The sidecar log folder, once [`init`] ran.
pub fn log_dir() -> Option<&'static Path> {
    LOG_DIR.get().map(PathBuf::as_path)
}

/// A file-name-safe form of a sidecar name.
pub fn file_stem(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .take(48)
        .collect();
    if s.is_empty() {
        "sidecar".to_string()
    } else {
        s
    }
}

/// What reaches the disk for each captured line.
pub fn disk_filter(line: &str) -> String {
    crate::telemetry::redact_secrets(line)
}

/// The log-file config for `name` under `dir`.
pub fn config_in(dir: &Path, name: &str) -> LogFileConfig {
    LogFileConfig {
        path: dir.join(format!("{}.log", file_stem(name))),
        max_bytes: MAX_LOG_BYTES,
        keep: KEEP_ROTATED,
        filter: Some(std::sync::Arc::new(disk_filter)),
    }
}

/// The crash-record file of each sidecar started in this session, by sidecar name, as its owner
/// configured it (read by the diagnostics bundle).
static CRASH_FILES: std::sync::Mutex<std::collections::BTreeMap<String, PathBuf>> =
    std::sync::Mutex::new(std::collections::BTreeMap::new());

/// Every registry owner calls this on its supervisor config before `Supervisor::start`: it sets
/// the rotating log file (none before [`init`], e.g. unit tests that build a manager directly,
/// which keeps output in the ring only) and records where the owner keeps crash records.
pub(crate) fn attach(config: &mut citrate_core_kit::supervisor::SupervisorConfig) {
    config.log_file = log_dir().map(|d| config_in(d, &config.spec.name));
    CRASH_FILES
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(config.spec.name.clone(), config.crash_record_path.clone());
}

/// The crash-record files recorded by [`attach`], by sidecar name.
pub(crate) fn crash_files_seen() -> Vec<(String, PathBuf)> {
    CRASH_FILES
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

// ---------------------------------------------------------------------------
// The diagnostics bundle
// ---------------------------------------------------------------------------

/// Redact runs of 64 or more hex digits not already behind `0x` (private-key shaped).
fn redact_bare_hex(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < b.len() {
        if b[i].is_ascii_hexdigit() && (i == 0 || !b[i - 1].is_ascii_alphanumeric()) {
            let mut j = i;
            while j < b.len() && b[j].is_ascii_hexdigit() {
                j += 1;
            }
            let bounded = j == b.len() || !b[j].is_ascii_alphanumeric();
            if j - i >= 64 && bounded {
                out.push_str("<redacted>");
                i = j;
                continue;
            }
            out.push_str(&s[i..j]);
            i = j;
            continue;
        }
        let ch_len = s[i..].chars().next().map_or(1, char::len_utf8);
        out.push_str(&s[i..i + ch_len]);
        i += ch_len;
    }
    out
}

/// The bundle's scrub: the telemetry scrub, then bare 64-hex runs.
pub fn bundle_scrub(text: &str, home: &str) -> String {
    redact_bare_hex(&crate::telemetry::scrub(text, home))
}

/// The key tripwire on a finished bundle: what must never be in it.
pub fn bundle_tripwire(text: &str) -> Result<(), String> {
    if crate::telemetry::has_vendor_key_shape(text) {
        return Err("the bundle still holds something shaped like a provider key".into());
    }
    if text.contains("-----BEGIN") {
        return Err("the bundle holds a PEM block".into());
    }
    if redact_bare_hex(text) != text {
        return Err("the bundle holds a 64-hex run".into());
    }
    Ok(())
}

/// The newest `max_lines` lines (within `max_bytes`) across a sidecar's log files, oldest first.
pub fn tail_of_logs(path: &Path, keep: usize, max_lines: usize, max_bytes: u64) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut budget = max_bytes;
    for f in existing_files(path, keep).iter().rev() {
        if lines.len() >= max_lines || budget == 0 {
            break;
        }
        let Ok(text) = read_tail(f, budget) else {
            continue;
        };
        budget = budget.saturating_sub(text.len() as u64);
        let mut these: Vec<String> = text.lines().map(str::to_string).collect();
        let room = max_lines - lines.len();
        if these.len() > room {
            these.drain(..these.len() - room);
        }
        these.append(&mut lines);
        lines = these;
    }
    lines
}

/// The last `max` bytes of `path`, from a line start, lossy UTF-8.
fn read_tail(path: &Path, max: u64) -> std::io::Result<String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path)?;
    let len = f.metadata()?.len();
    let start = len.saturating_sub(max);
    f.seek(SeekFrom::Start(start))?;
    let mut buf = Vec::with_capacity((len - start) as usize);
    f.take(max).read_to_end(&mut buf)?;
    let text = String::from_utf8_lossy(&buf).to_string();
    if start > 0 {
        // Drop the partial first line.
        return Ok(text
            .split_once('\n')
            .map_or(String::new(), |(_, r)| r.to_string()));
    }
    Ok(text)
}

/// The log names in `dir` (`<stem>.log`), sorted.
pub fn log_names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .map(|r| {
            r.filter_map(Result::ok)
                .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
                .filter_map(|e| e.file_name().to_str().map(str::to_string))
                .filter_map(|n| n.strip_suffix(".log").map(str::to_string))
                .filter(|stem| file_stem(stem) == *stem)
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

/// What the bundle is built from.
pub struct BundleInputs<'a> {
    pub app_version: &'a str,
    pub os: &'a str,
    pub made_at_unix_ms: u64,
    /// `sidecars_health` with no log tail, as JSON.
    pub health_json: String,
    /// The sidecar log folder.
    pub log_dir: Option<&'a Path>,
    /// (label, path) of each crash-record file.
    pub crash_files: Vec<(String, PathBuf)>,
    /// The app's panic log.
    pub panic_file: Option<PathBuf>,
    pub home: &'a str,
}

/// What the export reports.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsExport {
    pub path: String,
    pub bytes: usize,
    /// Sidecars whose log lines are in the bundle.
    pub sidecars: Vec<String>,
    pub log_lines: usize,
}

fn tail_lines(path: &Path, n: usize) -> Vec<String> {
    let text = read_tail(path, 256 * 1024).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(n)..]
        .iter()
        .map(|s| s.to_string())
        .collect()
}

/// Build the scrubbed bundle text. Returns the text, the sidecars included and the line count.
pub fn build_bundle(inp: &BundleInputs<'_>) -> Result<(String, Vec<String>, usize), String> {
    let mut out = String::new();
    out.push_str("Citrate Core diagnostics bundle\n");
    out.push_str(&format!(
        "app {} on {}, made at unix ms {}\n",
        inp.app_version, inp.os, inp.made_at_unix_ms
    ));
    out.push_str(
        "Review this file before you share it. Your home folder, addresses and hashes, emails, \
         tokens and key-shaped text are redacted. It holds no wallet, key or seed.\n",
    );
    out.push_str("\n== Sidecar health (sidecars_health) ==\n");
    out.push_str(&inp.health_json);
    out.push('\n');
    let mut sidecars = Vec::new();
    let mut count = 0usize;
    if let Some(dir) = inp.log_dir {
        for stem in log_names(dir) {
            let lines = tail_of_logs(
                &dir.join(format!("{stem}.log")),
                KEEP_ROTATED,
                BUNDLE_LINES_PER_SIDECAR,
                BUNDLE_BYTES_PER_SIDECAR,
            );
            out.push_str(&format!(
                "\n== {stem} log (newest {} lines) ==\n",
                lines.len()
            ));
            count += lines.len();
            for l in &lines {
                out.push_str(l);
                out.push('\n');
            }
            sidecars.push(stem);
        }
    } else {
        out.push_str("\n== Sidecar logs ==\nThe log folder is not set up in this session.\n");
    }
    for (label, path) in &inp.crash_files {
        let lines = tail_lines(path, CRASH_LINES);
        if lines.is_empty() {
            continue;
        }
        out.push_str(&format!("\n== {label} crash records ==\n"));
        for l in lines {
            out.push_str(&l);
            out.push('\n');
        }
    }
    if let Some(p) = &inp.panic_file {
        let lines = tail_lines(p, 40);
        if !lines.is_empty() {
            out.push_str("\n== App panics ==\n");
            for l in lines {
                out.push_str(&l);
                out.push('\n');
            }
        }
    }
    let text = bundle_scrub(&out, inp.home);
    bundle_tripwire(&text)?;
    if text.len() > MAX_BUNDLE_BYTES {
        return Err("the diagnostics bundle would be larger than 24 MiB".into());
    }
    Ok((text, sidecars, count))
}

/// The export path must be absolute and end in `.txt`.
pub fn check_export_path(path: &Path) -> Result<(), String> {
    let ok = path.is_absolute()
        && path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("txt"));
    if ok {
        Ok(())
    } else {
        Err("choose a .txt file to save the diagnostics bundle".into())
    }
}

/// Write `text` to `path` (owner-only; a link at the path is refused).
pub fn write_bundle(path: &Path, text: &str) -> Result<(), String> {
    use std::io::Write;
    check_export_path(path)?;
    let mut f = crate::journal_export::open_export_file(path)
        .map_err(|e| format!("the bundle could not be written: {e}"))?;
    f.write_all(text.as_bytes())
        .and_then(|()| f.sync_all())
        .map_err(|e| format!("the bundle could not be written: {e}"))
}

/// The crash-record files for the bundle: the node's (written by every session that ran the node,
/// `node.rs` `build_node_state`), then those recorded by [`attach`] in this session.
fn crash_files(data: &Path) -> Vec<(String, PathBuf)> {
    let node = data.join("node").join("crash-records.jsonl");
    let mut v = vec![("node".to_string(), node.clone())];
    v.extend(crash_files_seen().into_iter().filter(|(_, p)| *p != node));
    v
}

/// **diagnostics_logs_export** — write the scrubbed diagnostics bundle to a `.txt` file the
/// member chose. Data sources: each sidecar's supervisor (`sidecars_health`), the sidecar log
/// files under `<app log dir>/sidecars/`, the crash-record files in the app data dir and the app's
/// panic log. Local only: nothing is sent.
#[tauri::command]
pub async fn diagnostics_logs_export(
    app_h: tauri::AppHandle,
    path: String,
) -> std::result::Result<DiagnosticsExport, String> {
    crate::blocking::off_main(move || {
        use tauri::Manager;
        let dest = PathBuf::from(&path);
        check_export_path(&dest)?;
        let data = app_h.path().app_data_dir().map_err(|e| e.to_string())?;
        let home = std::env::var("HOME").unwrap_or_default();
        let health = crate::services::sidecars_health_sync(&app_h, 0);
        let health_json = serde_json::to_string_pretty(&health).map_err(|e| e.to_string())?;
        let made = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or_default();
        let (text, sidecars, log_lines) = build_bundle(&BundleInputs {
            app_version: env!("CARGO_PKG_VERSION"),
            os: std::env::consts::OS,
            made_at_unix_ms: made,
            health_json,
            log_dir: log_dir(),
            crash_files: crash_files(&data),
            panic_file: Some(data.join(crate::telemetry::CRASH_FILE)),
            home: &home,
        })?;
        write_bundle(&dest, &text)?;
        Ok(DiagnosticsExport {
            path: dest.display().to_string(),
            bytes: text.len(),
            sidecars,
            log_lines,
        })
    })
    .await
}

#[cfg(test)]
mod tests {
    include!("sidecar_logs_tests.rs");
}
