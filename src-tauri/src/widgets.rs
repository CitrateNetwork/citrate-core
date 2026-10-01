//! citrate-core — HUP-S10.3 widgets: small Hermes- or member-authored tiles on the Hermes home,
//! each running in a sandbox that can only ask for declared, read-only data (planset
//! 02_ARCHITECTURE §1 "Widget", §10 D-35; US-10.3 AC1).
//!
//! A widget is HTML/JS plus a list of the catalog queries it may ask for ([`WIDGET_QUERIES`]). It
//! never gets an app command:
//!
//! - **Its own document.** The main window frames `citrate-widget://localhost/<id>` (Windows:
//!   `http://citrate-widget.localhost/<id>`), served by [`serve`] from the widget store. Only the
//!   main window may load it; the document is NOT a local app page, so no capability (and so no app
//!   command) applies to it.
//! - **A strict CSP on that document** ([`widget_csp`]): `connect-src 'none'` (no fetch, XHR,
//!   WebSocket or `ipc://`), no remote script, style, image or frame, no forms, `sandbox
//!   allow-scripts` (an opaque origin even if it were loaded outside the iframe) and
//!   `frame-ancestors` limited to the app itself.
//! - **A sandboxed iframe** (`sandbox="allow-scripts"`, no same-origin, no popups, no top
//!   navigation, no forms), see `src/widgets/WidgetFrame.tsx`.
//! - **A narrow bridge.** The document's only channel is `postMessage` to its parent. The SDK
//!   ([`SDK_JS`]) offers `citrate.query(name)`; the main window answers a query only when the name
//!   is in this widget's declared list AND in the catalog, from state the app already holds
//!   (`src/widgets/host.ts`). Nothing in the catalog writes, signs or sends.
//!
//! The widget store is one JSON file per widget in `<app local data>/widgets/`.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::Manager;

/// The URI scheme widget documents are served on.
pub(crate) const SCHEME: &str = "citrate-widget";

/// The read-only queries a widget may declare (kept in step with `src/widgets/catalog.ts`).
pub(crate) const WIDGET_QUERIES: [&str; 4] = [
    "node.status",
    "wallet.summary",
    "model.active",
    "daemons.summary",
];

/// The commands this module registers (kept in step with lib.rs and main-window.toml by a test).
#[cfg(test)]
pub(crate) const COMMANDS: [&str; 4] = [
    "widgets_list",
    "widget_save",
    "widget_delete",
    "widget_source",
];

/// At most this many widgets.
pub(crate) const MAX_WIDGETS: usize = 24;
/// Largest widget source, bytes.
pub(crate) const HTML_MAX: usize = 64 * 1024;
/// Longest name, characters.
pub(crate) const NAME_MAX: usize = 60;
/// Longest description, characters.
pub(crate) const DESCRIPTION_MAX: usize = 200;
const DIR: &str = "widgets";
const AUTHORS: [&str; 3] = ["member", "hermes", "gallery"];

/// The Vite dev server, which hosts the main window in a debug build.
const DEV_ORIGIN: &str = "http://localhost:1420";

/// A stored widget.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WidgetSpec {
    pub id: String,
    pub name: String,
    pub description: String,
    pub html: String,
    pub queries: Vec<String>,
    /// "member" | "hermes" | "gallery": who wrote it (shown in the gallery).
    pub author: String,
    pub created_ms: u64,
}

/// What the gallery lists (no source).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WidgetMeta {
    pub id: String,
    pub name: String,
    pub description: String,
    pub queries: Vec<String>,
    pub author: String,
    pub created_ms: u64,
    pub bytes: usize,
}

impl From<&WidgetSpec> for WidgetMeta {
    fn from(w: &WidgetSpec) -> Self {
        WidgetMeta {
            id: w.id.clone(),
            name: w.name.clone(),
            description: w.description.clone(),
            queries: w.queries.clone(),
            author: w.author.clone(),
            created_ms: w.created_ms,
            bytes: w.html.len(),
        }
    }
}

/// A create (no id) or edit request.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WidgetInput {
    #[serde(default)]
    pub id: Option<String>,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub html: String,
    #[serde(default)]
    pub queries: Vec<String>,
    pub author: String,
}

/// A widget id: 16 lowercase hex characters (at most 32 accepted on input).
pub(crate) fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 32
        && id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn new_id() -> String {
    use rand::RngCore;
    let mut b = [0u8; 8];
    rand::thread_rng().fill_bytes(&mut b);
    hex::encode(b)
}

fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Check a request and build the spec (a fresh id unless editing).
pub(crate) fn validate(input: WidgetInput, now_ms: u64) -> Result<WidgetSpec, String> {
    let name = one_line(&input.name);
    if name.is_empty() || name.chars().count() > NAME_MAX {
        return Err(format!(
            "a widget needs a name of 1 to {NAME_MAX} characters"
        ));
    }
    let description = one_line(&input.description);
    if description.chars().count() > DESCRIPTION_MAX {
        return Err(format!(
            "the description is longer than {DESCRIPTION_MAX} characters"
        ));
    }
    if input.html.trim().is_empty() || input.html.len() > HTML_MAX {
        return Err(format!("a widget's source must be 1 to {HTML_MAX} bytes"));
    }
    if input.html.contains('\0') {
        return Err("a widget's source may not contain NUL".into());
    }
    if !AUTHORS.contains(&input.author.as_str()) {
        return Err("unknown author".into());
    }
    let mut seen = std::collections::BTreeSet::new();
    for q in &input.queries {
        if !WIDGET_QUERIES.contains(&q.as_str()) {
            return Err(format!(
                "{q:?} is not a query widgets can ask for (they can ask for: {})",
                WIDGET_QUERIES.join(", ")
            ));
        }
        if !seen.insert(q.as_str()) {
            return Err(format!("{q:?} is declared twice"));
        }
    }
    let id = match input.id {
        Some(id) if valid_id(&id) => id,
        Some(_) => return Err("bad widget id".into()),
        None => new_id(),
    };
    Ok(WidgetSpec {
        id,
        name,
        description,
        html: input.html,
        queries: input.queries,
        author: input.author,
        created_ms: now_ms,
    })
}

// ---------------------------------------------------------------------------
// The sandbox document
// ---------------------------------------------------------------------------

/// The bridge SDK every widget document starts with. It talks only to `window.parent`, answers
/// only messages from it, and exposes a frozen, non-replaceable `window.citrate`.
pub(crate) const SDK_JS: &str = r#"(function () {
  "use strict";
  var declared = __DECLARED__;
  var pending = new Map();
  var next = 0;
  window.addEventListener("message", function (e) {
    if (e.source !== window.parent) return;
    var d = e.data;
    if (!d || d.v !== 1 || d.type !== "widget.result" || typeof d.id !== "number") return;
    var p = pending.get(d.id);
    if (!p) return;
    pending.delete(d.id);
    if (d.ok === true) p.resolve(d.data);
    else p.reject(new Error(typeof d.error === "string" ? d.error : "refused"));
  });
  function query(name) {
    return new Promise(function (resolve, reject) {
      var id = ++next;
      pending.set(id, { resolve: resolve, reject: reject });
      window.parent.postMessage({ v: 1, type: "widget.query", id: id, query: String(name) }, "*");
      setTimeout(function () {
        if (pending.delete(id)) reject(new Error("no answer"));
      }, 5000);
    });
  }
  Object.defineProperty(window, "citrate", {
    value: Object.freeze({ queries: Object.freeze(declared.slice()), query: query }),
    writable: false,
    configurable: false,
    enumerable: true
  });
})();"#;

const BASE_CSS: &str = "html,body{margin:0;padding:0}body{font:13px/1.4 -apple-system,BlinkMacSystemFont,'Segoe UI',sans-serif;color:#e8e6e1;background:transparent;padding:10px;box-sizing:border-box;overflow:hidden}@media (prefers-color-scheme: light){body{color:#1d1b18}}";

/// The widget document's CSP. `dev` adds the Vite dev server to `frame-ancestors`.
pub(crate) fn widget_csp(dev: bool) -> String {
    let mut ancestors =
        String::from("tauri://localhost http://tauri.localhost https://tauri.localhost");
    if dev {
        ancestors.push(' ');
        ancestors.push_str(DEV_ORIGIN);
    }
    format!(
        "default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src data:; \
         font-src data:; connect-src 'none'; media-src 'none'; object-src 'none'; frame-src 'none'; \
         child-src 'none'; worker-src 'none'; manifest-src 'none'; form-action 'none'; \
         base-uri 'none'; sandbox allow-scripts; frame-ancestors {ancestors}"
    )
}

/// The full document for a widget: the SDK (with its declared queries), then the widget's markup.
pub(crate) fn document(w: &WidgetSpec) -> String {
    // The declared list holds catalog names only (validated), so its JSON is safe inside a script.
    let declared = serde_json::to_string(&w.queries).unwrap_or_else(|_| "[]".into());
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"referrer\" content=\"no-referrer\">\
         <style>{BASE_CSS}</style><script>{}</script></head><body>{}</body></html>",
        SDK_JS.replace("__DECLARED__", &declared),
        w.html
    )
}

/// A response for the scheme handler (kept free of Tauri types so it is testable).
#[derive(Debug, Clone)]
pub(crate) struct WidgetResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

fn plain(status: u16, msg: &str) -> WidgetResponse {
    WidgetResponse {
        status,
        headers: vec![
            ("Content-Type".into(), "text/plain; charset=utf-8".into()),
            ("Cache-Control".into(), "no-store".into()),
            ("X-Content-Type-Options".into(), "nosniff".into()),
        ],
        body: msg.as_bytes().to_vec(),
    }
}

/// Serve `path` (`/<id>`) to the webview labelled `label`. Only the main window may load a widget.
pub(crate) fn serve(
    method: &str,
    path: &str,
    label: &str,
    dev: bool,
    lookup: impl Fn(&str) -> Option<WidgetSpec>,
) -> WidgetResponse {
    if label != crate::popout::MAIN_LABEL {
        return plain(403, "widgets load only in the main window");
    }
    if method != "GET" {
        return plain(405, "method not allowed");
    }
    let Some(id) = path.strip_prefix('/') else {
        return plain(404, "no such widget");
    };
    if !valid_id(id) {
        return plain(404, "no such widget");
    }
    let Some(w) = lookup(id) else {
        return plain(404, "no such widget");
    };
    WidgetResponse {
        status: 200,
        headers: vec![
            ("Content-Type".into(), "text/html; charset=utf-8".into()),
            ("Content-Security-Policy".into(), widget_csp(dev)),
            ("X-Content-Type-Options".into(), "nosniff".into()),
            ("Cache-Control".into(), "no-store".into()),
            ("Referrer-Policy".into(), "no-referrer".into()),
        ],
        body: document(&w).into_bytes(),
    }
}

/// The scheme handler body: resolve the request against the store on disk.
pub(crate) fn respond(
    app: &tauri::AppHandle,
    label: &str,
    request: &tauri::http::Request<Vec<u8>>,
) -> tauri::http::Response<Vec<u8>> {
    let dir = widgets_dir(app);
    // A query string is not part of any widget address.
    let path = if request.uri().query().is_some() {
        ""
    } else {
        request.uri().path()
    };
    let r = serve(
        request.method().as_str(),
        path,
        label,
        cfg!(debug_assertions),
        |id| dir.as_ref().ok().and_then(|d| read_in(d, id).ok()),
    );
    let mut b = tauri::http::Response::builder().status(r.status);
    for (k, v) in &r.headers {
        b = b.header(k.as_str(), v.as_str());
    }
    b.body(r.body.clone()).unwrap_or_else(|_| {
        let mut resp = tauri::http::Response::new(Vec::new());
        *resp.status_mut() = tauri::http::StatusCode::INTERNAL_SERVER_ERROR;
        resp
    })
}

// ---------------------------------------------------------------------------
// The store
// ---------------------------------------------------------------------------

static STORE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn widgets_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_local_data_dir()
        .map_err(|e| e.to_string())?
        .join(DIR))
}

fn file_in(dir: &Path, id: &str) -> Result<PathBuf, String> {
    if !valid_id(id) {
        return Err("bad widget id".into());
    }
    Ok(dir.join(format!("{id}.json")))
}

/// Every stored widget. A file that does not parse is an error naming it, never skipped.
pub(crate) fn list_in(dir: &Path) -> Result<Vec<WidgetSpec>, String> {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(format!("the widget store could not be read: {e}")),
    };
    let mut out = vec![];
    for e in entries.flatten() {
        let p = e.path();
        if p.extension().and_then(|x| x.to_str()) != Some("json") {
            continue;
        }
        let text = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
        let w: WidgetSpec = serde_json::from_str(&text)
            .map_err(|e| format!("the widget file {} is damaged: {e}", p.display()))?;
        out.push(w);
    }
    out.sort_by(|a, b| a.created_ms.cmp(&b.created_ms).then(a.id.cmp(&b.id)));
    Ok(out)
}

pub(crate) fn read_in(dir: &Path, id: &str) -> Result<WidgetSpec, String> {
    let path = file_in(dir, id)?;
    let text = std::fs::read_to_string(&path).map_err(|_| "no such widget".to_string())?;
    let w: WidgetSpec =
        serde_json::from_str(&text).map_err(|e| format!("the widget file is damaged: {e}"))?;
    if w.id != id {
        return Err("the widget file does not match its name".into());
    }
    Ok(w)
}

pub(crate) fn save_in(dir: &Path, input: WidgetInput, now_ms: u64) -> Result<WidgetSpec, String> {
    let _guard = STORE_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let editing = input.id.clone();
    let mut spec = validate(input, now_ms)?;
    match &editing {
        Some(id) => {
            let old = read_in(dir, id)?;
            spec.created_ms = old.created_ms;
        }
        None => {
            if list_in(dir)?.len() >= MAX_WIDGETS {
                return Err(format!("at most {MAX_WIDGETS} widgets"));
            }
        }
    }
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let path = file_in(dir, &spec.id)?;
    let tmp = dir.join(format!("{}.json.tmp", spec.id));
    let text = serde_json::to_string(&spec).map_err(|e| e.to_string())?;
    std::fs::write(&tmp, text).map_err(|e| format!("could not save the widget: {e}"))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("could not save the widget: {e}"))?;
    Ok(spec)
}

pub(crate) fn delete_in(dir: &Path, id: &str) -> Result<(), String> {
    let _guard = STORE_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let path = file_in(dir, id)?;
    std::fs::remove_file(&path).map_err(|_| "no such widget".to_string())
}

// ---------------------------------------------------------------------------
// Commands (all async, off the main thread)
// ---------------------------------------------------------------------------

/// **widgets_list** — every saved widget (no source).
#[tauri::command]
pub async fn widgets_list(app: tauri::AppHandle) -> Result<Vec<WidgetMeta>, String> {
    crate::blocking::off_main(move || {
        let dir = widgets_dir(&app)?;
        Ok(list_in(&dir)?.iter().map(WidgetMeta::from).collect())
    })
    .await
}

/// **widget_save** — create (no id) or edit a widget.
#[tauri::command]
pub async fn widget_save(
    app: tauri::AppHandle,
    input: WidgetInput,
    now_ms: u64,
) -> Result<WidgetMeta, String> {
    crate::blocking::off_main(move || {
        let dir = widgets_dir(&app)?;
        save_in(&dir, input, now_ms).map(|w| WidgetMeta::from(&w))
    })
    .await
}

/// **widget_delete** — remove a widget.
#[tauri::command]
pub async fn widget_delete(app: tauri::AppHandle, id: String) -> Result<(), String> {
    crate::blocking::off_main(move || delete_in(&widgets_dir(&app)?, &id)).await
}

/// **widget_source** — a widget's source, for "view source" in the gallery (shown as text).
#[tauri::command]
pub async fn widget_source(app: tauri::AppHandle, id: String) -> Result<String, String> {
    crate::blocking::off_main(move || read_in(&widgets_dir(&app)?, &id).map(|w| w.html)).await
}

#[cfg(test)]
mod tests {
    include!("widgets_tests.rs");
}
