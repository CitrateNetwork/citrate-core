//! Linux black-window guard: WebKitGTK environment defaults set before the webview starts.
//!
//! WebKitGTK's GPU compositing and DMA-BUF renderer can leave the window black (and on an
//! NVIDIA GB10, driver 580, black out the whole X display) even though the page renders. Both
//! paths are turned off by default on Linux (first shipped in #237). A value the member already
//! set is never overridden, so a stack that renders fine can turn either path back on.
//!
//! [`decide`] is the whole policy and is pure, so it is tested on every platform; only
//! [`apply`] touches the process environment, and only on Linux.

use std::path::Path;

/// Disables WebKitGTK's DMA-BUF renderer.
pub const DMABUF_RENDERER: &str = "WEBKIT_DISABLE_DMABUF_RENDERER";
/// Disables WebKitGTK's accelerated compositing mode.
pub const COMPOSITING_MODE: &str = "WEBKIT_DISABLE_COMPOSITING_MODE";

/// The variables to set, given which are already set and whether an NVIDIA driver is loaded.
///
/// Both are set whenever unset. NVIDIA systems need both (the GB10 report); the compositing
/// default also stays on for other GPUs because #237 shipped it for every Linux system and
/// narrowing it would bring the black window back where that default was what fixed it.
/// `nvidia_present` is reported in the log line but does not change the outcome.
pub fn decide(
    is_set: impl Fn(&str) -> bool,
    _nvidia_present: bool,
) -> Vec<(&'static str, &'static str)> {
    [DMABUF_RENDERER, COMPOSITING_MODE]
        .into_iter()
        .filter(|k| !is_set(k))
        .map(|k| (k, "1"))
        .collect()
}

/// Whether the NVIDIA kernel driver is loaded, from the files it exposes.
#[cfg(target_os = "linux")]
pub fn nvidia_present() -> bool {
    nvidia_present_under(Path::new("/"))
}

fn nvidia_present_under(root: &Path) -> bool {
    root.join("proc/driver/nvidia/version").exists() || root.join("sys/module/nvidia").exists()
}

/// The one log line naming what was applied (and what the member had set already).
pub fn summary(applied: &[(&str, &str)], nvidia_present: bool) -> String {
    let gpu = if nvidia_present {
        "NVIDIA driver present"
    } else {
        "no NVIDIA driver"
    };
    if applied.is_empty() {
        format!("[linux-webkit] {gpu}; {DMABUF_RENDERER} and {COMPOSITING_MODE} already set by the environment, left as they are")
    } else {
        let set: Vec<String> = applied.iter().map(|(k, v)| format!("{k}={v}")).collect();
        format!("[linux-webkit] {gpu}; applied {}", set.join(", "))
    }
}

/// Applies the defaults to this process and logs what it did. Linux only.
///
/// Must run first in `run()`, before `tauri::Builder` creates GTK/WebKit and before this process
/// starts any thread, so the environment write cannot race a reader.
#[cfg(target_os = "linux")]
pub fn apply() {
    let nvidia = nvidia_present();
    let overrides = decide(|k| std::env::var_os(k).is_some(), nvidia);
    for (k, v) in &overrides {
        set_env_single_threaded(k, v);
    }
    eprintln!("{}", summary(&overrides, nvidia));
}

/// SAFETY (edition 2024 makes `set_var` unsafe): called only from [`apply`], at the top of
/// `run()`, while the process is still single-threaded and before GTK/WebKit read the environment.
#[cfg(target_os = "linux")]
fn set_env_single_threaded(key: &str, value: &str) {
    std::env::set_var(key, value);
}

#[cfg(test)]
#[path = "linux_webkit_env_tests.rs"]
mod tests;
