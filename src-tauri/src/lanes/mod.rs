//! L0-WP04 (planset v060, L0_INTEGRATION_SEAMS 3.2): the per-lane Rust module tree and the
//! start-up and quit hooks. Owned by L0; each `l<N>/` folder is owned by lane N.
//!
//! The rule for lanes:
//! - A new Rust module of lane N is declared in `lanes/l<N>/mod.rs` and lives under `lanes/l<N>/`.
//!   lib.rs is not edited for it. Modules that predate 0.6.0 keep their `mod` line in lib.rs
//!   (moving them is churn with no benefit).
//! - A lane registers a Tauri plugin, a start-up step or a quit step in its own
//!   `lanes/l<N>/hooks.rs` (`plugins`, `setup`, `on_exit`), never in lib.rs.
//! - lib.rs calls [`plugins_all`] after its own plugins (so `tauri_plugin_single_instance` stays
//!   first, which tauri requires), [`setup_all`] as the last step of its setup closure (so every
//!   hook runs after `network_profile::install_network_gate` and `install_payouts_gate`, and
//!   cannot sign before the gate), and [`on_exit_all`] after `shutdown_all_sidecars`. The existing
//!   start-up and quit order is unchanged.
//! - A plugin hook adds plugins only. A plugin's own init runs while the app is built, before the
//!   setup closure and the network gate, so signing or key work goes in a setup hook.
//! - Every phase runs the lanes in the fixed order l0 to l9. A failing setup hook stops start-up
//!   with the first error, prefixed with its lane id; later lanes' setup hooks do not run.
//!
//! `seams_tests.rs` pins all of this (the tree, the call sites and the order).

pub(crate) mod l0;
pub(crate) mod l1;
pub(crate) mod l2;
pub(crate) mod l3;
pub(crate) mod l4;
pub(crate) mod l5;
pub(crate) mod l6;
pub(crate) mod l7;
pub(crate) mod l8;
pub(crate) mod l9;

use tauri::{App, AppHandle, Builder, Runtime};

/// The error a setup hook returns (the setup closure's own error type).
pub(crate) type HookError = Box<dyn std::error::Error>;
/// A lane's plugin hook: takes the builder, returns it with the lane's plugins added.
pub(crate) type PluginsHook<R> = fn(Builder<R>) -> Builder<R>;
/// A lane's setup hook.
pub(crate) type SetupHook<R> = fn(&App<R>) -> Result<(), HookError>;
/// A lane's quit hook.
pub(crate) type ExitHook<R> = fn(&AppHandle<R>);

/// Every lane's plugins, l0 to l9, registered after the plugins lib.rs registers itself.
pub(crate) fn plugins_all<R: Runtime>(mut builder: Builder<R>) -> Builder<R> {
    let hooks: [(&'static str, PluginsHook<R>); 10] = [
        ("l0", l0::hooks::plugins),
        ("l1", l1::hooks::plugins),
        ("l2", l2::hooks::plugins),
        ("l3", l3::hooks::plugins),
        ("l4", l4::hooks::plugins),
        ("l5", l5::hooks::plugins),
        ("l6", l6::hooks::plugins),
        ("l7", l7::hooks::plugins),
        ("l8", l8::hooks::plugins),
        ("l9", l9::hooks::plugins),
    ];
    for (lane, hook) in hooks {
        record(lane, "plugins");
        builder = hook(builder);
    }
    builder
}

/// Every lane's setup step, l0 to l9, run as the last step of lib.rs's setup closure.
pub(crate) fn setup_all<R: Runtime>(app: &App<R>) -> Result<(), HookError> {
    let hooks: [(&'static str, SetupHook<R>); 10] = [
        ("l0", l0::hooks::setup),
        ("l1", l1::hooks::setup),
        ("l2", l2::hooks::setup),
        ("l3", l3::hooks::setup),
        ("l4", l4::hooks::setup),
        ("l5", l5::hooks::setup),
        ("l6", l6::hooks::setup),
        ("l7", l7::hooks::setup),
        ("l8", l8::hooks::setup),
        ("l9", l9::hooks::setup),
    ];
    run_setup(app, &hooks)
}

/// Run `hooks` in order. The first error stops the run and is returned with its lane id, so a
/// failed start-up says which lane failed; the hooks after it do not run.
pub(crate) fn run_setup<R: Runtime>(
    app: &App<R>,
    hooks: &[(&'static str, SetupHook<R>)],
) -> Result<(), HookError> {
    for &(lane, hook) in hooks {
        record(lane, "setup");
        hook(app).map_err(|e| HookError::from(format!("lane {lane} setup hook failed: {e}")))?;
    }
    Ok(())
}

/// Every lane's quit step, l0 to l9, run after lib.rs stops the sidecars.
pub(crate) fn on_exit_all<R: Runtime>(app: &AppHandle<R>) {
    let hooks: [(&'static str, ExitHook<R>); 10] = [
        ("l0", l0::hooks::on_exit),
        ("l1", l1::hooks::on_exit),
        ("l2", l2::hooks::on_exit),
        ("l3", l3::hooks::on_exit),
        ("l4", l4::hooks::on_exit),
        ("l5", l5::hooks::on_exit),
        ("l6", l6::hooks::on_exit),
        ("l7", l7::hooks::on_exit),
        ("l8", l8::hooks::on_exit),
        ("l9", l9::hooks::on_exit),
    ];
    for (lane, hook) in hooks {
        record(lane, "on_exit");
        hook(app);
    }
}

#[cfg(test)]
thread_local! {
    /// Test-only: the `(lane, phase)` of every hook the dispatchers above ran on this thread.
    static RECORDED: std::cell::RefCell<Vec<(&'static str, &'static str)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Test-only order recorder: notes that the dispatcher ran `lane`'s `phase` hook.
#[cfg(test)]
fn record(lane: &'static str, phase: &'static str) {
    RECORDED.with(|r| r.borrow_mut().push((lane, phase)));
}

/// Outside tests nothing is recorded; the call compiles away.
#[cfg(not(test))]
fn record(_lane: &'static str, _phase: &'static str) {}

/// Test-only: the hooks recorded on this thread since the last call, in run order.
#[cfg(test)]
pub(crate) fn take_recorded() -> Vec<(&'static str, &'static str)> {
    RECORDED.with(|r| std::mem::take(&mut *r.borrow_mut()))
}
