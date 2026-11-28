//! Lane L6's Rust module tree (L0-WP04, L0_INTEGRATION_SEAMS 3.2). Owned by L6.
//!
//! A new L6 module is declared here (`pub(crate) mod <name>;`, file `lanes/l6/<name>.rs`), never
//! in lib.rs. It is reached as `crate::lanes::l6::<name>`, for example from a command path in
//! `commands/l6.rs`. Modules that predate 0.6.0 keep their `mod` line in lib.rs.

pub(crate) mod hooks;
// L6-WP07: the Marketplace MCP catalog (verified brand servers pinned in the binary).
pub(crate) mod mcp_catalog;
