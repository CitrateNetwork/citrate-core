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

fn status_schema() -> Value {
    obj(
        json!({"id": {"type": "string", "description": "request id returned by a write tool"}}),
        &["id"],
    )
}

/// The memory tenants an MCP client may search. The member's `personal` tenant is deliberately
/// absent (pending owner sign-off on a per-token scope for personal memory).
pub const MEMORY_TENANTS: &[&str] = &["citrate-docs", "chain-state"];

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
        description: "Call a Citrate precompile read-only (eth_call) and return its output bytes. Only addresses in the precompile table are accepted." },
    ToolDef { name: "wallet_info", title: "Wallet (public)", kind: ToolKind::Read, input_schema: no_args,
        description: "This member's public wallet address and SALT balance. Never returns key material." },
    ToolDef { name: "address_book", title: "Address book", kind: ToolKind::Read, input_schema: no_args,
        description: "The deployed 40204 contract addresses this build was shipped with. A registry missing here is not deployed yet." },
    ToolDef { name: "memory_search", title: "Memory search", kind: ToolKind::Read, input_schema: memory_schema,
        description: "Search the node's shared knowledge graphs (Citrate docs and chain-state facts). The member's personal memory is not available over MCP." },
    ToolDef { name: "groups_list", title: "Groups", kind: ToolKind::Read, input_schema: no_args,
        description: "The groups this member belongs to (id and name)." },
    ToolDef { name: "cluster_status", title: "Cluster status", kind: ToolKind::Read, input_schema: group_schema,
        description: "A group's cluster: peers online, total peers, and files shared over the mesh." },
    ToolDef { name: "cluster_peers", title: "Cluster peers", kind: ToolKind::Read, input_schema: group_schema,
        description: "The group's authorized cluster peers and whether each is connected." },
    ToolDef { name: "invites_list", title: "Invites", kind: ToolKind::Read, input_schema: group_schema,
        description: "Outstanding invites this member created for a group (id, who it is for, when). Links and tokens are not returned." },
    ToolDef { name: "request_status", title: "Request status", kind: ToolKind::Read, input_schema: status_schema,
        description: "The state of a write request this client made: pending member approval, approved (with its result), rejected, failed, or expired." },
    ToolDef { name: "tx_propose", title: "Propose a transaction", kind: ToolKind::Signature, input_schema: tx_schema,
        description: "Propose a transaction from this member's wallet. It opens a signature request in Citrate Core; nothing is signed or sent unless the member approves it there. Returns a request id for request_status." },
    ToolDef { name: "cluster_join", title: "Join a cluster", kind: ToolKind::Action, input_schema: group_schema,
        description: "Ask to join this node to a group's cluster mesh. Runs only after the member approves it in Citrate Core." },
    ToolDef { name: "cluster_share", title: "Share a file with a cluster", kind: ToolKind::Action, input_schema: share_schema,
        description: "Ask to announce a pinned file (CID) to a group's cluster. Runs only after the member approves it in Citrate Core." },
    ToolDef { name: "invite_create", title: "Create an invite", kind: ToolKind::Action, input_schema: invite_create_schema,
        description: "Ask to create a one-time invite link for a group. Runs only after the member approves it in Citrate Core; the approved result carries the link." },
    ToolDef { name: "invite_revoke", title: "Revoke an invite", kind: ToolKind::Action, input_schema: invite_revoke_schema,
        description: "Ask to revoke an outstanding invite. Runs only after the member approves it in Citrate Core." },
];

/// Look a tool up by name.
pub fn tool(name: &str) -> Option<&'static ToolDef> {
    TOOLS.iter().find(|t| t.name == name)
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

/// A short free-text label (invite `for_handle`): 1..=64 chars, no control characters.
pub fn parse_label(s: &str, what: &str) -> Result<String, String> {
    let t = s.trim();
    if t.is_empty() || t.chars().count() > 64 || t.chars().any(|c| c.is_control()) {
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
