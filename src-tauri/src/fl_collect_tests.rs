// WP-H8: the compliant collector. The network is a fixed table (a test seam; production is
// LiveHttp), the pause records what it was asked to wait, and the dataset store is the real
// store in a scratch folder under an in-memory keyring.

use super::*;
use crate::custody::{CustodyError, Keyring};
use crate::fl_datasets::{ingest_folder, CreateRequest, CreateSource};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex as StdMutex;

#[derive(Default)]
struct MemKeyring(StdMutex<HashMap<String, Vec<u8>>>);

impl Keyring for MemKeyring {
    fn get(&self, a: &str) -> std::result::Result<Option<Vec<u8>>, CustodyError> {
        Ok(self.0.lock().unwrap().get(a).cloned())
    }
    fn set(&self, a: &str, s: &[u8]) -> std::result::Result<(), CustodyError> {
        self.0.lock().unwrap().insert(a.to_string(), s.to_vec());
        Ok(())
    }
    fn delete(&self, a: &str) -> std::result::Result<(), CustodyError> {
        self.0.lock().unwrap().remove(a);
        Ok(())
    }
}

/// A fixed web: URL -> answer. Anything else is a failed request. Records every URL asked for,
/// the addresses each fetch was pinned to, and every DNS lookup. `dns` answers in order (the last
/// answer repeats); a host not in it resolves to one public address.
#[derive(Default)]
struct Web {
    pages: StdMutex<HashMap<String, Vec<Fetched>>>,
    asked: StdMutex<Vec<String>>,
    pins: StdMutex<Vec<Vec<std::net::IpAddr>>>,
    dns: StdMutex<HashMap<String, Vec<Vec<std::net::IpAddr>>>>,
    lookups: StdMutex<Vec<String>>,
}

const PUBLIC_IP: std::net::IpAddr = std::net::IpAddr::V4(std::net::Ipv4Addr::new(93, 184, 215, 14));
const PRIVATE_IP: std::net::IpAddr = std::net::IpAddr::V4(std::net::Ipv4Addr::new(10, 0, 0, 7));

impl Web {
    fn page(self, url: &str, status: u16, ctype: &str, body: &str) -> Self {
        self.pages.lock().unwrap().entry(url.to_string()).or_default().push(Fetched {
            status,
            content_type: ctype.to_string(),
            retry_after: None,
            body: body.as_bytes().to_vec(),
        });
        self
    }
    fn answer(self, url: &str, f: Fetched) -> Self {
        self.pages.lock().unwrap().entry(url.to_string()).or_default().push(f);
        self
    }
    fn dns(self, host: &str, answers: Vec<Vec<std::net::IpAddr>>) -> Self {
        self.dns.lock().unwrap().insert(host.to_string(), answers);
        self
    }
    fn asked(&self) -> Vec<String> {
        self.asked.lock().unwrap().clone()
    }
    fn pages_asked(&self) -> Vec<String> {
        self.asked().into_iter().filter(|u| !u.ends_with("/robots.txt")).collect()
    }
}

impl CollectHttp for Web {
    fn resolve(&self, host: &str) -> Result<Vec<std::net::IpAddr>, String> {
        self.lookups.lock().unwrap().push(host.to_string());
        let mut dns = self.dns.lock().unwrap();
        match dns.get_mut(host) {
            Some(v) if v.len() > 1 => Ok(v.remove(0)),
            Some(v) if v.len() == 1 => Ok(v[0].clone()),
            _ => Ok(vec![PUBLIC_IP]),
        }
    }
    fn get(&self, url: &url::Url, pinned: &[std::net::IpAddr], _limit: u64) -> Result<Fetched, String> {
        self.asked.lock().unwrap().push(url.to_string());
        self.pins.lock().unwrap().push(pinned.to_vec());
        let mut pages = self.pages.lock().unwrap();
        match pages.get_mut(url.as_str()) {
            // Answers are used in order; the last one repeats.
            Some(v) if v.len() > 1 => Ok(v.remove(0)),
            Some(v) if v.len() == 1 => Ok(v[0].clone()),
            _ => Err("the request failed".into()),
        }
    }
}

#[derive(Default)]
struct Waits(StdMutex<Vec<Duration>>);
impl Pause for Waits {
    fn wait(&self, d: Duration) {
        self.0.lock().unwrap().push(d);
    }
}

const ARTICLE: &str = "<html><head><style>p{}</style><script>track()</script></head><body><h1>Apple</h1><p>An apple is the edible fruit of the apple tree, cultivated worldwide.</p><p>Apple trees are pruned in late winter, above an outward-facing bud.</p></body></html>";
const ROBOTS_OPEN: &str = "User-agent: *\nDisallow: /private/\n";

fn policy() -> SourcePolicy {
    SourcePolicy::bundled().expect("bundled policy")
}

fn web() -> Web {
    Web::default()
        .page(
            "https://en.wikipedia.org/robots.txt",
            200,
            "text/plain",
            ROBOTS_OPEN,
        )
        .page(
            "https://en.wikipedia.org/wiki/Apple",
            200,
            "text/html; charset=utf-8",
            ARTICLE,
        )
        .page(
            "https://en.wikipedia.org/wiki/Pear",
            200,
            "text/html",
            ARTICLE,
        )
}

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("fl-collect-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A dataset to collect into (v1 from a granted folder).
fn dataset(s: &DatasetStore<'_>, tag: &str) -> (String, u32) {
    let root = tmp(tag).join("notes");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("a.txt"), "Prune apple trees in late winter.\n").unwrap();
    let out = s
        .create(
            &CreateRequest {
                name: "Orchard".into(),
                data_class: "orchard-notes".into(),
                source: CreateSource::Folder {
                    path: String::new(),
                },
                consent_statement: "Use the files in notes.".into(),
            },
            ingest_folder(&root, 1_000).unwrap(),
            1_000,
        )
        .unwrap();
    (out.version.dataset.clone(), out.version.version)
}

fn clock() -> u64 {
    5_000
}

// ---- the policy ------------------------------------------------------------------------------

#[test]
fn the_bundled_policy_is_versioned_curated_and_valid() {
    let p = policy();
    assert!(p.version >= 1);
    assert!(p.curator.contains("Citrate"));
    assert!(p.allowed("en.wikipedia.org").is_some());
    for h in [
        "www.linkedin.com",
        "spokeo.com",
        "mail.google.com",
        "x.com",
        "nytimes.com",
    ] {
        assert!(p.denied(h).is_some(), "{h}");
    }
    assert!(p.denied("example.org").is_none() && p.allowed("example.org").is_none());
    // A parent domain does not match a look-alike.
    assert!(p.denied("notx.com").is_none());
}

#[test]
fn a_bad_policy_is_refused() {
    let mut p = policy();
    p.deny.push(DenyEntry {
        host: "en.wikipedia.org".into(),
        reason: "twice".into(),
    });
    assert!(p.check().is_err());
    let mut p = policy();
    p.allow[0].license = "NOASSERTION".into();
    assert!(p.check().is_err());
}

// ---- URLs and robots.txt ------------------------------------------------------------------------

#[test]
fn urls_that_are_local_credentialed_or_not_web_are_refused() {
    for bad in [
        "ftp://example.org/a",
        "file:///etc/passwd",
        "https://user:pw@example.org/",
        "https://127.0.0.1/",
        "https://[::1]/",
        "http://localhost/",
        "https://printer.local/",
        "https://example.org:8443/",
        "https://example.org/a?access_token=abc",
        "https://example.org/a?Session=abc",
        "not a url",
    ] {
        assert!(check_url(bad).is_err(), "{bad}");
    }
    assert_eq!(
        check_url("https://Example.org/a?q=1").unwrap().1,
        "example.org"
    );
}

#[test]
fn robots_rules_for_citrate_hermes_or_the_wildcard_group() {
    let r = Robots::parse("User-agent: *\nDisallow: /\n\nUser-agent: CitrateHermes\nAllow: /wiki/\nDisallow: /wiki/Special:\nCrawl-delay: 5\n");
    assert!(r.allows("/wiki/Apple"));
    assert!(!r.allows("/wiki/Special:Random"));
    assert!(r.allows("/other"), "no rule of our group matches: allowed");
    assert_eq!(r.crawl_delay, Some(Duration::from_secs(5)));
    let star = Robots::parse("User-agent: Googlebot\nDisallow: /\n\nUser-agent: *\nDisallow: /private/\nDisallow: /*.pdf$\n");
    assert!(star.allows("/public/a"));
    assert!(!star.allows("/private/a"));
    assert!(!star.allows("/docs/a.pdf"));
    assert!(star.allows("/docs/a.pdf?x=1"));
    // Allow wins a tie of equal length.
    let tie = Robots::parse("User-agent: *\nDisallow: /a\nAllow: /a\n");
    assert!(tie.allows("/a"));
    assert!(Robots::parse("").allows("/anything"));
}

// ---- source checks -----------------------------------------------------------------------------

#[test]
fn an_allowlisted_page_robots_allows_is_collectable_with_its_license() {
    let w = web();
    let c = check_source(
        &w,
        &policy(),
        &mut RobotsCache::default(),
        "https://en.wikipedia.org/wiki/Apple",
        None,
    );
    assert!(c.allowed, "{c:?}");
    assert_eq!(c.policy, PolicyClass::Allowlisted);
    assert_eq!(c.robots, RobotsState::Allowed);
    assert_eq!(c.license.id.as_deref(), Some("CC-BY-SA-4.0"));
    assert_eq!(c.license.basis, LicenseBasis::Found);
    assert_eq!(c.interval_secs, 2);
}

#[test]
fn a_page_robots_disallows_is_refused() {
    let w = web();
    let c = check_source(
        &w,
        &policy(),
        &mut RobotsCache::default(),
        "https://en.wikipedia.org/private/x",
        None,
    );
    assert!(!c.allowed);
    assert_eq!(c.robots, RobotsState::Disallowed);
    assert!(c.refusal.unwrap().contains("disallows"));
}

#[test]
fn no_robots_txt_and_no_license_is_not_permission() {
    let w = Web::default()
        .page("https://example.org/robots.txt", 404, "text/html", "")
        .page("https://example.org/a", 200, "text/html", ARTICLE);
    let c = check_source(
        &w,
        &policy(),
        &mut RobotsCache::default(),
        "https://example.org/a",
        None,
    );
    assert!(!c.allowed);
    assert_eq!(c.robots, RobotsState::Missing);
    assert_eq!(
        c.refusal.as_deref(),
        Some("example.org has no robots.txt and no license is stated. Hermes will not collect it.")
    );
    // With the member's attested license, the same page may be planned (the member still taps).
    let at = Attestation {
        license: "CC-BY-4.0".into(),
        statement: "The page footer says CC BY 4.0.".into(),
    };
    let c = check_source(
        &w,
        &policy(),
        &mut RobotsCache::default(),
        "https://example.org/a",
        Some(&at),
    );
    assert!(c.allowed, "{c:?}");
    assert_eq!(c.policy, PolicyClass::NeedsMemberApproval);
    assert_eq!(c.license.basis, LicenseBasis::MemberAttested);
    // An attestation needs words and a license DatasetRegistry accepts.
    for bad in [
        Attestation {
            license: "unknown".into(),
            statement: "x".into(),
        },
        Attestation {
            license: "MIT".into(),
            statement: " ".into(),
        },
    ] {
        assert!(
            !check_source(
                &w,
                &policy(),
                &mut RobotsCache::default(),
                "https://example.org/a",
                Some(&bad)
            )
            .allowed
        );
    }
}

#[test]
fn an_unreadable_or_forbidden_robots_txt_refuses_the_host() {
    for status in [401u16, 403, 500] {
        let w = Web::default().page("https://example.org/robots.txt", status, "text/plain", "");
        let at = Attestation {
            license: "MIT".into(),
            statement: "README says MIT.".into(),
        };
        let c = check_source(
            &w,
            &policy(),
            &mut RobotsCache::default(),
            "https://example.org/a",
            Some(&at),
        );
        assert!(!c.allowed, "{status}");
        assert_eq!(c.robots, RobotsState::Unreadable);
    }
}

#[test]
fn denied_and_private_hosts_are_refused_before_any_fetch() {
    let w = Web::default().dns("intranet.example.org", vec![vec![PRIVATE_IP]]);
    for (url, why) in [
        ("https://www.linkedin.com/in/someone", "do-not-collect list"),
        ("https://www.spokeo.com/search?q=x", "people-search"),
        ("https://mail.google.com/mail/u/0", "private messages"),
        ("https://intranet.example.org/a", "local or private"),
    ] {
        let c = check_source(&w, &policy(), &mut RobotsCache::default(), url, None);
        assert!(!c.allowed, "{url}");
        assert!(
            c.refusal.as_deref().unwrap_or("").contains(why),
            "{url}: {c:?}"
        );
    }
    assert!(w.asked().is_empty(), "nothing was fetched: {:?}", w.asked());
}

#[test]
fn a_crawl_delay_longer_than_two_seconds_slows_the_host_and_an_extreme_one_refuses_it() {
    let w = Web::default().page(
        "https://en.wikipedia.org/robots.txt",
        200,
        "text/plain",
        "User-agent: *\nCrawl-delay: 7\n",
    );
    let c = check_source(
        &w,
        &policy(),
        &mut RobotsCache::default(),
        "https://en.wikipedia.org/wiki/Apple",
        None,
    );
    assert!(c.allowed);
    assert_eq!(c.interval_secs, 7);
    let w = Web::default().page(
        "https://en.wikipedia.org/robots.txt",
        200,
        "text/plain",
        "User-agent: *\nCrawl-delay: 600\n",
    );
    assert!(
        !check_source(
            &w,
            &policy(),
            &mut RobotsCache::default(),
            "https://en.wikipedia.org/wiki/Apple",
            None
        )
        .allowed
    );
}

// ---- plans and runs ----------------------------------------------------------------------------

fn urls(u: &[&str]) -> Vec<String> {
    u.iter().map(|s| s.to_string()).collect()
}

#[test]
fn a_plan_fetches_no_page_and_a_run_needs_the_members_approval() {
    let kr = MemKeyring::default();
    let s = DatasetStore::new(tmp("plan-store"), &kr);
    let (id, v) = dataset(&s, "plan");
    let w = web();
    let p = plan(
        &w,
        &policy(),
        &id,
        v,
        &urls(&[
            "https://en.wikipedia.org/wiki/Apple",
            "https://www.linkedin.com/in/x",
        ]),
        None,
        1,
    )
    .unwrap();
    assert_eq!(p.pages, 1);
    assert_eq!(p.status, PlanStatus::Proposed);
    assert!(
        w.pages_asked().is_empty(),
        "planning reads robots.txt only: {:?}",
        w.asked()
    );
    assert!(p.statement.contains("Collect 1 page from en.wikipedia.org"));
    // Not approved: nothing runs.
    let err = run_plan(&w, &Waits::default(), &policy(), &s, &p.id, &clock).unwrap_err();
    assert!(err.contains("not approved"), "{err}");
    assert!(w.pages_asked().is_empty());
    // Declined: still nothing.
    decline(&p.id).unwrap();
    assert!(approve(&p.id).is_err());
    assert!(run_plan(&w, &Waits::default(), &policy(), &s, &p.id, &clock).is_err());
}

#[test]
fn an_approved_run_writes_provenance_through_the_store_and_records_the_scan() {
    let kr = MemKeyring::default();
    let s = DatasetStore::new(tmp("run-store"), &kr);
    let (id, v) = dataset(&s, "run");
    let w = web();
    let waits = Waits::default();
    let p = plan(
        &w,
        &policy(),
        &id,
        v,
        &urls(&[
            "https://en.wikipedia.org/wiki/Apple",
            "https://en.wikipedia.org/wiki/Pear",
        ]),
        None,
        2,
    )
    .unwrap();
    approve(&p.id).unwrap();
    let r = run_plan(&w, &waits, &policy(), &s, &p.id, &clock).unwrap();
    assert!(r.untrusted);
    assert_eq!(r.pages_fetched, 2);
    assert!(r.stopped.is_none());
    let version = r.version.clone().expect("a version");
    assert_eq!(version.version, v + 1);
    let (m, rows) = s.rows_of(&id, version.version).unwrap();
    let web_sources: Vec<_> = m
        .sources
        .iter()
        .filter(|s| s.kind == SourceKind::Web)
        .collect();
    assert_eq!(web_sources.len(), 2);
    for src in &web_sources {
        assert!(src.locator.starts_with("https://en.wikipedia.org/wiki/"));
        assert_eq!(src.collector_version, COLLECTOR_VERSION);
        assert_eq!(src.robots, RobotsVerdict::Allowed);
        assert_eq!(src.tos_class, TosClass::Allowlisted);
        assert_eq!(src.license.id.as_deref(), Some("CC-BY-SA-4.0"));
        assert!(src.untrusted);
        assert!(crate::fl_datasets::is_hash(&src.content_sha256));
        assert_eq!(src.fetched_at_ms, 5_000);
        assert!(!src.consent_ref.is_empty());
    }
    let texts: Vec<String> = rows
        .iter()
        .filter_map(|r| r.data["text"].as_str().map(str::to_string))
        .collect();
    assert!(
        texts
            .iter()
            .any(|t| t.contains("edible fruit of the apple tree")),
        "{texts:?}"
    );
    assert!(
        texts
            .iter()
            .all(|t| !t.contains("track()") && !t.contains("p{}")),
        "scripts and styles are dropped"
    );
    // The consent the store recorded is the plan's statement.
    let consents = s.consent_list(&id).unwrap();
    assert!(consents
        .iter()
        .any(|c| c.statement == p.statement && c.source_kind == SourceKind::Web));
    // The new version was scanned and the record names its rows.
    assert!(r.pii_scan.as_ref().unwrap().passed);
    let rec = crate::fl_dataset_gate::recorded_scan(&s, &id, version.version)
        .unwrap()
        .unwrap();
    assert_eq!(rec.content_hash, m.content_hash);
    // Codex review of #738: robots.txt is a request too. The run read robots.txt, waited the
    // 2 s interval before the first page, and again before the second.
    let waited = waits.0.lock().unwrap().clone();
    assert_eq!(waited.len(), 2, "{waited:?}");
    for w in &waited {
        assert!(
            *w > Duration::from_millis(1900) && *w <= MIN_INTERVAL,
            "{waited:?}"
        );
    }
    // A plan runs once.
    assert!(run_plan(&w, &waits, &policy(), &s, &p.id, &clock).is_err());
}

#[test]
fn a_429_or_403_stops_the_run_and_nothing_more_is_fetched() {
    for status in [429u16, 403] {
        let kr = MemKeyring::default();
        let s = DatasetStore::new(tmp(&format!("stop-{status}")), &kr);
        let (id, v) = dataset(&s, &format!("stop{status}"));
        let w = Web::default()
            .page(
                "https://en.wikipedia.org/robots.txt",
                200,
                "text/plain",
                ROBOTS_OPEN,
            )
            .page(
                "https://en.wikipedia.org/wiki/Apple",
                200,
                "text/html",
                ARTICLE,
            )
            .page(
                "https://en.wikipedia.org/wiki/Pear",
                status,
                "text/html",
                "",
            )
            .page(
                "https://en.wikipedia.org/wiki/Banana",
                200,
                "text/html",
                ARTICLE,
            );
        let p = plan(
            &w,
            &policy(),
            &id,
            v,
            &urls(&[
                "https://en.wikipedia.org/wiki/Apple",
                "https://en.wikipedia.org/wiki/Pear",
                "https://en.wikipedia.org/wiki/Banana",
            ]),
            None,
            3 + u64::from(status),
        )
        .unwrap();
        approve(&p.id).unwrap();
        let r = run_plan(&w, &Waits::default(), &policy(), &s, &p.id, &clock).unwrap();
        assert!(
            r.stopped.as_deref().unwrap().contains(&status.to_string()),
            "{r:?}"
        );
        assert!(
            !w.pages_asked().iter().any(|u| u.ends_with("/Banana")),
            "{:?}",
            w.asked()
        );
        assert_eq!(
            r.outcomes[2].note.as_deref(),
            Some("not fetched: the run stopped")
        );
        // What was collected before the stop is kept, with provenance.
        assert_eq!(r.version.as_ref().unwrap().version, v + 1);
    }
}

#[test]
fn a_503_with_retry_after_waits_once_and_a_sign_in_page_is_never_collected() {
    let kr = MemKeyring::default();
    let s = DatasetStore::new(tmp("retry-store"), &kr);
    let (id, v) = dataset(&s, "retry");
    let w = web()
        .answer(
            "https://en.wikipedia.org/wiki/Plum",
            Fetched {
                status: 503,
                content_type: "text/html".into(),
                retry_after: Some(Duration::from_secs(9)),
                body: vec![],
            },
        )
        .page(
            "https://en.wikipedia.org/wiki/Plum",
            200,
            "text/html",
            ARTICLE,
        )
        .page("https://en.wikipedia.org/wiki/Login", 401, "text/html", "");
    let waits = Waits::default();
    let p = plan(
        &w,
        &policy(),
        &id,
        v,
        &urls(&[
            "https://en.wikipedia.org/wiki/Plum",
            "https://en.wikipedia.org/wiki/Login",
        ]),
        None,
        4,
    )
    .unwrap();
    approve(&p.id).unwrap();
    let r = run_plan(&w, &waits, &policy(), &s, &p.id, &clock).unwrap();
    assert!(r.outcomes[0].collected, "{r:?}");
    assert!(waits.0.lock().unwrap().contains(&Duration::from_secs(9)));
    assert!(!r.outcomes[1].collected);
    assert!(r.outcomes[1].note.as_deref().unwrap().contains("sign-in"));
}

#[test]
fn rows_holding_a_key_or_seed_phrase_are_dropped() {
    let kr = MemKeyring::default();
    let s = DatasetStore::new(tmp("secret-store"), &kr);
    let (id, v) = dataset(&s, "secret");
    let key = ["0x", &"ab".repeat(32)].concat();
    let page = format!("<p>An apple is the edible fruit of the apple tree.</p><p>Deploy with private key {key} on mainnet today.</p>");
    let w = web().page(
        "https://en.wikipedia.org/wiki/Quince",
        200,
        "text/html",
        &page,
    );
    let p = plan(
        &w,
        &policy(),
        &id,
        v,
        &urls(&["https://en.wikipedia.org/wiki/Quince"]),
        None,
        5,
    )
    .unwrap();
    approve(&p.id).unwrap();
    let r = run_plan(&w, &Waits::default(), &policy(), &s, &p.id, &clock).unwrap();
    assert_eq!(r.outcomes[0].dropped_secret_rows, 1);
    let (_, rows) = s.rows_of(&id, r.version.unwrap().version).unwrap();
    assert!(rows.iter().all(|r| !r.data.to_string().contains(&key)));
}

#[test]
fn robots_is_checked_again_at_run_time() {
    let kr = MemKeyring::default();
    let s = DatasetStore::new(tmp("recheck-store"), &kr);
    let (id, v) = dataset(&s, "recheck");
    // Open when planned, closed by the time the run starts.
    let w = Web::default()
        .page(
            "https://en.wikipedia.org/robots.txt",
            200,
            "text/plain",
            ROBOTS_OPEN,
        )
        .page(
            "https://en.wikipedia.org/robots.txt",
            200,
            "text/plain",
            "User-agent: *\nDisallow: /\n",
        )
        .page(
            "https://en.wikipedia.org/wiki/Apple",
            200,
            "text/html",
            ARTICLE,
        );
    let p = plan(
        &w,
        &policy(),
        &id,
        v,
        &urls(&["https://en.wikipedia.org/wiki/Apple"]),
        None,
        6,
    )
    .unwrap();
    assert_eq!(p.pages, 1);
    approve(&p.id).unwrap();
    let r = run_plan(&w, &Waits::default(), &policy(), &s, &p.id, &clock).unwrap();
    assert!(r.version.is_none());
    assert!(w.pages_asked().is_empty(), "{:?}", w.asked());
    assert!(r.outcomes[0].note.as_deref().unwrap().contains("disallows"));
}

#[test]
fn a_plan_is_capped_at_500_pages_and_never_follows_links() {
    let many: Vec<String> = (0..=MAX_PAGES_PER_RUN)
        .map(|i| format!("https://en.wikipedia.org/wiki/P{i}"))
        .collect();
    assert!(plan(&web(), &policy(), "ds-0123456789abcdef", 1, &many, None, 7).is_err());
    let text = page_text("text/html", b"<p>See <a href=\"https://en.wikipedia.org/wiki/Pear\">pears</a> for more about this fruit and its trees.</p>");
    assert_eq!(
        text,
        vec!["See pears for more about this fruit and its trees.".to_string()]
    );
}

// ---- bypass closure (gap §3.2) -----------------------------------------------------------------

#[test]
fn bypass_closure_the_store_is_out_of_reach_of_the_agents_file_tools() {
    // The store lives under core app data, which citrate-agent-guard denies to every agent file
    // tool (prefix rule `ai.citrate.core`); datasets_dir refuses to open anywhere else. (The
    // Windows form is checked by the guard's own tests: a `\\` path has no components here.)
    for p in [
        "/Users/m/Library/Application Support/ai.citrate.core/datasets",
        "/home/m/.local/share/ai.citrate.core/datasets",
    ] {
        assert!(
            crate::fl_datasets::dataset_root_is_agent_denied(std::path::Path::new(p)),
            "{p}"
        );
    }
    assert!(!crate::fl_datasets::dataset_root_is_agent_denied(
        std::path::Path::new("/Users/m/Documents/datasets")
    ));
}

#[test]
fn bypass_closure_web_items_reach_a_dataset_only_with_collector_provenance() {
    // What the store refuses from anything but the collector: a web item without robots, terms
    // or an http(s) locator, and folder or conversation items through append.
    let kr = MemKeyring::default();
    let s = DatasetStore::new(tmp("bypass-store"), &kr);
    let (id, v) = dataset(&s, "bypass");
    let item =
        |kind: SourceKind, locator: &str, robots: RobotsVerdict, tos: TosClass| CollectedItem {
            kind,
            locator: locator.into(),
            fetched_at_ms: 1,
            content_sha256: format!("0x{}", "cd".repeat(32)),
            license: LicenseRecord::unknown(),
            robots,
            tos_class: tos,
            collector_version: COLLECTOR_VERSION.into(),
            rows: vec![serde_json::json!({"text": "x"})],
        };
    let consent = || ConsentInput::New {
        source_kind: SourceKind::Web,
        scope: "x".into(),
        statement: "x".into(),
    };
    for bad in [
        item(
            SourceKind::Web,
            "https://example.org/a",
            RobotsVerdict::NotApplicable,
            TosClass::MemberApproved,
        ),
        item(
            SourceKind::Web,
            "https://example.org/a",
            RobotsVerdict::Allowed,
            TosClass::NotApplicable,
        ),
        item(
            SourceKind::Web,
            "file:///etc/passwd",
            RobotsVerdict::Allowed,
            TosClass::MemberApproved,
        ),
    ] {
        assert!(s.append(&id, v, consent(), vec![bad], 1).is_err());
    }
}

#[test]
fn the_collector_identifies_itself_with_a_contact_url() {
    assert!(USER_AGENT.starts_with(UA_TOKEN));
    assert!(USER_AGENT.contains("https://"));
    assert_eq!(MIN_INTERVAL, Duration::from_secs(2));
    assert_eq!(MAX_PAGES_PER_RUN, 500);
}

// ---- DNS rebinding (review of #738) ---------------------------------------------------------

#[test]
fn every_fetch_is_pinned_to_the_one_vetted_answer_and_a_rebinding_answer_is_never_used() {
    // The host answers public first, then private (a rebinding resolver). The check resolves it
    // once; robots.txt is fetched pinned to the public answer; the private answer is never asked
    // for during that check.
    let w = web().dns("en.wikipedia.org", vec![vec![PUBLIC_IP], vec![PRIVATE_IP]]);
    let c = check_source(&w, &policy(), &mut RobotsCache::default(), "https://en.wikipedia.org/wiki/Apple", None);
    assert!(c.allowed, "{c:?}");
    assert_eq!(c.pinned, vec![PUBLIC_IP]);
    assert_eq!(w.lookups.lock().unwrap().len(), 1, "one DNS lookup per check");
    assert!(w.pins.lock().unwrap().iter().all(|p| p == &vec![PUBLIC_IP]), "{:?}", w.pins.lock().unwrap());
    // The pinned addresses never reach the webview.
    assert!(!serde_json::to_string(&c).unwrap().contains("93.184"));
}

#[test]
fn a_run_fetches_each_page_pinned_to_the_address_its_own_check_vetted() {
    let kr = MemKeyring::default();
    let s = DatasetStore::new(tmp("pin-run-store"), &kr);
    let (id, v) = dataset(&s, "pinrun");
    let other = std::net::IpAddr::V4(std::net::Ipv4Addr::new(93, 184, 215, 99));
    // Planned at one public address, re-checked at another: the page goes to the re-checked one.
    let w = web().dns("en.wikipedia.org", vec![vec![PUBLIC_IP], vec![other]]);
    let p = plan(&w, &policy(), &id, v, &urls(&["https://en.wikipedia.org/wiki/Apple"]), None, 9).unwrap();
    approve(&p.id).unwrap();
    let r = run_plan(&w, &Waits::default(), &policy(), &s, &p.id, &clock).unwrap();
    assert!(r.outcomes[0].collected, "{r:?}");
    let asked = w.asked();
    let pins = w.pins.lock().unwrap().clone();
    let page = asked.iter().position(|u| u.ends_with("/wiki/Apple")).unwrap();
    assert_eq!(pins[page], vec![other]);
}

#[test]
fn a_host_that_rebinds_to_a_private_address_before_the_run_is_not_fetched() {
    let kr = MemKeyring::default();
    let s = DatasetStore::new(tmp("rebind-store"), &kr);
    let (id, v) = dataset(&s, "rebind");
    // Public when planned, private when the run checks it again.
    let w = web().dns("en.wikipedia.org", vec![vec![PUBLIC_IP], vec![PRIVATE_IP]]);
    let p = plan(&w, &policy(), &id, v, &urls(&["https://en.wikipedia.org/wiki/Apple"]), None, 8).unwrap();
    assert_eq!(p.pages, 1);
    approve(&p.id).unwrap();
    let r = run_plan(&w, &Waits::default(), &policy(), &s, &p.id, &clock).unwrap();
    assert!(r.version.is_none());
    assert!(w.pages_asked().is_empty(), "{:?}", w.asked());
    assert!(r.outcomes[0].note.as_deref().unwrap().contains("local or private address"));
}

#[test]
fn the_pinned_resolver_answers_only_the_vetted_addresses() {
    use ureq::unversioned::resolver::Resolver;
    let u = url::Url::parse("https://en.wikipedia.org/wiki/Apple").unwrap();
    let r = PinnedResolver(pinned_socket_addrs(&u, &[PUBLIC_IP]));
    let cfg = ureq::Agent::config_builder().build();
    let uri: ureq::http::Uri = "https://attacker.example/".parse().unwrap();
    let got = r.resolve(&uri, &cfg, TIMEOUT).unwrap();
    assert_eq!(&got[..], &[std::net::SocketAddr::new(PUBLIC_IP, 443)][..]);
    assert!(PinnedResolver(Vec::new()).resolve(&uri, &cfg, TIMEOUT).is_err());
    // A private address is refused before any socket opens.
    assert!(LiveHttp.get(&u, &[PRIVATE_IP], 10).is_err());
    assert!(vet_addrs("x", vec![PUBLIC_IP, PRIVATE_IP]).is_err());
    assert!(vet_addrs("x", vec![]).is_err());
}

const TIMEOUT: ureq::unversioned::transport::NextTimeout = ureq::unversioned::transport::NextTimeout { after: ureq::unversioned::transport::time::Duration::Exact(std::time::Duration::from_secs(1)), reason: ureq::Timeout::Resolve };

// ---- Codex review of #738 (P2s) ----------------------------------------------------------------

#[test]
fn an_out_of_range_crawl_delay_refuses_the_host_and_never_panics() {
    let check = |delay: &str| {
        let w = Web::default().page(
            "https://en.wikipedia.org/robots.txt",
            200,
            "text/plain",
            &format!("User-agent: *\nCrawl-delay: {delay}\n"),
        );
        check_source(
            &w,
            &policy(),
            &mut RobotsCache::default(),
            "https://en.wikipedia.org/wiki/Apple",
            None,
        )
    };
    // Too long to represent as a duration: longer than the maximum honoured, so refused.
    for d in ["1e100", "1e30"] {
        assert!(!check(d).allowed, "{d}");
    }
    // Not a delay at all: ignored, and the 2 s floor applies.
    for d in ["inf", "NaN", "-1", "soon"] {
        let c = check(d);
        assert!(c.allowed, "{d}: {c:?}");
        assert_eq!(c.interval_secs, 2, "{d}");
    }
}

#[test]
fn robots_txt_belongs_to_its_origin_not_its_host() {
    let w = Web::default()
        .page("http://en.wikipedia.org/robots.txt", 200, "text/plain", ROBOTS_OPEN)
        .page(
            "https://en.wikipedia.org/robots.txt",
            200,
            "text/plain",
            "User-agent: *\nDisallow: /\n",
        );
    let mut cache = RobotsCache::default();
    let http = check_source(&w, &policy(), &mut cache, "http://en.wikipedia.org/wiki/Apple", None);
    let https = check_source(&w, &policy(), &mut cache, "https://en.wikipedia.org/wiki/Apple", None);
    assert_eq!(http.robots, RobotsState::Allowed, "{http:?}");
    assert!(!https.allowed, "https has its own robots.txt: {https:?}");
    assert!(w.asked().contains(&"https://en.wikipedia.org/robots.txt".to_string()));
}

#[test]
fn robots_matching_decodes_percent_encoded_unreserved_characters() {
    let r = Robots::parse("User-agent: *\nDisallow: /private/\nDisallow: /%7Euser/\n");
    assert!(!r.allows("/private/x"));
    assert!(!r.allows("/%70rivate/x"), "%70 is p");
    assert!(!r.allows("/~user/a"), "the rule's own %7E is ~");
    assert!(!r.allows("/%7euser/a"));
    // A reserved character stays encoded: %2F is not a path separator.
    let slash = Robots::parse("User-agent: *\nDisallow: /a/b\n");
    assert!(slash.allows("/a%2Fb"));
}

