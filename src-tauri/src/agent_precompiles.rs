//! HUP-S7.2 follow-up (US-7.5 AC1): typed encoders and decoders for the agent precompile fork.
//!
//! The fork adds four precompiles to citrate-chain (spec: citrate-chain
//! `docs/precompiles/AGENT_PRECOMPILES.md`, ADR `ADR-2026-10-01-agent-precompiles`):
//!
//! | Address  | Name                   | Input                                         | Output              |
//! |----------|------------------------|-----------------------------------------------|---------------------|
//! | `0x0112` | `LORA_APPLY`           | `W || B || A || alpha`, Q16.16 tensors        | tensor `[d, k]`     |
//! | `0x0113` | `LORA_MERGE`           | `n || n x (B_i || A_i || alpha_i || w_i)`     | tensor `[d, k]`     |
//! | `0x0121` | `MEMORY_ANCHOR_VERIFY` | packed nightly-anchor inclusion proof         | day commitment or 0 |
//! | `0x0122` | `AGENT_OPS`            | `op || body` (DeviceLink, DeviceRevocation)   | one word, 1 or 0    |
//!
//! This module builds those native inputs from the shapes core already holds (the sidecar's
//! [`AnchorProof`], the cluster's [`DeviceLinkWire`] / [`RevocationWire`], Q16.16 tensors) and
//! reads the outputs back. It is pure: no RPC, no key, no signing. It refuses an input the
//! chain would refuse (wrong shapes, caps, lengths) before anyone builds a call around it.
//!
//! Where the calls happen. A top-level `eth_call` whose `to` is a precompile returns `0x` on a
//! Citrate node at every height, so these inputs are for contract code: the chain's
//! `CitratePrecompiles` library (`loraApply`, `loraMerge`, `memoryAnchorCommitment`,
//! `deviceLinkValid`, `deviceRevocationValid`), a generated contract, or the devnet check
//! (citrate-chain `scripts/devnet-precompile-check.sh`). On chain 40204 the four addresses are
//! active from genesis: the 2026-10-05 reroll pins the agent precompile height to 0 in the node
//! binary (owner decision 2026-10-04), so the app applies no activation gate of its own. A node
//! built before the reroll does not have them, and there every library call reverts with
//! `PrecompileUnavailable`.
//!
//! The encodings are pinned against the chain's own Rust encoders and precompile answers by the
//! vectors citrate-chain generates (`core/execution/tests/agent_precompile_vectors.rs`), copied to
//! `tests/fixtures/precompiles/agent_precompile_vectors.json`.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::anchor_proof::{self, AnchorProof};
use crate::device_link::{DeviceLinkBody, DeviceLinkWire, RevocationWire};

/// `0x0112 LORA_APPLY`.
pub const LORA_APPLY: u16 = 0x0112;
/// `0x0113 LORA_MERGE`.
pub const LORA_MERGE: u16 = 0x0113;
/// `0x0121 MEMORY_ANCHOR_VERIFY`.
pub const MEMORY_ANCHOR_VERIFY: u16 = 0x0121;
/// `0x0122 AGENT_OPS`.
pub const AGENT_OPS: u16 = 0x0122;

/// `AGENT_OPS` operation byte: verify a DeviceLink.
pub const OP_DEVICE_LINK_VERIFY: u8 = 0x01;
/// `AGENT_OPS` operation byte: verify a DeviceRevocation.
pub const OP_DEVICE_REVOCATION_VERIFY: u8 = 0x02;

/// Output tile rows and columns are each at most 256 (citrate-chain `lora::caps::DIM_MAX`).
pub const LORA_DIM_MAX: u32 = 256;
/// LoRA rank is 1..=64 (`lora::caps::RANK_MAX`).
pub const LORA_RANK_MAX: u32 = 64;
/// `LORA_MERGE` takes 1..=16 adapters (`lora::caps::MERGE_MAX_ADAPTERS`).
pub const LORA_MERGE_MAX_ADAPTERS: usize = 16;
/// Longest anchor audit path (`memory_anchor::MAX_PATH`).
pub const ANCHOR_MAX_PATH: usize = anchor_proof::MAX_PATH;

/// The canonical tensor format's dtype byte for Q16.16 (8-byte little-endian i64 elements).
const DTYPE_Q16: u8 = 0x01;
/// Fixed part of a `MEMORY_ANCHOR_VERIFY` input before the path.
const ANCHOR_FIXED_LEN: usize = 4 + 8 + 8 + 8 + 8 + 32 + 8 + 8 + 32 + 1;
const SIG_LEN: usize = 65;

/// The four fork precompiles, for the precompile table and the tools.
pub const FORK_PRECOMPILES: &[(u16, &str, &str)] = &[
    (
        LORA_APPLY,
        "LORA_APPLY",
        "W + (alpha / r) (B . A) on one Q16.16 tile",
    ),
    (
        LORA_MERGE,
        "LORA_MERGE",
        "sum_i w_i (alpha_i / r_i) (B_i . A_i) on one Q16.16 tile",
    ),
    (
        MEMORY_ANCHOR_VERIFY,
        "MEMORY_ANCHOR_VERIFY",
        "nightly anchor inclusion proof to its day commitment (zero when invalid)",
    ),
    (
        AGENT_OPS,
        "AGENT_OPS",
        "DeviceLink (op 0x01) and DeviceRevocation (op 0x02) signature checks, 1 or 0",
    ),
];

/// What a caller should know before relying on any of these inputs.
pub const FORK_NOTE: &str = "Active from genesis on chain 40204 (the 2026-10-05 reroll pins the agent precompile height to 0). Reachable from contract code through citrate-chain CitratePrecompiles; a node built before the reroll does not have them and the library call reverts with PrecompileUnavailable. A top-level eth_call to the address returns 0x on a Citrate node, so do not call it directly.";

/// The agent precompiles are active from genesis on 40204 (reroll 2026-10-05).
pub const ACTIVE_FROM_GENESIS: bool = true;

/// The 20-byte address form of a short precompile id.
pub fn address(short: u16) -> String {
    crate::node_mcp_tools::precompile_address(short)
}

fn hex0x(bytes: &[u8]) -> String {
    format!("0x{}", hex::encode(bytes))
}

// ---------------------------------------------------------------------------------------------
// Q16.16 tensors in the canonical tensor format v1
// ---------------------------------------------------------------------------------------------

/// A Q16.16 tensor: `shape` (row-major) and raw fixed-point elements (`value * 65536`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Q16Tensor {
    pub shape: Vec<u32>,
    pub q16: Vec<i64>,
}

impl Q16Tensor {
    /// A rank-0 tensor holding one raw Q16.16 value.
    pub fn scalar(raw: i64) -> Self {
        Q16Tensor {
            shape: Vec::new(),
            q16: vec![raw],
        }
    }

    fn element_count(&self) -> Result<usize, String> {
        if self.shape.len() > 4 {
            return Err(format!(
                "a tensor has at most rank 4, got {}",
                self.shape.len()
            ));
        }
        self.shape.iter().try_fold(1usize, |n, d| {
            if *d == 0 {
                return Err("a tensor dimension must be at least 1".to_string());
            }
            n.checked_mul(*d as usize)
                .ok_or_else(|| "tensor element count overflows".to_string())
        })
    }

    /// The canonical v1 bytes: rank, big-endian u32 shape, dtype `0x01`, then each element as
    /// an 8-byte little-endian i64.
    pub fn encode(&self) -> Result<Vec<u8>, String> {
        let n = self.element_count()?;
        if n != self.q16.len() {
            return Err(format!(
                "shape {:?} holds {n} elements, got {}",
                self.shape,
                self.q16.len()
            ));
        }
        let mut out = Vec::with_capacity(2 + 4 * self.shape.len() + 8 * n);
        out.push(self.shape.len() as u8);
        for d in &self.shape {
            out.extend_from_slice(&d.to_be_bytes());
        }
        out.push(DTYPE_Q16);
        for v in &self.q16 {
            out.extend_from_slice(&v.to_le_bytes());
        }
        Ok(out)
    }

    /// Decode exactly one Q16.16 tensor (no trailing bytes).
    pub fn decode(bytes: &[u8]) -> Result<Self, String> {
        let (&rank, rest) = bytes.split_first().ok_or("empty tensor bytes")?;
        let rank = usize::from(rank);
        if rank > 4 {
            return Err(format!("a tensor has at most rank 4, got {rank}"));
        }
        let shape_bytes = rest.get(..4 * rank).ok_or("tensor shape is truncated")?;
        let shape: Vec<u32> = shape_bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| u32::from_be_bytes(*c))
            .collect();
        let rest = &rest[4 * rank..];
        let (&dtype, data) = rest.split_first().ok_or("tensor dtype is missing")?;
        if dtype != DTYPE_Q16 {
            return Err(format!(
                "expected a Q16.16 tensor (dtype 0x01), got 0x{dtype:02x}"
            ));
        }
        let t = Q16Tensor {
            shape,
            q16: Vec::new(),
        };
        let n = t.element_count()?;
        if data.len() != n.checked_mul(8).ok_or("tensor size overflows")? {
            return Err(format!(
                "tensor data is {} bytes, its shape needs {}",
                data.len(),
                n.saturating_mul(8)
            ));
        }
        let q16 = data
            .as_chunks::<8>()
            .0
            .iter()
            .map(|c| i64::from_le_bytes(*c))
            .collect();
        Ok(Q16Tensor { q16, ..t })
    }

    fn matrix(&self, what: &str) -> Result<(u32, u32), String> {
        match self.shape.as_slice() {
            [r, c] => Ok((*r, *c)),
            other => Err(format!(
                "{what} must be a matrix (rank 2), got rank {}",
                other.len()
            )),
        }
    }

    /// The elements as decimals (`raw / 65536`), for display only.
    pub fn values(&self) -> Vec<f64> {
        self.q16.iter().map(|v| *v as f64 / 65_536.0).collect()
    }
}

fn check_rank(r: u32, what: &str) -> Result<(), String> {
    if r == 0 || r > LORA_RANK_MAX {
        return Err(format!(
            "{what}: LoRA rank {r} is outside 1..={LORA_RANK_MAX}"
        ));
    }
    Ok(())
}

fn check_tile(d: u32, k: u32, what: &str) -> Result<(), String> {
    if d > LORA_DIM_MAX || k > LORA_DIM_MAX {
        return Err(format!(
            "{what}: tile {d}x{k} exceeds {LORA_DIM_MAX}x{LORA_DIM_MAX}; split it into tiles"
        ));
    }
    Ok(())
}

/// `0x0112 LORA_APPLY` input: `W [d, k] || B [d, r] || A [r, k] || alpha []`. Returns the input
/// and its scheduled gas.
pub fn lora_apply_input(
    w: &Q16Tensor,
    b: &Q16Tensor,
    a: &Q16Tensor,
    alpha: i64,
) -> Result<(Vec<u8>, u64), String> {
    let (d, k) = w.matrix("W")?;
    let (bd, r) = b.matrix("B")?;
    let (ar, ak) = a.matrix("A")?;
    check_tile(d, k, "LORA_APPLY")?;
    check_rank(r, "LORA_APPLY")?;
    if bd != d || ar != r || ak != k {
        return Err(format!(
            "LORA_APPLY: shape mismatch, W {d}x{k}, B {bd}x{r}, A {ar}x{ak} (B must be d x r and A r x k)"
        ));
    }
    let input = [
        w.encode()?,
        b.encode()?,
        a.encode()?,
        Q16Tensor::scalar(alpha).encode()?,
    ]
    .concat();
    Ok((input, lora_apply_gas(d, r, k)))
}

/// `LORA_APPLY` gas: `3000 + 4 d r k + 3 d k`.
pub fn lora_apply_gas(d: u32, r: u32, k: u32) -> u64 {
    let (d, r, k) = (u64::from(d), u64::from(r), u64::from(k));
    3_000 + 4 * d * r * k + 3 * d * k
}

/// One adapter of a `LORA_MERGE`: `B [d, r] || A [r, k] || alpha [] || weight []`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LoraAdapter {
    pub b: Q16Tensor,
    pub a: Q16Tensor,
    pub alpha: i64,
    pub weight: i64,
}

/// `0x0113 LORA_MERGE` input. Every adapter has the same `d` and `k`; ranks may differ.
/// Returns the input and its scheduled gas.
pub fn lora_merge_input(adapters: &[LoraAdapter]) -> Result<(Vec<u8>, u64), String> {
    if adapters.is_empty() || adapters.len() > LORA_MERGE_MAX_ADAPTERS {
        return Err(format!(
            "LORA_MERGE takes 1 to {LORA_MERGE_MAX_ADAPTERS} adapters, got {}",
            adapters.len()
        ));
    }
    let mut out = vec![adapters.len() as u8];
    let mut tile: Option<(u32, u32)> = None;
    let mut gas = 3_000u64;
    for (i, ad) in adapters.iter().enumerate() {
        let what = format!("LORA_MERGE adapter {i}");
        let (d, r) = ad.b.matrix(&format!("{what} B"))?;
        let (ar, k) = ad.a.matrix(&format!("{what} A"))?;
        check_tile(d, k, &what)?;
        check_rank(r, &what)?;
        if ar != r {
            return Err(format!("{what}: B is {d}x{r} but A is {ar}x{k}"));
        }
        if let Some((td, tk)) = tile {
            if (td, tk) != (d, k) {
                return Err(format!(
                    "{what}: tile {d}x{k} differs from adapter 0's {td}x{tk}"
                ));
            }
        }
        tile = Some((d, k));
        let (d64, r64, k64) = (u64::from(d), u64::from(r), u64::from(k));
        gas += 4 * d64 * r64 * k64 + 4 * d64 * k64;
        out.extend(ad.b.encode()?);
        out.extend(ad.a.encode()?);
        out.extend(Q16Tensor::scalar(ad.alpha).encode()?);
        out.extend(Q16Tensor::scalar(ad.weight).encode()?);
    }
    Ok((out, gas))
}

/// Read a `LORA_APPLY` / `LORA_MERGE` answer: one Q16.16 matrix.
pub fn decode_lora_output(bytes: &[u8]) -> Result<Q16Tensor, String> {
    let t = Q16Tensor::decode(bytes)?;
    t.matrix("the LoRA output")?;
    Ok(t)
}

// ---------------------------------------------------------------------------------------------
// 0x0121 MEMORY_ANCHOR_VERIFY
// ---------------------------------------------------------------------------------------------

/// `0x0121` input from the sidecar's proof shape: `be32 v || be64 day || be64 first_seq ||
/// be64 last_seq || be64 count || tree_root || be64 seq || be64 leaf_index || record_hash ||
/// u8 path_len || path`.
pub fn memory_anchor_input(p: &AnchorProof) -> Result<Vec<u8>, String> {
    if p.path.len() > ANCHOR_MAX_PATH {
        return Err(format!(
            "the audit path has {} hashes; the precompile takes at most {ANCHOR_MAX_PATH}",
            p.path.len()
        ));
    }
    let root =
        anchor_proof::hex32(&p.header.tree_root).ok_or("the tree root is not 32 bytes of hex")?;
    let record =
        anchor_proof::hex32(&p.record_hash).ok_or("the record hash is not 32 bytes of hex")?;
    let mut out = Vec::with_capacity(ANCHOR_FIXED_LEN + 32 * p.path.len());
    out.extend_from_slice(&p.header.v.to_be_bytes());
    out.extend_from_slice(&p.header.day.to_be_bytes());
    out.extend_from_slice(&p.header.first_seq.to_be_bytes());
    out.extend_from_slice(&p.header.last_seq.to_be_bytes());
    out.extend_from_slice(&p.header.count.to_be_bytes());
    out.extend_from_slice(&root);
    out.extend_from_slice(&p.seq.to_be_bytes());
    out.extend_from_slice(&p.leaf_index.to_be_bytes());
    out.extend_from_slice(&record);
    out.push(p.path.len() as u8);
    for h in &p.path {
        out.extend_from_slice(&anchor_proof::hex32(h).ok_or("a path hash is not 32 bytes of hex")?);
    }
    Ok(out)
}

/// `MEMORY_ANCHOR_VERIFY` gas: `1500 + 150 * path_len`.
pub fn memory_anchor_gas(path_len: usize) -> u64 {
    1_500 + 150 * path_len as u64
}

/// A `MEMORY_ANCHOR_VERIFY` answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnchorAnswer {
    /// The proof is valid; this is the day commitment to look up in `AnchorRegistry`.
    Commitment([u8; 32]),
    /// The precompile answered zero: the proof is not valid.
    Invalid,
}

/// Read a `MEMORY_ANCHOR_VERIFY` answer (exactly 32 bytes).
pub fn decode_memory_anchor_output(bytes: &[u8]) -> Result<AnchorAnswer, String> {
    let word: [u8; 32] = bytes.try_into().map_err(|_| {
        format!(
            "MEMORY_ANCHOR_VERIFY answers 32 bytes, got {} (empty means the precompile is not active)",
            bytes.len()
        )
    })?;
    Ok(if word == [0u8; 32] {
        AnchorAnswer::Invalid
    } else {
        AnchorAnswer::Commitment(word)
    })
}

// ---------------------------------------------------------------------------------------------
// 0x0122 AGENT_OPS
// ---------------------------------------------------------------------------------------------

fn bytes_n<const N: usize>(s: &str, what: &str) -> Result<[u8; N], String> {
    let h = s.strip_prefix("0x").unwrap_or(s);
    let mut out = [0u8; N];
    if h.len() != 2 * N {
        return Err(format!("{what} must be {N} bytes of hex"));
    }
    hex::decode_to_slice(h, &mut out).map_err(|_| format!("{what} must be {N} bytes of hex"))?;
    Ok(out)
}

fn message_gas(message_len: usize, signatures: u64) -> u64 {
    1_000 + 3_000 * signatures + 6 * (message_len as u64).div_ceil(32)
}

/// An encoded `AGENT_OPS` call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentOpsInput {
    /// The full precompile input (`op || body`).
    pub input: Vec<u8>,
    /// The scheduled gas.
    pub gas: u64,
    /// The exact text the signatures cover.
    pub message: String,
}

impl AgentOpsInput {
    /// The body without the operation byte: the argument of `CitratePrecompiles.deviceLinkValid`
    /// and `deviceRevocationValid`, which prefix the operation themselves.
    pub fn body(&self) -> &[u8] {
        &self.input[1..]
    }
}

/// `0x0122` DEVICE_LINK_VERIFY input from a stored link: `0x01 || member || device || wallet ||
/// be32 index || be64 issued_at || u8 label_len || label || member_sig || device_sig || wallet_sig`.
pub fn device_link_input(link: &DeviceLinkWire) -> Result<AgentOpsInput, String> {
    // The same validation and text as the link ceremony (and cluster-core); a link it refuses
    // is one the precompile answers 0 for.
    let body = DeviceLinkBody::new(
        &link.member,
        &link.device,
        &link.wallet,
        link.index,
        &link.label,
        link.issued_at,
    )
    .map_err(|e| format!("{e}; the precompile would answer 0"))?;
    let member = bytes_n::<20>(&body.member, "member")?;
    let device = bytes_n::<20>(&body.device, "device")?;
    let wallet = bytes_n::<20>(&body.wallet, "wallet")?;
    let ms = bytes_n::<SIG_LEN>(&link.member_sig, "member signature")?;
    let ds = bytes_n::<SIG_LEN>(&link.device_sig, "device signature")?;
    let ws = bytes_n::<SIG_LEN>(&link.wallet_sig, "wallet signature")?;
    let mut input = vec![OP_DEVICE_LINK_VERIFY];
    input.extend_from_slice(&member);
    input.extend_from_slice(&device);
    input.extend_from_slice(&wallet);
    input.extend_from_slice(&link.index.to_be_bytes());
    input.extend_from_slice(&link.issued_at.to_be_bytes());
    input.push(link.label.len() as u8);
    input.extend_from_slice(link.label.as_bytes());
    input.extend_from_slice(&ms);
    input.extend_from_slice(&ds);
    input.extend_from_slice(&ws);
    let message = body.signing_message();
    Ok(AgentOpsInput {
        input,
        gas: message_gas(message.len(), 3),
        message,
    })
}

/// `0x0122` DEVICE_REVOCATION_VERIFY input: `0x02 || member || device || be64 revoked_at ||
/// member_sig`.
pub fn device_revocation_input(rev: &RevocationWire) -> Result<AgentOpsInput, String> {
    let member = bytes_n::<20>(&rev.member, "member")?;
    let device = bytes_n::<20>(&rev.device, "device")?;
    let ms = bytes_n::<SIG_LEN>(&rev.member_sig, "member signature")?;
    let mut input = vec![OP_DEVICE_REVOCATION_VERIFY];
    input.extend_from_slice(&member);
    input.extend_from_slice(&device);
    input.extend_from_slice(&rev.revoked_at.to_be_bytes());
    input.extend_from_slice(&ms);
    let message = crate::device_link::revocation_message(
        &hex::encode(member),
        &hex::encode(device),
        rev.revoked_at,
    );
    Ok(AgentOpsInput {
        input,
        gas: message_gas(message.len(), 1),
        message,
    })
}

/// Read an `AGENT_OPS` answer: one word, exactly 1 or 0.
pub fn decode_agent_ops_output(bytes: &[u8]) -> Result<bool, String> {
    let word: [u8; 32] = bytes.try_into().map_err(|_| {
        format!(
            "AGENT_OPS answers 32 bytes, got {} (empty means the precompile is not active)",
            bytes.len()
        )
    })?;
    if word[..31].iter().any(|b| *b != 0) || word[31] > 1 {
        return Err("AGENT_OPS answered a word that is neither 1 nor 0".into());
    }
    Ok(word[31] == 1)
}

// ---------------------------------------------------------------------------------------------
// JSON entry points (the node MCP tools `agent_precompile_encode` / `agent_precompile_decode`)
// ---------------------------------------------------------------------------------------------

/// The operations the tools accept.
pub const OPERATIONS: &[&str] = &[
    "LORA_APPLY",
    "LORA_MERGE",
    "MEMORY_ANCHOR_VERIFY",
    "DEVICE_LINK_VERIFY",
    "DEVICE_REVOCATION_VERIFY",
];

fn parse<T: for<'de> Deserialize<'de>>(v: &Value, what: &str) -> Result<T, String> {
    serde_json::from_value(v.clone()).map_err(|e| format!("{what}: {e}"))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ApplyArgs {
    w: Q16Tensor,
    b: Q16Tensor,
    a: Q16Tensor,
    alpha: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MergeArgs {
    adapters: Vec<LoraAdapter>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AnchorArgs {
    proof: AnchorProof,
}

/// Encode `args` for `operation`; the answer names the address, the native input, the
/// Solidity helper that takes it, and the scheduled gas.
pub fn encode_json(operation: &str, args: &Value) -> Result<Value, String> {
    let (short, input, helper, gas, extra) = match operation {
        "LORA_APPLY" => {
            let a: ApplyArgs = parse(args, "LORA_APPLY arguments {w, b, a, alpha}")?;
            let (input, gas) = lora_apply_input(&a.w, &a.b, &a.a, a.alpha)?;
            let parts = json!({
                "w": hex0x(&a.w.encode()?),
                "b": hex0x(&a.b.encode()?),
                "a": hex0x(&a.a.encode()?),
                "alpha": hex0x(&Q16Tensor::scalar(a.alpha).encode()?),
            });
            let helper = "CitratePrecompiles.loraApply(w, b, a, alpha) with the four encoded tensors in `tensors` (the input is their concatenation)";
            (LORA_APPLY, input, helper, gas, json!({"tensors": parts}))
        }
        "LORA_MERGE" => {
            let a: MergeArgs = parse(
                args,
                "LORA_MERGE arguments {adapters: [{b, a, alpha, weight}]}",
            )?;
            let (input, gas) = lora_merge_input(&a.adapters)?;
            (
                LORA_MERGE,
                input,
                "CitratePrecompiles.loraMerge(input)",
                gas,
                json!({}),
            )
        }
        "MEMORY_ANCHOR_VERIFY" => {
            let a: AnchorArgs = parse(args, "MEMORY_ANCHOR_VERIFY arguments {proof}")?;
            let input = memory_anchor_input(&a.proof)?;
            // The answer the chain will give, computed here so a caller can compare.
            let expected = match anchor_proof::check_inclusion(&a.proof) {
                Ok(c) => json!({"valid": true, "commitment": hex0x(&c)}),
                Err(why) => json!({"valid": false, "why": why}),
            };
            let gas = memory_anchor_gas(a.proof.path.len());
            let helper = "CitratePrecompiles.memoryAnchorCommitment(input), or AnchorProofs.isRecordAnchored(registry, committer, input)";
            (
                MEMORY_ANCHOR_VERIFY,
                input,
                helper,
                gas,
                json!({"expected": expected}),
            )
        }
        "DEVICE_LINK_VERIFY" => {
            let link: DeviceLinkWire =
                parse(args, "DEVICE_LINK_VERIFY arguments (a stored device link)")?;
            let e = device_link_input(&link)?;
            let extra = json!({"body": hex0x(e.body()), "message": e.message});
            (
                AGENT_OPS,
                e.input,
                "CitratePrecompiles.deviceLinkValid(body)",
                e.gas,
                extra,
            )
        }
        "DEVICE_REVOCATION_VERIFY" => {
            let rev: RevocationWire = parse(
                args,
                "DEVICE_REVOCATION_VERIFY arguments (a stored revocation)",
            )?;
            let e = device_revocation_input(&rev)?;
            let extra = json!({"body": hex0x(e.body()), "message": e.message});
            (
                AGENT_OPS,
                e.input,
                "CitratePrecompiles.deviceRevocationValid(body)",
                e.gas,
                extra,
            )
        }
        other => {
            return Err(format!(
                "unknown operation {other:?}; one of {OPERATIONS:?}"
            ))
        }
    };
    let mut out = json!({
        "operation": operation,
        "address": address(short),
        "input": hex0x(&input),
        "gas": gas,
        "solidity": helper,
        "note": FORK_NOTE,
    });
    if let (Some(o), Some(x)) = (out.as_object_mut(), extra.as_object()) {
        for (k, v) in x {
            o.insert(k.clone(), v.clone());
        }
    }
    Ok(out)
}

/// Decode a precompile answer (`0x` hex) for `operation`.
pub fn decode_json(operation: &str, output: &str) -> Result<Value, String> {
    let h = output
        .strip_prefix("0x")
        .ok_or("output must be 0x-prefixed hex")?;
    let bytes = hex::decode(h)
        .map_err(|_| "output must be an even number of hex characters".to_string())?;
    match operation {
        "LORA_APPLY" | "LORA_MERGE" => {
            let t = decode_lora_output(&bytes)?;
            let values = t.values();
            Ok(json!({"operation": operation, "shape": t.shape, "q16": t.q16, "values": values}))
        }
        "MEMORY_ANCHOR_VERIFY" => Ok(match decode_memory_anchor_output(&bytes)? {
            AnchorAnswer::Commitment(c) => {
                json!({"operation": operation, "valid": true, "commitment": hex0x(&c)})
            }
            AnchorAnswer::Invalid => json!({"operation": operation, "valid": false}),
        }),
        "DEVICE_LINK_VERIFY" | "DEVICE_REVOCATION_VERIFY" => {
            Ok(json!({"operation": operation, "valid": decode_agent_ops_output(&bytes)?}))
        }
        other => Err(format!(
            "unknown operation {other:?}; one of {OPERATIONS:?}"
        )),
    }
}

#[cfg(test)]
mod tests {
    include!("agent_precompiles_tests.rs");
}
