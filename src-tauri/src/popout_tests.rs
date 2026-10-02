// HUP-S5.4 — pop-out window framework tests. Included into `popout.rs` as `mod tests`, so private
// helpers are reachable via `super::*`. The window itself needs a running app; everything that
// decides what a window may be, where it goes and what it may call is a pure function tested here,
// together with the capability files that bound it.
use super::*;
use std::collections::BTreeSet;

// ---------------------------------------------------------------------------
// The allowlist
// ---------------------------------------------------------------------------

#[test]
fn the_allowlist_is_the_five_planned_kinds() {
    let names: Vec<&str> = PopoutKind::ALL.iter().map(|k| k.as_str()).collect();
    assert_eq!(names, ["browser", "contract", "monitor", "diff", "media"]);
}

#[test]
fn kinds_parse_exactly_and_nothing_else_parses() {
    for k in PopoutKind::ALL {
        assert_eq!(PopoutKind::parse(k.as_str()), Ok(k));
    }
    for bad in [
        "",
        "main",
        "Monitor",
        "MONITOR",
        " monitor",
        "monitor ",
        "shell",
        "fs",
        "popout-monitor",
        "../monitor",
        "monitor\0",
        "mon",
    ] {
        assert!(PopoutKind::parse(bad).is_err(), "{bad:?} must not parse");
    }
}

#[test]
fn labels_are_prefixed_unique_and_never_main() {
    let labels: BTreeSet<String> = PopoutKind::ALL.iter().map(|k| k.label()).collect();
    assert_eq!(labels.len(), PopoutKind::ALL.len());
    for k in PopoutKind::ALL {
        assert_eq!(k.label(), format!("popout-{}", k.as_str()));
        assert_ne!(k.label(), MAIN_LABEL);
        assert_eq!(PopoutKind::from_label(&k.label()), Some(k));
        assert!(!k.title().is_empty());
    }
    for bad in [
        "main",
        "popout-",
        "popout-shell",
        "popout-Monitor",
        "xpopout-monitor",
    ] {
        assert_eq!(PopoutKind::from_label(bad), None);
    }
}

#[test]
fn the_typescript_allowlist_matches_this_one() {
    let ts = include_str!("../../src/popout/kinds.ts");
    let line = ts
        .lines()
        .find(|l| l.contains("export const POPOUT_KINDS"))
        .expect("POPOUT_KINDS in kinds.ts");
    let quoted: Vec<&str> = line.split('"').skip(1).step_by(2).collect();
    let rust: Vec<&str> = PopoutKind::ALL.iter().map(|k| k.as_str()).collect();
    assert_eq!(
        quoted, rust,
        "src/popout/kinds.ts and popout.rs must list the same kinds"
    );
}

#[test]
fn the_browser_contract_reader_activity_monitor_and_media_player_have_views_today() {
    // HUP-S5.1 added the Browser, HUP-S6.7 the Contract reader and HUP-S10.1 the Media player to
    // the S7.6 Activity monitor.
    let ready: Vec<PopoutKind> = PopoutKind::ALL
        .into_iter()
        .filter(|k| k.available())
        .collect();
    assert_eq!(
        ready,
        [
            PopoutKind::Browser,
            PopoutKind::Contract,
            PopoutKind::Monitor,
            PopoutKind::Media
        ]
    );
    assert_eq!(
        check_open_request("main", "browser"),
        Ok(PopoutKind::Browser)
    );
}

#[test]
fn the_main_window_may_open_the_contract_reader() {
    assert_eq!(
        check_open_request("main", "contract"),
        Ok(PopoutKind::Contract)
    );
    let err = check_open_request("popout-contract", "contract").expect_err("pop-out caller");
    assert!(err.contains("main window"), "{err}");
}

#[test]
fn hup_s10_1_the_media_player_opens_from_the_main_window_only() {
    assert_eq!(check_open_request("main", "media"), Ok(PopoutKind::Media));
    assert!(check_open_request("popout-media", "media").is_err());
    assert!(check_open_request("popout-monitor", "media").is_err());
}

// ---------------------------------------------------------------------------
// The open guard
// ---------------------------------------------------------------------------

#[test]
fn only_the_main_window_may_open_a_popout() {
    assert_eq!(
        check_open_request("main", "monitor"),
        Ok(PopoutKind::Monitor)
    );
    for caller in ["popout-monitor", "popout-browser", "", "Main", "other"] {
        let err = check_open_request(caller, "monitor").expect_err(caller);
        assert!(err.contains("main window"), "{err}");
    }
}

#[test]
fn unknown_kinds_are_refused_by_name() {
    let err = check_open_request("main", "shell").expect_err("shell");
    assert!(err.contains("not a pop-out"), "{err}");
}

#[test]
fn kinds_without_a_view_are_refused_honestly() {
    // The diff view is the one kind without a view after S5.1, S6.7 and S10.1.
    let err = check_open_request("main", "diff").expect_err("diff");
    assert!(err.contains("not built yet"), "{err}");
}

// ---------------------------------------------------------------------------
// Geometry: persisted size and position
// ---------------------------------------------------------------------------

fn g(width: f64, height: f64, x: Option<f64>, y: Option<f64>) -> Geometry {
    Geometry {
        width,
        height,
        x,
        y,
    }
}

#[test]
fn sizes_are_clamped_to_the_kinds_minimum_and_a_sane_maximum() {
    let k = PopoutKind::Monitor;
    let (min_w, min_h) = k.min_size();
    let tiny = sanitize(g(10.0, 10.0, None, None), k).expect("tiny");
    assert_eq!((tiny.width, tiny.height), (min_w, min_h));
    let huge = sanitize(g(1e9, 1e9, None, None), k).expect("huge");
    assert_eq!((huge.width, huge.height), (MAX_DIMENSION, MAX_DIMENSION));
}

#[test]
fn non_finite_geometry_is_dropped() {
    let k = PopoutKind::Monitor;
    assert!(sanitize(g(f64::NAN, 400.0, None, None), k).is_none());
    assert!(sanitize(g(400.0, f64::INFINITY, None, None), k).is_none());
    let pos = sanitize(g(400.0, 400.0, Some(f64::NAN), Some(5.0)), k).expect("size kept");
    assert_eq!(
        (pos.x, pos.y),
        (None, None),
        "a half-valid position is dropped whole"
    );
    let far = sanitize(g(400.0, 400.0, Some(1e9), Some(5.0)), k).expect("size kept");
    assert_eq!((far.x, far.y), (None, None));
}

#[test]
fn a_position_off_every_screen_is_dropped_and_the_window_centres() {
    let screens = [Rect {
        x: 0.0,
        y: 0.0,
        w: 1440.0,
        h: 900.0,
    }];
    let on = placement(g(400.0, 300.0, Some(100.0), Some(100.0)), &screens);
    assert_eq!((on.x, on.y), (Some(100.0), Some(100.0)));
    // A monitor that was unplugged: the saved spot is now nowhere.
    let off = placement(g(400.0, 300.0, Some(3000.0), Some(100.0)), &screens);
    assert_eq!((off.x, off.y), (None, None));
    assert_eq!((off.width, off.height), (400.0, 300.0));
    // Only a sliver of the title bar visible is not enough to grab it.
    let sliver = placement(g(400.0, 300.0, Some(1435.0), Some(100.0)), &screens);
    assert_eq!((sliver.x, sliver.y), (None, None));
    // A second screen to the left (negative coordinates) is a real place.
    let two = [
        screens[0],
        Rect {
            x: -1920.0,
            y: 0.0,
            w: 1920.0,
            h: 1080.0,
        },
    ];
    let left = placement(g(400.0, 300.0, Some(-1000.0), Some(50.0)), &two);
    assert_eq!((left.x, left.y), (Some(-1000.0), Some(50.0)));
    // No monitor information: keep the size, let the OS place it.
    let none = placement(g(400.0, 300.0, Some(100.0), Some(100.0)), &[]);
    assert_eq!((none.x, none.y), (None, None));
}

#[test]
fn the_geometry_store_round_trips_and_tolerates_garbage() {
    let mut m = BTreeMap::new();
    m.insert(PopoutKind::Monitor, g(420.0, 640.0, Some(10.0), Some(20.0)));
    let text = render_store(&m);
    assert_eq!(parse_store(&text), m);
    assert!(parse_store("").is_empty());
    assert!(parse_store("not json").is_empty());
    assert!(parse_store("[1,2,3]").is_empty());
    // Unknown kinds and malformed entries are skipped; good ones survive.
    let mixed = r#"{"monitor":{"width":420,"height":640,"x":null,"y":null},"shell":{"width":1,"height":1},"diff":"nope"}"#;
    let parsed = parse_store(mixed);
    assert_eq!(parsed.len(), 1);
    assert_eq!(
        parsed.get(&PopoutKind::Monitor),
        Some(&g(420.0, 640.0, None, None))
    );
}

#[test]
fn saving_one_kind_keeps_the_others() {
    let mut m = BTreeMap::new();
    m.insert(PopoutKind::Monitor, g(420.0, 640.0, None, None));
    let text = render_store(&m);
    let next = with_saved(
        &text,
        PopoutKind::Diff,
        g(800.0, 600.0, Some(1.0), Some(2.0)),
    );
    let parsed = parse_store(&next);
    assert_eq!(parsed.len(), 2);
    assert_eq!(
        parsed.get(&PopoutKind::Monitor),
        Some(&g(420.0, 640.0, None, None))
    );
}

// ---------------------------------------------------------------------------
// Navigation: a pop-out only ever shows the app's own pages
// ---------------------------------------------------------------------------

fn url(s: &str) -> url::Url {
    url::Url::parse(s).expect("test url")
}

#[test]
fn a_popout_only_navigates_within_the_app() {
    assert!(navigation_allowed(
        &url("tauri://localhost/index.html"),
        false
    ));
    assert!(navigation_allowed(
        &url("http://tauri.localhost/index.html"),
        false
    ));
    assert!(navigation_allowed(&url("https://tauri.localhost/"), false));
    for bad in [
        "https://example.com/",
        "http://localhost:1420/",
        "https://tauri.localhost.evil.com/",
        "file:///etc/passwd",
        "javascript:alert(1)",
        "data:text/html,hi",
        "tauri://evil/",
    ] {
        assert!(
            !navigation_allowed(&url(bad), false),
            "{bad} must be refused"
        );
    }
}

#[test]
fn the_dev_server_is_allowed_only_in_dev_builds() {
    assert!(navigation_allowed(&url("http://localhost:1420/"), true));
    assert!(!navigation_allowed(&url("http://localhost:1421/"), true));
    assert!(!navigation_allowed(&url("http://localhost:1420/"), false));
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

#[test]
fn monitor_facts_report_the_real_local_context_window() {
    let facts = tauri::async_runtime::block_on(popout_monitor_facts()).expect("facts");
    assert_eq!(facts.local_ctx_tokens, crate::serve::DEFAULT_CTX_SIZE);
    let json = serde_json::to_value(facts).expect("json");
    assert_eq!(json["localCtxTokens"], crate::serve::DEFAULT_CTX_SIZE);
}

#[test]
fn both_commands_are_registered_in_the_invoke_handler() {
    let src = include_str!("lib.rs");
    for cmd in ["popout::popout_open,", "popout::popout_monitor_facts,"] {
        assert!(src.contains(cmd), "not registered: {cmd}");
    }
}

// ---------------------------------------------------------------------------
// Capabilities: least privilege
// ---------------------------------------------------------------------------

fn capability(name: &str) -> serde_json::Value {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("capabilities")
        .join(name);
    let text = std::fs::read_to_string(&path).expect("read capability");
    serde_json::from_str(&text).expect("capability json")
}

fn strings(v: &serde_json::Value) -> Vec<String> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// Permissions a pop-out may hold: the bridge's events, nothing else.
const POPOUT_PERMISSIONS: &[&str] = &[
    "core:event:allow-listen",
    "core:event:allow-unlisten",
    "core:event:allow-emit-to",
];

#[test]
fn the_popout_capability_covers_exactly_the_popout_windows() {
    let cap = capability("popout.json");
    let mut windows = strings(&cap["windows"]);
    windows.sort();
    let mut expected: Vec<String> = PopoutKind::ALL.iter().map(|k| k.label()).collect();
    expected.sort();
    assert_eq!(windows, expected);
    assert!(cap.get("webviews").is_none());
    assert!(
        cap.get("remote").is_none(),
        "pop-outs never get a remote-origin capability"
    );
}

#[test]
fn the_popout_capability_grants_only_the_bridge_events() {
    let cap = capability("popout.json");
    let perms = strings(&cap["permissions"]);
    assert!(!perms.is_empty());
    for p in &perms {
        assert!(
            POPOUT_PERMISSIONS.contains(&p.as_str()),
            "pop-outs must not hold {p}"
        );
    }
    // Every permission is a plain string: no scoped (object) entries widen anything.
    assert_eq!(
        cap["permissions"].as_array().map(Vec::len),
        Some(perms.len())
    );
}

#[test]
fn the_main_capability_stays_on_the_main_window_and_holds_the_app_commands() {
    let cap = capability("default.json");
    assert_eq!(strings(&cap["windows"]), ["main"]);
    assert!(strings(&cap["permissions"]).contains(&MAIN_WINDOW_COMMANDS.to_string()));
}

#[test]
fn no_other_capability_grants_the_app_commands_or_names_a_popout() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("capabilities");
    for entry in std::fs::read_dir(&dir).expect("capabilities dir") {
        let path = entry.expect("entry").path();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_string();
        if name == "default.json" || name == "popout.json" {
            continue;
        }
        if name == "popout-contract.json" {
            // The one exception, exactly: the Contract reader's relay command, on its window only.
            let cap = capability(&name);
            assert_eq!(strings(&cap["windows"]), [PopoutKind::Contract.label()]);
            assert_eq!(strings(&cap["permissions"]), ["contract-reader-relay"]);
            continue;
        }
        let text = std::fs::read_to_string(&path).expect("read");
        assert!(
            !text.contains(MAIN_WINDOW_COMMANDS),
            "{name} must not grant the app commands"
        );
        assert!(
            !text.contains(LABEL_PREFIX),
            "{name} must not cover a pop-out window"
        );
    }
    let popout = std::fs::read_to_string(dir.join("popout.json")).expect("popout.json");
    assert!(!popout.contains(MAIN_WINDOW_COMMANDS));
}

/// The command names inside `generate_handler![…]` in lib.rs.
fn registered_commands() -> BTreeSet<String> {
    let src = include_str!("lib.rs");
    let start =
        src.find("generate_handler![").expect("generate_handler!") + "generate_handler![".len();
    let end = start + src[start..].find("])").expect("end of generate_handler!");
    src[start..end]
        .lines()
        .map(|l| l.split("//").next().unwrap_or_default())
        .flat_map(|l| l.split(','))
        .map(str::trim)
        .filter(|s| s.contains("::"))
        .filter_map(|s| s.rsplit("::").next().map(str::to_string))
        .collect()
}

/// The commands the main-window permission allows (`commands.allow = [...]`).
fn main_window_permission_commands() -> BTreeSet<String> {
    let toml = include_str!("../permissions/main-window.toml");
    let start =
        toml.find("commands.allow = [").expect("commands.allow") + "commands.allow = [".len();
    let end = start + toml[start..].find(']').expect("end of commands.allow");
    toml[start..end]
        .lines()
        .map(|l| l.split('#').next().unwrap_or_default())
        .flat_map(|l| {
            l.split('"')
                .skip(1)
                .step_by(2)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .collect()
}

#[test]
fn every_registered_command_is_allowed_for_the_main_window_and_nothing_more() {
    let registered = registered_commands();
    let allowed = main_window_permission_commands();
    assert!(
        registered.len() > 100,
        "parsed {} commands; the parser is broken",
        registered.len()
    );
    let missing: Vec<_> = registered.difference(&allowed).collect();
    let extra: Vec<_> = allowed.difference(&registered).collect();
    assert!(
        missing.is_empty(),
        "commands registered in lib.rs but not allowed for the main window (the main window \
         could not call them): {missing:?}. Add them to src-tauri/permissions/main-window.toml"
    );
    assert!(
        extra.is_empty(),
        "main-window.toml allows commands that are not registered: {extra:?}"
    );
}

#[test]
fn the_main_window_permission_is_named_as_the_code_expects() {
    let toml = include_str!("../permissions/main-window.toml");
    assert!(toml.contains(&format!("identifier = \"{MAIN_WINDOW_COMMANDS}\"")));
    assert!(
        !toml.contains("commands.deny"),
        "the main window keeps every command it had"
    );
}

/// Resolve the app-command ACL with Tauri's own resolver (plugin permissions are Tauri's and are
/// left out; this is about the app's commands).
fn resolved_app_acl() -> tauri_utils::acl::resolved::Resolved {
    use tauri_utils::acl::capability::Capability;
    use tauri_utils::acl::manifest::{Manifest, PermissionFile};
    let file: PermissionFile =
        toml::from_str(include_str!("../permissions/main-window.toml")).expect("main-window.toml");
    let relay: PermissionFile = toml::from_str(include_str!("../permissions/contract-reader.toml"))
        .expect("contract-reader.toml");
    let mut acl = BTreeMap::new();
    acl.insert(
        tauri_utils::acl::APP_ACL_KEY.to_string(),
        Manifest::new(vec![file, relay], None),
    );
    let mut caps = BTreeMap::new();
    for name in ["default.json", "popout.json", "popout-contract.json"] {
        let mut cap: Capability = serde_json::from_value(capability(name)).expect("capability");
        cap.permissions
            .retain(|p| p.identifier().get_prefix().is_none());
        caps.insert(cap.identifier.clone(), cap);
    }
    tauri_utils::acl::resolved::Resolved::resolve(&acl, caps, tauri_utils::platform::Target::MacOS)
        .expect("tauri resolves the app ACL")
}

#[test]
fn tauri_resolves_every_app_command_to_the_main_window_only() {
    let resolved = resolved_app_acl();
    let registered = registered_commands();
    for cmd in &registered {
        let grants = resolved
            .allowed_commands
            .get(cmd)
            .unwrap_or_else(|| panic!("{cmd} is not allowed anywhere"));
        assert!(
            grants
                .iter()
                .any(|g| g.windows.iter().any(|w| w.matches(MAIN_LABEL))),
            "{cmd} must stay callable from the main window"
        );
        for kind in PopoutKind::ALL {
            let label = kind.label();
            let callable = grants
                .iter()
                .any(|g| g.windows.iter().any(|w| w.matches(&label)));
            // The one exception: the Contract reader's relay, on the reader's window only (Rust
            // also checks the caller's label, see popout_contract.rs).
            let relay = cmd == "popout_contract_send" && kind == PopoutKind::Contract;
            assert_eq!(
                callable,
                relay,
                "{cmd} from {label}: callable={callable}, expected {relay}"
            );
        }
    }
    // The signing and custody commands, named, as the plainest statement of the boundary.
    for cmd in [
        "sign_approve",
        "sign_and_broadcast",
        "custody_unlock",
        "open_external",
        "hermes_session_stop",
    ] {
        assert!(
            registered.contains(cmd),
            "{cmd} should be a registered command"
        );
    }
}
