//! Join-13: deliver deep links to the running app on Linux and Windows; never log URL fragments.
//!
//! On macOS the OS hands a clicked `citrate://` link to the running app (an Apple Event), and
//! `tauri-plugin-deep-link` emits it. On Linux and Windows the OS starts a SECOND process with the
//! link in its argv; `tauri-plugin-single-instance` stops that process and passes its argv to the
//! running one. This module takes that argv, keeps only `citrate://` links, and hands each one to
//! the deep-link plugin's own `handle_cli_arguments`, which emits the same `deep-link://new-url`
//! event the frontend's `onOpenUrl` already listens to. One delivery path for every platform.
//!
//! The single-instance `deep-link` feature is deliberately NOT enabled: it forwards the raw,
//! unfiltered argv (and only when it is exactly one argument), and running it next to this filter
//! would deliver a link twice.
//!
//! Links may carry a value in the URL fragment (after `#`). The app needs it, so delivery keeps it
//! intact; nothing logged here ever contains it ([`redact_fragment`]). Arguments that are not
//! accepted links are counted in the log, never echoed.
//!
//! An AppImage is not installed through a `.desktop` file, so its scheme is never registered.
//! On Linux, when `APPIMAGE` is set, [`register_appimage_schemes`] registers the configured
//! schemes at startup ([`should_register_appimage_schemes`] is the pure guard).

// The pure helpers are called from the Linux/Windows paths only; macOS builds use them in tests
// (the AppImage helpers, Linux only, carry their own allow).
#![cfg_attr(not(any(target_os = "linux", windows)), allow(dead_code))]

use std::borrow::Cow;
use std::ffi::OsStr;

/// The schemes this app accepts from argv. Must equal `plugins.deep-link.desktop.schemes` in
/// tauri.conf.json (a test checks it): the plugin rejects any other scheme anyway.
pub const SCHEMES: &[&str] = &["citrate"];

/// Longest argument considered a link. Longer arguments are dropped unparsed.
pub const MAX_LINK_LEN: usize = 4096;

/// The placeholder binary name passed ahead of each link: the deep-link plugin skips argv[0].
#[cfg(any(target_os = "linux", windows))]
const ARGV0: &str = "citrate-core";

/// `s` with everything after its first `#` replaced by `<redacted>`. Use for every URL that is
/// logged, traced, written to a crash record or put in an error message.
pub fn redact_fragment(s: &str) -> Cow<'_, str> {
    match s.find('#') {
        Some(i) => Cow::Owned(format!("{}#<redacted>", &s[..i])),
        None => Cow::Borrowed(s),
    }
}

/// One argument as a link, if it is an accepted one. Surrounding whitespace and one balanced pair
/// of double quotes (Windows command lines) are removed first.
fn link_from_arg(arg: &str) -> Option<url::Url> {
    let trimmed = arg.trim();
    let unquoted = match trimmed.strip_prefix('"') {
        Some(rest) => rest.strip_suffix('"')?,
        None => trimmed,
    };
    if unquoted.is_empty() || unquoted.len() > MAX_LINK_LEN || unquoted.contains('"') {
        return None;
    }
    let url = url::Url::parse(unquoted).ok()?;
    let scheme_ok = SCHEMES.contains(&url.scheme());
    // `citrate://...` only; `citrate:join/...` has no authority and the app never issues it.
    let has_authority = unquoted
        .get(url.scheme().len()..)
        .is_some_and(|rest| rest.starts_with("://"));
    (scheme_ok && has_authority && !url.cannot_be_a_base()).then_some(url)
}

/// The accepted links in a second instance's argv, in order, exact duplicates removed. argv[0]
/// (the binary) is never a link.
pub fn urls_from_argv<S: AsRef<str>>(argv: &[S]) -> Vec<url::Url> {
    let mut out: Vec<url::Url> = Vec::new();
    for arg in argv.iter().skip(1) {
        if let Some(url) = link_from_arg(arg.as_ref()) {
            if !out.contains(&url) {
                out.push(url);
            }
        }
    }
    out
}

/// How many arguments (after argv[0]) were not delivered as links.
pub fn ignored_count<S: AsRef<str>>(argv: &[S], urls: &[url::Url]) -> usize {
    argv.len().saturating_sub(1).saturating_sub(urls.len())
}

/// The log line for one second-instance hand-off. Fragments are redacted; ignored arguments are
/// only counted.
pub fn delivery_log_line(urls: &[url::Url], ignored: usize) -> String {
    let links: Vec<Cow<'_, str>> = urls.iter().map(|u| redact_fragment(u.as_str())).collect();
    format!(
        "[deep-link] second launch: {} link(s) delivered to the running app [{}], {} ignored",
        urls.len(),
        links.join(", "),
        ignored
    )
}

/// Whether the AppImage scheme registration should run: Linux, with a non-empty `APPIMAGE`.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn should_register_appimage_schemes(is_linux: bool, appimage: Option<&OsStr>) -> bool {
    is_linux && appimage.is_some_and(|v| !v.is_empty())
}

/// The log line for the AppImage registration outcome. Never contains a URL fragment.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn appimage_register_log_line(result: &Result<(), String>) -> String {
    match result {
        Ok(()) => format!(
            "[deep-link] AppImage: registered {} scheme(s) with the desktop",
            SCHEMES.len()
        ),
        Err(e) => format!(
            "[deep-link] AppImage: scheme registration failed: {}",
            redact_fragment(e)
        ),
    }
}

/// Hand the accepted links from a second instance's argv to the deep-link plugin, which emits them
/// to the frontend. Linux and Windows only: on macOS links arrive through the plugin directly.
#[cfg(any(target_os = "linux", windows))]
pub fn deliver_second_instance_argv<R: tauri::Runtime>(app: &tauri::AppHandle<R>, argv: &[String]) {
    use tauri::Manager;
    let urls = urls_from_argv(argv);
    if urls.is_empty() {
        return;
    }
    match app.try_state::<tauri_plugin_deep_link::DeepLink<R>>() {
        Some(deep_link) => {
            for url in &urls {
                deep_link.handle_cli_arguments([ARGV0, url.as_str()].into_iter());
            }
            eprintln!("{}", delivery_log_line(&urls, ignored_count(argv, &urls)));
        }
        None => eprintln!(
            "[deep-link] second launch: deep-link plugin not ready, {} link(s) not delivered",
            urls.len()
        ),
    }
}

/// macOS (and any other target): second-instance argv carries no links to deliver.
#[cfg(not(any(target_os = "linux", windows)))]
pub fn deliver_second_instance_argv<R: tauri::Runtime>(
    _app: &tauri::AppHandle<R>,
    _argv: &[String],
) {
}

/// Register the configured schemes when running as an AppImage (Linux, `APPIMAGE` set). Runs at
/// every AppImage launch: it is idempotent and keeps the handler pointing at the AppImage's current
/// path if the file was moved. Logs the outcome, never a URL.
#[cfg(target_os = "linux")]
pub fn register_appimage_schemes<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    use tauri::Manager;
    let appimage = std::env::var_os("APPIMAGE");
    if !should_register_appimage_schemes(true, appimage.as_deref()) {
        return;
    }
    let result = match app.try_state::<tauri_plugin_deep_link::DeepLink<R>>() {
        Some(deep_link) => deep_link.register_all().map_err(|e| e.to_string()),
        None => Err("deep-link plugin not ready".to_string()),
    };
    eprintln!("{}", appimage_register_log_line(&result));
}

#[cfg(test)]
#[path = "deep_link_delivery_tests.rs"]
mod tests;
