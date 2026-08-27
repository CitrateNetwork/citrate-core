//! CX-S3 (lane s3) — groups: secure comms + admin RBAC host commands (C-19).
//!
//! S0.3 scaffold: honest `not wired` stubs. CX-S3 wires the `comms-relay` sidecar + the Group
//! roster/RBAC (signed RoleAssertions, atomic offboard — ADR-001). Names frozen; registered in lib.rs.

fn not_wired(cmd: &str) -> Result<(), String> {
    Err(format!("comms::{cmd} is not wired yet (CX-S3 scaffold)"))
}

#[tauri::command]
pub fn groups_create() -> Result<(), String> {
    not_wired("create")
}

#[tauri::command]
pub fn groups_list() -> Result<(), String> {
    not_wired("list")
}

#[tauri::command]
pub fn groups_join() -> Result<(), String> {
    not_wired("join")
}

#[tauri::command]
pub fn groups_roster() -> Result<(), String> {
    not_wired("roster")
}

#[tauri::command]
pub fn groups_assign_role() -> Result<(), String> {
    not_wired("assign_role")
}

#[tauri::command]
pub fn groups_offboard() -> Result<(), String> {
    not_wired("offboard")
}

#[tauri::command]
pub fn groups_send() -> Result<(), String> {
    not_wired("send")
}

#[tauri::command]
pub fn groups_messages() -> Result<(), String> {
    not_wired("messages")
}
