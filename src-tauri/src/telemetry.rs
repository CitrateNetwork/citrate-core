//! Telemetry WP-T.2/T.3/T.5 (client) — private, local, opt-in, anonymous diagnostics.
//!
//! Nothing here sends on its own. `diagnostics_bundle` ASSEMBLES a scrubbed bundle locally
//! (no network) for the member to review; `telemetry_send` is the ONE pinned HTTPS POST, and
//! it only ever runs when the UI calls it after explicit consent (the ConsentGate spec,
//! WP-T.1, `formal/ConsentGate.tla`). A panic hook appends crash context to a local file that
//! the bundle later reads. Scrubbing is a pure, unit-tested function (WP-T.3): HOME→~ and
//! address/email/token redaction — defence-in-depth on top of a bundle that by construction
//! carries no wallet, key, seed, or OIDC sub.
use serde::{Deserialize, Serialize};
use std::path::Path;

/// The one pinned ingest endpoint (WP-T.5 — a standalone service, isolated from the money
/// path, no IP logging). Pinned in Rust so the webview can never redirect a send. Not live
/// until the DGX stands it up; until then `telemetry_send` fails honestly (Rule 1).
const TELEMETRY_INGEST_URL: &str = "https://telemetry.citrate.ai/report";

/// The local crash file the panic hook appends to (under the app data dir).
const CRASH_FILE: &str = "diagnostics/last-panic.log";

// --------------------------------------------------------------------------
// WP-T.3 — scrub (pure). Best-effort PII strip; over-redaction is safe (Rule 1).
// --------------------------------------------------------------------------

fn is_hex(c: char) -> bool {
    c.is_ascii_hexdigit()
}

/// Redact every `0x`-prefixed run of >=40 hex chars (addresses = 40, hashes = 64) → `0x<redacted>`.
fn redact_hex_blobs(s: &str) -> String {
    let bytes: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == '0' && i + 1 < bytes.len() && (bytes[i + 1] == 'x' || bytes[i + 1] == 'X') {
            let mut j = i + 2;
            while j < bytes.len() && is_hex(bytes[j]) {
                j += 1;
            }
            if j - (i + 2) >= 40 {
                out.push_str("0x<redacted>");
                i = j;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    out
}

/// Redact whitespace-delimited tokens that look like an email (contain `@` with a `.` after it)
/// or a bearer/secret token (known prefixes, or a long JWT-ish `eyJ…`). Punctuation around a
/// token is preserved; only the sensitive core is replaced.
fn redact_tokens(s: &str) -> String {
    s.split_inclusive(|c: char| c.is_whitespace())
        .map(|chunk| {
            // Split trailing whitespace off so we redact just the token, keep the spacing.
            let trimmed = chunk.trim_end();
            let ws = &chunk[trimmed.len()..];
            let core = trimmed.trim_matches(|c: char| "\"'(),;:<>[]{}".contains(c));
            let lead = &trimmed[..trimmed.len()
                - trimmed
                    .trim_start_matches(|c: char| "\"'(),;:<>[]{}".contains(c))
                    .len()];
            let tail = &trimmed[lead.len() + core.len()..];
            let redacted = looks_email(core) || looks_secret(core);
            if redacted {
                format!("{lead}<redacted>{tail}{ws}")
            } else {
                chunk.to_string()
            }
        })
        .collect()
}

fn looks_email(t: &str) -> bool {
    match t.find('@') {
        Some(at) if at > 0 => t[at + 1..].contains('.') && !t[at + 1..].starts_with('.'),
        _ => false,
    }
}

fn looks_secret(t: &str) -> bool {
    let low = t.to_ascii_lowercase();
    low.starts_with("bearer")
        || low.starts_with("sk-")
        || low.starts_with("cgk_")
        || low.starts_with("sk_live_")
        || low.starts_with("sk_test_")
        || (t.starts_with("eyJ") && t.len() > 20) // JWT header
}

/// Scrub a text field: HOME→`~`, then redact hex blobs (addresses/hashes) and email/secret
/// tokens. Pure + deterministic (the report id is added by the caller, not here) so it is
/// unit-tested against fixtures with planted secrets.
pub fn scrub(text: &str, home: &str) -> String {
    let mut s = text.to_string();
    let h = home.trim_end_matches('/');
    if !h.is_empty() {
        s = s.replace(h, "~");
    }
    s = redact_hex_blobs(&s);
    redact_tokens(&s)
}

// --------------------------------------------------------------------------
// WP-T.2 — the bundle
// --------------------------------------------------------------------------

/// A scrubbed diagnostic bundle for the member to review before (optionally) sending.
/// `Deserialize` + `deny_unknown_fields` so `telemetry_send` can re-validate exactly this shape
/// (PBA-L7b-014) — an extra field smuggled in by the webview is refused, not forwarded.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DiagnosticBundle {
    /// Ephemeral, per-report id — random, never stored or linked to identity.
    #[serde(rename = "reportId")]
    pub report_id: String,
    #[serde(rename = "appVersion")]
    pub app_version: String,
    pub os: String,
    /// The last local panic (scrubbed), or empty.
    #[serde(rename = "crashTail")]
    pub crash_tail: String,
    /// The tail of the node crash-records (scrubbed), or empty.
    #[serde(rename = "nodeLogTail")]
    pub node_log_tail: String,
    /// Recent UI errors from the frontend ErrorBoundary ring (scrubbed).
    #[serde(rename = "uiErrors")]
    pub ui_errors: Vec<String>,
}

/// Assemble + scrub a bundle from already-gathered fields (pure — id passed in for tests).
pub fn build_bundle(
    report_id: String,
    app_version: &str,
    os: &str,
    crash_tail: &str,
    node_log_tail: &str,
    ui_errors: &[String],
    home: &str,
) -> DiagnosticBundle {
    DiagnosticBundle {
        report_id,
        app_version: app_version.to_string(),
        os: os.to_string(),
        crash_tail: scrub(crash_tail, home),
        node_log_tail: scrub(node_log_tail, home),
        ui_errors: ui_errors.iter().map(|e| scrub(e, home)).collect(),
    }
}

/// PBA-L7b-014: the most bytes `telemetry_send` will forward (the reviewed bundle is a few KB).
const MAX_BUNDLE_BYTES: usize = 256 * 1024;

/// PBA-L7b-014: re-validate and RE-SCRUB the webview-supplied bundle in Rust before egress.
/// ConsentGate (INV-Consent-3) lives in the webview; this is the Rust backstop so a compromised
/// or buggy renderer cannot post arbitrary JSON (or unscrubbed PII) through the one pinned POST.
/// Only the exact [`DiagnosticBundle`] shape is accepted; every text field is scrubbed again
/// (scrub is idempotent, so an honest bundle is unchanged) and the report id must be the
/// ephemeral `rpt_<hex>` form.
pub fn rescrub_bundle_json(bundle_json: &str, home: &str) -> Result<String, String> {
    if bundle_json.len() > MAX_BUNDLE_BYTES {
        return Err("diagnostic report is too large to send".into());
    }
    let b: DiagnosticBundle = serde_json::from_str(bundle_json)
        .map_err(|_| "diagnostic report is not a reviewed bundle".to_string())?;
    let id_ok = b.report_id.strip_prefix("rpt_").is_some_and(|h| {
        !h.is_empty() && h.len() <= 64 && h.bytes().all(|c| c.is_ascii_hexdigit())
    });
    if !id_ok {
        return Err("diagnostic report id is malformed".into());
    }
    let clean = DiagnosticBundle {
        report_id: b.report_id,
        app_version: scrub(&b.app_version, home),
        os: scrub(&b.os, home),
        crash_tail: scrub(&b.crash_tail, home),
        node_log_tail: scrub(&b.node_log_tail, home),
        ui_errors: b.ui_errors.iter().map(|e| scrub(e, home)).collect(),
    };
    serde_json::to_string(&clean).map_err(|e| e.to_string())
}

/// Read the last `n` lines of a file (for the crash/log tails), or "" if absent.
fn tail_of(path: &Path, n: usize) -> String {
    std::fs::read_to_string(path)
        .map(|s| {
            let lines: Vec<&str> = s.lines().collect();
            let start = lines.len().saturating_sub(n);
            lines[start..].join("\n")
        })
        .unwrap_or_default()
}

/// Install the panic hook: append the panic message + location to the local crash file. No
/// network. Chained after any existing hook. Call once at startup.
pub fn install_panic_hook<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    use tauri::Manager;
    let path = match app.path().app_data_dir() {
        Ok(d) => d.join(CRASH_FILE),
        Err(_) => return,
    };
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let loc = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_default();
        let msg = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_default();
        // Append (best-effort) so the next diagnostics_bundle can read it.
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
        {
            let _ = writeln!(f, "panic at {loc}: {msg}");
        }
        prev(info);
    }));
}

/// **Command — diagnostics_bundle.** Assemble the SCRUBBED bundle locally for review. No
/// network. `ui_errors` is the frontend ErrorBoundary ring. The ephemeral id is fresh each
/// call and never persisted.
#[tauri::command]
pub fn diagnostics_bundle<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    ui_errors: Vec<String>,
) -> std::result::Result<DiagnosticBundle, String> {
    use rand::RngCore;
    use tauri::Manager;
    let data = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let home = std::env::var("HOME").unwrap_or_default();
    let crash_tail = tail_of(&data.join(CRASH_FILE), 40);
    let node_log_tail = tail_of(&data.join("node").join("crash-records.jsonl"), 20);
    let mut id = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut id);
    Ok(build_bundle(
        format!("rpt_{}", hex::encode(id)),
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        &crash_tail,
        &node_log_tail,
        &ui_errors,
        &home,
    ))
}

/// **Command — telemetry_send.** The ONE pinned HTTPS POST. The UI calls this ONLY after the
/// member reviewed the bundle and explicitly consented (ConsentGate, WP-T.1) — this command
/// does not re-gate, but it is the sole egress path and the URL is Rust-pinned. Sends exactly
/// the reviewed bundle JSON (INV-Consent-3). Off the main thread; honest error until the
/// ingest service (WP-T.5) is live.
#[tauri::command]
pub async fn telemetry_send(bundle_json: String) -> std::result::Result<(), String> {
    // PBA-L7b-014: never forward webview JSON verbatim — re-validate + re-scrub in Rust first.
    let home = std::env::var("HOME").unwrap_or_default();
    let clean = rescrub_bundle_json(&bundle_json, &home)?;
    tauri::async_runtime::spawn_blocking(move || {
        ureq::post(TELEMETRY_INGEST_URL)
            .header("content-type", "application/json")
            .send(&clean)
            .map(|_| ())
            .map_err(|e| format!("couldn't send the report: {e}"))
    })
    .await
    .map_err(|e| format!("telemetry_send: background task failed: {e}"))?
}

#[cfg(test)]
mod tests {
    include!("telemetry_tests.rs");
}
