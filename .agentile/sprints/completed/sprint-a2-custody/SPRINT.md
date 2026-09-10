---
created: 2026-07-12
branch: docs/sprint-a2-custody
author: Claude Fable 5, directed by @SaulBuilds
status: completed
sprint: CORE-A2
planset: citrate-federation/.agentile/planset/2026-07-12-core-beta-wiring/00_STATE_AND_PLAN.md (Phase A2)
rule8: yes — touches key custody; security sign-off required before real key material (B1 wallet key, A3 OIDC tokens) relies on it
depends_on: CORE-A1 (the bridge) — landed
---

# Sprint CORE-A2 — OS-keyring custody service (the vault everything secret lives in)

Protocol (Agentile): Rule 1 — no mocks presented as live; Rule 2 — test count monotone,
recorded below; Rule 4 — this file is the authoritative status; Rule 5 — frontmatter on
every doc; Rule 8 — this sprint is key custody, so it ships with an adversarial suite and
carries a security-sign-off gate before any real key relies on it; Rule 11 — every
acceptance criterion names its data source. **Red-test-first is mandatory here:** each
custody guard lands as a failing adversarial test first, then the guard, then green.

## Why A2, and what it is NOT

A2 builds the **custody primitive**: the encrypted, OS-keyring-backed vault where every
secret will live — OIDC refresh tokens (A3), the wallet keystore (B1), gateway keys.
It is the on-chain-custody invariant (I-2) made real in the native backend: **keys live
only in the app process and the OS keyring; nothing else — no sidecar, daemon, log, or
frontend — ever holds key material.**

A2 is deliberately **not**: signing (CLAUDE.md rule 3 — no signing code paths until the
SignatureCeremony in B1); wallet-key generation/import (B1); OIDC token storage (A3
consumes A2); any surface beyond the minimal lock-state the Settings "Keys & security"
section already renders. A2 delivers the vault and proves it with a generic secret slot,
adversarially. A3 and B1 put real secrets in it.

## Design

- **Rust `custody` module (`src-tauri/src/custody.rs`)**, extending the A1 command idiom
  (`#[tauri::command] … -> Result<T, String>`, honest `Err` on the rejection path):
  - **Master key in the OS keyring** under service `ai.citrate.core`, account
    `custody-master-key` (the `keyring` v3 crate already in deps: apple-native /
    windows-native / sync-secret-service). The keyring holds the wrapping key; the
    envelope holds the secrets.
  - **Encrypted-at-rest envelope** on disk in the app data dir (`custody.enc`): each
    secret slot sealed with **AES-256-GCM**, the data key derived by **Argon2id
    (m=65536 KiB, t=3, p=1, 32-byte output)** from the user passphrase, matching the
    citrate-native / citrate-wallet-core keystore parameters exactly (Rule 9
    consistency — see DECISION D-A2-1). No passphrase hash stored: unlock is
    trial-decrypt of a known check-slot, GCM tag rejects a wrong passphrase.
  - **Session model:** `unlock(passphrase)` derives + holds the data key in memory for
    the session; `lock()` and the auto-lock timer (from `config.autolock`, the single
    source of truth A1 wired) drop it; `zeroize` on every secret buffer and on lock.
  - **Lockout:** N failed unlocks → cooloff (citrate-native pattern: 5 attempts →
    5-minute lock), constant-time tag check, no error oracle distinguishing "wrong
    passphrase" from "no vault".
  - **API (no signing):** `custody_status()`, `custody_init(passphrase)`,
    `custody_unlock(passphrase)`, `custody_lock()`, `custody_put(slot, bytes)`,
    `custody_get(slot)` (in-process only; see the boundary rule), `custody_list()`
    (slot metadata only, never secret bytes), `custody_keyring_status()`.
- **Bridge `custody` domain** (extends A1's set): `status / init / unlock / lock /
  listSlots` — status and metadata only. **The bridge custody domain never returns
  secret bytes to the frontend.** `custody_get` is an internal Rust API for A3/B1
  consumers within the process, not an `invoke` command exposed to JS.
- **Sim (web) adapter:** simulates lock/unlock *UI state only* — it holds no real
  secret and stores nothing (the prototype's Keys-&-security section still renders).
  Guarded out of packaged builds (A1's `assertSimAllowed`).

## WP checklist

- [x] **A2.1 — Keyring master-key service.** put/get/delete the wrapping key in the OS
  keyring under `ai.citrate.core / custody-master-key`.
  - Acceptance: round-trips against the real platform keyring (data source: OS keyring
    entry, via the `keyring` crate); absent-entry and locked-keyring paths fail closed.
  - **DONE.** `OsKeyring` impls `get/set/delete` via `keyring` v3 (`get_secret`/`set_secret`/
    `delete_credential`); `NoEntry` → `None`, any other backend error → `KeyringUnavailable`
    (fail closed). Abstracted behind the `Keyring` trait so tests use an in-memory fake AND
    the real backend. Real round-trip in `real_keyring_roundtrip_or_skip` (honestly skips on
    headless — see gaps).
- [x] **A2.2 — Envelope + Argon2id/AES-GCM seal/unseal (red-test-first).**
  - **DONE.** `Envelope { version, salt, wrapped_dk, slots }` in `custody.enc`. Argon2id
    (m=65536, t=3, p=1, 32B — asserted in `argon2_params_match_decision_d_a2_1`) derives the
    KEK; a random data key (DEK) seals every slot under AES-256-GCM; the DEK is itself wrapped
    under `KEK ⊕ keyring-master-key` so **both** the keyring entry and the passphrase are
    required (keyring made genuinely load-bearing). NO-PLAINTEXT proven by `adv4` (raw-bytes
    **and** serde number-array grep).
- [x] **A2.3 — Session unlock / lock / auto-lock / lockout (red-test-first).**
  - **DONE.** `unlock` recovers the DEK (keyring+passphrase) and trial-decrypts the check-slot;
    wrong passphrase / no vault / rotated master all fail closed as opaque `Denied` (no error-
    string oracle, GCM tag is the only signal). 5-attempt → 5-min cooloff lockout. Auto-lock reads
    `config.autolock` (seeded from the A1 store on setup) on a monotonic `Instant`.
  - **HARDENED post-review (citrate-security #13 / Day 2):** the lockout is now serialized under
    the mutex (concurrency, BND-1) AND persisted integrity-bound in the envelope (restart,
    CRY-3/BND-2); the absent-vault path spends a dummy Argon2id (timing parity, CRY-2). Auto-lock
    remains on-access / lazy (no background timer yet — BND-6, lands before B1). See Day 2.
- [x] **A2.4 — Zeroization + boundary invariant (red-test-first).**
  - **DONE.** Data key is `Zeroizing<[u8;32]>` with an explicit `Session::drop` wipe; the final
    owned Rust passphrase + `put` bytes copies are zeroized after use. Boundary proven twice:
    `adv8` (domain has no secret-read op) + the Rust `no_custody_invoke_command_returns_secret_bytes`
    test asserting the real `lib.rs` registration never lists `custody_get`.
  - **Honesty corrections (citrate-security #13):** the ADV-9 zeroize test proves the SESSION-
    TEARDOWN + `Zeroizing` TYPE contract, NOT a physical memory wipe (deferred to a zeroize-audit
    MIR/LLVM pass before B1 — BND-4). Passphrase zeroization covers the owned Rust copy only; the
    JS webview string + serde/IPC intermediates are un-scrubbed (BND-5) — "zeroized on every path"
    is corrected. See Day 2.
- [x] **A2.5 — Bridge `custody` domain + Settings wire.** status/init/unlock/lock/
  listSlots through the bridge; Settings "Keys & security" lock state + auto-lock read
  real custody status in a Tauri build; sim shim for web.
  - **DONE.** `custody` domain (status/init/unlock/lock/listSlots) — metadata only, never
    bytes. Tauri adapter invokes the real commands; sim adapter simulates lock/unlock UI state
    only (holds no secret, stores nothing), guarded by `assertSimAllowed`. Settings
    Keys-&-security shows a live "Custody vault · Session lock" row driven by
    `custody_status` (real in Tauri, sim shim on web); the prototype still renders.
- [x] **A2.6 — Adversarial + integration suite (the Rule-8 evidence).** All ten ADV-* landed
  red-first then green (table below); integration lifecycle across a simulated restart passes.

## Adversarial test plan (Rule 8 — write these FIRST, they must fail before the guard)

| # | Attack / property | Expected | Data source |
|---|---|---|---|
| ADV-1 | `custody_get` while locked | denied, no bytes | session state |
| ADV-2 | wrong passphrase | unlock fails; no distinguishing oracle vs empty-vault | GCM tag |
| ADV-3 | N wrong attempts | lockout/cooloff engages | attempt counter |
| ADV-4 | plaintext on disk | `custody.enc` has zero secret plaintext | raw-bytes grep of the file |
| ADV-5 | tampered ciphertext/tag | unseal fails closed, no partial | AES-GCM auth |
| ADV-6 | keyring master key absent/rotated | unseal fails closed | keyring probe |
| ADV-7 | secret in logs | no secret material in captured logs | log buffer |
| ADV-8 | frontend can read a secret | no `invoke` command returns secret bytes | command registry enumeration |
| ADV-9 | zeroization | data key + secret buffers wiped on lock/drop | zeroize + drop test |
| ADV-10 | auto-lock bypass (clock) | session expires per `config.autolock`; no extension by rollback | monotonic clock |

## Integration test plan

- Full lifecycle across a simulated restart: `init` → `put(slot, s)` → `lock` →
  `unlock(right)` → `get(slot) == s`, with the keyring master key + on-disk envelope
  persisting (the round-trip A3/B1 depend on).
- keyring put/get/delete round-trip against the real platform backend (skipped-with-note
  on headless CI if the CI runner has no keyring — honest, not faked).
- `config.autolock` change takes effect on the session timeout.

## Locked decision

- **D-A2-1 — crypto parameters.** Match citrate-native / citrate-wallet-core exactly:
  Argon2id (m=65536, t=3, p=1, 32-byte), AES-256-GCM, `zeroize`. Rationale: Rule-9
  consistency across the federation's keystores; a member's secrets behave identically
  in citrate-core and citrate-native. **A2 implements the general secret vault with
  these params; B1 brings in `citrate-wallet-core` for the actual wallet keystore**
  (cross-repo dep + Rule-12 `[[drift]]` entry deferred to B1). Open for security review.

## Test-count baseline (Rule 2 ratchet)

| Date | Suite | Count | Command |
|---|---|---|---|
| 2026-07-12 | Rust | 7 passed | `cargo test --workspace --locked` (post-A1) |
| 2026-07-12 | Frontend | 13 passed | `npm run test` (post-A1) |
| 2026-07-12 | Rust | **23 passed** (+16) | `cargo test --workspace --locked` (post-A2) |
| 2026-07-12 | Frontend | **20 passed** (+7) | `npm run test` (post-A2) |
| 2026-07-12 | Rust | **32 passed** (+9) | `cargo test --workspace --locked` (post-remediation, envelope v2) |
| 2026-07-12 | Frontend | **20 passed** (+0) | `npm run test` (remediation is Rust-only) |
| 2026-07-12 | Rust | **38 passed** (+6) | `cargo test --workspace --locked` (post-delta-remediation, DR-1..DR-5) |
| 2026-07-12 | Frontend | **20 passed** (+0) | `npm run test` (delta remediation is Rust-only) |

### ADV-* red→green evidence (each guard neutralized → test RED, guard restored → GREEN)

| # | Property | Guard neutralized to force RED | Verified |
|---|---|---|---|
| ADV-1 | locked `custody_get` denied | skipped the session gate, returned raw ct | `adv1` RED→GREEN |
| ADV-2 | no wrong-vs-empty ERROR-STRING oracle (see correction) | `load_envelope` leaked "no vault" on the no-file path | `adv2` RED→GREEN |
| ADV-3 | N-attempt lockout, in-process only (see correction) | dropped the failure-counter increment | `adv3` RED→GREEN |
| ADV-4 | zero plaintext on disk | wrote plaintext into the slot ct field | `adv4` RED→GREEN (raw + number-array grep) |
| ADV-5 | tampered ct fails closed | fell back to raw ct on GCM-tag failure | `adv5` RED→GREEN |
| ADV-6 | keyring absent/rotated fails closed | synthesized a fixed master key when absent | `adv6` RED→GREEN |
| ADV-7 | no secret in logs/errors | echoed the passphrase into the error string | `adv7` RED→GREEN |
| ADV-8 | no invoke returns secret bytes | registered a `custody_get` command / added a bridge `get()` | Rust `no_custody_invoke…` + FE `adv8` RED→GREEN |
| ADV-9 | session teardown + Zeroizing TYPE contract (see correction) | `lock()` no longer dropped the session | `adv9` RED→GREEN |
| ADV-10 | auto-lock, monotonic clock | `expire_if_stale` never expired | `adv10` RED→GREEN |

**Claim corrections (from the citrate-security #13 Rule-8 review — the original A2 claims were stronger than the code supported):**

- **ADV-2 correction.** ADV-2 proves only that the wrong-passphrase and no-vault ERROR STRINGS are identical. It is NOT a whole-path constant-time claim: pre-remediation, the no-vault path returned before any KDF while wrong-passphrase ran a full Argon2id, so vault existence was a ~19,680× TIMING oracle. Remediated under CRY-2 (dummy Argon2id on the absent-vault path); the new `adv_cry2_absent_vault_spends_kdf` test proves KDF parity structurally.
- **ADV-3 correction.** ADV-3 proved only an IN-PROCESS, single-threaded counter. It did NOT cover (a) concurrent unlocks (the pre-check released the mutex before the crypto → all trials ran) or (b) process restart (the counter was in-memory only → a restart reset it). Both were live bypasses. Remediated under BND-1 (mutex held across the whole unlock) + CRY-3/BND-2 (lockout persisted, integrity-bound, in the envelope). New tests: `adv_bnd1_concurrent_unlock_lockout_holds`, `adv_cry3_lockout_persists_across_restart`.
- **ADV-9 correction.** ADV-9 proves the SESSION-TEARDOWN + `Zeroizing` TYPE contract (lock drops the session; the DEK is a `Zeroizing<[u8;32]>` with an explicit `Session::drop` wipe). It does NOT prove the DEK bytes are physically erased from memory (reading post-drop memory is UB, which the test declines). A physical memory-residue proof is DEFERRED to a zeroize-audit MIR/LLVM pass before B1 stores real wallet keys (BND-4). The test is renamed `adv9_session_teardown_and_zeroizing_contract` and a code comment on `Session::drop` records the deferral.

### Remediation adversarial tests (citrate-security #13 — each RED-first: passes the attack on the pre-fix code, fails it after)

| # | Attack / property | Finding | Guard neutralized to force RED | Verified |
|---|---|---|---|---|
| ADV-11 | slot swap on disk | CRY-1 | header fingerprint check skipped + constant AAD | `adv_cry1_slot_swap_fails_closed` RED→GREEN |
| ADV-12 | slot rollback-in-place | CRY-1 | header fingerprint check skipped | `adv_cry1_slot_rollback_fails_closed` RED→GREEN |
| ADV-13 | slot remove behind header | CRY-1 | header fingerprint check skipped | `adv_cry1_slot_remove_fails_closed` RED→GREEN |
| ADV-14 | version downgrade/tamper | CRY-4 | version check + version-in-AAD disabled | `adv_cry4_version_downgrade_rejected` RED→GREEN |
| ADV-15 | timing parity (absent-vault spends KDF) | CRY-2 | dummy KDF on absent path removed | `adv_cry2_absent_vault_spends_kdf` RED→GREEN |
| ADV-16 | concurrent-unlock lockout holds | BND-1 | mutex released before the crypto | `adv_bnd1_concurrent_unlock_lockout_holds` RED→GREEN |
| ADV-17 | lockout persists across restart | CRY-3/BND-2 | lockout not persisted to envelope | `adv_cry3_lockout_persists_across_restart` RED→GREEN |
| ADV-18 | concurrent puts both survive | BND-3 | mutex released before load→insert→save | `adv_bnd3_concurrent_puts_both_survive` RED→GREEN |
| ADV-19 | keyring master not clobbered on re-init | CRY-5 | mint guard disabled | `adv_cry5_no_master_clobber_on_reinit` RED→GREEN |

### Delta-review remediation tests (citrate-security #13 / `01_DELTA_REVIEW.md` — Day 3; each RED-first against the pre-fix code, GREEN after)

| # | Attack / property | Finding | Guard neutralized to force RED | Verified |
|---|---|---|---|---|
| ADV-20 | whole-envelope rollback (gen-N file restored over gen-N+2) | DR-1 | generation counter was dead code (`let _generation`), no external anchor | `adv_dr1_whole_envelope_rollback_fails_closed` RED→GREEN (→`Corrupt`) |
| ADV-21 | mid-live-session whole-envelope rollback | DR-1 | `custody_get` re-checked only the internally-consistent header | `adv_dr1_mid_session_rollback_fails_closed` RED→GREEN |
| ADV-22 | high-water advances + honest restart still unlocks | DR-1 | no keyring anchor written/read at all | `adv_dr1_high_water_advances_and_persists` RED→GREEN |
| ADV-23 | lockout block rollback (pristine block over high-failure) | DR-2 | lockout block had no anchor | `adv_dr2_lockout_rollback_detected` RED→GREEN |
| ADV-24 | reserved domain-tag slot names (`header`/`wrapped_dk`/`lockout`) | DR-3 | `put_inner` accepted any non-NUL name | `adv_dr3_reserved_slot_names_rejected` RED→GREEN |
| ADV-25 | stale lockout after cooloff-then-success | DR-4 | elapsed-deadline success skipped the disk write | `adv_dr4_cooloff_success_clears_lockout_on_disk` RED→GREEN |

**Whole-set-rollback claim correction (DR-1).** The Day-2 note claimed the DEK-sealed header
"catches whole-set swap/rollback," and separately deferred whole-envelope rollback as "out of
scope for A2." The delta review proved the header's `generation` counter was **dead code**
(`let _generation = …`, read and discarded) — it provided ZERO anti-rollback protection, so a
restore of an entire older, internally-consistent `custody.enc` unlocked fine and resurrected
the OLD (revoked) secret while silently dropping newer slots. The header alone catches only
INDIVIDUAL-slot swap/rollback/add-remove (the on-disk `name→nonce` fingerprint diverges); a
WHOLE-envelope rollback needs an anchor OUTSIDE the attacker-writable file. The code comments
(`Envelope` integrity model + `Header` doc) are corrected to say exactly this, and whole-envelope
rollback is now IN scope and CLOSED via the DR-1 keyring high-water anchor (below), not deferred.

### Boundary proof (@rule8)

Registered custody `invoke` commands (all status / `()` / metadata — **none return secret bytes**):
`custody_status`, `custody_init`, `custody_unlock`, `custody_lock`, `custody_put`, `custody_list`,
`custody_keyring_status`. `custody_get` is a plain in-process `pub fn` for A3/B1 — deliberately
**not** a `#[tauri::command]`, asserted by `lib.rs::no_custody_invoke_command_returns_secret_bytes`.

### Gate results (post-A2)

`npm run typecheck` clean · `npm run test` 20 green · `npm run build` OK ·
`cargo test --workspace --locked` 23 green · `cargo fmt --check` clean ·
`cargo clippy --workspace --all-targets --locked -- -D warnings` clean · `cargo build` OK ·
`cargo audit` — 0 vulnerabilities; new crates argon2 0.5.3 / aes-gcm 0.10.3 / zeroize 1.9.0 /
rand 0.8.7 all clean (pre-existing unmaintained/unsound warnings are in the Tauri/GTK
transitive tree only).

### Gate results (post-remediation, envelope v2 — citrate-security #13)

`npm run typecheck` clean · `npm run test` 20 green · `npm run build` OK ·
`cargo test --workspace --locked` **32 green (+9)** · `cargo fmt --check` clean ·
`cargo clippy --workspace --all-targets --locked -- -D warnings` clean · `cargo build` OK ·
`cargo audit` — 0 vulnerabilities (same pre-existing GTK/atk unmaintained warnings only; no
new crates added — the fix is pure Rust logic + a serde-only envelope format bump).

## Definition of done

- The custody vault exists: keyring-backed master key + Argon2id/AES-GCM envelope +
  session unlock/lock/auto-lock/lockout + zeroization.
- Every ADV-* test lands red-first then green; the integration lifecycle passes across a
  simulated restart.
- The boundary holds: no secret bytes cross the `invoke` line; sim path unreachable in
  packaged builds; the web prototype still renders Keys-&-security.
- Gates green: typecheck, `vitest run`, `vite build`, `cargo test`/fmt/clippy `-D warnings`;
  Rule-2 counts recorded and up.
- **@rule8 gate:** the adversarial evidence bundle is filed and security sign-off is
  requested before A3/B1 store any real secret in the vault. citrate-core is T1; this is
  the custody surface an external auditor will look at first.
- Honest gap note if an interactive Tauri/keyring pass can't run headless.

## Out of scope (later)

- Signing of any kind (B1 SignatureCeremony). Wallet key gen/import (B1). OIDC token
  storage (A3 — consumes this vault). Guardian/social recovery (later). Hardware wallet.

## Daily updates

### Day 0 — 2026-07-12 (scoped)
- Sprint defined from CORE-BETA Phase A2. Extends the A1 command idiom (`keyring` v3 +
  `tauri-plugin-store` already in deps; the `seam.rs` honest-`Err` pattern). Red-test-first
  adversarial suite specified as the Rule-8 evidence. Awaiting go to dispatch A2.1.

### Day 1 — 2026-07-12 (built, all WPs closed)
- **Landed the full vault.** `src-tauri/src/custody.rs` + `custody_tests.rs`: `Keyring` trait
  seam (`OsKeyring` real / in-memory fake in tests), Argon2id(D-A2-1)/AES-256-GCM envelope,
  session unlock/lock/auto-lock/lockout, zeroization, `custody_get` in-process-only. Bridge
  `custody` domain + Tauri/sim adapters + Settings Keys-&-security wire. All 10 ADV-* landed
  red-first then green; integration lifecycle across a simulated restart passes.
- **Design refinement (worth flagging for review):** the spec sketched "data key derived from
  the passphrase" and separately "keyring holds the wrapping key." As first written, unlock
  re-derived the DEK from the passphrase alone, so the keyring master key was **not**
  load-bearing (ADV-6 only held on a secondary path). Reworked to `DEK` sealed under
  `Argon2id(passphrase) ⊕ keyring-master-key` — now **both** the passphrase and the keyring
  entry are required; absent/rotated master → unlock fails closed even with the right
  passphrase. This is stronger and matches the intent; **open for security review** (the XOR
  key-combiner is sound for two independent 32-byte secrets but a reviewer may prefer an HKDF
  combiner — no sha2/hkdf dep was added to keep the crate surface minimal; flag if you want it).
- **Gates green** (counts above). Rule-2 ratchet up: Rust 7→23, FE 13→20.
- **Honest gaps.**
  1. **Real OS keyring / interactive Tauri not exercised here.** All crypto/envelope/session/
     lockout/zeroize/tamper logic is proven headless via the in-memory keyring fake (the
     injected trait). `real_keyring_roundtrip_or_skip` exercises the REAL platform keyring
     when reachable and **prints a SKIP note** otherwise (headless CI with no secret service) —
     not a faked pass (ch08 honesty). A live `tauri dev` unlock/lock walk on macOS/Windows/
     Linux is a manual verification step for the security reviewer, not automated here.
  2. **No `tracing`/log sink is wired in the backend yet** (A1 has none), so ADV-7 asserts the
     error/`Debug` render carries no secret rather than draining a live log buffer. When a
     log subscriber lands (later phase), extend ADV-7 to assert against the captured buffer.
  3. **Constant-time:** the GCM tag comparison is `aes-gcm`'s constant-time verify; there is no
     other secret-dependent branch on the unlock path. The lockout counter is not itself a
     timing oracle (it gates on a coarse `Instant`), but a reviewer may want a dedicated
     constant-time review of the whole unlock path (recommended before A3/B1 store real keys).
- **@rule8:** this evidence bundle (ADV table + boundary proof + gate results above) is filed;
  **security sign-off is requested before A3 (OIDC tokens) or B1 (wallet keystore) stores any
  real secret in the vault.** DO NOT MERGE the PR until that sign-off. citrate-core is T1.

### Day 2 — 2026-07-12 (Rule-8 remediation — citrate-security #13)

The adversarial Rule-8 review (`citrate-security/reviews/2026-07-12-rule8-a2-custody-vault/`,
two lenses: crypto-construction + boundary-lifecycle) returned **SIGN-OFF: NO** with 1 CRITICAL,
3 HIGH, 4 MEDIUM. The boundary invariant I-2 and the primitives (Argon2id D-A2-1, AES-256-GCM,
XOR combiner) were adversarially CONFIRMED sound; the failures were envelope integrity, a timing
oracle, and a two-way-bypassable lockout — a harden-in-place, not a rip-and-replace. All findings
remediated on this branch (PR #7), each code fix RED-test-first:

- **CRY-1 (CRITICAL) — envelope integrity → envelope v2.** Bumped `version = 2`. Each slot now
  seals with **per-slot AAD = `version || slot_name`** (and `version || "wrapped_dk"`), plus a
  **DEK-sealed authenticated header** that pins a monotonic **generation counter** and each
  slot's `name -> nonce` fingerprint. Unlock verifies the header BEFORE trusting any slot, so a
  slot swap, rollback-in-place, or add/remove fails closed. Tests: `adv_cry1_slot_swap…`,
  `…_slot_rollback…`, `…_slot_remove…` (all RED→GREEN). Documented limitation: a whole-envelope
  rollback to an internally-consistent prior state (old header + old ct together) is not
  detectable without an external anchor — noted for A3/B1, out of scope for A2.
- **CRY-2 (HIGH) — timing oracle.** The absent-vault unlock path now spends one dummy Argon2id
  (fixed dummy salt) before denying. Made testable structurally via an injectable KDF-invocation
  counter (`derive_key_counted`), not wall-clock: `adv_cry2_absent_vault_spends_kdf` (RED→GREEN).
- **BND-1 (HIGH) — lockout concurrency.** The vault mutex is now held across the ENTIRE
  check→derive→verify→record sequence (unlocks serialized; at most one Argon2id trial in flight;
  attempts counted as started). `adv_bnd1_concurrent_unlock_lockout_holds` (RED→GREEN): 32
  concurrent wrong guesses → ≤ MAX_ATTEMPTS KDF trials, the rest `LockedOut`.
- **CRY-3 / BND-2 (HIGH) — lockout restart.** Lockout state (`failures` + absolute unix-ms
  deadline) is sealed under the keyring MASTER key (so it is writable on a failed attempt, which
  has no DEK) and re-armed on load. `adv_cry3_lockout_persists_across_restart` (RED→GREEN).
  Accepted + documented clock-rollback limitation (an absolute deadline can be ended early by
  winding the wall clock back; Argon2id cost is the always-on brake).
- **BND-3 (MED) — put TOCTOU + non-atomic write.** All envelope mutations are serialized under
  the mutex; `save_envelope` writes temp + fsync + rename. `adv_bnd3_concurrent_puts_both_survive`
  (RED→GREEN).
- **CRY-4 (MED) — version validation.** `load_envelope` rejects any `version != 2` with an
  explicit `VersionUnsupported` (and the version is AAD-bound). `adv_cry4_version_downgrade_rejected`
  (RED→GREEN).
- **CRY-5 (MED) — keyring master clobber.** `mint_master_key` refuses to overwrite an existing
  keyring master (get-first → fail closed as `Corrupt`); "master present + envelope missing" is
  treated as tamper/recovery, not clean init. `adv_cry5_no_master_clobber_on_reinit` (RED→GREEN).
- **BND-4 (MED) — test honesty.** ADV-9 re-scoped to "session teardown + Zeroizing type contract"
  (renamed test + code comment on `Session::drop`); physical memory-wipe proof deferred to a
  zeroize-audit MIR/LLVM pass before B1. See the claim-corrections block above.

**Non-blocking, recorded honestly (docs, not code):**
- **BND-5** — passphrase IPC/serde residue: only the final owned Rust `String`/`Vec<u8>` is
  scrubbed. The JS webview string and the serde/IPC intermediate copies are NOT the buffer that
  gets zeroized — the earlier "inbound passphrase zeroized on every path" claim is **corrected**;
  full-path zeroization is not claimed. Longer-term: a `Vec<u8>` IPC arg to avoid the UTF-8
  `String` intermediate. Inherent to Tauri IPC, not introduced here.
- **BND-6** — auto-lock is ON-ACCESS (lazy): `expire_if_stale` fires on the next command, there
  is no background timer yet, so an idle-but-unlocked DEK stays resident until the next access. A
  spawned auto-lock timer lands before B1 stores wallet keys. (The monotonic-`Instant` guarantee
  — ADV-10, no wall-clock rollback extension — still holds.)
- **BND-7** — `custody_status` exposes `initialized`/`unlocked` (session state, not secret
  material). Mild state oracle, noted since the sprint stresses "no oracle." No change.

**Envelope v2 format (serde JSON):**
`Envelope { version:2, salt, wrapped_dk: SealedSlot, header: SealedSlot, lockout: SealedSlot,
slots: BTreeMap<name, SealedSlot> }`. `wrapped_dk` sealed under `Argon2id(pass) ⊕ master`
(AAD `2||"wrapped_dk"`). `header` sealed under the DEK (AAD `2||"header"`, plaintext =
`{generation, slots: {name->nonce}}`). `lockout` sealed under the keyring master (AAD
`2||"lockout"`, plaintext = `{failures, locked_until_ms}`). Every caller/check slot sealed under
the DEK with AAD `2||slot_name`. No new crates.

**Gates (post-remediation):** typecheck clean · FE test 20 green · build OK · `cargo test
--workspace --locked` **32 green (+9 from 23)** · fmt clean · clippy `-D warnings` clean ·
`cargo audit` 0 vulns. HONESTY: every fix proven headless via the injected fake keyring +
`cargo test` (a live keyring / interactive Tauri was NOT run headless; `real_keyring_roundtrip_or_skip`
still skips-with-note). Each of the 9 new tests was confirmed RED against the neutralized pre-fix
guard and GREEN after — see the remediation ADV table above.

- **@rule8:** remediation complete on the branch. This goes back to the two adversarial lenses for
  a **delta re-review** (same crypto + boundary lenses), then owner sign-off. Still **DO NOT MERGE**.

### Day 3 — 2026-07-12 (delta-review remediation — citrate-security #13 / `01_DELTA_REVIEW.md`)

The delta re-review (`citrate-security/reviews/2026-07-12-rule8-a2-custody-vault/01_DELTA_REVIEW.md`,
target commit `2a8f1aa`, envelope v2) returned **SIGN-OFF: NO — one more round**, dominated by
**DR-1** (whole-envelope generation rollback — the anti-rollback anchor the header rework promised
was inert). All findings remediated on this branch (PR #7), each code fix RED-test-first (the six
new tests each PASS the attack on the pre-fix code, then fail closed after the fix):

- **DR-1 (CRITICAL/HIGH — the blocker) — whole-envelope generation rollback.** The header's
  `generation` counter was dead code (`let _generation = …`, read + discarded), so restoring an
  entire older, internally-consistent `custody.enc` unlocked fine and resurrected the OLD secret
  while silently dropping newer slots. **Fix:** a **keyring high-water generation anchor** stored
  OUTSIDE the envelope — new keyring entry `ai.citrate.core / custody-generation` (8-byte BE u64),
  read/written through the same `Keyring` seam as the master. On **unlock** (`try_derive_session`,
  replacing the discarded binding) AND on **every `custody_get`**, any envelope whose header
  generation is **below** the high-water is rejected `Corrupt` (fail closed). The anchor is bumped
  to the new generation on every `put` (before the atomic save, so a crash fails closed — anchor
  ahead of envelope, not behind). First-init seeds it; a keyring read/write failure is a hard
  fault (`KeyringUnavailable`/`Corrupt`), never treated as "no anchor." Tests:
  `adv_dr1_whole_envelope_rollback_fails_closed` (snapshot gen-N token=OLD → rotate to gen-N+2 +
  add slot → restore whole gen-N file → unlock `Corrupt`, get errors, OLD never served),
  `adv_dr1_mid_session_rollback_fails_closed` (live unlocked session, file rolled back under it →
  next `get` fails closed), `adv_dr1_high_water_advances_and_persists` (honest restart still
  unlocks + reads; anchor monotone, not bumped by reads). The overstated `Envelope`/`Header`
  doc-comments ("catches whole-set rollback") are corrected — see the claim-correction block above.
- **DR-2 (MEDIUM) — lockout block rollback.** The master-sealed lockout had no anchor, so a plain
  file write of a pristine (failures=0) block over a high-failure one reset the throttle
  undetectably. **Fix:** a SEPARATE keyring counter `ai.citrate.core / custody-lockout-generation`
  (its own lane, because failed-unlock lockout writes advance on attempts that do NOT mutate the
  envelope, so sharing the DR-1 lane would push the envelope anchor past the header generation and
  brick the next legitimate unlock). Every lockout write advances this counter and stamps it into
  the block's new `generation` field; unlock rejects any lockout block whose `generation` is below
  the lockout high-water (pre-DEK, so it gates a wrong-passphrase attempt too). Test:
  `adv_dr2_lockout_rollback_detected`. **Documented limitation** (code + here): a DUMPED keyring
  master lets an attacker forge a fresh block at the current generation and remove the throttle —
  only Argon2id remains the per-guess brake in that case; a blind FS-only rollback is caught.
- **DR-3 (LOW) — AAD cross-field collision.** `put_inner` now rejects the reserved domain-tag names
  `header` / `wrapped_dk` / `lockout` (whose per-slot AAD `version||name` aliased the domain-field
  AAD), via `is_reserved_domain_tag`, keeping the per-slot AAD namespace disjoint from the domain
  tags. Test: `adv_dr3_reserved_slot_names_rejected`.
- **DR-4 (LOW) — stale lockout after cooloff-then-success.** An elapsed-deadline success skipped the
  disk write, leaving `{failures:MAX, locked_until:past}` on disk. **Fix:** a `cooloff_elapsed`
  flag forces the cleared lockout to be written unconditionally when the elapsed branch fired (and
  the cleared block carries the current lockout generation, not 0, so it does not trip the DR-2
  anchor guard on the next unlock). Test: `adv_dr4_cooloff_success_clears_lockout_on_disk`.
- **DR-5 (LOW — availability) — mutex poison bricks the vault.** Policy chosen: **recover and
  continue**. All `inner.lock()` / `autolock_secs.lock()` sites go through `lock_inner()` /
  `autolock_secs()` helpers that `unwrap_or_else(|e| e.into_inner())`. Safe here because every
  mutating op re-reads the envelope from disk (atomic temp+fsync+rename) and re-verifies the header
  + anchors, failing CLOSED on the *operation* if anything is inconsistent — a mid-op panic can no
  longer brick the *process* until restart. Rationale recorded in the `lock_inner` doc-comment.
- **DR-6 / DR-7 (INFO) — recorded, no code.** DR-6: persisted-lockout griefing needs FS write +
  keyring-master read (an attacker who already owns the vault — no added capability). DR-7: a stray
  `.enc.tmp` on a mid-write error never corrupts the live envelope (the atomic rename only replaces
  the target on success); best-effort cleanup deferred as optional, not blocking.

**Keyring anchor design (DR-1/DR-2), exact:** two new keyring entries under service
`ai.citrate.core`, both 8-byte big-endian `u64`, read/written through the injected `Keyring` seam
(so headless-testable via the fake, real `OsKeyring` in production, integrity-protected the same
way the master is): `custody-generation` (envelope high-water, DR-1) and
`custody-lockout-generation` (lockout high-water, DR-2). **Bump points:** `custody-generation` on
every `put` (to `header.generation + 1`, before save) and adopted-forward whenever a load observes
a higher generation; `custody-lockout-generation` on every lockout write (each failed unlock, each
success-clear, each cooloff-clear). **Checks:** `custody-generation` on unlock and on every
`custody_get` (envelope gen `< hw` → `Corrupt`); `custody-lockout-generation` on unlock before the
throttle is evaluated (lockout gen `< hw` → `Corrupt`). **First-init:** seeded at gen 0; a fresh
keyring simply has no anchor until the first advancing write. **Keyring-unavailable (honest fail
closed):** a read error → `KeyringUnavailable`, a malformed (wrong-length) entry → `Corrupt`, a
write error → hard fault — never silently treated as "no anchor," which would re-open the rollback.

**Gates (post-delta-remediation):** typecheck clean · FE test 20 green · build OK · `cargo test
--workspace --locked` **38 green (+6 from 32)** · fmt clean · clippy `-D warnings` clean · `cargo
audit` 0 vulns (no new crates — pure Rust logic + two keyring entries + a serde-defaulted
`generation` field on the lockout block). HONESTY: every fix proven headless via the injected fake
keyring (the high-water anchors live IN the fake in tests) + `cargo test`. A live OS keyring /
interactive Tauri was **NOT** run headless in this environment — `real_keyring_roundtrip_or_skip`
still skips-with-note; a live `tauri dev` unlock/lock/rollback walk on macOS/Windows/Linux remains
a manual verification step for the reviewer. Each of the 6 new tests was confirmed RED against
today's pre-fix code and GREEN after the fix.

- **@rule8:** delta remediation complete on the branch (PR #7). Goes back to the crypto lens for a
  final rollback-path re-attack (whole-envelope + mid-session), then owner sign-off. Still
  **DO NOT MERGE**. `Reconciliation: OPEN` for the ChatGPT quorum leg.

### Day 4 — 2026-07-12 (round-3 close — honest documentation of F-1 + checked_add hygiene)

The round-3 delta re-attack (`citrate-security/reviews/2026-07-12-rule8-a2-custody-vault/01_DELTA_REVIEW.md`)
surfaced **F-1 (anchor-deletion downgrade — MEDIUM)**: the DR-1/DR-2 keyring high-water anchors
defend a **filesystem-only** rollback, but `read_anchor` maps a *deleted/absent* keyring entry to
`Ok(None)`, and both guards (`enforce_high_water` + the lockout-generation check) treat `None` as
benign **first-init**. So an attacker who can DELETE the two anchor entries (`custody-generation`,
`custody-lockout-generation`) while LEAVING `custody-master-key` converts a whole-envelope rollback
back into a re-seed — re-opening revoked-secret resurrection (A3 OIDC) and throttle reset (DR-2).

**This is STRUCTURAL, not a bug.** The `None ⇒ re-seed` path is a deliberate AVAILABILITY choice:
a legitimately-lost anchor (keychain reset, machine migration, backup-restore) must still open the
vault. From inside the vault we cannot distinguish lost-vs-maliciously-deleted without a second,
keyring-resident anchor. Round-3 therefore lands **honest documentation + one hygiene fix only** —
it does NOT change custody behavior or the anchor design.

- **F-1 documentation (mandatory).** Corrected the overstated anchor doc-comments in `custody.rs` to
  state the limit precisely: the anchor defends **FS-only** rollback; a keyring **delete** capability
  downgrades to no-rollback-protection (deletion intentionally treated as first-init for availability);
  and — restating the already-known limit — a **dumped keyring master** removes the throttle leaving
  only Argon2id. Corrected blocks: the `custody-generation` / `custody-lockout-generation` const docs
  (was: "integrity-protected the same way the master key is"), `read_anchor`, `enforce_high_water`, and
  the `Envelope` / `Header` DR-1/DR-2 doc-comments (was: the anchor "is what actually defeats a
  whole-envelope rollback"). No comment now describes the anchor as defeating rollback for a
  delete-capable or master-dumping attacker — matching the honesty bar already applied to the
  dumped-master case in Day-3.
- **`checked_add` hygiene.** The `generation + 1` bump in `put_inner` (`custody.rs`) now uses
  `checked_add(1)` and fails closed (`Corrupt`) on overflow instead of a debug-panic / release-wrap.
  Unreachable in practice (~1.8e19 puts to saturate a `u64`), but clean. No behavior change on any
  reachable path; no test added (the branch is not reachable through the public API without seeding a
  `u64::MAX` generation, which would require exposing the session DEK — declined as it would weaken the
  boundary; 38 tests stay green).

**OWNER-DECISION FOLLOW-UP (deferred — NOT done here).** The optional close for F-1 is a
**master-sealed "anchor-initialized" bit** inside the envelope: once the vault has ever written an
anchor, the envelope records (under the keyring master) that it is anchored, so
`anchor == None && envelope-says-anchored ⇒ Corrupt`. That would make a deleted anchor fail closed
instead of re-seeding — but it **trades keychain-reset / machine-migration / backup-restore
recoverability** (a legitimately-lost anchor would then brick the vault), so it needs its own review
and an owner decision on that recoverability tradeoff. Its **delete-then-rollback regression test**
lands with that hardening, not here. Do not implement it as part of round-3.

**Gates (post-round-3):** typecheck clean · FE test 20 green · build OK · `cargo test --workspace
--locked` **38 green (+0 — docs + one checked_add, no behavior change)** · fmt clean · clippy
`-D warnings` clean. F-1 recorded as an explicit, accepted anchor LIMITATION (FS-only scope;
delete-downgrade; dumped-master), with the master-sealed anchor-initialized bit as the deferred
owner-decision close.

- **@rule8:** round-3 close on the branch (PR #7) is honest-docs + hygiene only. F-1 stands as an
  accepted, documented limitation pending the owner decision on the anchor-initialized bit. Still
  **DO NOT MERGE**. `Reconciliation: OPEN` for the ChatGPT quorum leg.

### Closed — 2026-07-12 (SIGN-OFF cleared)
- Merged in citrate-core #7. Rule-8 sign-off cleared after 3 remediation rounds —
  full record in citrate-security #13 (00/FINDINGS round 1, 01_DELTA round 2,
  02_FINAL_REATTACK_AND_CLOSE close). Headline: the whole build passed its own
  10-case adversarial suite, but review found the lockout bypassable (concurrency
  + restart), a timing oracle, and the anti-rollback generation counter was dead
  code (`let _generation`) → whole-envelope rollback resurrected revoked secrets.
  v2 envelope (per-slot AAD + DEK-sealed header + master-sealed lockout) + a keyring
  high-water anchor closed it; F-1 (anchor-delete downgrade) documented honestly.
- Final gate: cargo test 38, vitest 20, fmt/clippy/audit clean, I-2 boundary intact.
- **Owner-decision follow-up (deferred):** the optional master-sealed
  "anchor-initialized" bit that would close F-1 (security vs keychain-reset/migration
  recoverability) — decide before A3/B1 rely on the vault. ChatGPT quorum on #13 OPEN.
- Status: **completed.** Next: A3 (OIDC loopback — stores the first real secret here).
