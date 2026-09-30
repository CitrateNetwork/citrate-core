//! HUP-S0.1 — UI-RESPONSIVENESS TRIPWIRE v2 (supersedes the 9-file direct-match scan in
//! `lib.rs`, which missed transitive blockers: `ai_chat_local_tools` → `AiManager::chat_local_tools`
//! → `UreqAiClient::post_json`, `node_status` → `status()` → `remote_network_tip`, `hermes_*` →
//! `UreqControl::get`, …).
//!
//! A SYNCHRONOUS `#[tauri::command]` runs on the MAIN THREAD. If anything it calls — directly or
//! through any chain of functions in this crate — performs network, process, socket, or sleep I/O,
//! the window freezes (macOS pinwheel) for the duration. The owner reported exactly that during
//! agent chat (2026-09-29).
//!
//! This test scans EVERY `src/*.rs` file at test time (so new files are covered automatically),
//! builds a crate-local "may block" set by fixpoint over function names, and fails for any sync
//! command whose body reaches that set. Name-based resolution is deliberately conservative: an
//! ambiguous name counts as blocking if ANY definition blocks. A false positive only costs making a
//! command `async`, which is always safe; a false negative freezes the UI.

use std::collections::{BTreeMap, BTreeSet};

/// Direct blocking-I/O markers. A function whose (comment-stripped) body contains one of these
/// may block the calling thread.
const BLOCKING_MARKERS: &[&str] = &[
    "RpcClient::",
    "ureq::",
    "UreqAiClient",
    "reqwest::blocking",
    "TcpStream::connect",
    "UnixStream::connect",
    "Command::new",
    "thread::sleep",
    ".wait()",
    "wait_timeout",
];

struct FnDef {
    file: String,
    name: String,
    is_async: bool,
    is_command: bool,
    /// Body lines after the signature line, comment lines removed.
    body: String,
}

fn parse_fn_sig(line: &str) -> Option<(bool, String)> {
    let mut t = line.trim_start();
    for vis in ["pub(crate) ", "pub(super) ", "pub "] {
        if let Some(rest) = t.strip_prefix(vis) {
            t = rest;
            break;
        }
    }
    let (is_async, t) = match t.strip_prefix("async ") {
        Some(rest) => (true, rest),
        None => (false, t),
    };
    let rest = t.strip_prefix("fn ")?;
    let name: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    if name.is_empty() {
        None
    } else {
        Some((is_async, name))
    }
}

fn collect_defs() -> Vec<FnDef> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .expect("read src/")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "rs"))
        .collect();
    files.sort();
    let attr = "#[tauri".to_string() + "::command]";
    let mut defs = Vec::new();
    for path in files {
        let file = path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let Ok(src) = std::fs::read_to_string(&path) else {
            continue;
        };
        let lines: Vec<&str> = src.lines().collect();
        let starts: Vec<(usize, bool, String)> = lines
            .iter()
            .enumerate()
            .filter_map(|(i, l)| parse_fn_sig(l).map(|(a, n)| (i, a, n)))
            .collect();
        for (k, (i, is_async, name)) in starts.iter().enumerate() {
            let end = starts.get(k + 1).map(|s| s.0).unwrap_or(lines.len());
            // A command attribute sits directly above the signature, possibly separated only by
            // other attributes, doc comments, or blank lines.
            let mut is_command = false;
            let mut j = *i;
            while j > 0 {
                j -= 1;
                let t = lines[j].trim();
                if t == attr {
                    is_command = true;
                    break;
                }
                if t.is_empty()
                    || t.starts_with("///")
                    || t.starts_with("//")
                    || t.starts_with("#[")
                {
                    continue;
                }
                break;
            }
            let body: String = lines[(i + 1).min(end)..end]
                .iter()
                .filter(|l| !l.trim_start().starts_with("//"))
                .copied()
                .collect::<Vec<_>>()
                .join("\n");
            defs.push(FnDef {
                file: file.clone(),
                name: name.clone(),
                is_async: *is_async,
                is_command,
                body,
            });
        }
    }
    defs
}

/// True when `body` calls `name(` (as a free fn, path, or method) — i.e. the name is not part of
/// a longer identifier.
fn calls(body: &str, name: &str) -> bool {
    let bytes = body.as_bytes();
    let mut from = 0;
    while let Some(pos) = body[from..].find(name) {
        let at = from + pos;
        let before_ok = at == 0 || {
            let c = bytes[at - 1] as char;
            !(c.is_ascii_alphanumeric() || c == '_')
        };
        let after = body[at + name.len()..].trim_start();
        if before_ok && after.starts_with('(') {
            return true;
        }
        from = at + name.len();
    }
    false
}

/// Every sync command whose body can reach blocking I/O, as `file::name`.
fn blocking_sync_commands() -> Vec<String> {
    let defs = collect_defs();
    let mut by_name: BTreeMap<&str, Vec<&FnDef>> = BTreeMap::new();
    for d in &defs {
        by_name.entry(d.name.as_str()).or_default().push(d);
    }
    let mut blocking: BTreeSet<&str> = defs
        .iter()
        .filter(|d| BLOCKING_MARKERS.iter().any(|m| d.body.contains(m)))
        .map(|d| d.name.as_str())
        .collect();
    loop {
        let before = blocking.len();
        for (name, ds) in &by_name {
            if blocking.contains(name) {
                continue;
            }
            if ds
                .iter()
                .any(|d| blocking.iter().any(|b| calls(&d.body, b)))
            {
                blocking.insert(name);
            }
        }
        if blocking.len() == before {
            break;
        }
    }
    let mut out: Vec<String> = defs
        .iter()
        .filter(|d| d.is_command && !d.is_async)
        .filter(|d| {
            BLOCKING_MARKERS.iter().any(|m| d.body.contains(m))
                || blocking.iter().any(|b| calls(&d.body, b))
        })
        .map(|d| format!("{}::{}", d.file, d.name))
        .collect();
    out.sort();
    out.dedup();
    out
}

#[test]
fn no_synchronous_tauri_command_blocks_the_main_thread() {
    let offenders = blocking_sync_commands();
    assert!(
        offenders.is_empty(),
        "{} synchronous #[tauri::command]s can reach blocking I/O and will freeze the UI \
         (macOS pinwheel). Make them `pub async fn` and move the blocking work into \
         `tauri::async_runtime::spawn_blocking`: {offenders:#?}",
        offenders.len()
    );
}

#[test]
fn tripwire_detects_a_transitive_blocker() {
    // Guard the guard: the scanner must follow a call chain, not just direct markers.
    assert!(calls("    let x = self.remote_status()?;", "remote_status"));
    assert!(!calls(
        "    let x = self.remote_status_cached;",
        "remote_status"
    ));
    assert!(!calls("    my_remote_status();", "remote_status"));
    let parsed = parse_fn_sig("    pub async fn hermes_status(app: tauri::AppHandle)");
    assert_eq!(parsed, Some((true, "hermes_status".to_string())));
    assert_eq!(
        parse_fn_sig("pub(crate) fn status(&self) -> X {"),
        Some((false, "status".to_string()))
    );
}
