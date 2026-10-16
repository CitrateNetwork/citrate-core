//! HUP-S4.2 / HUP-S8.5 — the citrate-node MCP catalog: tools (with MCP annotations), resources,
//! the precompile table, and the argument validators every tool runs before touching the node.
//!
//! Three kinds of tool:
//! - **read**: answered at once from the node, the chain RPC, memory, or the cluster daemon.
//!   Annotated `readOnlyHint: true`.
//! - **signature**: builds an unsigned transaction and opens a SignatureCeremony. Nothing is signed
//!   until the member approves it in the app. Annotated `destructiveHint: true`.
//! - **action**: a change that needs no signature (join a cluster, share a file, create or revoke
//!   an invite). Queued for the member's approval in the app, then run by core. Annotated
//!   `destructiveHint: true`.
//! - **budgeted** (HUP-S6.5): runs at once, but only inside a budget the member granted in the app
//!   (HIC-2), and core decides everything that matters. Today: `faucet_request`. Annotated
//!   `readOnlyHint: false`, `destructiveHint: false`.
//!
//! Write tools never return a result directly: they return a request id the client polls with
//! `request_status`.

use serde_json::{json, Value};

/// How a tool is executed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolKind {
    Read,
    Signature,
    Action,
    /// HUP-S6.5: runs inside a member-granted budget (HIC-2); see `crate::faucet`.
    Budgeted,
}

/// One tool in the catalog.
#[derive(Debug, Clone)]
pub struct ToolDef {
    pub name: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub kind: ToolKind,
    pub input_schema: fn() -> Value,
}

fn obj(props: Value, required: &[&str]) -> Value {
    json!({
        "type": "object",
        "properties": props,
        "required": required,
        "additionalProperties": false,
    })
}

fn no_args() -> Value {
    obj(json!({}), &[])
}

fn address_schema() -> Value {
    obj(
        json!({"address": {"type": "string", "description": "0x-prefixed 20-byte address"}}),
        &["address"],
    )
}

fn call_schema() -> Value {
    obj(
        json!({
            "to": {"type": "string", "description": "0x-prefixed 20-byte contract address"},
            "data": {"type": "string", "description": "0x-prefixed ABI-encoded calldata"},
            "from": {"type": "string", "description": "optional caller address"},
            "value_wei": {"type": "string", "description": "optional value in wei (decimal string)"},
        }),
        &["to"],
    )
}

fn logs_schema() -> Value {
    obj(
        json!({
            "address": {"type": "string", "description": "contract address (required)"},
            "topics": {"type": "array", "items": {"type": ["string", "null"]}, "maxItems": 4},
            "from_block": {"type": "integer", "minimum": 0},
            "to_block": {"type": "integer", "minimum": 0},
        }),
        &["address", "from_block", "to_block"],
    )
}

fn precompile_schema() -> Value {
    obj(
        json!({
            "address": {"type": "string", "description": "a precompile address from the precompile table, e.g. 0x0000000000000000000000000000000000000120"},
            "data": {"type": "string", "description": "0x-prefixed input bytes"},
        }),
        &["address", "data"],
    )
}

fn agent_precompile_encode_schema() -> Value {
    obj(
        json!({
            "operation": {"type": "string", "enum": crate::agent_precompiles::OPERATIONS,
                "description": "which agent precompile input to build"},
            "args": {"type": "object", "description": "LORA_APPLY: {w, b, a, alpha} with tensors {shape, q16} (q16 = raw Q16.16 integers, value x 65536); LORA_MERGE: {adapters: [{b, a, alpha, weight}]}; MEMORY_ANCHOR_VERIFY: {proof} in the sidecar's /anchor/proof shape; DEVICE_LINK_VERIFY: a stored device link {member, device, wallet, index, label, issuedAt, memberSig, deviceSig, walletSig}; DEVICE_REVOCATION_VERIFY: {member, device, revokedAt, memberSig}"},
        }),
        &["operation", "args"],
    )
}

fn agent_precompile_decode_schema() -> Value {
    obj(
        json!({
            "operation": {"type": "string", "enum": crate::agent_precompiles::OPERATIONS,
                "description": "which agent precompile produced the output"},
            "output": {"type": "string", "description": "0x-prefixed bytes the precompile returned (through a contract)"},
        }),
        &["operation", "output"],
    )
}

fn memory_schema() -> Value {
    obj(
        json!({
            "query": {"type": "string", "description": "what to search for"},
            "tenant": {"type": "string", "enum": MEMORY_TENANTS, "description": "which shared graph to search (default citrate-docs)"},
            "limit": {"type": "integer", "minimum": 1, "maximum": 25},
        }),
        &["query"],
    )
}

fn group_schema() -> Value {
    obj(
        json!({"group": {"type": "string", "description": "group id from groups_list"}}),
        &["group"],
    )
}

fn share_schema() -> Value {
    obj(
        json!({
            "group": {"type": "string", "description": "group id from groups_list"},
            "cid": {"type": "string", "description": "IPFS CID of a file already pinned on this node"},
        }),
        &["group", "cid"],
    )
}

fn invite_create_schema() -> Value {
    obj(
        json!({
            "group": {"type": "string", "description": "group id from groups_list"},
            "for_handle": {"type": "string", "description": "who the invite is for (a label only)"},
        }),
        &["group", "for_handle"],
    )
}

fn invite_revoke_schema() -> Value {
    obj(
        json!({
            "group": {"type": "string"},
            "invite_id": {"type": "string", "description": "invite id from invites_list"},
        }),
        &["group", "invite_id"],
    )
}

fn tx_schema() -> Value {
    obj(
        json!({
            "to": {"type": "string", "description": "0x-prefixed 20-byte destination"},
            "value_wei": {"type": "string", "description": "value in wei (decimal string, default 0)"},
            "data": {"type": "string", "description": "0x-prefixed calldata (default 0x)"},
        }),
        &["to"],
    )
}

/// HUP-S6.5: the faucet tool names the deploy it is for, and nothing that chooses a recipient,
/// an amount or a time (faucet ADR D3).
fn faucet_schema() -> Value {
    obj(
        json!({"initcode_hash": {"type": "string", "description": "0x keccak256 of the init code of a deploy the deploy gate marked READY"}}),
        &["initcode_hash"],
    )
}

fn ed25519_schema() -> Value {
    obj(
        json!({
            "public_key": {"type": "string", "description": "0x-prefixed 32-byte Ed25519 public key (64 hex characters)"},
            "signature": {"type": "string", "description": "0x-prefixed 64-byte Ed25519 signature (128 hex characters)"},
            "message": {"type": "string", "description": "0x-prefixed message bytes that were signed (at most 8 KiB)"},
        }),
        &["public_key", "signature", "message"],
    )
}

fn pin_schema() -> Value {
    obj(
        json!({"cid": {"type": "string", "description": "IPFS CID to keep on this node"}}),
        &["cid"],
    )
}

fn deploy_schema() -> Value {
    obj(
        json!({
            "bytecode": {"type": "string", "description": "0x-prefixed compiled deploy bytecode (the exact bytes the deploy gate checked)"},
            "constructor_args": {"type": "string", "description": "0x-prefixed ABI-encoded constructor arguments (default none)"},
            "value_wei": {"type": "string", "description": "value sent with the creation, in wei (decimal string, default 0)"},
            "gas": {"type": "integer", "minimum": 21000, "maximum": MAX_DEPLOY_GAS, "description": "gas limit for the creation (default 2,000,000)"},
        }),
        &["bytecode"],
    )
}

fn status_schema() -> Value {
    obj(
        json!({"id": {"type": "string", "description": "request id returned by a write tool"}}),
        &["id"],
    )
}

/// The memory tenants an MCP client may search. The member's `personal` tenant is deliberately
/// absent (pending owner sign-off on a per-token scope for personal memory).
pub const MEMORY_TENANTS: &[&str] = &["citrate-docs", "chain-state"];

/// The highest gas limit a `deploy_propose` may ask for (the member still sees it in the ceremony).
pub const MAX_DEPLOY_GAS: u64 = 30_000_000;

/// The full tool catalog, in the order `tools/list` returns it.
pub const TOOLS: &[ToolDef] = &[
    ToolDef { name: "node_status", title: "Node status", kind: ToolKind::Read, input_schema: no_args,
        description: "This node's state (running, starting, stopped), peer count, head height, and sync percentage." },
    ToolDef { name: "chain_head", title: "Chain head", kind: ToolKind::Read, input_schema: no_args,
        description: "The chain id and current head block number, read from this node (or the public RPC when the node is not running; the answer says which)." },
    ToolDef { name: "get_balance", title: "Get balance", kind: ToolKind::Read, input_schema: address_schema,
        description: "The native SALT balance of an address, in wei." },
    ToolDef { name: "chain_call", title: "Contract read (eth_call)", kind: ToolKind::Read, input_schema: call_schema,
        description: "Run a read-only contract call at the latest block and return the raw result bytes. Never sends a transaction." },
    ToolDef { name: "estimate_gas", title: "Estimate gas", kind: ToolKind::Read, input_schema: call_schema,
        description: "Ask the node how much gas a call would use. Never sends a transaction." },
    ToolDef { name: "get_logs", title: "Get logs", kind: ToolKind::Read, input_schema: logs_schema,
        description: "Event logs for one contract over a block range of at most 5,000 blocks." },
    ToolDef { name: "precompile_table", title: "Precompile table", kind: ToolKind::Read, input_schema: no_args,
        description: "The Citrate precompiles a client may call read-only with precompile_call, with their addresses and what they do." },
    ToolDef { name: "precompile_call", title: "Precompile call", kind: ToolKind::Read, input_schema: precompile_schema,
        description: "Call a Citrate precompile read-only (eth_call) and return its output bytes. Only addresses in the precompile table are accepted. On a node built for the 2026-10-05 reroll a top-level call to a precompile address runs the precompile; an empty reply (empty calldata, which is a plain transfer, or a node built before the reroll) is an error, never a result." },
    ToolDef { name: "ed25519_verify", title: "Verify an Ed25519 signature", kind: ToolKind::Read, input_schema: ed25519_schema,
        description: "Check an Ed25519 signature with the chain's ED25519_VERIFY precompile (0x0120), read-only. Encodes public key, signature and message for the precompile and returns whether the chain accepts the signature." },
    ToolDef { name: "agent_precompile_encode", title: "Encode an agent precompile input", kind: ToolKind::Read, input_schema: agent_precompile_encode_schema,
        description: "Build the exact input bytes, gas and Solidity helper for the agent precompile fork (0x0112 LORA_APPLY, 0x0113 LORA_MERGE, 0x0121 MEMORY_ANCHOR_VERIFY, 0x0122 AGENT_OPS device link and revocation checks). Pure: no RPC, no signing. These precompiles are active from genesis on chain 40204; contract code reaches them through CitratePrecompiles, and on a node built for the reroll a top-level call with calldata runs them too." },
    ToolDef { name: "agent_precompile_decode", title: "Decode an agent precompile answer", kind: ToolKind::Read, input_schema: agent_precompile_decode_schema,
        description: "Read an agent precompile's output: the LoRA result tensor, the anchor day commitment (or invalid), or the device link / revocation verdict. Empty output means the precompile was not active and is an error, never a verdict." },
    ToolDef { name: "wallet_info", title: "Wallet (public)", kind: ToolKind::Read, input_schema: no_args,
        description: "This member's public wallet address and SALT balance. Never returns key material." },
    ToolDef { name: "address_book", title: "Address book", kind: ToolKind::Read, input_schema: no_args,
        description: "The 40204 contract addresses this build was shipped with (the contracts this app reads, each checked to have code when the book was generated). A contract missing here is not in this app's book." },
    ToolDef { name: "memory_search", title: "Memory search", kind: ToolKind::Read, input_schema: memory_schema,
        description: "Search the node's shared knowledge graphs (Citrate docs and chain-state facts). The member's personal memory is not available over MCP." },
    ToolDef { name: "groups_list", title: "Groups", kind: ToolKind::Read, input_schema: no_args,
        description: "The groups this member belongs to (id and name)." },
    ToolDef { name: "cluster_status", title: "Cluster status", kind: ToolKind::Read, input_schema: group_schema,
        description: "A group's cluster: peers online, total peers, and files shared over the mesh." },
    ToolDef { name: "cluster_peers", title: "Cluster peers", kind: ToolKind::Read, input_schema: group_schema,
        description: "The group's authorized cluster peers and whether each is connected." },
    ToolDef { name: "cluster_devices", title: "Cluster devices", kind: ToolKind::Read, input_schema: group_schema,
        description: "The group's members with each member's linked machines (label, index, when linked) and whether each machine is connected. Addresses only; no keys or signatures." },
    ToolDef { name: "invites_list", title: "Invites", kind: ToolKind::Read, input_schema: group_schema,
        description: "Outstanding invites this member created for a group (id, who it is for, when). Links and tokens are not returned." },
    ToolDef { name: "dag_stats", title: "DAG statistics", kind: ToolKind::Read, input_schema: no_args,
        description: "The BlockDAG's current tips, height, highest blue score and GhostDAG parameters, from this node (or the public RPC; the answer says which)." },
    ToolDef { name: "devices_list", title: "My devices", kind: ToolKind::Read, input_schema: no_args,
        description: "The member's linked devices (address, name, link index, when linked, whether it is this machine) and revoked device addresses. Public information only; no signatures or keys." },
    ToolDef { name: "pins_list", title: "Pinned files", kind: ToolKind::Read, input_schema: no_args,
        description: "The files this node keeps (IPFS CIDs), with size and whether the local IPFS daemon still holds each one." },
    ToolDef { name: "request_status", title: "Request status", kind: ToolKind::Read, input_schema: status_schema,
        description: "The state of a write request this client made: pending member approval, approved (with its result), rejected, failed, or expired." },
    ToolDef { name: "tx_propose", title: "Propose a transaction", kind: ToolKind::Signature, input_schema: tx_schema,
        description: "Propose a transaction from this member's wallet. It opens a signature request in Citrate Core; nothing is signed or sent unless the member approves it there. Returns a request id for request_status." },
    ToolDef { name: "deploy_propose", title: "Propose a contract deploy", kind: ToolKind::Signature, input_schema: deploy_schema,
        description: "Propose deploying compiled contract bytecode from this member's wallet. Refused at once unless the deploy gate (tests, static analysis, fuzzing, fork dry run) is READY for exactly these bytes. Otherwise it opens a signature request in Citrate Core; nothing is signed or sent unless the member approves it there. Returns a request id for request_status." },
    ToolDef { name: "pin_add", title: "Keep a file on this node", kind: ToolKind::Action, input_schema: pin_schema,
        description: "Ask to pin an IPFS CID on this node (a local pin: no storage bond and no transaction). Runs only after the member approves it in Citrate Core." },
    ToolDef { name: "anchor_propose", title: "Prepare anchor approvals", kind: ToolKind::Action, input_schema: no_args,
        description: "Ask to run the decision-record anchor pass now instead of waiting for the nightly schedule. After the member approves this request, each closed day still gets its own approval card before anything is signed. Refused at once while anchoring is off or AnchorRegistry is not in this build's address book." },
    ToolDef { name: "cluster_join", title: "Join a cluster", kind: ToolKind::Action, input_schema: group_schema,
        description: "Ask to join this node to a group's cluster mesh. Runs only after the member approves it in Citrate Core." },
    ToolDef { name: "cluster_share", title: "Share a file with a cluster", kind: ToolKind::Action, input_schema: share_schema,
        description: "Ask to announce a pinned file (CID) to a group's cluster. Runs only after the member approves it in Citrate Core." },
    ToolDef { name: "invite_create", title: "Create an invite", kind: ToolKind::Action, input_schema: invite_create_schema,
        description: "Ask to create a one-time invite link for a group. Runs only after the member approves it in Citrate Core; the approved result carries the link." },
    ToolDef { name: "invite_revoke", title: "Revoke an invite", kind: ToolKind::Action, input_schema: invite_revoke_schema,
        description: "Ask to revoke an outstanding invite. Runs only after the member approves it in Citrate Core." },
    ToolDef { name: "faucet_request", title: "Faucet top-up for deploy gas", kind: ToolKind::Budgeted, input_schema: faucet_schema,
        description: "Ask the Citrate faucet for SALT to pay the gas of a deploy the member started (the deploy gate must be READY for this init code). Always for the member's own wallet; only when the balance is short; at most once per 24 hours; only if the member turned the in-app faucet on in Settings. The answer says plainly what happened, including when nothing was sent." },
];

/// Look a tool up by name.
pub fn tool(name: &str) -> Option<&'static ToolDef> {
    all_tools().find(|t| t.name == name)
}

/// Every tool `tools/list` returns: the node catalog, then the Hermes session tools (HUP-S1.1).
pub fn all_tools() -> impl Iterator<Item = &'static ToolDef> {
    TOOLS
        .iter()
        .chain(crate::node_mcp_hermes::HERMES_TOOLS.iter())
}

/// The MCP `tools/list` entry for a tool, with its annotations.
pub fn tool_json(t: &ToolDef) -> Value {
    let annotations = match t.kind {
        ToolKind::Read => json!({
            "title": t.title,
            "readOnlyHint": true,
            "openWorldHint": false,
        }),
        ToolKind::Signature | ToolKind::Action => json!({
            "title": t.title,
            "readOnlyHint": false,
            "destructiveHint": true,
            "idempotentHint": false,
            "openWorldHint": true,
        }),
        ToolKind::Budgeted => json!({
            "title": t.title,
            "readOnlyHint": false,
            "destructiveHint": false,
            "idempotentHint": false,
            "openWorldHint": true,
        }),
    };
    json!({
        "name": t.name,
        "title": t.title,
        "description": t.description,
        "inputSchema": (t.input_schema)(),
        "annotations": annotations,
    })
}

// ---------------------------------------------------------------------------
// Precompiles (citrate-chain core/execution/src/precompiles/mod.rs, the always-on set).
// ---------------------------------------------------------------------------

/// One callable precompile.
#[derive(Debug, Clone, Copy)]
pub struct Precompile {
    pub short: u16,
    pub name: &'static str,
    pub family: &'static str,
    pub summary: &'static str,
}

/// The precompiles exposed read-only. The inference family (0x0100-0x0106) needs a hosted model
/// runtime and is not listed.
pub const PRECOMPILES: &[Precompile] = &[
    Precompile {
        short: 0x0107,
        name: "TENSOR_COMMIT",
        family: "verification",
        summary: "Poseidon commitment over a tensor",
    },
    Precompile {
        short: 0x0108,
        name: "INFERENCE_PROOF_VERIFY",
        family: "verification",
        summary: "Halo2-KZG inference proof verifier",
    },
    Precompile {
        short: 0x0109,
        name: "MERKLE_VERIFY_TENSOR",
        family: "verification",
        summary: "Merkle path check for a tensor chunk",
    },
    Precompile {
        short: 0x0110,
        name: "BELNAP_AGGREGATE",
        family: "learning",
        summary: "Belnap FOUR aggregation (Q16)",
    },
    Precompile {
        short: 0x0111,
        name: "ROUTING_INFERENCE",
        family: "learning",
        summary: "routing-model inference (Q16)",
    },
    Precompile {
        short: 0x0120,
        name: "ED25519_VERIFY",
        family: "crypto",
        summary: "Ed25519 signature verification",
    },
    Precompile {
        short: 0x0130,
        name: "FOLD_COMMD_VERIFY",
        family: "verification",
        summary: "recursive-fold CommD proof verifier (feature-gated on chain)",
    },
    Precompile {
        short: 0x0200,
        name: "EIP712_VERIFY",
        family: "x402",
        summary: "EIP-712 signature check",
    },
    Precompile {
        short: 0x0201,
        name: "TRANSFER_AUTH_VERIFY",
        family: "x402",
        summary: "transfer-authorization check",
    },
    Precompile {
        short: 0x0202,
        name: "BATCH_PAYMENT_VERIFY",
        family: "x402",
        summary: "batch payment check",
    },
];

/// The 20-byte address form of a precompile short id (`0x0120` → `0x00…0120`).
pub fn precompile_address(short: u16) -> String {
    format!("0x{:040x}", short)
}

/// The precompile table as JSON.
pub fn precompile_table() -> Value {
    json!({
        "precompiles": PRECOMPILES.iter().map(|p| json!({
            "address": precompile_address(p.short),
            "short": format!("0x{:04x}", p.short),
            "name": p.name,
            "family": p.family,
            "summary": p.summary,
        })).collect::<Vec<_>>(),
        "note": "Called read-only with eth_call. Results depend on the node's chain rules; a precompile that is not active returns an error.",
        "agent_fork": {
            "precompiles": crate::agent_precompiles::FORK_PRECOMPILES.iter().map(|(short, name, summary)| json!({
                "address": precompile_address(*short),
                "short": format!("0x{short:04x}"),
                "name": name,
                "summary": summary,
            })).collect::<Vec<_>>(),
            "note": crate::agent_precompiles::FORK_NOTE,
            "active_from_genesis": crate::agent_precompiles::ACTIVE_FROM_GENESIS,
            "tools": ["agent_precompile_encode", "agent_precompile_decode"],
        },
    })
}

// ---------------------------------------------------------------------------
// Argument validation.
// ---------------------------------------------------------------------------

/// Largest calldata accepted (hex chars after `0x`): 64 KiB of bytes.
pub const MAX_DATA_HEX: usize = 128 * 1024;
/// Widest `get_logs` block range.
pub const MAX_LOG_RANGE: u64 = 5_000;
/// Longest memory query.
pub const MAX_QUERY_CHARS: usize = 512;

/// A required string argument.
pub fn arg_str<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("missing string argument `{key}`"))
}

/// An optional string argument (absent or null → None; any other non-string → error).
pub fn arg_opt_str<'a>(args: &'a Value, key: &str) -> Result<Option<&'a str>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.as_str())),
        Some(_) => Err(format!("argument `{key}` must be a string")),
    }
}

/// Reject arguments the schema does not name (schemas set `additionalProperties: false`).
pub fn reject_unknown(args: &Value, schema: &Value) -> Result<(), String> {
    let Some(map) = args.as_object() else {
        return Err("arguments must be a JSON object".to_string());
    };
    let props = schema.get("properties").and_then(Value::as_object);
    for k in map.keys() {
        if !props.is_some_and(|p| p.contains_key(k)) {
            return Err(format!("unknown argument `{k}`"));
        }
    }
    Ok(())
}

/// A `0x` 20-byte address, lowercased.
pub fn parse_address(s: &str) -> Result<String, String> {
    let h = s
        .strip_prefix("0x")
        .ok_or_else(|| format!("not a 0x address: {s}"))?;
    if h.len() != 40 || !h.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!("not a 20-byte 0x address: {s}"));
    }
    Ok(format!("0x{}", h.to_ascii_lowercase()))
}

/// `0x` hex bytes (even length, bounded), lowercased.
pub fn parse_data(s: &str) -> Result<String, String> {
    let h = s
        .strip_prefix("0x")
        .ok_or_else(|| "data must be 0x-prefixed hex".to_string())?;
    if h.len() % 2 != 0 || !h.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("data must be an even number of hex characters".to_string());
    }
    if h.len() > MAX_DATA_HEX {
        return Err("data is larger than 64 KiB".to_string());
    }
    Ok(format!("0x{}", h.to_ascii_lowercase()))
}

/// The ED25519_VERIFY precompile's longest message (citrate-chain
/// `core/execution/src/precompiles/ed25519.rs`, `MAX_MESSAGE_LEN`): a longer one reads as invalid.
pub const ED25519_MAX_MESSAGE_BYTES: usize = 8 * 1024;

/// Fixed-length hex bytes (`0x` + exactly `2 * len` hex characters), lowercased.
pub fn parse_fixed_hex(s: &str, len: usize, what: &str) -> Result<String, String> {
    let h = s
        .strip_prefix("0x")
        .ok_or_else(|| format!("{what} must be 0x-prefixed hex"))?;
    if h.len() != len * 2 || !h.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!(
            "{what} must be exactly {len} bytes ({} hex characters)",
            len * 2
        ));
    }
    Ok(h.to_ascii_lowercase())
}

/// The ED25519_VERIFY (0x0120) input: `pubkey (32) || signature (64) || message`.
pub fn ed25519_verify_input(
    public_key: &str,
    signature: &str,
    message: &str,
) -> Result<String, String> {
    let pk = parse_fixed_hex(public_key, 32, "public_key")?;
    let sig = parse_fixed_hex(signature, 64, "signature")?;
    let msg = parse_data(message)?;
    if (msg.len() - 2) / 2 > ED25519_MAX_MESSAGE_BYTES {
        return Err(format!(
            "message is longer than the precompile accepts ({ED25519_MAX_MESSAGE_BYTES} bytes)"
        ));
    }
    Ok(format!("0x{pk}{sig}{}", &msg[2..]))
}

/// A decimal wei amount.
pub fn parse_wei(s: &str) -> Result<u128, String> {
    if s.is_empty() || !s.chars().all(|c| c.is_ascii_digit()) {
        return Err("value_wei must be a decimal integer string".to_string());
    }
    s.parse::<u128>()
        .map_err(|_| "value_wei is out of range".to_string())
}

/// A group id: 1..=128 visible ASCII characters, no spaces.
pub fn parse_group(s: &str) -> Result<String, String> {
    if s.is_empty() || s.len() > 128 || !s.chars().all(|c| c.is_ascii_graphic()) {
        return Err("group must be a group id from groups_list".to_string());
    }
    Ok(s.to_string())
}

/// An IPFS CID: 1..=128 alphanumeric characters.
pub fn parse_cid(s: &str) -> Result<String, String> {
    if s.is_empty() || s.len() > 128 || !s.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err("cid must be an IPFS CID".to_string());
    }
    Ok(s.to_string())
}

/// Characters that must never appear in text shown on an approval card: control characters and
/// the Unicode bidirectional overrides, isolates and marks (they can reorder what the member reads).
pub fn is_unsafe_display_char(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}'
        )
}

/// A short free-text label (invite `for_handle`): 1..=64 chars, no control or bidi characters.
pub fn parse_label(s: &str, what: &str) -> Result<String, String> {
    let t = s.trim();
    if t.is_empty() || t.chars().count() > 64 || t.chars().any(is_unsafe_display_char) {
        return Err(format!(
            "{what} must be 1 to 64 characters with no control characters"
        ));
    }
    Ok(t.to_string())
}

/// A `0x` 32-byte topic.
pub fn parse_topic(s: &str) -> Result<String, String> {
    let h = s
        .strip_prefix("0x")
        .ok_or_else(|| "topics must be 0x-prefixed".to_string())?;
    if h.len() != 64 || !h.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("each topic must be 32 bytes of hex".to_string());
    }
    Ok(format!("0x{}", h.to_ascii_lowercase()))
}

/// Find the precompile at a 20-byte address, if it is in the table.
pub fn precompile_at(address: &str) -> Option<&'static Precompile> {
    PRECOMPILES
        .iter()
        .find(|p| precompile_address(p.short) == address)
}
