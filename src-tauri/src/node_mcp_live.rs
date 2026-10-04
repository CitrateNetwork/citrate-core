//! HUP-S4.2 — the app-backed [`NodeBackend`] for the citrate-node MCP server.
//!
//! Every answer comes from the same source the app's own surfaces use: the supervised node
//! (`NodeState`), the chain RPC (this node when it is running and caught up, else the public
//! 40204 RPC, and the answer names which), the custody vault's PUBLIC wallet address, the memory
//! daemon, the comms + cluster daemons, and the local invite records. Nothing is fabricated; a
//! source that is down yields an error the client sees verbatim.
//!
//! Signatures: `propose_transaction` builds the transaction JSON and opens a SignatureCeremony.
//! It signs nothing and holds no key. The member approves (or not) in the app.

use crate::ceremony::{IntentKind, SignatureIntent};
use crate::node_mcp_protocol::{NodeBackend, ProposedSignature, RpcRead};
use crate::rpc::{HttpTransport, RpcClient, RpcError, CITRATE_CHAIN_ID};
use serde_json::{json, Value};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// The node's local JSON-RPC (same address `node.rs` polls).
const LOCAL_RPC_URL: &str = "http://127.0.0.1:8545";
/// How long the "use the local node?" decision is reused.
const SOURCE_TTL: Duration = Duration::from_secs(10);

pub struct LiveBackend {
    app: tauri::AppHandle,
    /// Cached (use_local, reason, decided_at).
    source: Mutex<Option<(bool, Instant)>>,
}

impl LiveBackend {
    pub fn new(app: tauri::AppHandle) -> Self {
        LiveBackend {
            app,
            source: Mutex::new(None),
        }
    }

    /// Use this node's RPC only when it is running and caught up (a syncing node would answer
    /// with stale state); otherwise the public RPC. Decided at most every 10 s.
    fn use_local(&self) -> bool {
        {
            let g = self.source.lock().unwrap_or_else(|e| e.into_inner());
            if let Some((v, at)) = *g {
                if at.elapsed() < SOURCE_TTL {
                    return v;
                }
            }
        }
        let v = tauri::Manager::try_state::<crate::node::NodeState>(&self.app)
            .map(|s| {
                let st = s.0.status();
                st.state == "running" && st.sync_pct >= 100.0
            })
            .unwrap_or(false);
        *self.source.lock().unwrap_or_else(|e| e.into_inner()) = Some((v, Instant::now()));
        v
    }

    fn call(url: &str, method: &str, params: Value) -> Result<Value, RpcError> {
        use crate::rpc::RpcTransport;
        let client = RpcClient::with_transport(HttpTransport::new(url));
        let body = client.build_request(method, params);
        let resp = client.transport().call(body)?;
        if let Some(err) = resp.get("error") {
            let msg = err
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("node error")
                .to_string();
            return Err(RpcError::Node(msg));
        }
        resp.get("result")
            .cloned()
            .ok_or_else(|| RpcError::MissingField("result".into()))
    }
}

impl NodeBackend for LiveBackend {
    fn node_status(&self) -> Result<Value, String> {
        let st = tauri::Manager::try_state::<crate::node::NodeState>(&self.app)
            .ok_or("the node manager is not available")?;
        serde_json::to_value(st.0.status()).map_err(|e| e.to_string())
    }

    fn rpc_read(&self, method: &str, params: Value) -> Result<RpcRead, String> {
        if self.use_local() {
            match Self::call(LOCAL_RPC_URL, method, params.clone()) {
                Ok(v) => {
                    return Ok(RpcRead {
                        value: v,
                        source: "local-node".to_string(),
                    })
                }
                Err(RpcError::Transport(_)) => {} // node went away: fall through to public
                Err(e) => return Err(e.to_string()),
            }
        }
        Self::call(crate::rpc::CITRATE_RPC_URL, method, params)
            .map(|v| RpcRead {
                value: v,
                source: "public-rpc (this node is not running or still syncing)".to_string(),
            })
            .map_err(|e| e.to_string())
    }

    fn wallet_address(&self) -> Result<String, String> {
        let custody = tauri::Manager::try_state::<crate::custody::CustodyState>(&self.app)
            .ok_or("the wallet is not available")?;
        crate::wallet::address_auto_unlocked(&custody.0)
            .map(|w| w.address)
            .map_err(|e| format!("wallet unavailable: {e}"))
    }

    fn memory_search(&self, tenant: &str, query: &str, limit: usize) -> Result<Value, String> {
        let st = tauri::Manager::try_state::<crate::memory::MemoryState>(&self.app)
            .ok_or("memory is not available")?;
        let r =
            st.0.search(tenant, query, limit)
                .map_err(|e| format!("memory search failed: {e}"))?;
        serde_json::to_value(r).map_err(|e| e.to_string())
    }

    fn groups(&self) -> Result<Value, String> {
        let gs = tauri::async_runtime::block_on(crate::comms::groups_list(self.app.clone()))?;
        Ok(json!({
            "groups": gs.into_iter().map(|(id, name)| json!({"id": id, "name": name})).collect::<Vec<_>>()
        }))
    }

    fn cluster_status(&self, group: &str) -> Result<Value, String> {
        let s = tauri::async_runtime::block_on(crate::cluster::cluster_status(
            self.app.clone(),
            group.to_string(),
        ))?;
        serde_json::to_value(s).map_err(|e| e.to_string())
    }

    fn cluster_peers(&self, group: &str) -> Result<Value, String> {
        let p = tauri::async_runtime::block_on(crate::cluster::cluster_peers(
            self.app.clone(),
            group.to_string(),
        ))?;
        Ok(json!({ "peers": p }))
    }

    fn cluster_devices(&self, group: &str) -> Result<Value, String> {
        let m = tauri::async_runtime::block_on(crate::cluster::cluster_devices(
            self.app.clone(),
            group.to_string(),
        ))?;
        Ok(json!({ "members": m }))
    }

    fn invites(&self, group: &str) -> Result<Value, String> {
        let all = crate::invites::load(&self.app);
        Ok(json!({
            "invites": all
                .iter()
                .filter(|i| i.group == group)
                .map(|i| json!({
                    "inviteId": invite_id_for(&i.token),
                    "forHandle": i.for_handle,
                    "createdAt": i.created_at,
                }))
                .collect::<Vec<_>>()
        }))
    }

    fn propose_transaction(
        &self,
        origin: &str,
        to: &str,
        value_wei: u128,
        data: &str,
    ) -> Result<ProposedSignature, String> {
        let from = self.wallet_address()?;
        let call = json!({
            "from": from,
            "to": to,
            "value": format!("0x{value_wei:x}"),
            "data": data,
        });
        // The ceremony refuses to guess execution gas, so ask the chain. A failing estimate
        // usually means the call would revert: say so instead of opening a doomed ceremony.
        let est = self
            .rpc_read("eth_estimateGas", json!([call]))
            .map_err(|e| format!("the chain could not estimate gas for this transaction (it would likely fail): {e}"))?;
        let gas = est
            .value
            .as_str()
            .ok_or("the chain returned a malformed gas estimate")?
            .to_string();
        let raw = json!({
            "from": from,
            "to": to,
            "value": format!("0x{value_wei:x}"),
            "data": data,
            "gas": gas,
            "chainId": format!("0x{CITRATE_CHAIN_ID:x}"),
        })
        .to_string();
        let ceremony = tauri::Manager::try_state::<crate::ceremony::CeremonyState>(&self.app)
            .ok_or("the signature ceremony is not available")?;
        let view = ceremony.0.request(SignatureIntent {
            origin: origin.to_string(),
            kind: IntentKind::Transaction,
            chain_id: CITRATE_CHAIN_ID,
            raw,
        });
        Ok(ProposedSignature {
            ceremony_id: view.id.clone(),
            ceremony: serde_json::to_value(&view).map_err(|e| e.to_string())?,
        })
    }

    fn close_ceremony(&self, ceremony_id: &str) {
        if let Some(c) = tauri::Manager::try_state::<crate::ceremony::CeremonyState>(&self.app) {
            let _ = c.0.reject(ceremony_id);
        }
    }
}

/// The public id of an invite: the first 16 hex chars of `BLAKE3(token)` (the same hash the relay
/// already holds). The token itself never leaves the app over MCP.
pub fn invite_id_for(token: &str) -> String {
    crate::invites::blake3_token_hash(token)
        .chars()
        .take(16)
        .collect()
}
