//! Lane L5's Rust module tree (L0-WP04, L0_INTEGRATION_SEAMS 3.2). Owned by L5.
//!
//! A new L5 module is declared here (`pub(crate) mod <name>;`, file `lanes/l5/<name>.rs`), never
//! in lib.rs. It is reached as `crate::lanes::l5::<name>`, for example from a command path in
//! `commands/l5.rs`. Modules that predate 0.6.0 keep their `mod` line in lib.rs.

pub(crate) mod hooks;
