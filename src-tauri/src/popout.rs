//! HUP-S5.4 — the pop-out window framework (D-36).
//!
//! A pop-out is a separate Tauri window that renders one view of the app (the Activity monitor
//! and the Browser today; Contract reader, Code and diff, and Media player later). The rules it
//! keeps:
//!
//! - **Closed allowlist.** Only the five kinds in [`PopoutKind`] exist, each with one fixed label
//!   (`popout-<kind>`). Only kinds with a view ([`PopoutKind::available`]) can be opened; the
//!   others are refused with an honest "not built yet".
//! - **Only the main window opens them** ([`check_open_request`]).
//! - **Least privilege.** Pop-out windows are covered by `capabilities/popout.json`, which grants
//!   the bridge events and nothing else: no app commands (the app-command allowlist in
//!   `permissions/main-window.toml` goes to the main window only), no shell, no fs, no opener.
//!   They may only show the app's own pages ([`navigation_allowed`]).
//! - **One per kind.** Opening a kind that is already open focuses it.
//! - **Size and position persist** per kind (`popouts.json` in the app config dir), and a saved
//!   spot that is no longer on any screen is dropped so the window never opens out of reach.
//! - **Closing never kills work.** A pop-out's close only saves its geometry. The work it shows
//!   lives in the main window and the sidecar. When the main window goes away, the pop-outs close
//!   with it, so quitting the app behaves exactly as before.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};

/// The main window's label (tauri.conf.json leaves it at Tauri's default).
pub(crate) const MAIN_LABEL: &str = "main";
/// Every pop-out label is this prefix plus the kind.
pub(crate) const LABEL_PREFIX: &str = "popout-";
/// The app permission (permissions/main-window.toml) that allows the app's commands. Only the
/// main window's capability holds it (enforced by the capability tests).
#[cfg(test)]
pub(crate) const MAIN_WINDOW_COMMANDS: &str = "main-window-commands";
/// No pop-out is ever larger than this (logical px) in either dimension.
pub(crate) const MAX_DIMENSION: f64 = 8192.0;
/// A saved coordinate further out than this (logical px) is treated as corrupt.
const MAX_COORDINATE: f64 = 100_000.0;
/// How much of the title bar must be on a screen for a saved position to be kept.
const MIN_VISIBLE_W: f64 = 80.0;
const MIN_VISIBLE_H: f64 = 24.0;
/// The geometry store, in the app config dir.
const STORE_FILE: &str = "popouts.json";
/// The Vite dev server (tauri.conf.json `build.devUrl`), allowed only in debug builds.
const DEV_ORIGIN: (&str, &str, u16) = ("http", "localhost", 1420);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum PopoutKind {
    Browser,
    Contract,
    Monitor,
    Diff,
    Media,
}

impl PopoutKind {
    pub(crate) const ALL: [PopoutKind; 5] = [
        PopoutKind::Browser,
        PopoutKind::Contract,
        PopoutKind::Monitor,
        PopoutKind::Diff,
        PopoutKind::Media,
    ];

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            PopoutKind::Browser => "browser",
            PopoutKind::Contract => "contract",
            PopoutKind::Monitor => "monitor",
            PopoutKind::Diff => "diff",
            PopoutKind::Media => "media",
        }
    }

    pub(crate) fn parse(s: &str) -> Result<Self, String> {
        Self::ALL
            .into_iter()
            .find(|k| k.as_str() == s)
            .ok_or_else(|| format!("{s:?} is not a pop-out"))
    }

    pub(crate) fn label(self) -> String {
        format!("{LABEL_PREFIX}{}", self.as_str())
    }

    pub(crate) fn from_label(label: &str) -> Option<Self> {
        let kind = label.strip_prefix(LABEL_PREFIX)?;
        Self::parse(kind).ok()
    }

    pub(crate) fn title(self) -> &'static str {
        match self {
            PopoutKind::Browser => "Browser · Citrate",
            PopoutKind::Contract => "Contract reader · Citrate",
            PopoutKind::Monitor => "Activity monitor · Citrate",
            PopoutKind::Diff => "Code and diff · Citrate",
            PopoutKind::Media => "Media player · Citrate",
        }
    }

    /// Whether this kind has a view yet: the Activity monitor (S7.6) and the Browser (S5.1). The
    /// others ship in later work packages (S6.7 contract reader, S10.1 media, the diff view);
    /// until then they are refused, never opened empty.
    pub(crate) fn available(self) -> bool {
        matches!(self, PopoutKind::Monitor | PopoutKind::Browser)
    }

    /// The first-open size (logical px).
    pub(crate) fn default_size(self) -> (f64, f64) {
        match self {
            PopoutKind::Monitor => (420.0, 640.0),
            PopoutKind::Browser | PopoutKind::Diff => (1000.0, 720.0),
            PopoutKind::Contract => (760.0, 720.0),
            PopoutKind::Media => (720.0, 480.0),
        }
    }

    pub(crate) fn min_size(self) -> (f64, f64) {
        match self {
            PopoutKind::Monitor => (320.0, 360.0),
            _ => (480.0, 360.0),
        }
    }
}

/// The guard in front of window creation: only the main window may open a pop-out, only an
/// allowlisted kind, and only one that has a view.
pub(crate) fn check_open_request(caller_label: &str, kind: &str) -> Result<PopoutKind, String> {
    if caller_label != MAIN_LABEL {
        return Err("only the main window can open a pop-out".to_string());
    }
    let kind = PopoutKind::parse(kind)?;
    if !kind.available() {
        return Err(format!("the {} pop-out is not built yet", kind.as_str()));
    }
    Ok(kind)
}

// ---------------------------------------------------------------------------
// Geometry
// ---------------------------------------------------------------------------

/// A pop-out's saved size and position, in logical px. No position means "let the OS centre it".
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Geometry {
    pub width: f64,
    pub height: f64,
    #[serde(default)]
    pub x: Option<f64>,
    #[serde(default)]
    pub y: Option<f64>,
}

/// A screen's area in logical px.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// Clamp a saved geometry to sane bounds for `kind`. A non-finite size drops the whole entry; a
/// position that is non-finite or absurdly far away is dropped (the window centres instead).
pub(crate) fn sanitize(g: Geometry, kind: PopoutKind) -> Option<Geometry> {
    if !g.width.is_finite() || !g.height.is_finite() {
        return None;
    }
    let (min_w, min_h) = kind.min_size();
    let width = g.width.clamp(min_w, MAX_DIMENSION);
    let height = g.height.clamp(min_h, MAX_DIMENSION);
    let sane = |v: Option<f64>| v.filter(|c| c.is_finite() && c.abs() <= MAX_COORDINATE);
    let (x, y) = match (sane(g.x), sane(g.y)) {
        (Some(x), Some(y)) => (Some(x), Some(y)),
        _ => (None, None),
    };
    Some(Geometry {
        width,
        height,
        x,
        y,
    })
}

/// Keep a saved position only if enough of the window's title bar lands on one of `screens`.
/// With no screen information the position is dropped (the OS places the window).
pub(crate) fn placement(g: Geometry, screens: &[Rect]) -> Geometry {
    let (Some(x), Some(y)) = (g.x, g.y) else {
        return Geometry {
            x: None,
            y: None,
            ..g
        };
    };
    let bar = Rect {
        x,
        y,
        w: g.width,
        h: MIN_VISIBLE_H,
    };
    let visible = screens.iter().any(|s| {
        let w = (bar.x + bar.w).min(s.x + s.w) - bar.x.max(s.x);
        let h = (bar.y + bar.h).min(s.y + s.h) - bar.y.max(s.y);
        w >= MIN_VISIBLE_W && h >= MIN_VISIBLE_H
    });
    if visible {
        g
    } else {
        Geometry {
            x: None,
            y: None,
            ..g
        }
    }
}

/// Parse the geometry store. Anything unreadable is skipped (an empty store means defaults).
pub(crate) fn parse_store(text: &str) -> BTreeMap<PopoutKind, Geometry> {
    let Ok(serde_json::Value::Object(map)) = serde_json::from_str::<serde_json::Value>(text) else {
        return BTreeMap::new();
    };
    map.into_iter()
        .filter_map(|(k, v)| {
            let kind = PopoutKind::parse(&k).ok()?;
            let g: Geometry = serde_json::from_value(v).ok()?;
            Some((kind, g))
        })
        .collect()
}

pub(crate) fn render_store(map: &BTreeMap<PopoutKind, Geometry>) -> String {
    let obj: serde_json::Map<String, serde_json::Value> = map
        .iter()
        .filter_map(|(k, g)| Some((k.as_str().to_string(), serde_json::to_value(g).ok()?)))
        .collect();
    serde_json::Value::Object(obj).to_string()
}

/// The store text after saving `g` for `kind` (other kinds kept).
pub(crate) fn with_saved(text: &str, kind: PopoutKind, g: Geometry) -> String {
    let mut map = parse_store(text);
    map.insert(kind, g);
    render_store(&map)
}

// ---------------------------------------------------------------------------
// Navigation
// ---------------------------------------------------------------------------

/// A pop-out only ever shows the app's own pages: `tauri://localhost` (macOS/Linux),
/// `http(s)://tauri.localhost` (Windows), and the Vite dev server in debug builds.
pub(crate) fn navigation_allowed(url: &url::Url, dev: bool) -> bool {
    let host = url.host_str().unwrap_or_default();
    match url.scheme() {
        "tauri" => host == "localhost",
        "http" | "https" if host == "tauri.localhost" => true,
        scheme => {
            dev && scheme == DEV_ORIGIN.0
                && host == DEV_ORIGIN.1
                && url.port_or_known_default() == Some(DEV_ORIGIN.2)
        }
    }
}

// ---------------------------------------------------------------------------
// Windows (needs the running app; everything it decides is tested above)
// ---------------------------------------------------------------------------

/// Serialises opens so two quick clicks cannot race to create the same window.
static OPEN_LOCK: Mutex<()> = Mutex::new(());
/// The main-window hook that closes pop-outs with it is registered once.
static MAIN_HOOKED: AtomicBool = AtomicBool::new(false);

fn store_path(app: &tauri::AppHandle) -> Option<std::path::PathBuf> {
    app.path().app_config_dir().ok().map(|d| d.join(STORE_FILE))
}

fn read_store(app: &tauri::AppHandle) -> BTreeMap<PopoutKind, Geometry> {
    store_path(app)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|t| parse_store(&t))
        .unwrap_or_default()
}

/// Save one pop-out's geometry. Best effort: a failed write only loses the remembered spot.
fn save_geometry(app: &tauri::AppHandle, kind: PopoutKind, g: Geometry) {
    let Some(path) = store_path(app) else {
        return;
    };
    let current = std::fs::read_to_string(&path).unwrap_or_default();
    let next = with_saved(&current, kind, g);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Err(e) = std::fs::write(&path, next) {
        eprintln!(
            "[popout] could not save the {} window geometry: {e}",
            kind.as_str()
        );
    }
}

/// The window's current geometry in logical px.
fn current_geometry(w: &tauri::WebviewWindow) -> Option<Geometry> {
    let scale = w.scale_factor().ok().filter(|s| *s > 0.0)?;
    let size = w.inner_size().ok()?;
    let pos = w.outer_position().ok();
    Some(Geometry {
        width: f64::from(size.width) / scale,
        height: f64::from(size.height) / scale,
        x: pos.map(|p| f64::from(p.x) / scale),
        y: pos.map(|p| f64::from(p.y) / scale),
    })
}

fn screens(app: &tauri::AppHandle) -> Vec<Rect> {
    app.available_monitors()
        .unwrap_or_default()
        .iter()
        .filter_map(|m| {
            let s = m.scale_factor();
            (s > 0.0).then(|| Rect {
                x: f64::from(m.position().x) / s,
                y: f64::from(m.position().y) / s,
                w: f64::from(m.size().width) / s,
                h: f64::from(m.size().height) / s,
            })
        })
        .collect()
}

/// When the main window goes away, close the pop-outs too (each saves its geometry as it closes),
/// so the app quits exactly as it did before pop-outs existed.
fn hook_main_window(app: &tauri::AppHandle) {
    if MAIN_HOOKED.swap(true, Ordering::SeqCst) {
        return;
    }
    let Some(main) = app.get_webview_window(MAIN_LABEL) else {
        MAIN_HOOKED.store(false, Ordering::SeqCst);
        return;
    };
    let app_h = app.clone();
    main.on_window_event(move |event| {
        if matches!(event, WindowEvent::Destroyed) {
            for (label, w) in app_h.webview_windows() {
                if PopoutKind::from_label(&label).is_some() {
                    let _ = w.close();
                }
            }
        }
    });
}

/// Open (or focus) the pop-out of `kind`. Runs on the blocking pool; the window itself is built
/// on the main thread (macOS requires it), as `oidc::open_auth_popup` does.
fn open_sync(app: tauri::AppHandle, kind: PopoutKind) -> Result<String, String> {
    let _guard = OPEN_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let label = kind.label();
    let saved = read_store(&app).get(&kind).copied();
    let monitors = screens(&app);
    let (tx, rx) = std::sync::mpsc::channel::<Result<&'static str, String>>();
    let app_main = app.clone();
    app.run_on_main_thread(move || {
        let outcome = (|| {
            if let Some(existing) = app_main.get_webview_window(&label) {
                let _ = existing.unminimize();
                let _ = existing.show();
                existing.set_focus().map_err(|e| e.to_string())?;
                return Ok("focused");
            }
            let (dw, dh) = kind.default_size();
            let (mw, mh) = kind.min_size();
            let geometry = saved
                .and_then(|g| sanitize(g, kind))
                .map(|g| placement(g, &monitors))
                .unwrap_or(Geometry {
                    width: dw,
                    height: dh,
                    x: None,
                    y: None,
                });
            let dev = cfg!(debug_assertions);
            let mut builder =
                WebviewWindowBuilder::new(&app_main, &label, WebviewUrl::App("index.html".into()))
                    .title(kind.title())
                    .inner_size(geometry.width, geometry.height)
                    .min_inner_size(mw, mh)
                    .focused(true)
                    .on_navigation(move |url| navigation_allowed(url, dev));
            builder = match (geometry.x, geometry.y) {
                (Some(x), Some(y)) => builder.position(x, y),
                _ => builder.center(),
            };
            let window = builder.build().map_err(|e| e.to_string())?;
            // Closing a pop-out saves where it was and nothing else: no work stops.
            let app_close = app_main.clone();
            let w = window.clone();
            window.on_window_event(move |event| {
                if matches!(event, WindowEvent::CloseRequested { .. }) {
                    if let Some(g) = current_geometry(&w) {
                        save_geometry(&app_close, kind, g);
                    }
                }
            });
            hook_main_window(&app_main);
            Ok("opened")
        })();
        let _ = tx.send(outcome);
    })
    .map_err(|e| e.to_string())?;
    let outcome = rx
        .recv()
        .map_err(|_| "the window could not be created".to_string())??;
    Ok(outcome.to_string())
}

/// **popout_open** — open (or focus) a pop-out window. Only the main window may call it, only
/// for an allowlisted kind with a view. Returns `"opened"` or `"focused"`.
#[tauri::command]
pub async fn popout_open(
    app: tauri::AppHandle,
    webview_window: tauri::WebviewWindow,
    kind: String,
) -> std::result::Result<String, String> {
    let kind = check_open_request(webview_window.label(), &kind)?;
    crate::blocking::off_main(move || open_sync(app, kind)).await
}

/// What the Activity monitor reads from Rust.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorFacts {
    /// The local llama-server's context window (`--ctx-size`), the value serve.rs launches with.
    pub local_ctx_tokens: u32,
}

/// **popout_monitor_facts** — the facts the Activity monitor cannot read from the webview.
#[tauri::command]
pub async fn popout_monitor_facts() -> std::result::Result<MonitorFacts, String> {
    crate::blocking::off_main(|| {
        Ok(MonitorFacts {
            local_ctx_tokens: crate::serve::DEFAULT_CTX_SIZE,
        })
    })
    .await
}

#[cfg(test)]
mod tests {
    include!("popout_tests.rs");
}
