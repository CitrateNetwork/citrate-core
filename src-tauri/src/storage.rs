//! CX-S2 (lane s2) — storage & pinning host commands (C-17).
//!
//! S0.3 scaffold: honest `not wired` stubs. CX-S2 wires kubo add/pin/ls/rm/cat + the
//! ceremony-gated `IPFSIncentivesV3` bond (D-18/D-21). Command names frozen; registered in lib.rs.

fn not_wired(cmd: &str) -> Result<(), String> {
    Err(format!("storage::{cmd} is not wired yet (CX-S2 scaffold)"))
}

#[tauri::command]
pub fn storage_add() -> Result<(), String> {
    not_wired("add")
}

#[tauri::command]
pub fn storage_pin() -> Result<(), String> {
    not_wired("pin")
}

#[tauri::command]
pub fn storage_list() -> Result<(), String> {
    not_wired("list")
}

#[tauri::command]
pub fn storage_retrieve() -> Result<(), String> {
    not_wired("retrieve")
}

#[tauri::command]
pub fn storage_unpin() -> Result<(), String> {
    not_wired("unpin")
}
