//! The protocol pin (L1-WP02): which `agent-client-protocol` release and which ACP protocol version
//! this host is built against. `tests/pin_tests.rs` checks both against `Cargo.lock` and the crate.

/// The crates.io name of the ACP SDK.
pub const ACP_CRATE: &str = "agent-client-protocol";

/// The exact SDK release (`Cargo.toml` pins `=5.6.1`; `release/agents.lock.json` records its
/// crates.io sha256).
pub const ACP_CRATE_VERSION: &str = "5.6.1";

/// The ACP protocol version Core negotiates in `initialize` (the SDK's stable `LATEST`).
pub fn protocol_version() -> u16 {
    agent_client_protocol::schema::ProtocolVersion::LATEST.as_u16()
}
