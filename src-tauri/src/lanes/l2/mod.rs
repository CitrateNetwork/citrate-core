//! Lane L2's Rust module tree (L0-WP04, L0_INTEGRATION_SEAMS 3.2). Owned by L2.
//!
//! A new L2 module is declared here (`pub(crate) mod <name>;`, file `lanes/l2/<name>.rs`), never
//! in lib.rs. It is reached as `crate::lanes::l2::<name>`, for example from a command path in
//! `commands/l2.rs`. Modules that predate 0.6.0 keep their `mod` line in lib.rs.

pub(crate) mod hooks;
