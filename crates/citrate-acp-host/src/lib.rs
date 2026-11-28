//! citrate-acp-host: the Agent Client Protocol host for Citrate Core (planset v060, L1).
//!
//! Core is the ACP client: it checks an agent against its reviewed hash (`registry`), spawns it as
//! its own process group with a cleared environment and scanned stdio (`process`), drives
//! `initialize`, `session/new`, `session/prompt` and `session/cancel` (`session`), keeps its
//! `session/update` stream in a sequenced log (`events`), and answers `session/request_permission`
//! from Core's policy (`permission`).

pub mod events;
pub mod permission;
pub mod pin;
pub mod process;
pub mod registry;
pub mod session;

pub use session::{AcpHost, HostConfig, Session, SessionStart, StartFailure};
