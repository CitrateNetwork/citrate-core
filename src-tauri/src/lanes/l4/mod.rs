//! Lane L4's Rust module tree (L0-WP04, L0_INTEGRATION_SEAMS 3.2). Owned by L4.
//!
//! A new L4 module is declared here (`pub(crate) mod <name>;`, file `lanes/l4/<name>.rs`), never
//! in lib.rs. It is reached as `crate::lanes::l4::<name>`, for example from a command path in
//! `commands/l4.rs`. Modules that predate 0.6.0 keep their `mod` line in lib.rs.

pub(crate) mod hooks;
