//! L6-WP07: the verified brand servers pinned in the binary. Each row comes from the provider
//! matrix (citrate-federation `.agentile/planset/2026-11-07-v060/evidence/
//! L6-WP07_MCP_PROVIDER_MATRIX_2026-11-28.md`), status `supported` only.
use super::brand::{Auth, Client, BRAND_SERVERS};
use std::collections::BTreeSet;

/// The domains each brand publishes its server and docs on, written apart from the table so a
/// URL typo or a look-alike host in the table fails here.
fn brand_domains(id: &str) -> &'static [&'static str] {
    match id {
        "notion" => &["notion.com"],
        "linear" => &["linear.app"],
        "vercel" => &["vercel.com"],
        "atlassian" => &["atlassian.com"],
        "huggingface" => &["huggingface.co"],
        "stripe" => &["stripe.com"],
        "cloudflare" | "cloudflare-bindings" | "cloudflare-observability" | "cloudflare-docs" => {
            &["cloudflare.com"]
        }
        "sentry" => &["sentry.dev", "sentry.io"],
        "supabase" => &["supabase.com"],
        "neon" => &["neon.tech", "neon.com"],
        _ => &[],
    }
}

fn host(url: &str) -> &str {
    url.strip_prefix("https://")
        .and_then(|r| r.split(['/', '?', '#']).next())
        .unwrap_or("")
}

fn on_domain(h: &str, domains: &[&str]) -> bool {
    domains
        .iter()
        .any(|d| h == *d || h.ends_with(&format!(".{d}")))
}

#[test]
fn every_entry_is_https_on_the_brands_domain() {
    for s in BRAND_SERVERS {
        let domains = brand_domains(s.id);
        assert!(!domains.is_empty(), "{}: no brand domain recorded", s.id);
        assert!(s.url.starts_with("https://"), "{}: {}", s.id, s.url);
        let h = host(s.url);
        assert!(
            !h.is_empty() && !h.contains('@') && !h.contains(':'),
            "{}: host {h:?}",
            s.id
        );
        assert!(on_domain(h, domains), "{}: {h} is not on {domains:?}", s.id);
        assert!(
            s.url.bytes().all(|c| c.is_ascii_graphic()),
            "{}: url has spaces or non-ASCII",
            s.id
        );
    }
}

#[test]
fn every_entry_has_source_and_checked_date() {
    for s in BRAND_SERVERS {
        assert!(s.source.starts_with("https://"), "{}: source {}", s.id, s.source);
        assert!(
            on_domain(host(s.source), brand_domains(s.id)),
            "{}: the source {} is not the publisher's own page",
            s.id,
            s.source
        );
        let b = s.checked.as_bytes();
        assert!(
            b.len() == 10
                && b[4] == b'-'
                && b[7] == b'-'
                && b.iter()
                    .enumerate()
                    .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit()),
            "{}: checked {:?} is not YYYY-MM-DD",
            s.id,
            s.checked
        );
        // The matrix probes ran 2026-11-28; an older date means a row was not re-checked.
        assert!(s.checked >= "2026-11-28", "{}: checked {}", s.id, s.checked);
        for text in [s.name, s.publisher, s.summary] {
            assert!(
                !text.trim().is_empty() && text.chars().count() <= 200,
                "{}: {text:?}",
                s.id
            );
            assert!(!text.contains('\u{2014}'), "{}: no em-dashes in copy", s.id);
        }
    }
}

#[test]
fn no_duplicate_ids_or_urls() {
    let mut ids = BTreeSet::new();
    let mut urls = BTreeSet::new();
    for s in BRAND_SERVERS {
        assert!(ids.insert(s.id), "duplicate id {}", s.id);
        assert!(
            urls.insert(s.url.trim_end_matches('/')),
            "duplicate url {}",
            s.url
        );
    }
}

#[test]
fn auth_mode_declared() {
    for s in BRAND_SERVERS {
        match (s.auth, s.client) {
            (Auth::None, Client::NotNeeded) => {}
            (Auth::Oauth, Client::Cimd | Client::CimdThenDcr | Client::Dcr) => {}
            (a, c) => panic!("{}: auth {a:?} with client {c:?}", s.id),
        }
    }
    // Only the Cloudflare docs server answers without sign-in (matrix: "docs. (no auth)").
    let open: Vec<&str> = BRAND_SERVERS
        .iter()
        .filter(|s| s.auth == Auth::None)
        .map(|s| s.id)
        .collect();
    assert_eq!(open, ["cloudflare-docs"]);
}

#[test]
fn write_tools_are_off_by_default() {
    for s in BRAND_SERVERS {
        assert!(!s.write_tools_default, "{}: write tools on by default", s.id);
    }
}

#[test]
fn count_is_the_supported_rows_of_the_matrix() {
    // The lane file asked for 20 to 30 entries. The matrix verified 13 rows as `supported`
    // without a pending registration; WP07 says an entry that cannot be verified is left out,
    // never guessed. The floor rises as rows clear (matrix "Counts for WP07").
    let ids: BTreeSet<&str> = BRAND_SERVERS.iter().map(|s| s.id).collect();
    let want: BTreeSet<&str> = [
        "notion",
        "linear",
        "vercel",
        "atlassian",
        "huggingface",
        "stripe",
        "cloudflare",
        "cloudflare-bindings",
        "cloudflare-observability",
        "cloudflare-docs",
        "sentry",
        "supabase",
        "neon",
    ]
    .into_iter()
    .collect();
    assert_eq!(ids, want);
    assert!((10..=30).contains(&BRAND_SERVERS.len()));
}

#[test]
fn rows_that_are_not_supported_are_left_out() {
    // approval-dependent (Slack, Gmail, Drive, Figma, Asana, HubSpot), supported only after an
    // owner registration (GitHub after WP13, X after its redirect), unverified (PayPal, Discord),
    // unsupported (LinkedIn) and deferred (Microsoft Work IQ).
    let banned = [
        "slack.com",
        "githubcopilot.com",
        "googleapis.com",
        "figma.com",
        "asana.com",
        "hubspot.com",
        "x.com",
        "paypal.com",
        "discord.com",
        "linkedin.com",
        "cloud.microsoft",
    ];
    for s in BRAND_SERVERS {
        assert!(!on_domain(host(s.url), &banned), "{}: {}", s.id, s.url);
    }
}

#[test]
fn ids_do_not_collide_with_reserved_names() {
    // Each id becomes the name of a user-added server on install, so it must pass the same form
    // check a member's entry does: a valid name, not a reserved one (mcp_servers.rs
    // RESERVED_SERVER_NAMES), and a URL the http transport accepts.
    for s in BRAND_SERVERS {
        let input = crate::mcp_servers::ServerInput {
            name: s.id.to_string(),
            transport: "http".to_string(),
            command: None,
            args: Vec::new(),
            cwd: None,
            url: Some(s.url.to_string()),
            env: Vec::new(),
            allow_write_tools: false,
            previous_name: None,
        };
        let res = crate::mcp_servers::validate_input(&input);
        assert!(res.is_ok(), "{}: {:?}", s.id, res.err());
    }
    // Negative control: the check does refuse a reserved name.
    let reserved = crate::mcp_servers::ServerInput {
        name: "citrate".to_string(),
        transport: "http".to_string(),
        command: None,
        args: Vec::new(),
        cwd: None,
        url: Some("https://mcp.notion.com/mcp".to_string()),
        env: Vec::new(),
        allow_write_tools: false,
        previous_name: None,
    };
    assert!(crate::mcp_servers::validate_input(&reserved).is_err());
}
