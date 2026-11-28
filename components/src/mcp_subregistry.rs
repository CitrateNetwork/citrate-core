//! The signed MCP subregistry (lane L6, L6-WP08a): the Citrate curated mirror of the official MCP
//! Registry, verified with the catalog key under its own signature domain.
//!
//! L0-WP01d mount point: this file belongs to lane L6 (`.agentile/v60-ownership.map`), and lib.rs
//! (lane L2) mounts it with one `pub mod` line so L6 never edits lib.rs. It holds no code until
//! L6-WP08a lands the verifier; nothing in the crate refers to it.
