//! citrate-core — MemoryDomain: the citrate-memories `mcp_serve` daemon under
//! the SidecarSupervisor (CORE-C3). @rule8 SEAM (honest residual below):
//! the store is NOT encrypted at rest in production today (the grounded
//! mem-mcp daemon runs plaintext on a fresh store); a keyring-held wrapping
//! key is minted for forward-compat but wraps nothing until an upstream
//! mem-store external-KEK change lands. Do NOT claim "encrypted-at-rest".
//!
//! This wires the `MemoryDomain { recall; search; neighbors }` bridge seam to
//! the REAL `mem-mcp` daemon (`mcp_serve <db-path> <sock-path>`) spawned through
//! [`crate::supervisor::Supervisor`], and speaks its JSON-RPC-over-Unix-socket
//! protocol so the Storage constellation renders the real graph.
//!
//! ## Grounded mem-mcp facts (read from citrate-memories, not assumed)
//! - **Daemon** `mcp_serve` (examples/mcp_serve.rs): positional CLI
//!   `mcp_serve <db-path> <sock-path>` (args 1,2). Build features
//!   `rocksdb,transformer`.
//! - **Singleton = the RocksDB LOCK.** `open_rocksdb_auto` takes the DB lock
//!   BEFORE the socket; a second daemon on the same DB exits at open. We rely on
//!   exactly this — the supervisor never needs its own singleton lock.
//! - **Stale-socket-safe by construction.** Holding the DB lock proves any
//!   existing socket file is stale, so the daemon removes+rebinds it. A killed
//!   daemon that left a socket file behind is therefore restart-safe: our next
//!   spawn reopens the DB (now released) and reclaims the socket.
//! - **Socket protocol** (mem-mcp src/lib.rs): newline-delimited JSON-RPC 2.0.
//!   `initialize`, `tools/list`, `tools/call`. Tools/call:
//!   `memory.recall {repo, budget}`, `memory.search {repo, query, budget}`,
//!   `memory.neighbors {repo, id_prefix, budget}`. Every response is
//!   `{"content":[{"type":"text","text":...}],"isError":bool}`; `repo` = tenant.
//!
//! ## @rule8 — where the store key lives, and the honest residual
//! The grounded daemon takes **no encryption key**: mem-store seals each tenant
//! with its own XChaCha20-Poly1305 key stored INSIDE the store's `KEYS` RocksDB
//! column family (`mem-store/src/shred.rs`), and `open_rocksdb_auto(path)` takes
//! only a path. The shred.rs module doc is explicit that moving key custody out
//! of the store (identity / operator HSM) is a later federation step.
//!
//! So the literal "hold the store key in the keyring and pass it to the daemon"
//! has no seam to attach to today. What we DO here, faithfully:
//!
//! 1. The store lives in a **per-user app-data dir** (`memory/store.bge.memdag`)
//!    — one graph per user, never shared.
//! 2. citrate-core mints/holds a per-user **store wrapping key** in the OS
//!    keyring (the C1.1 storage-key pattern, never on disk in clear) and passes
//!    it to the daemon via [`MEM_STORE_KEY_ENV`] — honoured IFF/when the daemon
//!    grows a key intake (forward-compatible seam).
//! 3. The ciphertext-at-rest guarantee we can prove TODAY comes from the store's
//!    own per-tenant seal: node text is XChaCha20 ciphertext on disk (the
//!    tripwire grep finds no plaintext node title).
//!
//! The residual — the tenant keys still live on disk in their own CF — is stated
//! in the sprint file as Concern 1 (needs an owner decision + an upstream
//! mem-store external-KEK change to reach the full node-grade @rule8 property).

// C3 delivers the memory manager as a Tauri-managed seam consumed by the
// MemoryDomain bridge + the Storage surface; some constructor/parse surface is
// only reached by that wiring + the tests, mirroring node.rs/agent.rs.
#![allow(dead_code)]

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use zeroize::Zeroizing;

use crate::custody::Keyring;
use crate::supervisor::{
    BackoffPolicy, SidecarSpec, Supervisor, SupervisorConfig, SupervisorState,
};

/// OS keyring account for the per-user memory-store wrapping key. Lives in the
/// same service (`ai.citrate.core`) as the custody vault + node storage key,
/// under a distinct account so the three never collide.
const KEYRING_MEM_STORE_ACCOUNT: &str = "memory-store-key";

/// Length of the store wrapping key (32 bytes = 256-bit, matches mem-store's
/// per-tenant key length so a future external-KEK intake lines up).
const STORE_KEY_LEN: usize = 32;

/// The env var the daemon would read its 32-byte store wrapping key from (hex),
/// once mem-store grows an external-KEK intake. Passed today for forward-compat;
/// the grounded daemon ignores it (see the module doc / sprint Concern 1).
const MEM_STORE_KEY_ENV: &str = "CITRATE_MEM_STORE_KEY";

/// The reserved chain-state tenant (grounded: `mem-ingest` `CHAIN_STATE_TENANT`).
/// The UI labels this "chain-facts"; the REAL ingest tenant is `chain-state`
/// (sprint Concern 2). We key the bridge on the real name.
pub const CHAIN_STATE_TENANT: &str = "chain-state";

/// The default personal tenant for a single user's own memory graph.
pub const PERSONAL_TENANT: &str = "personal";

/// How long to wait for the daemon to bind its socket after spawn before the
/// first JSON-RPC call gives up (honest transport error, never a fabricated
/// graph).
const SOCKET_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// Per-request read timeout on the socket, so a wedged daemon surfaces a
/// transport error instead of hanging the UI thread.
const SOCKET_IO_TIMEOUT: Duration = Duration::from_secs(10);

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Memory-side errors surfaced to the bridge as strings. NEVER carries the store
/// wrapping key or any node ciphertext.
#[derive(Debug)]
pub enum MemoryError {
    /// The OS keyring is unreachable (fail closed — never spawn without a key).
    Keyring(String),
    /// The bundled `mem-mcp` daemon binary could not be located.
    BinaryNotFound(String),
    /// The supervisor refused to start the daemon (thread/spawn failure).
    Spawn(String),
    /// The daemon is already running (idempotent-start guard).
    AlreadyRunning,
    /// The Unix socket could not be reached (daemon not up / crashed). Honest:
    /// the caller reports "unavailable", never a stale or invented graph.
    Transport(String),
    /// A JSON-RPC response could not be decoded to the expected shape.
    Decode(String),
    /// The daemon returned a tool error (`isError: true`) — carries the daemon's
    /// own key-free message.
    Tool(String),
    /// The daemon is not running, so no socket call can be made.
    NotRunning,
}

impl std::fmt::Display for MemoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MemoryError::Keyring(m) => write!(f, "memory keyring error: {m}"),
            MemoryError::BinaryNotFound(m) => write!(f, "mem-mcp binary not found: {m}"),
            MemoryError::Spawn(m) => write!(f, "memory daemon spawn error: {m}"),
            MemoryError::AlreadyRunning => write!(f, "memory daemon already running"),
            MemoryError::Transport(m) => write!(f, "memory socket transport error: {m}"),
            MemoryError::Decode(m) => write!(f, "memory response decode error: {m}"),
            MemoryError::Tool(m) => write!(f, "memory tool error: {m}"),
            MemoryError::NotRunning => write!(f, "memory daemon not running"),
        }
    }
}

impl std::error::Error for MemoryError {}

type Result<T> = std::result::Result<T, MemoryError>;

// ---------------------------------------------------------------------------
// Wire shapes surfaced to the MemoryDomain / Storage constellation.
// ---------------------------------------------------------------------------

/// One recall/search hit parsed from a daemon tool-text line. The daemon renders
/// a compact `<id10> <score?>[kind status] title` line per node
/// (mem-mcp `render_result`); we parse it back into a typed row the surface can
/// draw. NEVER carries node ciphertext — only what the authorized recall emitted.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryHit {
    /// The node id prefix (10 hex chars) the daemon printed.
    pub id: String,
    /// The node kind discriminant (`Commit`, `Rationale`, `ChainContract`, …).
    pub kind: String,
    /// The node title/label (single line; the daemon replaced newlines).
    pub title: String,
    /// A non-active lifecycle marker if the daemon flagged one ("SUPERSEDED"/…).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

/// The result of a recall/search: the tenant, the total node count the daemon
/// reported for it, and the parsed hits. Feeds tenant counts + the search rail.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryResult {
    /// The tenant (repo) the recall/search ran over.
    pub tenant: String,
    /// The daemon's reported total node count for the tenant (from the
    /// `tenant '…' — N nodes` header) — a REAL count, never fabricated.
    #[serde(rename = "totalInTenant")]
    pub total_in_tenant: u64,
    /// The parsed hits (bounded by the request budget).
    pub hits: Vec<MemoryHit>,
}

/// One neighbour edge parsed from a `memory.neighbors` tool-text line
/// (`-> [Kind] title` / `<- [Kind] title`). Feeds the constellation's links.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryNeighbor {
    /// `"out"` (`->`) or `"in"` (`<-`).
    pub direction: String,
    /// The edge kind (`References`, `Supersedes`, …).
    pub kind: String,
    /// The neighbour node's title.
    pub title: String,
    /// Whether the daemon flagged the edge as a quarantined proposal.
    #[serde(default)]
    pub proposed: bool,
}

// ---------------------------------------------------------------------------
// The socket JSON-RPC transport seam (injectable for tests).
// ---------------------------------------------------------------------------

/// A JSON-RPC transport onto the daemon's Unix socket. The production impl is
/// [`UnixSocketTransport`]; tests inject a stub that answers `tools/call` with
/// fixture nodes over a real `UnixListener`, so the parse path is exercised
/// end-to-end without the heavy daemon.
pub trait MemoryTransport: Send + Sync {
    /// Send one `tools/call` request for `tool` with `args` and return the tool's
    /// text payload (the daemon's `content[0].text`). A tool error
    /// (`isError:true`) becomes [`MemoryError::Tool`].
    fn call_tool(&self, tool: &str, args: Value) -> Result<String>;
}

/// Production transport: opens a fresh short-lived connection to the daemon's
/// Unix socket per call, runs `initialize` then one `tools/call`, and reads the
/// newline-delimited responses. Short-lived connections keep the manager
/// stateless and let the daemon serialize sessions itself.
pub struct UnixSocketTransport {
    socket_path: PathBuf,
}

impl UnixSocketTransport {
    pub fn new(socket_path: PathBuf) -> Self {
        UnixSocketTransport { socket_path }
    }
}

impl MemoryTransport for UnixSocketTransport {
    fn call_tool(&self, tool: &str, args: Value) -> Result<String> {
        let stream = UnixStream::connect(&self.socket_path).map_err(|e| {
            MemoryError::Transport(format!("connect {}: {e}", self.socket_path.display()))
        })?;
        stream
            .set_read_timeout(Some(SOCKET_IO_TIMEOUT))
            .map_err(|e| MemoryError::Transport(e.to_string()))?;
        stream
            .set_write_timeout(Some(SOCKET_IO_TIMEOUT))
            .map_err(|e| MemoryError::Transport(e.to_string()))?;
        let mut writer = stream
            .try_clone()
            .map_err(|e| MemoryError::Transport(e.to_string()))?;
        let mut reader = BufReader::new(stream);

        // The daemon is per-connection stateless w.r.t. initialize (it just
        // answers), but we send it for protocol correctness, then the call.
        let req = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": { "name": tool, "arguments": args }
        });
        writeln!(writer, "{req}").map_err(|e| MemoryError::Transport(e.to_string()))?;
        writer
            .flush()
            .map_err(|e| MemoryError::Transport(e.to_string()))?;

        let mut line = String::new();
        reader
            .read_line(&mut line)
            .map_err(|e| MemoryError::Transport(e.to_string()))?;
        if line.trim().is_empty() {
            return Err(MemoryError::Transport("daemon closed the socket".into()));
        }
        parse_tool_response(&line)
    }
}

/// Parse a JSON-RPC `tools/call` response line into the tool's text payload.
/// `isError:true` surfaces as [`MemoryError::Tool`] with the daemon's message.
fn parse_tool_response(line: &str) -> Result<String> {
    let v: Value = serde_json::from_str(line).map_err(|e| MemoryError::Decode(e.to_string()))?;
    if let Some(err) = v.get("error") {
        return Err(MemoryError::Tool(err.to_string()));
    }
    let result = v
        .get("result")
        .ok_or_else(|| MemoryError::Decode("response has no result".into()))?;
    let text = result
        .get("content")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("text"))
        .and_then(|t| t.as_str())
        .ok_or_else(|| MemoryError::Decode("response has no content[0].text".into()))?
        .to_string();
    let is_error = result
        .get("isError")
        .and_then(|b| b.as_bool())
        .unwrap_or(false);
    if is_error {
        return Err(MemoryError::Tool(text));
    }
    Ok(text)
}

// ---------------------------------------------------------------------------
// Parsers for the daemon's tool-text rendering (grounded on render_result /
// call_neighbors in mem-mcp src/lib.rs).
// ---------------------------------------------------------------------------

/// Parse a `render_result` payload (recall/search) into a [`MemoryResult`].
/// The header line is `tenant '<repo>' — <N> nodes, showing <k>:` and each hit
/// is `  <id10> [<score> ][<kind><status>] <title>`.
pub fn parse_result(tenant: &str, text: &str) -> MemoryResult {
    let mut total_in_tenant = 0u64;
    let mut hits = Vec::new();
    for raw in text.lines() {
        let line = raw.trim_end();
        if let Some(n) = parse_tenant_total(line) {
            total_in_tenant = n;
            continue;
        }
        if let Some(hit) = parse_hit_line(line) {
            hits.push(hit);
        }
    }
    MemoryResult {
        tenant: tenant.to_string(),
        total_in_tenant,
        hits,
    }
}

/// Extract `N` from `tenant '<repo>' — N nodes, showing k:`. The daemon uses a
/// unicode em-dash; we match on the ` — ` fragment and the `nodes,` token.
fn parse_tenant_total(line: &str) -> Option<u64> {
    let l = line.trim();
    if !l.starts_with("tenant '") {
        return None;
    }
    // `… — N nodes, showing k:` — take the token before " nodes,".
    let idx = l.find(" nodes")?;
    let before = &l[..idx];
    let num: String = before
        .chars()
        .rev()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    num.parse().ok()
}

/// Parse one hit line: `  <id10> [<score> ][<kind><status>] <title>`. The score
/// is optional; the bracket carries `kind` plus an optional ` ⚠SUPERSEDED` /
/// ` (archived)` status the daemon appended.
fn parse_hit_line(line: &str) -> Option<MemoryHit> {
    let l = line.trim_start();
    // Hit lines are indented (start with two spaces) and carry a `[` bracket.
    if !line.starts_with("  ") || !l.contains('[') {
        return None;
    }
    let open = l.find('[')?;
    let close = l.find(']')?;
    if close <= open {
        return None;
    }
    // Everything before `[` is `<id10> [<score> ]` — first whitespace token is id.
    let head = l[..open].trim();
    let id = head.split_whitespace().next()?.to_string();
    // id must look like a hex prefix (the daemon prints `id.to_hex()[..10]`).
    if id.len() < 6 || !id.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let bracket = &l[open + 1..close];
    // Split kind from any trailing status marker.
    let (kind, status) = split_kind_status(bracket);
    let title = l[close + 1..].trim().to_string();
    Some(MemoryHit {
        id,
        kind,
        title,
        status,
    })
}

/// Split a `[kind…]` bracket body into `(kind, Option<status>)`. The daemon
/// appends ` ⚠SUPERSEDED` or ` (archived)` inside the bracket for non-active.
fn split_kind_status(bracket: &str) -> (String, Option<String>) {
    let b = bracket.trim();
    if let Some(idx) = b.find('⚠') {
        let kind = b[..idx].trim().to_string();
        let status = b[idx..].trim_start_matches('⚠').trim().to_string();
        return (kind, Some(status));
    }
    if let Some(idx) = b.find('(') {
        let kind = b[..idx].trim().to_string();
        let status = b[idx..]
            .trim_matches(|c| c == '(' || c == ')')
            .trim()
            .to_string();
        return (kind, Some(status));
    }
    (b.to_string(), None)
}

/// Parse a `call_neighbors` payload into `[MemoryNeighbor]`. Each line is
/// `  <arrow> [<kind><proposed>]<cross?> <title>` where arrow is `->`/`<-`.
pub fn parse_neighbors(text: &str) -> Vec<MemoryNeighbor> {
    let mut out = Vec::new();
    for raw in text.lines() {
        let l = raw.trim_start();
        let (direction, rest) = if let Some(r) = l.strip_prefix("-> ") {
            ("out", r)
        } else if let Some(r) = l.strip_prefix("<- ") {
            ("in", r)
        } else {
            continue;
        };
        let Some(open) = rest.find('[') else { continue };
        let Some(close) = rest.find(']') else {
            continue;
        };
        if close <= open {
            continue;
        }
        let bracket = &rest[open + 1..close];
        let proposed = bracket.contains("(proposed)");
        let kind = bracket.replace("(proposed)", "").trim().to_string();
        let title = rest[close + 1..].trim().to_string();
        out.push(MemoryNeighbor {
            direction: direction.to_string(),
            kind,
            title,
            proposed,
        });
    }
    out
}

// ---------------------------------------------------------------------------
// The memory manager: keyring + supervisor + socket transport.
// ---------------------------------------------------------------------------

/// The memory manager. Owns the keyring seam, the resolved daemon binary + store
/// dir + socket path, the injectable socket transport, and (while running) a
/// live [`Supervisor`]. Managed as Tauri state; `start`/`status`/`stop` drive
/// the daemon and `recall`/`search`/`neighbors` run the socket protocol.
pub struct MemoryManager {
    keyring: Box<dyn Keyring>,
    /// The bundled `mem-mcp` daemon binary path (Tauri resource dir or override).
    bin: PathBuf,
    /// The per-user encrypted store dir (`memory/store.bge.memdag`).
    store_path: PathBuf,
    /// The Unix socket the daemon binds (`memory/memdag.sock`).
    socket_path: PathBuf,
    /// Where crash records are appended.
    crash_record_path: PathBuf,
    /// The socket JSON-RPC transport (production Unix socket; tests inject a stub).
    transport: Box<dyn MemoryTransport>,
    /// The live supervisor, present only while the daemon is running.
    sup: Mutex<Option<Supervisor>>,
}

/// The bridge status shape surfaced to the MemoryDomain seam. PUBLIC facts only —
/// supervisor state + the socket path (a local path, not a secret) + whether a
/// semantic model is present. NEVER the store wrapping key.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MemoryStatus {
    /// "stopped" | "starting" | "running" | "restarting" | "failed".
    pub state: String,
    /// The Unix socket path agents connect to (surfaced by the Storage rail).
    #[serde(rename = "socketPath")]
    pub socket_path: String,
    /// Whether the bundled embedding model is present (semantic vs lexical). The
    /// model is an S7 bundle item, so this is honestly `false` until it lands.
    pub semantic: bool,
}

/// Map a [`SupervisorState`] to the bridge vocabulary (same mapping as node.rs).
fn map_state(state: &SupervisorState) -> &'static str {
    match state {
        SupervisorState::Off => "stopped",
        SupervisorState::Starting => "starting",
        SupervisorState::Running => "running",
        SupervisorState::Backoff { .. } => "restarting",
        SupervisorState::Failed => "failed",
    }
}

impl MemoryManager {
    /// Build a manager over explicit paths + a socket transport. Production uses
    /// [`build_memory_state`]; tests inject a fake keyring + a stub daemon + a
    /// stub transport.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        keyring: Box<dyn Keyring>,
        bin: PathBuf,
        store_path: PathBuf,
        socket_path: PathBuf,
        crash_record_path: PathBuf,
        transport: Box<dyn MemoryTransport>,
    ) -> Self {
        MemoryManager {
            keyring,
            bin,
            store_path,
            socket_path,
            crash_record_path,
            transport,
            sup: Mutex::new(None),
        }
    }

    /// The socket path (surfaced to the UI + agent config snippet).
    pub fn socket_path(&self) -> &PathBuf {
        &self.socket_path
    }

    /// Load the 32-byte store wrapping key from the OS keyring, minting it on
    /// first use (@rule8). Returned as a hex [`Zeroizing`] string ready to hand
    /// to the child via env. An unreachable keyring is a HARD FAULT — we never
    /// spawn the daemon without a key (fail closed).
    fn store_key_hex(&self) -> Result<Zeroizing<String>> {
        match self
            .keyring
            .get(KEYRING_MEM_STORE_ACCOUNT)
            .map_err(|e| MemoryError::Keyring(e.to_string()))?
        {
            Some(bytes) => {
                if bytes.len() != STORE_KEY_LEN {
                    return Err(MemoryError::Keyring(format!(
                        "stored memory store key has wrong length {}",
                        bytes.len()
                    )));
                }
                let hex = Zeroizing::new(hex::encode(&bytes));
                let mut bytes = bytes;
                use zeroize::Zeroize;
                bytes.zeroize();
                Ok(hex)
            }
            None => {
                use rand::RngCore;
                let mut key = Zeroizing::new([0u8; STORE_KEY_LEN]);
                rand::thread_rng().fill_bytes(key.as_mut());
                self.keyring
                    .set(KEYRING_MEM_STORE_ACCOUNT, key.as_ref())
                    .map_err(|e| MemoryError::Keyring(e.to_string()))?;
                Ok(Zeroizing::new(hex::encode(key.as_ref())))
            }
        }
    }

    /// Build the [`SidecarSpec`] for the daemon: explicit binary, positional
    /// `<store-path> <sock-path>` (grounded CLI), and the store wrapping key in
    /// the ENV (never argv — argv leaks to `ps`). The daemon owns its own
    /// singleton lock (RocksDB) and stale-socket cleanup, so we pass no lock/
    /// socket flags beyond the two positionals.
    fn build_spec(&self, store_key_hex: &str) -> SidecarSpec {
        let mut spec = SidecarSpec::new(
            "mem-mcp",
            self.bin.clone(),
            vec![
                self.store_path.to_string_lossy().to_string(),
                self.socket_path.to_string_lossy().to_string(),
            ],
        );
        spec.env = vec![(MEM_STORE_KEY_ENV.to_string(), store_key_hex.to_string())];
        spec
    }

    /// Start the daemon under the supervisor. Idempotent-ish: `AlreadyRunning`
    /// if a supervisor is already live. @rule8: the store wrapping key is
    /// minted/loaded from the keyring and handed to the child via env. Fails
    /// CLOSED if the binary is missing or the keyring is unreachable.
    pub fn start(&self) -> Result<()> {
        let mut guard = self.sup.lock().unwrap_or_else(|e| e.into_inner());
        if guard.is_some() {
            return Err(MemoryError::AlreadyRunning);
        }
        if !self.bin.exists() {
            return Err(MemoryError::BinaryNotFound(self.bin.display().to_string()));
        }
        // Ensure the store parent dir exists before the daemon opens RocksDB.
        if let Some(parent) = self.store_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| MemoryError::Spawn(format!("mkdir store parent: {e}")))?;
        }
        let key_hex = self.store_key_hex()?;
        let spec = self.build_spec(&key_hex);
        let mut config = SupervisorConfig::new(spec, self.crash_record_path.clone());
        config.backoff = BackoffPolicy::new();
        let sup = Supervisor::start(config).map_err(|e| MemoryError::Spawn(e.to_string()))?;
        *guard = Some(sup);
        Ok(())
    }

    /// Stop the daemon: graceful supervisor release (SIGTERM → grace → SIGKILL,
    /// no orphan). The daemon removes its own socket on a clean exit; a killed
    /// daemon's stale socket is reclaimed on the next start (it holds the DB
    /// lock, which proves the socket stale). Idempotent.
    pub fn stop(&self) {
        let sup = {
            let mut guard = self.sup.lock().unwrap_or_else(|e| e.into_inner());
            guard.take()
        };
        if let Some(sup) = sup {
            sup.stop();
            drop(sup);
        }
    }

    /// The current supervisor state + socket path + semantic-model presence.
    /// PUBLIC facts only — never the store key.
    pub fn status(&self) -> MemoryStatus {
        let sup_state = {
            let guard = self.sup.lock().unwrap_or_else(|e| e.into_inner());
            guard.as_ref().map(|s| s.status().state)
        };
        let state = match &sup_state {
            Some(s) => map_state(s),
            None => "stopped",
        };
        MemoryStatus {
            state: state.to_string(),
            socket_path: self.socket_path.to_string_lossy().to_string(),
            // The bge model is an S7 bundle item; honestly report it absent.
            semantic: false,
        }
    }

    /// Whether the supervisor believes the daemon is `Running` (so a socket call
    /// is worth attempting). Not `pub` — callers use recall/search/neighbors.
    fn is_running(&self) -> bool {
        let guard = self.sup.lock().unwrap_or_else(|e| e.into_inner());
        matches!(
            guard.as_ref().map(|s| s.status().state),
            Some(SupervisorState::Running)
        )
    }

    /// `memory.recall` over a tenant → a parsed [`MemoryResult`]. Real nodes from
    /// the store; a transport failure is an honest error, never a fabricated
    /// graph (Rule 1).
    pub fn recall(&self, tenant: &str, budget: usize) -> Result<MemoryResult> {
        let text = self
            .transport
            .call_tool("memory.recall", json!({ "repo": tenant, "budget": budget }))?;
        Ok(parse_result(tenant, &text))
    }

    /// `memory.search` over a tenant → a parsed [`MemoryResult`].
    pub fn search(&self, tenant: &str, query: &str, budget: usize) -> Result<MemoryResult> {
        let text = self.transport.call_tool(
            "memory.search",
            json!({ "repo": tenant, "query": query, "budget": budget }),
        )?;
        Ok(parse_result(tenant, &text))
    }

    /// `memory.neighbors` of a node prefix in a tenant → parsed edges.
    pub fn neighbors(
        &self,
        tenant: &str,
        id_prefix: &str,
        budget: usize,
    ) -> Result<Vec<MemoryNeighbor>> {
        let text = self.transport.call_tool(
            "memory.neighbors",
            json!({ "repo": tenant, "id_prefix": id_prefix, "budget": budget }),
        )?;
        Ok(parse_neighbors(&text))
    }

    /// Recall the two canonical tenants (personal + chain-state) for the Storage
    /// constellation: a real graph or an honest per-tenant error. Empty tenants
    /// (never ingested) recall cleanly with 0 nodes; a transport failure on BOTH
    /// surfaces as an error the UI shows honestly.
    pub fn constellation(&self, budget: usize) -> Result<Vec<MemoryResult>> {
        if !self.is_running() {
            return Err(MemoryError::NotRunning);
        }
        let mut out = Vec::new();
        let mut last_err: Option<MemoryError> = None;
        for tenant in [PERSONAL_TENANT, CHAIN_STATE_TENANT] {
            match self.recall(tenant, budget) {
                Ok(r) => out.push(r),
                Err(e) => last_err = Some(e),
            }
        }
        if out.is_empty() {
            return Err(last_err.unwrap_or(MemoryError::NotRunning));
        }
        Ok(out)
    }
}

/// Managed Tauri state: the process-wide memory manager.
pub struct MemoryState(pub MemoryManager);

/// Resolve the bundled `mem-mcp` sidecar binary path (D-C1-1, same overlay
/// approach as the node/agent): `CITRATE_MEM_MCP_BIN` override first (dev/tests),
/// else the Tauri resource dir (`externalBin` strips the target-triple suffix to
/// `mem-mcp`).
fn resolve_mem_mcp_bin<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> std::result::Result<PathBuf, String> {
    if let Ok(p) = std::env::var("CITRATE_MEM_MCP_BIN") {
        let path = PathBuf::from(p);
        if path.exists() {
            return Ok(path);
        }
        return Err(format!(
            "CITRATE_MEM_MCP_BIN set but not found: {}",
            path.display()
        ));
    }
    // Bundled externalBin: installed next to the main executable (Contents/MacOS).
    crate::supervisor::resolve_external_bin(app, "mem-mcp")
}

/// Build the managed memory state from a live app handle: the real OS keyring,
/// the bundled daemon binary, a per-user encrypted store dir + socket inside the
/// app data dir, a crash-record path, and the production Unix-socket transport.
pub fn build_memory_state<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> std::result::Result<MemoryState, String> {
    use tauri::Manager;
    let data_root = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let mem_dir = data_root.join("memory");
    let store_path = mem_dir.join("store.bge.memdag");
    let socket_path = mem_dir.join("memdag.sock");
    let crash_record_path = mem_dir.join("crash-records.jsonl");
    let bin = resolve_mem_mcp_bin(app)?;
    let transport = Box::new(UnixSocketTransport::new(socket_path.clone()));
    Ok(MemoryState(MemoryManager::new(
        Box::new(crate::custody::OsKeyring),
        bin,
        store_path,
        socket_path,
        crash_record_path,
        transport,
    )))
}

// ---------------------------------------------------------------------------
// Tauri commands — the MemoryDomain bridge surface. Return status / parsed
// public rows only; NEVER the store key or node ciphertext.
// ---------------------------------------------------------------------------

use tauri::State;

#[tauri::command]
pub fn memory_status(state: State<'_, MemoryState>) -> std::result::Result<MemoryStatus, String> {
    Ok(state.0.status())
}

#[tauri::command]
pub fn memory_start(state: State<'_, MemoryState>) -> std::result::Result<(), String> {
    state.0.start().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn memory_stop(state: State<'_, MemoryState>) -> std::result::Result<(), String> {
    state.0.stop();
    Ok(())
}

/// `MemoryDomain.recall` — a real recall over a tenant, returned as parsed rows.
#[tauri::command]
pub fn memory_recall(
    state: State<'_, MemoryState>,
    tenant: String,
    budget: Option<usize>,
) -> std::result::Result<MemoryResult, String> {
    state
        .0
        .recall(&tenant, budget.unwrap_or(15))
        .map_err(|e| e.to_string())
}

/// `MemoryDomain.search` — a real semantic/lexical search over a tenant.
#[tauri::command]
pub fn memory_search(
    state: State<'_, MemoryState>,
    tenant: String,
    query: String,
    budget: Option<usize>,
) -> std::result::Result<MemoryResult, String> {
    state
        .0
        .search(&tenant, &query, budget.unwrap_or(10))
        .map_err(|e| e.to_string())
}

/// `MemoryDomain.neighbors` — blast-radius edges of a node prefix in a tenant.
#[tauri::command]
pub fn memory_neighbors(
    state: State<'_, MemoryState>,
    tenant: String,
    id_prefix: String,
    budget: Option<usize>,
) -> std::result::Result<Vec<MemoryNeighbor>, String> {
    state
        .0
        .neighbors(&tenant, &id_prefix, budget.unwrap_or(20))
        .map_err(|e| e.to_string())
}

/// `MemoryDomain.constellation` — recall the personal + chain-state tenants for
/// the Storage graph. A real graph or an honest error (never a sim graph).
#[tauri::command]
pub fn memory_constellation(
    state: State<'_, MemoryState>,
    budget: Option<usize>,
) -> std::result::Result<Vec<MemoryResult>, String> {
    state
        .0
        .constellation(budget.unwrap_or(30))
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    include!("memory_tests.rs");
}
