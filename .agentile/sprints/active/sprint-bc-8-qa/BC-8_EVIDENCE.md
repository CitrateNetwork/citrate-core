---
title: "BC-8 — T1–T9 Adversarial QA Evidence Matrix (S7.4 security gate)"
created: 2026-07-20
branch: sprint/bc-8-qa-t1-t9
author: Claude (Opus 4.8) for @SaulBuilds
status: build-and-stop — awaiting separate reviewer (not self-reviewed)
repos:
  - citrate-core (branch sprint/bc-8-qa-t1-t9)
  - core-membership (branch sprint/bc-8-qa-t1-t9)
planset: citrate-federation/.agentile/planset/2026-07-11-citrate-core/06_SECURITY_AND_COMPLIANCE.md §1
---

# BC-8 — T1–T9 Adversarial QA Evidence Matrix

The threat-model-driven security QA gate for citrate-core + core-membership. One row
per Ti: **verdict**, the exact test(s) that prove it, the negative-control result (did
the guard bite when neutralized?), and the honest residual/waiver. Rule-1 applies to
this matrix: anything that is design-asserted-not-tested or infra-gated is labelled as
such — no overclaiming.

## Test counts (before → after)

| Repo | Gate | Before | After | Δ |
|---|---|---|---|---|
| citrate-core | `cargo test --workspace --locked` | 352 pass / 6 ign | 352 pass / 6 ign | 0 (no new Rust tests needed; count held, monotone OK) |
| citrate-core | `npm run test` | 171 | 171 | 0 |
| citrate-core | `npm run typecheck` | clean | clean | — |
| core-membership | `pnpm test` | 263 pass / 1 skip | 271 pass / 1 skip | **+8** (ratchet ≥263 OK) |
| core-membership | `pnpm typecheck` | clean¹ | clean | — |

¹ A stale `.next/types/validator.ts` artifact produced a spurious TS2307 on first run
(pre-existing, reproduced under `git stash`); `rm -rf .next/types` clears it and
typecheck is clean. Not caused by BC-8 changes.

## Evidence matrix

| Ti | Threat | Verdict | Proving test(s) (file:line) | Negative control — did it bite? | Residual / waiver |
|---|---|---|---|---|---|
| **T1** | Keystore theft / sidecar key exfil | **COVERED** (disk) / **PARTIAL** (memory, by design) | `adv1_adv7_signer_only_reachable_via_approve` (ceremony_tests.rs:202); `adv8_no_invoke_command_returns_secret_bytes` (custody_tests.rs:287); `adv2_no_signing_command_returns_secret_material` (ceremony_tests.rs:305); disk: `adv4_no_plaintext_on_disk` (custody_tests.rs:172), `adv4_no_secret_plaintext_at_rest_and_no_keys_json` (wallet_tests.rs:302); AI exfil: `local_inference_url_is_rust_derived_and_never_carries_a_foreign_host` (ai_tests.rs:420) | **YES.** Widened `pub(crate) fn sign_message` → `pub` in wallet.rs:353 → `adv1_adv7_signer_only_reachable_via_approve` FAILED ("the message signer must be pub(crate), never pub"). Reverted clean. | **Memory byte-residue is NOT tested** — `adv9_*` prove the teardown + `Zeroizing` TYPE contract only; a live-process RAM scrape for the DEK/entropy is deferred to a zeroize-audit MIR/LLVM pass (custody.rs:1489-1496). T1(a)/(b) are structural source/registry scans (robust, each with a stated neg-control) that rely on the `strip_test_module` truncation heuristic being exact. |
| **T2** | Signature-ceremony spoofing | **COVERED** (Rust API) / UI leg out of scope | forged-origin verbatim + undecodable→raw-gated: `adv5_true_origin_surfaced_and_undecodable_is_raw_gated` (ceremony_tests.rs:440), `adv5_tx_path_true_origin_and_undecodable_raw_gated_end_to_end` (ceremony_tests.rs:804), `b1_4_undecodable_tx_calldata_still_raw_gated` (ceremony_tests.rs:1102); one-approval-one-sig: `adv10_one_approval_one_signature_consumed` (ceremony_tests.rs:571), `adv10_concurrent_duplicate_approvals_yield_one_signature` (ceremony_tests.rs:594); no-approve-latest: `adv6_approval_is_bound_to_explicit_id_no_approve_latest` (ceremony_tests.rs:530) | **YES ×2.** (a) Neutralized consume-first (`map.remove` → `map.get().cloned()`) → both `adv10` tests FAILED (a replay produced a 2nd signature). (b) Neutralized raw-ack gate (`if pending.requires_raw_ack` → `if false && …`) → both `adv5` tests FAILED. Both reverted clean. | **"Approve is never default-focused" is UNTESTED in Rust** and untestable here — it is a webview/UI property. The Rust layer proves only that no auto-approve / approve-latest API exists (`adv6`). Flag: the actual default-focus one-click UI behavior needs a frontend harness test (not in this gate). |
| **T3** | Fake update / update MITM | **WAIVED — infra** | (none) | (n/a) | The Tauri updater is **NOT built** (BC-7.2 / WO-2, @rule8 — confirmed in docs/CITRATE_CORE_FINISH_PLAN.md WO-2 = PENDING). Design answer: offline signature verify + pinned pubkey + TLS. The real tampered-manifest AND tampered-artifact rejection tests land WITH the updater. Not fabricated. |
| **T4** | Entitlement bypass (clock rollback, token replay, patched binary) | **PARTIAL** — forgery + replay COVERED; **clock-rollback = GAP (documented, tripwire added)** | replay/expiry: `a token whose entitlement.expiresAt has passed verifies as locked/expired` (license.test.ts:185), route `expired entitlement → 200 {locked, entitlement_expired}` (license-route.test.ts:115); forward-skew forgery: `a token whose iat is far in the FUTURE … is locked/future` (license.test.ts:302); grace clamp: `an inflated graceMaxAgeSeconds cannot buy MORE than 72h` (license.test.ts:322); patched-client: `no signing key → honest not-configured, NEVER a token` (license.test.ts:61), JWKS publishes public half only (license.test.ts:246); **NEW** `T4 — clock-rollback grace` suite (license.test.ts, 2 tests) | **YES (T6 revoke control, shared).** For the T4-new tripwire: it is a *characterization* test — it asserts the CURRENT (gap) behavior, so a bite = the code changing. The forward-skew half has a real neg control via the existing `future`-guard tests. | **`verifyLicense` is a PURE, stateless verifier: grace = `now − iat`, no monotonic anchor.** A BACKWARD device-clock rollback re-grants a token that was stale a moment earlier. The design answer's "monotonic-time grace check" must live in the citrate-core **Rust license consumer**, which **does not exist yet** (no license consumer in src-tauri; WO-10-gated). New test `goes stale at 73h, then re-grants when the device clock is rolled BACK to 1h (documented gap)` (license.test.ts) is a **red tripwire** asserting `valid:true` today — it MUST flip to `false` once the monotonic anchor lands. This is the single most important residual for the reviewer. |
| **T5** | Signed-URL leakage | **COVERED (server side)** / in-app leg WAIVED-infra | consumed-URL refusal: `first redeem serves (302)… a REPLAY of the SAME token is 410 (audited)` (commissary-download.test.ts:293), single-writer `the FIRST consume wins; a REPLAY … gets null` (download-grants-single-use.test.ts:73); audit row: chain-intact assertions (commissary-download.test.ts:318); TTL/per-sub: `verify rejects an expired token` (token.test.ts:73), mint round-trip pins sub/art/jti (token.test.ts:58) | Existing single-use suite carries its own RED neg-control test (`more than one concurrent handler 'wins' the naive consume (the vulnerability)`, download-grants-single-use.test.ts:64) — the vulnerable path is demonstrated, then closed. Not re-run under BC-8 (already load-bearing by construction). | **Refusal status is 410 (Gone), not 403** — the planset red-test says "403"; the implemented contract is 410 for a consumed token, 403 for a forged/cross-artifact token. Both refuse. **The in-app Commissary byte-download (BC-4 / WO-7) is NOT wired** — the artifact route 302-redirects to an external store; with the store unset it honestly 503s after consuming. Server-side signed-URL logic is fully covered; the in-app leg is BC-4-gated. |
| **T6** | Grant-flow abuse (double-grant, refund, KYC-swap) | **GAP-CLOSED** (refund revocation built + tested) | double-grant idempotency: `REPLAY of the same event id settles once` (webhook-settlement.test.ts:117), on-chain skips (execute.test.ts:127/136/144); triple-proof: `orchestrator … exhaustive over 2^3` (orchestrator.test.ts:34), `a failing gate is a REAL denial and NEVER calls the chain` (orchestrator.test.ts:77); **NEW refund path (3 webhook + 3 repo tests):** `a refund revokes the entitlement (license fails closed) AND audits a membership flag`, `a replayed refund event revokes ONCE and preserves the first revokedAt`, `a refund with no sub is acked as unactionable` (webhook-settlement.test.ts); `revokeEntitlement … revokes an active row`, `re-revoke is idempotent and PRESERVES the original revokedAt`, `revoking a non-existent subject is a safe no-op` (entitlements-repo.test.ts) | **YES.** Neutralized the refund revoke (`const rev = await revokeEntitlement(sub)` → hardcoded no-op) → `a refund revokes the entitlement…` FAILED. Reverted clean. | **Before BC-8 there was NO refund handler at all** — the webhook only settled `checkout.session.completed`; the revocation *primitives* (`revokedAt` column, downstream license-lock) existed but nothing wired them. BC-8 added: `revokeEntitlement(sub)` write primitive (entitlements.ts) + a `charge.refunded` / `refund.*` branch in webhook.ts that flags MEMBERSHIP (audited `stripe.refund` denial keyed to order+sub) AND ENTITLEMENT (revoke → license fails closed). **@rule8 note for reviewer:** this touches the money path; the refund branch trusts our own OIDC-set `metadata.sub`/`order_id` (same trusted-metadata pattern as settlement), does NOT reverse the on-chain grant (staked SALT), and does not add a "refunded" order state (avoided a machine migration). Reviewer should confirm the metadata-propagation and on-chain-reversal policy are acceptable or scope a follow-up WP. **KYC-swap:** the live `recheckKyc` seam is real and wired but every integration test MOCKS it — the authority round-trip itself is design-asserted, not integration-tested. |
| **T7** | Malicious MCP client on memory socket | **WAIVED — infra / design-asserted (weaker than agent path)** | read-denial surfacing only: `tool_error_surfaces_not_swallowed` (memory_tests.rs:301), `parse_tool_response_maps_errors` (memory_tests.rs:323); ciphertext-at-rest (stub) `store_on_disk_is_ciphertext_not_plaintext` (memory_tests.rs:248) | (n/a — no app-side enforcement to neutralize) | **No foreign-uid-refused test, and no app code sets socket perms.** The socket is bound by the spawned `mcp_serve` daemon; citrate-core passes only the two paths and does NOT chmod the socket or harden the store dir (unlike agent.rs which hardens its token dir to 0700). **No write path exists in citrate-core's memory bridge** (read-only: recall/search/neighbors) — so "over-scoped write denied + audited" has no seam here and no audit sink; it lives in the daemon. Both T7 red-tests are OS/daemon-gated and cannot be proven in this crate. Cross-repo action: audit the `mem-store` daemon's socket perms + grant/write-scope. |
| **T8** | Node/agent supervision hijack | **PARTIAL** — bearer gate COVERED; non-loopback-bind + constant-time-compare WAIVED (daemon-owned) | bearer gate over REAL loopback: `ureq_transport_wrong_bearer_is_401` (agent_tests.rs:426), `absent_bearer_is_401` (agent_tests.rs:438), `no_session_bearer_fails_closed` (agent_tests.rs:464); token file 0600/parent 0700 `token_file_is_0600_parent_0700` (agent_tests.rs:276); token never leaks `bearer_never_in_errors_or_debug` (agent_tests.rs:297); no-direct-sign `adv7_agent_module_never_calls_the_gated_signer` (agent_tests.rs:944) | Not re-run under BC-8; the wrong-bearer→401 test IS itself the negative control for the bearer gate (a real loopback round-trip). | **The two SPECIFIC red-tests are NOT provable in citrate-core:** (a) "remote (non-loopback) bind fails" — the addr is a constant handed to the child; the child (`citrate-node-agent` server.rs:47) binds loopback-only; citrate-core neither binds nor validates. (b) "wrong token is timing-invariant" — **there is NO constant-time compare in citrate-core**; core only MINTS the bearer. The comparison lives in the un-vendored `citrate-node-agent/crates/supervision/src/auth.rs` (`SupervisionAuth`). Cross-repo audit action: confirm that repo uses `subtle::ConstantTimeEq`/`ct_eq`. The test stub uses variable-time `==` (agent_tests.rs:393) — a fixture, not production. |
| **T9** | Slashing via mismanagement | **COVERED** (crash-restart) — heartbeat/pause N/A by design | forced-crash→restart: `crash_produces_record_and_restart` (supervisor_tests.rs:114); **soak:** `intermittent_crashes_with_healthy_runs_never_permanently_fail` (supervisor_tests.rs:534, kills child 6×, always restarts, never permanently Failed); hung-probe doesn't stall `hanging_health_probe_does_not_stall_crash_detection_or_stop` (supervisor_tests.rs:687); bounds `fork_bomb_is_bounded_and_backoff_increases` (supervisor_tests.rs:178) + neg-control `fork_bomb_bound_is_load_bearing` (supervisor_tests.rs:237) | The suite SHIPS its own neg control: `fork_bomb_bound_is_load_bearing` (supervisor_tests.rs:237) proves the retry cap is load-bearing. Not additionally neutralized under BC-8. | **The supervisor has NO "heartbeat" field / no `pause`** — it is a crash-detect-and-restart monitor. "pause warns on in-flight jobs" and "crash-restart preserves heartbeat continuity" have no corresponding code (heartbeat is an on-chain intent the node-agent emits, agent.rs:53). The forced-crash→restart-within-window property IS strongly tested (real SIGKILL + soak). Treat the heartbeat-preservation wording as satisfied by "recovering node keeps restarting rather than dying permanently." |

## Zero-`.unwrap()` gate (UI-facing Rust)

`grep -rn '.unwrap()' src-tauri/src | grep -v unwrap_or` (production paths, excluding
`_tests.rs` and inline `#[test]`): **1 residual production hit** —
`custody.rs:1540` `*v.autolock_secs.lock().unwrap()` inside the `custody_status`
`#[tauri::command]`. This is a `Mutex` poison-unwrap (panics only if another thread
already panicked while holding the lock). The other 3 hits (config.rs:237/270/276) are
inside inline `#[test]` fns, not production. **Reviewer decision needed:** is the
poison-unwrap acceptable (standard idiom, poisoned-lock recovery is arguably worse) or
should the command map a poisoned lock to an honest `Err(String)` fail-closed? Flagged,
not changed (out of scope for a QA gate; a 1-line fix for the owner).

## Negative controls run (all reverted; working trees clean)

| Control | Guard neutralized | Result |
|---|---|---|
| T1 signer isolation | `pub(crate) fn sign_message` → `pub` (wallet.rs) | `adv1_adv7_signer_only_reachable_via_approve` FAILED — bit. |
| T2 single-use | consume-first `map.remove` → `map.get().cloned()` (ceremony.rs) | `adv10_one_approval_one_signature_consumed` + `adv10_concurrent_…` FAILED — bit. |
| T2 raw-ack | `if pending.requires_raw_ack` → `if false && …` (ceremony.rs) | `adv5_*` (both paths) FAILED — bit. |
| T6 refund revoke | `revokeEntitlement(sub)` → hardcoded no-op (webhook.ts) | `a refund revokes the entitlement…` FAILED — bit. |

## New tests written under BC-8 (core-membership only; +8)

- `src/lib/db/entitlements.ts` — NEW `revokeEntitlement(sub, at)` write primitive (idempotent, preserves original revokedAt).
- `src/lib/stripe/webhook.ts` — NEW `charge.refunded` / `refund.*` branch → flags membership (audit) + entitlement (revoke), idempotent by event id.
- `tests/integration/webhook-settlement.test.ts` — +3 refund tests.
- `tests/integration/entitlements-repo.test.ts` — +3 `revokeEntitlement` tests.
- `src/lib/license/license.test.ts` — +2 T4 clock-rollback tripwire tests (one characterizes the gap, one confirms the forward-skew half is closed).

## Summary verdicts

- COVERED: T5 (server), T9.
- COVERED with a named untestable/UI residual: T1 (memory-residue deferred), T2 (default-focus is UI), T8 (bearer gate strong; the two named red-tests are daemon-owned).
- GAP-CLOSED: T6 (refund revocation built + red-then-green + neg-control).
- PARTIAL / documented GAP: T4 (clock-rollback tripwire — needs the Rust monotonic anchor).
- WAIVED-infra: T3 (updater unbuilt), T7 (socket perms + write-scope daemon-owned), T5 in-app leg (BC-4/WO-7).

**Top concerns for the reviewer:** (1) T4 device-clock rollback is a real, currently-open
hole — the tripwire asserts today's granting behavior and must flip once the Rust
consumer's monotonic anchor lands. (2) T6 refund path is new money-path code (@rule8) —
confirm the trusted-metadata source, the deliberate non-reversal of the on-chain grant,
and the no-order-state-change decision. (3) T8 constant-time compare + non-loopback-bind
and T7 socket perms/write-audit are all delegated to `citrate-node-agent` / `mem-store`
daemons — schedule a cross-repo audit; they are NOT provable in citrate-core. (4) One
production `.unwrap()` at custody.rs:1540 (Mutex poison) — owner call.
