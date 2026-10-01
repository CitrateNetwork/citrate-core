// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // HUP-S4.2 — `--mcp-stdio` runs only the MCP stdio shim (no window, no sidecars).
    if std::env::args().skip(1).any(|a| a == "--mcp-stdio") {
        std::process::exit(citrate_core_lib::node_mcp_stdio_main());
    }
    citrate_core_lib::run()
}
