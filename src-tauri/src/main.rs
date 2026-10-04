// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // HUP-S4.3: when Hermes's MCP host starts this executable as the read-only memory bridge,
    // relay stdio to the mem-mcp socket and exit before any app start-up.
    if let Some(code) = citrate_core_lib::mem_mcp_bridge::maybe_run_from_args() {
        std::process::exit(code);
    }
    // HUP-S4.2 — `--mcp-stdio` runs only the MCP stdio shim (no window, no sidecars).
    if std::env::args()
        .skip(1)
        .any(|a| a == citrate_core_lib::NODE_MCP_STDIO_FLAG)
    {
        std::process::exit(citrate_core_lib::node_mcp_stdio_main());
    }
    citrate_core_lib::run()
}
