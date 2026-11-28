//! L7-WP01: the link vocabulary every L7 surface reports in (lanes/L7_NETWORKING.md section 3.2,
//! 3.3), mirrored in TS by `src/bridge/lanes/l7.ts`. The shared fixture
//! `net/fixtures/net_status_v1.json` is checked against both sides, so the names cannot drift.
//!
//! The rule this module exists for (F-24, AR-E15.5): a link is reported as post-quantum only when
//! the hybrid ML-KEM session was established on it with exactly the expected suite. A classical
//! tunnel (WireGuard, Noise) is never labelled post-quantum, whatever runs underneath.

// The `net_status` and `net_machines` commands (L7 section 3.1) are the first readers; until they
// land the types and `classify` are exercised by tests only.
#![cfg_attr(not(test), allow(dead_code))]

use serde::{Deserialize, Serialize};

/// The private link a machine uses to reach the member's other machines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum LinkKind {
    /// The member's own Tailscale tailnet (the default choice, E9.1).
    TailnetOwn,
    /// A Citrate-managed tailnet (E9.2). Deferred past 0.6.0 by the owner; named so the type is
    /// stable, never chosen in this release.
    TailnetManaged,
    /// Cloudflare WARP in the member's Zero Trust organization (E9.3).
    Warp,
    /// The same local network.
    Lan,
    /// No private link.
    None,
}

impl LinkKind {
    /// Every kind, in the order the fixture lists them.
    pub(crate) const ALL: [LinkKind; 5] = [
        LinkKind::TailnetOwn,
        LinkKind::TailnetManaged,
        LinkKind::Warp,
        LinkKind::Lan,
        LinkKind::None,
    ];
}

/// How far a link's protection goes, as the member is told it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum LinkSecurity {
    /// The L9 hybrid ML-KEM session is established end to end on this link with [`PQ_SUITE`].
    PqHybridE2e,
    /// End-to-end encrypted, classical only (Noise or WireGuard without the PQ layer).
    ClassicalE2e,
    /// TLS ends at the provider (the Cloudflare edge), so the provider can read the traffic.
    TerminatedAtProvider,
    /// Plaintext. Only loopback is ever allowed here.
    None,
}

impl LinkSecurity {
    /// Every value, in the order the fixture lists them.
    pub(crate) const ALL: [LinkSecurity; 4] = [
        LinkSecurity::PqHybridE2e,
        LinkSecurity::ClassicalE2e,
        LinkSecurity::TerminatedAtProvider,
        LinkSecurity::None,
    ];
}

/// The one hybrid session suite a link may be reported post-quantum for (L7 section 3.3, consumed
/// from L9 E15.5; L9 freezes the name, and a change here is a change to this test-pinned string).
pub(crate) const PQ_SUITE: &str = "X25519MLKEM768-HKDF-SHA384-AES256GCM";

/// The state of the hybrid session on one link, as the daemon or the pairing channel reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PqState {
    /// The session completed with the named suite.
    Established { suite: String },
    /// No session was attempted or the peer does not offer one.
    Missing,
    /// The peer refused the session, with the reason it gave.
    Refused(String),
}

/// What a channel that runs libp2p Noise end to end inside `link` (the cluster mesh) is reported
/// as. `None` means there is no private link to run it over, so nothing is protected by one.
/// A session with any suite other than [`PQ_SUITE`] counts as no session.
pub(crate) fn classify(link: LinkKind, pq: &PqState) -> LinkSecurity {
    if link == LinkKind::None {
        return LinkSecurity::None;
    }
    match pq {
        PqState::Established { suite } if suite == PQ_SUITE => LinkSecurity::PqHybridE2e,
        _ => LinkSecurity::ClassicalE2e,
    }
}

#[cfg(test)]
mod tests {
    include!("link_security_tests.rs");
}
