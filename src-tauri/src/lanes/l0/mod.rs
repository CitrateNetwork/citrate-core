//! Lane L0's Rust module tree (L0-WP04, L0_INTEGRATION_SEAMS 3.2). Owned by L0.
//!
//! A new L0 module is declared here (`pub(crate) mod <name>;`, file `lanes/l0/<name>.rs`), never
//! in lib.rs. It is reached as `crate::lanes::l0::<name>`, for example from a command path in
//! `commands/l0.rs`. Modules that predate 0.6.0 keep their `mod` line in lib.rs.

pub(crate) mod hooks;
