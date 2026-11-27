//! 0.6.0 L3-WP00 characterization: what app quit does today (citrate-core a826e441), pinned
//! before L3-WP16 (tray / menu-bar agent, close-to-tray) changes it. A failure here means the
//! quit path changed: update the expectation only together with that reviewed change.
//!
//! 1. The app's run loop stops every sidecar on `ExitRequested` and on `Exit`, in this order.
//! 2. Nothing intercepts a quit or the main window's close: closing the window ends the app.
//!
//! The Tauri run loop cannot run in a unit test, so the pins read the real sources. This is an
//! integration test (src-tauri/tests) so that the L0-owned `lib.rs` needs no new `mod` line.

const LIB: &str = include_str!("../src/lib.rs");
const POPOUT: &str = include_str!("../src/popout.rs");

/// The closure passed to the app's `.run(...)`.
fn run_handler() -> &'static str {
    assert_eq!(
        LIB.matches(".run(|app_handle, event| {").count(),
        1,
        "one run loop handler"
    );
    LIB.split(".run(|app_handle, event| {")
        .nth(1)
        .and_then(|s| s.split("\n        });\n").next())
        .expect("the run loop handler")
}

/// The body of `shutdown_all_sidecars`.
fn shutdown_body() -> &'static str {
    LIB.split("pub(crate) fn shutdown_all_sidecars(app: &tauri::AppHandle) {")
        .nth(1)
        .and_then(|s| s.split("\n}\n").next())
        .expect("shutdown_all_sidecars exists")
}

#[test]
fn exit_requested_stops_all_sidecars() {
    let handler = run_handler();
    assert!(
        handler.contains("tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit"),
        "{handler}"
    );
    assert!(
        handler.contains("shutdown_all_sidecars(app_handle);"),
        "{handler}"
    );

    // Today's stop order: the knowledge importer first, then each supervised sidecar held in
    // managed state, then the lazily started daemons.
    let stops: Vec<&str> = shutdown_body()
        .lines()
        .map(str::trim)
        .filter_map(|l| {
            if let Some(rest) = l.strip_prefix("if let Some(s) = app.try_state::<") {
                rest.split(">()").next()
            } else if l.ends_with("::shutdown();") && !l.starts_with("//") {
                Some(l)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(
        stops,
        [
            "knowledge_import::shutdown();",
            "node::NodeState",
            "agent::AgentState",
            "memory::MemoryState",
            "serve::ServeState",
            "ipfs::IpfsState",
            "comms::shutdown();",
            "cluster::shutdown();",
            "hermes::shutdown();",
        ]
    );
    assert_eq!(
        shutdown_body().matches("s.0.stop();").count(),
        5,
        "each managed sidecar is stopped"
    );
}

#[test]
fn no_window_close_or_exit_is_intercepted() {
    let handler = run_handler();
    assert!(!handler.contains("prevent_exit"), "{handler}");
    for (name, src) in [("lib.rs", LIB), ("popout.rs", POPOUT)] {
        assert!(!src.contains("prevent_close"), "{name} intercepts a close");
        assert!(!src.contains("prevent_exit"), "{name} intercepts a quit");
    }
    // The only main-window hook closes the pop-outs when the main window is destroyed.
    assert_eq!(LIB.matches("on_window_event").count(), 0);
    assert!(POPOUT.contains("if matches!(event, WindowEvent::Destroyed) {"));
}
