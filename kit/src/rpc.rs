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

/// The network a signing path must be talking to before it signs or sends (K6 network switch).
///
/// A reroll or a new genesis can keep chain id 40204, so the chain id alone (EIP-155) does not
/// tell two networks apart: the block-0 hash does. `None` in either field means the build has no
/// value for it yet (an explicit "not set", never a placeholder), and every check against it
/// refuses. The app installs one with [`install_network_gate`] at startup from its network
/// profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpectedNetwork {
    /// The profile name, for messages.
    pub name: String,
    /// The EIP-155 chain id, or `None` when not set.
    pub chain_id: Option<u64>,
    /// The block-0 hash (`0x` + 64 lowercase hex), or `None` when not set.
    pub genesis_hash: Option<String>,
}

static NETWORK_GATE: std::sync::OnceLock<ExpectedNetwork> = std::sync::OnceLock::new();

/// Install the process-wide network gate. The first install wins; returns `false` when one was
/// already installed (the earlier one stays). Every transaction sent through
/// [`RpcClient::send_raw_transaction`] and every ceremony signature is checked against it.
/// A process that never installs one (tests, other apps on this kit) is not gated.
pub fn install_network_gate(expected: ExpectedNetwork) -> bool {
    NETWORK_GATE.set(expected).is_ok()
}

/// The installed network gate, if any.
pub fn network_gate() -> Option<&'static ExpectedNetwork> {
    NETWORK_GATE.get()
}

impl ExpectedNetwork {
    /// Both values are set and well formed. A gate that is not set refuses every signature.
    pub fn is_set(&self) -> bool {
        self.chain_id.is_some_and(|c| c != 0)
            && self
                .genesis_hash
                .as_deref()
                .is_some_and(|g| is_block_hash(&g.to_ascii_lowercase()))
    }
}

/// What a wallet-key signature commits to, for the signing gate ([`check_wallet_signing`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignatureScope {
    /// The signature commits to no chain: a plain `personal_sign` (EIP-191) message. It cannot be
    /// checked against a network, so it is NOT network-bound; it is still refused while the
    /// network is not set.
    Unbound,
    /// The signature commits to this EIP-155 chain id: a transaction, a Sign-In with Ethereum
    /// message (`Chain ID:`), or EIP-712 typed data (`domain.chainId`). It must be the chain id
    /// of the network this build targets.
    Chain(u64),
}

thread_local! {
    /// Gates added for the current thread by [`with_added_network_gate`].
    static ADDED_GATES: std::cell::RefCell<Vec<ExpectedNetwork>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Run `f` with `extra` checked as well as the process-wide gate, on this thread. It can only add
/// a check: the installed gate still applies, so this never loosens signing. Tests use it to put
/// a not-set or another network in front of a signing entry point without touching the
/// process-wide gate.
pub fn with_added_network_gate<R>(extra: ExpectedNetwork, f: impl FnOnce() -> R) -> R {
    struct Pop;
    impl Drop for Pop {
        fn drop(&mut self) {
            ADDED_GATES.with(|g| {
                g.borrow_mut().pop();
            });
        }
    }
    ADDED_GATES.with(|g| g.borrow_mut().push(extra));
    let _pop = Pop;
    f()
}

fn check_one(gate: &ExpectedNetwork, scope: SignatureScope) -> Result<(), RpcError> {
    let name = &gate.name;
    if !gate.is_set() {
        return Err(RpcError::WrongNetwork(format!(
            "this build has no network set (profile {name:?}), so the wallet signs nothing; \
             update Citrate Core"
        )));
    }
    if let SignatureScope::Chain(c) = scope {
        let want = gate.chain_id.unwrap_or_default();
        if c != want {
            return Err(RpcError::WrongNetwork(format!(
                "the signature is for chain {c}, this build targets chain {want} ({name})"
            )));
        }
    }
    Ok(())
}

/// **The one gate on wallet-key signing (K6).** Every signer in `wallet.rs` calls this (through
/// `wallet::signing_key`) before the wallet key is derived, whatever path reached it: a ceremony
/// approval, the budgeted sign-in, a transaction broadcast. It checks, locally and with no RPC
/// call, the installed gate and any added for this thread:
/// - a gate that is not set refuses every signature, of any kind;
/// - a chain-committing signature ([`SignatureScope::Chain`]) must name the gate's chain id.
///
/// The RPC genesis check ([`RpcClient::verify_network`]) is separate and applies where a signature
/// goes to an endpoint (transactions). A process with no gate installed and none added (other
/// apps on this kit) is not gated.
pub fn check_wallet_signing(scope: SignatureScope) -> Result<(), RpcError> {
    if let Some(g) = network_gate() {
        check_one(g, scope)?;
    }
    ADDED_GATES.with(|gates| {
        for g in gates.borrow().iter() {
            check_one(g, scope)?;
        }
        Ok(())
    })
}

/// **No RPC without a network (K6, invariant P3).** Every JSON-RPC request made through
/// [`HttpTransport`] calls this first. It refuses, before any connection is made, while the
/// installed gate or a gate added for this thread ([`with_added_network_gate`]) is not set: a
/// build whose network profile is not ready reads nothing from any chain. A process with no gate
/// installed and none added (other apps on this kit) is not gated.
pub fn check_rpc_allowed() -> Result<(), RpcError> {
    let one = |g: &ExpectedNetwork| {
        if g.is_set() {
            Ok(())
        } else {
            Err(RpcError::WrongNetwork(format!(
                "this build has no network set (profile {:?}), so no RPC request is made; \
                 update Citrate Core",
                g.name
            )))
        }
    };
    if let Some(g) = network_gate() {
        one(g)?;
    }
    ADDED_GATES.with(|gates| {
        for g in gates.borrow().iter() {
            one(g)?;
        }
        Ok(())
    })
}

fn is_block_hash(s: &str) -> bool {
    s.len() == 66
        && s.starts_with("0x")
        && s[2..].bytes().all(|b| b.is_ascii_hexdigit())
        && !s[2..].bytes().all(|b| b == b'0')
}

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
    /// The network check refused this endpoint: it is not the network this build targets, or
    /// the build has no network set (see [`ExpectedNetwork`]). Nothing was signed or sent.
    WrongNetwork(String),
}

impl std::fmt::Display for RpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RpcError::Transport(m) => write!(f, "rpc transport error: {m}"),
            RpcError::BadResponse(m) => write!(f, "rpc bad response: {m}"),
            RpcError::Node(m) => write!(f, "rpc node error: {m}"),
            RpcError::MissingField(m) => write!(f, "rpc missing/invalid field: {m}"),
            RpcError::ReceiptTimeout => write!(f, "rpc receipt not found within poll budget"),
            RpcError::WrongNetwork(m) => write!(f, "network check refused: {m}"),
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

impl RpcTransport for HttpTransport {
    fn call(&self, body: Value) -> Result<Value, RpcError> {
        // K6 (P3): no request at all while this build has no network set.
        check_rpc_allowed()?;
        // Never follow a redirect: a loopback fork target checked by URL must not be able to send
        // the request (or the next one) to another host. A 3xx is an error, not an answer.
        let resp = ureq::post(&self.url)
            .config()
            .timeout_global(Some(std::time::Duration::from_secs(RPC_TIMEOUT_SECS)))
            .max_redirects(0)
            .build()
            .send_json(&body)
            .map_err(|e| RpcError::Transport(e.to_string()))?;
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

    /// `eth_blockNumber` → the node's current head height (real value from the
    /// spawned node's local RPC; CORE-C1.1 NodeDomain status). The node returns
    /// a `0x`-prefixed hex quantity.
    pub fn block_number(&self) -> Result<u64, RpcError> {
        let result = self.request("eth_blockNumber", json!([]))?;
        parse_hex_quantity(&result, "eth_blockNumber")
    }

    /// `eth_chainId` → the endpoint's EIP-155 chain id.
    pub fn chain_id(&self) -> Result<u64, RpcError> {
        let result = self.request("eth_chainId", json!([]))?;
        parse_hex_quantity(&result, "eth_chainId")
    }

    /// The hash of block 0 (`eth_getBlockByNumber("0x0", false).hash`), lowercase.
    pub fn genesis_hash(&self) -> Result<String, RpcError> {
        let result = self.request("eth_getBlockByNumber", json!(["0x0", false]))?;
        let hash = result
            .get("hash")
            .and_then(Value::as_str)
            .ok_or_else(|| RpcError::MissingField("block 0 hash".into()))?
            .to_ascii_lowercase();
        if !is_block_hash(&hash) {
            return Err(RpcError::MissingField(format!("block 0 hash: {hash}")));
        }
        Ok(hash)
    }

    /// Check that this endpoint is `expected`: its chain id and its block-0 hash must both match.
    /// Fails closed: a value the build has not set, or an endpoint that cannot be read, refuses.
    /// Reads `eth_chainId` and block 0 only; nothing is signed or sent.
    pub fn verify_network(&self, expected: &ExpectedNetwork) -> Result<(), RpcError> {
        let name = &expected.name;
        let want_id = expected.chain_id.ok_or_else(|| {
            RpcError::WrongNetwork(format!(
                "this build has no chain id set for network {name:?}"
            ))
        })?;
        let want_genesis = expected
            .genesis_hash
            .as_deref()
            .map(str::to_ascii_lowercase)
            .filter(|g| is_block_hash(g))
            .ok_or_else(|| {
                RpcError::WrongNetwork(format!(
                    "this build has no genesis hash set for network {name:?}"
                ))
            })?;
        let got_id = self.chain_id()?;
        if got_id != want_id {
            return Err(RpcError::WrongNetwork(format!(
                "the endpoint is chain {got_id}, this build targets chain {want_id} ({name})"
            )));
        }
        let got_genesis = self.genesis_hash()?;
        if got_genesis != want_genesis {
            return Err(RpcError::WrongNetwork(format!(
                "the endpoint's block 0 is {got_genesis}, this build targets {want_genesis} \
                 ({name}); update Citrate Core"
            )));
        }
        Ok(())
    }

    /// [`Self::send_raw_transaction`] against an explicit gate: with `Some`, the endpoint is
    /// checked first ([`Self::verify_network`]) and nothing is sent unless it passes.
    pub fn send_raw_transaction_gated(
        &self,
        raw: &[u8],
        gate: Option<&ExpectedNetwork>,
    ) -> Result<String, RpcError> {
        if let Some(expected) = gate {
            self.verify_network(expected)?;
        }
        let raw_hex = format!("0x{}", hex::encode(raw));
        let result = self.request("eth_sendRawTransaction", json!([raw_hex]))?;
        result
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| RpcError::MissingField("sendRawTransaction hash".into()))
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
    ///
    /// K6: when the app installed a network gate ([`install_network_gate`]), the endpoint's
    /// chain id and block-0 hash are checked first and nothing is sent unless they match.
    pub fn send_raw_transaction(&self, raw: &[u8]) -> Result<String, RpcError> {
        self.send_raw_transaction_gated(raw, network_gate())
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

#[cfg(test)]
mod tests {
    include!("rpc_tests.rs");
}
