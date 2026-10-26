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
//!    it stands now. It takes effect the next time Hermes starts.
//!
//! Storage (RC52 P-9a, D-11): `<app local data>/hermes/mcp-servers.json` (`0600`, written
//! atomically) holds every entry with no env values: each env name maps to a reference, and the
//! value lives in the OS keychain (service [`KEYRING_SERVICE`], account [`env_account`]). A
//! reference never changes what it resolves to: a new value gets a new keychain item, so a review
//! (which covers the references) can never be carried over to a value nobody checked.
//! Values leave the keychain only when a server is about to run:
//! - **Hermes start**: core writes the one allowlist the sidecar loads (`hermes_mcp::EFFECTIVE_FILE`,
//!   `0600`) right before the spawn and removes it as soon as the sidecar answers `/health` (it
//!   loads the allowlist before it serves), when Hermes stops, and at app start.
//! - **Review**: the probe request carries the entry, and [`PROBE_FILE`] (the sidecar checks the
//!   request against it) holds exactly that entry for the length of the probe.
//!
//! They never cross back into the webview, which sees masks. Older versions kept values in plain
//! text in the registry and in `mcp-allowlist.json`; [`McpStore::load`] moves them to the keychain
//! (keychain first, then an atomic rewrite of the file, so a crash or failure at any point leaves
//! either the old file or the new one, and a rerun finishes the job) and removes the old allowlist.
//! With no enabled server Hermes runs no member MCP server at all, so a member who never opens
//! this screen sees no change.
//!
//! Every MCP tool is untrusted: its output taints the session (enforced in the runtime), and write
//! tools are not offered unless the member turns them on for that server. Keyless (Rule 3): nothing
//! here holds a key or signs.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zeroize::{Zeroize, Zeroizing};

/// The registry file (every entry, no env values), under `<app local data>/hermes/`.
pub const REGISTRY_FILE: &str = "mcp-servers.json";
/// The env var naming [`PROBE_FILE`] to the Hermes sidecar: its dry-run probe (`POST /mcp/probe`)
/// starts only an entry saved there exactly as sent.
pub const MCP_REGISTRY_ENV: &str = "CITRATE_HERMES_MCP_REGISTRY";
/// The entry being checked, with its env values: written `0600` right before `POST /mcp/probe`
/// and removed when the probe returns ([`McpStore::with_probe_file`]).
pub const PROBE_FILE: &str = "mcp-probe.json";
/// The allowlist older versions kept, with env values in plain text. Nothing reads it now;
/// [`McpStore::load`] and [`McpStore::save`] remove it.
pub const LEGACY_ALLOWLIST_FILE: &str = "mcp-allowlist.json";
/// The OS keychain service the env values live under (the app's keyring namespace).
pub const KEYRING_SERVICE: &str = "ai.citrate.core";
/// Keychain account prefix for env values; see [`env_account`].
pub const ENV_ACCOUNT_PREFIX: &str = "mcp-server-env:";
/// What the webview sees in place of an env value.
pub const MASK: &str = "•••••••• (in the system keychain)";
/// The runtime's limit on configured servers (`agent-mcp-host::config::MAX_SERVERS`).
pub const MAX_SERVERS: usize = 16;

const KEYCHAIN_UNAVAILABLE: &str = "the system keychain is not available";
const KEYCHAIN_SAVE_FAILED: &str =
    "the system keychain could not save an env value, so nothing was saved; try again";

// ---------------------------------------------------------------------------
// Env values in the OS keychain
// ---------------------------------------------------------------------------

/// The keychain account of one env value: `mcp-server-env:<server>:<ENV_NAME>:<reference>`.
/// Server names (`[a-z0-9_-]`) and env names (`[A-Za-z0-9_]`) cannot contain `:`, so the parts
/// never run together. The reference is random and fixed for the item's life.
pub fn env_account(server: &str, key: &str, reference: &str) -> String {
    format!("{ENV_ACCOUNT_PREFIX}{server}:{key}:{reference}")
}

fn new_reference() -> String {
    use rand::RngCore;
    let mut b = [0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut b);
    hex::encode(b)
}

/// Where env values are kept (the OS keychain in production; an in-memory double in tests).
pub trait SecretStore: Send + Sync {
    /// The value for `account`, or `None` if there is none. `Err` = the store cannot be asked.
    fn get(&self, account: &str) -> Result<Option<Zeroizing<String>>, String>;
    /// Store `value` for `account`.
    fn set(&self, account: &str, value: &str) -> Result<(), String>;
    /// Delete `account` (absent is fine).
    fn delete(&self, account: &str) -> Result<(), String>;
}

/// The OS keychain (`keyring` v3, as `ai.rs`).
pub struct OsSecretStore;

impl SecretStore for OsSecretStore {
    fn get(&self, account: &str) -> Result<Option<Zeroizing<String>>, String> {
        let entry = keyring::Entry::new(KEYRING_SERVICE, account)
            .map_err(|_| KEYCHAIN_UNAVAILABLE.to_string())?;
        match entry.get_password() {
            Ok(v) => Ok(Some(Zeroizing::new(v))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err(KEYCHAIN_UNAVAILABLE.to_string()),
        }
    }

    fn set(&self, account: &str, value: &str) -> Result<(), String> {
        keyring::Entry::new(KEYRING_SERVICE, account)
            .and_then(|e| e.set_password(value))
            .map_err(|_| KEYCHAIN_UNAVAILABLE.to_string())
    }

    fn delete(&self, account: &str) -> Result<(), String> {
        let entry = keyring::Entry::new(KEYRING_SERVICE, account)
            .map_err(|_| KEYCHAIN_UNAVAILABLE.to_string())?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err(KEYCHAIN_UNAVAILABLE.to_string()),
        }
    }
}

/// Best effort: remove items a failed change wrote (an item left behind is unreferenced, so it is
/// never read).
fn remove_items(secrets: &dyn SecretStore, accounts: &[String]) {
    for a in accounts {
        let _ = secrets.delete(a);
    }
}

/// Test double: an in-memory keychain that can be told to fail.
#[cfg(test)]
#[derive(Default)]
pub(crate) struct MemSecrets {
    items: Mutex<BTreeMap<String, String>>,
    /// Fail every `set` after this many have succeeded.
    pub fail_set_after: Mutex<Option<usize>>,
    sets: std::sync::atomic::AtomicUsize,
    /// Fail every call (the keychain cannot be asked).
    pub unavailable: std::sync::atomic::AtomicBool,
}

#[cfg(test)]
impl MemSecrets {
    pub fn items(&self) -> BTreeMap<String, String> {
        self.items.lock().map(|m| m.clone()).unwrap_or_default()
    }
    pub fn set_count(&self) -> usize {
        self.sets.load(std::sync::atomic::Ordering::SeqCst)
    }
    fn down(&self) -> bool {
        self.unavailable.load(std::sync::atomic::Ordering::SeqCst)
    }
}

#[cfg(test)]
impl SecretStore for MemSecrets {
    fn get(&self, account: &str) -> Result<Option<Zeroizing<String>>, String> {
        if self.down() {
            return Err(KEYCHAIN_UNAVAILABLE.into());
        }
        Ok(self
            .items
            .lock()
            .map_err(|e| e.to_string())?
            .get(account)
            .map(|v| Zeroizing::new(v.clone())))
    }
    fn set(&self, account: &str, value: &str) -> Result<(), String> {
        let limit = *self.fail_set_after.lock().map_err(|e| e.to_string())?;
        if self.down() || limit.is_some_and(|n| self.set_count() >= n) {
            return Err(KEYCHAIN_UNAVAILABLE.into());
        }
        self.sets.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.items
            .lock()
            .map_err(|e| e.to_string())?
            .insert(account.to_string(), value.to_string());
        Ok(())
    }
    fn delete(&self, account: &str) -> Result<(), String> {
        if self.down() {
            return Err(KEYCHAIN_UNAVAILABLE.into());
        }
        self.items
            .lock()
            .map_err(|e| e.to_string())?
            .remove(account);
        Ok(())
    }
}

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
/// Redacted in `Debug` and wiped on drop.
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EnvInput {
    pub key: String,
    #[serde(default)]
    pub value: Option<String>,
}

impl std::fmt::Debug for EnvInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EnvInput")
            .field("key", &self.key)
            .field("value", &self.value.as_ref().map(|_| "<redacted>"))
            .finish()
    }
}

impl Drop for EnvInput {
    fn drop(&mut self) {
        if let Some(v) = self.value.as_mut() {
            v.zeroize();
        }
    }
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

/// Env values as versions before RC52 stored them: in plain text in the registry file. Read only
/// so [`McpStore::load`] can move them to the keychain; never written back. Redacted in `Debug`
/// and wiped on drop.
#[derive(Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(transparent)]
pub struct LegacyEnv(BTreeMap<String, String>);

impl LegacyEnv {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl std::fmt::Debug for LegacyEnv {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "LegacyEnv({} values, redacted)", self.0.len())
    }
}

impl Drop for LegacyEnv {
    fn drop(&mut self) {
        for v in self.0.values_mut() {
            v.zeroize();
        }
    }
}

/// One stored entry. Holds no env value: each env name maps to the reference of the keychain item
/// that holds its value ([`env_account`]).
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
    /// Env name → keychain reference.
    #[serde(default)]
    pub env_refs: BTreeMap<String, String>,
    /// Plain-text values from an older version (migration input only; never serialized).
    #[serde(default, rename = "env", skip_serializing)]
    pub legacy_env: LegacyEnv,
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
/// (names and keychain references), and the write-tools switch. A reference names one immutable
/// keychain item, so a new value is a new fingerprint. Stored on disk only.
pub fn fingerprint(s: &StoredServer) -> String {
    let material = serde_json::json!({
        "name": s.name,
        "transport": s.transport,
        "command": s.command,
        "args": s.args,
        "cwd": s.cwd,
        "url": s.url,
        "env_refs": s.env_refs,
        "allow_write_tools": s.allow_write_tools,
    });
    hex::encode(Sha256::digest(material.to_string().as_bytes()))
}

/// The fingerprint an older version stored, over the plain-text values (migration only: a server
/// reviewed then stays reviewed once its values are in the keychain).
fn legacy_fingerprint(s: &StoredServer) -> String {
    let mut material = serde_json::json!({
        "name": s.name,
        "transport": s.transport,
        "command": s.command,
        "args": s.args,
        "cwd": s.cwd,
        "url": s.url,
        "env": s.legacy_env.0,
        "allow_write_tools": s.allow_write_tools,
    });
    let text = Zeroizing::new(material.to_string());
    if let Some(env) = material.get_mut("env").and_then(|e| e.as_object_mut()) {
        for v in env.values_mut() {
            if let serde_json::Value::String(v) = v {
                v.zeroize();
            }
        }
    }
    hex::encode(Sha256::digest(text.as_bytes()))
}

fn review_matches(s: &StoredServer) -> bool {
    s.reviewed.as_deref() == Some(fingerprint(s).as_str())
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
            .env_refs
            .keys()
            .map(|k| EnvView {
                key: k.clone(),
                masked: MASK.to_string(),
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

/// The keychain items a registry change wrote and the ones it no longer references. Account names
/// only. [`McpStore::commit`] removes `written` if the file cannot be saved, and `obsolete` once
/// it is.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct SaveEffects {
    pub written: Vec<String>,
    pub obsolete: Vec<String>,
}

/// Add or edit an entry. A new or changed entry is stored disabled; an edit that changes nothing
/// that runs keeps its review. New env values are written to the keychain here, before the caller
/// saves the file (an item the file never references is never read). A value retyped unchanged
/// keeps its item; a renamed server's values are copied to items under the new name.
pub fn apply_save(
    reg: &mut Registry,
    input: ServerInput,
    secrets: &dyn SecretStore,
) -> Result<SaveEffects, Vec<FieldError>> {
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
    let stdio = input.transport == "stdio";
    let prev_ref = |key: &str| prev.as_ref().and_then(|p| p.env_refs.get(key));
    if stdio {
        let errs: Vec<FieldError> = input
            .env
            .iter()
            .filter(|e| e.value.is_none() && prev_ref(&e.key).is_none())
            .map(|e| ferr(format!("env.{}", e.key), "enter a value"))
            .collect();
        if !errs.is_empty() {
            return Err(errs);
        }
    }
    let mut effects = SaveEffects::default();
    let mut env_refs = BTreeMap::new();
    if stdio {
        for e in &input.env {
            let prev_item = prev
                .as_ref()
                .zip(prev_ref(&e.key))
                .map(|(p, r)| (env_account(&p.name, &e.key, r), r.clone()));
            let same_name = prev.as_ref().is_some_and(|p| p.name == input.name);
            // Keep the existing item when the server keeps its name and the value is omitted or
            // retyped unchanged.
            let keep = match (&e.value, &prev_item) {
                (None, Some((_, r))) if same_name => Some(r.clone()),
                (Some(v), Some((account, r))) if same_name => match secrets.get(account) {
                    Ok(Some(old)) if subtle_eq(old.as_bytes(), v.as_bytes()) => Some(r.clone()),
                    _ => None,
                },
                _ => None,
            };
            if let Some(r) = keep {
                env_refs.insert(e.key.clone(), r);
                continue;
            }
            let value: Zeroizing<String> = match (&e.value, &prev_item) {
                (Some(v), _) => Zeroizing::new(v.clone()),
                (None, Some((account, _))) => match secrets.get(account) {
                    Ok(Some(v)) => v,
                    Ok(None) => {
                        remove_items(secrets, &effects.written);
                        return Err(vec![ferr(
                            format!("env.{}", e.key),
                            "the saved value is missing from the system keychain; enter it again",
                        )]);
                    }
                    Err(_) => {
                        remove_items(secrets, &effects.written);
                        return Err(vec![ferr("entry", KEYCHAIN_SAVE_FAILED)]);
                    }
                },
                (None, None) => {
                    remove_items(secrets, &effects.written);
                    return Err(vec![ferr(format!("env.{}", e.key), "enter a value")]);
                }
            };
            let r = new_reference();
            let account = env_account(&input.name, &e.key, &r);
            if secrets.set(&account, &value).is_err() {
                remove_items(secrets, &effects.written);
                return Err(vec![ferr("entry", KEYCHAIN_SAVE_FAILED)]);
            }
            effects.written.push(account);
            env_refs.insert(e.key.clone(), r);
        }
    }
    if let Some(p) = &prev {
        for (k, r) in &p.env_refs {
            if !(p.name == input.name && env_refs.get(k) == Some(r)) {
                effects.obsolete.push(env_account(&p.name, k, r));
            }
        }
    }
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
        env_refs,
        legacy_env: LegacyEnv::default(),
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
    Ok(effects)
}

/// Remove an entry. Returns the keychain items it referenced (to remove once the file is saved),
/// or `None` when there was no such entry.
pub fn apply_remove(reg: &mut Registry, name: &str) -> Option<SaveEffects> {
    let idx = reg.servers.iter().position(|s| s.name == name)?;
    let s = reg.servers.remove(idx);
    Some(SaveEffects {
        written: Vec::new(),
        obsolete: s
            .env_refs
            .iter()
            .map(|(k, r)| env_account(&s.name, k, r))
            .collect(),
    })
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

/// The env values of `s` from the keychain, for a spawn or a probe only. Fails closed when an item
/// is missing or the keychain cannot be asked.
fn resolve_env(
    s: &StoredServer,
    secrets: &dyn SecretStore,
) -> Result<BTreeMap<String, Zeroizing<String>>, String> {
    let mut out = BTreeMap::new();
    if s.transport != "stdio" {
        return Ok(out);
    }
    for (k, r) in &s.env_refs {
        match secrets.get(&env_account(&s.name, k, r))? {
            Some(v) => {
                out.insert(k.clone(), v);
            }
            None => {
                return Err(format!(
                    "the value of {k} for MCP server '{}' is missing from the system keychain; edit the server and enter it again",
                    s.name
                ))
            }
        }
    }
    Ok(out)
}

/// The runtime's `[[servers]]` entry (snake_case; only the keys the runtime accepts), borrowing
/// the env values so the only serialized copy is the wiped buffer [`to_secret_json`] writes.
#[derive(Serialize)]
struct RuntimeEntry<'a> {
    name: &'a str,
    transport: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    command: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    args: Option<&'a [String]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    env: Option<BTreeMap<&'a str, &'a str>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cwd: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    url: Option<&'a str>,
    allow_write_tools: bool,
}

fn runtime_entry<'a>(
    s: &'a StoredServer,
    env: &'a BTreeMap<String, Zeroizing<String>>,
) -> RuntimeEntry<'a> {
    let stdio = s.transport == "stdio";
    RuntimeEntry {
        name: &s.name,
        transport: &s.transport,
        command: s.command.as_deref().filter(|_| stdio),
        args: stdio.then_some(s.args.as_slice()),
        env: stdio.then(|| env.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect()),
        cwd: s.cwd.as_deref().filter(|_| stdio),
        url: s.url.as_deref().filter(|_| !stdio),
        allow_write_tools: s.allow_write_tools,
    }
}

/// Serialize into a buffer that is wiped on drop. The buffer is sized up front so it is not
/// reallocated (which would leave an unwiped copy behind).
fn to_secret_json<T: Serialize>(v: &T, values_len: usize) -> Result<Zeroizing<String>, String> {
    let mut buf = Zeroizing::new(Vec::with_capacity(2 * values_len + 64 * 1024));
    serde_json::to_writer(&mut *buf, v)
        .map_err(|_| "the MCP entry could not be encoded".to_string())?;
    match String::from_utf8(std::mem::take(&mut *buf)) {
        Ok(text) => Ok(Zeroizing::new(text)),
        Err(e) => {
            e.into_bytes().zeroize();
            Err("the MCP entry could not be encoded".to_string())
        }
    }
}

fn values_len(env: &BTreeMap<String, Zeroizing<String>>) -> usize {
    env.values().map(|v| v.len()).sum()
}

/// `s` in the runtime shape with its env values from the keychain: the probe request body and the
/// content of [`PROBE_FILE`].
pub fn runtime_entry_json(
    s: &StoredServer,
    secrets: &dyn SecretStore,
) -> Result<Zeroizing<String>, String> {
    let env = resolve_env(s, secrets)?;
    to_secret_json(&runtime_entry(s, &env), values_len(&env))
}

/// The members' entries the sidecar may start: enabled, with a review that still matches.
pub fn enabled_servers(reg: &Registry) -> impl Iterator<Item = &StoredServer> {
    reg.servers
        .iter()
        .filter(|s| s.enabled && review_matches(s))
}

/// The allowlist the sidecar loads when it starts: `builtin` servers first, then every enabled
/// entry whose review still matches, with env values from the keychain. `None` = no server at all.
pub fn runtime_allowlist_json(
    reg: &Registry,
    secrets: &dyn SecretStore,
    builtin: &[serde_json::Value],
) -> Result<Option<Zeroizing<String>>, String> {
    let enabled: Vec<&StoredServer> = enabled_servers(reg).collect();
    if builtin.is_empty() && enabled.is_empty() {
        return Ok(None);
    }
    let envs = enabled
        .iter()
        .map(|s| resolve_env(s, secrets))
        .collect::<Result<Vec<_>, _>>()?;
    #[derive(Serialize)]
    #[serde(untagged)]
    enum Item<'a> {
        Builtin(&'a serde_json::Value),
        Member(RuntimeEntry<'a>),
    }
    #[derive(Serialize)]
    struct File<'a> {
        servers: Vec<Item<'a>>,
    }
    let file = File {
        servers: builtin
            .iter()
            .map(Item::Builtin)
            .chain(
                enabled
                    .iter()
                    .zip(envs.iter())
                    .map(|(s, env)| Item::Member(runtime_entry(s, env))),
            )
            .collect(),
    };
    let len = envs.iter().map(values_len).sum::<usize>()
        + builtin.iter().map(|b| b.to_string().len()).sum::<usize>();
    to_secret_json(&file, len).map(Some)
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

const MIGRATE_FAILED: &str = "the MCP server env values could not be moved to the system keychain; nothing was changed and the move is tried again next time";

/// Serialises every read and write of the registry file (the commands, the Hermes start and the
/// migration at app start).
static FILE_LOCK: Mutex<()> = Mutex::new(());

fn remove_if_present(path: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// Remove the files that hold env values only while a server is about to run (the spawn-time
/// allowlist and the probe file). Called when Hermes stops and at app start, so a crash never
/// leaves them behind for long.
pub fn sweep_runtime_files(dir: &Path) {
    for f in [crate::hermes_mcp::EFFECTIVE_FILE, PROBE_FILE] {
        if let Err(e) = remove_if_present(&dir.join(f)) {
            eprintln!("mcp servers: could not remove {f}: {}", e.kind());
        }
    }
}

/// The registry file and the keychain items it references, under `<app local data>/hermes/`.
#[derive(Clone)]
pub struct McpStore {
    dir: PathBuf,
    secrets: Arc<dyn SecretStore>,
}

impl McpStore {
    pub fn new(dir: PathBuf, secrets: Arc<dyn SecretStore>) -> Self {
        McpStore { dir, secrets }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn secrets(&self) -> &dyn SecretStore {
        &*self.secrets
    }

    pub fn registry_path(&self) -> PathBuf {
        self.dir.join(REGISTRY_FILE)
    }

    pub fn probe_path(&self) -> PathBuf {
        self.dir.join(PROBE_FILE)
    }

    pub fn legacy_allowlist_path(&self) -> PathBuf {
        self.dir.join(LEGACY_ALLOWLIST_FILE)
    }

    /// Read the file. Missing = empty; unreadable or corrupt = an error (never silently empty,
    /// which would let the next save drop every entry).
    fn read(&self) -> Result<Registry, String> {
        match std::fs::read_to_string(self.registry_path()) {
            Ok(text) => {
                let text = Zeroizing::new(text);
                serde_json::from_str(&text).map_err(|_| {
                    "the MCP server list could not be read (file is damaged)".to_string()
                })
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Registry::default()),
            Err(e) => Err(format!(
                "the MCP server list could not be read: {}",
                e.kind()
            )),
        }
    }

    /// Write the file atomically (a temp file `0600`, synced, renamed over it).
    fn write(&self, reg: &Registry) -> Result<(), String> {
        let text = serde_json::to_string_pretty(reg).map_err(|e| e.to_string())?;
        citrate_core_kit::fsutil::ensure_private_dir(&self.dir)
            .and_then(|()| crate::hermes_mcp::write_private(&self.registry_path(), text.as_bytes()))
            .map_err(|e| format!("saving the MCP server list: {}", e.kind()))
    }

    fn remove_legacy_allowlist(&self) -> Result<(), String> {
        remove_if_present(&self.legacy_allowlist_path())
            .map_err(|e| format!("removing the old MCP allowlist: {}", e.kind()))
    }

    /// Read the registry, first moving env values an older version stored in plain text into the
    /// keychain, and removing the old allowlist file (which held them too).
    ///
    /// The move writes every keychain item first and then rewrites the file atomically. If an item
    /// cannot be written, the items this attempt wrote are removed and the file is left as it was;
    /// if the file cannot be rewritten, the same. So the file is always either the old one (and the
    /// next load tries again) or the new one (and the next load has nothing to move).
    pub fn load(&self) -> Result<Registry, String> {
        let _f = FILE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut reg = self.read()?;
        if reg.servers.iter().any(|s| !s.legacy_env.is_empty()) {
            self.migrate(&mut reg)?;
        }
        self.remove_legacy_allowlist()?;
        Ok(reg)
    }

    fn migrate(&self, reg: &mut Registry) -> Result<(), String> {
        let mut written = Vec::new();
        for s in reg.servers.iter_mut().filter(|s| !s.legacy_env.is_empty()) {
            let was_reviewed =
                s.enabled && s.reviewed.as_deref() == Some(legacy_fingerprint(s).as_str());
            let legacy = std::mem::take(&mut s.legacy_env);
            for (k, v) in &legacy.0 {
                let r = new_reference();
                let account = env_account(&s.name, k, &r);
                if self.secrets.set(&account, v).is_err() {
                    remove_items(&*self.secrets, &written);
                    return Err(MIGRATE_FAILED.to_string());
                }
                written.push(account);
                s.env_refs.insert(k.clone(), r);
            }
            s.reviewed = was_reviewed.then(|| fingerprint(s));
        }
        if let Err(e) = self.write(reg) {
            remove_items(&*self.secrets, &written);
            eprintln!("mcp servers: {e}");
            return Err(MIGRATE_FAILED.to_string());
        }
        Ok(())
    }

    /// Write the registry (no env values) and remove the old allowlist file.
    pub fn save(&self, reg: &Registry) -> Result<(), String> {
        let _f = FILE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        self.write(reg)?;
        self.remove_legacy_allowlist()
    }

    /// Save after [`apply_save`] or [`apply_remove`]: if the file cannot be written, the keychain
    /// items the change wrote are removed (the old file still references the old ones); once it is
    /// written, the items it no longer references are removed.
    pub fn commit(&self, reg: &Registry, effects: &SaveEffects) -> Result<(), String> {
        if let Err(e) = self.save(reg) {
            remove_items(&*self.secrets, &effects.written);
            return Err(e);
        }
        remove_items(&*self.secrets, &effects.obsolete);
        Ok(())
    }

    /// Run `f` while [`PROBE_FILE`] holds exactly `entry_json` (`0600`), then remove the file. One
    /// probe at a time, so a second check never replaces the entry the first is being checked
    /// against.
    pub fn with_probe_file<T>(&self, entry_json: &str, f: impl FnOnce() -> T) -> Result<T, String> {
        static PROBE_LOCK: Mutex<()> = Mutex::new(());
        let _p = PROBE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        struct RemoveOnDrop(PathBuf);
        impl Drop for RemoveOnDrop {
            fn drop(&mut self) {
                let _ = remove_if_present(&self.0);
            }
        }
        let path = self.probe_path();
        let guard = RemoveOnDrop(path.clone());
        let text = Zeroizing::new(format!("{{\"servers\":[{entry_json}]}}"));
        citrate_core_kit::fsutil::ensure_private_dir(&self.dir)
            .and_then(|()| crate::hermes_mcp::write_private(&path, text.as_bytes()))
            .map_err(|e| format!("preparing the server check: {}", e.kind()))?;
        let out = f();
        drop(guard);
        Ok(out)
    }
}

/// The keychain items a registry file references, with a label, for "delete my local data".
/// A file that cannot be parsed lists nothing.
pub fn keychain_accounts(registry_text: &str) -> Vec<(String, String)> {
    let Ok(reg) = serde_json::from_str::<Registry>(registry_text) else {
        return Vec::new();
    };
    reg.servers
        .iter()
        .flat_map(|s| {
            s.env_refs.iter().map(|(k, r)| {
                (
                    env_account(&s.name, k, r),
                    format!("MCP server {}: {k}", s.name),
                )
            })
        })
        .collect()
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

/// The member's MCP servers in this app's Hermes dir, with values in the OS keychain.
pub fn os_store(dir: PathBuf) -> McpStore {
    McpStore::new(dir, Arc::new(OsSecretStore))
}

fn store(app: &tauri::AppHandle) -> Result<McpStore, String> {
    Ok(os_store(hermes_dir(app)?))
}

/// At app start: remove spawn-time files an earlier run left (they hold env values), then, off the
/// main thread, move any plain-text values an older version stored into the keychain. Without a
/// registry file this touches nothing (no keychain access).
pub fn startup<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    let Ok(dir) = hermes_dir(app) else {
        return;
    };
    sweep_runtime_files(&dir);
    std::thread::spawn(move || {
        let _g = gate().lock().unwrap_or_else(|e| e.into_inner());
        if let Err(e) = os_store(dir).load() {
            eprintln!("mcp servers: {e}");
        }
    });
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
    match apply_save(&mut reg, input, st.secrets()) {
        Ok(effects) => {
            st.commit(&reg, &effects)?;
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
        if let Some(effects) = apply_remove(&mut reg, &name) {
            st.commit(&reg, &effects)?;
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
    let st = store(app)?;
    let entry = {
        let _g = gate().lock().unwrap_or_else(|e| e.into_inner());
        let reg = st.load()?;
        reg.servers
            .into_iter()
            .find(|s| s.name == name)
            .ok_or_else(|| "no such server".to_string())?
    };
    let mgr = crate::hermes::manager_for(app)?;
    if !mgr.is_running() {
        return Err(NOT_RUNNING.into());
    }
    // The probe can take up to 20 s; the registry lock is not held meanwhile. The entry's values
    // leave the keychain only for the request and the probe file, both gone when it returns.
    let body = runtime_entry_json(&entry, st.secrets())?;
    let outcome = st
        .with_probe_file(&body, || mgr.mcp_probe(&body))?
        .map_err(|e| e.to_string())?;
    let mut g = gate().lock().unwrap_or_else(|e| e.into_inner());
    // The entry may have been edited while the probe ran: record against what was probed, and
    // build the review only if it is still the same.
    let current = st.load()?.servers.into_iter().find(|s| s.name == name);
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

/// Enable a reviewed server. Applies the next time Hermes starts.
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
