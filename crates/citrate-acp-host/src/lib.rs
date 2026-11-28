//! citrate-acp-host: the Agent Client Protocol host for Citrate Core (planset v060, L1).
//!
//! Core is the ACP client. This crate pins the protocol crate (`pin`), checks an agent against its
//! reviewed hash before it runs (`registry`), spawns it as its own process group with a cleared
//! environment and scanned stdio (`process`), keeps its updates in a sequenced log (`events`), and
//! answers its permission requests from Core's policy (`permission`). The session manager that
//! drives ACP over these parts is L1-WP03b.

pub mod events;
pub mod permission;
pub mod pin;
pub mod process;
pub mod registry;
