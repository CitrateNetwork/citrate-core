---
created: 2026-08-30T00:00:00Z
branch: audit/2026-08-30-core-money-surfaces
author: Claude Fable 5 (drafting) — signers: Larry Klosowski (owner) + security lead
status: active
audit_id: 2026-08-30-core-money-surfaces
standard: AGENTILE_AUDIT_STANDARD v0.2
severity_rubric: v2.0 (Severity × Difficulty)
tier: Tier-1 (money / keys / identity / distribution)
target_repo: citrate-core @ 97cf4c74629cef25f600e9778640631c7c335b16
gate: gateSec / @rule8 (Milestone B — citrate-core leg of the SETL-S4 money-surface sign-off)
reconciles_with: citrate-settlement/.agentile/audits/2026-08-30-setl-money-surface (settlement leg)
---

# citrate-core @rule8 / gateSec — money & key surface security review (Milestone B)

The citrate-core leg of the T1 money-surface security sign-off. It reviews the five @rule8 surfaces
in the desktop full-node that gate real member SALT / keys / distribution, reconciled into the central
gateSec / SETL-S4 package in citrate-security. Reviewers: owner + security lead.

> **Headline:** the citrate-core money/key surfaces are **well-architected**. Ceremony-only wallet
> signing is verified and *physically* enforced (the gated signer is `pub(crate)` in the `kit` crate,
> unreachable from the `src-tauri` command registry); there is **no ungated money write, no signer
> outside the ceremony, and no user key crossing the invoke boundary**. **No CRITICAL or HIGH
> finding.** The @rule8 items that remain are **pre-ship process gates** (updater private-key custody,
> keeping `SessionBudget` unwired, a release keyring intake for confidential OAuth, and applying the
> model path's hash discipline to the not-yet-built Commissary), not code defects — so the leg's
> opinion is **PROVISIONAL-PENDING-GATES** (§6).

## 1. Scope & threat model

### 1.1 Surfaces in scope (the five @rule8 items)

| # | Surface | @rule8 item | Why it's T1 |
|---|---|---|---|
| S1 | **Treasury custody** | D2.4 | The desktop touches a treasury/grant money path; a key or token that moves funds. |
| S2 | **Updater keys** | WO-2 | The auto-update path — a mis-verified update is remote code execution on every install. |
| S3 | **Gated downloads (Commissary)** | WO-7 | Signed catalog + streamed binary/model downloads; an unverified download is RCE / poisoned weights. |
| S4 | **Membership stake-lock** | M-3 | On-chain stake lock — member SALT committed to a contract. |
| S5 | **Credential surfaces** | — | OAuth/identity/social credential + token handling; a leak is account takeover. |

Out of scope: the settlement signer custody (`0x9D5d16FD…`) — covered in the settlement leg (SETL-S4);
chain consensus/sync (the node sync wedge is a separate, chain-side thread); the SBT/KYC contract
internals (audited separately).

### 1.2 The security oracle — what must hold

- **Rule 3 (ceremony-only signing).** Every signature routes through `SignatureCeremony::approve`;
  the gated signer (`wallet::sign_message`) is reachable from nowhere else. No `#[tauri::command]`,
  sidecar, daemon, or remote service signs or returns key/seed/entropy. Approval is bound to an
  explicit `CeremonyId` (no auto-approve), undecodable calldata is blocked until a raw-mode ack, one
  approval = exactly one signature, a locked vault fails closed. *A signer on any of the five surfaces
  that bypasses this is the top finding class.*
- **Rule 1 (no mocks / honest surfaces).** Every surface states what is real; no fabricated money
  state, no download that pretends to be verified. The training write-paths that would move real
  member SALT must be honestly gated until gateSec flips.
- **Integrity of anything executed or trusted.** Updates and gated downloads must be
  signature/hash-verified against a pinned key before they run or are trusted — fail closed on a
  mismatch. No downgrade / rollback to an unsigned or older-signed artifact.
- **No secret at rest in the clear / no secret over the command surface.** Treasury tokens, updater
  private keys, OAuth secrets, and credentials must not be logged, embedded in the bundle, returned by
  a command, or stored world-readable.

### 1.3 Attacker classes

1. A malicious **update/catalog server** (or a MITM on the download) serving a poisoned artifact.
2. A **malicious micro-app / node-agent / chat-agent / sidecar** inside the app trying to sign, move
   funds, or read a key/credential without a human ceremony approval.
3. A **local attacker** on the same machine reading secrets at rest (keystore, tokens, OAuth creds).
4. A **compromised or rogue holder** of a high-value key/token (treasury signer token, updater key).
5. An **honest-mistake operator** — a footgun that ships an unsigned build, an unrotated token, or a
   download with verification disabled.

## 2. Method

Two independent passes, reconciled, mirroring the settlement leg:
- **Primary pass** — structured read of each surface, tracing every money move / key touch / download /
  credential to the gate that protects it (or the absence of one).
- **Adversarial pass** — an independent reviewer per surface instructed to find the bypass: an
  out-of-ceremony signer, an unverified download, a secret at rest, an ungated write, a rollback.

Every CRITICAL/HIGH is re-verified against source (file:line) before it enters the register. Severity
uses rubric v2.0 (Severity × Difficulty) with the payment/crypto/identity/infra asset-class floors;
each finding records a `CIT-SEV` vector so the grade is replayable.

## 3. Findings register

Two independent passes (primary map + adversarial refutation) reconciled; each load-bearing claim
re-verified against source. **No CRITICAL/HIGH.** The register is process-gate + INFO/LOW — expected
for a surface this well-built. Grades use rubric v2.0.

| ID | Sev | Diff | Surface | Loc | One-line |
|---|---|---|---|---|---|
| CORE-G1 | MEDIUM (gate) | HIGH | S2 Updater (WO-2) | `src-tauri/tauri.conf.json:26-29` | Updater signature verification is enforced, but the **minisign private-key custody is undocumented in-repo** and the signed build is unshipped — a CRITICAL-stakes pre-ship gate. |
| CORE-G2 | MEDIUM (gate) | HIGH | Ceremony (Rule 3) | `kit/src/ceremony.rs:150-214` | `SessionBudget` auto-approve is present but **dead** (no non-test caller). Must **stay unwired** until ADR-2026-08-29 + its own @rule8 review — wiring it relaxes the per-tx HITL. |
| CORE-L1 | LOW | MEDIUM | S5 Credentials | `src-tauri/src/connections.rs:370-382` | Confidential MCP `client_secret` is read from a plaintext `oauth.dev.json`; **confirmed dev-only + unbundled + gitignored**, but the release keyring-intake path is a pending WP — must land before any confidential provider is wired in a shipped build. |
| CORE-I1 | INFO | — | S3 Downloads (WO-7) | `src-tauri/src/seam.rs:42,54` | The in-app "Commissary" gated-download leg is **not built** (honest `unavailable:` stub, fails closed); the entitlement + signed-URL gate is server-side (core-membership) and out of this repo. When built it MUST carry the model path's hash+signature discipline. |
| CORE-I2 | INFO | — | Signing scope | `src-tauri/src/validator.rs:134-146`, `comms.rs`, `cluster.rs` | Non-wallet signatures exist outside the ceremony **by design**: the ed25519 proposer PoP (embedded as calldata into a ceremony-gated tx, moves no funds) and comms/cluster MLS/libp2p device identities. Sign-off language must scope the invariant to *wallet/EVM + fund-moving*. |
| CORE-I3 | INFO | — | S5 Credentials | `src-tauri/src/invites.rs:72` | One-time group-invite tokens are stored device-local plaintext JSON — a consent token, not a key/credential; low sensitivity. |

### CORE-G1 — MEDIUM (pre-ship gate) — updater private-key custody (WO-2)
`CIT-SEV: sev=M; diff=H; asset=infra; impact=rce; reach=privileged; conf=firm` (stakes CRITICAL if the key leaks)

The updater is configured correctly: `tauri.conf.json:29` pins a **minisign public key**
(`F6E74B821EAAEB32…`), the endpoint is HTTPS GitHub releases (`:26-28`), and verification is enforced
by `tauri-plugin-updater` (`lib.rs:111`) — there is **no custom download/apply code** and **no
`dangerousInsecure`/skip-verify flag anywhere** (verified). So a poisoned/MITM'd artifact is rejected
offline against the pinned key. **The gap is custody + shipping:** WO-2 (signed/notarized DMG +
updater) is unshipped (`CITRATE_CORE_FINISH_PLAN.md:84`), and **where the minisign private key lives
is not documented in this repo.** If that key is generated on / stored with CI, a CI compromise is
RCE on every install. **Gate (discharge before WO-2 ships):** document the minisign private-key
custody (an offline/HSM/hardware holder, NOT co-located with CI), and confirm the notarization
identity custody. Also rotate `TREASURY_SIGNER_TOKEN` (open owner TODO, `:64,106`).

### CORE-G2 — MEDIUM (pre-ship gate) — keep `SessionBudget` unwired
`CIT-SEV: sev=M; diff=H; asset=crypto; impact=authbypass; reach=privileged; conf=firm`

`SessionBudget` (`ceremony.rs:150-214`) is a defined-but-**dead** auto-approve primitive: its
`.covers()`/`.consume()` have zero non-test call sites, and `request`/`approve`/`approve_and_broadcast`
never consult it — every intent prompts (verified line-by-line). This is correct today. The finding is
a **standing condition**: wiring `SessionBudget` into `approve` would convert the per-tx
human-in-the-ceremony property into a scoped "one approval covers N" budget — the single biggest Rule-3
regression surface. **Gate:** it must not be consulted by any signer path until ADR-2026-08-29 lands
*and* a fresh @rule8 review signs off on the budget semantics. Recommend a CI tripwire asserting
`SessionBudget::{covers,consume}` have no non-test callers.

### CORE-L1 — LOW — confidential OAuth secret release path (credentials)
`CIT-SEV: sev=L; diff=M; asset=crypto; impact=infoleak; reach=local; conf=firm`

`connections.rs` (MCP OAuth for GitHub/GoogleDrive/Notion/HuggingFace) is a confidential client with a
`client_secret`. In-memory it is `Zeroizing<String>`, never `{:?}`-printed, and sealed into the OS
keyring. The secret is loaded from `oauth.dev.json` in the cwd (or `CITRATE_OAUTH_DEV_FILE`).
**Verified safe for release:** there is **no `bundle.resources` key** in `tauri.conf.json`, the file is
**gitignored (`.gitignore:33`) and untracked**, and a release build with no such file reports
`NotConfigured`. So a shipped build carries no confidential secret. **Condition:** the release
keyring-intake WP (how a shipped build receives these secrets) must land — and be reviewed — before any
confidential MCP provider is enabled in a shipped build. Until then, keep confidential providers
dev-only.

### CORE-I1 / CORE-I2 / CORE-I3
Informational — see the table. CORE-I1: the Commissary in-app leg is a fail-closed stub (Rule 1
honest); its future build must reuse the `model.rs` hash-pin+quarantine pattern. CORE-I2: a
documentation-precision fix, not a defect. CORE-I3: low-sensitivity consent tokens.

## 4. Positives — verified SAFE

- **Ceremony-only wallet signing (Rule 3) — verified, physically enforced.** `wallet::sign_message`
  /`sign_transaction`/`sign_personal` are `pub(crate)` in the `kit` crate and called ONLY from
  `ceremony.rs` approve/approve_and_broadcast; `src-tauri` is a separate crate, so the signer is
  unreachable from the command registry — a stronger barrier than the negative-control test
  (`lib.rs:706`). No agent tool, hermes capsule, sidecar, or command signs or reads key material.
  Every fund-moving command (`wallet_send`, `wallet_stake`, withdrawals, `node_register_validator`,
  `user_claim`) builds an unsigned intent → `ceremony.request`. Approval is per-`CeremonyId`, single-use,
  raw-ack-gated, fail-closed on a locked vault.
- **No treasury key in the desktop (D2.4).** `grant_status.rs`/`membership.rs` are pure `eth_call`
  reads + a capability-isolated checkout webview; the treasury signer is the out-of-repo droplet under
  its operator gate. No in-repo treasury finding.
- **Model downloads hash-pinned (WO-7/3a).** `model.rs` quarantines on any sha256/size/magic mismatch;
  `Ready` is earned only by a real verify. `model_catalog.rs` refuses to emit a descriptor without a
  real sha256 and re-fetches a fresh pin so the webview can't forge a hash.
- **Stake-lock intents ceremony-gated (M-3), selectors drift-tested.** `staking.rs` submits every
  value-bearing write as an intent and signs nothing; the keccak-correct withdrawal selectors are
  pinned with drift tests that **run green** (27/27).
- **Credentials keyring-sealed.** Social OAuth is public-client PKCE (no secret); tokens are
  `custody.put`-sealed into the OS keyring and never cross the invoke boundary; the comms/cluster
  identity is a separate scoped key passed as a 0600 file, never the wallet.
- **Honest gating (Rule 1).** Unwired domains (`commissary_catalog`, `membership_entitlement`,
  `comms_connections`, `chat_backend`) return `unavailable:` stubs. **Training write-paths are
  code-gated:** `training_start`/`training_contribute`/`training_claim` return unconditional `Err`
  ("SETTLER-only" / "not yet authorized — pending @rule8/gateSec") — real member SALT cannot move
  until gateSec flips and the claim ceremony wiring lands (verified).
- **Tight CSP.** `connect-src` allows only self + auth/rpc/infer.citrate.ai; `frame-ancestors 'none'`,
  `object-src 'none'`, `base-uri 'self'`.

## 5. Reconciliation

This leg reconciles into the central gateSec / SETL-S4 trail in citrate-security. The settlement leg
(settlement signer custody + the coop money surface) is at
`citrate-settlement/.agentile/audits/2026-08-30-setl-money-surface/`. The combined @rule8 sign-off
needs both legs plus the owner + security-lead signatures and the federation manifest pin.

## 6. Verdict & disposition

- **In-repo code (all five surfaces): CLEAN.** No CRITICAL/HIGH. The Rule-3 ceremony invariant is
  verified and physically enforced; no ungated money write; no key over the invoke boundary; downloads
  hash-pinned; credentials keyring-sealed; training write-paths honestly code-gated.
- **The citrate-core @rule8 leg: PROVISIONAL-PENDING-GATES.** The remaining @rule8 items are pre-ship
  process gates, not code defects, and some features they gate aren't shipped yet.

**Gate conditions to discharge the citrate-core leg of gateSec (none block current development, each
blocks the specific feature shipping):**
1. **CORE-G1 / WO-2** — before the signed/notarized build + updater ships: document the minisign
   private-key custody (offline/HSM, NOT on CI) + the notarization identity custody; rotate
   `TREASURY_SIGNER_TOKEN`.
2. **CORE-G2** — `SessionBudget` stays unwired until ADR-2026-08-29 + a dedicated @rule8 review; add a
   CI tripwire on its callers.
3. **CORE-L1** — the release keyring-intake WP lands + is reviewed before any confidential MCP provider
   is enabled in a shipped build; confidential providers stay dev-only until then.
4. **CORE-I1 / WO-7** — when the Commissary in-app download leg is built, it reuses the `model.rs`
   hash-pin + quarantine discipline (+ the server-side signed-URL/entitlement gate, audited in its own
   repo).
5. Training write-paths stay code-gated (verified) until the combined gateSec flips.

**Disposition:** CORE-I2 is a one-line documentation fix (scope the invariant to wallet/EVM +
fund-moving). CORE-I3 is accepted (low sensitivity). This leg + the settlement leg + owner and
security-lead signatures + the federation manifest pin together constitute the @rule8 / gateSec
sign-off; until it flips, real member SALT stays blocked.
