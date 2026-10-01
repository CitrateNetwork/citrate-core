//! HUP-S4.4 — user-added MCP servers (Settings > MCP servers).
//!
//! A member adds their own MCP servers here. Nothing a member adds reaches Hermes until they have
//! reviewed it:
//!
//! 1. **Save** (add or edit): the entry is validated (the same rules as the runtime's
//!    `agent-mcp-host::user`, which re-validates on every probe) and stored DISABLED.
//!    Any change to what runs (command, args, working folder, URL, env, write tools) drops a
//!    previous review, so an edited server is disabled until reviewed again.
//! 2. **Review**: core asks the running Hermes sidecar for a dry-run probe (`POST /mcp/probe`):
//!    it starts the server, runs `initialize` and `tools/list`, and stops it, registering nothing.
//!    The review screen shows the exact command line or URL, the env keys (values masked), and every
//!    tool with its annotations (read-only / destructive / idempotent / open-world), each marked
//!    untrusted.
//! 3. **Enable**: allowed only with the review token of a successful probe of the entry exactly as
//!    it stands now. Core then writes the allowlist file the sidecar reads
//!    (`CITRATE_HERMES_MCP`). It takes effect the next time Hermes starts.
//!
//! Storage: `<app local data>/hermes/mcp-servers.json` (every entry, including disabled ones) and
//! `<app local data>/hermes/mcp-allowlist.json` (only enabled, reviewed entries, in the runtime's
//! shape). Both are written `0600`. Env values (often API tokens) live only in these files and in
//! the sidecar's child process; they never cross back into the webview, which sees masks.
//! With no enabled server the allowlist file does not exist and Hermes runs no MCP at all, so a
//! member who never opens this screen sees no change.
//!
//! Every MCP tool is untrusted: its output taints the session (enforced in the runtime), and write
//! tools are not offered unless the member turns them on for that server. Keyless (Rule 3): nothing
//! here holds a key or signs.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The registry file (every entry), under `<app local data>/hermes/`.
pub const REGISTRY_FILE: &str = "mcp-servers.json";
/// The allowlist file the sidecar reads (enabled, reviewed entries only).
pub const ALLOWLIST_FILE: &str = "mcp-allowlist.json";
/// The runtime's limit on configured servers (`agent-mcp-host::config::MAX_SERVERS`).
pub const MAX_SERVERS: usize = 16;

// Mirrors `agent-mcp-host::user` (the source of truth; the sidecar re-validates every probe with these
// rules, and the allowlist on load with the base rules, failing closed).
// Pending owner sign-off: the reserved names, the loader env denylist and the limits below are
// conservative placeholders (docs/MCP_USER_SERVERS.md, "Owner decisions").
const RESERVED_SERVER_NAMES: &[&str] = &[
    "citrate",
    "citrate-node",
    "node",
    "mem",
    "memory",
    "citratescan",
    "scan",
    "browser",
    "search",
    "toolchain",
    "fs",
    "shell",
    "office",
    "media",
    "hermes",
];
const LOADER_ENV_EXACT: &[&str] = &[
    "NODE_OPTIONS",
    "NODE_PATH",
    "BASH_ENV",
    "ENV",
    "PYTHONSTARTUP",
    "PYTHONPATH",
    "PYTHONHOME",
    "PERL5OPT",
    "PERL5LIB",
    "RUBYOPT",
    "RUBYLIB",
    "JAVA_TOOL_OPTIONS",
    "_JAVA_OPTIONS",
];
const LOADER_ENV_PREFIXES: &[&str] = &["LD_", "DYLD_"];
const MAX_ENV_VALUE: usize = 8192;
const MAX_ARG: usize = 4096;
const MAX_ARGS: usize = 64;
const MAX_ENV: usize = 64;
const MAX_ENV_KEY: usize = 128;
const MAX_URL: usize = 2048;
const MAX_PATH: usize = 4096;

// ---------------------------------------------------------------------------
// Shapes
// ---------------------------------------------------------------------------

/// One problem with one field (`name`, `transport`, `command`, `args`, `cwd`, `url`,
/// `env.<KEY>`, or `entry`). Messages never contain an env value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldError {
    pub field: String,
    pub message: String,
}

fn ferr(field: impl Into<String>, message: impl Into<String>) -> FieldError {
    FieldError {
        field: field.into(),
        message: message.into(),
    }
}

/// One env entry from the form. `value: None` keeps the value already stored under this key (the
/// webview never sees stored values, so an edit that does not retype a value sends `None`).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EnvInput {
    pub key: String,
    #[serde(default)]
    pub value: Option<String>,
}

/// The add/edit form.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ServerInput {
    pub name: String,
    pub transport: String,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub env: Vec<EnvInput>,
    #[serde(default)]
    pub allow_write_tools: bool,
    /// The entry being edited (absent = a new entry).
    #[serde(default)]
    pub previous_name: Option<String>,
}

/// One stored entry. Holds env VALUES: never returned across `invoke` (see [`ServerView`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredServer {
    pub name: String,
    pub transport: String,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub allow_write_tools: bool,
    #[serde(default)]
    pub enabled: bool,
    /// The fingerprint of the entry as it was when the member enabled it after a review.
    #[serde(default)]
    pub reviewed: Option<String>,
}

/// The registry file.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Registry {
    #[serde(default)]
    pub servers: Vec<StoredServer>,
}

/// An env key with its value masked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvView {
    pub key: String,
    pub masked: String,
}

/// One entry as the webview sees it: everything except env values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerView {
    pub name: String,
    pub transport: String,
    pub command: Option<String>,
    pub args: Vec<String>,
    pub cwd: Option<String>,
    pub url: Option<String>,
    pub env: Vec<EnvView>,
    pub allow_write_tools: bool,
    pub enabled: bool,
    /// Not enabled, or enabled under a review that no longer matches the entry.
    pub needs_review: bool,
}

/// The runtime's per-tool annotation hints, as the server sent them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeHints {
    #[serde(default)]
    pub read_only_hint: Option<bool>,
    #[serde(default)]
    pub destructive_hint: Option<bool>,
    #[serde(default)]
    pub idempotent_hint: Option<bool>,
    #[serde(default)]
    pub open_world_hint: Option<bool>,
    #[serde(default)]
    pub title: Option<String>,
}

/// The annotations Hermes applies after the spec defaults.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeEffective {
    pub read_only: bool,
    pub destructive: bool,
    pub idempotent: bool,
    pub open_world: bool,
}

/// One tool from the dry-run probe (the runtime's `probe::ProbedTool`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeTool {
    pub name: String,
    #[serde(default)]
    pub exposed_name: Option<String>,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub annotations: ProbeHints,
    pub effective: ProbeEffective,
    pub trust: String,
    pub offered: bool,
    #[serde(default)]
    pub skip_reason: Option<String>,
}

/// The dry-run probe's report (the runtime's `probe::ProbeReport`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeReport {
    pub name: String,
    pub transport: String,
    pub ok: bool,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub protocol_version: Option<String>,
    #[serde(default)]
    pub server_name: Option<String>,
    #[serde(default)]
    pub server_version: Option<String>,
    #[serde(default)]
    pub capabilities: Option<serde_json::Value>,
    #[serde(default)]
    pub tools: Vec<ProbeTool>,
    #[serde(default)]
    pub tools_truncated: bool,
    #[serde(default)]
    pub allow_write_tools: bool,
}

/// What the sidecar said about a probe request.
#[derive(Debug, Clone, PartialEq)]
pub enum ProbeOutcome {
    Report(ProbeReport),
    Invalid(Vec<FieldError>),
}

/// The review screen.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewView {
    pub server: ServerView,
    /// The exact command line (stdio) or URL (http) that will run.
    pub command_line: String,
    pub probe: ProbeReport,
    /// Pass back to enable. Binds the review to the entry exactly as probed.
    pub review_token: String,
    pub can_enable: bool,
}

/// The result of a save: field errors (nothing stored) or the updated list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveResult {
    pub ok: bool,
    pub errors: Vec<FieldError>,
    pub servers: Vec<ServerView>,
}

/// The review command's result: field errors from the sidecar, or the review screen.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewResult {
    pub errors: Vec<FieldError>,
    pub review: Option<ReviewView>,
}

// ---------------------------------------------------------------------------
// Validation (mirrors agent-mcp-host::user)
// ---------------------------------------------------------------------------

fn valid_server_name(name: &str) -> bool {
    let ok_len = !name.is_empty() && name.len() <= 24;
    let ok_first = name
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
    let ok_chars = name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-');
    ok_len && ok_first && ok_chars && !name.contains("__")
}

fn valid_env_key(k: &str) -> bool {
    let mut chars = k.chars();
    let first_ok = chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_');
    first_ok && k.len() <= MAX_ENV_KEY && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn is_loader_env(k: &str) -> bool {
    let up = k.to_ascii_uppercase();
    LOADER_ENV_EXACT.contains(&up.as_str()) || LOADER_ENV_PREFIXES.iter().any(|p| up.starts_with(p))
}

/// Whether `v` refers to another variable rather than being a value.
pub fn is_env_reference(v: &str) -> bool {
    if v.contains("${") {
        return true;
    }
    let t = v.trim();
    if let Some(rest) = t.strip_prefix('$') {
        if rest.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_') {
            return true;
        }
    }
    if t.len() >= 3 && t.starts_with('%') && t.ends_with('%') && valid_env_key(&t[1..t.len() - 1]) {
        return true;
    }
    false
}

/// `https://<host>` or `http://<loopback>`, no credentials in the authority.
fn url_problem(url: &str) -> Option<String> {
    if url.len() > MAX_URL {
        return Some(format!("the URL is longer than {MAX_URL} characters"));
    }
    let (https, rest) = if let Some(r) = url.strip_prefix("https://") {
        (true, r)
    } else if let Some(r) = url.strip_prefix("http://") {
        (false, r)
    } else {
        return Some("use https://, or http:// to this computer (localhost)".into());
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.is_empty() {
        return Some("the URL has no host".into());
    }
    if authority.contains('@') {
        return Some("credentials in the URL are not allowed; use the server's own sign-in".into());
    }
    if https {
        return None;
    }
    let host = if let Some(h) = authority.strip_prefix('[') {
        h.split(']').next().unwrap_or("")
    } else {
        authority
            .rsplit_once(':')
            .map(|(h, _)| h)
            .unwrap_or(authority)
    };
    let loopback = host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .map(|ip| ip.is_loopback())
            .unwrap_or(false);
    if loopback {
        None
    } else {
        Some("plain http is only allowed to this computer (localhost); use https".into())
    }
}

fn has_text(v: &Option<String>) -> bool {
    v.as_deref().is_some_and(|s| !s.is_empty())
}

/// Validate the form. Env entries with `value: None` are checked by key only (their value is
/// resolved against the stored entry in [`apply_save`]).
pub fn validate_input(i: &ServerInput) -> Result<(), Vec<FieldError>> {
    let mut errs = Vec::new();
    if !valid_server_name(&i.name) {
        errs.push(ferr(
            "name",
            "use 1-24 characters: a-z, 0-9, '_' or '-', starting with a letter or digit, no '__'",
        ));
    } else if RESERVED_SERVER_NAMES.contains(&i.name.as_str()) {
        errs.push(ferr(
            "name",
            "this name is reserved for a built-in server; choose another",
        ));
    }
    match i.transport.as_str() {
        "stdio" => {
            if has_text(&i.url) {
                errs.push(ferr("url", "a URL is for http servers"));
            }
            match i.command.as_deref() {
                None | Some("") => {
                    errs.push(ferr("command", "a command (absolute path) is required"))
                }
                Some(c) if !Path::new(c).is_absolute() => errs.push(ferr(
                    "command",
                    "use the absolute path to the program (no PATH lookup)",
                )),
                Some(c) if c.contains('\0') || c.len() > MAX_PATH => {
                    errs.push(ferr("command", "the command path is not valid"))
                }
                Some(_) => {}
            }
            if let Some(c) = i.cwd.as_deref().filter(|c| !c.is_empty()) {
                if !Path::new(c).is_absolute() || c.contains('\0') || c.len() > MAX_PATH {
                    errs.push(ferr("cwd", "the working folder must be an absolute path"));
                }
            }
            if i.args.len() > MAX_ARGS {
                errs.push(ferr("args", format!("at most {MAX_ARGS} arguments")));
            } else if i
                .args
                .iter()
                .any(|a| a.contains('\0') || a.chars().count() > MAX_ARG)
            {
                errs.push(ferr(
                    "args",
                    format!("each argument is at most {MAX_ARG} characters, with no NUL"),
                ));
            }
            if i.env.len() > MAX_ENV {
                errs.push(ferr("env", format!("at most {MAX_ENV} env entries")));
            }
            let mut seen = std::collections::BTreeSet::new();
            for e in &i.env {
                let field = format!("env.{}", e.key);
                if !valid_env_key(&e.key) {
                    errs.push(ferr(
                        field,
                        "env names use letters, digits and '_', starting with a letter or '_'",
                    ));
                    continue;
                }
                if !seen.insert(e.key.as_str()) {
                    errs.push(ferr(field, "this env name is listed twice"));
                    continue;
                }
                if is_loader_env(&e.key) {
                    errs.push(ferr(
                        field,
                        "this env name changes which code the server loads, so it is not allowed",
                    ));
                    continue;
                }
                if let Some(v) = &e.value {
                    if v.contains('\0') {
                        errs.push(ferr(field, "env values cannot contain a NUL character"));
                    } else if v.chars().count() > MAX_ENV_VALUE {
                        errs.push(ferr(
                            field,
                            format!("env values are at most {MAX_ENV_VALUE} characters"),
                        ));
                    } else if is_env_reference(v) {
                        errs.push(ferr(
                            field,
                            "values are passed literally: references to other variables are not expanded and nothing secret is inherited. Enter the value itself",
                        ));
                    }
                }
            }
        }
        "http" => {
            if has_text(&i.command) {
                errs.push(ferr("command", "a command is for stdio servers"));
            }
            if !i.args.is_empty() {
                errs.push(ferr("args", "arguments are for stdio servers"));
            }
            if !i.env.is_empty() {
                errs.push(ferr("env", "env entries are for stdio servers"));
            }
            if has_text(&i.cwd) {
                errs.push(ferr("cwd", "a working folder is for stdio servers"));
            }
            match i.url.as_deref() {
                None | Some("") => errs.push(ferr("url", "a URL is required")),
                Some(u) => {
                    if let Some(m) = url_problem(u) {
                        errs.push(ferr("url", m));
                    }
                }
            }
        }
        _ => errs.push(ferr(
            "transport",
            "choose stdio (a program) or http (a URL)",
        )),
    }
    if errs.is_empty() {
        Ok(())
    } else {
        Err(errs)
    }
}

// ---------------------------------------------------------------------------
// Registry operations (pure)
// ---------------------------------------------------------------------------

/// SHA-256 over everything that decides what runs: name, transport, command, args, cwd, URL, env
/// (keys and values), and the write-tools switch. Stored on disk only.
pub fn fingerprint(s: &StoredServer) -> String {
    let material = serde_json::json!({
        "name": s.name,
        "transport": s.transport,
        "command": s.command,
        "args": s.args,
        "cwd": s.cwd,
        "url": s.url,
        "env": s.env,
        "allow_write_tools": s.allow_write_tools,
    });
    hex::encode(Sha256::digest(material.to_string().as_bytes()))
}

fn review_matches(s: &StoredServer) -> bool {
    s.reviewed.as_deref() == Some(fingerprint(s).as_str())
}

/// Mask an env value: its length only, never any of its characters.
pub fn mask(v: &str) -> String {
    let n = v.chars().count();
    if n == 0 {
        "(empty)".to_string()
    } else {
        format!("•••••••• ({n} characters)")
    }
}

fn view(s: &StoredServer) -> ServerView {
    ServerView {
        name: s.name.clone(),
        transport: s.transport.clone(),
        command: s.command.clone(),
        args: s.args.clone(),
        cwd: s.cwd.clone(),
        url: s.url.clone(),
        env: s
            .env
            .iter()
            .map(|(k, v)| EnvView {
                key: k.clone(),
                masked: mask(v),
            })
            .collect(),
        allow_write_tools: s.allow_write_tools,
        enabled: s.enabled && review_matches(s),
        needs_review: !(s.enabled && review_matches(s)),
    }
}

/// Every entry, masked.
pub fn views(reg: &Registry) -> Vec<ServerView> {
    reg.servers.iter().map(view).collect()
}

/// Add or edit an entry. A new or changed entry is stored disabled; an edit that changes nothing
/// that runs keeps its review.
pub fn apply_save(reg: &mut Registry, input: ServerInput) -> Result<(), Vec<FieldError>> {
    validate_input(&input)?;
    let prev_idx = match &input.previous_name {
        Some(p) => Some(
            reg.servers
                .iter()
                .position(|s| &s.name == p)
                .ok_or_else(|| vec![ferr("entry", "the server being edited no longer exists")])?,
        ),
        None => None,
    };
    if reg
        .servers
        .iter()
        .enumerate()
        .any(|(i, s)| s.name == input.name && Some(i) != prev_idx)
    {
        return Err(vec![ferr("name", "another server already uses this name")]);
    }
    if prev_idx.is_none() && reg.servers.len() >= MAX_SERVERS {
        return Err(vec![ferr(
            "entry",
            format!("at most {MAX_SERVERS} MCP servers; remove one first"),
        )]);
    }
    let prev = prev_idx.and_then(|i| reg.servers.get(i)).cloned();
    let mut env = BTreeMap::new();
    let mut errs = Vec::new();
    if input.transport == "stdio" {
        for e in &input.env {
            match &e.value {
                Some(v) => {
                    env.insert(e.key.clone(), v.clone());
                }
                None => match prev.as_ref().and_then(|p| p.env.get(&e.key)) {
                    Some(v) => {
                        env.insert(e.key.clone(), v.clone());
                    }
                    None => errs.push(ferr(format!("env.{}", e.key), "enter a value")),
                },
            }
        }
    }
    if !errs.is_empty() {
        return Err(errs);
    }
    let stdio = input.transport == "stdio";
    let mut next = StoredServer {
        name: input.name,
        transport: input.transport,
        command: if stdio { input.command } else { None },
        args: if stdio { input.args } else { Vec::new() },
        cwd: if stdio {
            input.cwd.filter(|c| !c.is_empty())
        } else {
            None
        },
        url: if stdio { None } else { input.url },
        env,
        allow_write_tools: input.allow_write_tools,
        enabled: false,
        reviewed: None,
    };
    if let Some(p) = &prev {
        // Keep the review only when nothing that runs has changed.
        if p.enabled && review_matches(p) && fingerprint(p) == fingerprint(&next) {
            next.enabled = true;
            next.reviewed = p.reviewed.clone();
        }
    }
    match prev_idx {
        Some(i) => reg.servers[i] = next,
        None => reg.servers.push(next),
    }
    Ok(())
}

/// Remove an entry. Returns whether it existed.
pub fn apply_remove(reg: &mut Registry, name: &str) -> bool {
    let before = reg.servers.len();
    reg.servers.retain(|s| s.name != name);
    reg.servers.len() != before
}

/// Disable an entry (keeps it; a later enable needs a fresh review, so this session's check of
/// it is forgotten too).
pub fn apply_disable(reg: &mut Registry, gate: &mut ReviewGate, name: &str) -> bool {
    gate.forget(name);
    match reg.servers.iter_mut().find(|s| s.name == name) {
        Some(s) => {
            s.enabled = false;
            s.reviewed = None;
            true
        }
        None => false,
    }
}

/// Successful probes in this app session: server name → the fingerprint that was probed. Process
/// memory only, so a review never carries over a restart.
#[derive(Debug)]
pub struct ReviewGate {
    salt: [u8; 32],
    probed: HashMap<String, String>,
}

impl Default for ReviewGate {
    fn default() -> Self {
        use rand::RngCore;
        let mut salt = [0u8; 32];
        rand::rngs::OsRng.fill_bytes(&mut salt);
        ReviewGate {
            salt,
            probed: HashMap::new(),
        }
    }
}

impl ReviewGate {
    /// Record a probe of `s`. Only a successful probe whose every tool is untrusted counts; any
    /// other result clears an earlier one.
    pub fn record(&mut self, s: &StoredServer, r: &ProbeReport) {
        let good = r.ok && r.tools.iter().all(|t| t.trust == "untrusted");
        if good {
            self.probed.insert(s.name.clone(), fingerprint(s));
        } else {
            self.probed.remove(&s.name);
        }
    }

    fn probed_as_is(&self, s: &StoredServer) -> bool {
        self.probed.get(&s.name).map(String::as_str) == Some(fingerprint(s).as_str())
    }

    /// Forget a server (removed or renamed).
    pub fn forget(&mut self, name: &str) {
        self.probed.remove(name);
    }
}

/// The token the webview passes back to enable: a salted hash of the fingerprint, so the stored
/// fingerprint (a hash over env values) never crosses `invoke`.
pub fn review_token(gate: &ReviewGate, s: &StoredServer) -> String {
    let mut h = Sha256::new();
    h.update(gate.salt);
    h.update(fingerprint(s).as_bytes());
    hex::encode(h.finalize())
}

/// Enable `name` under a review. Refused unless this session probed the entry exactly as it
/// stands, the probe succeeded with every tool untrusted, and `token` is that entry's review
/// token.
pub fn apply_enable(
    reg: &mut Registry,
    gate: &ReviewGate,
    name: &str,
    token: &str,
) -> Result<(), String> {
    let s = reg
        .servers
        .iter_mut()
        .find(|s| s.name == name)
        .ok_or_else(|| "no such server".to_string())?;
    if !gate.probed_as_is(s) {
        return Err("review this server first: check it and look over its tools".into());
    }
    let expected = review_token(gate, s);
    if !subtle_eq(expected.as_bytes(), token.as_bytes()) {
        return Err("the server changed since it was reviewed; review it again".into());
    }
    s.reviewed = Some(fingerprint(s));
    s.enabled = true;
    Ok(())
}

fn subtle_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// The runtime's `[[servers]]` entry for `s` (snake_case; only the keys the runtime accepts).
pub fn entry_json(s: &StoredServer) -> serde_json::Value {
    let mut m = serde_json::Map::new();
    m.insert("name".into(), s.name.clone().into());
    m.insert("transport".into(), s.transport.clone().into());
    if s.transport == "stdio" {
        if let Some(c) = &s.command {
            m.insert("command".into(), c.clone().into());
        }
        m.insert("args".into(), serde_json::json!(s.args));
        m.insert("env".into(), serde_json::json!(s.env));
        if let Some(c) = &s.cwd {
            m.insert("cwd".into(), c.clone().into());
        }
    } else if let Some(u) = &s.url {
        m.insert("url".into(), u.clone().into());
    }
    m.insert("allow_write_tools".into(), s.allow_write_tools.into());
    serde_json::Value::Object(m)
}

/// The allowlist file's text: enabled entries whose review still matches. `None` = no file.
pub fn allowlist_json(reg: &Registry) -> Option<String> {
    let servers: Vec<serde_json::Value> = reg
        .servers
        .iter()
        .filter(|s| s.enabled && review_matches(s))
        .map(entry_json)
        .collect();
    if servers.is_empty() {
        None
    } else {
        Some(serde_json::json!({ "servers": servers }).to_string())
    }
}

fn quote_arg(a: &str) -> String {
    if !a.is_empty()
        && a.chars()
            .all(|c| c.is_ascii_alphanumeric() || "/._-=:,+@%".contains(c))
    {
        a.to_string()
    } else {
        format!("\"{}\"", a.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

/// The command line as it runs (for display; nothing is executed through a shell).
pub fn command_line(command: &str, args: &[String]) -> String {
    std::iter::once(quote_arg(command))
        .chain(args.iter().map(|a| quote_arg(a)))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Build the review screen for `s` from its probe.
pub fn review_view(gate: &ReviewGate, s: &StoredServer, probe: ProbeReport) -> ReviewView {
    let command_line = if s.transport == "stdio" {
        command_line(s.command.as_deref().unwrap_or(""), &s.args)
    } else {
        s.url.clone().unwrap_or_default()
    };
    let can_enable = gate.probed_as_is(s);
    ReviewView {
        server: view(s),
        command_line,
        probe,
        review_token: review_token(gate, s),
        can_enable,
    }
}

// ---------------------------------------------------------------------------
// Storage
// ---------------------------------------------------------------------------

/// The two files under `<app local data>/hermes/`.
pub struct McpStore {
    dir: PathBuf,
}

impl McpStore {
    pub fn new(dir: PathBuf) -> Self {
        McpStore { dir }
    }

    pub fn registry_path(&self) -> PathBuf {
        self.dir.join(REGISTRY_FILE)
    }

    pub fn allowlist_path(&self) -> PathBuf {
        self.dir.join(ALLOWLIST_FILE)
    }

    /// Read the registry. Missing = empty; unreadable or corrupt = an error (never silently empty,
    /// which would let the next save drop every entry).
    pub fn load(&self) -> Result<Registry, String> {
        match std::fs::read_to_string(self.registry_path()) {
            Ok(text) => serde_json::from_str(&text)
                .map_err(|_| "the MCP server list could not be read (file is damaged)".to_string()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Registry::default()),
            Err(e) => Err(format!(
                "the MCP server list could not be read: {}",
                e.kind()
            )),
        }
    }

    /// Write the registry and the allowlist (both `0600`), or remove the allowlist when nothing is
    /// enabled.
    pub fn save(&self, reg: &Registry) -> Result<(), String> {
        let text = serde_json::to_string_pretty(reg).map_err(|e| e.to_string())?;
        citrate_core_kit::fsutil::write_secret_file(&self.registry_path(), text.as_bytes())
            .map_err(|e| format!("saving the MCP server list: {}", e.kind()))?;
        match allowlist_json(reg) {
            Some(allow) => citrate_core_kit::fsutil::write_secret_file(
                &self.allowlist_path(),
                allow.as_bytes(),
            )
            .map_err(|e| format!("writing the MCP allowlist: {}", e.kind())),
            None => match std::fs::remove_file(self.allowlist_path()) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(format!("removing the MCP allowlist: {}", e.kind())),
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------

/// Serialises every read-modify-write of the registry, and holds the session's review gate.
static STATE: OnceLock<Mutex<ReviewGate>> = OnceLock::new();

fn gate() -> &'static Mutex<ReviewGate> {
    STATE.get_or_init(|| Mutex::new(ReviewGate::default()))
}

/// `<app local data>/hermes` (where the Hermes manager keeps its files too).
pub fn hermes_dir<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Result<PathBuf, String> {
    use tauri::Manager;
    Ok(app
        .path()
        .app_local_data_dir()
        .map_err(|e| e.to_string())?
        .join("hermes"))
}

fn store(app: &tauri::AppHandle) -> Result<McpStore, String> {
    Ok(McpStore::new(hermes_dir(app)?))
}

const NOT_RUNNING: &str =
    "Start Hermes to check a server: the check runs inside the agent sidecar, which is not running.";

/// Every user-added server (env values masked).
#[tauri::command]
pub async fn mcp_servers_list(app: tauri::AppHandle) -> Result<Vec<ServerView>, String> {
    crate::blocking::off_main(move || mcp_servers_list_sync(&app)).await
}

fn mcp_servers_list_sync(app: &tauri::AppHandle) -> Result<Vec<ServerView>, String> {
    let _g = gate().lock().unwrap_or_else(|e| e.into_inner());
    Ok(views(&store(app)?.load()?))
}

/// Add or edit a server. It is stored disabled until reviewed (an unchanged edit keeps its review).
#[tauri::command]
pub async fn mcp_server_save(
    app: tauri::AppHandle,
    input: ServerInput,
) -> Result<SaveResult, String> {
    crate::blocking::off_main(move || mcp_server_save_sync(&app, input)).await
}

fn mcp_server_save_sync(app: &tauri::AppHandle, input: ServerInput) -> Result<SaveResult, String> {
    let mut g = gate().lock().unwrap_or_else(|e| e.into_inner());
    let st = store(app)?;
    let mut reg = st.load()?;
    let renamed_from = input.previous_name.clone().filter(|p| p != &input.name);
    match apply_save(&mut reg, input) {
        Ok(()) => {
            st.save(&reg)?;
            if let Some(old) = renamed_from {
                g.forget(&old);
            }
            Ok(SaveResult {
                ok: true,
                errors: Vec::new(),
                servers: views(&reg),
            })
        }
        Err(errors) => Ok(SaveResult {
            ok: false,
            errors,
            servers: views(&reg),
        }),
    }
}

/// Remove a server.
#[tauri::command]
pub async fn mcp_server_remove(
    app: tauri::AppHandle,
    name: String,
) -> Result<Vec<ServerView>, String> {
    crate::blocking::off_main(move || {
        let mut g = gate().lock().unwrap_or_else(|e| e.into_inner());
        let st = store(&app)?;
        let mut reg = st.load()?;
        if apply_remove(&mut reg, &name) {
            st.save(&reg)?;
        }
        g.forget(&name);
        Ok(views(&reg))
    })
    .await
}

/// Disable a server (kept in the list; enabling again needs a fresh review).
#[tauri::command]
pub async fn mcp_server_disable(
    app: tauri::AppHandle,
    name: String,
) -> Result<Vec<ServerView>, String> {
    crate::blocking::off_main(move || {
        let mut g = gate().lock().unwrap_or_else(|e| e.into_inner());
        let st = store(&app)?;
        let mut reg = st.load()?;
        if apply_disable(&mut reg, &mut g, &name) {
            st.save(&reg)?;
        }
        Ok(views(&reg))
    })
    .await
}

/// Check a server through the running sidecar's dry-run probe and build the review screen.
#[tauri::command]
pub async fn mcp_server_review(
    app: tauri::AppHandle,
    name: String,
) -> Result<ReviewResult, String> {
    crate::blocking::off_main(move || mcp_server_review_sync(&app, &name)).await
}

fn mcp_server_review_sync(app: &tauri::AppHandle, name: &str) -> Result<ReviewResult, String> {
    let entry = {
        let _g = gate().lock().unwrap_or_else(|e| e.into_inner());
        let reg = store(app)?.load()?;
        reg.servers
            .into_iter()
            .find(|s| s.name == name)
            .ok_or_else(|| "no such server".to_string())?
    };
    let mgr = crate::hermes::manager_for(app)?;
    if !mgr.is_running() {
        return Err(NOT_RUNNING.into());
    }
    // The probe can take up to 20 s; the registry lock is not held meanwhile.
    let outcome = mgr
        .mcp_probe(&entry_json(&entry))
        .map_err(|e| e.to_string())?;
    let mut g = gate().lock().unwrap_or_else(|e| e.into_inner());
    // The entry may have been edited while the probe ran: record against what was probed, and
    // build the review only if it is still the same.
    let current = store(app)?
        .load()?
        .servers
        .into_iter()
        .find(|s| s.name == name);
    match outcome {
        ProbeOutcome::Invalid(errors) => {
            g.forget(name);
            Ok(ReviewResult {
                errors,
                review: None,
            })
        }
        ProbeOutcome::Report(report) => {
            g.record(&entry, &report);
            match current {
                Some(cur) if fingerprint(&cur) == fingerprint(&entry) => Ok(ReviewResult {
                    errors: Vec::new(),
                    review: Some(review_view(&g, &cur, report)),
                }),
                _ => Err("the server changed while it was being checked; check it again".into()),
            }
        }
    }
}

/// Enable a reviewed server and rewrite the allowlist. Applies the next time Hermes starts.
#[tauri::command]
pub async fn mcp_server_enable(
    app: tauri::AppHandle,
    name: String,
    review_token: String,
) -> Result<Vec<ServerView>, String> {
    crate::blocking::off_main(move || {
        let g = gate().lock().unwrap_or_else(|e| e.into_inner());
        let st = store(&app)?;
        let mut reg = st.load()?;
        apply_enable(&mut reg, &g, &name, &review_token)?;
        st.save(&reg)?;
        Ok(views(&reg))
    })
    .await
}

/// The servers the running sidecar loaded (`GET /mcp/servers`), or `{running: false}`.
#[tauri::command]
pub async fn mcp_servers_runtime(app: tauri::AppHandle) -> Result<serde_json::Value, String> {
    crate::blocking::off_main(move || {
        let mgr = crate::hermes::manager_for(&app)?;
        if !mgr.is_running() {
            return Ok(serde_json::json!({ "running": false }));
        }
        let mut v = mgr.mcp_servers_status().map_err(|e| e.to_string())?;
        if let Some(o) = v.as_object_mut() {
            o.insert("running".into(), true.into());
        }
        Ok(v)
    })
    .await
}

#[cfg(test)]
mod tests {
    include!("mcp_servers_tests.rs");
}
