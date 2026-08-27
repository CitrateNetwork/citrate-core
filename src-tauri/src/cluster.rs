//! CX-S4 (lane s4) — group private P2P cluster host commands (C-20).
//!
//! S0.3 scaffold: honest `not wired` stubs. CX-S4 wires the hybrid Noise-identity (roster-minted)
//! + libp2p-gossipsub cluster (D-24). Names frozen; registered in lib.rs.

fn not_wired(cmd: &str) -> Result<(), String> {
    Err(format!("cluster::{cmd} is not wired yet (CX-S4 scaffold)"))
}

#[tauri::command]
pub fn cluster_status() -> Result<(), String> {
    not_wired("status")
}

#[tauri::command]
pub fn cluster_join() -> Result<(), String> {
    not_wired("join")
}

#[tauri::command]
pub fn cluster_peers() -> Result<(), String> {
    not_wired("peers")
}

#[tauri::command]
pub fn cluster_share_file() -> Result<(), String> {
    not_wired("share_file")
}

#[tauri::command]
pub fn cluster_leave() -> Result<(), String> {
    not_wired("leave")
}
