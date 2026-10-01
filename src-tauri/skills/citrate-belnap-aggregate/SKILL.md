---
name: citrate-belnap-aggregate
description: How to prepare input for the Citrate Belnap aggregation precompile 0x0110 and how to read its output (aggregated Q16 values, then one state byte per dimension). Use when building or checking a 0x0110 call, encoding Q16 fixed-point numbers, interpreting states[] (0 Neither, 1 True, 3 Both), estimating its gas, or calling it from a contract. Includes a worked example checked against chain 40204.
license: Apache-2.0
metadata:
  created: 2026-10-01
  branch: hup/n3-faucet-adr-literacy
  author: Larry Klosowski + Claude Opus 5.5
  status: active
  wp: HUP-S7.7
  citrate-chain: 0aab474b437389ccd54c41c26378f1b3197ec4e5
  citrate-docs: 73ea7c56bc2087058ecc58eae5dbf734c86da072
---

# Calling `0x0110 BELNAP_AGGREGATE`

Address `0x0000000000000000000000000000000000000110`. Source:
`citrate-chain:core/execution/src/precompiles/q16/belnap.rs#pub const BELNAP_AGGREGATE`. Read the
`citrate-paraconsensus` skill first for what the four values mean; this skill is the byte-level
how-to. The code at the pinned commit is the authority. Where the docs page
(`citrate-docs:content/chain/precompiles.md`) reads differently, follow the code and say so.

## Q16 numbers

Every number is a Q16.16 fixed-point value stored as a signed 64-bit integer, big-endian: the real
value is `raw / 65536`. So 1.0 is `0x0000000000010000`, 0.5 is `0x0000000000008000`, -1.0 is
`0xffffffffffff0000`, 0.8 is `0x000000000000cccd` (52429, rounded). Arithmetic saturates at the
i64 limits and never panics (`citrate-chain:core/execution/src/precompiles/q16/mod.rs#pub fn saturating_mul`).
Multiplication is `(a * b) >> 16` with an i128 intermediate.

## Input layout

Big-endian throughout (`citrate-chain:core/execution/src/precompiles/q16/belnap.rs#pub fn decode`):

| Field | Type | Bytes |
|---|---|---|
| `dim` | u32 | 4 |
| `n` (participants) | u32 | 4 |
| embeddings, participant-major: `e[i*dim + d]` | n x dim x i64 | 8 n dim |
| confidences, same order | n x dim x i64 | 8 n dim |
| weights, one per participant | n x i64 | 8 n |
| `threshold_pos` | i64 | 8 |
| `threshold_neg` | i64 | 8 |

Total length must be exactly `24 + 16 n dim + 8 n`, or the call fails with a length mismatch.
`dim` and `n` must each be between 1 and 1024 (`MAX_DIM`, `MAX_N`). Note that the weight is per
participant, not per dimension. `threshold_neg` is parsed for forward compatibility and not used.

## Output layout

`9 * dim` bytes (`citrate-chain:core/execution/src/precompiles/q16/belnap.rs#pub fn encode_output`):
first **all** `dim` aggregated values (i64 Q16 each), then **all** `dim` state bytes. The values and
the states are two blocks, not interleaved per dimension.

| State byte | Meaning in the reduced output |
|---|---|
| 0 | Neither: no participant with positive weight reached `threshold_pos` |
| 1 | True: every confident participant is on the same side (all agree, or all oppose) |
| 3 | Both: confident participants on both sides |
| 2 | False: defined in the enum but never produced at the reduced level |

The aggregated value is `sum(weight[i] * embedding[i][d])`, saturating, over every participant
(including low-confidence ones). It is a weighted sum, not a mean: pass weights that sum to 1.0
when you want a mean. Classification uses sign: with confidence at or above `threshold_pos`, an
embedding of 0 or more is "agree" and a negative one is "oppose"; participants with weight 0 or
less are skipped (`citrate-chain:core/execution/src/precompiles/q16/belnap.rs#pub fn classify_dim_threshold`).
Because True means "one side", read the sign from the aggregated value: True with a negative value
means the confident sources agree the dimension is negative.

## Gas

The formula is in `citrate-chain:core/execution/src/precompiles/q16/belnap.rs#pub fn gas_for`:
`2000 + 50 * dim`, scaled by `max(n, 1)` from a chain activation height onward. Budget for the
larger figure, `2000 + 50 * dim * max(n, 1)`. A malformed input
still costs at least the 2000 base. If you forward too little gas or the input is malformed, the
precompile errors, and like any failing precompile the STATICCALL returns false and consumes the
gas it was given, so forward a bounded amount.

## Worked example (checked on chain 40204)

Three participants, four dimensions, `threshold_pos` 0.8, `threshold_neg` 0.3.

| | d0 | d1 | d2 | d3 | weight |
|---|---|---|---|---|---|
| p0 embedding / confidence | 1.0 / 1.0 | 1.0 / 1.0 | 0.25 / 0.25 | -1.0 / 1.0 | 0.5 |
| p1 embedding / confidence | 0.5 / 1.0 | -1.0 / 1.0 | -0.25 / 0.25 | -0.5 / 1.0 | 0.5 |
| p2 embedding / confidence | 0.25 / 0.5 | 0.0 / 0.5 | 0.5 / 0.25 | 0.75 / 0.5 | 0.25 |

Expected, by hand:

- d0: p0 and p1 are confident and agree, p2 is not confident: **True**. Value
  0.5x1.0 + 0.5x0.5 + 0.25x0.25 = **0.8125**.
- d1: p0 agrees, p1 opposes, both confident: **Both**. Value 0.5 - 0.5 + 0 = **0.0**. The number
  alone would hide the disagreement; the state shows it.
- d2: nobody is confident: **Neither**. Value 0.125 - 0.125 + 0.125 = **0.125**.
- d3: p0 and p1 are confident and both oppose, p2 is not confident: **True**, with value
  -0.5 - 0.25 + 0.1875 = **-0.5625**.

Input (240 bytes, shown as 480 hex digits):

```text
0x0000000400000003000000000001000000000000000100000000000000004000ffffffffffff000000000000000080
00ffffffffffff0000ffffffffffffc000ffffffffffff80000000000000004000000000000000000000000000000080
00000000000000c000000000000001000000000000000100000000000000004000000000000001000000000000000100
000000000000010000000000000000400000000000000100000000000000008000000000000000800000000000000040
000000000000008000000000000000800000000000000080000000000000004000000000000000cccd0000000000004c
cd
```

Output returned by chain 40204 on 2026-10-01 (block 61,978, 08:50 UTC, called from contract code with
STATICCALL; the call succeeded):

```text
0x000000000000d000 0000000000000000 0000000000002000 ffffffffffff7000 01 03 00 01
```

Values 53248, 0, 8192, -36864 are 0.8125, 0.0, 0.125, -0.5625. States 1, 3, 0, 1 are True, Both,
Neither, True. The output matches the hand calculation.

## Calling it

- **From Solidity:** `(bool ok, bytes memory out) = address(0x0110).staticcall{gas: g}(input);`
  then check `ok` and `out.length == 9 * dim` before decoding.
- **From an RPC client:** on the current node a top-level `eth_call` whose `to` is a precompile
  address returned `0x` for every precompile tried, including the standard SHA-256 at `0x02`
  (observed 2026-10-01). Read precompiles through contract code. A read-only way that sends
  nothing: `eth_estimateGas` on the init code of a throwaway contract whose constructor does the
  STATICCALL and then reverts with the result; the revert data carries the output, and nothing is
  deployed.
- **In the learning daemon** the same code runs in process, byte-identical to the chain-side
  precompile (`citrate-chain:core/learning-daemon/src/aggregator.rs#belnap::aggregate`), and the
  daemon commits the result through its chain adapter.
- **Anvil does not have Citrate precompiles.** A local anvil fork gives an empty account at
  `0x0110`, so a contract that uses it cannot be tested there; that is why the planset calls for a
  Citrate-aware fork (HUP-S6.10).

## Preparing inputs for Hermes

1. Collect `n` contributions of equal `dim`; reject mismatched lengths before encoding.
2. Convert each embedding and confidence to Q16 (`round(x * 65536)`), clamping to the i64 range.
3. Choose weights (for example normalised trust weights) and convert them to Q16. Give a
   participant that must be ignored weight 0.
4. Encode in the order above, check the total length, then call. Decode the two blocks.
5. Report states with their meaning, and keep Both visible to the member. A `Both` is a reason to
   look closer or escalate (the ContradictionLedger is the on-chain record), not something to
   average away.

Anything that writes to the chain from these results is a transaction and goes through the
member's SignatureCeremony; preparing and reading `0x0110` input needs no signature.
