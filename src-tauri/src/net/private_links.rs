//! L7-WP03 (AR-E9.5, F-23): which of this machine's addresses Citrate Core may listen on for
//! pairing (and, from L7-WP10, the mesh): private links only. Pure, fixture-tested.
//!
//! The ranges are exactly the pairing hint filter's (`fleet::hint_ip_allowed`: RFC 1918, CGNAT
//! and tailnet 100.64.0.0/10, link-local, IPv6 unique-local and link-local; never loopback, a
//! public address, an unspecified address or the cloud metadata address), so a listener can never
//! sit on an address a pairing link may not point at. Cloudflare WARP's range sits inside
//! 100.64.0.0/10; telling it apart from a tailnet address is L7-WP17, so until then a CGNAT address
//! is reported as a tailnet one.

use std::net::IpAddr;

use super::link_security::LinkKind;

/// Which kinds of private link may be listened on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PrivateLinkPolicy {
    pub allow_tailnet: bool,
    pub allow_warp: bool,
    pub allow_lan: bool,
}

impl PrivateLinkPolicy {
    /// Tailnet, WARP and the local network: what pairing listens on.
    pub(crate) const ALL: PrivateLinkPolicy = PrivateLinkPolicy {
        allow_tailnet: true,
        allow_warp: true,
        allow_lan: true,
    };

    fn allows(&self, kind: LinkKind) -> bool {
        match kind {
            LinkKind::TailnetOwn | LinkKind::TailnetManaged => self.allow_tailnet,
            LinkKind::Warp => self.allow_warp,
            LinkKind::Lan => self.allow_lan,
            LinkKind::None => false,
        }
    }
}

/// The most addresses listened on at once (the same cap as a pairing link's hints).
pub(crate) const MAX_LISTEN_ADDRS: usize = crate::fleet_pairing::MAX_HINTS;

/// The private link an address belongs to, or `None` for any address that is not on one.
pub(crate) fn is_private_link_ip(ip: IpAddr) -> Option<LinkKind> {
    if !crate::fleet::hint_ip_allowed(ip) {
        return None;
    }
    let tailnet = match ip {
        // 100.64.0.0/10 (CGNAT; Tailscale hands out addresses here).
        IpAddr::V4(v) => v.octets()[0] == 100 && (v.octets()[1] & 0xc0) == 64,
        // fd7a:115c:a1e0::/48, Tailscale's IPv6 range.
        IpAddr::V6(v) => {
            let s = v.segments();
            s[0] == 0xfd7a && s[1] == 0x115c && s[2] == 0xa1e0
        }
    };
    Some(if tailnet {
        LinkKind::TailnetOwn
    } else {
        LinkKind::Lan
    })
}

/// The addresses to listen on: the private-link ones among `candidates` that `policy` allows,
/// deduplicated, in order, capped at [`MAX_LISTEN_ADDRS`]. Never an unspecified, loopback or public
/// address.
pub(crate) fn private_listen_addrs(policy: PrivateLinkPolicy, candidates: &[IpAddr]) -> Vec<IpAddr> {
    let mut out: Vec<IpAddr> = Vec::new();
    for ip in candidates {
        let Some(kind) = is_private_link_ip(*ip) else {
            continue;
        };
        if policy.allows(kind) && !out.contains(ip) && out.len() < MAX_LISTEN_ADDRS {
            out.push(*ip);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    include!("private_links_tests.rs");
}
