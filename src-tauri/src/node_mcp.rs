//! HUP-S4.2 + HUP-S8.5 — the citrate-node MCP server (US-4.2 "My node is an MCP server",
//! US-8.3 "Cluster tools for Hermes").
//!
//! A member can point Claude Code, Cursor, another agent, or Hermes itself at this node. The
//! server is OFF by default and listens on loopback only. Every request needs a connect token the
//! member creates in Settings (stored hashed, revocable). Read tools answer at once; every write
//! becomes a request the member approves in the app, and a write that needs a signature goes
//! through the SignatureCeremony, the app's one signing path. No MCP client ever holds or reaches
//! a key.
//!
//! Layout (flat files so the main-thread tripwire scans them all):
//! - `node_mcp_protocol.rs`: JSON-RPC / MCP dispatch, independent of transport and data source.
//! - `node_mcp_tools.rs`: the tool + resource catalog, annotations, validators, precompile table.
//! - `node_mcp_approvals.rs`: the approval inbox for write requests.
//! - `node_mcp_token.rs`: connect tokens (hashed store).
//! - `node_mcp_http.rs`: loopback streamable HTTP + the stdio shim (`citrate-core --mcp-stdio`).
//! - `node_mcp_live.rs`: the app-backed data source.
//! - this file: managed state + the Settings commands.
//!
//! Docs: `docs/NODE_MCP_SERVER.md`.

use crate::node_mcp_approvals::{ApprovalInbox, McpAction, McpRequest, RequestKind};
use crate::node_mcp_http::{self as http, RunningServer, ServerShared};
use crate::node_mcp_protocol::{now_ms, CallLogEntry, McpCore, NodeBackend};
use crate::node_mcp_token::{TokenIssued, TokenStore, TokenView};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// HUP-S4.1: the label of the built-in Hermes entry's token (shown on its approval requests). It
/// is reserved ([`RESERVED_TOKEN_LABELS`]): members cannot issue a token with it.
pub const HERMES_TOKEN_LABEL: &str = crate::hermes_mcp::HERMES_NODE_TOKEN_LABEL;

/// The persisted switch (`<app data>/node-mcp/config.json`). Off unless the member turns it on.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct NodeMcpConfig {
    #[serde(default)]
    pub enabled: bool,
    /// Loopback port; `None` = [`http::DEFAULT_PORT`].
    #[serde(default)]
    pub port: Option<u16>,
}

/// Managed state.
pub struct NodeMcpState {
    shared: Arc<ServerShared>,
    server: Mutex<Option<RunningServer>>,
    dir: Option<PathBuf>,
    last_error: Mutex<Option<String>>,
}

/// What Settings shows.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeMcpStatus {
    pub running: bool,
    pub enabled: bool,
    pub port: u16,
    pub endpoint: String,
    pub last_error: Option<String>,
    pub tokens: Vec<TokenView>,
    pub pending_requests: usize,
    pub recent_calls: Vec<CallLogEntry>,
    /// The app binary, for the stdio shim command line (`<exe> --mcp-stdio`).
    pub stdio_command: Option<String>,
}

impl NodeMcpState {
    /// Build the state around any backend (tests use fixtures; the app uses `LiveBackend`).
    pub fn new(backend: Arc<dyn NodeBackend>, tokens: TokenStore, dir: Option<PathBuf>) -> Self {
        let core = Arc::new(McpCore::new(backend, Arc::new(ApprovalInbox::new())));
        NodeMcpState {
            shared: Arc::new(ServerShared::new(core, Arc::new(tokens))),
            server: Mutex::new(None),
            dir,
            last_error: Mutex::new(None),
        }
    }

    #[cfg(test)]
    pub fn shared(&self) -> &Arc<ServerShared> {
        &self.shared
    }

    fn config_path(&self) -> Option<PathBuf> {
        self.dir.as_ref().map(|d| d.join("config.json"))
    }

    pub fn config(&self) -> NodeMcpConfig {
        self.config_path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    fn save_config(&self, cfg: &NodeMcpConfig) -> Result<(), String> {
        match self.config_path() {
            Some(p) => {
                let body = serde_json::to_vec_pretty(cfg).map_err(|e| e.to_string())?;
                crate::node_mcp_token::write_private(&p, &body)
            }
            None => Ok(()),
        }
    }

    /// The port: `CITRATE_NODE_MCP_PORT`, else the config, else the default.
    pub fn port(&self) -> u16 {
        std::env::var("CITRATE_NODE_MCP_PORT")
            .ok()
            .and_then(|p| p.parse::<u16>().ok())
            .or(self.config().port)
            .unwrap_or(http::DEFAULT_PORT)
    }

    pub fn is_running(&self) -> bool {
        self.server
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_some()
    }

    /// Start listening (idempotent). The bound port is returned.
    pub fn start_on(&self, port: u16) -> Result<u16, String> {
        let mut g = self.server.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(s) = g.as_ref() {
            return Ok(s.addr.port());
        }
        match http::start(self.shared.clone(), port) {
            Ok(s) => {
                let p = s.addr.port();
                *g = Some(s);
                *self.last_error.lock().unwrap_or_else(|e| e.into_inner()) = None;
                Ok(p)
            }
            Err(e) => {
                *self.last_error.lock().unwrap_or_else(|e| e.into_inner()) = Some(e.clone());
                Err(e)
            }
        }
    }

    pub fn stop(&self) {
        let s = self.server.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(s) = s {
            s.stop();
        }
    }

    /// Turn the server on or off and remember the choice.
    pub fn set_enabled(&self, enabled: bool) -> Result<(), String> {
        let mut cfg = self.config();
        cfg.enabled = enabled;
        self.save_config(&cfg)?;
        if enabled {
            self.start_on(self.port()).map(|_| ())
        } else {
            self.stop();
            Ok(())
        }
    }

    pub fn status(&self) -> NodeMcpStatus {
        let port = self
            .server
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .map(|s| s.addr.port())
            .unwrap_or_else(|| self.port());
        let now = self.shared.core.now();
        let (requests, close) = self.shared.core.inbox().list(now);
        for c in close {
            self.shared.core.backend().close_ceremony(&c.0);
        }
        NodeMcpStatus {
            running: self.is_running(),
            enabled: self.config().enabled,
            port,
            endpoint: http::endpoint_url(port),
            last_error: self
                .last_error
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone(),
            tokens: self.shared.tokens.list(),
            pending_requests: requests
                .iter()
                .filter(|r| r.state == crate::node_mcp_approvals::RequestState::Pending)
                .count(),
            recent_calls: self
                .shared
                .core
                .recent_calls()
                .into_iter()
                .take(25)
                .collect(),
            stdio_command: std::env::current_exe()
                .ok()
                .map(|p| p.to_string_lossy().to_string()),
        }
    }

    /// Whether the member turned the server on (it listens while on).
    pub fn enabled(&self) -> bool {
        self.config().enabled
    }

    /// The port the server listens on now (or would).
    pub fn bound_port(&self) -> u16 {
        self.server
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .map(|s| s.addr.port())
            .unwrap_or_else(|| self.port())
    }

    /// HUP-S4.1: the connect token for the built-in Hermes entry. `current` (the token in the
    /// allowlist file Hermes was last given) is kept while it is still a live in-memory token;
    /// otherwise every earlier Hermes token is revoked (its sessions and pending requests close)
    /// and a new one is minted. Only its hash is held, in memory.
    pub fn hermes_token(&self, current: Option<&str>) -> Result<String, String> {
        if let Some(t) = current.filter(|t| self.shared.tokens.is_live_ephemeral(t)) {
            return Ok(t.to_string());
        }
        self.revoke_hermes_tokens();
        self.shared
            .tokens
            .issue_ephemeral(
                HERMES_TOKEN_LABEL,
                !crate::hermes_mcp::HERMES_NODE_WRITE_TOOLS,
                now_ms(),
            )
            .map(|t| t.connect_token)
    }

    /// HUP-S4.1: revoke every in-memory (Hermes) token.
    pub fn revoke_hermes_tokens(&self) {
        for id in self.shared.tokens.ephemeral_ids() {
            let _ = self.revoke_token(&id);
        }
    }

    /// Issue a token from Settings (a full token: read and write tools). Labels core reserves for
    /// its own tokens are refused, so no client can be labelled as Hermes on an approval card and
    /// no member token is ever revoked by Hermes's re-issue.
    pub fn create_token(&self, label: &str) -> Result<TokenIssued, String> {
        if is_reserved_label(label) {
            return Err(format!(
                "\"{}\" is reserved for the token Citrate Core issues for Hermes. Pick another label.",
                label.trim()
            ));
        }
        self.shared.tokens.issue(label, now_ms())
    }

    /// Revoke a token: delete it, end its sessions, and close its pending requests.
    pub fn revoke_token(&self, id: &str) -> Result<bool, String> {
        let removed = self.shared.tokens.revoke(id)?;
        self.shared.drop_sessions_for(id);
        for c in self.shared.core.inbox().revoke_token(id, now_ms()) {
            self.shared.core.backend().close_ceremony(&c.0);
        }
        Ok(removed)
    }

    pub fn requests(&self) -> Vec<McpRequest> {
        let (v, close) = self.shared.core.inbox().list(now_ms());
        for c in close {
            self.shared.core.backend().close_ceremony(&c.0);
        }
        v
    }
}

/// Token labels core keeps for the tokens it issues itself (compared trimmed, ignoring case).
pub const RESERVED_TOKEN_LABELS: &[&str] = &[crate::hermes_mcp::HERMES_NODE_TOKEN_LABEL];

/// Whether `label` is one of [`RESERVED_TOKEN_LABELS`].
pub fn is_reserved_label(label: &str) -> bool {
    let t = label.trim();
    RESERVED_TOKEN_LABELS
        .iter()
        .any(|r| r.eq_ignore_ascii_case(t))
}

/// Build the managed state for the app: tokens + config under `<app data>/node-mcp/`, the live
/// backend, and (only if the member turned it on before) the listener.
pub fn build_node_mcp_state(app: &tauri::AppHandle) -> NodeMcpState {
    use tauri::Manager;
    let dir = app.path().app_data_dir().ok().map(|d| d.join("node-mcp"));
    let tokens = match &dir {
        Some(d) => TokenStore::load(d.join("tokens.json")),
        None => TokenStore::in_memory(),
    };
    let backend: Arc<dyn NodeBackend> =
        Arc::new(crate::node_mcp_live::LiveBackend::new(app.clone()));
    let state = NodeMcpState::new(backend, tokens, dir);
    if state.config().enabled {
        // A failed bind is recorded in last_error and shown in Settings; the app still starts.
        let _ = state.start_on(state.port());
    }
    state
}

/// The result of an approved action, for the client's `request_status`.
async fn run_action(app: &tauri::AppHandle, action: McpAction) -> Result<Value, String> {
    match action {
        McpAction::ClusterJoin { group } => {
            crate::cluster::cluster_join(app.clone(), group.clone()).await?;
            Ok(json!({"joined": group}))
        }
        McpAction::ClusterShare { group, cid } => {
            crate::cluster::cluster_share_file(app.clone(), group.clone(), cid.clone()).await?;
            Ok(json!({"shared": cid, "group": group}))
        }
        McpAction::InviteCreate { group, for_handle } => {
            let minted =
                crate::invites::group_invite_create(app.clone(), group.clone(), for_handle).await?;
            Ok(json!({
                "group": group,
                "inviteId": crate::node_mcp_live::invite_id_for(&minted.token),
                "link": minted.link,
            }))
        }
        McpAction::InviteRevoke { group, invite_id } => {
            let app2 = app.clone();
            let group2 = group.clone();
            let id2 = invite_id.clone();
            let token = crate::blocking::off_main(move || {
                crate::invites::load(&app2)
                    .into_iter()
                    .find(|i| {
                        i.group == group2 && crate::node_mcp_live::invite_id_for(&i.token) == id2
                    })
                    .map(|i| i.token)
                    .ok_or_else(|| format!("no outstanding invite {id2} for that group"))
            })
            .await?;
            crate::invites::group_invite_revoke(app.clone(), group.clone(), token).await?;
            Ok(json!({"revoked": invite_id, "group": group}))
        }
        McpAction::PinAdd { cid } => {
            let app2 = app.clone();
            let cid2 = cid.clone();
            crate::blocking::off_main(move || crate::storage::storage_pin_local_sync(app2, &cid2))
                .await?;
            Ok(json!({"pinned": cid, "bond": "none (local pin only)"}))
        }
        McpAction::AnchorPropose => {
            let app2 = app.clone();
            let report =
                crate::blocking::off_main(move || crate::chain_agent::anchor_now(&app2)).await?;
            Ok(json!({
                "approvalCardsRaised": report.raised.len(),
                "skipped": report.skipped.iter().map(|(day, why)| json!({"day": day, "why": why})).collect::<Vec<_>>(),
                "next": "Each raised card waits for the member's approval in Citrate Core before anything is signed.",
            }))
        }
        // HUP-S1.1: the member approved it; the app's Hermes client carries it to the session.
        a @ (McpAction::HermesSessionSend { .. } | McpAction::HermesSessionStop { .. }) => {
            let app2 = app.clone();
            crate::blocking::off_main(move || {
                let mgr = crate::hermes::manager_for(&app2)?;
                crate::node_mcp_hermes::run_action(mgr, &a)
            })
            .await
        }
    }
}

fn state_of(app: &tauri::AppHandle) -> Result<tauri::State<'_, NodeMcpState>, String> {
    tauri::Manager::try_state::<NodeMcpState>(app)
        .ok_or_else(|| "internal: managed state unavailable".to_string())
}

// ---------------------------------------------------------------------------
// Tauri commands (all async; blocking bodies run off the main thread).
// ---------------------------------------------------------------------------

/// `node_mcp_status` — server state, tokens (no secrets), pending count, recent calls.
#[tauri::command]
pub async fn node_mcp_status(app_h: tauri::AppHandle) -> Result<NodeMcpStatus, String> {
    crate::blocking::off_main(move || Ok(state_of(&app_h)?.status())).await
}

/// `node_mcp_set_enabled` — turn the loopback server on or off (remembered across launches).
#[tauri::command]
pub async fn node_mcp_set_enabled(
    app_h: tauri::AppHandle,
    enabled: bool,
) -> Result<NodeMcpStatus, String> {
    crate::blocking::off_main(move || {
        let st = state_of(&app_h)?;
        st.set_enabled(enabled)?;
        Ok(st.status())
    })
    .await
}

/// `node_mcp_token_create` — issue a connect token. The plaintext is returned this once and is
/// never stored; only its SHA-256 persists.
#[tauri::command]
pub async fn node_mcp_token_create(
    app_h: tauri::AppHandle,
    label: String,
) -> Result<TokenIssued, String> {
    crate::blocking::off_main(move || state_of(&app_h)?.create_token(&label)).await
}

/// `node_mcp_token_revoke` — delete a token, end its sessions, close its pending requests.
#[tauri::command]
pub async fn node_mcp_token_revoke(app_h: tauri::AppHandle, id: String) -> Result<bool, String> {
    crate::blocking::off_main(move || state_of(&app_h)?.revoke_token(&id)).await
}

/// `node_mcp_requests` — every kept write request, newest first.
#[tauri::command]
pub async fn node_mcp_requests(app_h: tauri::AppHandle) -> Result<Vec<McpRequest>, String> {
    crate::blocking::off_main(move || Ok(state_of(&app_h)?.requests())).await
}

/// `node_mcp_decide` — the member's decision on one request. Approving a signature request runs
/// its ceremony through `sign_and_broadcast` (the one signing path); approving an action runs it.
/// `raw_ack` must be set for a ceremony whose calldata could not be decoded.
#[tauri::command]
pub async fn node_mcp_decide(
    app_h: tauri::AppHandle,
    id: String,
    approve: bool,
    raw_ack: bool,
) -> Result<McpRequest, String> {
    // Raw-ack gate first, so a missing acknowledgement leaves the request pending.
    if approve {
        let app2 = app_h.clone();
        let id2 = id.clone();
        crate::blocking::off_main(move || {
            let st = state_of(&app2)?;
            let req = st
                .requests()
                .into_iter()
                .find(|r| r.id == id2)
                .ok_or_else(|| format!("no request {id2}"))?;
            if let RequestKind::Signature { ceremony_id, .. } = &req.kind {
                let ceremony =
                    tauri::Manager::try_state::<crate::ceremony::CeremonyState>(&app2)
                        .ok_or("the signature ceremony is not available")?;
                match ceremony.0.status(ceremony_id) {
                    Some(v) if v.requires_raw_ack && !raw_ack => {
                        return Err("This transaction's data could not be decoded. Tick the acknowledgement that you have checked the raw data before approving.".to_string())
                    }
                    Some(_) => {}
                    None => return Err("this signature request is no longer open".to_string()),
                }
            }
            Ok(())
        })
        .await?;
    }

    let app2 = app_h.clone();
    let id2 = id.clone();
    let req = crate::blocking::off_main(move || {
        let st = state_of(&app2)?;
        let (req, close) = st
            .shared
            .core
            .inbox()
            .begin_decision(&id2, approve, now_ms())?;
        for c in close {
            st.shared.core.backend().close_ceremony(&c.0);
        }
        if !approve {
            if let RequestKind::Signature { ceremony_id, .. } = &req.kind {
                st.shared.core.backend().close_ceremony(ceremony_id);
            }
        }
        Ok(req)
    })
    .await?;
    if !approve {
        return Ok(req);
    }

    let outcome: Result<Value, String> = match req.kind.clone() {
        RequestKind::Signature { ceremony_id, .. } => {
            let app3 = app_h.clone();
            crate::blocking::off_main(move || {
                let c = tauri::Manager::try_state::<crate::ceremony::CeremonyState>(&app3)
                    .ok_or_else(|| "the signature ceremony is not available".to_string())?;
                let v = tauri::Manager::try_state::<crate::custody::CustodyState>(&app3)
                    .ok_or_else(|| "the wallet is not available".to_string())?;
                let r =
                    crate::ceremony::sign_and_broadcast_sync(c, v, ceremony_id.clone(), raw_ack);
                if r.is_err() {
                    // Never leave a ceremony open behind a failed request.
                    if let Some(c) =
                        tauri::Manager::try_state::<crate::ceremony::CeremonyState>(&app3)
                    {
                        let _ = c.0.reject(&ceremony_id);
                    }
                }
                r.and_then(|b| serde_json::to_value(b).map_err(|e| e.to_string()))
            })
            .await
        }
        RequestKind::Action { action } => run_action(&app_h, action).await,
    };
    let app4 = app_h.clone();
    crate::blocking::off_main(move || {
        let st = state_of(&app4)?;
        st.shared
            .core
            .inbox()
            .finish(&id, outcome, now_ms())
            .ok_or_else(|| format!("request {id} changed while it ran"))
    })
    .await
}

#[cfg(test)]
mod tests {
    include!("node_mcp_tests.rs");
}
