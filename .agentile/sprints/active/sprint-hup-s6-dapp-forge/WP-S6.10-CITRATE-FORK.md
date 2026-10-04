---
created: 2026-10-01
branch: hup/n5-chain-fork
updated: 2026-10-04
author: Larry Klosowski + Claude Opus 5.5
status: active
sprint: HUP-S6
wp: HUP-S6.10
issue: CitrateNetwork/citrate-federation#283
---

# HUP-S6.10: the Citrate-aware fork for the dApp forge

Planset: `05_SPRINTS_AND_WPS` S6.10 ("Citrate-aware fork (REVM + Citrate precompiles) for
dry-runs of precompile-using contracts"); `02_ARCHITECTURE` §7 (anvil fork of 40204, deploy
and mint dry run); `04_FEATURES_BDD` US-6.1.

## The problem

The D-4 gate's fork item accepted an anvil receipt only. Anvil runs Ethereum's precompiles,
so a contract that calls a Citrate precompile reaches an empty account there and gets
success with no data (checked on 2026-10-01: `eth_call` to `0x…0110` on an anvil fork of
40204 returns `0x`). The gate therefore refused every precompile-using contract.

## What shipped

**citrate-chain `crates/citrate-fork`** (branch `hup/n5-chain-fork`). A read-only binary
that runs a plan (create, then calls) over state read from a JSON-RPC endpoint at one pinned
block (40204 itself, or the member's local anvil fork of it), with the node's own rules:

| Rule | Where it comes from |
|---|---|
| Citrate precompiles | the node's `register_citrate_precompiles` (made `pub`, no logic change) |
| PBA hardening flag | the release pin for the chain id (247,436 on 40204) |
| Contract value transfers, EIP-161 contract nonces | `value_semantics_at`, `persist_contract_nonces_at`; a block under a legacy rule is refused |
| EVM | CANCUN, the endpoint's chain id, gas price 0 inside the EVM (as the executor runs it) |

`citrate-fork precompiles` prints the coverage table. The fork **cannot** reproduce: the
inference family 0x0100-0x0106 (hosted model runtime), the reserved slots 0x0112-0x013F and
0x0203-0x0209, the top-level model/artifact/governance precompiles 0x1000/0x1002/0x1003, and
0x0130 at a hardened height unless built with `commd-fold-verify`. Every touched address is
in the report with its coverage; nothing unavailable is ever simulated as success.

**Core** (this branch):

- `src-tauri/src/fork_dry_run.rs`: builds the plan (the gated init code, then an optional
  test mint `mint(uint256)` at `PRICE * quantity`, the S6.6 after-deploy test run on the fork
  first), runs `citrate-fork` (`CITRATE_FORK_BIN`, else the installed `citrate-fork`
  component) with a 120 s bound, and returns the gate's `forkDryRun` input bound to the init
  code the fork executed. Command `deploy_gate_fork_dry_run` (read-only, ACL entry added);
  bridge `contracts.gateForkDryRun`.
- `deploy_gate.rs` `parse_fork`: a citrate-fork report passes when it ran on chain 40204
  state, started with the create, every later step succeeded, nothing unavailable was
  touched, and every Citrate call site the bytecode scan finds is one the fork runs with real
  code. A plain anvil receipt keeps the old rule (no Citrate precompiles).
- `deploy_gate_submit` with `forkInCore` (added in the fan-out 6 review): core runs the fork
  step itself on exactly the submitted init code (`evaluate_submission`), so the gate's fork
  item no longer depends on a caller to produce it. A report handed in through `forkDryRun`
  (provenance `Caller`) cannot vouch for Citrate precompiles: its coverage list is the
  caller's word, so a contract that uses them passes only with `forkInCore`. Exactly one of
  `forkDryRun` and `forkInCore`; neither FAILs the item. Bridge type `ForkInCore`.
- `scripts/e2e-postdeploy-reader.sh --fork-bin <path>`: runs the fork step on the rendered
  hello-mint contract plus a 2-token test mint against the e2e anvil (now genesis block
  100,000, above the CREATE-nonce activation).

## Proof

- citrate-chain: `cargo test -p citrate-fork` 21 passed (incl. a real-anvil run); clippy
  clean on 1.96.0 and 1.98.1. Mutation: removing the precompile registration fails 3 tests.
  Live read-only run against `https://rpc.citrate.ai` (block 91,449): the 0x0110 probe
  deployed and returned the node's 9-byte answer.
- core: `cargo test --lib` 1303 passed / 10 ignored (was 1282 / 10; +21 in
  `fork_dry_run`). `e2e-postdeploy-reader.sh --fork-bin …` passed end to end: LemonDrops
  created and test-minted on the Citrate-aware fork over anvil state, fork item PASS.
  `tsc` clean; contracts bridge + agent vitest 371 passed.

Fan-out 6 review proof (2026-10-04, branches merged up to `hup/m2-core`): citrate-fork 24
passed (+3: `node_parity`, a top-level precompile trace, an oversized RPC answer); core
`cargo test --lib` 1383 passed / 10 ignored after the merge (`fork_dry_run` 21 -> 30);
mutation: disabling the provenance guard fails both caller-supplied tests; clippy 1.98.1
`-D warnings` clean; `cargo fmt --check` clean; `tsc` clean; the e2e script with
`--fork-bin` passed again, now also through `evaluate_submission` with `forkInCore`.

## Review (fan-out 6, 2026-10-04)

An adversarial review of both branches found and fixed:

- **Block environment drift (chain).** The fork set the REVM block gas limit to 30,000,000;
  the node never sets it, so on 40204 `GASLIMIT` reads `U256::MAX`. New
  `tests/node_parity.rs` runs the same init code through the node's own
  `execute_contract_create` / `execute_contract_call` and through the fork and compares nine
  environment words; it failed on GASLIMIT before the fix.
- **Caller-vouched coverage (core).** The gate trusted the report's own `real` list, so a
  forged `engine: citrate-fork` report could mark any call site as real. Fixed with
  `ForkProvenance` and `forkInCore` (above).
- **Unbounded steps around the time bound (core).** `run_fork` ran `citrate-fork --version`
  with no timeout, and wrote the plan to stdin before the timed wait (a binary that never
  reads stdin blocked the caller on a large plan). The version now comes from the report's
  `engineVersion`, and the plan is written from its own thread.
- **RPC answer size (chain).** Answers are read through a 4 MiB cap instead of being
  buffered whole first.

## Not done

- `citrate-fork` is not yet a signed component: it needs a release build per OS and a
  `toolchain-bundle.json` entry (component key ceremony). Until then the gate finds it only
  through `CITRATE_FORK_BIN`, and the fork item FAILs as not installed.
- The gate's fork step is wired in core (`forkInCore`), but nothing submits a gate run
  automatically yet: the runtime S6.3 verifiers (or the hello-mint workflow) must call
  `deploy_gate_submit` with `forkInCore` (retro A27 join, another lane).
- The site preview still runs on plain anvil, which cannot answer Citrate precompiles; the
  verdict comes from the Citrate-aware fork. A preview RPC on the fork itself is not built.
- Rule 6: core/execution changed (visibility only); the daily benchmark was not run here.
- The fork's PREVRANDAO and COINBASE are 0 and the fork block's miner: it cannot know the
  VRF output or proposer of the block it simulates. A contract that branches on them can
  behave differently on 40204.
