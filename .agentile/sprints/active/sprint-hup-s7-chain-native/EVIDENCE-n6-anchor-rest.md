---
created: 2026-10-04T18:00:00Z
branch: hup/n6-anchor-rest
author: Larry Klosowski + Claude Opus 5.5
status: active
sprint: HUP-S7
wp: HUP-S7.3, HUP-S7.5 (core halves, rest), gate g4-anchor (local evidence)
---

# HUP-S7.3 + S7.5 rest: proofs checked by core, opt-in benchmark sharing, anvil rehearsal

Builds on `WP-S7.3-S7.5-ANCHOR-METERING.md` (fan-out 4) and the HIC decision records that now
land in the anchored log (HUP-S2.6 on `hup/m2-core` / `hup/m2-runtime`). It takes the stopped
fan-out 5 lane's work (`hup/n5-anchor-rest`, committed there as unreviewed WIP), reviews it,
fixes what the review found, and proves the whole path on a local anvil. Branches:
citrate-core `hup/n6-anchor-rest` and citrate-agent-runtime `hup/n6-anchor-rest`, both from the
M2 branches. Sprint issue: CitrateNetwork/citrate-federation#284.

## What landed

| Piece | Where | State |
|---|---|---|
| Sent anchors kept across restarts (owner-only in-flight file written before the send; an unreadable file blocks new anchors), gas caps (placeholders), anchor key used only while the vault is unlocked | `kit/src/ceremony/anchor.rs`, `src-tauri/src/chain_agent.rs` | implemented, tested |
| "Prove a past decision": core recomputes the record hash, the RFC 6962 audit path and the day commitment itself, then reads `AnchorRegistry` on 40204 (`getAnchorBy` on the next registry version, `getAnchor` on the deployed one) | `src-tauri/src/anchor_proof.rs`, `src/journal/DecisionProofs.tsx`, `proofView.ts` | implemented, proven on anvil |
| Record list with each day's anchor state | runtime `GET /anchor/records`; proof answer carries the hashed record bytes | implemented, tested |
| Opt-in BenchmarkRegistry sharing: the sidecar's calls are rebuilt byte for byte by core and become one pending wallet card per metric (HIC-1, member's account) | `src-tauri/src/benchmark_share.rs`, `src/journal/BenchmarkShare.tsx` | implemented, proven on anvil |
| `AnchorRegistry` and `BenchmarkRegistry` as optional pins in the generated 40204 book | `scripts/sync-addresses.py`, `src-tauri/addresses/40204.json` | regenerated with `--rpc https://rpc.citrate.ai`: chain id, genesis and code at every pin checked live |
| Anvil rehearsal of the whole path, both registry versions | `scripts/anvil-anchor-e2e.sh`, `src-tauri/src/anchor_e2e_tests.rs`, runtime `agent-sidecar/tests/anchor_e2e_seed.rs` | PASS (below) |

Both features still ship **off**: the member's settings default to off, so a pinned registry
starts no schedule and shares nothing until the member turns it on.

## Review findings fixed (the WIP had not been run)

1. **Last leaf of an unbalanced day never verified (core).** Core's second copy of the RFC 9162
   verifier dropped the `fn == sn` branch (section 2.1.3.2 step 5.2). Any day whose record count
   is not a power of two would have shown its last record as "the proof does not hold". Red:
   `every_fixture_proof_holds_and_leads_to_the_planned_commitment` (seq 4 of 5) and
   `an_edited_record_is_never_proven_even_if_anchored` failed. Fixed; new test checks every leaf
   of every tree size 1 to 33 against an independent RFC 6962 tree builder.
2. **A batched day was listed as anchored (runtime).** `/anchor/records` set `anchored` equal to
   `batched`, so the Journal said "anchored in block ..." before core confirmed any transaction.
   Red: `a_batched_day_is_not_anchored_until_core_confirms_it`. Fixed (`anchored` only with a
   confirmation); mutant re-applied and killed.
3. **On 40204 an unanchored day would read as a registry mismatch (core).** The Citrate node puts
   revert data in the error message, not in `data` (read from rpc.citrate.ai on 2026-10-04:
   `"... Contract call reverted: 0x46716752 (gas used: 32425)"`, `0x46716752` =
   `AnchorNotFound()`). Core now reads revert data from the message too; tests pin both live
   message shapes.
4. **Record bytes were bound by hash only (core).** Link 1 now also requires the record's own
   `seq` to be the proof's and its timestamp to fall on the batch's UTC day, as the runtime's
   `verify_record_proof` does.
5. **Main-thread tripwire.** `InFlightAnchors::remove` writes a file; the name-based tripwire then
   counted every `.remove(` (for example `social.rs::directory_forget`) as blocking. Renamed to
   `settle_day`.
6. **Stale copy.** "not deployed on 40204 yet" (registry absent) now says "not in this app's 40204
   address book"; the node MCP address-book note no longer claims unlisted contracts are
   undeployed.
7. **Shared-target races in the rehearsal.** The script now builds core's test binary once and
   copies it aside, like the sidecar binary.

## Anvil rehearsal (gate g4-anchor, local evidence), 2026-10-04

`CITRATE_ANCHOR_E2E_NEXT_REF=origin/hup/n5-chain-redeploy scripts/anvil-anchor-e2e.sh`: registries
compiled from citrate-chain `0aab474b` (main, the deployed version) and from
`origin/hup/n5-chain-redeploy` `3c3d5eeb` (the next redeploy's version); private anvil on chain id
40204; real sidecar binary started by core's `HermesManager`. Nothing touched 40204.

| Step | current registry | next registry |
|---|---|---|
| Core HIC outbox (grant, budgeted escalation, ceremony card, tool card) handed to the seed | 4 records | 4 records |
| Closed day | 15 records (5 plain + 7 from core's outbox + `shell.run` decision and outcome + declined `ceremony.capsule_effect`) | same |
| Nightly tick | one card; locked vault signs nothing; unlocked: in-flight record, send, settle | same |
| Anchor gas used | 179,465 | 335,227 (cap 400,000, placeholder) |
| Records proven by core against the chain | 15 of 15, block 4, "by your anchor key" | 15 of 15 |
| HIC kinds proven | grant.folder_added, escalation.spend, ceremony.approval, agent.tool_approval, shell.run, ceremony.capsule_effect | same |
| Live export of core's outbox to the running sidecar | lands on the open day, not batched, not anchored | same |
| Tampered proof | not proven | not proven |
| Stranger anchors the same value | reverts (AlreadyAnchored); member still proven | succeeds under the stranger's address; member still proven |
| BenchmarkRegistry: payload rechecked by core, 15 `record()` calls from the member account, read back | 15 of 15 match | 15 of 15 match |

## Gates

| Gate | Result |
|---|---|
| core `cargo test --lib` (src-tauri) | 1398 passed, 0 failed, 12 ignored (45 new Rust tests incl. 2 ignored rehearsal steps) |
| kit `cargo test -p citrate-core-kit --lib` | 298 passed, 1 ignored (6 new) |
| `npx vitest run` | 1849 passed, 10 skipped (10 new) |
| `npx tsc --noEmit` | clean |
| clippy 1.98.1 `-D warnings` (kit, citrate-core, all targets) | clean |
| `cargo fmt --all -- --check` (core) | clean |
| runtime `cargo test -p agent-sidecar` lib | 333 passed, 2 ignored (5 new route tests, 1 ignored seed step) |
| runtime agent-anchor / agent-records tests | 32 / 44 passed |
| runtime clippy 1.98.1 `-D warnings` (agent-sidecar) | clean |
| runtime `cargo fmt -p agent-sidecar -- --check` | clean (the runtime workspace's `--all` check fails on files this lane did not touch) |
| copy lint `scripts/cx-copy-lint.sh` | OK |
| Mutants (hand) | 4 of 4 killed: `anchored = batched` (runtime); verifier without `fn == sn` (the red state, killed by the fixture and the 1..33 tree test); no revert data from the message (`on_40204_a_value_that_is_not_anchored_reads_as_not_anchored`); no seq/day binding (`record_bytes_for_another_number_or_day_are_not_bound_even_when_rehashed`) |

## Decisions taken with conservative placeholders (pending owner sign-off)

1. Hermes's registry is `AnchorRegistry` (0x41e0f9A4...), not `CitAgentAnchorRegistry`.
2. Anchor gas caps 50 gwei and 400,000 gas (O-5 open; the next registry version needs about
   335,000).
3. Benchmark sharing: one wallet card per metric from the member's account, agent id = the
   member's first AgentSBT, a day prepared once per app session.
4. Every nightly anchor still needs the member's explicit approval (no unattended HIC-2).

## Not done (honest)

- No live anchor or BenchmarkRegistry record on 40204. That needs O-5 (how the anchor key pays
  gas) and a member approving a real card; no agent can do it.
- D-27 measures (time to first token, tokens per second, SALT, gas, resource peaks, energy,
  labelled self-review) are still reported as unknown (US-7.3 AC1).
- Nothing was clicked through in a packaged app; the Journal panels are covered by vitest.
- The benchmark share needs an AgentSBT; none exists on 40204 yet (g4-identity).
