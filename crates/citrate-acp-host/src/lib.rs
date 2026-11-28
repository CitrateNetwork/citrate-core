//! citrate-acp-host: the Agent Client Protocol host for Citrate Core (planset v060, L1).
//!
//! Core is the ACP client. This crate pins the protocol crate and records which protocol version
//! Core speaks; the session manager (L1-WP03) builds on it.

pub mod pin;
