//! HUP-S0.1 — run a blocking command body OFF the main thread (shared by the app and the kit).
//!
//! A synchronous `#[tauri::command]` executes on the main (UI) thread; any network, socket,
//! keychain, KDF, file, process, or sleep work it reaches freezes the window (the macOS pinwheel).
//! Every such command is an `async` wrapper that hands its unchanged `*_sync` body to Tauri's
//! blocking pool through [`off_main`]. The tripwire in `src-tauri/src/main_thread_tripwire.rs`
//! scans both crates and keeps it that way.

/// Run `f` on the async runtime's blocking pool and await its result. A panic or cancellation in
/// the background task surfaces as a coarse `Err` (never a crash, never an `unwrap`).
pub async fn off_main<T, F>(f: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| format!("background task failed: {e}"))?
}
