//! HUP-S4.3 — the MCP servers Hermes may use: mem-mcp (the member's local memory graph) and
//! CitrateScan (the public explorer), configured through the S4.1 MCP host's allowlist file.
//!
//! citrate-agent-runtime's `agent-mcp-host` reads the file named by `CITRATE_HERMES_MCP` once, at
//! sidecar start; unset means no MCP at all. Core owns that file: it renders it from the member's
//! settings ([`McpSettings`]), writes it `0600` next to the Hermes bearer file, and the Hermes
//! manager passes its path to the child only while it exists ([`crate::hermes`] `build_spec`).
//!
//! Servers:
//! - `scan`: CitrateScan's MCP endpoint (`{EXPLORER_BASE}/api/mcp`), HTTP, read-only tools.
//! - `mem`: the citrate-core executable itself run as [`crate::mem_mcp_bridge`] (stdio), which
//!   relays to the local mem-mcp socket and offers only the read tools.
//! - `node` (HUP-S4.1, US-4.1 AC1): this node's own MCP server ([`crate::node_mcp`]) through the
//!   stdio shim (`citrate-core --mcp-stdio`), with a connect token core mints for Hermes at each
//!   start. The token is held only as a hash, in memory ([`crate::node_mcp::NodeMcpState::hermes_token`]);
//!   its plaintext reaches the sidecar only through this 0600 file, which is rewritten at every
//!   start. Offered only while the node's MCP server is on.
//!
//! `mem` and `scan` set `allow_write_tools = false`. `node` sets it true (PENDING OWNER SIGN-OFF,
//! A24): its write tools never act on their own; each becomes a request the member approves in
//! the app, and a transaction goes through the SignatureCeremony. MCP output is always untrusted to
//! the loop, so the first MCP result taints the session and every effectful call after it also
//! needs the member's decision on the sidecar's MCP approval card first.
//!
//! PENDING OWNER SIGN-OFF: every server defaults to OFF ([`McpSettings::default`]), so members see
//! no change until they (or a later default) turn them on. Turning them on by default is the
//! owner's call.
//!
//! Keyless (Rule 3): nothing here holds a key or signs.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// The environment variable the sidecar's MCP host reads (agent-mcp-host `MCP_CONFIG_ENV`).
pub const MCP_CONFIG_ENV: &str = "CITRATE_HERMES_MCP";

/// CitrateScan's MCP endpoint, on the same pinned explorer host the wallet activity read uses.
pub const SCAN_MCP_URL: &str = "https://explorer.citrate.ai/api/mcp";

/// Per-call deadline handed to the host for both servers (ms).
const CALL_TIMEOUT_MS: u64 = 30_000;

/// The member's choices. PENDING OWNER SIGN-OFF on the defaults: both off.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpSettings {
    /// Offer the local memory graph's read tools to Hermes over MCP.
    #[serde(default)]
    pub mem: bool,
    /// Offer CitrateScan's read-only explorer tools to Hermes over MCP.
    #[serde(default)]
    pub scan: bool,
    /// HUP-S4.1: offer this node's own MCP server to Hermes (reads, and writes that wait for the
    /// member's approval in the app).
    #[serde(default)]
    pub node: bool,
}

/// The name of the built-in node entry (reserved for user-added servers in agent-mcp-host).
pub const NODE_SERVER_NAME: &str = "node";
/// The environment the stdio shim reads its connect token and port from.
pub const NODE_TOKEN_ENV: &str = "CITRATE_NODE_MCP_TOKEN";
pub const NODE_PORT_ENV: &str = "CITRATE_NODE_MCP_PORT";

/// Where the node entry runs: this executable as the stdio shim, the node server's loopback port,
/// and the connect token minted for Hermes.
#[derive(Clone, PartialEq, Eq)]
pub struct NodeTarget {
    pub exe: PathBuf,
    pub port: u16,
    pub token: String,
}

impl std::fmt::Debug for NodeTarget {
    // Never print the token.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NodeTarget")
            .field("exe", &self.exe)
            .field("port", &self.port)
            .finish_non_exhaustive()
    }
}

/// Where the memory bridge runs: this executable, pointed at the mem-mcp socket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemBridgeTarget {
    pub exe: PathBuf,
    pub socket: PathBuf,
}

/// The allowlist file inside the Hermes data dir.
pub fn config_path(dir: &Path) -> PathBuf {
    dir.join("mcp.json")
}

/// The persisted settings inside the Hermes data dir.
pub fn settings_path(dir: &Path) -> PathBuf {
    dir.join("mcp-settings.json")
}

/// Render the allowlist (agent-mcp-host JSON shape) or `None` when no server is configured.
pub fn render_config(
    settings: &McpSettings,
    mem: Option<&MemBridgeTarget>,
    node: Option<&NodeTarget>,
) -> Option<Value> {
    let mut servers: Vec<Value> = Vec::new();
    if settings.mem {
        if let Some(t) = mem.filter(|t| t.exe.is_absolute()) {
            servers.push(json!({
                "name": "mem",
                "transport": "stdio",
                "command": t.exe.to_string_lossy(),
                "args": [crate::mem_mcp_bridge::BRIDGE_FLAG, t.socket.to_string_lossy()],
                "timeout_ms": CALL_TIMEOUT_MS,
                "allow_write_tools": false,
            }));
        }
    }
    if settings.scan {
        servers.push(json!({
            "name": "scan",
            "transport": "http",
            "url": SCAN_MCP_URL,
            "timeout_ms": CALL_TIMEOUT_MS,
            "allow_write_tools": false,
        }));
    }
    if settings.node {
        if let Some(t) = node.filter(|t| t.exe.is_absolute() && !t.token.is_empty()) {
            servers.push(json!({
                "name": NODE_SERVER_NAME,
                "transport": "stdio",
                "command": t.exe.to_string_lossy(),
                "args": [crate::node_mcp_http::STDIO_FLAG],
                "env": { NODE_TOKEN_ENV: t.token, NODE_PORT_ENV: t.port.to_string() },
                "timeout_ms": CALL_TIMEOUT_MS,
                // PENDING OWNER SIGN-OFF (A24): writes only create requests the member approves.
                "allow_write_tools": true,
            }));
        }
    }
    (!servers.is_empty()).then(|| json!({ "servers": servers }))
}

fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let tmp = path.with_extension("json.tmp");
    let _ = std::fs::remove_file(&tmp);
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts.open(&tmp)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    drop(f);
    std::fs::rename(&tmp, path)
}

/// HUP-S4.3 + S4.4: the file the child gets when both the built-in servers (this module) and the
/// member's reviewed servers (`mcp_servers.rs`) are on. It sits next to them in the Hermes dir.
pub const EFFECTIVE_FILE: &str = "mcp-effective.json";

/// HUP-S4.3 + S4.4: the one allowlist the sidecar reads (its MCP host reads exactly one file).
/// Neither file: `None` (no MCP). One file: that file as is. Both: their `servers` lists joined into
/// [`EFFECTIVE_FILE`] (0600), built-in servers first; member names never clash with the built-in
/// ones (they are reserved). If either file cannot be read, nothing is passed (fail closed). A
/// joined file left from an earlier start is removed whenever it is not needed, since it can hold
/// the member's server credentials.
pub fn effective_allowlist(builtin: Option<&Path>, user: Option<&Path>) -> Option<PathBuf> {
    let dir = builtin
        .or(user)
        .and_then(Path::parent)
        .map(Path::to_path_buf);
    let joined = dir.as_ref().map(|d| d.join(EFFECTIVE_FILE));
    let drop_joined = || {
        if let Some(j) = &joined {
            let _ = std::fs::remove_file(j);
        }
    };
    let b = builtin.filter(|p| p.is_file());
    let u = user.filter(|p| p.is_file());
    let (b, u) = match (b, u) {
        (None, None) => {
            drop_joined();
            return None;
        }
        (Some(only), None) | (None, Some(only)) => {
            drop_joined();
            return Some(only.to_path_buf());
        }
        (Some(b), Some(u)) => (b, u),
    };
    let read = |p: &Path| -> Option<Vec<Value>> {
        let v: Value = serde_json::from_str(&std::fs::read_to_string(p).ok()?).ok()?;
        v.get("servers")?.as_array().cloned()
    };
    let (Some(mut servers), Some(more), Some(path)) = (read(b), read(u), joined.clone()) else {
        eprintln!("hermes: an MCP allowlist could not be read; Hermes starts without MCP servers");
        drop_joined();
        return None;
    };
    servers.extend(more);
    let written = serde_json::to_vec_pretty(&json!({ "servers": servers }))
        .map_err(std::io::Error::other)
        .and_then(|text| write_private(&path, &text));
    match written {
        Ok(()) => Some(path),
        Err(e) => {
            eprintln!("hermes: could not write the joined MCP allowlist ({e}); Hermes starts without MCP servers");
            drop_joined();
            None
        }
    }
}

/// Write (or remove) the allowlist so it matches `settings`. Returns the path when a file exists.
pub fn sync_config_file(
    dir: &Path,
    settings: &McpSettings,
    mem: Option<&MemBridgeTarget>,
    node: Option<&NodeTarget>,
) -> std::io::Result<Option<PathBuf>> {
    let path = config_path(dir);
    match render_config(settings, mem, node) {
        Some(cfg) => {
            std::fs::create_dir_all(dir)?;
            let text = serde_json::to_vec_pretty(&cfg).map_err(std::io::Error::other)?;
            write_private(&path, &text)?;
            Ok(Some(path))
        }
        None => {
            match std::fs::remove_file(&path) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e),
            }
            Ok(None)
        }
    }
}

/// Load the settings; a missing or unreadable file is the default (both off).
pub fn load_settings(dir: &Path) -> McpSettings {
    std::fs::read_to_string(settings_path(dir))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

pub fn save_settings(dir: &Path, s: &McpSettings) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let text = serde_json::to_vec_pretty(s).map_err(std::io::Error::other)?;
    write_private(&settings_path(dir), &text)
}

/// One row of the settings view.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct McpServerView {
    pub name: String,
    pub label: String,
    pub transport: String,
    pub enabled: bool,
    /// Whether core can configure it on this machine right now.
    pub available: bool,
    pub detail: String,
}

/// What the Agent surface shows.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct McpView {
    pub settings: McpSettings,
    pub servers: Vec<McpServerView>,
    /// Whether an allowlist file is in place for the next sidecar start.
    pub config_written: bool,
    /// The sidecar is running and reads the file only at start: restart Hermes to apply.
    pub restart_required: bool,
}

/// The node token in the allowlist file Hermes was last given, if any (so a still-live token is
/// kept instead of cutting a running Hermes off).
pub fn current_node_token(dir: &Path) -> Option<String> {
    let v: Value = serde_json::from_str(&std::fs::read_to_string(config_path(dir)).ok()?).ok()?;
    v.get("servers")?
        .as_array()?
        .iter()
        .find(|s| s.get("name").and_then(Value::as_str) == Some(NODE_SERVER_NAME))?
        .pointer(&format!("/env/{NODE_TOKEN_ENV}"))?
        .as_str()
        .map(str::to_string)
}

pub fn build_view(
    settings: &McpSettings,
    mem: Option<&MemBridgeTarget>,
    node_available: bool,
    config_written: bool,
    running: bool,
) -> McpView {
    let mem_ok = mem.is_some_and(|t| t.exe.is_absolute());
    McpView {
        settings: *settings,
        servers: vec![
            McpServerView {
                name: "mem".into(),
                label: "Your memory graph".into(),
                transport: "stdio".into(),
                enabled: settings.mem,
                available: mem_ok,
                detail: if mem_ok {
                    "Read-only memory tools (recall, search, neighbors, as-of, verify, critique, analogy). Writes stay behind your approval.".into()
                } else {
                    "The memory bridge is not available on this machine right now, so it will not be configured.".into()
                },
            },
            McpServerView {
                name: "scan".into(),
                label: "CitrateScan explorer".into(),
                transport: "http".into(),
                enabled: settings.scan,
                available: true,
                detail: "Read-only public chain tools from explorer.citrate.ai, including verified contract source. Addresses Hermes looks up are sent to the explorer.".into(),
            },
            McpServerView {
                name: NODE_SERVER_NAME.into(),
                label: "This node".into(),
                transport: "stdio".into(),
                enabled: settings.node,
                available: node_available,
                detail: if node_available {
                    "Your node's MCP server: chain, wallet and cluster reads answer at once. A transaction, a cluster or invite change waits for your approval in the app, and a transaction is signed only through the signature ceremony. Hermes gets its own connect token, renewed at each start.".into()
                } else {
                    "Turn on the node's MCP server first (Settings, API endpoints & keys).".into()
                },
            },
        ],
        config_written,
        restart_required: running,
    }
}

// ---------------------------------------------------------------------------
// App wiring.
// ---------------------------------------------------------------------------

/// The Hermes data dir (the same dir `hermes.rs` keeps the bearer file in).
pub fn hermes_dir<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Result<PathBuf, String> {
    use tauri::Manager;
    Ok(app
        .path()
        .app_local_data_dir()
        .map_err(|e| e.to_string())?
        .join("hermes"))
}

/// This executable + the managed memory daemon's socket, when both resolve.
fn mem_target<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Option<MemBridgeTarget> {
    use tauri::Manager;
    let exe = std::env::current_exe().ok()?;
    let socket = app
        .try_state::<crate::memory::MemoryState>()?
        .0
        .socket_path()
        .clone();
    Some(MemBridgeTarget { exe, socket })
}

/// HUP-S4.1: the node entry's target when the member turned it on and the node's MCP server is
/// on; otherwise every Hermes token is revoked and there is none.
fn node_target<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    dir: &Path,
    settings: &McpSettings,
) -> Option<NodeTarget> {
    use tauri::Manager;
    let node = app.try_state::<crate::node_mcp::NodeMcpState>()?;
    if !(settings.node && node.enabled()) {
        node.revoke_hermes_tokens();
        return None;
    }
    let exe = std::env::current_exe().ok()?;
    let current = current_node_token(dir);
    match node.hermes_token(current.as_deref()) {
        Ok(token) => Some(NodeTarget {
            exe,
            port: node.bound_port(),
            token,
        }),
        Err(e) => {
            eprintln!("hermes: no connect token for the node entry ({e}); it is left out");
            None
        }
    }
}

fn node_available<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> bool {
    use tauri::Manager;
    app.try_state::<crate::node_mcp::NodeMcpState>()
        .is_some_and(|n| n.enabled())
}

/// Bring the allowlist file in line with the saved settings. Called before every Hermes start.
pub fn sync_for_app<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> Result<Option<PathBuf>, String> {
    let dir = hermes_dir(app)?;
    let settings = load_settings(&dir);
    let node = node_target(app, &dir, &settings);
    sync_config_file(&dir, &settings, mem_target(app).as_ref(), node.as_ref())
        .map_err(|e| e.to_string())
}

fn view_for_app<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Result<McpView, String> {
    let dir = hermes_dir(app)?;
    let settings = load_settings(&dir);
    let target = mem_target(app);
    Ok(build_view(
        &settings,
        target.as_ref(),
        node_available(app),
        config_path(&dir).is_file(),
        crate::hermes::sidecar_running(),
    ))
}

/// The Hermes MCP settings view.
#[tauri::command]
pub async fn hermes_mcp_settings(app_h: tauri::AppHandle) -> Result<McpView, String> {
    crate::blocking::off_main(move || hermes_mcp_settings_sync(app_h.clone())).await
}

/// Blocking body of [`hermes_mcp_settings`]; reached only through [`crate::blocking::off_main`].
pub fn hermes_mcp_settings_sync(app: tauri::AppHandle) -> Result<McpView, String> {
    view_for_app(&app)
}

/// Save the member's MCP choices and rewrite the allowlist for the next Hermes start.
#[tauri::command]
pub async fn hermes_mcp_set(
    app_h: tauri::AppHandle,
    settings: McpSettings,
) -> Result<McpView, String> {
    crate::blocking::off_main(move || hermes_mcp_set_sync(app_h.clone(), settings)).await
}

/// Blocking body of [`hermes_mcp_set`]; reached only through [`crate::blocking::off_main`].
pub fn hermes_mcp_set_sync(
    app: tauri::AppHandle,
    settings: McpSettings,
) -> Result<McpView, String> {
    let dir = hermes_dir(&app)?;
    save_settings(&dir, &settings).map_err(|e| e.to_string())?;
    sync_for_app(&app)?;
    view_for_app(&app)
}

#[cfg(test)]
mod tests {
    include!("hermes_mcp_tests.rs");
}
