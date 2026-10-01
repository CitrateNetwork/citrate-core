// HUP-S10.3 — widget store + sandbox document tests. Included into `widgets.rs` as `mod tests`.
// The iframe itself needs a running webview; everything that decides what a widget document may do
// (its CSP, its sandbox, who may load it, which data it may ask for) is decided here and tested.
use super::*;

fn input(html: &str, queries: &[&str]) -> WidgetInput {
    WidgetInput {
        id: None,
        name: "Block height".into(),
        description: "Shows the node's height".into(),
        html: html.into(),
        queries: queries.iter().map(|q| q.to_string()).collect(),
        author: "member".into(),
    }
}

fn saved(html: &str, queries: &[&str]) -> WidgetSpec {
    validate(input(html, queries), 1_000).expect("valid widget")
}

fn header<'a>(r: &'a WidgetResponse, name: &str) -> Option<&'a str> {
    r.headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

fn directives(csp: &str) -> std::collections::BTreeMap<String, String> {
    csp.split(';')
        .map(str::trim)
        .filter(|d| !d.is_empty())
        .map(|d| match d.split_once(' ') {
            Some((k, v)) => (k.to_string(), v.to_string()),
            None => (d.to_string(), String::new()),
        })
        .collect()
}

// ---------------------------------------------------------------------------
// The query catalog
// ---------------------------------------------------------------------------

#[test]
fn the_catalog_is_read_only_queries_and_matches_typescript() {
    let ts = include_str!("../../src/widgets/catalog.ts");
    let line = ts
        .lines()
        .find(|l| l.contains("export const WIDGET_QUERIES"))
        .expect("WIDGET_QUERIES in catalog.ts");
    let quoted: Vec<&str> = line.split('"').skip(1).step_by(2).collect();
    assert_eq!(
        quoted, WIDGET_QUERIES,
        "catalog.ts and widgets.rs must list the same queries"
    );
    for q in WIDGET_QUERIES {
        // Names say what they read; nothing in the catalog writes, signs or sends.
        for verb in [
            "send", "sign", "write", "set", "deploy", "invoke", "approve", "transfer",
        ] {
            assert!(!q.contains(verb), "{q} looks like a write");
        }
    }
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

#[test]
fn a_widget_may_only_declare_catalog_queries() {
    assert!(validate(input("<p>x</p>", &["node.status"]), 1).is_ok());
    for bad in [
        vec!["node.status", "wallet.send"],
        vec!["invoke"],
        vec!["__TAURI__"],
        vec!["node.status", "node.status"],
        vec![""],
    ] {
        assert!(validate(input("<p>x</p>", &bad), 1).is_err(), "{bad:?}");
    }
    let too_many: Vec<&str> = WIDGET_QUERIES
        .iter()
        .chain(WIDGET_QUERIES.iter())
        .copied()
        .collect();
    assert!(validate(input("<p>x</p>", &too_many), 1).is_err());
}

#[test]
fn names_sizes_and_authors_are_bounded() {
    let mut i = input("<p>x</p>", &[]);
    i.name = " ".into();
    assert!(validate(i, 1).is_err());
    let mut i = input("<p>x</p>", &[]);
    i.name = "n".repeat(NAME_MAX + 1);
    assert!(validate(i, 1).is_err());
    let mut i = input("<p>x</p>", &[]);
    i.description = "d".repeat(DESCRIPTION_MAX + 1);
    assert!(validate(i, 1).is_err());
    assert!(validate(input("", &[]), 1).is_err());
    assert!(validate(input(&"x".repeat(HTML_MAX + 1), &[]), 1).is_err());
    assert!(validate(input(&"x".repeat(HTML_MAX), &[]), 1).is_ok());
    assert!(validate(input("<p>\0</p>", &[]), 1).is_err());
    let mut i = input("<p>x</p>", &[]);
    i.author = "root".into();
    assert!(validate(i, 1).is_err());
    for a in ["member", "hermes", "gallery"] {
        let mut i = input("<p>x</p>", &[]);
        i.author = a.into();
        assert!(validate(i, 1).is_ok(), "{a}");
    }
}

#[test]
fn widget_ids_are_short_lowercase_hex_and_nothing_else() {
    let w = saved("<p>x</p>", &[]);
    assert!(valid_id(&w.id));
    for bad in [
        "",
        "../x",
        "W1",
        "a/b",
        "a.b",
        "a b",
        "%2e%2e",
        &"a".repeat(33),
    ] {
        assert!(!valid_id(bad), "{bad:?}");
    }
}

// ---------------------------------------------------------------------------
// The sandbox document
// ---------------------------------------------------------------------------

fn serve_one(w: &WidgetSpec, method: &str, path: &str, label: &str) -> WidgetResponse {
    let w = w.clone();
    serve(method, path, label, false, move |id| {
        (id == w.id).then(|| w.clone())
    })
}

#[test]
fn the_document_carries_a_csp_that_forbids_every_network_and_ipc_path() {
    let w = saved("<p id=h>…</p>", &["node.status"]);
    let r = serve_one(&w, "GET", &format!("/{}", w.id), "main");
    assert_eq!(r.status, 200);
    let csp = header(&r, "Content-Security-Policy").expect("CSP header");
    let d = directives(csp);
    assert_eq!(d.get("default-src").map(String::as_str), Some("'none'"));
    assert_eq!(
        d.get("connect-src").map(String::as_str),
        Some("'none'"),
        "no fetch, XHR, WebSocket or ipc://"
    );
    assert_eq!(
        d.get("script-src").map(String::as_str),
        Some("'unsafe-inline'"),
        "inline only: no remote script"
    );
    assert_eq!(d.get("img-src").map(String::as_str), Some("data:"));
    for none in [
        "frame-src",
        "child-src",
        "worker-src",
        "object-src",
        "media-src",
        "form-action",
        "base-uri",
        "manifest-src",
    ] {
        assert_eq!(d.get(none).map(String::as_str), Some("'none'"), "{none}");
    }
    // Sandboxed even if something loads it outside the iframe: scripts only, opaque origin.
    assert_eq!(d.get("sandbox").map(String::as_str), Some("allow-scripts"));
    // Only the app may frame it.
    let ancestors = d.get("frame-ancestors").expect("frame-ancestors");
    assert!(ancestors.contains("tauri://localhost"));
    assert!(!ancestors.contains('*'));
    assert!(
        !ancestors.contains("localhost:1420"),
        "no dev server in a release build"
    );
    assert_eq!(header(&r, "X-Content-Type-Options"), Some("nosniff"));
    assert_eq!(header(&r, "Cache-Control"), Some("no-store"));
    assert_eq!(header(&r, "Referrer-Policy"), Some("no-referrer"));
    assert!(header(&r, "Content-Type")
        .unwrap_or("")
        .starts_with("text/html"));
}

#[test]
fn the_dev_server_may_frame_widgets_only_in_dev_builds() {
    let w = saved("<p>x</p>", &[]);
    let w2 = w.clone();
    let r = serve("GET", &format!("/{}", w.id), "main", true, move |id| {
        (id == w2.id).then(|| w2.clone())
    });
    let csp = header(&r, "Content-Security-Policy").expect("csp");
    assert!(directives(csp)["frame-ancestors"].contains("http://localhost:1420"));
}

#[test]
fn the_document_wraps_the_widget_with_the_bridge_and_its_declared_queries() {
    let w = saved("<p id=h>hello widget</p>", &["node.status", "model.active"]);
    let r = serve_one(&w, "GET", &format!("/{}", w.id), "main");
    let body = String::from_utf8(r.body.clone()).expect("utf8");
    assert!(body.starts_with("<!doctype html>"));
    assert!(body.contains("<p id=h>hello widget</p>"));
    assert!(
        body.contains(r#"["node.status","model.active"]"#),
        "declared queries embedded"
    );
    assert!(body.contains("widget.query"));
    // The bridge only talks to its parent and never to Tauri.
    assert!(!body.contains("__TAURI"));
    assert!(!body.contains("invoke"));
    // The SDK is installed before the widget's own markup, and frozen.
    let sdk = body.find("widget.query").expect("sdk");
    let content = body.find("hello widget").expect("content");
    assert!(sdk < content);
    assert!(body.contains("Object.freeze"));
}

#[test]
fn a_widget_that_tries_to_close_the_head_cannot_move_ahead_of_the_bridge() {
    let w = saved("</script></head><script>window.citrate=1</script>", &[]);
    let r = serve_one(&w, "GET", &format!("/{}", w.id), "main");
    let body = String::from_utf8(r.body).expect("utf8");
    // The SDK script closes before the widget markup starts; `citrate` is non-writable and
    // non-configurable, so a later assignment cannot replace it.
    assert!(
        body.find("Object.defineProperty(window, \"citrate\"")
            .expect("sdk")
            < body.find("window.citrate=1").expect("widget")
    );
    assert!(body.contains("writable: false"));
    assert!(body.contains("configurable: false"));
}

#[test]
fn only_the_main_window_may_load_a_widget_document() {
    let w = saved("<p>x</p>", &[]);
    for label in ["popout-monitor", "popout-browser", "", "Main", "main2"] {
        assert_eq!(
            serve_one(&w, "GET", &format!("/{}", w.id), label).status,
            403,
            "{label:?}"
        );
    }
}

#[test]
fn bad_paths_methods_and_unknown_widgets_are_refused() {
    let w = saved("<p>x</p>", &[]);
    assert_eq!(
        serve_one(&w, "POST", &format!("/{}", w.id), "main").status,
        405
    );
    for p in [
        "/".to_string(),
        "".to_string(),
        "/../widgets.json".to_string(),
        format!("/{}/x", w.id),
        format!("/{}?x=1", w.id),
        "/ffffffffffffffff".to_string(),
        "/%2e%2e".to_string(),
    ] {
        let r = serve_one(&w, "GET", &p, "main");
        assert_eq!(r.status, 404, "{p:?}");
        assert!(r.body.len() < 200, "no widget content on an error");
    }
}

// ---------------------------------------------------------------------------
// The store
// ---------------------------------------------------------------------------

#[test]
fn the_store_saves_lists_reads_and_deletes_widgets() {
    let dir = std::env::temp_dir().join(format!("citrate-widgets-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let w = save_in(&dir, input("<p>a</p>", &["node.status"]), 5).expect("save");
    let list = list_in(&dir).expect("list");
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, w.id);
    assert_eq!(list[0].queries, vec!["node.status".to_string()]);
    assert_eq!(read_in(&dir, &w.id).expect("read").html, "<p>a</p>");
    // Editing keeps the id.
    let mut edit = input("<p>b</p>", &[]);
    edit.id = Some(w.id.clone());
    let w2 = save_in(&dir, edit, 6).expect("edit");
    assert_eq!(w2.id, w.id);
    assert_eq!(read_in(&dir, &w.id).expect("read").html, "<p>b</p>");
    // Unknown ids are refused for edit, read and delete; bad ids never touch the disk.
    let mut ghost = input("<p>c</p>", &[]);
    ghost.id = Some("0123456789abcdef".into());
    assert!(save_in(&dir, ghost, 7).is_err());
    assert!(read_in(&dir, "../x").is_err());
    assert!(delete_in(&dir, "../x").is_err());
    delete_in(&dir, &w.id).expect("delete");
    assert!(list_in(&dir).expect("list").is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_store_has_a_cap_and_skips_nothing_silently() {
    let dir = std::env::temp_dir().join(format!("citrate-widgets-cap-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for _ in 0..MAX_WIDGETS {
        save_in(&dir, input("<p>a</p>", &[]), 1).expect("under the cap");
    }
    assert!(save_in(&dir, input("<p>a</p>", &[]), 1).is_err());
    // A damaged file is reported, not dropped.
    std::fs::write(dir.join("0000000000000000.json"), "{").expect("damage");
    assert!(list_in(&dir).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// Registration, ACL, CSP
// ---------------------------------------------------------------------------

#[test]
fn every_widget_command_is_async_registered_and_allowed_for_the_main_window_only() {
    let lib = include_str!("lib.rs");
    let acl = include_str!("../permissions/main-window.toml");
    let src = include_str!("widgets.rs");
    for cmd in COMMANDS {
        assert!(
            lib.contains(&format!("widgets::{cmd},")),
            "{cmd} registered"
        );
        assert!(
            acl.contains(&format!("\"{cmd}\"")),
            "{cmd} in main-window.toml"
        );
        assert!(
            src.contains(&format!("pub async fn {cmd}(")),
            "{cmd} is async"
        );
    }
    assert!(
        lib.contains("register_asynchronous_uri_scheme_protocol(widgets::SCHEME"),
        "the widget scheme is registered"
    );
}

#[test]
fn the_app_csp_frames_only_the_widget_scheme() {
    let conf: serde_json::Value =
        serde_json::from_str(include_str!("../tauri.conf.json")).expect("tauri.conf.json");
    let csp = conf["app"]["security"]["csp"].as_str().expect("csp string");
    let d = directives(csp);
    let frame = d.get("frame-src").expect("frame-src");
    let sources: Vec<&str> = frame.split_whitespace().collect();
    assert_eq!(
        sources,
        [
            format!("{SCHEME}:").as_str(),
            &format!("http://{SCHEME}.localhost"),
            &format!("https://{SCHEME}.localhost"),
        ],
        "only widget documents may be framed"
    );
    // The app's own script policy is untouched.
    assert_eq!(d.get("script-src").map(String::as_str), Some("'self'"));
}
