//! CORE-D5 (0.5.3) — sidecar log files and the diagnostics bundle.
//!
//! **Log files.** Every registry owner gives its supervisor a rotating on-disk log
//! ([`citrate_core_kit::sidecar_log`]) under the OS app log folder:
//! `<app log dir>/sidecars/<sidecar>.log`, rotated at [`MAX_LOG_BYTES`] with [`KEEP_ROTATED`]
//! older files, so one sidecar holds at most 4 × 4 MiB and the nine at most 144 MiB. Each line is
//! passed through [`disk_filter`] first (#613 review):
//! - [`crate::secret_shapes::redact`]: bearer and basic auth values, JWTs and three-segment tokens
//!   anywhere in the line, values of password/secret/token/api-key/private-key/mnemonic/seed keys,
//!   64-hex values (bare or `0x`, a short stable tag replaces them, so a block hash still lines
//!   up), emails; then the D-11 vendor key prefixes. 40-hex addresses are kept.
//! - [`LogPolicy::LifecycleOnly`] for the sidecars whose output can carry user content (Hermes,
//!   memory, messaging, the chat and embedding models) is an allowlist: only a line of lifecycle
//!   words and technical tokens (numbers, addresses, paths, versions) is written, up to
//!   [`LIFECYCLE_LINE_CHARS`]. An error, warning or traceback leaves a marker with its level and
//!   length and no text from it; every other line is withheld.
//! - What it cannot promise: free text printed with none of those shapes, and a key or seed
//!   phrase printed with no key name in front of it, on a sidecar with the full policy.
//!
//! "Delete my local data" removes the folder with the rest of the app log dir (`local_data.rs`,
//! `RootKind::Logs`).
//!
//! **Diagnostics bundle.** [`diagnostics_logs_export`] writes one plain-text file the member picks
//! with the save dialog, to review before sending it to anyone. It holds the app version and OS,
//! every sidecar's health row (state, phase, restarts, last crash; from `sidecars_health`), the
//! newest lines of each sidecar's log files, the crash records and the app's last panic. All of it
//! is scrubbed with [`crate::secret_shapes::redact`] and [`crate::telemetry::scrub`] (home folder,
//! addresses and hashes, emails, tokens, key shapes); a content-bearing sidecar's crash records
//! lose their stderr tail. Then the final check runs on the whole text: if any secret shape, key
//! shape or PEM block is left, nothing is written. The scrub is best effort and the bundle says
//! so; it is written for the member to read before sharing. Nothing is sent.
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

/// What a sidecar's log file may keep (#613 review).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogPolicy {
    /// Every line, redacted (node, node agent, IPFS, cluster).
    Full,
    /// Only cut lifecycle and error lines, redacted, never a payload field (Hermes, memory,
    /// messaging, the chat and embedding models: their output can carry prompts, memory results
    /// or message previews).
    LifecycleOnly,
}

/// The longest line a [`LogPolicy::LifecycleOnly`] sidecar keeps, in characters.
pub const LIFECYCLE_LINE_CHARS: usize = 200;
/// Lines queued for a sidecar's log writer.
pub const LOG_QUEUE_LINES: usize = citrate_core_kit::sidecar_log::DEFAULT_QUEUE_LINES;

/// The words an allowlisted lifecycle line may use besides technical tokens (lowercase).
const LIFECYCLE_WORDS: &[&str] = &[
    "listening",
    "listen",
    "starting",
    "started",
    "start",
    "stopping",
    "stopped",
    "stop",
    "shutting",
    "shutdown",
    "down",
    "exiting",
    "exited",
    "exit",
    "ready",
    "loaded",
    "loading",
    "model",
    "version",
    "build",
    "bound",
    "bind",
    "server",
    "http",
    "https",
    "on",
    "at",
    "in",
    "port",
    "ms",
    "s",
    "with",
    "threads",
    "slots",
    "api",
    "ok",
    "done",
    "info",
    "debug",
    "main",
    "up",
    "healthy",
    "health",
    "embed",
    "embedding",
    "chat",
    "hermes",
    "memory",
    "comms",
    "agent",
    "daemon",
    "sidecar",
    "relay",
    "store",
    "socket",
    "llama",
    "llama-server",
    "and",
    "of",
    "to",
];
/// At least one of these makes the line a lifecycle event.
const LIFECYCLE_EVENTS: &[&str] = &[
    "listening",
    "starting",
    "started",
    "stopping",
    "stopped",
    "shutting",
    "shutdown",
    "exiting",
    "exited",
    "ready",
    "loaded",
    "loading",
    "version",
    "bound",
    "healthy",
];
/// Error-level words: such a line leaves a marker with no text from it (lowercase).
const ERROR_LEVELS: &[(&str, &str)] = &[
    ("traceback", "traceback"),
    ("panic", "error"),
    ("fatal", "error"),
    ("exception", "error"),
    ("error", "error"),
    ("warn", "warning"),
];

/// The keys a `k=v` token on an allowed lifecycle line may have (lowercase).
const LIFECYCLE_KEYS: &[&str] = &[
    "port",
    "host",
    "addr",
    "address",
    "bind",
    "threads",
    "slots",
    "ctx",
    "n_ctx",
    "model",
    "version",
    "pid",
    "code",
    "status",
    "ms",
    "gpu",
    "layers",
    "n_gpu_layers",
    "batch",
];

/// A token that reads as text (round 5 P3): a path under a home folder, or three or more
/// letter-only segments joined by `-` or `.` (`call-mom-about-the-biopsy`).
fn free_text(tok: &str) -> bool {
    let low = tok.to_ascii_lowercase();
    if low.starts_with("/users/")
        || low.starts_with("/home/")
        || low.starts_with("~/")
        || low.starts_with("~\\")
        || low.contains(":\\users\\")
        || low.contains("/users/")
        || low.contains("/home/")
    {
        return true;
    }
    tok.split(['/', '\\']).any(|part| {
        let segs: Vec<&str> = part.split(['-', '.']).collect();
        segs.len() >= 3
            && segs
                .iter()
                .all(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphabetic()))
    })
}

/// A technical token: a number, address, path or version, or `k=v` with a lifecycle key, with no
/// quote or bracket.
fn technical(tok: &str) -> bool {
    if tok.is_empty() || tok.contains(['"', '\'', '{', '}', '[', ']', '(', ')', '`']) {
        return false;
    }
    if free_text(tok) {
        return false;
    }
    if let Some((k, v)) = tok.split_once('=') {
        return LIFECYCLE_KEYS.contains(&k.to_ascii_lowercase().as_str()) && !free_text(v);
    }
    tok.chars()
        .any(|c| c.is_ascii_digit() || ".:/_-<>".contains(c))
}

/// Whether `line` is an allowlisted lifecycle line: every word a lifecycle word or technical, at
/// least one lifecycle event, and within [`LIFECYCLE_LINE_CHARS`].
fn allowlisted(line: &str) -> bool {
    if line.chars().count() > LIFECYCLE_LINE_CHARS {
        return false;
    }
    let mut event = false;
    for raw in line.split_whitespace() {
        let tok = raw.trim_end_matches([':', ',', ';']);
        let low = tok.to_ascii_lowercase();
        if LIFECYCLE_EVENTS.contains(&low.as_str()) {
            event = true;
        }
        if !(LIFECYCLE_WORDS.contains(&low.as_str()) || technical(tok)) {
            return false;
        }
    }
    event
}

/// What reaches the disk for one captured line: the redacted line, or `None` to withhold it.
///
/// [`LogPolicy::LifecycleOnly`] is an allowlist (#613 review round 3): only a line whose every word
/// is a lifecycle word or technical is written. Any other line is withheld; one at an error level
/// leaves `[withheld: <level> line, <n> chars, from a content-bearing sidecar]`, with no text from
/// it, so free text, tracebacks, Python dicts and JSON bodies never reach the disk.
pub fn disk_filter(line: &str, policy: LogPolicy) -> Option<String> {
    let red = crate::telemetry::redact_key_shapes(&crate::secret_shapes::redact(line));
    match policy {
        LogPolicy::Full => Some(red),
        LogPolicy::LifecycleOnly => {
            if allowlisted(&red) {
                return Some(red);
            }
            let low = line.to_ascii_lowercase();
            ERROR_LEVELS
                .iter()
                .find(|(w, _)| low.contains(w))
                .map(|(_, level)| {
                    format!(
                        "[withheld: {level} line, {} chars, from a content-bearing sidecar]",
                        line.chars().count()
                    )
                })
        }
    }
}

/// The log-file config for `name` under `dir`.
pub fn config_in(dir: &Path, name: &str, policy: LogPolicy) -> LogFileConfig {
    LogFileConfig {
        path: dir.join(format!("{}.log", file_stem(name))),
        max_bytes: MAX_LOG_BYTES,
        keep: KEEP_ROTATED,
        filter: Some(std::sync::Arc::new(move |l: &str| disk_filter(l, policy))),
        queue: LOG_QUEUE_LINES,
    }
}

/// The crash-record file of each sidecar started in this session, by sidecar name, as its owner
/// configured it, with its log policy (read by the diagnostics bundle).
static CRASH_FILES: std::sync::Mutex<std::collections::BTreeMap<String, (PathBuf, LogPolicy)>> =
    std::sync::Mutex::new(std::collections::BTreeMap::new());

/// Every registry owner calls this on its supervisor config before `Supervisor::start`: it sets
/// the rotating log file with the owner's policy (none before [`init`], e.g. unit tests that build
/// a manager directly, which keeps output in the ring only) and records where the owner keeps
/// crash records.
pub(crate) fn attach(
    config: &mut citrate_core_kit::supervisor::SupervisorConfig,
    policy: LogPolicy,
) {
    config.log_file = log_dir().map(|d| config_in(d, &config.spec.name, policy));
    CRASH_FILES
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(
            config.spec.name.clone(),
            (config.crash_record_path.clone(), policy),
        );
}

/// The crash-record files recorded by [`attach`]: (sidecar, file, policy).
pub(crate) fn crash_files_seen() -> Vec<(String, PathBuf, LogPolicy)> {
    CRASH_FILES
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .map(|(k, (p, pol))| (k.clone(), p.clone(), *pol))
        .collect()
}

// ---------------------------------------------------------------------------
// The diagnostics bundle
// ---------------------------------------------------------------------------

/// The bundle's scrub: the secret shapes, the telemetry scrub (home folder, `0x` hex, key shapes,
/// tokens, emails), then the secret shapes again (idempotent; it catches what the first two
/// expose). The secret shapes run first so that the telemetry scrub cannot hide a token's scheme
/// word and leave the token behind.
pub fn bundle_scrub(text: &str, home: &str) -> String {
    let first = crate::secret_shapes::redact(text);
    crate::secret_shapes::redact(&crate::telemetry::scrub(&first, home))
}

/// The bundle's final check: what must never be in it. Refuses on any survivor.
pub fn bundle_tripwire(text: &str) -> Result<(), String> {
    if crate::telemetry::has_key_shape(text) {
        return Err("the bundle still holds something shaped like a provider key".into());
    }
    if text.contains("-----BEGIN") {
        return Err("the bundle holds a PEM block".into());
    }
    if let Some(shape) = crate::secret_shapes::first_shape(text) {
        return Err(format!("the bundle still holds a {shape}"));
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
    /// (label, path, the sidecar's log policy) of each crash-record file.
    pub crash_files: Vec<(String, PathBuf, LogPolicy)>,
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

/// One crash-record line for the bundle. A content-bearing sidecar's stderr tail can carry user
/// text, so it is withheld; a line that is not a crash record is withheld whole.
fn crash_line(line: &str, policy: LogPolicy) -> String {
    if policy == LogPolicy::Full {
        return line.to_string();
    }
    match serde_json::from_str::<serde_json::Value>(line) {
        Ok(serde_json::Value::Object(mut m)) => {
            if m.contains_key("stderr_tail") {
                m.insert(
                    "stderr_tail".into(),
                    "(stderr tail withheld: this sidecar's output can carry user content)".into(),
                );
            }
            serde_json::Value::Object(m).to_string()
        }
        _ => "(a line that is not a crash record was withheld)".to_string(),
    }
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
        "Read this file before you share it. It is scrubbed on a best-effort basis: your home \
         folder, addresses and hashes, emails, tokens, passwords and key-shaped values are \
         redacted, and the model, memory, messaging and Hermes sidecars keep only start, stop and \
         error lines. Free text a sidecar printed with none of those shapes is not removed.\n",
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
    for (label, path, policy) in &inp.crash_files {
        let lines = tail_lines(path, CRASH_LINES);
        if lines.is_empty() {
            continue;
        }
        out.push_str(&format!("\n== {label} crash records ==\n"));
        for l in lines {
            out.push_str(&crash_line(&l, *policy));
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

/// Refuse a link at `path`, on every platform (#613 review: Windows has no `O_NOFOLLOW`, so this
/// check is what refuses it there; on unix the open refuses it as well).
pub fn refuse_link(path: &Path) -> Result<(), String> {
    match std::fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_symlink() => {
            Err("a link at the export path is refused; choose a plain file".into())
        }
        _ => Ok(()),
    }
}

/// The home folder for the scrub: `HOME`, else `USERPROFILE` (Windows), else none.
pub fn home_from(get: impl Fn(&str) -> Option<String>) -> String {
    get("HOME")
        .filter(|h| !h.is_empty())
        .or_else(|| get("USERPROFILE").filter(|h| !h.is_empty()))
        .unwrap_or_default()
}

/// Write `text` to `path` (owner-only; a link at the path is refused).
pub fn write_bundle(path: &Path, text: &str) -> Result<(), String> {
    use std::io::Write;
    check_export_path(path)?;
    refuse_link(path)?;
    let mut f = crate::journal_export::open_export_file(path)
        .map_err(|e| format!("the bundle could not be written: {e}"))?;
    f.write_all(text.as_bytes())
        .and_then(|()| f.sync_all())
        .map_err(|e| format!("the bundle could not be written: {e}"))
}

/// The crash-record files for the bundle: the node's (written by every session that ran the node,
/// `node.rs` `build_node_state`), then those recorded by [`attach`] in this session.
fn crash_files(data: &Path) -> Vec<(String, PathBuf, LogPolicy)> {
    let node = data.join("node").join("crash-records.jsonl");
    let mut v = vec![("node".to_string(), node.clone(), LogPolicy::Full)];
    v.extend(
        crash_files_seen()
            .into_iter()
            .filter(|(_, p, _)| *p != node),
    );
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
        let home = home_from(|k| std::env::var(k).ok());
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
