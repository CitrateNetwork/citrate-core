//! HUP-S6.7 hardening: the Contract reader's requests reach the main window through Rust.
//!
//! Every pop-out holds `core:event:allow-emit-to` for the typed bridge, and a Tauri event carries
//! no sender, so the main window cannot tell which window emitted a message. The Contract reader's
//! requests (view calls, write proposals, explanations) therefore do not travel over the event bus.
//! The reader calls [`popout_contract_send`], the one app command its own capability
//! (`capabilities/popout-contract.json`) grants it; Rust accepts it only from the `popout-contract`
//! window, queues it and pings the main window, which drains the queue with
//! [`popout_contract_take`] (main window only). A ping carries nothing, so a forged one only drains
//! an empty queue. Answers still go back over the event bus.

use std::collections::VecDeque;
use std::sync::Mutex;

/// The only window whose requests are accepted.
pub const READER_LABEL: &str = "popout-contract";
/// The window that drains the queue.
pub const MAIN_LABEL: &str = "main";
/// The bridge event both sides use (`src/popout/bridge.ts`, `POPOUT_EVENT`).
pub const POPOUT_EVENT: &str = "citrate-popout";
/// Largest request accepted (serialized JSON). The biggest legitimate one is a write with 64 KiB
/// of calldata as hex.
pub const MAX_REQUEST_BYTES: usize = 160 * 1024;
/// Requests waiting for the main window; more are refused until it drains.
pub const MAX_QUEUED: usize = 32;

/// The queue between the reader and the main window.
#[derive(Default)]
pub struct ContractInbox(Mutex<VecDeque<serde_json::Value>>);

impl ContractInbox {
    fn lock(&self) -> std::sync::MutexGuard<'_, VecDeque<serde_json::Value>> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Queue a request from `caller`. Only the Contract reader may send.
    pub fn push(&self, caller: &str, payload: serde_json::Value) -> Result<(), String> {
        if caller != READER_LABEL {
            return Err("only the Contract reader may send contract requests".into());
        }
        let size = serde_json::to_vec(&payload)
            .map(|v| v.len())
            .unwrap_or(usize::MAX);
        if size > MAX_REQUEST_BYTES {
            return Err("the request is too large".into());
        }
        let mut q = self.lock();
        if q.len() >= MAX_QUEUED {
            return Err("the main window is busy; try again".into());
        }
        q.push_back(payload);
        Ok(())
    }

    /// Everything queued, oldest first. Only the main window may drain.
    pub fn take(&self, caller: &str) -> Result<Vec<serde_json::Value>, String> {
        if caller != MAIN_LABEL {
            return Err("only the main window reads contract requests".into());
        }
        Ok(self.lock().drain(..).collect())
    }
}

/// **popout_contract_send** — the Contract reader hands one request to the main window.
#[tauri::command]
pub async fn popout_contract_send(
    app: tauri::AppHandle,
    webview_window: tauri::WebviewWindow,
    payload: serde_json::Value,
) -> Result<(), String> {
    use tauri::{Emitter, Manager};
    let inbox = app
        .try_state::<ContractInbox>()
        .ok_or("internal: contract inbox unavailable")?;
    inbox.push(webview_window.label(), payload)?;
    app.emit_to(
        MAIN_LABEL,
        POPOUT_EVENT,
        serde_json::json!({ "v": 1, "type": "contract.inbox" }),
    )
    .map_err(|e| e.to_string())
}

/// **popout_contract_take** — the main window drains the reader's requests.
#[tauri::command]
pub async fn popout_contract_take(
    app: tauri::AppHandle,
    webview_window: tauri::WebviewWindow,
) -> Result<Vec<serde_json::Value>, String> {
    use tauri::Manager;
    let inbox = app
        .try_state::<ContractInbox>()
        .ok_or("internal: contract inbox unavailable")?;
    inbox.take(webview_window.label())
}

#[cfg(test)]
#[path = "popout_contract_tests.rs"]
mod tests;
