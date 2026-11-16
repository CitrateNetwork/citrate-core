//! citrate-core 0.5.4 (WP-H8): **the compliant collector**. It is the one way Hermes adds web
//! data to a member's dataset.
//!
//! The collection policy is the owner's strict default (2026-11-16, Hermes x FL gap analysis
//! §3.1). It is enforced here, in core, and never in the prompt.
//!
//! 1. **One collector.** Web content reaches a dataset only through [`run_plan`], which writes
//!    through [`crate::fl_datasets::DatasetStore::append`]. The agent's other ways to reach the web
//!    (`read_url`, the browser, shell, file tools) cannot write the store: it lives in core app
//!    data, which the agent's default-deny list covers (`fl_datasets::dataset_root_is_agent_denied`).
//!    `node <script>` no longer runs without a card (V1, citrate-agent-runtime#107).
//! 2. **robots.txt.** It is fetched for every host and honoured for the `CitrateHermes` user agent:
//!    the longest matching rule wins, and `Allow` beats `Disallow` on a tie. A host with no
//!    robots.txt (404 or 410) is collected only when the source's license is known (allowlisted,
//!    or attested by the member). A missing robots.txt is never permission on its own. A 401 or
//!    403 for robots.txt, or one that cannot be read, refuses the host.
//! 3. **Allow and deny lists.** Citrate curates them. They ship in the bundle and carry a version
//!    (`fl-collect/source-policy.json`, [`SourcePolicy`]). A denied host is refused. A host that
//!    is on neither list needs the member's tap: the plan card names it, and the member approves
//!    the plan.
//!    DNS is resolved once per check and vetted ([`vet_addrs`]); every fetch for that source is
//!    pinned to the vetted addresses ([`PinnedResolver`]), so a rebinding answer is never used.
//! 4. **Never collect:**
//!    - anything behind authentication: the collector sends no cookies and no credentials, and a
//!      401 stops that source;
//!    - URLs that carry credentials or session tokens;
//!    - local or private addresses;
//!    - hosts on the deny list, which covers people-search and OSINT sites, private messaging,
//!      and sites whose terms forbid automated collection;
//!    - rows with a key, token or seed phrase: the scanner finds them and the row is dropped.
//!
//!    Paywalls cannot be detected reliably. Paywalled publishers go on the deny list.
//! 5. **Rate limits.** At most one request every 2 s per host, or the host's `Crawl-delay` if that
//!    is longer, and at most [`MAX_PAGES_PER_RUN`] pages per run. A 429 or 403 stops the run. A 503
//!    with `Retry-After` waits up to [`MAX_RETRY_WAIT`] once, then stops. The collector fetches
//!    only the URLs on the approved plan and never follows links.
//! 6. **Provenance.** Every item records its URL, fetch time, content SHA-256, license, robots
//!    verdict, terms class, consent and [`COLLECTOR_VERSION`]. The dataset manifest carries these
//!    records.
//! 7. **The member decides.** A plan ([`plan`]) is only a proposal. Core shows it on an HIC-1 card,
//!    and only a plan the member approved ([`approve`]) runs. The run checks every source again
//!    before it fetches.
//! 8. **Untrusted.** Everything collected is untrusted. The store marks web sources untrusted, and
//!    the tool result taints the session.
//!
//! After a run, the new version is scanned with [`crate::pii_scan`] and the result is recorded in
//! the store (WP-H7).
//!
//! Rule 3: nothing here signs. Rule 7: the data sources are the network fetch of each approved
//! URL and of its host's robots.txt, the bundled source policy, and the dataset store.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::fl_datasets::{
    CollectedItem, ConsentInput, DatasetStore, LicenseBasis, LicenseRecord, RobotsVerdict,
    SourceKind, TosClass, VersionView,
};

/// What adds collected items, recorded in every source record.
pub const COLLECTOR_VERSION: &str = "citrate-collector/1";
/// The product token robots.txt rules are matched against (case-insensitive).
pub const UA_TOKEN: &str = "CitrateHermes";
/// The full user agent, with a contact URL.
pub const USER_AGENT: &str = "CitrateHermes/0.5 (+https://citrate.ai/hermes/collector)";
/// The least time between two requests to one host.
pub const MIN_INTERVAL: Duration = Duration::from_secs(2);
/// The longest `Crawl-delay` honoured; a longer one refuses the host (a run would not finish).
pub const MAX_CRAWL_DELAY: Duration = Duration::from_secs(60);
/// The most pages one run fetches.
pub const MAX_PAGES_PER_RUN: usize = 500;
/// The longest `Retry-After` waited for, once.
pub const MAX_RETRY_WAIT: Duration = Duration::from_secs(60);
/// The most bytes read from one page or robots.txt.
pub const MAX_PAGE_BYTES: u64 = 2 * 1024 * 1024;
pub const MAX_ROBOTS_BYTES: u64 = 512 * 1024;
/// The most characters of extracted text kept per row.
pub const MAX_ROW_CHARS: usize = 8000;
/// The most plans kept in memory.
const MAX_PLANS: usize = 32;

/// The bundled, versioned source policy.
const POLICY_JSON: &str = include_str!("../fl-collect/source-policy.json");

// ------------------------------------------------------------------------------ the policy

/// One allowlisted host and the license its content is published under.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AllowEntry {
    pub host: String,
    /// SPDX id of the license the host publishes its text under.
    pub license: String,
    /// Where that license is stated.
    pub evidence: String,
}

/// One denied host and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DenyEntry {
    pub host: String,
    pub reason: String,
}

/// Citrate's curated allow and deny lists.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourcePolicy {
    pub version: u32,
    pub updated: String,
    pub curator: String,
    pub allow: Vec<AllowEntry>,
    pub deny: Vec<DenyEntry>,
}

impl SourcePolicy {
    /// The policy shipped in this build.
    pub fn bundled() -> Result<Self, String> {
        let p: SourcePolicy = serde_json::from_str(POLICY_JSON)
            .map_err(|e| format!("the bundled source policy is invalid: {e}"))?;
        p.check()?;
        Ok(p)
    }

    fn check(&self) -> Result<(), String> {
        let mut seen = std::collections::BTreeSet::new();
        for h in self
            .allow
            .iter()
            .map(|a| &a.host)
            .chain(self.deny.iter().map(|d| &d.host))
        {
            if h.is_empty()
                || h != &h.to_ascii_lowercase()
                || h.starts_with('.')
                || !h.contains('.')
            {
                return Err(format!(
                    "source policy host {h:?} is not a lowercase host name"
                ));
            }
            if !seen.insert(h.clone()) {
                return Err(format!("source policy lists {h} twice"));
            }
        }
        for a in &self.allow {
            crate::fl_datasets::registry_license_ok(&a.license)
                .map_err(|e| format!("allowlisted {}: {e}", a.host))?;
        }
        Ok(())
    }

    /// The deny entry covering `host` (the host itself or a parent domain).
    pub fn denied(&self, host: &str) -> Option<&DenyEntry> {
        self.deny.iter().find(|d| covers(&d.host, host))
    }

    /// The allow entry covering `host`.
    pub fn allowed(&self, host: &str) -> Option<&AllowEntry> {
        self.allow.iter().find(|a| covers(&a.host, host))
    }
}

fn covers(entry: &str, host: &str) -> bool {
    host == entry || host.ends_with(&format!(".{entry}"))
}

// ------------------------------------------------------------------------------ URLs

/// Query parameter names that carry a credential or a session (never collected).
const SECRET_PARAMS: &[&str] = &[
    "token",
    "access_token",
    "id_token",
    "auth",
    "key",
    "api_key",
    "apikey",
    "sig",
    "signature",
    "session",
    "sessionid",
    "sid",
    "password",
    "pass",
    "code",
    "jwt",
];
const LOCAL_SUFFIXES: &[&str] = &[
    ".localhost",
    ".local",
    ".internal",
    ".lan",
    ".home.arpa",
    ".onion",
];

/// Check one URL's shape: http or https, a public host name (no IP literal, no local name), the
/// default port, no credentials in it. Returns the parsed URL and its lowercase host.
pub fn check_url(raw: &str) -> Result<(url::Url, String), String> {
    let u = url::Url::parse(raw.trim()).map_err(|_| "that is not a URL".to_string())?;
    if !matches!(u.scheme(), "https" | "http") {
        return Err("only http and https pages can be collected".into());
    }
    if !u.username().is_empty() || u.password().is_some() {
        return Err("a URL with a user name or password in it is never collected".into());
    }
    let host = match u.host() {
        Some(url::Host::Domain(d)) => d.to_ascii_lowercase(),
        Some(_) => {
            return Err("an IP address is not a host the policy can check; use its name".into())
        }
        None => return Err("the URL has no host".into()),
    };
    if host == "localhost"
        || !host.contains('.')
        || host.ends_with('.')
        || LOCAL_SUFFIXES.iter().any(|s| host.ends_with(s))
    {
        return Err("local and private hosts are never collected".into());
    }
    if u.port().is_some() {
        return Err("only the default port is collected".into());
    }
    if let Some(p) = u
        .query_pairs()
        .find(|(k, _)| SECRET_PARAMS.contains(&k.to_ascii_lowercase().as_str()))
    {
        return Err(format!(
            "the URL carries a credential or session ({}); it is never collected",
            p.0
        ));
    }
    Ok((u, host))
}

/// Whether `ip` is a public address (no loopback, private, link-local, CGNAT, multicast,
/// documentation, unique-local or IPv4-mapped address).
pub fn is_public_ip(ip: std::net::IpAddr) -> bool {
    use std::net::IpAddr;
    match ip {
        IpAddr::V4(v) => {
            let o = v.octets();
            !(v.is_loopback()
                || v.is_private()
                || v.is_link_local()
                || v.is_unspecified()
                || v.is_broadcast()
                || v.is_multicast()
                || v.is_documentation()
                || (o[0] == 100 && (64..=127).contains(&o[1])))
        }
        IpAddr::V6(v) => {
            let s = v.segments();
            !(v.is_loopback()
                || v.is_unspecified()
                || v.is_multicast()
                || (s[0] & 0xfe00) == 0xfc00
                || (s[0] & 0xffc0) == 0xfe80
                || v.to_ipv4_mapped().is_some())
        }
    }
}

/// Check the addresses a host resolved to: at least one, and every one public. Returns them: they
/// are the only addresses the fetch may connect to (review of #738: DNS is resolved once, the
/// connection is pinned to the vetted answer, so a second, rebound answer is never used).
pub fn vet_addrs(
    host: &str,
    addrs: Vec<std::net::IpAddr>,
) -> Result<Vec<std::net::IpAddr>, String> {
    if addrs.is_empty() {
        return Err(format!("{host} does not resolve"));
    }
    if addrs.iter().any(|ip| !is_public_ip(*ip)) {
        return Err(format!(
            "{host} resolves to a local or private address; it is never collected"
        ));
    }
    Ok(addrs)
}

// ------------------------------------------------------------------------------ robots.txt

/// The rules of robots.txt that apply to [`UA_TOKEN`] (RFC 9309).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Robots {
    /// (allow, path pattern)
    rules: Vec<(bool, String)>,
    pub crawl_delay: Option<Duration>,
}

impl Robots {
    /// Parse robots.txt. The group that names `CitrateHermes` applies; otherwise the `*` group.
    pub fn parse(text: &str) -> Self {
        #[derive(Default)]
        struct Group {
            agents: Vec<String>,
            rules: Vec<(bool, String)>,
            delay: Option<Duration>,
        }
        let mut groups: Vec<Group> = Vec::new();
        let mut cur = Group::default();
        let mut in_agents = false;
        for line in text.lines() {
            let line = line.split('#').next().unwrap_or("").trim();
            let Some((k, v)) = line.split_once(':') else {
                continue;
            };
            let (k, v) = (k.trim().to_ascii_lowercase(), v.trim());
            match k.as_str() {
                "user-agent" => {
                    if !in_agents && (!cur.agents.is_empty()) {
                        groups.push(std::mem::take(&mut cur));
                    }
                    cur.agents.push(v.to_ascii_lowercase());
                    in_agents = true;
                }
                "allow" | "disallow" => {
                    in_agents = false;
                    if !v.is_empty() {
                        cur.rules.push((k == "allow", v.to_string()));
                    }
                }
                "crawl-delay" => {
                    in_agents = false;
                    cur.delay = v
                        .parse::<f64>()
                        .ok()
                        .filter(|d| d.is_finite() && *d >= 0.0)
                        .map(Duration::from_secs_f64);
                }
                _ => {}
            }
        }
        if !cur.agents.is_empty() {
            groups.push(cur);
        }
        let token = UA_TOKEN.to_ascii_lowercase();
        let mine: Vec<&Group> = groups
            .iter()
            .filter(|g| g.agents.iter().any(|a| a == &token))
            .collect();
        let chosen: Vec<&Group> = if mine.is_empty() {
            groups
                .iter()
                .filter(|g| g.agents.iter().any(|a| a == "*"))
                .collect()
        } else {
            mine
        };
        let mut out = Robots::default();
        for g in chosen {
            out.rules.extend(g.rules.iter().cloned());
            out.crawl_delay = out.crawl_delay.max(g.delay);
        }
        out
    }

    /// Whether `path` (with its query) may be fetched: the longest matching rule wins, `Allow` on
    /// a tie; no rule = allowed.
    pub fn allows(&self, path: &str) -> bool {
        let mut best: Option<(usize, bool)> = None;
        for (allow, pat) in &self.rules {
            if robots_match(pat, path) {
                let len = pat.len();
                best = match best {
                    Some((l, a)) if l > len || (l == len && a) => Some((l, a)),
                    _ => Some((len, *allow)),
                };
            }
        }
        best.map(|(_, a)| a).unwrap_or(true)
    }
}

/// RFC 9309 matching: `*` matches any run of characters, a trailing `$` anchors the end.
fn robots_match(pat: &str, path: &str) -> bool {
    let (pat, anchored) = match pat.strip_suffix('$') {
        Some(p) => (p, true),
        None => (pat, false),
    };
    let parts: Vec<&str> = pat.split('*').collect();
    let mut at = 0usize;
    for (i, part) in parts.iter().enumerate() {
        if i == 0 {
            if !path.starts_with(part) {
                return false;
            }
            at = part.len();
            continue;
        }
        if i == parts.len() - 1 && anchored {
            return path.len() >= at + part.len() && path.ends_with(part);
        }
        match path[at..].find(part) {
            Some(p) => at += p + part.len(),
            None => return false,
        }
    }
    !anchored || at == path.len()
}

// ------------------------------------------------------------------------------ HTTP

/// One HTTP answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fetched {
    pub status: u16,
    pub content_type: String,
    pub retry_after: Option<Duration>,
    pub body: Vec<u8>,
}

/// The collector's network seam. Production is [`LiveHttp`]; tests pass a fixed table.
pub trait CollectHttp: Send + Sync {
    /// Resolve `host` (DNS). Called once per source check; the answer is vetted by
    /// [`vet_addrs`] and then pinned.
    fn resolve(&self, host: &str) -> Result<Vec<std::net::IpAddr>, String>;
    /// GET `url` with [`USER_AGENT`], no cookies, no credentials, no redirects, at most `limit`
    /// bytes, connecting **only** to `pinned` (the vetted addresses; the URL's host name is still
    /// the Host header and the TLS server name). The implementation must not resolve again.
    fn get(
        &self,
        url: &url::Url,
        pinned: &[std::net::IpAddr],
        limit: u64,
    ) -> Result<Fetched, String>;
}

/// A ureq resolver that answers only the vetted addresses, whatever the host name.
#[derive(Debug)]
pub struct PinnedResolver(pub Vec<std::net::SocketAddr>);

impl ureq::unversioned::resolver::Resolver for PinnedResolver {
    fn resolve(
        &self,
        _uri: &ureq::http::Uri,
        _config: &ureq::config::Config,
        _timeout: ureq::unversioned::transport::NextTimeout,
    ) -> Result<ureq::unversioned::resolver::ResolvedSocketAddrs, ureq::Error> {
        let mut out = self.empty();
        // ureq keeps at most 16 addresses (its MAX_ADDRS); `push` panics past that.
        for a in self.0.iter().take(16) {
            out.push(*a);
        }
        if out.is_empty() {
            return Err(ureq::Error::HostNotFound);
        }
        Ok(out)
    }
}

/// The socket addresses a pinned fetch of `url` connects to.
pub fn pinned_socket_addrs(
    url: &url::Url,
    pinned: &[std::net::IpAddr],
) -> Vec<std::net::SocketAddr> {
    let port = url.port_or_known_default().unwrap_or(443);
    pinned
        .iter()
        .map(|ip| std::net::SocketAddr::new(*ip, port))
        .collect()
}

/// The production network: ureq, no proxy from the environment, no redirects followed, statuses
/// read rather than raised, no cookie jar.
pub struct LiveHttp;

impl CollectHttp for LiveHttp {
    fn resolve(&self, host: &str) -> Result<Vec<std::net::IpAddr>, String> {
        use std::net::ToSocketAddrs;
        (host, 443)
            .to_socket_addrs()
            .map(|a| a.map(|x| x.ip()).collect())
            .map_err(|_| format!("{host} does not resolve"))
    }

    fn get(
        &self,
        url: &url::Url,
        pinned: &[std::net::IpAddr],
        limit: u64,
    ) -> Result<Fetched, String> {
        if pinned.is_empty() || pinned.iter().any(|ip| !is_public_ip(*ip)) {
            return Err("no vetted public address to connect to".into());
        }
        let config = ureq::Agent::config_builder()
            .max_redirects(0)
            .max_redirects_will_error(false)
            .proxy(None)
            .timeout_connect(Some(Duration::from_secs(10)))
            .timeout_global(Some(Duration::from_secs(30)))
            .http_status_as_error(false)
            .build();
        // The connection goes only to the vetted addresses; ureq's own DNS lookup is replaced.
        let agent = ureq::Agent::with_parts(
            config,
            ureq::unversioned::transport::DefaultConnector::new(),
            PinnedResolver(pinned_socket_addrs(url, pinned)),
        );
        let mut resp = agent
            .get(url.as_str())
            .header("User-Agent", USER_AGENT)
            .header("Accept", "text/html, text/plain;q=0.9")
            .call()
            .map_err(|e| match e {
                ureq::Error::Timeout(_) => "the request timed out".to_string(),
                _ => "the request failed".to_string(),
            })?;
        let status = resp.status().as_u16();
        let header = |n: &str| {
            resp.headers()
                .get(n)
                .and_then(|v| v.to_str().ok())
                .map(str::to_string)
        };
        let content_type = header("content-type")
            .unwrap_or_default()
            .to_ascii_lowercase();
        let retry_after = header("retry-after")
            .and_then(|v| v.trim().parse::<u64>().ok())
            .map(Duration::from_secs);
        let body = resp
            .body_mut()
            .with_config()
            .limit(limit)
            .read_to_vec()
            .map_err(|_| "the page was too large or could not be read".to_string())?;
        Ok(Fetched {
            status,
            content_type,
            retry_after,
            body,
        })
    }
}

// ------------------------------------------------------------------------------ source check

/// How the policy classes a host.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PolicyClass {
    Allowlisted,
    /// On neither list: the member's approval of the plan is the tap.
    NeedsMemberApproval,
    Denied,
}

/// What robots.txt said.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RobotsState {
    Allowed,
    Disallowed,
    /// 404 or 410: allowed by the standard, collected here only with a known license.
    Missing,
    /// Could not be read, or 401/403/5xx: the host is refused.
    Unreadable,
    /// Not fetched (the URL or host was refused first).
    NotChecked,
}

/// A license the member attests for a source with no license on it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Attestation {
    /// SPDX id (DatasetRegistry's license rule applies).
    pub license: String,
    /// The member's own words: where the license is stated or why they may use the text.
    pub statement: String,
}

/// The verdict on one source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceCheck {
    pub url: String,
    pub host: String,
    pub allowed: bool,
    /// Why it is refused (in plain words), when it is.
    pub refusal: Option<String>,
    pub policy: PolicyClass,
    pub robots: RobotsState,
    pub license: LicenseRecord,
    /// Seconds between requests to this host on a run.
    pub interval_secs: u64,
    /// The vetted public addresses the host resolved to in this check; every fetch for this
    /// source connects only to these (never sent to the webview).
    #[serde(skip)]
    pub pinned: Vec<std::net::IpAddr>,
}

fn refused(
    url: &str,
    host: &str,
    policy: PolicyClass,
    robots: RobotsState,
    why: impl Into<String>,
) -> SourceCheck {
    SourceCheck {
        url: url.to_string(),
        host: host.to_string(),
        allowed: false,
        refusal: Some(why.into()),
        policy,
        robots,
        license: LicenseRecord::unknown(),
        interval_secs: MIN_INTERVAL.as_secs(),
        pinned: Vec::new(),
    }
}

/// Per-host robots.txt, read once per check or run.
#[derive(Default)]
pub struct RobotsCache(BTreeMap<String, (RobotsState, Option<Robots>)>);

impl RobotsCache {
    fn get(
        &mut self,
        http: &dyn CollectHttp,
        u: &url::Url,
        host: &str,
        pinned: &[std::net::IpAddr],
    ) -> (RobotsState, Option<Robots>) {
        if let Some(v) = self.0.get(host) {
            return v.clone();
        }
        let robots_url = match url::Url::parse(&format!("{}://{host}/robots.txt", u.scheme())) {
            Ok(r) => r,
            Err(_) => return (RobotsState::Unreadable, None),
        };
        let v = match http.get(&robots_url, pinned, MAX_ROBOTS_BYTES) {
            Ok(f) if f.status == 200 => (
                RobotsState::Allowed,
                Some(Robots::parse(&String::from_utf8_lossy(&f.body))),
            ),
            Ok(f) if f.status == 404 || f.status == 410 => (RobotsState::Missing, None),
            _ => (RobotsState::Unreadable, None),
        };
        self.0.insert(host.to_string(), v.clone());
        v
    }
}

/// Check one source against the policy and its host's robots.txt. `attested` is the member's
/// license for a source that states none.
pub fn check_source(
    http: &dyn CollectHttp,
    policy: &SourcePolicy,
    robots: &mut RobotsCache,
    raw: &str,
    attested: Option<&Attestation>,
) -> SourceCheck {
    let (u, host) = match check_url(raw) {
        Ok(v) => v,
        Err(e) => {
            return refused(
                raw,
                "",
                PolicyClass::NeedsMemberApproval,
                RobotsState::NotChecked,
                e,
            )
        }
    };
    if let Some(d) = policy.denied(&host) {
        return refused(
            raw,
            &host,
            PolicyClass::Denied,
            RobotsState::NotChecked,
            format!("{host} is on Citrate's do-not-collect list: {}", d.reason),
        );
    }
    let pinned = match http.resolve(&host).and_then(|a| vet_addrs(&host, a)) {
        Ok(a) => a,
        Err(e) => {
            return refused(
                raw,
                &host,
                PolicyClass::NeedsMemberApproval,
                RobotsState::NotChecked,
                e,
            )
        }
    };
    let allow = policy.allowed(&host);
    let class = if allow.is_some() {
        PolicyClass::Allowlisted
    } else {
        PolicyClass::NeedsMemberApproval
    };
    let license = match (allow, attested) {
        (Some(a), _) => LicenseRecord {
            id: Some(a.license.clone()),
            basis: LicenseBasis::Found,
            evidence: Some(a.evidence.clone()),
        },
        (None, Some(at)) => match crate::fl_datasets::registry_license_ok(&at.license) {
            Ok(()) if !at.statement.trim().is_empty() => LicenseRecord {
                id: Some(at.license.clone()),
                basis: LicenseBasis::MemberAttested,
                evidence: Some(at.statement.trim().chars().take(512).collect()),
            },
            Ok(()) => {
                return refused(
                    raw,
                    &host,
                    class,
                    RobotsState::NotChecked,
                    "an attested license needs your statement of where it comes from",
                )
            }
            Err(e) => {
                return refused(
                    raw,
                    &host,
                    class,
                    RobotsState::NotChecked,
                    format!("the attested license is refused: {e}"),
                )
            }
        },
        (None, None) => LicenseRecord::unknown(),
    };
    let (state, rules) = robots.get(http, &u, &host, &pinned);
    let path = match u.query() {
        Some(q) => format!("{}?{q}", u.path()),
        None => u.path().to_string(),
    };
    let known_license = license.basis != LicenseBasis::Unknown;
    let (state, why) = match (state, &rules) {
        (RobotsState::Allowed, Some(r)) if !r.allows(&path) => (
            RobotsState::Disallowed,
            Some(format!(
                "robots.txt on {host} disallows collecting this page"
            )),
        ),
        (RobotsState::Allowed, Some(r)) if r.crawl_delay.is_some_and(|d| d > MAX_CRAWL_DELAY) => (
            RobotsState::Allowed,
            Some(format!(
                "{host} asks for more than {} s between requests",
                MAX_CRAWL_DELAY.as_secs()
            )),
        ),
        (RobotsState::Allowed, _) if !known_license => (
            RobotsState::Allowed,
            Some(
                "no license is stated for this page; attest one or choose a licensed source"
                    .to_string(),
            ),
        ),
        (RobotsState::Allowed, _) => (RobotsState::Allowed, None),
        (RobotsState::Missing, _) if !known_license => (
            RobotsState::Missing,
            Some(format!(
                "{host} has no robots.txt and no license is stated. Hermes will not collect it."
            )),
        ),
        (RobotsState::Missing, _) => (RobotsState::Missing, None),
        (s, _) => (
            s,
            Some(format!(
                "robots.txt on {host} could not be read, so Hermes will not collect from it"
            )),
        ),
    };
    let delay = rules
        .as_ref()
        .and_then(|r| r.crawl_delay)
        .unwrap_or(MIN_INTERVAL)
        .max(MIN_INTERVAL);
    SourceCheck {
        url: u.to_string(),
        host,
        allowed: why.is_none(),
        refusal: why,
        policy: class,
        robots: state,
        license,
        interval_secs: delay.as_secs().max(MIN_INTERVAL.as_secs()),
        pinned,
    }
}

// ------------------------------------------------------------------------------ plans

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PlanStatus {
    Proposed,
    Approved,
    Ran,
    Declined,
}

/// A collection plan: what would be fetched, checked, and the words the member agrees to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectPlan {
    pub id: String,
    pub dataset: String,
    pub base_version: u32,
    pub sources: Vec<SourceCheck>,
    /// Pages a run fetches: the allowed sources, at most [`MAX_PAGES_PER_RUN`].
    pub pages: usize,
    /// The hosts the member approves by approving the plan (not on the allowlist).
    pub member_approved_hosts: Vec<String>,
    /// The consent statement the card shows and the store records.
    pub statement: String,
    pub status: PlanStatus,
    pub created_at_ms: u64,
    pub policy_version: u32,
}

static PLANS: Mutex<Vec<CollectPlan>> = Mutex::new(Vec::new());

/// Build a plan (no fetch of any page; robots.txt is read per host). It is a proposal only.
pub fn plan(
    http: &dyn CollectHttp,
    policy: &SourcePolicy,
    dataset: &str,
    base_version: u32,
    urls: &[String],
    attested: Option<&Attestation>,
    now_ms: u64,
) -> Result<CollectPlan, String> {
    if !crate::fl_datasets::valid_dataset_id(dataset) {
        return Err("that is not a dataset id".into());
    }
    if urls.is_empty() {
        return Err("a plan needs at least one URL".into());
    }
    if urls.len() > MAX_PAGES_PER_RUN {
        return Err(format!("a run collects at most {MAX_PAGES_PER_RUN} pages"));
    }
    let mut cache = RobotsCache::default();
    let mut sources = Vec::with_capacity(urls.len());
    let mut seen = std::collections::BTreeSet::new();
    for u in urls {
        let c = check_source(http, policy, &mut cache, u, attested);
        if seen.insert(c.url.clone()) {
            sources.push(c);
        }
    }
    let pages = sources.iter().filter(|s| s.allowed).count();
    let mut hosts: Vec<String> = sources
        .iter()
        .filter(|s| s.allowed && s.policy == PolicyClass::NeedsMemberApproval)
        .map(|s| s.host.clone())
        .collect();
    hosts.sort();
    hosts.dedup();
    let mut all_hosts: Vec<String> = sources
        .iter()
        .filter(|s| s.allowed)
        .map(|s| s.host.clone())
        .collect();
    all_hosts.sort();
    all_hosts.dedup();
    let statement = format!(
        "Collect {pages} page{} from {} into dataset {dataset}, honouring robots.txt and at most one request every {} s per host. Each item records its URL, time, hash and license.",
        if pages == 1 { "" } else { "s" },
        all_hosts.join(", "),
        MIN_INTERVAL.as_secs()
    );
    let p = CollectPlan {
        id: format!(
            "cp-{}",
            hex::encode(
                &Sha256::digest(format!("{dataset}{base_version}{now_ms}{urls:?}").as_bytes())[..8]
            )
        ),
        dataset: dataset.to_string(),
        base_version,
        sources,
        pages,
        member_approved_hosts: hosts,
        statement,
        status: PlanStatus::Proposed,
        created_at_ms: now_ms,
        policy_version: policy.version,
    };
    let mut plans = PLANS.lock().unwrap_or_else(|e| e.into_inner());
    plans.retain(|x| x.id != p.id);
    if plans.len() >= MAX_PLANS {
        plans.remove(0);
    }
    plans.push(p.clone());
    Ok(p)
}

/// The plan with `id`.
pub fn get_plan(id: &str) -> Option<CollectPlan> {
    PLANS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .find(|p| p.id == id)
        .cloned()
}

fn set_status(id: &str, from: PlanStatus, to: PlanStatus) -> Result<CollectPlan, String> {
    let mut plans = PLANS.lock().unwrap_or_else(|e| e.into_inner());
    let p = plans
        .iter_mut()
        .find(|p| p.id == id)
        .ok_or_else(|| format!("no collection plan {id}"))?;
    if p.status != from {
        return Err(format!("plan {id} is {:?}, not {:?}", p.status, from));
    }
    p.status = to;
    Ok(p.clone())
}

/// The member approved the plan on its card (HIC-1). Only a plan with something to collect can be
/// approved.
pub fn approve(id: &str) -> Result<CollectPlan, String> {
    if get_plan(id).is_some_and(|p| p.pages == 0) {
        return Err("nothing on this plan can be collected".into());
    }
    set_status(id, PlanStatus::Proposed, PlanStatus::Approved)
}

/// The member declined it.
pub fn decline(id: &str) -> Result<CollectPlan, String> {
    set_status(id, PlanStatus::Proposed, PlanStatus::Declined)
}

// ------------------------------------------------------------------------------ the run

/// One source's outcome on a run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceOutcome {
    pub url: String,
    pub collected: bool,
    pub rows: u32,
    pub dropped_secret_rows: u32,
    pub note: Option<String>,
}

/// What a run did.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunReport {
    pub plan: String,
    pub pages_fetched: usize,
    pub outcomes: Vec<SourceOutcome>,
    /// Why the run stopped early, if it did (a 403 or 429).
    pub stopped: Option<String>,
    /// The version written, if anything was collected.
    pub version: Option<VersionView>,
    /// The private-information scan of that version.
    pub pii_scan: Option<crate::pii_scan::ScanReport>,
    /// Always true: the content is other people's.
    pub untrusted: bool,
}

/// Sleep between requests (tests pass a recording clock).
pub trait Pause {
    fn wait(&self, d: Duration);
}

pub struct RealPause;
impl Pause for RealPause {
    fn wait(&self, d: Duration) {
        std::thread::sleep(d);
    }
}

/// Plain text from an HTML or text page: scripts, styles and tags removed, common entities
/// decoded, whitespace folded, split into paragraphs of at most [`MAX_ROW_CHARS`].
pub fn page_text(content_type: &str, body: &[u8]) -> Vec<String> {
    let s = String::from_utf8_lossy(body);
    let text = if content_type.contains("html") || s.trim_start().starts_with('<') {
        strip_html(&s)
    } else {
        s.to_string()
    };
    let mut out = Vec::new();
    for para in text.split("\n\n") {
        let p = para.split_whitespace().collect::<Vec<_>>().join(" ");
        if p.chars().count() < 20 {
            continue;
        }
        let mut cur = String::new();
        for w in p.split(' ') {
            if cur.chars().count() + w.chars().count() + 1 > MAX_ROW_CHARS && !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            if !cur.is_empty() {
                cur.push(' ');
            }
            cur.push_str(w);
        }
        if !cur.is_empty() {
            out.push(cur);
        }
    }
    out
}

fn strip_html(s: &str) -> String {
    let lower = s.to_ascii_lowercase();
    let mut out = String::with_capacity(s.len() / 2);
    let mut i = 0;
    let b = s.as_bytes();
    while i < b.len() {
        if b[i] == b'<' {
            let rest = &lower[i..];
            let skip_to = |tag: &str| rest.find(&format!("</{tag}")).map(|e| i + e);
            let end_tag = |from: usize| {
                lower[from..]
                    .find('>')
                    .map(|e| from + e + 1)
                    .unwrap_or(b.len())
            };
            if rest.starts_with("<script")
                || rest.starts_with("<style")
                || rest.starts_with("<noscript")
                || rest.starts_with("<template")
            {
                let tag = if rest.starts_with("<script") {
                    "script"
                } else if rest.starts_with("<style") {
                    "style"
                } else if rest.starts_with("<noscript") {
                    "noscript"
                } else {
                    "template"
                };
                i = skip_to(tag).map(end_tag).unwrap_or(b.len());
                continue;
            }
            if rest.starts_with("<!--") {
                i = lower[i..].find("-->").map(|e| i + e + 3).unwrap_or(b.len());
                continue;
            }
            let block = [
                "<p", "</p", "<br", "<div", "</div", "<li", "<h1", "<h2", "<h3", "<h4", "<tr",
                "<section", "<article", "</h",
            ]
            .iter()
            .any(|t| rest.starts_with(t));
            out.push_str(if block { "\n\n" } else { " " });
            i = end_tag(i);
            continue;
        }
        let next = lower[i..].find('<').map(|e| i + e).unwrap_or(b.len());
        out.push_str(&decode_entities(&s[i..next]));
        i = next;
    }
    out
}

fn decode_entities(s: &str) -> String {
    s.replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// Run an approved plan. Every source is checked again first; pages are fetched one at a time
/// within the rate limit; rows holding a key, token or seed phrase are dropped; the rest are
/// appended to the dataset under a new consent record whose statement is the plan's.
pub fn run_plan(
    http: &dyn CollectHttp,
    pause: &dyn Pause,
    policy: &SourcePolicy,
    store: &DatasetStore<'_>,
    plan_id: &str,
    now_ms: &dyn Fn() -> u64,
) -> Result<RunReport, String> {
    let p = set_status(plan_id, PlanStatus::Approved, PlanStatus::Ran).map_err(|_| {
        format!("plan {plan_id} is not approved by the member, so nothing was collected")
    })?;
    if p.policy_version != policy.version {
        return Err("the source policy changed since this plan was made; make a new plan".into());
    }
    let attest_for = |s: &SourceCheck| {
        (s.license.basis == LicenseBasis::MemberAttested).then(|| Attestation {
            license: s.license.id.clone().unwrap_or_default(),
            statement: s.license.evidence.clone().unwrap_or_default(),
        })
    };
    let mut cache = RobotsCache::default();
    let mut last: BTreeMap<String, Instant> = BTreeMap::new();
    let mut items: Vec<CollectedItem> = Vec::new();
    let mut outcomes = Vec::new();
    let mut fetched = 0usize;
    let mut stopped = None;
    for s in p
        .sources
        .iter()
        .filter(|s| s.allowed)
        .take(MAX_PAGES_PER_RUN)
    {
        let mut out = SourceOutcome {
            url: s.url.clone(),
            collected: false,
            rows: 0,
            dropped_secret_rows: 0,
            note: None,
        };
        if stopped.is_some() {
            out.note = Some("not fetched: the run stopped".into());
            outcomes.push(out);
            continue;
        }
        // Checked again now: robots.txt, the lists and the host may have changed.
        let again = check_source(http, policy, &mut cache, &s.url, attest_for(s).as_ref());
        if !again.allowed {
            out.note = again.refusal;
            outcomes.push(out);
            continue;
        }
        let interval = Duration::from_secs(again.interval_secs.max(MIN_INTERVAL.as_secs()));
        if let Some(t) = last.get(&again.host) {
            let since = t.elapsed();
            if since < interval {
                pause.wait(interval - since);
            }
        }
        let Ok(u) = url::Url::parse(&again.url) else {
            continue;
        };
        let mut got = http.get(&u, &again.pinned, MAX_PAGE_BYTES);
        last.insert(again.host.clone(), Instant::now());
        fetched += 1;
        if let Ok(f) = &got {
            if f.status == 503 {
                if let Some(wait) = f.retry_after.filter(|w| *w <= MAX_RETRY_WAIT) {
                    pause.wait(wait.max(interval));
                    got = http.get(&u, &again.pinned, MAX_PAGE_BYTES);
                    last.insert(again.host.clone(), Instant::now());
                    fetched += 1;
                }
            }
        }
        let f = match got {
            Ok(f) => f,
            Err(e) => {
                out.note = Some(e);
                outcomes.push(out);
                continue;
            }
        };
        match f.status {
            200 => {}
            403 | 429 => {
                stopped = Some(format!(
                    "{} answered {}; the run stopped and nothing more was fetched",
                    again.host, f.status
                ));
                out.note = stopped.clone();
                outcomes.push(out);
                continue;
            }
            401 => {
                out.note = Some("the page needs a sign-in; it is never collected".into());
                outcomes.push(out);
                continue;
            }
            300..=399 => {
                out.note = Some("the page redirects; redirects are not followed".into());
                outcomes.push(out);
                continue;
            }
            503 => {
                stopped = Some(format!(
                    "{} is unavailable (503); the run stopped",
                    again.host
                ));
                out.note = stopped.clone();
                outcomes.push(out);
                continue;
            }
            n => {
                out.note = Some(format!("the page answered {n}"));
                outcomes.push(out);
                continue;
            }
        }
        if !(f.content_type.starts_with("text/html")
            || f.content_type.starts_with("text/plain")
            || f.content_type.starts_with("application/xhtml"))
        {
            out.note = Some(format!(
                "{} pages are not collected",
                if f.content_type.is_empty() {
                    "untyped".to_string()
                } else {
                    f.content_type.clone()
                }
            ));
            outcomes.push(out);
            continue;
        }
        let mut rows = Vec::new();
        for text in page_text(&f.content_type, &f.body) {
            if crate::pii_scan::scan(&text)
                .iter()
                .any(|x| x.class == crate::pii_scan::PiiClass::Secret)
            {
                out.dropped_secret_rows += 1;
                continue;
            }
            rows.push(serde_json::json!({ "text": text }));
        }
        if rows.is_empty() {
            out.note = Some("no text to collect on the page".into());
            outcomes.push(out);
            continue;
        }
        out.collected = true;
        out.rows = rows.len() as u32;
        items.push(CollectedItem {
            kind: SourceKind::Web,
            locator: again.url.clone(),
            fetched_at_ms: now_ms(),
            content_sha256: format!("0x{}", hex::encode(Sha256::digest(&f.body))),
            license: again.license.clone(),
            robots: RobotsVerdict::Allowed,
            tos_class: if again.policy == PolicyClass::Allowlisted {
                TosClass::Allowlisted
            } else {
                TosClass::MemberApproved
            },
            collector_version: COLLECTOR_VERSION.to_string(),
            rows,
        });
        outcomes.push(out);
    }
    let (version, pii_scan) = if items.is_empty() {
        (None, None)
    } else {
        let scope: Vec<String> = {
            let mut h: Vec<String> = p
                .sources
                .iter()
                .filter(|s| s.allowed)
                .map(|s| s.host.clone())
                .collect();
            h.sort();
            h.dedup();
            h
        };
        let v = store.append(
            &p.dataset,
            p.base_version,
            ConsentInput::New {
                source_kind: SourceKind::Web,
                scope: scope.join(", "),
                statement: p.statement.clone(),
            },
            items,
            now_ms(),
        )?;
        let report = crate::fl_dataset_gate::scan_version(store, &p.dataset, v.version, now_ms())?;
        (Some(v), Some(report))
    };
    Ok(RunReport {
        plan: p.id,
        pages_fetched: fetched,
        outcomes,
        stopped,
        version,
        pii_scan,
        untrusted: true,
    })
}

// ------------------------------------------------------------------------------ commands

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// **fl_collect_policy**: the bundled allow and deny lists (read).
#[tauri::command]
pub async fn fl_collect_policy() -> Result<SourcePolicy, String> {
    SourcePolicy::bundled()
}

/// **fl_collect_source_check**: the verdict on one URL (read; fetches the host's robots.txt only).
#[tauri::command]
pub async fn fl_collect_source_check(
    url: String,
    attested: Option<Attestation>,
) -> Result<SourceCheck, String> {
    crate::blocking::off_main(move || {
        let policy = SourcePolicy::bundled()?;
        Ok(check_source(
            &LiveHttp,
            &policy,
            &mut RobotsCache::default(),
            &url,
            attested.as_ref(),
        ))
    })
    .await
}

/// **fl_collect_plan**: build a plan for the member's card. Fetches no page.
#[tauri::command]
pub async fn fl_collect_plan(
    dataset: String,
    base_version: u32,
    urls: Vec<String>,
    attested: Option<Attestation>,
) -> Result<CollectPlan, String> {
    crate::blocking::off_main(move || {
        let policy = SourcePolicy::bundled()?;
        plan(
            &LiveHttp,
            &policy,
            &dataset,
            base_version,
            &urls,
            attested.as_ref(),
            now_ms(),
        )
    })
    .await
}

/// **fl_collect_plan_decide**: the member's decision on the plan card (main window only; Hermes
/// has no tool for it).
#[tauri::command]
pub async fn fl_collect_plan_decide(plan_id: String, approve: bool) -> Result<CollectPlan, String> {
    if approve {
        self::approve(&plan_id)
    } else {
        decline(&plan_id)
    }
}

/// **fl_collect_run**: run a plan the member approved.
#[tauri::command]
pub async fn fl_collect_run(app: tauri::AppHandle, plan_id: String) -> Result<RunReport, String> {
    crate::blocking::off_main(move || {
        let policy = SourcePolicy::bundled()?;
        let keyring = crate::custody::OsKeyring::legacy();
        let store = DatasetStore::new(crate::fl_datasets::datasets_dir(&app)?, &keyring);
        run_plan(&LiveHttp, &RealPause, &policy, &store, &plan_id, &now_ms)
    })
    .await
}

#[cfg(test)]
mod tests {
    include!("fl_collect_tests.rs");
}
