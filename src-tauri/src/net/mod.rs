//! L7 networking (planset v060, lanes/L7_NETWORKING.md): many machines under one member.
//!
//! L7-WP01 freezes the lane's shared vocabulary here (section 3.2 and 3.3): which private link a
//! machine uses ([`link_security::LinkKind`]) and how far a link's protection goes
//! ([`link_security::LinkSecurity`]), plus the characterization suite that pins today's
//! networking behavior before any L7 work package changes it.
//!
//! The module is declared from `fleet.rs` (an L7 file) with `#[path]`, so it is `crate::fleet::net`
//! until L0's per-lane module tree (L0-WP04, `crate::lanes::l7`) gives it a lane home. Nothing in
//! the running app reads it yet: the first consumer is the `net_status` command (L7 section 3.1).

pub(crate) mod link_security;

#[cfg(test)]
mod characterization_tests;
