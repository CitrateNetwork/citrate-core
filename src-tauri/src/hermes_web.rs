//! HUP-S5.1 / S5.2 / S5.3: the member's web opt-ins for Hermes: its own browser (the managed
//! Chromium, `CITRATE_HERMES_BROWSER`), private search (`web_search` over a local SearXNG), page
//! reading (`read_url`, local readability by default, the Jina Reader only as an explicit choice),
//! and the opt-in TypeSafe Jev backend of the `decide()` slot.
//!
//! The tools themselves run in the Hermes sidecar (citrate-agent-runtime `agent-search`,
//! `agent-loop::decide`). Core owns the member's choices: it stores them in
//! `<app data>/hermes/web-settings.json` and turns them into the sidecar's environment when it
//! starts Hermes ([`sidecar_env`]). Changes apply the next time Hermes starts.
//!
//! **Defaults change nothing.** Everything is off: no browser, no search tools, local reading, no
//! Jev. With the default settings no variable is passed and the sidecar behaves exactly as before.
//!
//! **The browser.** With the switch on, the sidecar offers the `browser_*` tools and runs a
//! headless Chromium with a fresh private profile. It prefers the managed Chromium installed by the
//! signed component updater (`chromium` component, HUP-S5.5): when that is installed, core passes
//! its executable as `CITRATE_BROWSER_CHROMIUM`. Until then the sidecar uses a Chrome already on
//! this computer, or reports "not installed". Attaching to the member's own Chrome is never a
//! setting: it stays a per-session consent in the Browser pop-out.
//!
//! **Keys.** A Jina or TypeSafe key is a file the member chooses (absolute path, readable only by
//! them); core passes the path, never the key, and never reads or returns it. Moving these keys into
//! the custody vault is pending owner sign-off.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// The settings file inside the Hermes data folder.
pub const SETTINGS_FILE: &str = "web-settings.json";
/// Most Jev origins a member can list.
pub const MAX_JEV_ORIGINS: usize = 32;

/// Every variable [`sidecar_env`] may emit. The manager drops anything else from an env source, so
/// these settings can never override the control bind, the bearer file, or the capsule folder.
pub const SIDECAR_ENV_KEYS: &[&str] = &[
    "CITRATE_HERMES_BROWSER",
    "CITRATE_BROWSER_CHROMIUM",
    "CITRATE_HERMES_SEARCH",
    "CITRATE_HERMES_SEARXNG",
    "CITRATE_HERMES_SEARXNG_DATA",
    "CITRATE_HERMES_READER",
    "CITRATE_HERMES_JINA_KEY_FILE",
    "CITRATE_HERMES_JEV",
    "CITRATE_HERMES_JEV_KEY_FILE",
    "CITRATE_HERMES_JEV_ORIGINS",
    "CITRATE_HERMES_JEV_NON_WEB",
    "CITRATE_HERMES_DECIDE_LOG",
];

/// Extra sidecar environment, computed at each start.
pub type EnvSource = Arc<dyn Fn() -> Vec<(String, String)> + Send + Sync>;

/// How `read_url` turns a page into text.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReaderChoice {
    /// Fetch here and extract on this machine.
    #[default]
    Local,
    /// Send the URL to the Jina Reader (a third-party service).
    Jina,
}

/// The member's choices. Every field defaults to off / local.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct HermesWebSettings {
    /// HUP-S5.1: offer the `browser_*` tools (Hermes's own headless browser). Off by default.
    pub browser_enabled: bool,
    /// Offer `web_search` + `read_url` to Hermes.
    pub search_enabled: bool,
    /// Absolute path of `searxng-run` (or its virtualenv). None = search reports "not installed".
    pub searxng_path: Option<String>,
    pub reader: ReaderChoice,
    /// Absolute path of a file holding a Jina API key (optional; the reader works without one).
    pub jina_key_file: Option<String>,
    /// Let the TypeSafe Jev backend answer `decide()` for the listed origins.
    pub jev_enabled: bool,
    /// Absolute path of a file holding the TypeSafe API key (Jev stays off without one).
    pub jev_key_file: Option<String>,
    /// `https://host[:port]` origins Jev may decide for.
    pub jev_origins: Vec<String>,
    /// Jev may also answer decisions with no web origin (routing, ranking).
    pub jev_non_web: bool,
}

/// What the Settings card shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HermesWebStatus {
    pub settings: HermesWebSettings,
    /// HUP-S5.1: the managed Chromium's executable when the signed `chromium` component is
    /// installed; `None` = not installed (the sidecar then looks for a Chrome on this computer).
    pub managed_chromium: Option<String>,
    /// The configured SearXNG program exists.
    pub searxng_found: bool,
    pub jina_key_file_found: bool,
    pub jev_key_file_found: bool,
    /// Plain statements of what leaves the machine, and what is off and why.
    pub notices: Vec<String>,
    /// Settings take effect when Hermes next starts (always true; stated so the UI says it).
    pub applies_on_restart: bool,
    /// A settings file that could not be read (the defaults are shown instead).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub load_error: Option<String>,
}

fn clean_path(p: Option<String>, what: &str) -> Result<Option<String>, String> {
    match p.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) {
        None => Ok(None),
        Some(s) if Path::new(&s).is_absolute() => Ok(Some(s)),
        Some(_) => Err(format!("{what} must be an absolute path")),
    }
}

/// `https://host[:port]`, lowercased, no path, query, fragment or userinfo.
fn normalize_https_origin(raw: &str) -> Result<String, String> {
    let s = raw.trim();
    let rest = s
        .get(..8)
        .filter(|p| p.eq_ignore_ascii_case("https://"))
        .map(|_| &s[8..])
        .ok_or_else(|| format!("{s:?}: a Jev origin must start with https://"))?;
    let rest = rest.strip_suffix('/').unwrap_or(rest);
    if rest.is_empty()
        || rest
            .chars()
            .any(|c| matches!(c, '/' | '?' | '#' | '@' | '\\') || c.is_whitespace())
    {
        return Err(format!(
            "{s:?}: a Jev origin is scheme and host only (https://host or https://host:port)"
        ));
    }
    Ok(format!("https://{}", rest.to_ascii_lowercase()))
}

/// Check and normalize settings before they are stored.
pub fn validate(s: HermesWebSettings) -> Result<HermesWebSettings, String> {
    let mut origins: Vec<String> = Vec::new();
    for o in &s.jev_origins {
        let n = normalize_https_origin(o)?;
        if !origins.contains(&n) {
            origins.push(n);
        }
    }
    if origins.len() > MAX_JEV_ORIGINS {
        return Err(format!("at most {MAX_JEV_ORIGINS} Jev origins"));
    }
    Ok(HermesWebSettings {
        searxng_path: clean_path(s.searxng_path, "the SearXNG path")?,
        jina_key_file: clean_path(s.jina_key_file, "the Jina key file")?,
        jev_key_file: clean_path(s.jev_key_file, "the TypeSafe key file")?,
        jev_origins: origins,
        ..s
    })
}

/// The stored settings; a missing file is the default, a corrupt one is an error.
pub fn load(dir: &Path) -> Result<HermesWebSettings, String> {
    match std::fs::read_to_string(dir.join(SETTINGS_FILE)) {
        Ok(text) => serde_json::from_str(&text)
            .map_err(|_| "the web settings file is not valid; defaults are in use".to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(HermesWebSettings::default()),
        Err(_) => Err("the web settings file could not be read; defaults are in use".into()),
    }
}

/// Store settings (written to a temp file, then renamed).
pub fn save(dir: &Path, s: &HermesWebSettings) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|_| "cannot create the Hermes data folder".to_string())?;
    let text = serde_json::to_string_pretty(s).map_err(|e| e.to_string())?;
    let tmp = dir.join(format!("{SETTINGS_FILE}.tmp"));
    std::fs::write(&tmp, text).map_err(|_| "cannot write the web settings".to_string())?;
    std::fs::rename(&tmp, dir.join(SETTINGS_FILE))
        .map_err(|_| "cannot write the web settings".to_string())
}

/// The sidecar environment for these settings, with no managed Chromium installed. The default
/// settings produce nothing.
#[cfg(test)]
pub fn sidecar_env(s: &HermesWebSettings, hermes_dir: &Path) -> Vec<(String, String)> {
    sidecar_env_with(s, hermes_dir, None)
}

/// The sidecar environment for these settings; `managed_chromium` is the installed managed
/// Chromium's executable, passed only while the browser switch is on.
pub fn sidecar_env_with(
    s: &HermesWebSettings,
    hermes_dir: &Path,
    managed_chromium: Option<&Path>,
) -> Vec<(String, String)> {
    let mut env = Vec::new();
    let mut put = |k: &str, v: String| env.push((k.to_string(), v));
    if s.browser_enabled {
        put("CITRATE_HERMES_BROWSER", "1".into());
        if let Some(p) = managed_chromium.filter(|p| p.is_absolute()) {
            put("CITRATE_BROWSER_CHROMIUM", p.to_string_lossy().into_owned());
        }
    }
    if s.search_enabled {
        put("CITRATE_HERMES_SEARCH", "1".into());
        if let Some(p) = &s.searxng_path {
            put("CITRATE_HERMES_SEARXNG", p.clone());
        }
        put(
            "CITRATE_HERMES_SEARXNG_DATA",
            hermes_dir.join("searxng").to_string_lossy().into_owned(),
        );
        if s.reader == ReaderChoice::Jina {
            put("CITRATE_HERMES_READER", "jina".into());
            if let Some(k) = &s.jina_key_file {
                put("CITRATE_HERMES_JINA_KEY_FILE", k.clone());
            }
        }
    }
    if s.jev_enabled {
        put("CITRATE_HERMES_JEV", "1".into());
        if let Some(k) = &s.jev_key_file {
            put("CITRATE_HERMES_JEV_KEY_FILE", k.clone());
        }
        if !s.jev_origins.is_empty() {
            put("CITRATE_HERMES_JEV_ORIGINS", s.jev_origins.join(","));
        }
        if s.jev_non_web {
            put("CITRATE_HERMES_JEV_NON_WEB", "1".into());
        }
    }
    // The content-free decision log is on only with search or Jev; its retention and rotation
    // are pending owner sign-off.
    if s.search_enabled || s.jev_enabled {
        put(
            "CITRATE_HERMES_DECIDE_LOG",
            hermes_dir
                .join("decisions.jsonl")
                .to_string_lossy()
                .into_owned(),
        );
    }
    env
}

fn file_exists(p: &Option<String>) -> bool {
    p.as_deref()
        .map(|p| Path::new(p).is_file())
        .unwrap_or(false)
}

/// The status the Settings card renders, with no managed Chromium installed.
#[cfg(test)]
pub fn status_for(s: HermesWebSettings, load_error: Option<String>) -> HermesWebStatus {
    status_with(s, load_error, None)
}

/// The status the Settings card renders.
pub fn status_with(
    s: HermesWebSettings,
    load_error: Option<String>,
    managed_chromium: Option<&Path>,
) -> HermesWebStatus {
    let mut notices = Vec::new();
    if s.browser_enabled {
        notices.push(match managed_chromium {
            Some(_) => "Hermes's browser is on. It uses the managed Chromium with a fresh private profile each time, never your own browser profile.".to_string(),
            None => "Hermes's browser is on. The managed Chromium is not installed yet (it comes with the signed component updater), so Hermes uses a Chrome already on this computer with a fresh private profile, or says it is not installed.".to_string(),
        });
        notices.push(
            "Pages Hermes opens are treated as untrusted: after it reads one, every click, entry or new address needs your approval. Attaching to your own Chrome is asked for each session in the Browser pop-out.".to_string(),
        );
    }
    let searxng_found = file_exists(&s.searxng_path)
        || s.searxng_path
            .as_deref()
            .map(|p| Path::new(p).join("bin").join("searxng-run").is_file())
            .unwrap_or(false);
    if s.search_enabled {
        if !searxng_found {
            notices.push(
                "Web search is on, but no SearXNG program was found at the configured path, so web_search will say it is not installed. Reading a known URL still works.".to_string(),
            );
        }
        notices.push(
            "SearXNG runs on this machine and forwards each query to the search engines it is configured for.".to_string(),
        );
        if s.reader == ReaderChoice::Jina {
            notices.push(
                "Pages are read by the Jina Reader, a third-party service: every URL Hermes reads is sent to r.jina.ai.".to_string(),
            );
        }
    }
    let jev_key_file_found = file_exists(&s.jev_key_file);
    if s.jev_enabled {
        if jev_key_file_found {
            notices.push(format!(
                "Fast decisions for {} origin(s){} are sent to TypeSafe (Jev), a third-party service, with the page snapshot. Never for a site with a session cookie or in attach mode.",
                s.jev_origins.len(),
                if s.jev_non_web { " and for non-web choices" } else { "" }
            ));
        } else {
            notices.push(
                "Jev (TypeSafe) is on, but no key file was found, so it stays off and every decision stays local.".to_string(),
            );
        }
    }
    HermesWebStatus {
        managed_chromium: managed_chromium.map(|p| p.to_string_lossy().into_owned()),
        searxng_found,
        jina_key_file_found: file_exists(&s.jina_key_file),
        jev_key_file_found,
        notices,
        applies_on_restart: true,
        load_error,
        settings: s,
    }
}

/// An env source over a settings getter, with no managed Chromium (tests use a closure).
#[cfg(test)]
pub fn env_source_from(
    get: impl Fn() -> HermesWebSettings + Send + Sync + 'static,
    hermes_dir: PathBuf,
) -> EnvSource {
    Arc::new(move || sidecar_env(&get(), &hermes_dir))
}

/// The production env source: the stored settings at each start (a corrupt file = defaults),
/// and the managed Chromium as installed in `components_root` at that moment.
pub fn file_env_source(hermes_dir: PathBuf, components_root: Option<PathBuf>) -> EnvSource {
    Arc::new(move || {
        let s = load(&hermes_dir).unwrap_or_default();
        let managed = match (&components_root, s.browser_enabled) {
            (Some(root), true) => crate::components::managed_chromium(root),
            _ => None,
        };
        sidecar_env_with(&s, &hermes_dir, managed.as_deref())
    })
}

fn hermes_dir<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Result<PathBuf, String> {
    use tauri::Manager;
    app.path()
        .app_local_data_dir()
        .map(|d| d.join("hermes"))
        .map_err(|e| e.to_string())
}

/// **hermes_web_settings_get**: the member's web opt-ins and what they mean.
#[tauri::command]
pub async fn hermes_web_settings_get(app: tauri::AppHandle) -> Result<HermesWebStatus, String> {
    crate::blocking::off_main(move || {
        let dir = hermes_dir(&app)?;
        let managed = crate::components::components_root(&app)
            .ok()
            .and_then(|r| crate::components::managed_chromium(&r));
        Ok(match load(&dir) {
            Ok(s) => status_with(s, None, managed.as_deref()),
            Err(e) => status_with(HermesWebSettings::default(), Some(e), managed.as_deref()),
        })
    })
    .await
}

/// **hermes_web_settings_set**: validate and store the member's web opt-ins. They apply the next
/// time Hermes starts.
#[tauri::command]
pub async fn hermes_web_settings_set(
    app: tauri::AppHandle,
    settings: HermesWebSettings,
) -> Result<HermesWebStatus, String> {
    crate::blocking::off_main(move || {
        let dir = hermes_dir(&app)?;
        let s = validate(settings)?;
        save(&dir, &s)?;
        let managed = crate::components::components_root(&app)
            .ok()
            .and_then(|r| crate::components::managed_chromium(&r));
        Ok(status_with(s, None, managed.as_deref()))
    })
    .await
}

#[cfg(test)]
mod tests {
    include!("hermes_web_tests.rs");
}
