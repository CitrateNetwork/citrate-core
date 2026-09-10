---
created: 2026-07-12
branch: docs/sprint-a3-oidc
author: Claude Fable 5, directed by @SaulBuilds
status: completed
sprint: CORE-A3
planset: citrate-federation/.agentile/planset/2026-07-12-core-beta-wiring/00_STATE_AND_PLAN.md (Phase A3)
rule8: yes — auth + OIDC refresh-token custody; security sign-off before relied on for real auth
depends_on:
  - CORE-A1 (bridge) — landed
  - CORE-A2 (custody vault) — landed + signed off; A3 is its first real consumer
  - authority redeploy (owner) — hard gate for end-to-end prod testing
---

# Sprint CORE-A3 — OIDC loopback sign-in + entitlement + KYC seam

Protocol (Agentile): Rule 1 — no mocks-as-live; Rule 2 — test count monotone, recorded;
Rule 3 — **no signing** (that is B1); Rule 4 — this file is the record; Rule 8 — auth +
token custody carries a security-sign-off gate and ships an adversarial suite; Rule 11 —
every acceptance names its data source. Red-test-first for the auth guards.

## Why A3, and what it is

A3 turns the sim sign-in into real identity. It flips the bridge `auth` domain from the
honest `Unavailable` seam to a real loopback-PKCE flow against auth.citrate.ai (the
`citrate-core` client, already registered — citrate-identity #58), stores the rotating
refresh token in the **A2 custody vault** (A2's first real revocable secret), and replaces
the sim persona/tier with the live `https://citrate.ai/entitlement` claim. It also wires
the KYC seam (S2). Onboarding S1/S2 then drive off real events. Everything downstream of
identity (Commissary gating, Settings RBAC, the whole membership path) depends on this.

**Not in A3:** signing / wallet / UserOps (B1); membership payment + grant (Phase D);
device-code grant (upstream E-6); the sidecar supervisor (Phase C).

## Two hard dependencies (name them up front)

1. **Authority redeploy (owner/DGX).** The `citrate-core` client registration is merged
   but auth.citrate.ai must be redeployed to *serve* it. Until then, A3 is built and
   tested against a **mock/local OIDC authority** (a test fixture implementing /auth,
   /token, /userinfo, /kyc/* enough to exercise the flow); the live prod round-trip is a
   post-redeploy acceptance step, flagged honestly, not faked.
2. **A2 F-1 decision.** A3 stores a **revocable** refresh token in the vault — exactly the
   secret A2's whole-envelope-rollback threat (F-1) targets (resurrect a revoked token).
   The A2 sign-off documented F-1 (anchor-delete downgrade) as an accepted, honestly-scoped
   limit; the optional master-sealed hardening was deferred to the owner "before A3/B1
   rely on the vault." **This is that point.** Owner decides: ship A3 on the
   documented-FS-only anchor, or land the hardening first. Recorded as decision D-A3-1.

## Design

- **Rust `src-tauri/src/oidc.rs`** (extends the A1/A2 command idiom; consumes A2 custody):
  - `auth_login()` — bind a loopback listener on `127.0.0.1:<random>` (RFC 8252); build
    the `/authorize` URL with **PKCE S256** (code_challenge), a random `state` and
    `nonce`, `redirect_uri = http://127.0.0.1:<port>/callback`, scope
    `openid profile wallet kyc offline_access`; open the **system browser**
    (tauri-plugin-opener, already a dep); capture `code`+`state` on the single-use
    loopback callback (validate `state`, bind-timeout); exchange at `/token` with the
    `code_verifier`; validate the `id_token` (`nonce`, iss, aud, exp, signature via JWKS).
  - **Token custody:** the rotating **refresh token → A2 custody vault** (slot
    `oidc-refresh`); access token + expiry held in memory only; claims (`sub`,
    `wallet_address`, `kyc_status`, entitlement) parsed for the UI. **No token ever
    crosses the invoke boundary to the frontend** (the A2 I-2 pattern — `auth_status`
    returns claim-derived flags, never tokens).
  - `auth_userinfo()` — live `/userinfo` re-check (the federation RP rule). `auth_refresh()`
    — silent refresh using the vaulted refresh token (survives app restart). `auth_logout()`
    — revoke + clear the vault slot + wipe memory.
- **Bridge `auth` domain** — wire status/login/userinfo/logout (currently `seam::auth_*`
  returning Unavailable). Sim shim keeps the prototype persona flow for web-dev; guarded
  out of packaged builds.
- **Entitlement engine (frontend):** read the live claim (`tier/orgId/citrateRole/
  expiresAt`); gating replaces the sim persona/tier everywhere (Commissary locks, Settings
  RBAC, the onboarding tier). Onboarding S1 drives off real login events.
- **KYC seam (S2):** `kyc_start()` opens `/kyc/start` in the browser; poll `/kyc/status`
  + `/userinfo`; S2's none/pending/verified/failed/review states drive off real polling.

## WP checklist

- [x] **A3.1 — Loopback PKCE flow (Rust).** `src-tauri/src/oidc.rs`: `LoopbackListener`
  binds `127.0.0.1:0` (RFC 8252), `Pkce::new()` (S256), random `state`+`nonce`, `/authorize`
  URL builder, browser open (injected — `tauri-plugin-opener` in prod), single-use callback
  parse + `state` validate, `/token` exchange with `code_verifier`, id_token validation
  (nonce/iss/aud/exp/ES256-sig via JWKS), claim parse.
  - Acceptance MET: `login_roundtrip_yields_claims_and_vaults_refresh` — full round-trip
    against the mock authority yields parsed claims (data source: mock `/authorize` + `/token`).
    Live staging/prod round-trip is a post-redeploy step (flagged — Day-1 note).
- [x] **A3.2 — Refresh-token custody + lifecycle.** refresh token → A2 vault slot
  `oidc-refresh` (`custody_put`/in-process `custody_get`, never invoke); access token +
  expiry in memory only; `refresh()` silent refresh; `logout()` revoke + `clear_slot` +
  wipe. Added `CustodyVault::clear_slot` (in-process, header-resealed, generation-bumped).
  - Acceptance MET: `integration_lifecycle_login_restart_refresh_userinfo_logout` — refresh
    token persisted in the A2 slot (data source: A2 `custody_put/get`, NOT memory/logs/frontend);
    silent refresh succeeds after a simulated restart re-reading the vault; logout clears the
    slot and a revoked token can no longer mint a session.
- [x] **A3.3 — Bridge `auth` wired + entitlement engine.** bridge `auth` domain →
  `status/login/userinfo/refresh/logout/kycStart` invoking the real Rust commands (Tauri);
  sim shim derives the claim-derived `AuthStatus` from the persona/AppState (web-dev, guarded
  by `assertSimAllowed`). Store `refreshAuth/applyAuthStatus` folds the live claim
  (`tier/org/citrateRole/kycStatus`) into the entitlement engine; Settings Account/RBAC reads
  `s.citrateRole`; Sign-out calls the real `auth_logout`.
  - Acceptance MET: tauri.test.ts proves the adapter invokes the OIDC commands and passes
    claim-derived flags (never a token); sim.test.ts proves the persona-derived claim +
    onboarding gate (data source: `/userinfo` entitlement claim / sim persona).
- [x] **A3.4 — KYC seam (S2).** `kyc_start` opens `/kyc/start`; store `pollKyc` polls
  `auth_userinfo` every 5s while S2 is pending; `applyAuthStatus` maps `kyc_status` →
  none/pending/verified/failed/review. Onboarding S2 drives off this.
  - Acceptance MET: mock `/userinfo` `kyc_status` change flips S2 (data source: `/userinfo`
    `kyc_status`). (Mock has no separate `/kyc/status`; polling is via `/userinfo`, per design.)
- [x] **A3.5 — Adversarial + integration suite (@rule8 evidence).** table below, all green;
  representative guards RED-confirmed (ADV-1 state, ADV-4 loopback, ADV-7 nonce).

## Adversarial test plan (Rule 8 — red-test-first)

| # | Attack / property | Expected | Test(s) | Result | RED-confirmed |
|---|---|---|---|---|---|
| ADV-1 | callback `state` mismatch (CSRF) | rejected | `adv1_state_mismatch_is_rejected` | GREEN | yes — guard removed → test fails (accepted tampered state) |
| ADV-2 | missing/mismatched `code_verifier` (PKCE) | token exchange rejected | `adv2_pkce_verifier_mismatch_rejects_token_exchange` (+ `adv2b` positive control) | GREEN | server PKCE check drives it; positive control isolates the verifier |
| ADV-3 | PKCE `plain` downgrade | rejected; S256 enforced | `adv3_plain_downgrade_rejected_and_client_never_offers_it` | GREEN | client hard-wires `S256`; authority 400s a `plain` probe |
| ADV-4 | loopback bind address | `127.0.0.1` only, never `0.0.0.0` | `adv4_loopback_binds_localhost_only` | GREEN | yes — bind `UNSPECIFIED` → test fails |
| ADV-5 | auth-code replay / injection | single-use; rejected | `adv5_code_is_single_use` | GREEN | server removes code on first use; replay + never-issued both rejected |
| ADV-6 | foreign/no-`state` callback | rejected + listener closes | `adv6_callback_without_state_is_rejected_and_listener_closes` | GREEN | `parse_callback_target` fails closed without both params |
| ADV-7 | id_token nonce/iss/aud/exp/sig | each validated; forgery rejected | `adv7_{forged_nonce,wrong_issuer,wrong_audience,expired,forged_signature}_rejected` (+ `adv7_valid_..._accepted`) | GREEN | yes — nonce guard removed → forged-nonce test fails; each knob isolates one check |
| ADV-8 | **token boundary** — invoke returns a token | none does; `auth_status` = flags only | `adv8_no_auth_invoke_command_returns_a_token`, `adv8_status_is_claim_derived_only_at_runtime` | GREEN | structural: `AuthStatus` has no token field (adding one breaks compilation); runtime status JSON carries no `at_`/`rt_` |
| ADV-9 | refresh token in logs/errors | absent | `adv9_refresh_token_absent_from_errors` | GREEN | every `AuthError` Display is a fixed secret-free string; a real refresh-failure error is asserted not to contain the token |
| ADV-10 | listener timeout / no callback | fails closed, port released | `adv10_listener_timeout_fails_closed_and_releases_port` | GREEN | non-blocking accept loop + deadline; the port re-binds after timeout |

Token-boundary proof (ADV-8) — the six registered auth invoke commands and their return
types (from `oidc.rs` + `lib.rs`), NONE returns a token:

| command | returns |
|---|---|
| `auth_status` | `AuthStatus` (claim-derived flags) |
| `auth_login` | `AuthStatus` |
| `auth_userinfo` | `AuthStatus` |
| `auth_refresh` | `AuthStatus` |
| `auth_logout` | `()` |
| `kyc_start` | `()` |

`AuthStatus` fields are `signedIn, sub, tier, org, role, kycStatus, walletAddr, expiresAt,
email` — no access/refresh token field exists. The access token lives only in
`AuthManager.session` (in-memory, zeroized); the refresh token lives only in the A2 vault.
`TokenResponse` (the sole struct that names tokens) is private and never a command return
type (asserted by the ADV-8 test).

## Integration test plan

- Full lifecycle against the mock authority: login → refresh token in vault → app restart
  → silent refresh → `/userinfo` → logout clears the vault slot + revokes. No live-prod
  dependency in CI (mock fixture).
- Entitlement claim change (mock) flips tier gating.

## Locked / open decisions

- **D-A3-1 (RESOLVED by owner — ship FS-only anchor).** A3 ships on the A2 documented
  FS-only rollback anchor, accepting the F-1 anchor-delete-downgrade limit. Rationale (owner-
  confirmed): the refresh token is REVOCABLE and network-recoverable — a rolled-back token
  still fails server-side revocation on next use, and re-login re-issues; the FS-only anchor
  is defensible for A3 with the limitation documented. The A2 master-sealed "anchor-initialized"
  hardening is NOT landed here; it remains the harder case for B1 (wallet key, not re-issuable).
  A3 did NOT modify A2's crypto; it only consumes the vault (`custody_put`/`custody_get` +
  the new in-process `clear_slot`).
- **D-A3-2 (locked) — mock authority fixture** for CI (no live-prod dependency); prod
  round-trip is a post-redeploy manual/staging acceptance step (honest, per Rule 1).

## Test-count baseline (Rule 2)

| Date | Suite | Count | Command |
|---|---|---|---|
| 2026-07-12 | Rust | 38 | `cargo test --workspace --locked` (post-A2) |
| 2026-07-12 | Frontend | 20 | `npm run test` (post-A2) |
| 2026-07-12 | Rust | 57 | `cargo test --workspace --locked` (post-A3: +19 — 12 ADV cases, 2 integration/happy-path, 5 positive controls) |
| 2026-07-12 | Frontend | 25 | `npm run test` (post-A3: +5 — 2 tauri auth, 3 sim auth) |
| 2026-07-12 | Rust | 64 | `cargo test --workspace --locked` (post-security-remediation: +7 — OIDC-1 absent/wrong aud, OIDC-2 kidless-multikey + single-key control, A3-04 refresh sub-substitution, A3-01 reserved-slot predicate + in-process control) |
| 2026-07-12 | Rust | 68 | `cargo test --workspace --locked` (post-delta-re-attack: +4 — NEW-1 multi-aud/azp: no-azp, foreign-azp, single-aud+foreign-azp + our-azp control) |
| 2026-07-12 | Frontend | 30 | `npm run test` (post-delta-re-attack: +5 — `isExpiredClaim` A3-03 coverage incl. fail-closed on unparseable) |
| 2026-07-12 | Rust | 73 | `cargo test --workspace --locked` (post-authority-integration: +5 — A3-AUTH-1 redirect `/auth/callback` path, A3-AUTH-2 discovery issuer trust-anchor, A3-AUTH-3 valid-RS256 + forged-alg-header + alg-pin discovery∩verifiable unit) |

## Definition of done

- Real loopback-PKCE sign-in works end-to-end against the mock authority (and against prod
  after redeploy — flagged); refresh token lives in the A2 vault, never crosses the invoke
  boundary, never logged.
- Live `/userinfo` entitlement replaces the sim persona/tier; KYC seam drives S2 off real
  polling; sim shim still renders web-dev.
- Every ADV-* lands red-first then green; the integration lifecycle passes across a restart.
- Gates green: typecheck, vitest, build, cargo test/fmt/clippy `-D warnings`, audit; counts up.
- **@rule8 gate:** adversarial evidence filed; security sign-off (cross-model, house style)
  before A3 is relied on for real auth. Consumes A2 (signed off).
- Honest gap note for anything not runnable headless (real keyring, live authority, browser).

## Daily updates

### Day 0 — 2026-07-12 (scoped)
- Scoped from CORE-BETA Phase A3, after A2 custody merged (#7) + signed off (citrate-security
  #13). A3 is the vault's first real consumer. Two hard deps named: authority redeploy
  (owner/DGX) and the D-A3-1 F-1 decision. Awaiting go to dispatch A3.1.

### Day 1 — 2026-07-12 (built A3.1–A3.5 on `feat/core-a3-oidc`)
- **Built.** `src-tauri/src/oidc.rs` (loopback-PKCE flow, token custody, entitlement claims,
  6 invoke commands) + `src-tauri/src/oidc_tests.rs` (mock OIDC authority fixture + ADV-1..10 +
  integration lifecycle). Added `CustodyVault::clear_slot` (in-process, header-resealed) for
  logout. Bridge `auth` domain wired (tauri invokes real; sim derives persona claim). Store
  entitlement engine (`refreshAuth`/`applyAuthStatus`/`pollKyc`) + `citrateRole` claim.
  Settings Account/RBAC + Sign-out and onboarding S1/S2 drive off real auth/KYC events in a
  Tauri build (sim timers preserved for web-dev).
- **D-A3-1 RESOLVED** (owner): ship FS-only anchor. A3 consumes A2 unchanged; no A2 crypto
  touched. Recorded above.
- **Gates (all green).** `cargo test --workspace --locked` 57 (↑ from 38) · `cargo fmt --check`
  clean · `cargo clippy --workspace --all-targets --locked -- -D warnings` clean ·
  `cargo audit` 0 vulnerabilities (17 pre-existing Tauri/GTK "unmaintained" warnings unchanged
  from baseline) · `npm run typecheck` clean · `npm run test` 25 (↑ from 20) · `npm run build` OK.
- **New crates (justified).** `ureq` 3 (json/rustls) — light blocking HTTP for /token,
  /userinfo, JWKS, revoke (reqwest not in the Tauri tree; blocking is correct — the flow runs
  off the UI thread). `jsonwebtoken` 10 (`aws_lc_rs`, `use_pem`; NOT `rust_crypto`) — id_token
  ES256 sig + claim validation; `aws_lc_rs` provider chosen over `rust_crypto` because the
  latter pulls `rsa` (RUSTSEC-2023-0071 Marvin timing) even though A3 only uses ES256 — with
  `aws_lc_rs` there is no `rsa` in the tree and `cargo audit` is clean. `url` 2, `sha2` 0.10,
  `base64` 0.22 — PKCE S256 + URL build/parse. dev-only: `p256` 0.13 (`pem`) — the test
  authority's ES256 signer/JWKS (not shipped).
- **RED-first evidence.** Representative guards confirmed RED then GREEN by neutralizing the
  check and observing the specific ADV test fail: ADV-1 (state), ADV-4 (loopback bind), ADV-7
  (nonce). ADV-8 is structurally stronger — adding a token field to `AuthStatus` fails to
  COMPILE. The full red→green log per case is in the PR body.
- **HONEST GAPS (not runnable headless — per Rule 1, no faked pass).**
  1. **Live authority round-trip** — auth.citrate.ai is being redeployed; the `citrate-core`
     client registration is merged but not yet served. Everything is proven against the mock
     authority fixture; the live-prod / staging round-trip is a post-redeploy acceptance step,
     NOT run here and NOT faked. `AuthError::Unavailable` is the honest production-path error
     until then.
  2. **Real system browser** — `login_with` takes an injected `open_browser`; production wires
     `tauri-plugin-opener`, tests inject the mock's 302-follow. The interactive-browser leg is
     not exercised in CI.
  3. **Real OS keyring** — the A2 vault runs over the in-memory keyring fake in these tests (as
     A2 itself does headless); the real-keyring round-trip is A2's honestly-skipped integration
     leg, unchanged.
  4. **@rule8 sign-off** — the cross-model security review (house style) is the remaining gate
     before A3 is relied on for real auth. Adversarial evidence is filed (this suite); PR is
     open and MUST NOT be merged until sign-off (Rule 8 — owner merges).

### Day 1b — 2026-07-12 (security remediation — two adversarial reviews)
Two cross-model adversarial reviews ran against PR #9 (loopback/custody/transport lens and
OIDC/PKCE protocol lens). One CRITICAL + several supporting findings were fixed in-branch,
each with a red-first test (guard neutralized → the new test fails → guard restored → green).

| id | severity | finding | fix | test (RED-confirmed) |
|---|---|---|---|---|
| OIDC-1 | **CRITICAL** | audience-confusion — an id_token that OMITS `aud` bypassed the audience binding (`set_audience` doesn't mark `aud` required); a forged aud-less token was accepted with attacker `sub`/`tier` | `aud` is now a REQUIRED, non-defaulted `IdTokenClaims` field + `set_required_spec_claims(["exp","iss","aud"])` + a manual `aud.contains(client_id)` re-check (handles string OR array aud) | `oidc1_absent_aud_is_rejected` (RED-confirmed), `oidc1_wrong_aud_still_rejected` |
| OIDC-2 | MEDIUM | JWKS key selection fell back to the FIRST key for a kid-less token — an attacker-ordered multi-key JWKS + attacker-signed kid-less token would verify | exact-`kid` match required when present; a kid-less token is accepted ONLY against a single-key JWKS, else fail closed (no positional fallback) | `oidc2_kidless_token_multikey_jwks_is_rejected` (RED-confirmed, attacker-signed), `oidc2_kidless_token_single_key_jwks_accepted` (control) |
| A3-01 | HIGH | `custody_put` INVOKE command did not reserve the `oidc-refresh` slot — a compromised/XSS'd webview could `invoke("custody_put",{slot:"oidc-refresh",...})` and overwrite the vaulted refresh token | added `is_backend_reserved_slot` (`oidc-` prefix); the `custody_put` COMMAND rejects backend-owned slots while the in-process `put`/`custody_get`/`clear_slot` (used by the auth backend) stay unrestricted | `a3_01_backend_reserved_slot_predicate`, `a3_01_in_process_put_still_writes_backend_slot` |
| A3-02 | HIGH | `"csp": null` — any XSS/compromised dep could reach every custody/auth invoke | set a strict CSP (`default-src 'self'`; `connect-src` limited to self + auth/rpc/infer.citrate.ai + Tauri ipc/asset) in `tauri.conf.json` | config-level (needs a packaged-app smoke test — honest gap below) |
| A3-03 | MEDIUM | entitlement was rendered, never enforced; `expiresAt` unchecked | `applyAuthStatus` enforces `expiresAt` at the decision point — a past expiry downgrades to free/lapsed (`isExpiredClaim`) | typecheck/build; store-level (frontend has no store harness yet — folded into the engine) |
| A3-04 / OIDC-3 | LOW→MED | `refresh()` trusted the refresh access token + `/userinfo` with no id_token re-validation nor `sub` continuity | `refresh()` now re-validates a refresh id_token when present (iss/aud/exp/nbf/sig, nonce-exempt) AND enforces `sub` continuity against the prior session (id_token + `/userinfo`) | `a3_04_refresh_subject_substitution_is_rejected` |
| OIDC-4 | LOW | `nbf`/`iat` not validated | `validate_nbf = true` + 30s leeway for clock skew | covered by the shared validator path |
| A3-06 | INFO | `TokenResponse`/`CallbackParams` derived `Debug` over raw token/code | custom REDACTING `Debug` impls — token/code never printed by `{:?}`/dbg!/tracing | — |

**Not changed (accepted, honestly-scoped):** A3-05 (listener 200s the first local connection before validating `code`/`state` — a self-DoS race, not code interception; the code is PKCE-S256-bound and useless without the verifier) and A3-07 (`status()` keeps entitlement flags for an expired access-token session to avoid UI flap — entitlement stays claim-derived, never a token, and the new `expiresAt` enforcement (A3-03) is the real gate). Both documented for the owner.

- **Gates after remediation (all green).** `cargo test --workspace --locked` **64** (↑ 57) · `cargo fmt --check` clean · `cargo clippy --all-targets --locked -- -D warnings` clean · `cargo audit` 0 vulns · `npm run typecheck` clean · `npm run test` 25 · `npm run build` OK.
- **New honest gap.** The strict CSP (A3-02) is a config change verified by build only; it needs a **packaged-app smoke test** (the webview must still load assets + reach the IPC + the auth/rpc/infer hosts under the new policy) before deploy. Not runnable headless here.

### Day 1c — 2026-07-12 (delta re-attack — independent verification of 9191fbf)
An independent delta re-attack re-ran against the remediation commit (not trusting the shipped
tests — it wrote its own probes for the exact bypass shapes). It confirmed the CRITICAL (OIDC-1
absent-aud) + both HIGHs (A3-01 custody slot, A3-02 CSP) are genuinely CLOSED, plus OIDC-2/03/04/06
closed, with reproduced evidence. Verdict: **yes-with-conditions**. One new code-blocking finding
was fixed in-branch; test-debt closed.

| id | severity | finding | fix | test (RED-confirmed) |
|---|---|---|---|---|
| NEW-1 | MEDIUM | multi-valued `aud` with a mismatched `azp` was accepted — no `azp` verification (OIDC Core §3.1.3.7 rules 4–5). A token issued to a DIFFERENT authorized party that merely lists us in `aud` (confused-deputy / multi-client misissuance) would sign the user in. Precondition: authority misissuance (sig + iss + our-id-in-aud still hold) | `azp` added to `IdTokenClaims`; a MULTI-valued `aud` now requires `azp == client_id`; a present `azp` (even single-aud) must name us, else reject | `new1_multi_aud_without_azp_is_rejected` (RED-confirmed), `new1_multi_aud_with_foreign_azp_is_rejected`, `new1_single_aud_with_foreign_azp_is_rejected`, `new1_multi_aud_with_our_azp_is_accepted` (control) |
| A3-03 test-debt | — | the entitlement-expiry fix shipped with ZERO automated coverage (the 25 npm tests were all bridge) | exported `isExpiredClaim` + added `src/shell/store.test.ts` (5 cases); ALSO hardened it to **fail-closed** on a present-but-unparseable `expiresAt` (was fail-open) — absent stays not-expired | `isExpiredClaim` suite (past/future ISO + unix, absent, unparseable→expired) |

**Recorded (NEW-2, not a bug — scope note):** A3-03 entitlement gating is UI-side in this build.
`applyAuthStatus` correctly downgrades tier/entitlement on a past expiry, but end-to-end download
enforcement is the (currently `Unavailable`) backend signed-URL seam (`membership_entitlement` /
`commissary_catalog`). A webview-level attacker can invoke the download path regardless of the
locked UI. This is consistent with the doctrine (real bytes only via an authority-minted signed
URL) and acceptable for A3 scope — but it MUST NOT be marketed as enforced gating until that seam
lands (Phase D). Owner note.

- **Gates after delta re-attack (all green).** `cargo test --workspace --locked` **68** (↑ 64) ·
  `cargo fmt --check` clean · `cargo clippy --all-targets --locked -- -D warnings` clean ·
  `cargo audit` 0 vulns · `npm run typecheck` clean · `npm run test` **30** (↑ 25) · `npm run build` OK.

### Day 2 — 2026-07-12 (authority-integration follow-up — the deployed authority is real)

The authority auth.citrate.ai + the `citrate-core` client are now DEPLOYED and VERIFIED
(citrate-identity `2026-07-11-core-oidc-client/DEPLOY_LOG.md`, commit `6e4cc60`;
`citrate-labs/handoffs/IDENTITY_DEPLOY_HANDOFF.md`). Standing the A3 client up against the
LIVE authority surfaced four wrong assumptions the old mock had baked in (it mirrored the
code's guesses, not reality, so the bugs sailed through). Fixed on `feat/core-a3-oidc`,
red-test-first where behavior changed. The independent `@rule8` sign-off properties (alg
pin / OIDC-1, exact-kid JWKS / OIDC-2, azp / NEW-1, aud, nonce, state, PKCE-S256, token
boundary, custody slot) are all PRESERVED — see the "not regressed" note below.

| # | Wrong assumption (old A3) | Live authority (verified) | Fix | Test (RED-confirmed) |
|---|---|---|---|---|
| 1 | redirect path `…/callback` | `…/auth/callback` (303 verified); `/callback` 400s | `login_with` builds `http://127.0.0.1:<port>/auth/callback`; mock registers it | `a3auth1_redirect_uri_path_is_auth_callback` (RED: reverting to `/callback` fails the path assert) |
| 2 | validates **ES256-only** | signs **RS256** (one RSA JWKS key `kid citrate-1780633938850`) | verify RS256 via `jsonwebtoken` **`aws_lc_rs`** RSA (JWKS `n`/`e` → `DecodingKey::from_rsa_components`); NO `rsa` crate (RUSTSEC-2023-0071 stays out); ES256 support kept for a future per-client key | `a3auth3_valid_rs256_id_token_accepted` (whole flow now RS256), `a3auth3_forged_alg_header_is_rejected`, `a3auth3_alg_pin_gates_on_discovery_and_verifiable_set` (RED-confirmed) |
| 3 | hardcodes `/authorize`, `/userinfo`, `/.well-known/jwks.json`, `/revoke` | serves `/auth`, `/token`, `/me`, `/jwks`, `/token/revocation` | consume the discovery doc (`${issuer}/.well-known/openid-configuration`, HTTPS cert-verified) at login/refresh; endpoints come FROM discovery, not guesses | covered by every login/refresh/userinfo/logout test now routing through discovery |
| 4 | discovery not fetched → no attacker-authority defense | — | verify discovery `issuer` == the hardcoded trust anchor `https://auth.citrate.ai`; a mismatch fails closed | `a3auth2_discovery_issuer_mismatch_is_rejected` (RED: dropping the gate consumes swapped endpoints) |

- **Alg-pin (OIDC-1 NOT regressed).** The accepted alg set is now driven by discovery's
  `id_token_signing_alg_values_supported` ∩ what we can verify (RS256 + ES256), NOT a
  hardcoded constant. `decode_and_validate_id_token`: (a) `authority_accepts_alg` — alg must
  be verifiable AND advertised (empty-advertised falls back to the verifiable set; HS*/none
  are never verifiable, so never accepted); (b) `header.alg` must equal the JWKS key's alg
  for the token's `kid` (`jwk_decoding_key` returns `(key, key_alg)`); (c) `Validation::new
  (key_alg)` — a single key-attested alg, never a broad allow-list. `alg:none`, HS256, and
  any unadvertised/unkeyed alg die before any signature work. Exact-kid JWKS selection (no
  first-key fallback), required exp/iss/aud, `aud.contains(client_id)`, the azp rules,
  nonce, nbf, state CSRF, PKCE-S256 are all unchanged.
- **Discovery issuer trust anchor.** `AuthorityConfig` keeps the hardcoded prod `issuer`
  (`https://auth.citrate.ai`) and the discovery URL; `discover()` fetches the doc and rejects
  it unless `doc.issuer == cfg.issuer`, THEN derives endpoints from it. So no endpoint is ever
  trusted from an authority whose issuer we did not pin.
- **Rebuilt mock authority.** `oidc_tests.rs` now MIRRORS the real one: serves a discovery
  doc with `/auth` `/token` `/me` `/jwks` `/token/revocation` +
  `id_token_signing_alg_values_supported: ["RS256"]`; signs id_tokens **RS256** with a STATIC
  test RSA key (embedded PEM + precomputed JWK `n`/`e`, `e=AQAB`) so no key-gen crate — and
  NOT the `rsa` crate — is pulled in (signing via `aws_lc_rs` from PEM); registers the
  `/auth/callback` loopback redirect. All existing ADV/integration tests now run against
  these REAL shapes (that is why the four bugs no longer hide). The old ES256/`/callback`/
  standard-path mock is gone.
- **Gates (all green).** `cargo test --workspace --locked` **73** (↑ 68) · `cargo fmt --check`
  clean · `cargo clippy --workspace --all-targets --locked -- -D warnings` clean · `cargo
  audit` 0 vulns (no `rsa`; RUSTSEC-2023-0071 absent; 17 pre-existing GTK/Tauri unmaintained
  warnings unchanged) · `npm run typecheck` clean · `npm run test` **30** · `npm run build` OK.
- **HONEST GAP (unchanged doctrine).** The live auth.citrate.ai + a real system browser still
  cannot run headless in CI; everything is proven against the rebuilt RS256 mock that mirrors
  the deployed authority's exact shapes. The live-prod / staging interactive round-trip
  remains a post-merge manual acceptance step, NOT run here and NOT faked.
- **Scope.** BUILD-and-STOP: this agent authored the fixes only. The independent `@rule8`
  reviewer (not the author) re-attacks these changes; no self-sign-off is claimed.

### Closed — 2026-07-13 (SIGN-OFF cleared, #9 merged)
- Real loopback-PKCE sign-in + entitlement + KYC seam merged (citrate-core #9).
- Rule-8 sign-off cleared (citrate-security #14): build round found CRITICAL OIDC-1
  (absent-aud takeover) + A3-01/02 + NEW-1 azp — all closed + independently re-attacked.
- **Authority-integration follow-up (b30aaeb):** grounding in the identity team's deploy
  records (DEPLOY_LOG 6e4cc60) exposed that A3 assumed the WRONG authority shape (ES256 +
  standard paths) while the live authority is RS256 + /auth /me /jwks + /auth/callback.
  Fixed: redirect path, RS256 via aws_lc_rs (no rsa crate), consume discovery, rebuilt the
  mock to mirror the real authority. Independent delta (#14 03_) = clear; OIDC-1 no
  regression (real EC-signed probe). KYC confirmed LIVE by DevOps; kyc_status read from /me.
- Standing (deploy-time, non-blocking): live-authority round-trip (now runnable), CSP
  runtime smoke, A3-AUTH-DERIV LOW hardening, ChatGPT quorum OPEN.
- Status: **completed.** Next: B1 (the wallet keystore + SignatureCeremony) — the vault's
  first NON-re-issuable secret; the A2 F-1 hardening decision bites here.
