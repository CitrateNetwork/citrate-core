// L7-WP03: the private-link address filter. Written red-first: the module body was absent.

use super::*;

fn ips(xs: &[&str]) -> Vec<IpAddr> {
    xs.iter().map(|x| x.parse().expect("ip")).collect()
}

#[test]
fn private_listen_addrs_includes_tailnet_warp_lan_only() {
    let candidates = ips(&[
        "0.0.0.0",
        "::",
        "127.0.0.1",
        "::1",
        "8.8.8.8",
        "2001:db8::1",
        "169.254.169.254",
        "192.168.1.5",
        "10.0.0.7",
        "172.16.3.4",
        "100.64.1.10",
        "100.100.0.9",
        "fd00::1",
        "fd7a:115c:a1e0::5",
    ]);
    // Seven private addresses in order; the cap keeps the first six.
    assert_eq!(MAX_LISTEN_ADDRS, 6);
    assert_eq!(
        private_listen_addrs(PrivateLinkPolicy::ALL, &candidates),
        ips(&[
            "192.168.1.5",
            "10.0.0.7",
            "172.16.3.4",
            "100.64.1.10",
            "100.100.0.9",
            "fd00::1",
        ])
    );
}

#[test]
fn the_link_kind_of_each_private_range() {
    for (ip, want) in [
        ("192.168.1.5", Some(LinkKind::Lan)),
        ("10.1.2.3", Some(LinkKind::Lan)),
        ("172.31.0.1", Some(LinkKind::Lan)),
        ("169.254.10.1", Some(LinkKind::Lan)),
        ("fd00::1", Some(LinkKind::Lan)),
        ("fe80::1", Some(LinkKind::Lan)),
        ("100.64.0.1", Some(LinkKind::TailnetOwn)),
        ("100.127.255.254", Some(LinkKind::TailnetOwn)),
        ("fd7a:115c:a1e0::1", Some(LinkKind::TailnetOwn)),
        ("100.128.0.1", None),
        ("8.8.8.8", None),
        ("127.0.0.1", None),
        ("0.0.0.0", None),
        ("169.254.169.254", None),
        ("2001:4860:4860::8888", None),
        ("::", None),
    ] {
        assert_eq!(is_private_link_ip(ip.parse().expect("ip")), want, "{ip}");
    }
}

#[test]
fn the_filter_agrees_with_the_pairing_hint_filter() {
    // Every address a listener may sit on is one a pairing link may point at, and the reverse.
    let mut n = 0u32;
    for a in [0u8, 10, 100, 127, 169, 172, 192, 224, 255] {
        for b in [0u8, 16, 31, 64, 100, 127, 128, 168, 254, 255] {
            for d in [0u8, 1, 254] {
                let ip = IpAddr::V4(std::net::Ipv4Addr::new(a, b, 169, d));
                assert_eq!(
                    is_private_link_ip(ip).is_some(),
                    crate::fleet::hint_ip_allowed(ip),
                    "{ip}"
                );
                n += 1;
            }
        }
    }
    for s0 in [0u16, 0x2001, 0xfc00, 0xfd7a, 0xfe80, 0xfec0, 0xff02] {
        let ip = IpAddr::V6(std::net::Ipv6Addr::new(s0, 0x115c, 0xa1e0, 0, 0, 0, 0, 1));
        assert_eq!(
            is_private_link_ip(ip).is_some(),
            crate::fleet::hint_ip_allowed(ip),
            "{ip}"
        );
        n += 1;
    }
    assert!(n > 250);
}

#[test]
fn the_policy_filters_by_kind() {
    let candidates = ips(&["192.168.1.5", "100.64.1.10"]);
    let tailnet_only = PrivateLinkPolicy {
        allow_tailnet: true,
        allow_warp: false,
        allow_lan: false,
    };
    assert_eq!(
        private_listen_addrs(tailnet_only, &candidates),
        ips(&["100.64.1.10"])
    );
    let lan_only = PrivateLinkPolicy {
        allow_tailnet: false,
        allow_warp: false,
        allow_lan: true,
    };
    assert_eq!(
        private_listen_addrs(lan_only, &candidates),
        ips(&["192.168.1.5"])
    );
}

#[test]
fn listen_addrs_are_deduplicated_and_capped() {
    let many: Vec<IpAddr> = (1..=20)
        .flat_map(|i| {
            let ip = IpAddr::V4(std::net::Ipv4Addr::new(192, 168, 1, i));
            [ip, ip]
        })
        .collect();
    let got = private_listen_addrs(PrivateLinkPolicy::ALL, &many);
    assert_eq!(got.len(), MAX_LISTEN_ADDRS);
    assert_eq!(got[0], ips(&["192.168.1.1"])[0]);
    assert_eq!(got[1], ips(&["192.168.1.2"])[0]);
}
