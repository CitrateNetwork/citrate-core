//! Lane L9's Rust module tree (L0-WP04, L0_INTEGRATION_SEAMS 3.2). Owned by L9.
//!
//! A new L9 module is declared here (`pub(crate) mod <name>;`, file `lanes/l9/<name>.rs`), never
//! in lib.rs. It is reached as `crate::lanes::l9::<name>`, for example from a command path in
//! `commands/l9.rs`. Modules that predate 0.6.0 keep their `mod` line in lib.rs.

pub(crate) mod hooks;
