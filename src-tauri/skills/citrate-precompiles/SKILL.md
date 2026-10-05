---
name: citrate-precompiles
description: The Citrate precompile map for chain 40204 with addresses, what each one does, gas formulas, input and output shapes, which ones contract code can actually reach today, and the agent precompiles (LoRA, memory anchor, agent ops) that are active from genesis on 40204. Use when someone asks what a Citrate precompile does, which address to call, what it costs, how to call one from Solidity or from Hermes, or why a call returned empty data. Cites the citrate-chain source for every row.
license: Apache-2.0
metadata:
  created: 2026-10-01
  branch: hup/n3-faucet-adr-literacy
  author: Larry Klosowski + Claude Opus 5.5
  status: active
  updated: 2026-10-04 on hup/reroll-agentsbt-member-mint (agent precompiles active from genesis, reroll 2026-10-05)
  wp: HUP-S7.7
  citrate-chain: 3a6e45398904bbb3aff0fed46e26c22171aa30b1
  citrate-docs: 73ea7c56bc2087058ecc58eae5dbf734c86da072
---

# Citrate precompiles

A precompile is an address that runs native node code instead of EVM bytecode. Citrate keeps the
nine standard Ethereum precompiles at `0x01` to `0x09` (ECRECOVER through BLAKE2F) and adds its own
pages above them. Addresses are written by their short name: `0x0110` means
`0x0000000000000000000000000000000000000110`, which is `address(0x0110)` in Solidity. The address
book `citrate-chain:contracts/addresses/40204.json#BelnapAggregate` lists them under
`precompiles`.

The authority is the source at the pinned commit:
`citrate-chain:core/execution/src/precompiles/mod.rs#pub fn execute_pure_at` routes the
"pure" families, and `citrate-chain:core/execution/src/precompiles/mod.rs#pub const PURE_PRECOMPILE_ADDRESSES`
is the exact set the EVM can call. The docs pages
(`citrate-docs:content/chain/precompiles.md`, `citrate-docs:content/chain/precompiles-zkp.md`)
are summaries; where they differ from the code, the code wins.

## What contract code can reach

The EVM bridge registers only the 16 addresses in `PURE_PRECOMPILE_ADDRESSES`
(`citrate-chain:core/execution/src/revm_adapter.rs#fn register_citrate_precompiles`). Everything
else in the Citrate ranges, notably the hosted-inference family `0x0100` to `0x0106`, is **not
bridged**: contract code gets no result from them. Depending on the node build, a call to one of
these reserved addresses either fails or succeeds with **empty data**, like a call to an empty
account (`citrate-chain:core/execution/src/precompiles/mod.rs#pub fn reserved_unbridged_addresses`).
Either way: **always check `ok` and the length of the return data**. Never treat empty data as a
result.

## The map

| Address | Name | What it does | Gas | Reachable from EVM |
|---|---|---|---|---|
| `0x0100` to `0x0106` | ModelDeploy, ModelInference, BatchInference, ModelMetadata, (`0x0104` retired), ModelBenchmark, ModelEncryption | hosted model runtime; inference returns a signed receipt gated by hardware attestation, not a proof | per call, `citrate-chain:core/execution/src/precompiles/inference.rs#INFERENCE_BASE` | no (not bridged) |
| `0x0107` | TENSOR_COMMIT | Poseidon commitment over a canonical-format tensor, 32-byte field element | `3000 + 30` per 32-byte word | yes |
| `0x0108` | INFERENCE_PROOF_VERIFY | Halo2-KZG inference-proof verifier, 32-byte boolean | `500000 + 50` per byte; needs the `halo2-substrate` build, otherwise a "feature absent" error | yes (address) |
| `0x0109` | MERKLE_VERIFY_TENSOR | proves a tensor element is in a committed tensor, 32-byte boolean | `3000 + 200` per level, depth at most 32 | yes |
| `0x010A` | TENSOR_MATMUL_Q16 | Q16.16 matrix multiply | `5000 + 4` per multiply-add | yes |
| `0x010B` | TENSOR_DOT_Q16 | Q16.16 dot product | `2000 + 4` per element | yes |
| `0x010C` | TENSOR_SOFTMAX_Q16 | Q16.16 softmax | `5000 + 30` per element | yes |
| `0x010D` | TENSOR_RELU_Q16 | Q16.16 ReLU | `1000 + 1` per element | yes |
| `0x010E` | TENSOR_LINEAR_Q16 | Q16.16 linear layer | `5000 + 4` per multiply-add `+ 1` per bias | yes |
| `0x010F` | TENSOR_TRANSPOSE_Q16 | Q16.16 transpose | `1000 + 1` per element | yes |
| `0x0110` | BELNAP_AGGREGATE | Belnap-FOUR aggregation: Q16 values plus one state per dimension | `2000 + 50 * dim`, times `n` from an activation height | yes |
| `0x0111` | ROUTING_INFERENCE | fixed 768-128-3 Q16 MLP forward pass, returns mentor id, adapter id, confidence | `5000 + 4` per parameter (465,324 at the canonical shape) | yes |
| `0x0120` | ED25519_VERIFY | strict RFC 8032 Ed25519 verify, 32-byte 0/1 word | flat 2000, message at most 8 KiB | yes |
| `0x0112` | LORA_APPLY | `W + (alpha / r) (B . A)` on one Q16.16 tile, returns the tile | `3000 + 4 d r k + 3 d k` | yes, from genesis |
| `0x0113` | LORA_MERGE | `sum_i w_i (alpha_i / r_i) (B_i . A_i)` on one tile, up to 16 adapters | `3000 + sum_i (4 d r_i k + 4 d k)` | yes, from genesis |
| `0x0121` | MEMORY_ANCHOR_VERIFY | nightly decision-anchor inclusion proof, returns the day commitment or 32 zero bytes | `1500 + 150` per path hash | yes, from genesis |
| `0x0122` | AGENT_OPS | op `0x01` DeviceLink check (3 signatures), op `0x02` DeviceRevocation check, returns a 0/1 word | `1000 + 3000` per signature `+ 6` per message word | yes, from genesis |
| `0x0130` | FOLD_COMMD_VERIFY | recursive-fold CommD proof verifier | `2000000 + 50` per byte | yes (address); feature-gated, not consensus-active |
| `0x0200` | EIP712_VERIFY | recovers the signer of EIP-712 typed data | 3450 | yes |
| `0x0201` | TRANSFER_AUTH_VERIFY | checks an EIP-3009 TransferWithAuthorization signature, signer must equal `from` | 4200 | yes |
| `0x0202` | BATCH_PAYMENT_VERIFY | batch of TransferWithAuthorization checks | `2000 + 3800` per payment | yes |

Sources for the gas figures: `citrate-chain:core/execution/src/precompiles/verify.rs#pub mod gas_costs`,
`citrate-chain:core/execution/src/precompiles/compute.rs#pub mod gas_costs`,
`citrate-chain:core/execution/src/precompiles/q16/belnap.rs#pub fn gas_for`,
`citrate-chain:core/execution/src/precompiles/q16/routing.rs#GAS_PER_PARAM`,
`citrate-chain:core/execution/src/precompiles/ed25519.rs#pub const ED25519_VERIFY_GAS`,
`citrate-chain:core/execution/src/precompiles/commd_fold_verify.rs#FOLD_VERIFY_BASE` and
`citrate-chain:core/execution/src/precompiles/x402.rs#pub mod gas_costs`. Hardened-mode changes
for `0x0109` and `0x0110` are listed in `execute_pure_at`. The fork rows come from
`citrate-chain:docs/precompiles/AGENT_PRECOMPILES.md` (sections 2 to 5), which is the one
specification for them.

Docs drift to know about: the docs page calls `0x0111` future work, while the code marks its
forward pass live and the EVM bridge includes it; its Halo2 circuit is deferred, so `0x0111`
results are deterministic but come with no ZK proof. The docs also describe the `0x0110` output
as per-dimension value-then-state; the code emits all values first, then all states.

## Families in one paragraph each

**Verification (`0x0107` to `0x0109`).** Commit to a tensor, verify an inference proof, verify a
Merkle path into a committed tensor. Deterministic, and their byte output is frozen: drift would
fork the chain and invalidate earlier commitments. Inputs use the frozen canonical tensor format
version 1: rank byte (0 to 4), rank x u32 big-endian shape, dtype byte, then the data. Element byte
order depends on the dtype: Q16.16 (`0x01`) elements are 8-byte **little-endian** i64, Field32 (`0x02`)
elements are 32-byte big-endian
(`citrate-chain:core/execution/src/precompiles/tensor_format.rs#Q16_16`).

**Deterministic compute (`0x010A` to `0x010F`).** Six Q16.16 fixed-point tensor primitives in
integer arithmetic, so every node computes the same bits. Overflow saturates instead of failing,
so a saturated result is a clamped value, not an error; check ranges if it matters. Size caps such
as `MATMUL_DIM_MAX` 256 apply (`citrate-chain:core/execution/src/precompiles/compute.rs#MATMUL_DIM_MAX`).

**Learning (`0x0110`, `0x0111`).** Belnap aggregation for federated learning, and the routing
model forward pass. See the `citrate-belnap-aggregate` skill for `0x0110` byte layouts and a worked
example, and `citrate-paraconsensus` for the logic.

**Crypto (`0x0120`).** Ed25519 verification, the scheme the consensus layer and most agent
identities use. Input is `pubkey (32) || signature (64) || message`. Malformed input returns the
all-zero word with success, so check for exactly `0x...01`
(`citrate-chain:core/execution/src/precompiles/ed25519.rs#MIN_INPUT_LEN`).

**x402 payments (`0x0200` to `0x0202`).** ecrecover plus keccak for x402 payment checks, about nine
times cheaper than the same check in Solidity per the source. On a bad signature they return the
zero address instead of reverting, so a caller that skips the check treats a failed verification
as an unknown signer.

**Agent precompile fork (`0x0112`, `0x0113`, `0x0121`, `0x0122`).** Four pure byte functions for
agents: LoRA arithmetic on one tile (so a challenger can recompute one disputed tile of a federated
aggregate), the on-chain twin of the nightly decision-anchor proof check, and DeviceLink /
DeviceRevocation signature checks. On 40204 they are **active from genesis**: the 2026-10-05 reroll
pins the activation height to 0 (`citrate-chain:core/execution/src/agent_fork.rs#AGENT_PRECOMPILES_PINS`),
so there is no height to wait for. Only a node built before the reroll lacks them. Contracts reach
them through `citrate-chain:contracts/src/lib/CitratePrecompiles.sol#library CitratePrecompiles`, which
reverts with `PrecompileUnavailable(address)` when a precompile does not answer, so "not active" can never
read as "invalid". An anchor proof only counts together with the registry and the expected committer
(`AnchorProofs.isRecordAnchored`). The gas values are the placeholder schedule the owner accepted with the ADR.

**Hosted inference (`0x0100` to `0x0106`).** The model runtime family. It is not deterministic
across nodes, sits behind an attestation gate that defaults to always-reject, and is not reachable
from contract code today (see above). Since HUP-S7.2 the model contracts (ModelRegistry, LoRAFactory,
ModelAccessControl) call `0x0101` and `0x0106` through `CitratePrecompiles` in their native layout
(`model_id || caller || input`), so on 40204 those calls revert with `PrecompileUnavailable(0x0101)`
instead of silently paying for nothing. Training and merges run off chain and are recorded by an
operator. Do not promise contract-level inference.

## Calling precompiles

- **From Solidity:** `staticcall` the address with the packed input, bound the forwarded gas,
  check `ok` and the return length, then decode. A failing precompile consumes the gas it was
  given (observed on 40204 for malformed input to `0x0107`, `0x0111` and `0x0130`).
- **From an RPC client:** on the current node a top-level `eth_call` with `to` set to a precompile
  returned `0x` for every precompile tried, including the standard `0x02` (observed 2026-10-01).
  Call precompiles from contract code. A read-only check that sends nothing: `eth_estimateGas` on
  the init code of a throwaway contract whose constructor does the STATICCALL and reverts with the
  result.
- **On anvil:** Citrate precompiles do not exist on a stock anvil fork; contracts that use them
  need the Citrate-aware fork (planset HUP-S6.10).
- **Hermes helpers:** the node MCP `precompile_call` read tool covers `0x0107` to `0x0111`, `0x0120`,
  `0x0130` and `0x0200` to `0x0202` (subject to the top-level `eth_call` caveat above), and
  `ed25519_verify` is a typed helper for `0x0120`. For the fork precompiles, `agent_precompile_encode`
  builds the exact input bytes, the scheduled gas and the Solidity helper to pass them to (from Q16.16
  tensors, the sidecar's anchor proof, or a stored device link or revocation), and
  `agent_precompile_decode` reads an answer back; empty output is reported as "not active", never as a
  verdict. Both are pure: no RPC, no key. The encodings are pinned to the chain's own encoders by
  `citrate-chain:core/execution/tests/fixtures/agent_precompile_vectors.json`. A read needs no
  signature. Anything that writes on-chain from the result is a transaction and goes to the member's
  SignatureCeremony.
- **On a devnet:** `citrate-chain:scripts/devnet-precompile-check.sh` checks both sides of the fork
  height with those vectors. It refuses 40204.

## Citing the docs

Cite the docs section next to any code reference:

| Topic | Docs section to cite |
|---|---|
| the address pages and their ranges | `citrate-docs:content/chain/precompiles.md#what-it-is` |
| how a contract calls a precompile, where exact encodings come from | `citrate-docs:content/chain/precompiles.md#how-to-use-it` |
| the tensor format | `citrate-docs:content/chain/precompiles.md#tensor-primitives` |
| x402 verifiers and their gas | `citrate-docs:content/chain/precompiles.md#x402-payment-precompiles` |
| what a precompile returns on a bad input or signature | `citrate-docs:content/chain/precompiles.md#failure-modes` |
| `0x0110` and Q16 | `citrate-docs:content/chain/precompiles.md#belnap-q16-lattice-aggregation` |
| hosted inference `0x0100` to `0x0106` | `citrate-docs:content/chain/precompiles-zkp.md#hosted-inference-0x0100-to-0x0106` |
| `0x0107` to `0x0109` verification | `citrate-docs:content/chain/precompiles-zkp.md#verification-0x0107-to-0x0109` |
| Q16.16 compute `0x010A` to `0x010F` | `citrate-docs:content/chain/precompiles-zkp.md#deterministic-q1616-compute-0x010a-to-0x010f` |
| the attestation gate | `citrate-docs:content/chain/precompiles-zkp.md#attestation-gate` |

## Answering well

- Give the full address and the source file for any claim about inputs, outputs or gas.
- Say "not bridged" plainly for `0x0100` to `0x0106`, and say what an empty return means.
- For `0x0112`, `0x0113`, `0x0121` and `0x0122`, say they are active from genesis on 40204 (the
  2026-10-05 reroll) and reachable from contract code; empty data still means "no answer", never a
  verdict.
- Distinguish "deterministic and verifiable" (verification, compute, learning, crypto, x402) from
  "signed receipt under attestation" (hosted inference).
- If asked for a live figure the sources do not hold (current hardening height, adoption, prices),
  say it is not in the sources.
