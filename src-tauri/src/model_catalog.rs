//! CX-S1 (lane s1) — model catalog & switcher host commands (C-16).
//!
//! S0.3 scaffold: each `#[tauri::command]` returns an honest `not wired` error (Rule 1) until
//! CX-S1 implements the Hugging Face Hub / GitHub Releases resolver + runtime-selectable
//! llama-server (`-m`). Command NAMES are frozen here and registered once in `lib.rs`; CX-S1
//! fills in the bodies (and may widen the signatures) inside THIS module — never `lib.rs`.

fn not_wired(cmd: &str) -> Result<(), String> {
    Err(format!("model_catalog::{cmd} is not wired yet (CX-S1 scaffold)"))
}

#[tauri::command]
pub fn model_catalog_local() -> Result<(), String> {
    not_wired("local")
}

#[tauri::command]
pub fn model_catalog_search() -> Result<(), String> {
    not_wired("search")
}

#[tauri::command]
pub fn model_catalog_download() -> Result<(), String> {
    not_wired("download")
}

#[tauri::command]
pub fn model_catalog_select() -> Result<(), String> {
    not_wired("select")
}
