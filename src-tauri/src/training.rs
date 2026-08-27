//! CX-S5 (lane s5) — group train-together host commands (C-21).
//!
//! S0.3 scaffold: honest `not wired` stubs. CX-S5 wires the round-coordinator + ceremony-gated
//! SALT settlement (D-23) — a T1 money surface (06_SECURITY §2). Names frozen; registered in lib.rs.

fn not_wired(cmd: &str) -> Result<(), String> {
    Err(format!("training::{cmd} is not wired yet (CX-S5 scaffold)"))
}

#[tauri::command]
pub fn training_start() -> Result<(), String> {
    not_wired("start")
}

#[tauri::command]
pub fn training_status() -> Result<(), String> {
    not_wired("status")
}

#[tauri::command]
pub fn training_contribute() -> Result<(), String> {
    not_wired("contribute")
}

#[tauri::command]
pub fn training_reward() -> Result<(), String> {
    not_wired("reward")
}

#[tauri::command]
pub fn training_claim() -> Result<(), String> {
    not_wired("claim")
}
