//! Lane L1's Rust module tree (L0-WP04, L0_INTEGRATION_SEAMS 3.2). Owned by L1.
//!
//! A new L1 module is declared here (`pub(crate) mod <name>;`, file `lanes/l1/<name>.rs`), never
//! in lib.rs. It is reached as `crate::lanes::l1::<name>`, for example from a command path in
//! `commands/l1.rs`. Modules that predate 0.6.0 keep their `mod` line in lib.rs.

pub(crate) mod hooks;
