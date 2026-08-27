//! CX-S6 (lane s6) — Hermes agent harness host commands (C-22).
//!
//! S0.3 scaffold: honest `not wired` stubs. CX-S6 wires the keyless Hermes sidecar
//! (`HermesManager` mirroring `agent.rs`) + skills/code behind the mandatory HITL, with every
//! chain effect routed through the SignatureCeremony (D-18). Names frozen; registered in lib.rs.
//! Distinct from the legacy `agent` module (node-agent GPU market).

fn not_wired(cmd: &str) -> Result<(), String> {
    Err(format!("hermes::{cmd} is not wired yet (CX-S6 scaffold)"))
}

#[tauri::command]
pub fn hermes_start() -> Result<(), String> {
    not_wired("start")
}

#[tauri::command]
pub fn hermes_status() -> Result<(), String> {
    not_wired("status")
}

#[tauri::command]
pub fn hermes_skills() -> Result<(), String> {
    not_wired("skills")
}

#[tauri::command]
pub fn hermes_run_skill() -> Result<(), String> {
    not_wired("run_skill")
}

#[tauri::command]
pub fn hermes_pending_approvals() -> Result<(), String> {
    not_wired("pending_approvals")
}

#[tauri::command]
pub fn hermes_stop() -> Result<(), String> {
    not_wired("stop")
}
