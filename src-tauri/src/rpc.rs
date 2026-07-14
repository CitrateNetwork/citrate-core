//! citrate-core — minimal in-process JSON-RPC client for 40204 (CORE-B1.4).
//!
//! This is the APP-side broadcast client, NOT part of the lean wallet crypto
//! crate. It POSTs the ceremony-signed raw tx to the live Citrate RPC
//! (`eth_sendRawTransaction`) and polls `eth_getTransactionReceipt` for
//! inclusion. It also fetches the two on-chain values a real tx needs — the
//! sender's pending nonce (`eth_getTransactionCount(addr,"pending")`) and the
//! current gas price (`eth_gasPrice`) — so nothing is hardcoded (Rule 1).
//!
//! ## Transport
//! Blocking HTTP via `ureq` (already the A3 OIDC client's transport: light,
//! rustls TLS, no tokio/hyper). The ceremony's broadcast command runs on a
//! spawned thread, not the Tauri async runtime, so blocking is correct.
//!
//! ## Secret discipline (@rule8)
//! This module never sees key material. It only handles the RLP-signed raw tx
//! bytes (public once broadcast) + public chain reads. No function here takes
//! or returns a key/seed/entropy.

// B1.4 seam: the broadcast client is consumed by the ceremony transaction path
// (wired) and exercised by tests via the injectable `RpcTransport`. Some helper
// surface is only reached by tests until the full connector round-trip lands.
#![allow(dead_code)]

use serde_json::{json, Value};

/// The live Citrate RPC endpoint (chain id 40204). Rule 1: real endpoint, real
/// reads/broadcast — no mocked chain data.
pub const CITRATE_RPC_URL: &str = "https://rpc.citrate.ai";

/// The Citrate chain id used for EIP-155 replay protection.
pub const CITRATE_CHAIN_ID: u64 = 40204;

/// Errors from the JSON-RPC client. Coarse + secret-free (this client never
/// touches key material, so no variant can carry it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RpcError {
    /// The HTTP transport failed (connection, TLS, non-2xx).
    Transport(String),
    /// The response body was not valid JSON-RPC.
    BadResponse(String),
    /// The node returned a JSON-RPC `error` object (e.g. "insufficient funds",
    /// "nonce too low"). Carries the node's message verbatim (public info).
    Node(String),
    /// A field we required (result, hex number) was missing or malformed.
    MissingField(String),
    /// The receipt did not appear within the poll budget.
    ReceiptTimeout,
}

impl std::fmt::Display for RpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RpcError::Transport(m) => write!(f, "rpc transport error: {m}"),
            RpcError::BadResponse(m) => write!(f, "rpc bad response: {m}"),
            RpcError::Node(m) => write!(f, "rpc node error: {m}"),
            RpcError::MissingField(m) => write!(f, "rpc missing/invalid field: {m}"),
            RpcError::ReceiptTimeout => write!(f, "rpc receipt not found within poll budget"),
        }
    }
}

impl std::error::Error for RpcError {}

/// A transaction receipt's confirmation facts (the Rule-11 proof surface):
/// the tx hash the node accepted + the block it was included in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Receipt {
    /// The transaction hash (`0x…`).
    pub tx_hash: String,
    /// The block number the tx was included in (decoded from the hex quantity).
    pub block_number: u64,
    /// The receipt `status` (1 = success, 0 = reverted), if the node returns it.
    pub status: Option<u64>,
}

/// The JSON-RPC transport seam. Production uses [`HttpTransport`] (real POSTs to
/// 40204); tests inject a mock so the request shape + poll logic are CI-safe
/// (Rule 1: the mock is a TEST transport, never wired as the default).
pub trait RpcTransport {
    /// Send one JSON-RPC request body and return the parsed JSON response.
    fn call(&self, body: Value) -> Result<Value, RpcError>;
}

/// The real HTTP transport: blocking `ureq` POST to the Citrate RPC.
pub struct HttpTransport {
    url: String,
}

impl HttpTransport {
    /// Build a transport bound to the given RPC URL (defaults to 40204's).
    pub fn new(url: impl Into<String>) -> Self {
        HttpTransport { url: url.into() }
    }

    /// The default transport, pointed at the live Citrate RPC.
    pub fn citrate() -> Self {
        HttpTransport::new(CITRATE_RPC_URL)
    }
}

impl RpcTransport for HttpTransport {
    fn call(&self, body: Value) -> Result<Value, RpcError> {
        let resp = ureq::post(&self.url)
            .send_json(&body)
            .map_err(|e| RpcError::Transport(e.to_string()))?;
        let mut resp = resp;
        resp.body_mut()
            .read_json::<Value>()
            .map_err(|e| RpcError::BadResponse(e.to_string()))
    }
}

/// A JSON-RPC client over an injectable transport. Holds a monotonic request id
/// so each call is uniquely addressable in the response.
pub struct RpcClient<T: RpcTransport> {
    /// The transport. `pub(crate)` so the ceremony/rpc tests can inspect a mock
    /// transport's recorded requests; production code uses only the methods.
    pub(crate) transport: T,
    next_id: std::cell::Cell<u64>,
}

impl RpcClient<HttpTransport> {
    /// The production client: real HTTP to the live 40204 RPC.
    pub fn citrate() -> Self {
        RpcClient::with_transport(HttpTransport::citrate())
    }
}

impl<T: RpcTransport> RpcClient<T> {
    /// Build a client over an explicit transport (tests inject a mock).
    pub fn with_transport(transport: T) -> Self {
        RpcClient {
            transport,
            next_id: std::cell::Cell::new(1),
        }
    }

    /// Build a well-formed JSON-RPC 2.0 request body for `method`/`params`.
    /// Public so tests can assert the exact request shape (raw hex, method).
    pub fn build_request(&self, method: &str, params: Value) -> Value {
        let id = self.next_id.get();
        self.next_id.set(id + 1);
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        })
    }

    /// Send `method`/`params` and return the `result` value, surfacing a node
    /// `error` object as [`RpcError::Node`].
    fn request(&self, method: &str, params: Value) -> Result<Value, RpcError> {
        let body = self.build_request(method, params);
        let resp = self.transport.call(body)?;
        if let Some(err) = resp.get("error") {
            let msg = err
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("unknown node error")
                .to_string();
            return Err(RpcError::Node(msg));
        }
        resp.get("result")
            .cloned()
            .ok_or_else(|| RpcError::MissingField("result".into()))
    }

    /// `eth_getTransactionCount(address, "pending")` → the next nonce to use.
    /// Real value from the node (Rule 1): the PENDING count so back-to-back
    /// sends do not collide.
    pub fn pending_nonce(&self, address: &str) -> Result<u64, RpcError> {
        let result = self.request("eth_getTransactionCount", json!([address, "pending"]))?;
        parse_hex_quantity(&result, "eth_getTransactionCount")
    }

    /// `eth_gasPrice` → the current network gas price in wei (real value).
    pub fn gas_price(&self) -> Result<u64, RpcError> {
        let result = self.request("eth_gasPrice", json!([]))?;
        parse_hex_quantity(&result, "eth_gasPrice")
    }

    /// `eth_estimateGas({from,to,value,data})` → the gas the node estimates a
    /// call needs (real value from the node; CORE-C1.2 agent bridge). A
    /// node-agent contract call (`claimRewards`, `bidOnJob`, …) carries calldata
    /// but no gas — B1.4's `finalize` refuses to GUESS execution gas, so the
    /// agent bridge asks the node for a real estimate rather than fabricating a
    /// number (Rule 1). `params` is the pre-built call object (caller-shaped so we
    /// stay transport-only). Returns the estimated gas limit.
    pub fn estimate_gas(&self, call: Value) -> Result<u64, RpcError> {
        let result = self.request("eth_estimateGas", json!([call]))?;
        parse_hex_quantity(&result, "eth_estimateGas")
    }

    /// `eth_call({to,data}, "latest")` → the raw returned bytes of a read-only
    /// contract call (CORE-C2 earnings: `ContributionAccounting.claimable`). The
    /// node returns a `0x`-prefixed hex string of the ABI-encoded return; we hand
    /// back the decoded bytes for the caller's ABI decoder. Real value from the
    /// live 40204 RPC (Rule 1 — no fabricated read). `call` is the pre-built call
    /// object (`{to, data}`); we pin the block tag to `"latest"`.
    pub fn eth_call(&self, call: Value) -> Result<Vec<u8>, RpcError> {
        let result = self.request("eth_call", json!([call, "latest"]))?;
        let s = result
            .as_str()
            .ok_or_else(|| RpcError::MissingField("eth_call: result not a string".into()))?;
        let stripped = s
            .strip_prefix("0x")
            .ok_or_else(|| RpcError::MissingField("eth_call: missing 0x prefix".into()))?;
        hex::decode(stripped)
            .map_err(|_| RpcError::MissingField(format!("eth_call: result not hex ({s})")))
    }

    /// `eth_blockNumber` → the node's current head height (real value from the
    /// spawned node's local RPC; CORE-C1.1 NodeDomain status). The node returns
    /// a `0x`-prefixed hex quantity.
    pub fn block_number(&self) -> Result<u64, RpcError> {
        let result = self.request("eth_blockNumber", json!([]))?;
        parse_hex_quantity(&result, "eth_blockNumber")
    }

    /// `net_peerCount` → the node's live peer count (real value; CORE-C1.1
    /// NodeDomain status). Returned as a `0x`-prefixed hex quantity.
    pub fn peer_count(&self) -> Result<u64, RpcError> {
        let result = self.request("net_peerCount", json!([]))?;
        parse_hex_quantity(&result, "net_peerCount")
    }

    /// `eth_sendRawTransaction(rawHex)` → the tx hash the node accepted. `raw`
    /// is the RLP-signed tx bytes; we submit the canonical `0x…` hex form.
    pub fn send_raw_transaction(&self, raw: &[u8]) -> Result<String, RpcError> {
        let raw_hex = format!("0x{}", hex::encode(raw));
        let result = self.request("eth_sendRawTransaction", json!([raw_hex]))?;
        result
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| RpcError::MissingField("sendRawTransaction hash".into()))
    }

    /// `eth_getTransactionReceipt(txHash)` → `Some(Receipt)` once mined, `None`
    /// while still pending (the node returns JSON `null`).
    pub fn transaction_receipt(&self, tx_hash: &str) -> Result<Option<Receipt>, RpcError> {
        let result = self.request("eth_getTransactionReceipt", json!([tx_hash]))?;
        if result.is_null() {
            return Ok(None);
        }
        let block_number = result
            .get("blockNumber")
            .ok_or_else(|| RpcError::MissingField("receipt.blockNumber".into()))
            .and_then(|v| parse_hex_quantity(v, "receipt.blockNumber"))?;
        let status = result
            .get("status")
            .and_then(|v| parse_hex_quantity(v, "receipt.status").ok());
        Ok(Some(Receipt {
            tx_hash: tx_hash.to_string(),
            block_number,
            status,
        }))
    }

    /// Poll `eth_getTransactionReceipt` until the tx is included or the budget
    /// (`attempts` × `interval`) is exhausted. Returns the confirmed receipt
    /// (the block-inclusion proof) or [`RpcError::ReceiptTimeout`].
    pub fn poll_receipt(
        &self,
        tx_hash: &str,
        attempts: u32,
        interval: std::time::Duration,
    ) -> Result<Receipt, RpcError> {
        for i in 0..attempts {
            if let Some(receipt) = self.transaction_receipt(tx_hash)? {
                return Ok(receipt);
            }
            if i + 1 < attempts {
                std::thread::sleep(interval);
            }
        }
        Err(RpcError::ReceiptTimeout)
    }
}

/// Decode a JSON string hex quantity (`"0x1a"`) to a `u64`. Ethereum quantities
/// are minimal-length big-endian hex with a `0x` prefix.
fn parse_hex_quantity(v: &Value, field: &str) -> Result<u64, RpcError> {
    let s = v
        .as_str()
        .ok_or_else(|| RpcError::MissingField(format!("{field}: not a string")))?;
    let stripped = s
        .strip_prefix("0x")
        .ok_or_else(|| RpcError::MissingField(format!("{field}: missing 0x prefix")))?;
    u64::from_str_radix(stripped, 16)
        .map_err(|_| RpcError::MissingField(format!("{field}: not hex ({s})")))
}

#[cfg(test)]
mod tests {
    include!("rpc_tests.rs");
}
