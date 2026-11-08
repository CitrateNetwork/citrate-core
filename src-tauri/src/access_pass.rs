//! First-run access check: an invited install records its access code once (download gate).
//!
//! **What it is.** People admitted from a cohort get an email with a one-time download link and an
//! access code (citrate-landing `docs/DOWNLOAD_GATE.md`). On a fresh install this build asks for the
//! code (pasted, or from a `citrate://access?code=<code>` link), sends it with this install's id to
//! the gate API, and records the answer. The first install to present a code keeps it; the same
//! install may present it again (a reinstall over the same data, a retried request); another install
//! is refused.
//!
//! **What it is not.** It is not a group invite (D-12, [`crate::invites`], `citrate://invite?…`),
//! and it joins nothing: no group, no relay, no roster, no chain. It only answers "was this install
//! invited". Group invites keep their one flow.
//!
//! **Off by default.** The gate URL is fixed at build time from `CITRATE_ACCESS_GATE_URL`. A build
//! without it (every build so far) has the check off: [`access_pass_status`] says `gate: "off"` and
//! nothing is asked or sent.
//!
//! **Updating installs are not asked.** An install that already holds app data from an earlier run
//! (`config.json` in the app data dir) when the check first runs is recorded as `existing` and never
//! asked, so members updating from an earlier version keep working.
//!
//! **Data source (Rule 7).** `POST <gate>` (`/api/access/verify` on citrate.ai), body
//! `{"code","install"}`, backed by the `download_invites` table in the site database. Answers:
//! 200 `{"ok":true,"platform"}`, 409 `{"ok":false,"reason":"used"|"expired"|"revoked"}`, 400, 404
//! (gate off on the server), 429, 503.
//!
//! **Store.** `access/pass.json`, owner-only (`0600`) in an owner-only directory, written atomically
//! ([`crate::atomic_file`]), one format ([`STORE_FORMAT`]). It keeps the install id (32 random bytes,
//! made here, sent only to the gate), and once recorded the platform and time. The access code is
//! never stored or logged.
use serde::{Deserialize, Serialize};
use tauri::Manager;

/// The gate API this build checks against, fixed at build time. `None` = the check is off.
pub(crate) const GATE_URL: Option<&str> = option_env!("CITRATE_ACCESS_GATE_URL");

/// The one `pass.json` format this build reads and writes.
pub(crate) const STORE_FORMAT: &str = "citrate-access-pass-1";

/// Length of an access code and of an install id: 32 bytes, unpadded base64url.
pub(crate) const CODE_LEN: usize = 43;

/// The longest pasted input looked at.
const MAX_INPUT_LEN: usize = 512;

/// The longest answer body read from the gate.
const MAX_BODY: u64 = 4096;

/// Platforms the gate can name (citrate-landing `GATE_PLATFORMS`).
const PLATFORMS: &[&str] = &["mac", "linux-x86_64", "linux-aarch64", "linux-deb-x86_64", "linux-deb-aarch64"];

/// `pass.json`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PassFile {
    pub format: String,
    /// This install's id (sent to the gate; the server keeps only its SHA-256).
    pub install_id: String,
    /// "invited" once the gate accepted a code, "existing" for an install that predates the check,
    /// absent while nothing is recorded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recorded: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platform: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recorded_at: Option<u64>,
}

/// What the app shows. Crosses `invoke`; never carries the code or the install id.
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AccessPassView {
    /// "off" (this build does not check) or "on".
    pub gate: &'static str,
    /// True when the app must ask for a code before setup continues.
    pub required: bool,
    /// "invited", "existing", or None.
    pub recorded: Option<String>,
    pub platform: Option<String>,
}

/// Why a code could not be used, in words the person can act on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AccessError {
    /// Not an access code or `citrate://access` link.
    NotACode,
    /// A group invite link was pasted where an access code goes.
    GroupInviteLink,
    /// The code was used by another install.
    Used,
    Expired,
    Revoked,
    /// This build has no gate.
    GateOff,
    RateLimited,
    /// The gate could not be reached or answered something else.
    Unavailable,
    /// Local store problem.
    Store(String),
}

impl AccessError {
    pub(crate) fn message(&self) -> String {
        match self {
            AccessError::NotACode => "That isn't an access code. Paste the code from your invite email, or open the citrate://access link in it.".into(),
            AccessError::GroupInviteLink => "That is a group invite link. Paste it on the Join screen after setup; here, use the access code from your download email.".into(),
            AccessError::Used => "This access code is already in use on another computer. Reply to your invite email if you need a new one.".into(),
            AccessError::Expired => "This access code has expired. Reply to your invite email for a new one.".into(),
            AccessError::Revoked => "This access code was withdrawn. Reply to your invite email if you think this is a mistake.".into(),
            AccessError::GateOff => "This build does not use access codes.".into(),
            AccessError::RateLimited => "Too many tries from this network. Wait a few minutes and try again.".into(),
            AccessError::Unavailable => "Could not check the code right now. Your code has not been used. Check your connection and try again.".into(),
            AccessError::Store(e) => format!("Could not save the access check on this computer: {e}"),
        }
    }
}

fn is_code(s: &str) -> bool {
    s.len() == CODE_LEN && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Read an access code from what was pasted or deep-linked: a bare code, or
/// `citrate://access?code=<code>`. Anything else is refused; a group invite link gets its own message.
pub(crate) fn parse_input(input: &str) -> Result<String, AccessError> {
    let s = input.trim();
    if s.is_empty() || s.len() > MAX_INPUT_LEN {
        return Err(AccessError::NotACode);
    }
    if is_code(s) {
        return Ok(s.to_string());
    }
    let url = url::Url::parse(s).map_err(|_| AccessError::NotACode)?;
    if url.scheme() != "citrate" {
        return Err(AccessError::NotACode);
    }
    match url.host_str() {
        Some("invite") => Err(AccessError::GroupInviteLink),
        Some("access") => {
            let codes: Vec<String> = url
                .query_pairs()
                .filter(|(k, _)| k == "code")
                .map(|(_, v)| v.into_owned())
                .collect();
            match codes.as_slice() {
                [one] if is_code(one) => Ok(one.clone()),
                _ => Err(AccessError::NotACode),
            }
        }
        _ => Err(AccessError::NotACode),
    }
}

/// Whether a gate URL may be used: https, or plain http to this machine (local test fakes).
pub(crate) fn gate_url_ok(u: &str) -> bool {
    match url::Url::parse(u) {
        Ok(p) if p.scheme() == "https" => p.host_str().is_some(),
        Ok(p) if p.scheme() == "http" => matches!(p.host_str(), Some("127.0.0.1") | Some("localhost")),
        _ => false,
    }
}

/// The gate's answer, read strictly.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Answer {
    Recorded { platform: String },
    Refused(AccessError),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OkBody {
    ok: bool,
    platform: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NoBody {
    ok: bool,
    reason: String,
}

pub(crate) fn read_answer(status: u16, body: &str) -> Answer {
    match status {
        200 => match serde_json::from_str::<OkBody>(body) {
            Ok(b) if b.ok && PLATFORMS.contains(&b.platform.as_str()) => Answer::Recorded { platform: b.platform },
            _ => Answer::Refused(AccessError::Unavailable),
        },
        409 => match serde_json::from_str::<NoBody>(body) {
            Ok(b) if !b.ok => Answer::Refused(match b.reason.as_str() {
                "used" => AccessError::Used,
                "expired" => AccessError::Expired,
                "revoked" => AccessError::Revoked,
                _ => AccessError::Unavailable,
            }),
            _ => Answer::Refused(AccessError::Unavailable),
        },
        400 => Answer::Refused(AccessError::NotACode),
        404 => Answer::Refused(AccessError::GateOff),
        429 => Answer::Refused(AccessError::RateLimited),
        _ => Answer::Refused(AccessError::Unavailable),
    }
}

// ---------------------------------------------------------------------------
// The store.
// ---------------------------------------------------------------------------

fn new_install_id() -> String {
    crate::connections::random_state()
}

/// Read `pass.json`. A missing file is `None`. A file that does not parse, or is in another format,
/// is an error (never read as "nothing recorded"); the next write replaces it.
pub(crate) fn read_store(path: &std::path::Path) -> Result<Option<PassFile>, AccessError> {
    match std::fs::read(path) {
        Ok(bytes) => {
            let f: PassFile =
                serde_json::from_slice(&bytes).map_err(|_| AccessError::Store("pass.json is damaged".into()))?;
            if f.format != STORE_FORMAT || !is_code(&f.install_id) {
                return Err(AccessError::Store("pass.json is in another format".into()));
            }
            Ok(Some(f))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(AccessError::Store(e.to_string())),
    }
}

pub(crate) fn write_store(path: &std::path::Path, f: &PassFile) -> Result<(), AccessError> {
    let bytes = serde_json::to_vec_pretty(f).map_err(|e| AccessError::Store(e.to_string()))?;
    let staged = crate::atomic_file::stage_secret(path, &bytes).map_err(|e| AccessError::Store(e.to_string()))?;
    crate::atomic_file::commit_staged(&staged, path).map_err(|e| AccessError::Store(e.to_string()))
}

fn load_or_new(path: &std::path::Path) -> Result<PassFile, AccessError> {
    let existing = match read_store(path) {
        Ok(f) => f,
        // A damaged or foreign file is replaced by a fresh one (nothing recorded), never trusted.
        Err(AccessError::Store(_)) if path.exists() => None,
        Err(e) => return Err(e),
    };
    Ok(existing.unwrap_or_else(|| PassFile {
        format: STORE_FORMAT.into(),
        install_id: new_install_id(),
        recorded: None,
        platform: None,
        recorded_at: None,
    }))
}

/// The status, recording `existing` the first time for an install that already had app data.
pub(crate) fn status_at(
    path: &std::path::Path,
    gate: Option<&str>,
    prior_install: bool,
    now: u64,
) -> Result<AccessPassView, AccessError> {
    let Some(url) = gate else {
        return Ok(AccessPassView { gate: "off", required: false, recorded: None, platform: None });
    };
    if !gate_url_ok(url) {
        return Ok(AccessPassView { gate: "off", required: false, recorded: None, platform: None });
    }
    let mut f = load_or_new(path)?;
    if f.recorded.is_none() && prior_install {
        f.recorded = Some("existing".into());
        f.recorded_at = Some(now);
        write_store(path, &f)?;
    }
    Ok(AccessPassView {
        gate: "on",
        required: f.recorded.is_none(),
        recorded: f.recorded.clone(),
        platform: f.platform.clone(),
    })
}

/// Check a code with the gate and record it. `post(url, body)` returns `(status, body)`.
pub(crate) fn verify_at(
    path: &std::path::Path,
    gate: Option<&str>,
    input: &str,
    now: u64,
    post: impl FnOnce(&str, &str) -> Result<(u16, String), String>,
) -> Result<AccessPassView, AccessError> {
    let url = gate.filter(|u| gate_url_ok(u)).ok_or(AccessError::GateOff)?;
    let code = parse_input(input)?;
    // The install id is saved BEFORE the request, so a retry after a lost answer is the same install.
    let mut f = load_or_new(path)?;
    if !path.exists() {
        write_store(path, &f)?;
    }
    let body = serde_json::json!({ "code": code, "install": f.install_id }).to_string();
    let (status, text) = post(url, &body).map_err(|_| AccessError::Unavailable)?;
    match read_answer(status, &text) {
        Answer::Recorded { platform } => {
            f.recorded = Some("invited".into());
            f.platform = Some(platform);
            f.recorded_at = Some(now);
            write_store(path, &f)?;
            Ok(AccessPassView { gate: "on", required: false, recorded: f.recorded, platform: f.platform })
        }
        Answer::Refused(e) => Err(e),
    }
}

/// POST JSON with a bounded deadline; non-2xx comes back as a status, not an error.
pub(crate) fn post_json(url: &str, body: &str) -> Result<(u16, String), String> {
    let mut resp = ureq::post(url)
        .header("Content-Type", "application/json")
        .config()
        .http_status_as_error(false)
        .timeout_global(Some(std::time::Duration::from_secs(10)))
        .build()
        .send(body)
        .map_err(|e| crate::deep_link_delivery::log_redact_text(&e.to_string()))?;
    let status = resp.status().as_u16();
    let text = resp
        .body_mut()
        .with_config()
        .limit(MAX_BODY)
        .read_to_string()
        .map_err(|e| e.to_string())?;
    Ok((status, text))
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn store_path(app: &tauri::AppHandle) -> Result<std::path::PathBuf, AccessError> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| AccessError::Store(e.to_string()))?
        .join("access");
    citrate_core_kit::fsutil::ensure_private_dir(&dir).map_err(|e| AccessError::Store(e.to_string()))?;
    Ok(dir.join("pass.json"))
}

/// An earlier run of the app left its settings (`config.json`, written by every version since 0.2).
fn prior_install(app: &tauri::AppHandle) -> bool {
    app.path()
        .app_data_dir()
        .map(|d| d.join("config.json").is_file())
        .unwrap_or(false)
}

/// `access_pass_status`: whether this build checks access, and whether setup must ask for a code.
#[tauri::command]
pub async fn access_pass_status(app: tauri::AppHandle) -> Result<AccessPassView, String> {
    crate::blocking::off_main(move || {
        if GATE_URL.is_none() {
            return status_at(std::path::Path::new(""), None, false, 0).map_err(|e| e.message());
        }
        let path = store_path(&app).map_err(|e| e.message())?;
        status_at(&path, GATE_URL, prior_install(&app), now_unix()).map_err(|e| e.message())
    })
    .await
}

/// `access_pass_verify`: check a pasted code or `citrate://access` link with the gate and record it.
#[tauri::command]
pub async fn access_pass_verify(app: tauri::AppHandle, input: String) -> Result<AccessPassView, String> {
    crate::blocking::off_main(move || {
        if GATE_URL.is_none() {
            return Err(AccessError::GateOff.message());
        }
        let path = store_path(&app).map_err(|e| e.message())?;
        verify_at(&path, GATE_URL, &input, now_unix(), post_json).map_err(|e| e.message())
    })
    .await
}

#[cfg(test)]
mod tests {
    include!("access_pass_tests.rs");
}
