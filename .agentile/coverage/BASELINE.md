---
created: 2026-07-13
branch: docs/skill-artifacts
author: Claude Fable 5, directed by @SaulBuilds
status: active
purpose: The four-axis coverage ratchet baseline (agentile:test-plan skill, day-0). Canonical count commands so every session counts the same way. Ratchet rules per Agentile Rule 2.
---

# citrate-core — coverage baseline

Backfilled 2026-07-13 when the Agentile skills were adopted mid-build. Updated at every
sprint close going forward. Current snapshot: **main @ Phase C complete** (a7a51d2).

## The four ratchet axes (never decrease within a sprint; a decrease requires an ADR)

| Axis | Canonical count command | Current (2026-07-13) |
|---|---|---|
| **Rust tests** | `cd src-tauri && cargo test --workspace --locked 2>&1 | grep -E "^test result: ok" | awk '{s+=$4} END{print s}'` | **223** passing (+5 `#[ignore]` live-proofs) |
| **Frontend tests** | `npm run test 2>&1 | tail -1` (vitest) | **65** passing |
| **Formal specs** | `find . -name '*.tla' | wc -l` | **1** (`src-tauri/formal/SidecarSupervisor.tla`, TLC-checked — first formal spec in citrate-core) |
| **CI tripwires** | gate steps in `.github/workflows/ci.yml` | **rust**(fmt, clippy `-D warnings`, test `--locked`, cargo-audit) + **node**(typecheck, vitest, build) |
| **Frontmatter coverage** | `grep -rl '^---' .agentile --include='*.md' | wc -l` vs total | **100%** |

## Ratchet history (Rust axis — the load-bearing one for the custody/signing spine)
A3 (73) → B1.0/0b (82) → B1.1 (94) → B1.2 (109) → B1.4 (135) → B1.5 (141, **Phase B close**)
→ C1.0 (150) → C1.0b (156, incl. the first TLA+ formal spec) → C1.1 (168, real citrate-node
under the supervisor: NodeDomain wiring + @rule8 keyring storage-key + ciphertext-at-rest,
+1 `#[ignore]` live bounded-sync proof — 2 ignored total)
→ C1.2 (188, node-agent under the supervisor: @rule8 per-session OsRng bearer minted +
handed via 0600 token-file IPC + zeroized + 401 negative control; the node-agent
`SignatureRequest` → `SignatureIntent{origin:"agent:node-agent"}` → SignatureCeremony
request→approve→sign→broadcast bridge (reusing B1.4); ADV-7 no-direct-sign source-scan
extended for agent.rs; +1 `#[ignore]` live real-node-agent handshake proof — 3 ignored total).
Frontend 53 → 55 (agent-domain wiring: no bearer/token across the bridge).
→ C2 (204, earnings: REAL claimable via ContributionAccounting.claimable(addr) eth_call on 40204
[decode + selector cross-check 0x402914f5/0x372500ab + rpc.eth_call]; the user Claim + node-agent
sweep share ONE claimRewards() intent → agent bridge → ceremony → B1.4; C1.2-F-1 dedup [one request →
one ceremony → one broadcast] with a RED negative control; the sim validation/pinning/compute split
DROPPED/labeled as off-chain [no on-chain breakdown — Rule 1/I-3]; +1 `#[ignore]` live real-claimable
read on 40204 = 0 wei for a fresh address — 4 ignored total. No new Cargo deps.
Frontend 55 → 57 (AgentDomain.earnings() tauri/sim wiring).
CONFIRMED-CLAIM GAP: a broadcast claimRewards() needs a node with ACCRUED earnings + unlocked funded
vault; real claimable is 0, so nothing to claim — NOT fabricated (see C2_SCOPE.md for the exact command).
→ C2-remediation (209, citrate-security #30 C2 review — three findings closed): C2-F-1 (RULE-1 HONESTY)
the frontend Claim button was a PURE SIM (toasted "Claimed — balance updated from chain" + local
setState liquid+amt/claimable:0 via the sim ceremony); FIXED — the fake toast+setState are GONE, the
button now drives the REAL path: `Node.tsx onClaim → store.claimRewards → bridge.agent.claim →`
(Tauri) the registered `user_claim` command [reads on-chain claimable, honest "nothing to claim" for 0,
else bridges the real claimRewards() intent → PENDING ceremony] → approve via `sign_and_broadcast`
(B1.4 real 40204 tx); the balance changes ONLY when the tx settles + claimable is re-read (no local
mutation, no faked hash). Web-dev sim mints a sim ceremony that HONESTLY cannot broadcast (no key/chain).
C2-F-2 (LOW) the dedup check-and-mint in `agent.rs::bridge_one_pending` was NON-ATOMIC (lock released
before mint, re-acquired to insert → two concurrent bridges of one req.id minted two approvable
ceremonies = double-claim); FIXED — the check+mint+insert is now under ONE hold of the `bridged` lock
(shared `bridge_request` helper, disjoint from the ceremony's own mutex → no deadlock), with a RED
concurrency test (2 threads + a Barrier tripped in the RPC to force the pre-fix interleaving;
goes red against the non-atomic code, green after). C2-F-3 (INFO) `earnings::user_claim_request`
hardcoded `id:0` → collided with a node-agent req id 0 in the shared dedup map; FIXED — user claims use
a DISJOINT id space `USER_CLAIM_ID = 1<<63` (top bit set, unreachable by the node-agent's small
sequential counter) with a non-aliasing test. Rust 204→209 (+5: concurrency, 2 user-claim, id-alias,
id-space). Frontend 57→61 (+4: tauri claim→user_claim + honest-zero; sim claim no-fake + zero).
No new Cargo deps; cargo audit unchanged (0 vulns, pre-existing GTK unmaintained warnings only).

## Notes
- **Formal-spec axis opened at C1.0b** — the SidecarSupervisor state machine is the first
  TLA+ model in citrate-core, added because the `test-plan` skill flags state machines for
  formal-first and because a spec bug (the lifetime-vs-consecutive restart counter, F-1)
  slipped past 9 example tests and was caught by the model's TLC negative control.
- **Tripwire gap (Phase-E / S7 item, noted so it's not silently absent):** citrate-core has
  fmt/clippy/audit but not yet the federation semgrep / no-stub / no-`.unwrap()`-in-prod /
  test-count-ratchet tripwires that citrate-security CI runs. Adding the class-level
  tripwires is the S7.3/S7.4 hardening scope.

## 2026-11-26: 0.6.0 lane start (LQ-WP01)

Re-measured at the 0.6.0 lane start; everything above stays as written (history is not edited).
The numbers are the test runners' own counts on ubuntu-latest, never a Mac build or a grep.
`.agentile/coverage/ratchet.json` (schema v1, `lanes/LQ_NON_REGRESSION_GUARD.md` section 3.1 in
the 0.6.0 planset) is the machine-readable copy, taken unedited from the `ratchet-json` artifact of
the measuring run.

- **Lane-start commit:** main `a826e441` (#775). Its own `ci` run on main,
  [45549119759](https://github.com/CitrateNetwork/citrate-core/actions/runs/45549119759), is the
  cross-check for the runner counts at exactly that commit.
- **Measuring run:** `lq ratchet`
  [45925355813](https://github.com/CitrateNetwork/citrate-core/actions/runs/45925355813) at
  `567ae074` (PR #786 head: a826e441 plus this WP's script, workflow and frontmatter keys; no
  product code). The commit that adds ratchet.json changes only this file and ratchet.json, which
  no runner counts.

### The axes

| Axis | Canonical command (the ratchet) | a826e441 (ci 45549119759) | ratchet.json (567ae074) |
|---|---|---|---|
| Rust tests passed | `cargo test --workspace --locked --no-fail-fast`, sum of `passed` over every `test result:` line (23 lines: unit, integration and doc-test targets) | **4150** | **4150** |
| Rust tests ignored | same log, sum of `ignored` (counted separately, never in passed) | **24** | **24** |
| Rust tests failed | same log, sum of `failed`; any non-zero refuses the baseline | 0 | 0 |
| Vitest passed | `npx vitest run --reporter=json`, `numPassedTests` | **6020** | **6025** (+5: `scripts/ci/lq/ratchet.test.mjs`) |
| Vitest skipped | same file, `numPendingTests + numTodoTests` | **38** | **38** (todo 0) |
| Formal specs | `ls src-tauri/formal/*.tla \| wc -l` | 15 | **15** |
| TLC configs in CI | `grep -l 'EXPECT:' src-tauri/formal/*.cfg \| wc -l` (what `tla-models.yml` checks) | 61 | **61** |
| Tripwire tests | tests the runners executed whose full name contains "tripwire": rust per-test lines 63 + vitest assertion full names 187 | n/a | **250** |
| CI gate steps | named `run:` steps of `ci.yml`, `merge-gate.yml`, `eval-check.yml`, setup steps (authenticate, install, fetch, upload) excepted: ci 11, merge-gate 3, eval-check 2 | 16 | **16** |
| Rule-5 frontmatter | Markdown under `docs/` and `.agentile/` whose leading frontmatter has `created`, `branch`, `author` and `status` | 250 of 261 | **261 of 261** |

All of it in one command, as the workflow runs it:
`node scripts/ci/lq/ratchet.mjs measure --cargo-log <log> --vitest-json <json> --root . --rev <sha> --out ratchet.json`.
A log with a failed test, or with no `test result:` line, is refused and nothing is written.

### Grep inventory cross-check (not the ratchet)

| Axis | Grep at a826e441 | Runner | Difference |
|---|---|---|---|
| Rust tests | line-start `#[test]`/`#[tokio::test]`: src-tauri 3252, kit 791, components 101, templates 33 = 4177 | 4150 + 24 ignored = 4174 | 3 (0.07%): attributes in modules compiled only for macOS or Windows |
| Rust ignored | line-start `#[ignore`: src-tauri 22, kit 1, components 1 = 24 | 24 | none. (The planset's 38 counted `#[ignore` anywhere in a line, including strings and comments.) |
| Vitest | 6058 tests at a826e441 | 6020 passed + 38 skipped = 6058 | none; `.each` rows expand at run time, so `it`/`test` block greps undercount |
| Vitest skipped | 0 `.skip`/`.todo`; 19 `skipIf`/`runIf` guards (live, env or tool gated suites) | 38 | the guards skip whole describe blocks when their env is absent on the runner |
| Tripwires | 12 `*tripwire*` files; vitest titles with "tripwire" 20 | rust 63, vitest 187 | the runner count matches the full name, so a test inside a `describe("... tripwire")` counts. LQ-WP03's registry (`tripwires.json`) replaces this measure, stricter only |
| IPC commands | 475 entries in `generate_handler!` (`src-tauri/src/lib.rs`) | n/a | inventory only; `static-ratchets.mjs ipc` is LQ-WP17 |

### What changed to reach these numbers

- The one Markdown file with no frontmatter (`.agentile/BRANCH_CLEANUP_LEDGER_2026-07-28.md`) and
  ten Hermes planset and evidence files missing `branch` (one also `author`) got the missing keys,
  marked "backfilled by LQ-WP01"; the branch comes from each file's originating PR. No body text
  changed. The axis starts at 100%.
- The tripwire gap noted above (no test-count ratchet in core CI) closes in LQ-WP02 (`lq / ratchet`
  compares a PR against this file) and LQ-WP03 (the tripwire registry).
