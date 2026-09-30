//! HUP-S0.1 — run a blocking command body OFF the main thread.
//!
//! A synchronous `#[tauri::command]` executes on the main (UI) thread; any network, socket,
//! process, or sleep I/O it reaches freezes the window (the macOS pinwheel). Every such command is
//! an `async` wrapper that hands its unchanged `*_sync` body to Tauri's blocking pool through
//! [`off_main`]. The tripwire in `main_thread_tripwire.rs` keeps it that way.

/// Run `f` on the async runtime's blocking pool and await its result. A panic or cancellation in
/// the background task surfaces as a coarse `Err` (never a crash, never an `unwrap`).
pub(crate) async fn off_main<T, F>(f: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| format!("background task failed: {e}"))?
}

#[cfg(test)]
mod tests {
    use super::off_main;

    #[test]
    fn off_main_returns_the_body_result() {
        let ok = tauri::async_runtime::block_on(off_main(|| Ok::<_, String>(7)));
        assert_eq!(ok, Ok(7));
        let err = tauri::async_runtime::block_on(off_main(|| Err::<u8, _>("nope".to_string())));
        assert_eq!(err, Err("nope".to_string()));
    }

    #[test]
    fn off_main_runs_on_a_different_thread() {
        let caller = std::thread::current().id();
        let worker = tauri::async_runtime::block_on(off_main(|| {
            Ok::<_, String>(std::thread::current().id())
        }));
        assert_ne!(worker, Ok(caller));
    }
}
