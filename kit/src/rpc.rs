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
    /// The node's rate limiter answered HTTP 429 before the request reached a handler.
    RateLimited,
}

impl std::fmt::Display for RpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RpcError::Transport(m) => write!(f, "rpc transport error: {m}"),
            RpcError::BadResponse(m) => write!(f, "rpc bad response: {m}"),
            RpcError::Node(m) => write!(f, "rpc node error: {m}"),
            RpcError::MissingField(m) => write!(f, "rpc missing/invalid field: {m}"),
            RpcError::ReceiptTimeout => write!(f, "rpc receipt not found within poll budget"),
            RpcError::RateLimited => write!(f, "rpc rate limited (HTTP 429)"),
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
    /// HUP-S7.5 (D-27): `gasUsed`, if the node returns it.
    pub gas_used: Option<u64>,
    /// HUP-S7.5 (D-27): `effectiveGasPrice` (wei per gas actually paid), if the node returns it.
    pub effective_gas_price: Option<u128>,
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

/// RPC calls are BOUNDED so a slow/wedged node or public RPC can never hang the caller forever —
/// critical because `node_status` polls this on a timer and a blocking call freezes the UI thread.
/// On timeout the call returns `RpcError::Transport` and the surface falls to its honest state.
const RPC_TIMEOUT_SECS: u64 = 6;

/// Map a transport failure. HTTP 429 is the node's rate limiter refusing the request before any
/// handler ran (UI truth contract v1.1 §1), so it stays distinct; everything else is Transport.
pub(crate) fn map_ureq_error(e: ureq::Error) -> RpcError {
    match e {
        ureq::Error::StatusCode(429) => RpcError::RateLimited,
        other => RpcError::Transport(other.to_string()),
    }
}

impl RpcTransport for HttpTransport {
    fn call(&self, body: Value) -> Result<Value, RpcError> {
        // Never follow a redirect: a loopback fork target checked by URL must not be able to send
        // the request (or the next one) to another host. A 3xx is an error, not an answer.
        let resp = ureq::post(&self.url)
            .config()
            .timeout_global(Some(std::time::Duration::from_secs(RPC_TIMEOUT_SECS)))
            .max_redirects(0)
            .build()
            .send_json(&body)
            .map_err(map_ureq_error)?;
        let mut resp = resp;
        resp.body_mut()
            .read_json::<Value>()
            .map_err(|e| RpcError::BadResponse(e.to_string()))
    }
}

/// A single `eth_getLogs` log entry, decoded to the facts the withdraw
/// enumeration needs (WP2). `topics` are the indexed event params (topic0 = the
/// event signature hash, topic1/2 = indexed args, `0x…` hex words); `data` is
/// the ABI-encoded non-indexed params (`0x…` hex); `block_number` is the block
/// the log was emitted in (decoded from the `blockNumber` hex quantity). Rule 1:
/// every field is a live-node fact, never fabricated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogEntry {
    /// The log topics (`0x…` hex, 32-byte words): topic0 = event signature hash,
    /// then the indexed args in declaration order.
    pub topics: Vec<String>,
    /// The ABI-encoded non-indexed event params (`0x…` hex).
    pub data: String,
    /// The block the log was emitted in (decoded from `blockNumber`).
    pub block_number: u64,
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

    /// Read-only access to the underlying transport. Added in the WP-S1.2 kit
    /// extraction so tests in a *consuming* crate (citrate-core / citrate-quorum)
    /// can inspect a mock transport's recorded requests now that the `transport`
    /// field is crate-private to citrate-core-kit. Production code uses only the
    /// request methods; this is test-support introspection, not a new capability.
    pub fn transport(&self) -> &T {
        &self.transport
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
        Self::result_of(resp)
    }

    /// The `result` of a JSON-RPC response, or its `error` as [`RpcError::Node`].
    fn result_of(resp: Value) -> Result<Value, RpcError> {
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

    /// `eth_getTransactionCount(address, "latest")`: how many of the address's transactions are
    /// mined. A sent transaction whose nonce is below this and that has no receipt was replaced or
    /// dropped and can never be mined.
    pub fn latest_nonce(&self, address: &str) -> Result<u64, RpcError> {
        let result = self.request("eth_getTransactionCount", json!([address, "latest"]))?;
        parse_hex_quantity(&result, "eth_getTransactionCount")
    }

    /// `eth_getTransactionByHash(hash)`: whether the node holds the transaction (pending or
    /// mined). `false` only on an explicit `null` answer.
    pub fn transaction_known(&self, tx_hash: &str) -> Result<bool, RpcError> {
        let result = self.request("eth_getTransactionByHash", json!([tx_hash]))?;
        Ok(!result.is_null())
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

    /// `eth_getLogs([filter])` → the matching log entries (real values from the
    /// live 40204 RPC; CORE WP2 withdraw enumeration). `filter` is the pre-built
    /// filter object (caller-shaped so we stay transport-only), e.g.
    /// `{address, topics:[topic0, null, staker_padded], fromBlock, toBlock}`. Each
    /// entry is decoded to a [`LogEntry`] (topics + data + block number). A
    /// malformed entry (missing/short field) errors rather than silently drops
    /// (Rule 1 — no fabricated or partial withdrawal list).
    pub fn get_logs(&self, filter: Value) -> Result<Vec<LogEntry>, RpcError> {
        let result = self.request("eth_getLogs", json!([filter]))?;
        parse_logs(&result)
    }

    /// `eth_getLogs([filter])` for a caller that pages a block range (C1-W). Like
    /// [`RpcClient::get_logs`], except the answers a paging caller acts on are told apart
    /// ([`GetLogsError`]): a range over the node's limit, a retryable answer, a missing method.
    pub fn get_logs_range(&self, filter: Value) -> Result<Vec<LogEntry>, GetLogsError> {
        let result = self.logs_request("eth_getLogs", filter)?;
        parse_logs(&result).map_err(GetLogsError::Rpc)
    }

    /// `citrate_getLogs([filter])` (citrate-chain#337): the `eth_getLogs` logs plus `through`, the
    /// executed block the answer is complete through, both from one read of the node's applied
    /// tip (`null` when nothing in the range has executed). `filter` may carry `afterBlockHash`,
    /// the caller's previous `through`; a node whose chain no longer holds it at `fromBlock - 1`
    /// answers -32001 ([`GetLogsError::Reorged`]). A malformed answer is an error: a `through`
    /// outside `fromBlock..=toBlock`, a hash that is not 32 bytes, a log outside
    /// `fromBlock..=through`, or logs without a `through` (C1-W).
    pub fn citrate_get_logs(&self, filter: Value) -> Result<ServedLogs, GetLogsError> {
        let bad =
            |m: String| GetLogsError::Rpc(RpcError::BadResponse(format!("citrate_getLogs: {m}")));
        let window = |field: &str| {
            filter
                .get(field)
                .ok_or_else(|| bad(format!("the filter has no {field}")))
                .and_then(|v| parse_hex_quantity(v, field).map_err(GetLogsError::Rpc))
        };
        let (from, to) = (window("fromBlock")?, window("toBlock")?);
        let result = self.logs_request("citrate_getLogs", filter.clone())?;
        let logs =
            parse_logs(result.get("logs").unwrap_or(&Value::Null)).map_err(GetLogsError::Rpc)?;
        let through = match result.get("through") {
            None => return Err(bad("no through".into())),
            Some(Value::Null) => None,
            Some(t) => {
                let number = t
                    .get("number")
                    .ok_or_else(|| bad("through.number missing".into()))
                    .and_then(|v| {
                        parse_hex_quantity(v, "through.number").map_err(GetLogsError::Rpc)
                    })?;
                let hash = t
                    .get("hash")
                    .and_then(Value::as_str)
                    .filter(|h| is_block_hash(h))
                    .ok_or_else(|| bad("through.hash is not a 32-byte hash".into()))?;
                Some(BlockRef {
                    number,
                    hash: hash.to_ascii_lowercase(),
                })
            }
        };
        match &through {
            None if !logs.is_empty() => return Err(bad("logs without a through".into())),
            Some(t) if t.number < from || t.number > to => {
                return Err(bad(format!(
                    "through {} is outside the window {from}-{to}",
                    t.number
                )))
            }
            _ => {}
        }
        if let Some(t) = &through {
            if let Some(l) = logs
                .iter()
                .find(|l| l.block_number < from || l.block_number > t.number)
            {
                return Err(bad(format!(
                    "a log at block {} is outside the served range {from}-{}",
                    l.block_number, t.number
                )));
            }
        }
        Ok(ServedLogs { logs, through })
    }

    /// One log request (`eth_getLogs` or `citrate_getLogs`) with its error answers told apart:
    /// - -32005 with `data.maxBlockRange`: the range is over the node's limit
    ///   (citrate-chain core/api/src/canonical_logs.rs:127-146). The method budget also answers
    ///   -32005, with no data (core/api/src/rate_limit.rs:96-101); a smaller window does not help
    ///   there, so it is an ordinary error.
    /// - -32002 with `data.retryable: true`: retry the same range (index warming, or a receipt not
    ///   stored yet; canonical_logs.rs:285-291, 646-658). Without the flag it is an ordinary error.
    /// - -32001 with `data.reorged: true`: `afterBlockHash` is not on the node's executed chain
    ///   (canonical_logs.rs:660-673).
    /// - -32601: the node does not have the method.
    fn logs_request(&self, method: &str, filter: Value) -> Result<Value, GetLogsError> {
        let body = self.build_request(method, json!([filter]));
        let resp = self.transport.call(body).map_err(GetLogsError::Rpc)?;
        if let Some(err) = resp.get("error") {
            let msg = || {
                err.get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("node error")
                    .to_string()
            };
            let flag = |name: &str| err.pointer(name).and_then(Value::as_bool) == Some(true);
            match err.get("code").and_then(Value::as_i64) {
                Some(RPC_LIMIT_EXCEEDED) if err.pointer("/data/maxBlockRange").is_some() => {
                    return Err(GetLogsError::RangeTooLarge(msg()))
                }
                Some(RPC_RETRY) if flag("/data/retryable") => {
                    return Err(GetLogsError::Retry(msg()))
                }
                Some(RPC_REORGED) if flag("/data/reorged") => {
                    return Err(GetLogsError::Reorged(msg()))
                }
                Some(RPC_METHOD_NOT_FOUND) => return Err(GetLogsError::MethodNotFound),
                _ => {}
            }
        }
        Self::result_of(resp).map_err(GetLogsError::Rpc)
    }

    /// `citrate_isExecuted(hash)` → `(executed, number)` (citrate-chain#337): whether the block with
    /// that hash is on the executed chain (an abandoned block is still stored, but `false`), and its
    /// height when it is. `None` when the node does not have the method (-32601: an older node).
    /// -32603 (no applied tip), -32602 (malformed hash) and a malformed result are errors (C1-W).
    pub fn is_executed(&self, hash: &str) -> Result<Option<(bool, Option<u64>)>, RpcError> {
        let body = self.build_request("citrate_isExecuted", json!([hash]));
        let resp = self.transport.call(body)?;
        if resp.pointer("/error/code").and_then(Value::as_i64) == Some(RPC_METHOD_NOT_FOUND) {
            return Ok(None);
        }
        let result = Self::result_of(resp)?;
        let executed = result
            .get("executed")
            .and_then(Value::as_bool)
            .ok_or_else(|| RpcError::MissingField("citrate_isExecuted.executed".into()))?;
        let number = match result.get("number") {
            None | Some(Value::Null) => None,
            Some(v) => Some(parse_hex_quantity(v, "citrate_isExecuted.number")?),
        };
        Ok(Some((executed, number)))
    }

    /// Several `eth_call`s (`[call, "latest"]`) in ONE JSON-RPC batch (C1-W). Answers are matched
    /// by id and returned in call order; any failed item fails the whole batch (no partial list).
    pub fn eth_call_batch(&self, calls: &[Value]) -> Result<Vec<Vec<u8>>, RpcError> {
        if calls.is_empty() {
            return Ok(Vec::new());
        }
        let reqs: Vec<Value> = calls
            .iter()
            .map(|c| self.build_request("eth_call", json!([c, "latest"])))
            .collect();
        let resp = self.transport.call(Value::Array(reqs.clone()))?;
        let answers = resp
            .as_array()
            .ok_or_else(|| RpcError::BadResponse("eth_call batch: not an array".into()))?;
        reqs.iter()
            .map(|req| {
                let a = answers
                    .iter()
                    .find(|a| a.get("id") == req.get("id"))
                    .ok_or_else(|| {
                        RpcError::MissingField("eth_call batch: an answer is missing".into())
                    })?;
                let result = Self::result_of(a.clone())?;
                let s = result.as_str().ok_or_else(|| {
                    RpcError::MissingField("eth_call: result not a string".into())
                })?;
                let hexs = s
                    .strip_prefix("0x")
                    .ok_or_else(|| RpcError::MissingField("eth_call: missing 0x prefix".into()))?;
                hex::decode(hexs)
                    .map_err(|_| RpcError::MissingField(format!("eth_call: result not hex ({s})")))
            })
            .collect()
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

    /// `eth_getBalance(address, "latest")` → the address's native SALT balance in
    /// wei (real value from the live 40204 RPC; CORE wallet balances). SALT is the
    /// chain's native currency (`chain.ts` nativeCurrency), so the wallet's liquid
    /// balance is a plain `eth_getBalance`. Returned as `u128` — a SALT balance
    /// (e.g. a 32,000-SALT grant = 3.2e22 wei) exceeds `u64`, so we parse the wide
    /// quantity and never truncate (Rule 1).
    pub fn get_balance(&self, address: &str) -> Result<u128, RpcError> {
        let result = self.request("eth_getBalance", json!([address, "latest"]))?;
        parse_hex_quantity_u128(&result, "eth_getBalance")
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
        let gas_used = result
            .get("gasUsed")
            .and_then(|v| parse_hex_quantity(v, "receipt.gasUsed").ok());
        let effective_gas_price = result
            .get("effectiveGasPrice")
            .and_then(|v| parse_hex_quantity_u128(v, "receipt.effectiveGasPrice").ok());
        Ok(Some(Receipt {
            tx_hash: tx_hash.to_string(),
            block_number,
            status,
            gas_used,
            effective_gas_price,
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

/// Decode a JSON string hex quantity (`"0x…"`) to a `u128`. Native SALT balances
/// (wei) exceed `u64`, so balance reads use the wide form. Same shape as
/// [`parse_hex_quantity`] otherwise.
fn parse_hex_quantity_u128(v: &Value, field: &str) -> Result<u128, RpcError> {
    let s = v
        .as_str()
        .ok_or_else(|| RpcError::MissingField(format!("{field}: not a string")))?;
    let stripped = s
        .strip_prefix("0x")
        .ok_or_else(|| RpcError::MissingField(format!("{field}: missing 0x prefix")))?;
    u128::from_str_radix(stripped, 16)
        .map_err(|_| RpcError::MissingField(format!("{field}: not hex ({s})")))
}

/// Whether `h` is a `0x`-prefixed 32-byte hex block hash.
fn is_block_hash(h: &str) -> bool {
    h.strip_prefix("0x")
        .is_some_and(|d| d.len() == 64 && d.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// Decode an `eth_getLogs` result array. A malformed entry errors rather than being dropped.
fn parse_logs(result: &Value) -> Result<Vec<LogEntry>, RpcError> {
    let arr = result
        .as_array()
        .ok_or_else(|| RpcError::MissingField("eth_getLogs: result not an array".into()))?;
    let mut logs = Vec::with_capacity(arr.len());
    for entry in arr {
        let topics = entry
            .get("topics")
            .and_then(Value::as_array)
            .ok_or_else(|| RpcError::MissingField("log.topics".into()))?
            .iter()
            .map(|t| {
                t.as_str()
                    .map(str::to_string)
                    .ok_or_else(|| RpcError::MissingField("log.topics[]: not a string".into()))
            })
            .collect::<Result<Vec<String>, RpcError>>()?;
        let data = entry
            .get("data")
            .and_then(Value::as_str)
            .ok_or_else(|| RpcError::MissingField("log.data".into()))?
            .to_string();
        let block_number = entry
            .get("blockNumber")
            .ok_or_else(|| RpcError::MissingField("log.blockNumber".into()))
            .and_then(|v| parse_hex_quantity(v, "log.blockNumber"))?;
        logs.push(LogEntry {
            topics,
            data,
            block_number,
        });
    }
    Ok(logs)
}

/// The JSON-RPC error code for a request over a node limit; citrate-chain#334 returns it, with
/// `data.maxBlockRange`, for a log range wider than 1000 blocks (the method budget uses it too,
/// without data).
pub const RPC_LIMIT_EXCEEDED: i64 = -32005;

/// The JSON-RPC code citrate-chain#334/#337 return, with `data.retryable = true`, while a deep
/// historical range is not served yet (the node's cache fills over several requests) or, for
/// `citrate_getLogs`, while an executed block's receipts are not stored yet: retry the same range
/// later. Without that flag it is an ordinary error.
pub const RPC_RETRY: i64 = -32002;

/// The JSON-RPC code citrate-chain#337 `citrate_getLogs` returns, with `data.reorged = true`, when
/// `afterBlockHash` is not the executed block at `fromBlock - 1`.
pub const RPC_REORGED: i64 = -32001;

/// The JSON-RPC "method not found" code.
pub const RPC_METHOD_NOT_FOUND: i64 = -32601;

/// A block named by number and hash (`citrate_getLogs` `through`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockRef {
    pub number: u64,
    pub hash: String,
}

/// A `citrate_getLogs` answer: the logs, and the executed block the answer is complete through.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServedLogs {
    pub logs: Vec<LogEntry>,
    pub through: Option<BlockRef>,
}

/// A log-read failure (`eth_getLogs`, `citrate_getLogs`), with the answers a paging caller acts on
/// told apart from every other error (C1-W).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GetLogsError {
    /// `afterBlockHash` is not the node's executed block at `fromBlock - 1`: the chain the caller
    /// scanned was replaced ([`RPC_REORGED`] with `data.reorged`, citrate-chain#337).
    Reorged(String),
    /// The node does not have the method ([`RPC_METHOD_NOT_FOUND`]): today's public RPC has no
    /// `citrate_getLogs`.
    MethodNotFound,
    /// The node refused the block range as too wide ([`RPC_LIMIT_EXCEEDED`]).
    RangeTooLarge(String),
    /// The node asked for the same range again later ([`RPC_RETRY`]).
    Retry(String),
    /// Any other failure.
    Rpc(RpcError),
}

impl std::fmt::Display for GetLogsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GetLogsError::RangeTooLarge(m) => write!(f, "rpc block range too large: {m}"),
            GetLogsError::Retry(m) => write!(f, "rpc asked to retry: {m}"),
            GetLogsError::Rpc(e) => write!(f, "{e}"),
            GetLogsError::Reorged(m) => write!(f, "{m}"),
            GetLogsError::MethodNotFound => write!(f, "method not found"),
        }
    }
}

#[cfg(test)]
mod tests {
    include!("rpc_tests.rs");
}
