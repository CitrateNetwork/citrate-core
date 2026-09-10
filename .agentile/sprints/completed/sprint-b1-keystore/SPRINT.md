---
created: 2026-07-13
branch: docs/sprint-b1-keystore
author: Claude Fable 5, directed by @SaulBuilds
status: active
sprint: CORE-B1
planset: citrate-federation/.agentile/planset/2026-07-12-core-beta-wiring/00_STATE_AND_PLAN.md (Phase B1)
rule8: yes — this is THE signing-key custody surface; security sign-off required; highest-stakes phase
depends_on:
  - CORE-A2 (custody vault) — merged + signed off; B1 stores the wallet key here (its first NON-re-issuable secret)
  - CORE-A3 (identity) — merged; the wallet address comes from the `wallet_address` claim
  - citrate-wallet-core (citrate-chain/wallet-core) — cross-repo dep (Rule 12 [[drift]])
---

# Sprint CORE-B1 — Wallet keystore + SignatureCeremony (the custody-of-signing spine)

Protocol (Agentile): Rule 1 (no mocks-as-live); Rule 2 (test count monotone); Rule 4
(sprint file is truth); Rule 5 (frontmatter); **Rule 3 is LIFTED here — the
SignatureCeremony is the ONE sanctioned signing path; until B1 there were no signing code
paths at all, and after B1 every signature MUST go through the ceremony**; Rule 8 (this is
the signing-key custody surface — the highest-stakes surface in citrate-core; ships an
adversarial suite + carries a security-sign-off gate); Rule 11 (acceptance names its data
source); Rule 12 (citrate-wallet-core dep goes through the manifest [[drift]] map first).
Red-test-first for every custody/ceremony guard.

## Why B1, and the invariant it makes real

B1 is where the vault finally holds a real signing key and where a signature can first
happen — through, and only through, the **SignatureCeremony**. It makes the I-2 custody
invariant concrete for signing: **the signing key lives only in the app process + the A2
OS-keyring vault; no sidecar, daemon, agent, frontend, or micro-app ever holds it or signs
without an explicit human approval in the ceremony.** Every later phase (Node earnings
claims, Wallet sends, node-agent's unsigned `SignatureRequest` seam, chat-agent tool
writes, micro-apps) routes its signature intents through this one surface.

## What B1 is NOT
- Not ERC-4337 UserOps / paymaster-sponsored sends (that path is blocked on the E-8
  paymaster remediation + bundler; it's a follow-on, CORE-S2.3). B1 signs **classic
  secp256k1** transactions/messages for chain 40204.
- Not the live balance/vitals reads (Phase B2) or the send UX end-to-end (Phase B3).
- Not guardian/social recovery, hardware wallet, or Ed25519 agent-signing (later).

## The load-bearing decision — D-B1-1 (A2 F-1 hardening, OWNER)

B1 stores the **first NON-re-issuable secret** in the A2 vault. The A2 sign-off documented
**F-1 (anchor-deletion downgrade)**: deleting the two keyring anchor entries re-seeds the
high-water from a rolled-back envelope, which could resurrect an OLD keystore or lose a
rotated key. For A3's revocable OIDC token the FS-only anchor was fine (server revocation +
re-login backstop it). **A wallet key has no such backstop** — a silent rollback to an old
key or loss of a rotated key is fund-relevant. **Recommendation: land the deferred A2
master-sealed "anchor-initialized" bit (closes F-1) BEFORE a wallet key goes in the vault**,
as B1.0. It trades keychain-reset/machine-migration recoverability, which for a
BIP39-seed-backed wallet is acceptable (the seed phrase is the real recovery, not the
anchor). Owner confirms: land F-1 hardening as B1.0, or accept FS-only for the wallet key
with the seed phrase as the documented recovery.

## Design
- **Keystore (Rust, via `citrate-wallet-core`):** reuse the crate's keystore — BIP39
  create/import, Argon2id (m=65536/t=3/p=1) + AES-256-GCM, secp256k1 signing. **Do NOT
  reimplement crypto** (Rule 9). The encrypted keystore material is stored in the **A2
  custody vault** (slot `wallet-keystore`, backend-reserved `oidc-`-style prefix — reserve
  a `wallet-` prefix too). The key is unwrapped only in-process, only while the vault is
  unlocked, only inside the signer, and zeroized after each signature.
- **SignatureCeremony (Rust core + UI):** the single HITL approval surface. A typed
  `SignatureIntent { origin, decoded_action, cost, destination, chain_id, raw }` from any
  source (user wallet action, node-agent unsigned `SignatureRequest`, chat-agent tool,
  micro-app provider). The ceremony renders origin + human-decoded action + cost +
  destination; **Approve is never the default-focused / pre-selected control**; nothing is
  signed without explicit approval (or a user-authored allowlist rule, out of B1 scope).
  On approval: unwrap the key from the vault → sign (secp256k1) → zeroize → return the
  signature. **No `#[tauri::command]` returns key material** (mirror A2/A3 I-2); the signer
  is in-process only.
- **wagmi connector (D-13):** a custom connector wrapping the internal EIP-1193 provider;
  `personal_sign` / `eth_signTypedData_v4` / `eth_sendTransaction` resolve ONLY after the
  ceremony approves and the Rust signer signs. **Signing never happens in JS.**
- **Wallet identity binding:** the address must match the `wallet_address` claim (A3) for
  the local key path where applicable, or be an explicitly-linked local key; surface which
  key signs.

## WP checklist
- [x] **B1.0 — A2 F-1 hardening (D-B1-1, owner-approved).** Master-sealed
  "anchor-initialized" marker so a deleted anchor fails closed; delete-then-rollback
  regression test. **Includes B1.0b — F-1b close (legacy-absent-field downgrade)
  below.** **DONE — independent delta re-review CLEAR** (citrate-security #15 F-1 +
  #16 F-1b; both negative controls reviewer-run, each guard load-bearing; self-heal-on-read
  confirmed safe; no regression). Merged: citrate-core #11 (F-1) + #13 (F-1b). Verdict of
  record: **F-1 AND F-1b both fully closed → B1.1 (store a wallet key) may proceed.**
  - Acceptance: `custody.enc` restored over a wiped anchor → fail closed (data source: the
    A2 custody delete-rollback probe). **MET** — see `adv_f1_delete_then_rollback_fails_closed`.
  - **Placement:** the bit is a `#[serde(default)] anchor_initialized: bool` field on the
    already-master-sealed `LockoutState` (sealed under the keyring MASTER key, read on
    every unlock path before the DEK exists). An attacker who can DELETE the plaintext
    anchors cannot forge/clear a master-sealed field without the master key. Set `true`
    once at genuine first-init (`init_inner`); `init` now `seed_anchor`s the gen-0 keyring
    high-water UNCONDITIONALLY (old `bump_high_water(0)` was `gen > cur` → no-op at 0, which
    would have left the anchor absent and tripped the new guard on the first unlock).
  - **Enforcement (`enforce_anchor_initialized`):** on load/unlock, on every `custody_get`,
    and before every `put`/`clear_slot` mutation — if the keyring high-water anchor is ABSENT
    (`None`) but the master-sealed bit is `true` → `Corrupt` (fail closed). Genuine first-init
    (anchor absent AND bit `false`) still allowed. A keyring READ error stays
    `KeyringUnavailable` (never masquerades as "absent").
  - **Accepted tradeoff (documented in code + here):** genuine anchor LOSS (keychain reset /
    machine migration / backup-restore) now also fails closed and requires an EXPLICIT
    re-init — there is intentionally NO silent recovery path. For a BIP39-seed-backed wallet
    the seed phrase is the real recovery (D-B1-1). Remaining honest limit (unchanged, not in
    scope): a DUMPED keyring master can re-seal the bit + forge the block, leaving only
    Argon2id as the per-guess brake.
  - **Red→green (proven via the injected FakeKeyring — anchors + master live in the fake;
    a live OS keyring cannot run headless, so it is honestly skipped, not faked):**
    | Test | Attack (pre-fix passes) | Post-fix |
    |---|---|---|
    | `adv_f1_delete_then_rollback_fails_closed` | delete both anchors, restore older whole `custody.enc`, unlock → re-seeds + serves OLD | `Corrupt`; `custody_get` also errors |
    | `adv_f1_genuine_first_init_still_works` | (benign) fresh vault, no anchor/master | init + unlock + round-trip succeed |
    | `adv_f1_genuine_anchor_loss_fails_closed` | delete anchors only (no rollback), unlock → silent re-seed; live-session `custody_get` → serves | `Corrupt` on unlock AND on live-session `custody_get` |
    Verified red-first: neutralizing `enforce_anchor_initialized` to `Ok(())` makes the two
    attack tests fail (attack succeeds) while first-init still passes; guard restored → all green.
  - **B1.0b — F-1b close (legacy-absent-field downgrade), from the independent review of
    #11/`92ccf95` (`review/rule8-a2-f1-hardening`, MEDIUM).** The B1.0 field was
    `#[serde(default)] anchor_initialized: bool`, which cannot tell a LEGACY pre-B1.0 block
    (field ABSENT → serde-default `false`) apart from a genuine post-B1.0 first-init
    (`false` on purpose). A legacy, validly-master-sealed block reads `false`, passes the
    guard, and delete-then-rollback re-seeds off the rolled-back envelope with **no master
    key** — and a READ-ONLY vault (the A3 OIDC-refresh pattern) never self-healed under the
    "re-anchors on next mutation" mitigation.
    - **Representation:** field is now `#[serde(default)] anchor_initialized: Option<bool>`
      (`None` = legacy/field-absent, `Some(false)` = genuine post-B1.0 first-init,
      `Some(true)` = anchored). Legacy-absence is now detectable. No `skip_serializing_if`,
      so a healed/initialized block always writes an explicit `Some(_)`.
    - **Three-way decision (`enforce_anchor_initialized`, now takes `&lockout, &env, &mek`):**
      `Some(true)`+absent ⇒ `Corrupt` (F-1); `Some(true)`+present ⇒ Ok; `Some(false)`+absent
      ⇒ Ok (genuine first-init preserved); **`None`+present ⇒ self-heal** (re-stamp
      `Some(true)`, re-seal under the master, persist) then Ok; **`None`+absent ⇒ `Corrupt`**
      (indistinguishable from the F-1b attack — the case that closes the hole).
    - **Read-only coverage:** the decision + self-heal now fire on `unlock_inner` AND
      `custody_get` (both read paths), not only on mutation, so a read-mostly A3 vault
      upgrades on its first B1.0b unlock/read. `put_inner`/`clear_slot` also carry the heal
      into their whole-envelope save so a mutation cannot clobber `Some(true)` back to `None`.
    - **Self-heal safety:** heal only runs when the anchor is PRESENT (already vouches for the
      vault); it preserves `failures`/`locked_until_ms`/`generation` (no throttle reset, no
      DR-2 trip, DEK-header/slots untouched → DR-1 untouched). An attacker who deletes the
      anchor lands in `None`+absent ⇒ `Corrupt` and never reaches a heal — the heal cannot be
      forced to launder a rolled-back block.
    - **Red→green (injected FakeKeyring; live OS keyring can't run headless — honestly not faked):**
      | Test | Attack (pre-fix passes) | Post-fix |
      |---|---|---|
      | `adv_f1b_legacy_absent_field_anchor_deleted_fails_closed` | legacy field-absent block, delete anchors, restore OLD envelope, READ-ONLY → `unlock=Ok get=Ok("OLD")` | `Corrupt` on unlock AND `custody_get` |
      | `adv_f1b_legacy_present_anchor_self_heals` | legacy block + anchor present | unlock Ok + marker re-stamped `Some(true)`; later anchor-delete now fails closed |
      | `legacy_selfheal_survives_readonly` | read-only unlock never re-anchors (B1.0 gap) | heal persists with no mutation; later delete+rollback fails closed |
      | `legacy_selfheal_on_custody_get_readonly` | legacy-under-live-session read never heals | `custody_get` heals (anchor present) + serves |
      | `adv_f1b_genuine_first_init_present_false_still_works` | — | `Some(false)`+absent still unlocks; marker left as-is |
      | `adv_f1b_negative_control_legacy_guard_is_load_bearing` | `Some(false)` (bool-equivalent) + absent re-seeds + serves OLD | (control — documents why the `None` marker is the load-bearing distinction) |
      Verified red-first two ways: (1) neutralizing the `None`+absent ⇒ `Corrupt` arm to `Ok`
      makes `adv_f1b_..._fails_closed` fail (`unlock` returns `Ok(())`, attack passes through);
      (2) neutralizing the `None`+present self-heal to `Ok` (no heal) fails all three
      self-heal tests. Both arms restored → all green.
- [x] **B1.1 — citrate-wallet-core integration + keystore in the vault.** BUILT (build-and-stop,
  `feat/core-b1-1-keystore`; awaiting independent review — see Day 3). Rule-12 [[drift]]
  entry in `citrate-federation/manifest.toml` FIRST, then the Cargo dep. BIP39 create/import;
  keystore material (raw BIP39 entropy, D-B1.1-3) stored in the A2 vault (`wallet-` reserved
  slot); requires vault unlocked, fails closed if locked.
  - Acceptance: create → key sealed in the vault (raw-bytes grep of `custody.enc` finds no
    key plaintext) — MET (`adv4_no_secret_plaintext_at_rest_and_no_keys_json`); import the
    canonical BIP44 vector → `0x9858EfFD232B4033E47d90003D41EC34EcaEda94` — MET
    (`import_canonical_vector_reproduces_standard_address`, data source: published MetaMask
    vector). In-process ecrecover round-trip — MET (`sign_message_ecrecovers_to_wallet_address`).
- [ ] **B1.2 — SignatureCeremony (the single custody surface).** Typed intents from all
  origins → one ceremony → explicit approval → in-process secp256k1 signer → zeroize.
  - Acceptance: a signature is produced ONLY after ceremony approval; no invoke command
    returns key material (command-registry enumeration test); signing while the vault is
    locked fails closed (data source: session state).
- [ ] **B1.3 — wagmi connector routes signing to the ceremony (D-13).** `personal_sign` /
  `signTypedData_v4` / `eth_sendTransaction` via the connector → ceremony → signer.
  - Acceptance: a JS-initiated sign resolves only after ceremony approval; no key or signer
    logic in JS (data source: the connector delegates to `invoke`, which delegates to the
    ceremony).
- [ ] **B1.4 — prove it (real signature).** Sign a message → recovers to the wallet address;
  sign + broadcast a real classic secp256k1 tx to chain 40204 → receipt.
  - Acceptance: ecrecover(signature) == wallet address; a 40204 tx hash confirmed by block
    inclusion (data source: 40204 RPC receipt). This is the proof the whole
    keystore→ceremony→sign→broadcast path works.
- [ ] **B1.5 — adversarial + integration suite (@rule8 evidence).** Below.

## Adversarial test plan (Rule 8 — red-test-first)
| # | Attack / property | Expected | Data source |
|---|---|---|---|
| ADV-1 | sign without ceremony approval | impossible — no code path signs unapproved | signer entry |
| ADV-2 | any invoke command returns key/keystore bytes | none does (enumerate commands) | command registry |
| ADV-3 | sign while the vault is locked | fail closed | session state |
| ADV-4 | key plaintext on disk | `custody.enc` holds only ciphertext | raw-bytes grep |
| ADV-5 | malicious origin spoofs a benign intent (decode/display) | the ceremony shows the TRUE origin + decoded action; undecodable calldata blocks approve without explicit raw-mode ack | intent decode |
| ADV-6 | Approve is default-focused / one-click | Approve is NOT pre-selected/default-focused | UI |
| ADV-7 | a sidecar/agent signs directly (bypass ceremony) | impossible — the signer is in-process, gated on ceremony approval; node-agent only emits unsigned requests | signer gating |
| ADV-8 | key material in logs / errors / `Debug` | absent (redacted) | log buffer |
| ADV-9 | key survives after signing (not zeroized) | zeroized after each signature | zeroize/drop |
| ADV-10 | duplicate/replay a single approval to sign twice | one approval → one signature | ceremony state |

## Integration test plan
- Full lifecycle: create wallet → key in the A2 vault → unlock → ceremony approve → sign a
  message → ecrecover == address; import BIP39 vector → address matches; sign + broadcast a
  40204 tx → receipt. Against a testnet (or a local node fixture) — no faked broadcast.

## Locked / open decisions
- **D-B1-1 (open, owner):** land the A2 F-1 hardening as B1.0 before a wallet key goes in
  (recommended) vs accept FS-only anchor with the BIP39 seed as documented recovery.
- **D-B1-2 (locked):** classic **secp256k1** signing for 40204 in B1; ERC-4337 UserOps +
  paymaster are the follow-on (blocked on the E-8 remediation + bundler).
- **D-B1-3 (locked):** reuse `citrate-wallet-core` keystore crypto (Rule 9); no
  reimplementation. Cross-repo dep via Rule-12 [[drift]] first.

## Test-count baseline (Rule 2)
| Date | Suite | Count | Command |
|---|---|---|---|
| 2026-07-13 | Rust | 73 | `cargo test --workspace --locked` (post-A3) |
| 2026-07-13 | Frontend | 30 | `npm run test` (post-A3) |
| 2026-07-13 | Rust | 76 | `cargo test --workspace --locked` (post-B1.0: +3 F-1 tests) |
| 2026-07-13 | Frontend | 30 | `npm run test` (unchanged — B1.0 is Rust-only) |
| 2026-07-12 | Rust | 82 | `cargo test --workspace --locked` (post-B1.0b: +6 F-1b tests) |
| 2026-07-12 | Frontend | 30 | `npm run test` (unchanged — B1.0b is Rust-only) |
| 2026-07-13 | Rust | 94 | `cargo test --workspace --locked` (post-B1.1: +12 wallet keystore tests) |
| 2026-07-13 | Frontend | 30 | `npm run test` (unchanged — B1.1 is Rust-only) |

## Definition of done
- The wallet keystore is integrated (citrate-wallet-core), the key lives only in the A2
  vault (never on disk in the clear, never across the invoke boundary, never in logs), and
  is unwrapped only in-process while unlocked and zeroized after each signature.
- The SignatureCeremony is the ONE signing path: every signature intent (all origins)
  routes through it; nothing signs without explicit human approval; Approve is not
  default-focused; the wagmi connector routes JS signing to it.
- Proven end-to-end: a real 40204 tx signed through the ceremony and confirmed on-chain.
- Every ADV-* lands red-first then green; the integration lifecycle passes.
- Gates green: typecheck, vitest, build, cargo test/fmt/clippy `-D warnings`, audit; counts up.
- **@rule8 gate:** adversarial evidence filed; **independent** security sign-off (build
  agent builds-and-stops; review is a separate read-only party — the session's standing
  lesson). Consumes A2 (+ its F-1 hardening if landed).
- CLAUDE.md rule 3 updated: the SignatureCeremony now exists; signing outside it is forbidden.
- Honest gap note for anything not runnable headless (real keyring, interactive approval UI).

## Out of scope (later)
- ERC-4337 UserOps + paymaster (CORE-S2.3, follow-on). Live balance/vitals reads (B2).
  Send UX end-to-end (B3). Guardian/social recovery, hardware wallet, Ed25519 agent-signing.

## Daily updates
### Day 0 — 2026-07-13 (scoped)
- Scoped from CORE-BETA Phase B1, after A2 (custody) + A3 (identity) merged. B1 is the
  signing-key custody spine and lifts Rule 3 (the ceremony becomes the one signing path).
  The A2 F-1 hardening decision (D-B1-1) is the gating owner call before a wallet key goes
  in the vault. Awaiting go to dispatch (build-and-stop, then independent review).

### Day 1 — 2026-07-13 (B1.0 built — build-and-stop)
- Owner approved landing F-1 hardening as B1.0. Built on `feat/core-b1-0-anchor-init`.
- Closed A2 F-1 (anchor-deletion downgrade) with a master-sealed `anchor_initialized`
  bit on `LockoutState`. Set once at first-init; enforced (`enforce_anchor_initialized`)
  on load/unlock, every `custody_get`, and before every `put`/`clear_slot`: anchor ABSENT
  + bit set ⇒ `Corrupt`. Init now seeds the gen-0 keyring anchor unconditionally so the
  anchored state is real from the first unlock. 3 red-first F-1 tests added (verified they
  fail with the guard neutralized). No A2 sign-off property regressed (DR-1/DR-2 anchors,
  I-2 boundary, DEK-sealed header, DR-3 all still green).
- Gates: `cargo test --workspace --locked` 76 passed (up from 73); `cargo fmt --check`
  clean; `cargo clippy --workspace --all-targets --locked -- -D warnings` clean;
  `cargo audit` 0 vulnerabilities (17 pre-existing allowed warnings, unrelated);
  `npm run typecheck` / `test` (30) / `build` clean. Cargo.lock unchanged (no new deps).
- STOP for independent delta re-review (author ≠ reviewer). Not self-signed off.
- Honesty: the real OS keyring cannot run headless; the whole attack (anchors + master)
  runs against the injected FakeKeyring. `real_keyring_roundtrip_or_skip` still
  honestly skips on headless CI.

### Day 3 — 2026-07-13 (B1.1 built — wallet keystore into the A2 vault, build-and-stop)
- Owner go received; branched `feat/core-b1-1-keystore` off main. Rule 12 [[drift]]
  landed FIRST in `citrate-federation/manifest.toml` (citrate-core → citrate-wallet-core,
  pinned `55f284b`), then the Cargo dep.
- **WP-0:** added `citrate-wallet-core` as a git dep pinned to citrate-chain `55f284b`
  (subdir `wallet-core`). Its transitive `citrate-security = { path = "../core/security" }`
  resolves inside the git checkout (verified — `cargo build` links). **Concern:**
  `default-features = false` does NOT build at `55f284b` (`types.rs::default_keystore_path`
  uses `dirs::` unconditionally while `dirs` is `native`-gated), so the default `native`
  feature is on, pulling tokio/reqwest/citrate-execution(ark-*)/etc. even though Option A
  needs only the stateless crypto path. That native chain (ark-relations) brings a NEW
  `cargo audit` advisory RUSTSEC-2025-0055 (tracing-subscriber 0.2.25, ANSI log-injection,
  LOW; not reachable from the keystore path). Flagged for the reviewer — the upstream fix is
  a `crypto`-only wallet-core feature; NOT acted on (out of scope, would move the pin).
- **D-B1.1-3 decided: seal the raw BIP39 ENTROPY** (smallest 32-byte canonical unit,
  re-derivable, loss-less ⇄ mnemonic; seed/mnemonic-string alternatives are larger and put
  more/human-readable secret at rest). Documented in `wallet.rs`.
- **WP-1/2/3 in `src-tauri/src/wallet.rs`:** create (24-word BIP39 → BIP44 secp256k1
  `m/44'/60'/0'/0/0` → seal entropy in the backend-reserved `wallet-entropy-0` slot; returns
  address+pubkey + the mnemonic once for backup); import (canonical vector →
  `0x9858EfFD232B4033E47d90003D41EC34EcaEda94`); in-process sign round-trip (ecrecover ==
  address). The `wallet-` reserved prefix EXTENDS custody's `oidc-` guard
  (`BACKEND_SLOT_PREFIXES`), so an invoke `custody_put("wallet-…")` is rejected (ADV-R).
  All ops require the vault UNLOCKED and fail closed when locked; secrets zeroized after use;
  NO `#[tauri::command]` returns secret material; NO `keys.json` written.
- **12 adversarial + integration tests** (`wallet_tests.rs`), red-first with negative
  controls: ADV-R (remove `wallet-` → attack passes → restore → green, verified); ADV-4
  (write `keys.json` in vault dir → assertion fails → restore → green, verified); ADV-2
  (non-`#[tauri::command]` fns can't even enter `generate_handler!` — compile-time barrier
  confirmed); ADV-3/9/S covered. F-1/F-1b anchor hardening UNDISTURBED (all A2 tests green).
- Gates: `cargo test --workspace --locked` **94 passed** (up from 82); `cargo fmt --check`
  clean; `cargo clippy --workspace --all-targets --locked -- -D warnings` clean; `npm run
  typecheck`/`test` (30)/`build` clean. `cargo audit`: 1 advisory (RUSTSEC-2025-0055, new via
  native-feature ark chain, see WP-0 concern) + 21 allowed unmaintained warnings.
- STOP for independent review (author ≠ reviewer). Not self-signed off. Honesty: custody is
  proven via the injected FakeKeyring (a live OS keyring can't run headless); the interactive
  one-time backup-DISPLAY UI is not headless-testable (B1.2+) — stated, not faked.

### Day 2 — 2026-07-12 (B1.0b built — F-1b close, build-and-stop)
- The independent delta re-review of #11/`92ccf95` filed **F-1b (MEDIUM)**: the B1.0
  `#[serde(default)] anchor_initialized: bool` cannot distinguish a LEGACY pre-B1.0 block
  (field ABSENT → serde-default `false`) from a genuine post-B1.0 first-init, so a legacy,
  validly-master-sealed block re-opens the F-1 delete-then-rollback resurrection with **no
  master key**, and a READ-ONLY vault (A3 OIDC-refresh) never self-healed.
- Fixed on `feat/core-b1-0b-legacy-anchor` (stacked on B1.0). Field is now
  `Option<bool>` (`None`=legacy, `Some(false)`=genuine first-init, `Some(true)`=anchored).
  `enforce_anchor_initialized` is now a three-way decision that takes `&env`/`&mek` and
  SELF-HEALS a legacy+anchor-present block (re-stamp `Some(true)`, re-seal) on the
  **read-only** path (unlock AND `custody_get`), and fails closed (`Corrupt`) on
  legacy+anchor-absent. `put_inner`/`clear_slot` carry the heal into their save so a
  mutation cannot clobber it back. 6 red-first F-1b tests added.
- Red-first proven two ways: neutralizing the `None`+absent ⇒ `Corrupt` arm makes
  `adv_f1b_..._fails_closed` pass through (`unlock=Ok`, attack succeeds); neutralizing the
  `None`+present self-heal fails all three self-heal tests. Both restored → green.
- No regression: all B1.0 F-1 tests (`adv_f1_*`) + DR-1/DR-2/CRY-1/DR-3/I-2 boundary still
  green; the A2 sign-off properties hold (the reviewer's no-regression table).
- Gates: `cargo test --workspace --locked` **82 passed** (up from 76); `cargo fmt --check`
  clean; `cargo clippy --workspace --all-targets --locked -- -D warnings` clean; `cargo
  audit` 0 vulnerabilities (17 pre-existing allowed warnings, unrelated); `npm run
  typecheck` / `test` (30) / `build` clean. Cargo.lock unchanged (no new deps).
- STOP for independent review (author ≠ reviewer). Not self-signed off. Same honesty
  scope: the real OS keyring can't run headless — every anchor/master/legacy probe runs
  against the injected FakeKeyring; `real_keyring_roundtrip_or_skip` still skips on CI.

## CLOSED — 2026-07-13 (PHASE B COMPLETE, signed off)
B1.0 (F-1/F-1b vault hardening) · B1.1.0 (BIP44 HD, upstream) · B1.1-F-1 (lean crypto build) ·
B1.1 (keystore in vault) · B1.2 (SignatureCeremony, Rule 3 lifted) · B1.3 (wagmi connector) ·
B1.4.0 (lean EIP-155 signer, upstream) · B1.4 (real 40204 broadcast — confirmed tx
0xd40ce789… @ block 380644) · B1.5 (full adversarial suite). Independent Phase-B custody
SIGN-OFF: citrate-security #24 (SIGNED OFF WITH CONDITIONS — all non-blocking: zeroize
byte-wipe before scale, ADV-matrix enumeration for S7.5, audit-baseline). Rust 141 tests,
audit 0. The custody/auth/signing spine is complete and independently reviewed at every step.
Next: Phase C (sprint-c1-supervisor).
