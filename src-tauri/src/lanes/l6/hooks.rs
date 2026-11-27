//! Lane L6's start-up and quit hooks (L0-WP04, L0_INTEGRATION_SEAMS 3.2). Owned by L6.
//!
//! `crate::lanes` calls these with every other lane's, in lane order l0 to l9:
//! - `plugins` after every plugin lib.rs registers (single-instance stays first). Add plugins only
//!   (`builder.plugin(..)`): lib.rs sets `.setup` and `.invoke_handler` after this, which would
//!   replace a lane's. A plugin's own init runs while the app is built, before the setup closure
//!   and so before the network gate: anything that signs or reads keys belongs in `setup`;
//! - `setup` at the end of lib.rs's setup closure, after the network and payouts gates are
//!   installed and every existing state is managed;
//! - `on_exit` after the existing sidecar teardown, on `ExitRequested` and again on `Exit`, so
//!   it must be safe to run twice.
//!
//! Each is a registration point and does nothing until L6 adds work here (Rule 1).

use tauri::{App, AppHandle, Builder, Runtime};

/// L6's Tauri plugins, registered after the existing ones.
pub(crate) fn plugins<R: Runtime>(builder: Builder<R>) -> Builder<R> {
    builder
}

/// L6's start-up step. An error aborts start-up; `lanes::setup_all` names this lane in it.
pub(crate) fn setup<R: Runtime>(_app: &App<R>) -> Result<(), Box<dyn std::error::Error>> {
    Ok(())
}

/// L6's quit step. Runs after the sidecars are stopped, possibly twice per quit.
pub(crate) fn on_exit<R: Runtime>(_app: &AppHandle<R>) {}
